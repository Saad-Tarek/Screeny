//! Vision-LLM backends that OCR and describe captured screenshots.
//! Privacy-first: local backends (Ollama, LM Studio) are the default; any
//! OpenAI-compatible endpoint is supported for users who opt in.

pub mod detect;
pub mod ollama;
pub mod openai_compat;
pub mod prompts;

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::config::{LlmBackendKind, LlmConfig};
use crate::error::Result;
use crate::secrets::SecretStore;

/// Result of analyzing one screenshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Analysis {
    pub model: String,
    pub ocr_text: String,
    pub description: String,
    pub latency_ms: u64,
}

#[async_trait]
pub trait LlmBackend: Send + Sync {
    fn id(&self) -> &'static str;

    /// Reachability check; returns the models the backend has available.
    async fn list_models(&self) -> Result<Vec<String>>;

    /// Analyze a JPEG/PNG image and return OCR text + description.
    async fn analyze(&self, image: &[u8], model: &str, prompt: &str) -> Result<Analysis>;
}

/// Build the configured backend. LM Studio speaks the OpenAI API.
pub fn backend_from_config(
    config: &LlmConfig,
    secrets: Arc<dyn SecretStore>,
) -> Arc<dyn LlmBackend> {
    match config.backend {
        LlmBackendKind::Ollama => Arc::new(ollama::OllamaBackend::new(config.base_url.clone())),
        LlmBackendKind::Lmstudio => Arc::new(openai_compat::OpenAiCompatBackend::new(
            config.base_url.clone(),
            None,
        )),
        LlmBackendKind::Custom => Arc::new(openai_compat::OpenAiCompatBackend::new(
            config.base_url.clone(),
            Some(secrets),
        )),
    }
}
