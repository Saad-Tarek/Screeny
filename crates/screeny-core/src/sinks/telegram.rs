//! Telegram Bot API sink. Free official API: just a bot token (from
//! @BotFather) and a chat id. Photos are sent per capture with the AI
//! description as caption; oversized images fall back to documents.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use crate::config::{ContentMode, TelegramConfig};
use crate::error::{CoreError, Result};
use crate::secrets::{SecretStore, TELEGRAM_BOT_TOKEN};
use crate::sinks::{DeliveryItem, Sink, SinkKind};

/// Telegram rejects photos above ~10MB; larger files go as documents.
const PHOTO_LIMIT_BYTES: usize = 9_500_000;
const CAPTION_LIMIT: usize = 1024;
const MESSAGE_LIMIT: usize = 4096;

pub struct TelegramSink {
    config: TelegramConfig,
    secrets: Arc<dyn SecretStore>,
    client: reqwest::Client,
    /// Base URL override for tests; production uses api.telegram.org.
    base_url: String,
}

#[derive(Deserialize)]
struct ApiResponse<T> {
    ok: bool,
    #[serde(default)]
    description: Option<String>,
    result: Option<T>,
}

#[derive(Deserialize)]
struct Update {
    message: Option<UpdateMessage>,
}

#[derive(Deserialize)]
struct UpdateMessage {
    chat: Chat,
}

#[derive(Deserialize)]
struct Chat {
    id: i64,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default, rename = "first_name")]
    first_name: Option<String>,
}

/// A chat the bot has recently seen — used by the "detect chat ID" helper.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DiscoveredChat {
    pub id: i64,
    pub label: String,
}

impl TelegramSink {
    pub fn new(config: TelegramConfig, secrets: Arc<dyn SecretStore>) -> TelegramSink {
        Self::with_base_url(config, secrets, "https://api.telegram.org".into())
    }

    pub fn with_base_url(
        config: TelegramConfig,
        secrets: Arc<dyn SecretStore>,
        base_url: String,
    ) -> TelegramSink {
        TelegramSink {
            config,
            secrets,
            client: reqwest::Client::new(),
            base_url,
        }
    }

    fn err(message: impl std::fmt::Display) -> CoreError {
        CoreError::Delivery {
            sink: "telegram".into(),
            message: message.to_string(),
        }
    }

    async fn token(&self) -> Result<String> {
        let secrets = self.secrets.clone();
        let token = tokio::task::spawn_blocking(move || secrets.get(TELEGRAM_BOT_TOKEN))
            .await
            .map_err(|e| Self::err(format!("keychain task: {e}")))??;
        token
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .ok_or_else(|| Self::err("no bot token saved — set one in Settings"))
    }

    fn validate(&self) -> Result<()> {
        if self.config.chat_id.is_empty() {
            return Err(Self::err(
                "chat ID is empty — use \"Detect chat ID\" in Settings",
            ));
        }
        Ok(())
    }

    async fn check<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> Result<T> {
        let status = response.status();
        let body: ApiResponse<T> = response
            .json()
            .await
            .map_err(|e| Self::err(format!("bad response ({status}): {e}")))?;
        if !body.ok {
            return Err(Self::err(
                body.description
                    .unwrap_or_else(|| format!("telegram API error ({status})")),
            ));
        }
        body.result
            .ok_or_else(|| Self::err("telegram API returned no result"))
    }

    async fn send_text(&self, token: &str, text: &str) -> Result<()> {
        let text: String = text.chars().take(MESSAGE_LIMIT).collect();
        let response = self
            .client
            .post(format!("{}/bot{token}/sendMessage", self.base_url))
            .json(&serde_json::json!({ "chat_id": self.config.chat_id, "text": text }))
            .send()
            .await
            .map_err(Self::err)?;
        Self::check::<serde_json::Value>(response).await.map(|_| ())
    }

    async fn send_image(
        &self,
        token: &str,
        filename: &str,
        bytes: Vec<u8>,
        caption: &str,
    ) -> Result<()> {
        // Photos above the limit are rejected; send as document instead.
        let method = if bytes.len() > PHOTO_LIMIT_BYTES {
            "sendDocument"
        } else {
            "sendPhoto"
        };
        let field = if method == "sendPhoto" {
            "photo"
        } else {
            "document"
        };
        let caption: String = caption.chars().take(CAPTION_LIMIT).collect();

        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(filename.to_string())
            .mime_str(if filename.ends_with(".png") {
                "image/png"
            } else {
                "image/jpeg"
            })
            .map_err(|e| Self::err(format!("mime: {e}")))?;
        let mut form = reqwest::multipart::Form::new()
            .text("chat_id", self.config.chat_id.clone())
            .part(field.to_string(), part);
        if !caption.is_empty() {
            form = form.text("caption", caption);
        }

        let response = self
            .client
            .post(format!("{}/bot{token}/{method}", self.base_url))
            .multipart(form)
            .send()
            .await
            .map_err(Self::err)?;
        Self::check::<serde_json::Value>(response).await.map(|_| ())
    }

    fn analysis_text(item: &DeliveryItem) -> String {
        let time = item
            .capture
            .taken_at
            .get(11..19)
            .unwrap_or(&item.capture.taken_at);
        match &item.analysis {
            Some(analysis) => {
                let mut text = format!("🖥 {time} — {}", analysis.description);
                if !analysis.ocr_text.is_empty() {
                    text.push_str(&format!("\n\nOn-screen text:\n{}", analysis.ocr_text));
                }
                text
            }
            None => format!("🖥 {time} — capture (no AI analysis available)"),
        }
    }

    /// List chats seen in the bot's recent updates. The user messages the
    /// bot once, clicks "Detect chat ID", and we read it from getUpdates.
    pub async fn discover_chats(&self) -> Result<Vec<DiscoveredChat>> {
        let token = self.token().await?;
        let response = self
            .client
            .get(format!("{}/bot{token}/getUpdates", self.base_url))
            .send()
            .await
            .map_err(Self::err)?;
        let updates: Vec<Update> = Self::check(response).await?;
        let mut chats: Vec<DiscoveredChat> = Vec::new();
        for update in updates {
            let Some(message) = update.message else {
                continue;
            };
            let chat = message.chat;
            if chats.iter().any(|c| c.id == chat.id) {
                continue;
            }
            let label = chat
                .username
                .map(|u| format!("@{u}"))
                .or(chat.title)
                .or(chat.first_name)
                .unwrap_or_else(|| chat.id.to_string());
            chats.push(DiscoveredChat { id: chat.id, label });
        }
        Ok(chats)
    }
}

#[async_trait]
impl Sink for TelegramSink {
    fn kind(&self) -> SinkKind {
        SinkKind::Telegram
    }

    async fn deliver(&self, batch: &[DeliveryItem]) -> Result<()> {
        self.validate()?;
        let token = self.token().await?;
        for item in batch {
            match self.config.content {
                ContentMode::Analysis => {
                    self.send_text(&token, &Self::analysis_text(item)).await?;
                }
                ContentMode::Image | ContentMode::Both => {
                    let path = &item.capture.path;
                    let bytes = tokio::fs::read(path)
                        .await
                        .map_err(|e| Self::err(format!("read {path}: {e}")))?;
                    let filename = Path::new(path)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| format!("capture_{}.jpg", item.capture.id));
                    let caption = if self.config.content == ContentMode::Both {
                        item.analysis
                            .as_ref()
                            .map(|a| a.description.clone())
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    self.send_image(&token, &filename, bytes, &caption).await?;
                }
            }
        }
        Ok(())
    }

    async fn test(&self) -> Result<()> {
        self.validate()?;
        let token = self.token().await?;
        self.send_text(&token, "Screeny is connected to this chat. 🎉")
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemoryStore;
    use crate::store::CaptureRow;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sink_with(server: &MockServer, chat_id: &str) -> TelegramSink {
        let secrets = Arc::new(MemoryStore::default());
        secrets.set(TELEGRAM_BOT_TOKEN, "123:abc").unwrap();
        TelegramSink::with_base_url(
            TelegramConfig {
                enabled: true,
                chat_id: chat_id.into(),
                content: ContentMode::Analysis,
            },
            secrets,
            server.uri(),
        )
    }

    fn item(analysis: Option<crate::llm::Analysis>) -> DeliveryItem {
        DeliveryItem {
            capture: CaptureRow {
                id: 1,
                taken_at: "2026-07-02T10:00:01Z".into(),
                path: "missing.jpg".into(),
                monitor: "M".into(),
                width: 10,
                height: 10,
                status: "captured".into(),
                description: None,
                delivery_summary: None,
            },
            analysis,
        }
    }

    #[tokio::test]
    async fn analysis_mode_sends_text_message() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/bot123:abc/sendMessage"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({ "ok": true, "result": { "message_id": 5 } }),
                ),
            )
            .expect(1)
            .mount(&server)
            .await;

        let sink = sink_with(&server, "42");
        sink.deliver(&[item(Some(crate::llm::Analysis {
            model: "m".into(),
            ocr_text: "hello".into(),
            description: "A test.".into(),
            latency_ms: 1,
        }))])
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn api_error_description_is_surfaced() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/bot123:abc/sendMessage"))
            .respond_with(ResponseTemplate::new(400).set_body_json(
                serde_json::json!({ "ok": false, "description": "Bad Request: chat not found" }),
            ))
            .mount(&server)
            .await;

        let sink = sink_with(&server, "42");
        let err = sink.test().await.unwrap_err().to_string();
        assert!(err.contains("chat not found"), "got: {err}");
    }

    #[tokio::test]
    async fn missing_chat_id_fails_before_network() {
        let server = MockServer::start().await;
        let sink = sink_with(&server, "");
        let err = sink.test().await.unwrap_err().to_string();
        assert!(err.contains("chat ID is empty"), "got: {err}");
    }

    #[tokio::test]
    async fn discover_chats_dedupes_and_labels() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/bot123:abc/getUpdates"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": [
                    { "message": { "chat": { "id": 7, "username": "saad" } } },
                    { "message": { "chat": { "id": 7, "username": "saad" } } },
                    { "message": { "chat": { "id": -100, "title": "My Group" } } },
                    {}
                ]
            })))
            .mount(&server)
            .await;

        let sink = sink_with(&server, "42");
        let chats = sink.discover_chats().await.unwrap();
        assert_eq!(chats.len(), 2);
        assert_eq!(chats[0].label, "@saad");
        assert_eq!(chats[1].label, "My Group");
    }
}
