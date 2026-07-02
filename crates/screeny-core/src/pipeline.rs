//! The capture engine: a drift-corrected interval loop that captures the
//! screen, persists the image + metadata, and broadcasts events. Later
//! milestones extend this with LLM analysis and delivery sinks.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{Local, Utc};
use image::RgbaImage;
use serde::Serialize;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};

use crate::capture;
use crate::config::Config;
use crate::error::{CoreError, Result};
use crate::store::{image_path_for, CaptureRow, NewCapture, Store};

/// A raw captured frame, before encoding.
pub struct Frame {
    pub image: RgbaImage,
    pub monitor: String,
}

/// Screen-grab function. Injected so tests (and headless CI) don't need a
/// real display. Blocking — the engine calls it inside `spawn_blocking`.
pub type CaptureFn = Arc<dyn Fn() -> Result<Frame> + Send + Sync>;

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
}

pub struct Engine {
    state_tx: watch::Sender<RunState>,
    config_tx: watch::Sender<Config>,
    config_path: PathBuf,
    events: broadcast::Sender<CoreEvent>,
    capture_now_tx: mpsc::Sender<()>,
    store: Arc<Store>,
}

impl Engine {
    /// Spawns the background capture loop and returns a handle for control.
    /// Must be called from within a tokio runtime.
    pub fn start(
        config: Config,
        config_path: PathBuf,
        data_dir: PathBuf,
        store: Arc<Store>,
        capture_fn: CaptureFn,
    ) -> Arc<Engine> {
        let config = config.sanitized();
        let initial_state = if config.capture.start_on_launch {
            RunState::Running
        } else {
            RunState::Paused
        };
        let (state_tx, state_rx) = watch::channel(initial_state);
        let (config_tx, config_rx) = watch::channel(config);
        let (events, _) = broadcast::channel(64);
        let (capture_now_tx, capture_now_rx) = mpsc::channel(4);

        let engine = Arc::new(Engine {
            state_tx,
            config_tx,
            config_path,
            events: events.clone(),
            capture_now_tx,
            store: store.clone(),
        });

        tokio::spawn(run_loop(
            state_rx,
            config_rx,
            capture_now_rx,
            store,
            data_dir,
            capture_fn,
            events,
        ));

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

async fn run_loop(
    state_rx: watch::Receiver<RunState>,
    mut config_rx: watch::Receiver<Config>,
    mut capture_now_rx: mpsc::Receiver<()>,
    store: Arc<Store>,
    data_dir: PathBuf,
    capture_fn: CaptureFn,
    events: broadcast::Sender<CoreEvent>,
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
                    do_capture(&store, &data_dir, &capture_fn, &config_rx, &events).await;
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
                do_capture(&store, &data_dir, &capture_fn, &config_rx, &events).await;
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
            let _ = events.send(CoreEvent::CaptureTaken(row));
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

const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Engine>();
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CaptureConfig;

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
            ..Config::default()
        }
    }

    async fn start_test_engine(
        config: Config,
        dir: &std::path::Path,
    ) -> (Arc<Engine>, broadcast::Receiver<CoreEvent>) {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let engine = Engine::start(
            config,
            dir.join("config.json"),
            dir.to_path_buf(),
            store,
            fake_capture_fn(),
        );
        let rx = engine.subscribe();
        (engine, rx)
    }

    #[tokio::test]
    async fn capture_now_persists_row_image_and_emits_event() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, mut rx) = start_test_engine(test_config(3600, true), dir.path()).await;

        engine.capture_now().await.unwrap();
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for capture event")
            .unwrap();

        match event {
            CoreEvent::CaptureTaken(row) => {
                assert_eq!(row.monitor, "FakeDisplay");
                assert_eq!((row.width, row.height), (8, 6));
                assert!(std::path::Path::new(&row.path).exists());
            }
            other => panic!("expected CaptureTaken, got {other:?}"),
        }
        assert_eq!(engine.store().capture_count().unwrap(), 1);
    }

    #[tokio::test]
    async fn paused_engine_does_not_capture_on_schedule() {
        let dir = tempfile::tempdir().unwrap();
        // interval floor is 5s; start paused so ticks are ignored
        let (engine, _rx) = start_test_engine(test_config(5, false), dir.path()).await;
        assert_eq!(engine.state(), RunState::Paused);

        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(engine.store().capture_count().unwrap(), 0);
    }

    #[tokio::test]
    async fn toggle_flips_state_and_emits_events() {
        let dir = tempfile::tempdir().unwrap();
        let (engine, mut rx) = start_test_engine(test_config(3600, true), dir.path()).await;

        assert_eq!(engine.toggle(), RunState::Paused);
        assert_eq!(engine.toggle(), RunState::Running);

        let first = rx.recv().await.unwrap();
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
        let (engine, _rx) = start_test_engine(test_config(3600, true), dir.path()).await;

        let updated = engine.set_config(test_config(60, true)).unwrap();
        assert_eq!(updated.capture.interval_seconds, 60);
        assert_eq!(engine.config().capture.interval_seconds, 60);
        assert!(dir.path().join("config.json").exists());
    }

    #[tokio::test]
    async fn capture_failure_emits_failed_event() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open_in_memory().unwrap());
        let failing: CaptureFn = Arc::new(|| Err(CoreError::Capture("no permission".into())));
        let engine = Engine::start(
            test_config(3600, true),
            dir.path().join("config.json"),
            dir.path().to_path_buf(),
            store,
            failing,
        );
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
