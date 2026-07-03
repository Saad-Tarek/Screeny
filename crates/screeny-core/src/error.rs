use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("screen capture failed: {0}")]
    Capture(String),

    #[error("image encoding failed: {0}")]
    Encode(String),

    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("secret storage error: {0}")]
    Secret(String),

    #[error("delivery failed via {sink}: {message}")]
    Delivery { sink: String, message: String },

    #[error("llm error: {0}")]
    Llm(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;
