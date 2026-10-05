//! Casos de uso e portas (traits). Não pode importar `worker`.

pub mod ingest;
pub mod ports;
pub mod process;
pub mod reconcile;

pub use ingest::{Envelope, IngestEmail, IngestOutcome};
pub use ports::{
    BlobStore, Clock, ExistingMessage, FailureLog, IdGenerator, IngestJob, IngestQueue, MailIndex,
    MessageRecord, PortError, RawListing, RawMailStore, RawObject, RawPage, ReconcileIndex,
    ReprocessMode, ReprocessRequest,
};
pub use process::{
    handle_job, Delivery, ProcessError, ProcessMessage, ProcessOutcome, TRUSTED_AUTHSERV_ID,
};
pub use reconcile::{Reconcile, ReconcileReport};
