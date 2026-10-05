use mailbox_application::{IngestJob, IngestQueue, PortError};
use serde::{Deserialize, Serialize};
use worker::Queue;

/// Formato JSON do job na fila (docs seção 5.3). Separado do `IngestJob`
/// para o `application` não depender de serde.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireIngestJob {
    pub raw_key: String,
    pub domain: String,
    pub envelope_from: String,
    pub envelope_to: String,
    pub received_at: i64,
    pub size: u64,
}

impl From<&IngestJob> for WireIngestJob {
    fn from(j: &IngestJob) -> Self {
        Self {
            raw_key: j.raw_key.clone(),
            domain: j.domain.clone(),
            envelope_from: j.envelope_from.clone(),
            envelope_to: j.envelope_to.clone(),
            received_at: j.received_at,
            size: j.size,
        }
    }
}

impl From<WireIngestJob> for IngestJob {
    fn from(j: WireIngestJob) -> Self {
        Self {
            raw_key: j.raw_key,
            domain: j.domain,
            envelope_from: j.envelope_from,
            envelope_to: j.envelope_to,
            received_at: j.received_at,
            size: j.size,
        }
    }
}

pub struct QueueIngest<'a> {
    pub queue: &'a Queue,
}

impl IngestQueue for QueueIngest<'_> {
    async fn enqueue(&self, job: &IngestJob) -> Result<(), PortError> {
        self.queue
            .send(WireIngestJob::from(job))
            .await
            .map_err(|e| PortError(format!("queue send {}: {e}", job.raw_key)))
    }
}
