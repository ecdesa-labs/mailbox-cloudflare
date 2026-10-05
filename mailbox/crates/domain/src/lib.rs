//! Entidades, value objects e regras. Puro: sem I/O, sem `worker`.

pub mod address;
pub mod auth;
pub mod error;
pub mod folder;
pub mod id;
pub mod keys;
pub mod model;
pub mod processing;
pub mod rule;
pub mod subject;
pub mod text;
pub mod time;

pub use address::{DomainName, EmailAddress, Mailbox};
pub use auth::{AuthResults, AuthVerdict};
pub use error::DomainError;
pub use folder::Folder;
pub use id::{AttachmentId, DomainId, IdentityId, MessageId, RuleId, ThreadId};
pub use keys::{AttachmentKey, HtmlKey, RawKey};
pub use model::{Attachment, Domain, Identity, Message, Thread};
pub use processing::Processing;
pub use rule::{MatchPattern, Rule, RuleAction, RuleOutcome};
pub use subject::normalize_subject;
pub use text::{snippet, truncate_text_body, SNIPPET_MAX_CHARS, TEXT_BODY_MAX_BYTES};
pub use time::UnixMs;
