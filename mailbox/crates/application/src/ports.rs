//! Portas (docs seção 4.2). Os adaptadores da Cloudflare ficam em `adapters-cf`.

use std::fmt;

use mailbox_domain::{
    Attachment, DomainId, DomainName, EmailAddress, IdentityId, Message, MessageId, Processing,
    Rule, Thread, ThreadId, UnixMs,
};

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
    fn identity_id(&self, at: UnixMs) -> IdentityId;
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
    /// Uma sub-requisição para até 100 jobs.
    async fn enqueue_batch(&self, jobs: &[IngestJob]) -> Result<(), PortError>;
}

/// Objetos do R2 por chave (raw, anexos, HTML).
#[allow(async_fn_in_trait)]
pub trait BlobStore {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, PortError>;
    async fn put(&self, key: &str, data: &[u8], content_type: &str) -> Result<(), PortError>;
}

/// O que já existe no índice para uma mensagem (reprocessamento).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingMessage {
    pub thread_id: Option<ThreadId>,
}

/// Tudo que o processamento grava no índice de uma vez.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRecord {
    pub message: Message,
    pub processing: Processing,
    pub attachments: Vec<Attachment>,
    /// Criada se ainda não existir; contadores recalculados a partir das mensagens.
    pub thread: Thread,
    /// Primeira vez que esta mensagem entra no índice. Num reprocessamento,
    /// pasta, lido, estrela e exclusão do usuário são preservados.
    pub is_new: bool,
}

/// Índice (D1). Gravar é idempotente por `message.id`.
#[allow(async_fn_in_trait)]
pub trait MailIndex {
    async fn domain_id(&self, name: &DomainName) -> Result<Option<DomainId>, PortError>;
    async fn existing(&self, id: MessageId) -> Result<Option<ExistingMessage>, PortError>;
    /// Thread de alguma mensagem cujo `Message-ID` está em `message_ids`.
    async fn thread_of(&self, message_ids: &[String]) -> Result<Option<ThreadId>, PortError>;
    /// Thread de alguma mensagem que responde a `message_id` (chegou antes dela).
    async fn thread_of_reply_to(&self, message_id: &str) -> Result<Option<ThreadId>, PortError>;
    async fn rules(&self) -> Result<Vec<Rule>, PortError>;
    /// Identidade do endereço; cria com `new_id` se não existir.
    async fn ensure_identity(
        &self,
        domain_id: DomainId,
        address: &EmailAddress,
        new_id: IdentityId,
    ) -> Result<IdentityId, PortError>;
    /// Grava tudo numa transação e limpa a falha registrada desse `raw_key`.
    async fn save(&self, record: &MessageRecord) -> Result<(), PortError>;
}

/// Falhas de processamento (`ingest_failures`).
#[allow(async_fn_in_trait)]
pub trait FailureLog {
    /// Mais uma tentativa que falhou; ainda vai ser tentada de novo.
    async fn record(&self, raw_key: &str, error: &str, at: UnixMs) -> Result<(), PortError>;
    /// Desistiu (erro permanente ou fim das tentativas, via DLQ).
    async fn give_up(
        &self,
        raw_key: &str,
        error: Option<&str>,
        at: UnixMs,
    ) -> Result<(), PortError>;
}

/// Objeto `raw/` no R2, como a listagem devolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawObject {
    pub key: String,
    pub uploaded_at: UnixMs,
    /// Só quando a listagem pediu metadados (refaz o job do envelope).
    pub job: Option<IngestJob>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawPage {
    pub objects: Vec<RawObject>,
    /// `None` = acabou.
    pub cursor: Option<String>,
}

/// Leitura do `raw/` no R2 para a reconciliação.
#[allow(async_fn_in_trait)]
pub trait RawListing {
    /// Uma página (até 1000). `with_jobs` inclui os metadados, que custam
    /// CPU para ler: só quando todos os objetos da página vão ser usados.
    async fn list(
        &self,
        prefix: &str,
        cursor: Option<&str>,
        with_jobs: bool,
    ) -> Result<RawPage, PortError>;
    /// Refaz o job a partir dos metadados do objeto (envelope). `None` se
    /// o objeto não existe.
    async fn job_for(&self, key: &str) -> Result<Option<IngestJob>, PortError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReprocessMode {
    Key,
    Failed,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReprocessRequest {
    pub id: i64,
    pub mode: ReprocessMode,
    pub raw_key: Option<String>,
    pub cursor: Option<String>,
}

/// Consultas e marcações da reconciliação (D1).
#[allow(async_fn_in_trait)]
pub trait ReconcileIndex {
    /// Chaves com o prefixo que já estão em `messages` ou em
    /// `ingest_failures` (em andamento ou desistidas).
    async fn handled_with_prefix(
        &self,
        prefix: &str,
    ) -> Result<std::collections::HashSet<String>, PortError>;
    async fn pending_requests(&self) -> Result<Vec<ReprocessRequest>, PortError>;
    async fn update_request(
        &self,
        id: i64,
        cursor: Option<&str>,
        enqueued: usize,
        done: bool,
    ) -> Result<(), PortError>;
    /// Até `limit` chaves que foram desistidas.
    async fn given_up(&self, limit: usize) -> Result<Vec<String>, PortError>;
    /// Apaga as falhas dessas chaves (vão ser tentadas de novo).
    async fn clear_failures(&self, raw_keys: &[String]) -> Result<(), PortError>;
}
