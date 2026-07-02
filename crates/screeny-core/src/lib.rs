//! Screeny core: capture scheduling, image storage, and (in later milestones)
//! LLM analysis and delivery sinks. This crate has no Tauri dependencies so it
//! stays unit-testable on any machine.

pub mod capture;
pub mod config;
pub mod error;
pub mod pipeline;
pub mod store;

pub use config::{CaptureConfig, Config, ImageFormat};
pub use error::{CoreError, Result};
pub use pipeline::{CoreEvent, Engine, Frame, RunState};
pub use store::{CaptureRow, Store};
