//! Adaptadores D1, R2, Queue e Mailer. Só compila para wasm32; em build
//! nativo o crate fica vazio, assim `cargo test --workspace` não quebra.
#![cfg(target_arch = "wasm32")]

mod d1;
mod queue;
mod r2;
mod runtime;

pub use d1::D1Index;
pub use queue::{QueueIngest, WireIngestJob};
pub use r2::{EmailRawStore, R2Blobs};
pub use runtime::{CryptoIds, WorkerClock};
