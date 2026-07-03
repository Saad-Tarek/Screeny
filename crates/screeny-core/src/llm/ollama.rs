//! Ollama's native API. Used instead of its OpenAI shim because the native
//! API also exposes model listing and streaming pulls, which drive the
//! onboarding wizard's download progress bar.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::Engine as _;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;

use crate::error::{CoreError, Result};
use crate::llm::{prompts, Analysis, LlmBackend};

const ANALYZE_TIMEOUT: Duration = Duration::from_secs(120);
const LIST_TIMEOUT: Duration = Duration::from_secs(5);

pub struct OllamaBackend {
    base_url: String,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct TagsResponse {
    models: Vec<TagModel>,
}

#[derive(Deserialize)]
struct TagModel {
    name: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

/// One line of Ollama's streaming /api/pull response.
#[derive(Debug, Clone, Deserialize)]
pub struct PullProgress {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(default)]
    pub completed: Option<u64>,
}

impl OllamaBackend {
    pub fn new(base_url: String) -> OllamaBackend {
        OllamaBackend {
            base_url,
            client: reqwest::Client::new(),
        }
    }

    fn err(message: impl std::fmt::Display) -> CoreError {
        CoreError::Llm(format!("ollama: {message}"))
    }

    /// Stream a model download, invoking `on_progress` per status line.
    pub async fn pull_model(
        &self,
        model: &str,
        mut on_progress: impl FnMut(PullProgress) + Send,
    ) -> Result<()> {
        let response = self
            .client
            .post(format!("{}/api/pull", self.base_url))
            .json(&json!({ "model": model, "stream": true }))
            .send()
            .await
            .map_err(Self::err)?
            .error_for_status()
            .map_err(Self::err)?;

        let mut stream = response.bytes_stream();
        let mut buffer = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(Self::err)?;
            buffer.extend_from_slice(&chunk);
            // NDJSON: parse complete lines, keep the remainder buffered.
            while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buffer.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&line);
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(error) = serde_json::from_str::<serde_json::Value>(line) {
                    if let Some(message) = error.get("error").and_then(|e| e.as_str()) {
                        return Err(Self::err(message));
                    }
                }
                if let Ok(progress) = serde_json::from_str::<PullProgress>(line) {
                    on_progress(progress);
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl LlmBackend for OllamaBackend {
    fn id(&self) -> &'static str {
        "ollama"
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let response = self
            .client
            .get(format!("{}/api/tags", self.base_url))
            .timeout(LIST_TIMEOUT)
            .send()
            .await
            .map_err(Self::err)?
            .error_for_status()
            .map_err(Self::err)?;
        let tags: TagsResponse = response.json().await.map_err(Self::err)?;
        Ok(tags.models.into_iter().map(|m| m.name).collect())
    }

    async fn analyze(&self, image: &[u8], model: &str, prompt: &str) -> Result<Analysis> {
        let started = Instant::now();
        let body = json!({
            "model": model,
            "stream": false,
            "options": { "temperature": 0 },
            "messages": [{
                "role": "user",
                "content": prompt,
                "images": [base64::engine::general_purpose::STANDARD.encode(image)],
            }],
        });
        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .timeout(ANALYZE_TIMEOUT)
            .json(&body)
            .send()
            .await
            .map_err(Self::err)?
            .error_for_status()
            .map_err(Self::err)?;
        let chat: ChatResponse = response.json().await.map_err(Self::err)?;
        let (ocr_text, description) = prompts::parse_response(&chat.message.content);
        Ok(Analysis {
            model: model.to_string(),
            ocr_text,
            description,
            latency_ms: started.elapsed().as_millis() as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn list_models_parses_tags() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "models": [{"name": "moondream:latest"}, {"name": "qwen2.5vl:7b"}]
            })))
            .mount(&server)
            .await;

        let backend = OllamaBackend::new(server.uri());
        let models = backend.list_models().await.unwrap();
        assert_eq!(models, vec!["moondream:latest", "qwen2.5vl:7b"]);
    }

    #[tokio::test]
    async fn analyze_parses_json_content() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "message": {
                    "role": "assistant",
                    "content": "{\"ocr\": \"fn main()\", \"description\": \"Rust code in an editor.\"}"
                }
            })))
            .mount(&server)
            .await;

        let backend = OllamaBackend::new(server.uri());
        let analysis = backend
            .analyze(&[1, 2, 3], "moondream", prompts::DEFAULT_PROMPT)
            .await
            .unwrap();
        assert_eq!(analysis.ocr_text, "fn main()");
        assert_eq!(analysis.description, "Rust code in an editor.");
        assert_eq!(analysis.model, "moondream");
    }

    #[tokio::test]
    async fn analyze_surfaces_http_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let backend = OllamaBackend::new(server.uri());
        let err = backend
            .analyze(&[1], "moondream", "p")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("ollama"), "got: {err}");
    }
}
