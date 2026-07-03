//! Delivery sinks: destinations that receive captured screenshots.
//! M2 ships email; Telegram and WhatsApp arrive in later milestones.

pub mod email;

use async_trait::async_trait;
use serde::Serialize;

use crate::error::Result;
use crate::llm::Analysis;
use crate::store::CaptureRow;

/// A capture on its way out, with analysis when the LLM produced one.
#[derive(Debug, Clone)]
pub struct DeliveryItem {
    pub capture: CaptureRow,
    pub analysis: Option<Analysis>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SinkKind {
    Email,
}

impl SinkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SinkKind::Email => "email",
        }
    }
}

#[async_trait]
pub trait Sink: Send + Sync {
    fn kind(&self) -> SinkKind;

    /// How many captures to accumulate before delivering as one batch.
    fn batch_size(&self) -> usize {
        1
    }

    /// Deliver a batch of captures. Implementations read image bytes from
    /// each item's capture `path`.
    async fn deliver(&self, batch: &[DeliveryItem]) -> Result<()>;

    /// Cheap connectivity check used by the "Send test" button.
    async fn test(&self) -> Result<()>;
}
