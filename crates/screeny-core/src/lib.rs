//! Screeny core: capture scheduling, image storage, and (in later milestones)
//! LLM analysis and delivery sinks. This crate has no Tauri dependencies so it
//! stays unit-testable on any machine.

pub mod capture;
pub mod config;
pub mod error;
pub mod pipeline;
pub mod secrets;
pub mod sinks;
pub mod store;

pub use config::{CaptureConfig, ChannelsConfig, Config, EmailConfig, ImageFormat, SmtpSecurity};
pub use error::{CoreError, Result};
pub use pipeline::{CoreEvent, Engine, EngineOptions, Frame, RunState};
pub use secrets::{KeyringStore, MemoryStore, SecretStore, SMTP_PASSWORD};
pub use sinks::{email::EmailSink, Sink, SinkKind};
pub use store::{CaptureRow, Store};
