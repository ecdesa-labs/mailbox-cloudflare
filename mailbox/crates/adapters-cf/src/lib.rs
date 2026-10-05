//! Adaptadores D1, R2, Queue e Mailer. Só compila para wasm32; em build
//! nativo o crate fica vazio, assim `cargo test --workspace` não quebra.
#![cfg(target_arch = "wasm32")]
