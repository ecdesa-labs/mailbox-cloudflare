//! Adaptadores D1, R2, Queue e Mailer. Só compila para wasm32; em build
//! nativo o crate fica vazio, assim `cargo test --workspace` não quebra.
#![cfg(target_arch = "wasm32")]

mod queue;
mod r2;
mod runtime;

pub use queue::{QueueIngest, WireIngestJob};
pub use r2::EmailRawStore;
pub use runtime::{CryptoIds, WorkerClock};
