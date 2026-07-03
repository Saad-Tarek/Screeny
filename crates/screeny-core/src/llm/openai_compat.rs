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

const ANALYZE_TIMEOUT: Duration = Duration::from_secs(120);
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
    content: String,
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
            "max_tokens": 800,
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
            .map(|c| c.message.content.as_str())
            .unwrap_or_default();
        let (ocr_text, description) = prompts::parse_response(content);
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
