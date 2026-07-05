//! OpenAI-compatible chat-completions backend. Covers LM Studio (no key),
//! OpenAI, OpenRouter, and any custom endpoint (key from the OS keychain).

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::json;

use crate::error::{CoreError, Result};
use crate::llm::{prompts, Analysis, LlmBackend};
use crate::secrets::{SecretStore, LLM_API_KEY};

// Big local models on CPU can take minutes per image.
const ANALYZE_TIMEOUT: Duration = Duration::from_secs(300);
const LIST_TIMEOUT: Duration = Duration::from_secs(5);

pub struct OpenAiCompatBackend {
    base_url: String,
    /// None = no auth needed (LM Studio / local proxies).
    secrets: Option<Arc<dyn SecretStore>>,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    #[serde(default)]
    content: Option<String>,
    /// Reasoning models (served by LM Studio and others) put their chain of
    /// thought here; if they exhaust max_tokens, `content` stays empty.
    #[serde(default)]
    reasoning_content: Option<String>,
}

impl ChoiceMessage {
    fn best_text(&self) -> &str {
        match &self.content {
            Some(content) if !content.trim().is_empty() => content,
            _ => self.reasoning_content.as_deref().unwrap_or(""),
        }
    }
}

impl OpenAiCompatBackend {
    pub fn new(base_url: String, secrets: Option<Arc<dyn SecretStore>>) -> OpenAiCompatBackend {
        OpenAiCompatBackend {
            base_url,
            secrets,
            client: reqwest::Client::new(),
        }
    }

    fn err(message: impl std::fmt::Display) -> CoreError {
        CoreError::Llm(format!("openai-compatible: {message}"))
    }

    async fn api_key(&self) -> Result<Option<String>> {
        let Some(secrets) = self.secrets.clone() else {
            return Ok(None);
        };
        let key = tokio::task::spawn_blocking(move || secrets.get(LLM_API_KEY))
            .await
            .map_err(|e| Self::err(format!("keychain task: {e}")))??;
        Ok(key.filter(|k| !k.trim().is_empty()))
    }

    fn with_auth(
        &self,
        request: reqwest::RequestBuilder,
        key: Option<String>,
    ) -> reqwest::RequestBuilder {
        match key {
            Some(key) => request.bearer_auth(key),
            None => request,
        }
    }
}

#[async_trait]
impl LlmBackend for OpenAiCompatBackend {
    fn id(&self) -> &'static str {
        "openai-compat"
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let key = self.api_key().await?;
        let request = self
            .client
            .get(format!("{}/v1/models", self.base_url))
            .timeout(LIST_TIMEOUT);
        let response = self
            .with_auth(request, key)
            .send()
            .await
            .map_err(Self::err)?
            .error_for_status()
            .map_err(Self::err)?;
        let models: ModelsResponse = response.json().await.map_err(Self::err)?;
        Ok(models.data.into_iter().map(|m| m.id).collect())
    }

    async fn analyze(&self, image: &[u8], model: &str, prompt: &str) -> Result<Analysis> {
        let started = Instant::now();
        let data_uri = format!(
            "data:image/jpeg;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(image)
        );
        let body = json!({
            "model": model,
            "temperature": 0,
            // Generous budget: reasoning models spend tokens thinking before
            // they emit the JSON answer.
            "max_tokens": 2000,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": prompt },
                    { "type": "image_url", "image_url": { "url": data_uri } },
                ],
            }],
        });
        let key = self.api_key().await?;
        let request = self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .timeout(ANALYZE_TIMEOUT)
            .json(&body);
        let response = self
            .with_auth(request, key)
            .send()
            .await
            .map_err(Self::err)?
            .error_for_status()
            .map_err(Self::err)?;
        let completion: ChatCompletionResponse = response.json().await.map_err(Self::err)?;
        let content = completion
            .choices
            .first()
            .map(|c| c.message.best_text())
            .unwrap_or_default();
        let (ocr_text, description) = prompts::parse_response(content);
        if ocr_text.is_empty() && description.is_empty() {
            return Err(Self::err(
                "model returned an empty answer (it may have spent all tokens reasoning, \
                 or it is not a vision model)",
            ));
        }
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
    use crate::secrets::MemoryStore;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn analyze_sends_bearer_token_and_parses_choice() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer sk-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "{\"ocr\": \"TOTAL 42\", \"description\": \"A spreadsheet.\"}"
                    }
                }]
            })))
            .mount(&server)
            .await;

        let secrets = Arc::new(MemoryStore::default());
        secrets.set(LLM_API_KEY, "sk-test").unwrap();
        let backend = OpenAiCompatBackend::new(server.uri(), Some(secrets));
        let analysis = backend.analyze(&[9, 9], "gpt-test", "p").await.unwrap();
        assert_eq!(analysis.ocr_text, "TOTAL 42");
        assert_eq!(analysis.description, "A spreadsheet.");
    }

    #[tokio::test]
    async fn falls_back_to_reasoning_content_when_content_empty() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "",
                        "reasoning_content": "Looking at the image... {\"ocr\": \"btn OK\", \"description\": \"A dialog.\"}"
                    }
                }]
            })))
            .mount(&server)
            .await;

        let backend = OpenAiCompatBackend::new(server.uri(), None);
        let analysis = backend.analyze(&[1], "gemma-vl", "p").await.unwrap();
        assert_eq!(analysis.ocr_text, "btn OK");
        assert_eq!(analysis.description, "A dialog.");
    }

    #[tokio::test]
    async fn empty_answer_is_an_error_not_blank_analysis() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{ "message": { "role": "assistant", "content": "" } }]
            })))
            .mount(&server)
            .await;

        let backend = OpenAiCompatBackend::new(server.uri(), None);
        let err = backend
            .analyze(&[1], "m", "p")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("empty answer"), "got: {err}");
    }

    #[tokio::test]
    async fn list_models_works_without_key() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": "qwen2-vl-7b-instruct"}]
            })))
            .mount(&server)
            .await;

        let backend = OpenAiCompatBackend::new(server.uri(), None);
        let models = backend.list_models().await.unwrap();
        assert_eq!(models, vec!["qwen2-vl-7b-instruct"]);
    }
}
