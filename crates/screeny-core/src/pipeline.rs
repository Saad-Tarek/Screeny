//! The capture engine: a drift-corrected interval loop that captures the
//! screen, persists the image + metadata, fans deliveries out to enabled
//! sinks, and broadcasts events. LLM analysis arrives in M3.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Local, Utc};
use image::RgbaImage;
use serde::Serialize;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};

use crate::capture;
use crate::config::Config;
use crate::error::{CoreError, Result};
use crate::secrets::SecretStore;
use crate::sinks::email::EmailSink;
use crate::sinks::{Sink, SinkKind};
use crate::store::{image_path_for, CaptureRow, NewCapture, Store};

/// A raw captured frame, before encoding.
pub struct Frame {
    pub image: RgbaImage,
    pub monitor: String,
}

/// Screen-grab function. Injected so tests (and headless CI) don't need a
/// real display. Blocking — the engine calls it inside `spawn_blocking`.
pub type CaptureFn = Arc<dyn Fn() -> Result<Frame> + Send + Sync>;

/// Builds the currently enabled sinks from config. Injected for testability.
pub type SinkFactory = Arc<dyn Fn(&Config) -> Vec<Arc<dyn Sink>> + Send + Sync>;

/// Production factory: one sink per enabled channel.
pub fn default_sink_factory(secrets: Arc<dyn SecretStore>) -> SinkFactory {
    Arc::new(move |config: &Config| {
        let mut sinks: Vec<Arc<dyn Sink>> = Vec::new();
        if config.channels.email.enabled {
            sinks.push(Arc::new(EmailSink::new(
                config.channels.email.clone(),
                secrets.clone(),
            )));
        }
        sinks
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    Running,
    Paused,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum CoreEvent {
    CaptureTaken(CaptureRow),
    CaptureFailed { message: String },
    StateChanged { state: RunState },
    ConfigChanged(Config),
    DeliverySucceeded { sink: SinkKind, count: usize },
    DeliveryFailed { sink: SinkKind, message: String },
}

pub struct EngineOptions {
    pub config: Config,
    pub config_path: PathBuf,
    pub data_dir: PathBuf,
    pub store: Arc<Store>,
    pub capture_fn: CaptureFn,
    pub secrets: Arc<dyn SecretStore>,
    /// Defaults to `default_sink_factory(secrets)` when None.
    pub sink_factory: Option<SinkFactory>,
}

pub struct Engine {
    state_tx: watch::Sender<RunState>,
    config_tx: watch::Sender<Config>,
    config_path: PathBuf,
    events: broadcast::Sender<CoreEvent>,
    capture_now_tx: mpsc::Sender<()>,
    store: Arc<Store>,
    secrets: Arc<dyn SecretStore>,
}

impl Engine {
    /// Spawns the background capture + delivery loops and returns a handle.
    /// Must be called from within a tokio runtime.
    pub fn start(options: EngineOptions) -> Arc<Engine> {
        let EngineOptions {
            config,
            config_path,
            data_dir,
            store,
            capture_fn,
            secrets,
            sink_factory,
        } = options;

        let config = config.sanitized();
        let sink_factory = sink_factory.unwrap_or_else(|| default_sink_factory(secrets.clone()));
        let initial_state = if config.capture.start_on_launch {
            RunState::Running
        } else {
            RunState::Paused
        };
        let (state_tx, state_rx) = watch::channel(initial_state);
        let (config_tx, config_rx) = watch::channel(config);
        let (events, _) = broadcast::channel(64);
        let (capture_now_tx, capture_now_rx) = mpsc::channel(4);
        let (delivery_tx, delivery_rx) = mpsc::channel::<CaptureRow>(32);

        let engine = Arc::new(Engine {
            state_tx,
            config_tx,
            config_path,
            events: events.clone(),
            capture_now_tx,
            store: store.clone(),
            secrets,
        });

        tokio::spawn(run_loop(
            state_rx,
            config_rx.clone(),
            capture_now_rx,
            store.clone(),
            data_dir,
            capture_fn,
            events.clone(),
            delivery_tx,
        ));
        tokio::spawn(delivery_loop(
            delivery_rx,
            config_rx.clone(),
            sink_factory,
            store.clone(),
            events.clone(),
        ));
        tokio::spawn(prune_loop(config_rx, store));

        engine
    }

    pub fn state(&self) -> RunState {
        *self.state_tx.borrow()
    }

    pub fn config(&self) -> Config {
        self.config_tx.borrow().clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CoreEvent> {
        self.events.subscribe()
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    pub fn secrets(&self) -> &Arc<dyn SecretStore> {
        &self.secrets
    }

    pub fn set_state(&self, state: RunState) {
        if *self.state_tx.borrow() != state {
            let _ = self.state_tx.send(state);
            let _ = self.events.send(CoreEvent::StateChanged { state });
            info!(?state, "capture state changed");
        }
    }

    pub fn toggle(&self) -> RunState {
        let next = match self.state() {
            RunState::Running => RunState::Paused,
            RunState::Paused => RunState::Running,
        };
        self.set_state(next);
        next
    }

    /// Persist and apply a new configuration.
    pub fn set_config(&self, config: Config) -> Result<Config> {
        let config = config.sanitized();
        config.save(&self.config_path)?;
        let _ = self.config_tx.send(config.clone());
        let _ = self.events.send(CoreEvent::ConfigChanged(config.clone()));
        info!("config updated");
        Ok(config)
    }

    /// Request an immediate out-of-schedule capture.
    pub async fn capture_now(&self) -> Result<()> {
        self.capture_now_tx
            .send(())
            .await
            .map_err(|_| CoreError::Capture("capture loop is not running".into()))
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_loop(
    state_rx: watch::Receiver<RunState>,
    mut config_rx: watch::Receiver<Config>,
    mut capture_now_rx: mpsc::Receiver<()>,
    store: Arc<Store>,
    data_dir: PathBuf,
    capture_fn: CaptureFn,
    events: broadcast::Sender<CoreEvent>,
    delivery_tx: mpsc::Sender<CaptureRow>,
) {
    let mut interval_secs = config_rx.borrow().capture.interval_seconds;
    let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // Consume the immediate first tick so the first capture happens one full
    // interval after launch, not instantly at startup.
    ticker.tick().await;

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if *state_rx.borrow() == RunState::Running {
                    do_capture(&store, &data_dir, &capture_fn, &config_rx, &events, &delivery_tx).await;
                }
            }
            changed = config_rx.changed() => {
                if changed.is_err() {
                    break; // engine dropped
                }
                let new_secs = config_rx.borrow().capture.interval_seconds;
                if new_secs != interval_secs {
                    interval_secs = new_secs;
                    ticker = tokio::time::interval(Duration::from_secs(interval_secs));
                    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
                    ticker.tick().await;
                }
            }
            request = capture_now_rx.recv() => {
                if request.is_none() {
                    break; // engine dropped
                }
                do_capture(&store, &data_dir, &capture_fn, &config_rx, &events, &delivery_tx).await;
            }
        }
    }
}

async fn do_capture(
    store: &Arc<Store>,
    data_dir: &Path,
    capture_fn: &CaptureFn,
    config_rx: &watch::Receiver<Config>,
    events: &broadcast::Sender<CoreEvent>,
    delivery_tx: &mpsc::Sender<CaptureRow>,
) {
    let capture_config = config_rx.borrow().capture.clone();
    let capture_fn = capture_fn.clone();

    let encoded = tokio::task::spawn_blocking(move || {
        let frame = capture_fn()?;
        let bytes = capture::encode(
            &frame.image,
            capture_config.format,
            capture_config.jpeg_quality,
        )?;
        let (width, height) = frame.image.dimensions();
        Ok::<_, CoreError>((bytes, frame.monitor, width, height, capture_config.format))
    })
    .await;

    let result = match encoded {
        Ok(Ok((bytes, monitor, width, height, format))) => persist(
            store,
            data_dir,
            bytes,
            monitor,
            width,
            height,
            format.extension(),
        ),
        Ok(Err(e)) => Err(e),
        Err(join_err) => Err(CoreError::Capture(format!(
            "capture task panicked: {join_err}"
        ))),
    };

    match result {
        Ok(row) => {
            let _ = events.send(CoreEvent::CaptureTaken(row.clone()));
            // try_send: a saturated delivery queue must never stall capture.
            if let Err(e) = delivery_tx.try_send(row) {
                warn!(error = %e, "delivery queue full; capture stays local-only");
            }
        }
        Err(e) => {
            warn!(error = %e, "capture failed");
            let _ = events.send(CoreEvent::CaptureFailed {
                message: e.to_string(),
            });
        }
    }
}

fn persist(
    store: &Arc<Store>,
    data_dir: &Path,
    bytes: Vec<u8>,
    monitor: String,
    width: u32,
    height: u32,
    ext: &str,
) -> Result<CaptureRow> {
    let now_local = Local::now();
    let path = image_path_for(data_dir, now_local, ext)?;
    std::fs::write(&path, &bytes)?;
    store.insert_capture(&NewCapture {
        taken_at: Utc::now(),
        path: path.to_string_lossy().into_owned(),
        monitor,
        width,
        height,
    })
}

/// Fan-out worker: accumulates captures per sink and delivers when each
/// sink's batch size is reached. A failing sink never blocks the others.
async fn delivery_loop(
    mut delivery_rx: mpsc::Receiver<CaptureRow>,
    config_rx: watch::Receiver<Config>,
    sink_factory: SinkFactory,
    store: Arc<Store>,
    events: broadcast::Sender<CoreEvent>,
) {
    let mut queues: HashMap<SinkKind, Vec<CaptureRow>> = HashMap::new();

    while let Some(row) = delivery_rx.recv().await {
        let sinks = sink_factory(&config_rx.borrow().clone());
        for sink in sinks {
            let queue = queues.entry(sink.kind()).or_default();
            queue.push(row.clone());
            if queue.len() >= sink.batch_size() {
                let batch = std::mem::take(queue);
                deliver_with_retry(sink, batch, &store, &events).await;
            }
        }
    }
}

const DELIVERY_ATTEMPTS: u32 = 2;
const DELIVERY_RETRY_DELAY: Duration = Duration::from_secs(2);

async fn deliver_with_retry(
    sink: Arc<dyn Sink>,
    batch: Vec<CaptureRow>,
    store: &Arc<Store>,
    events: &broadcast::Sender<CoreEvent>,
) {
    let ids: Vec<i64> = batch.iter().map(|r| r.id).collect();
    let mut last_error = String::new();

    for attempt in 1..=DELIVERY_ATTEMPTS {
        match sink.deliver(&batch).await {
            Ok(()) => {
                if let Err(e) = store.record_delivery(&ids, sink.kind().as_str(), "sent", None) {
                    warn!(error = %e, "failed to record delivery");
                }
                let _ = events.send(CoreEvent::DeliverySucceeded {
                    sink: sink.kind(),
                    count: batch.len(),
                });
                return;
            }
            Err(e) => {
                last_error = e.to_string();
                warn!(sink = sink.kind().as_str(), attempt, error = %last_error, "delivery attempt failed");
                if attempt < DELIVERY_ATTEMPTS {
                    tokio::time::sleep(DELIVERY_RETRY_DELAY).await;
                }
            }
        }
    }

    // Give up on this batch, keep capturing (same policy as the legacy script).
    if let Err(e) = store.record_delivery(&ids, sink.kind().as_str(), "failed", Some(&last_error)) {
        warn!(error = %e, "failed to record delivery failure");
    }
    let _ = events.send(CoreEvent::DeliveryFailed {
        sink: sink.kind(),
        message: last_error,
    });
}

/// Hourly retention pass: drop DB rows past the cutoff and their files.
async fn prune_loop(config_rx: watch::Receiver<Config>, store: Arc<Store>) {
    let mut ticker = tokio::time::interval(Duration::from_secs(60 * 60));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        let retention_days = config_rx.borrow().capture.retention_days;
        let Some(days) = retention_days else { continue };
        let cutoff = Utc::now() - ChronoDuration::days(i64::from(days));
        match store.prune_older_than(cutoff) {
            Ok(paths) => {
                let count = paths.len();
                for path in paths {
                    if let Err(e) = std::fs::remove_file(&path) {
                        if e.kind() != std::io::ErrorKind::NotFound {
                            warn!(path, error = %e, "could not delete pruned capture file");
                        }
                    }
                }
                if count > 0 {
                    info!(count, days, "pruned old captures");
                }
            }
            Err(e) => warn!(error = %e, "retention pruning failed"),
        }
    }
}

const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Engine>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CaptureConfig, ChannelsConfig, EmailConfig};
    use crate::secrets::MemoryStore;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    fn fake_capture_fn() -> CaptureFn {
        Arc::new(|| {
            Ok(Frame {
                image: RgbaImage::from_pixel(8, 6, image::Rgba([10, 200, 30, 255])),
                monitor: "FakeDisplay".into(),
            })
        })
    }

    fn test_config(interval_seconds: u64, start_on_launch: bool) -> Config {
        Config {
            capture: CaptureConfig {
                interval_seconds,
                start_on_launch,
                ..CaptureConfig::default()
            },
            channels: ChannelsConfig {
                email: EmailConfig {
                    enabled: true,
                    ..EmailConfig::default()
                },
            },
            ..Config::default()
        }
    }

    /// Records batches; optionally fails the first N deliver calls.
    struct FakeSink {
        kind: SinkKind,
        batch_size: usize,
        fail_first: usize,
        calls: AtomicUsize,
        batches: Mutex<Vec<Vec<i64>>>,
    }

    impl FakeSink {
        fn new(batch_size: usize, fail_first: usize) -> Arc<FakeSink> {
            Arc::new(FakeSink {
                kind: SinkKind::Email,
                batch_size,
                fail_first,
                calls: AtomicUsize::new(0),
                batches: Mutex::new(Vec::new()),
            })
        }

        fn delivered(&self) -> Vec<Vec<i64>> {
            self.batches.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Sink for FakeSink {
        fn kind(&self) -> SinkKind {
            self.kind
        }
        fn batch_size(&self) -> usize {
            self.batch_size
        }
        async fn deliver(&self, batch: &[CaptureRow]) -> Result<()> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if call < self.fail_first {
                return Err(CoreError::Delivery {
                    sink: "email".into(),
                    message: "simulated outage".into(),
                });
            }
            self.batches
                .lock()
                .unwrap()
                .push(batch.iter().map(|r| r.id).collect());
            Ok(())
        }
        async fn test(&self) -> Result<()> {
            Ok(())
        }
    }

    struct TestEngine {
        engine: Arc<Engine>,
        events: broadcast::Receiver<CoreEvent>,
        sink: Arc<FakeSink>,
    }

    async fn start_with_sink(config: Config, dir: &Path, sink: Arc<FakeSink>) -> TestEngine {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let sink_for_factory = sink.clone();
        let engine = Engine::start(EngineOptions {
            config,
            config_path: dir.join("config.json"),
            data_dir: dir.to_path_buf(),
            store,
            capture_fn: fake_capture_fn(),
            secrets: Arc::new(MemoryStore::default()),
            sink_factory: Some(Arc::new(move |_| {
                vec![sink_for_factory.clone() as Arc<dyn Sink>]
            })),
        });
        let events = engine.subscribe();
        TestEngine {
            engine,
            events,
            sink,
        }
    }

    async fn wait_for(
        rx: &mut broadcast::Receiver<CoreEvent>,
        mut predicate: impl FnMut(&CoreEvent) -> bool,
    ) -> CoreEvent {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                .await
                .expect("timed out waiting for event")
                .expect("event channel closed");
            if predicate(&event) {
                return event;
            }
        }
    }

    #[tokio::test]
    async fn capture_now_persists_row_image_and_emits_event() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = start_with_sink(test_config(3600, true), dir.path(), FakeSink::new(1, 0)).await;

        t.engine.capture_now().await.unwrap();
        let event = wait_for(&mut t.events, |e| matches!(e, CoreEvent::CaptureTaken(_))).await;

        match event {
            CoreEvent::CaptureTaken(row) => {
                assert_eq!(row.monitor, "FakeDisplay");
                assert_eq!((row.width, row.height), (8, 6));
                assert!(Path::new(&row.path).exists());
            }
            other => panic!("expected CaptureTaken, got {other:?}"),
        }
        assert_eq!(t.engine.store().capture_count().unwrap(), 1);
    }

    #[tokio::test]
    async fn delivery_batches_to_sink_batch_size() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = start_with_sink(test_config(3600, true), dir.path(), FakeSink::new(3, 0)).await;

        for _ in 0..3 {
            t.engine.capture_now().await.unwrap();
        }
        wait_for(&mut t.events, |e| {
            matches!(e, CoreEvent::DeliverySucceeded { count: 3, .. })
        })
        .await;

        let batches = t.sink.delivered();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 3);
    }

    #[tokio::test]
    async fn delivery_retries_then_succeeds() {
        tokio::time::pause();
        let dir = tempfile::tempdir().unwrap();
        let mut t = start_with_sink(
            test_config(3600, true),
            dir.path(),
            FakeSink::new(1, 1), // first attempt fails, retry succeeds
        )
        .await;

        t.engine.capture_now().await.unwrap();
        wait_for(&mut t.events, |e| {
            matches!(e, CoreEvent::DeliverySucceeded { .. })
        })
        .await;
        assert_eq!(t.sink.delivered().len(), 1);
    }

    #[tokio::test]
    async fn delivery_gives_up_after_retries_and_records_failure() {
        tokio::time::pause();
        let dir = tempfile::tempdir().unwrap();
        let mut t = start_with_sink(
            test_config(3600, true),
            dir.path(),
            FakeSink::new(1, 99), // always fails
        )
        .await;

        t.engine.capture_now().await.unwrap();
        let event = wait_for(&mut t.events, |e| {
            matches!(e, CoreEvent::DeliveryFailed { .. })
        })
        .await;
        match event {
            CoreEvent::DeliveryFailed { message, .. } => {
                assert!(message.contains("simulated outage"))
            }
            _ => unreachable!(),
        }
        // Capturing continues after a failed batch.
        t.engine.capture_now().await.unwrap();
        wait_for(&mut t.events, |e| matches!(e, CoreEvent::CaptureTaken(_))).await;
    }

    #[tokio::test]
    async fn paused_engine_does_not_capture_on_schedule() {
        let dir = tempfile::tempdir().unwrap();
        let t = start_with_sink(test_config(5, false), dir.path(), FakeSink::new(1, 0)).await;
        assert_eq!(t.engine.state(), RunState::Paused);

        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(t.engine.store().capture_count().unwrap(), 0);
    }

    #[tokio::test]
    async fn toggle_flips_state_and_emits_events() {
        let dir = tempfile::tempdir().unwrap();
        let mut t = start_with_sink(test_config(3600, true), dir.path(), FakeSink::new(1, 0)).await;

        assert_eq!(t.engine.toggle(), RunState::Paused);
        assert_eq!(t.engine.toggle(), RunState::Running);

        let first = t.events.recv().await.unwrap();
        assert!(matches!(
            first,
            CoreEvent::StateChanged {
                state: RunState::Paused
            }
        ));
    }

    #[tokio::test]
    async fn set_config_saves_file_and_applies() {
        let dir = tempfile::tempdir().unwrap();
        let t = start_with_sink(test_config(3600, true), dir.path(), FakeSink::new(1, 0)).await;

        let updated = t.engine.set_config(test_config(60, true)).unwrap();
        assert_eq!(updated.capture.interval_seconds, 60);
        assert_eq!(t.engine.config().capture.interval_seconds, 60);
        assert!(dir.path().join("config.json").exists());
    }

    #[tokio::test]
    async fn capture_failure_emits_failed_event() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open_in_memory().unwrap());
        let failing: CaptureFn = Arc::new(|| Err(CoreError::Capture("no permission".into())));
        let engine = Engine::start(EngineOptions {
            config: test_config(3600, true),
            config_path: dir.path().join("config.json"),
            data_dir: dir.path().to_path_buf(),
            store,
            capture_fn: failing,
            secrets: Arc::new(MemoryStore::default()),
            sink_factory: None,
        });
        let mut rx = engine.subscribe();

        engine.capture_now().await.unwrap();
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(event, CoreEvent::CaptureFailed { .. }));
        assert_eq!(engine.store().capture_count().unwrap(), 0);
    }
}
