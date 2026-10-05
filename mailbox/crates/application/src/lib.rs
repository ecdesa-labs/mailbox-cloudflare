//! Casos de uso e portas (traits). Não pode importar `worker`.

pub mod ingest;
pub mod ports;

pub use ingest::{Envelope, IngestEmail, IngestOutcome};
pub use ports::{Clock, IdGenerator, IngestJob, IngestQueue, PortError, RawMailStore};
