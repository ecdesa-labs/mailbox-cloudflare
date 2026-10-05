//! Casos de uso e portas (traits). Não pode importar `worker`.

pub mod auth;
pub mod ingest;
pub mod ports;
pub mod process;
pub mod read;
pub mod reconcile;

pub use auth::{
    authenticate, base64url_decode, AccessConfig, AuthError, Identity, SignatureVerifier,
    VerifyError,
};
pub use ingest::{Envelope, IngestEmail, IngestOutcome};
pub use ports::{
    BlobStore, Clock, ExistingMessage, FailureLog, IdGenerator, IngestJob, IngestQueue, MailIndex,
    MessageRecord, PortError, RawListing, RawMailStore, RawObject, RawPage, ReconcileIndex,
    ReprocessMode, ReprocessRequest,
};
pub use process::{
    handle_job, Delivery, ProcessError, ProcessMessage, ProcessOutcome, TRUSTED_AUTHSERV_ID,
};
pub use read::{
    content_disposition, is_inline_safe, list_page, AttachmentMeta, Cursor, DomainSummary,
    IdentitySummary, ListItem, ListQuery, MessageDetail, Page, Person, QueryError, ReadModel,
    ThreadView,
};
pub use reconcile::{Reconcile, ReconcileReport};
