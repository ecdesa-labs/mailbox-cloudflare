//! Portas (docs seção 4.2). Os adaptadores da Cloudflare ficam em `adapters-cf`.

use std::fmt;

use mailbox_domain::{MessageId, UnixMs};

/// Falha de infraestrutura, com a mensagem do adaptador para log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortError(pub String);

impl fmt::Display for PortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PortError {}

pub trait Clock {
    fn now(&self) -> UnixMs;
}

pub trait IdGenerator {
    fn message_id(&self, at: UnixMs) -> MessageId;
}

/// Grava o `.eml` recebido em `job.raw_key`. O adaptador já conhece a origem
/// dos bytes (o stream do email). Os campos do envelope vão junto como
/// metadados: não estão no `.eml` e a reconciliação (seção 6.6) precisa
/// refazer o job só a partir do R2.
#[allow(async_fn_in_trait)] // só usado com tipos concretos, sem `dyn` nem `Send`
pub trait RawMailStore {
    async fn store(&self, job: &IngestJob) -> Result<(), PortError>;
}

/// Payload da fila `mail-ingest` (docs seção 5.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestJob {
    pub raw_key: String,
    pub domain: String,
    /// Como veio no envelope; vazio em bounce (`MAIL FROM:<>`).
    pub envelope_from: String,
    pub envelope_to: String,
    pub received_at: i64,
    pub size: u64,
}

#[allow(async_fn_in_trait)]
pub trait IngestQueue {
    async fn enqueue(&self, job: &IngestJob) -> Result<(), PortError>;
}
