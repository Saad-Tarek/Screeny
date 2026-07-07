//! Screeny core: capture scheduling, image storage, and (in later milestones)
//! LLM analysis and delivery sinks. This crate has no Tauri dependencies so it
//! stays unit-testable on any machine.

pub mod capture;
pub mod config;
pub mod error;
pub mod llm;
pub mod maintenance;
pub mod pipeline;
pub mod secrets;
pub mod sinks;
pub mod store;

pub use config::{
    CaptureConfig, ChannelsConfig, Config, ContentMode, EmailConfig, ImageFormat, LlmBackendKind,
    LlmConfig, SmtpSecurity, TelegramConfig,
};
pub use error::{CoreError, Result};
pub use llm::{
    backend_from_config, detect::detect_local_backends, detect::DetectResult,
    ollama::OllamaBackend, ollama::PullProgress, Analysis, LlmBackend,
};
pub use pipeline::{CoreEvent, Engine, EngineOptions, Frame, RunState};
pub use secrets::{
    KeyringStore, MemoryStore, SecretStore, LLM_API_KEY, SMTP_PASSWORD, TELEGRAM_BOT_TOKEN,
};
pub use sinks::{
    email::EmailSink, telegram::DiscoveredChat, telegram::TelegramSink, DeliveryItem, Sink,
    SinkKind,
};
pub use store::{CaptureRow, Store};
