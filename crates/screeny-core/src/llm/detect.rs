//! Local backend auto-detection: probe the default Ollama and LM Studio
//! ports in parallel with a short timeout.

use std::time::Duration;

use serde::Serialize;

use crate::llm::ollama::OllamaBackend;
use crate::llm::openai_compat::OpenAiCompatBackend;
use crate::llm::LlmBackend;

const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Default, Serialize)]
pub struct DetectResult {
    /// Installed model names when the backend is reachable, else None.
    pub ollama: Option<Vec<String>>,
    pub lmstudio: Option<Vec<String>>,
}

pub async fn detect_local_backends(
    ollama_url: Option<&str>,
    lmstudio_url: Option<&str>,
) -> DetectResult {
    let ollama_url = ollama_url.unwrap_or("http://localhost:11434").to_string();
    let lmstudio_url = lmstudio_url.unwrap_or("http://localhost:1234").to_string();

    let ollama_probe = async {
        let backend = OllamaBackend::new(ollama_url);
        tokio::time::timeout(PROBE_TIMEOUT, backend.list_models())
            .await
            .ok()
            .and_then(|r| r.ok())
    };
    let lmstudio_probe = async {
        let backend = OpenAiCompatBackend::new(lmstudio_url, None);
        tokio::time::timeout(PROBE_TIMEOUT, backend.list_models())
            .await
            .ok()
            .and_then(|r| r.ok())
    };

    let (ollama, lmstudio) = tokio::join!(ollama_probe, lmstudio_probe);
    DetectResult { ollama, lmstudio }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unreachable_ports_detect_as_absent() {
        // Port 1 is never an LLM server.
        let result =
            detect_local_backends(Some("http://127.0.0.1:1"), Some("http://127.0.0.1:1")).await;
        assert!(result.ollama.is_none());
        assert!(result.lmstudio.is_none());
    }
}
