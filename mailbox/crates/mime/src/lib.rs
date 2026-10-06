//! bytes -> ParsedMessage, sanitização de HTML, reescrita de `cid:`,
//! bloqueio de imagens remotas e montagem do `.eml` enviado. Puro: sem I/O
//! e sem `worker`.
//!
//! Este crate não conhece IDs nem chaves do R2. Quem chama decide como
//! resolver um `cid:` (ex.: para `/api/attachments/{id}`).

mod build;
mod parse;
mod sanitize;

pub use build::{build, OutAddress, Outgoing};
pub use parse::{
    parse, parse_headers, Address, Attachment, AuthResultsHeader, MimeError, ParsedMessage,
    ReceivedSpf,
};
pub use sanitize::{restore_remote_images, sanitize_html, REMOTE_SRC_ATTR};
