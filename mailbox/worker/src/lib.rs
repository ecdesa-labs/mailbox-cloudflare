//! Casca: três handlers + montagem dos adaptadores.
use mailbox_adapters_cf::{CryptoIds, EmailRawStore, QueueIngest, WorkerClock};
use mailbox_application::{Envelope, IngestEmail, IngestOutcome};
use mailbox_domain::DomainName;
use serde_json::json;
use worker::*;

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        .get("/api/ping", |_, _| Response::ok("pong"))
        .run(req, env)
        .await
}

/// Exportação `email` escrita à mão, sem `#[event(email)]`: no workers-rs
/// 0.8.7 a macro transforma `Err` em `panic!`, e o pânico vira trap
/// (`unreachable`) que pode deixar a instância wasm inutilizável. Aqui o
/// `Err` vira um `Error` JavaScript comum (promise rejeitada), que é o que o
/// Email Routing trata como falha do handler.
mod email_export {
    use worker::wasm_bindgen::prelude::wasm_bindgen;
    use worker::wasm_bindgen::JsValue;

    #[wasm_bindgen(
        wasm_bindgen = ::worker::wasm_bindgen,
        wasm_bindgen_futures = ::worker::wasm_bindgen_futures,
        js_sys = ::worker::js_sys
    )]
    pub async fn email(
        message: ::worker::ForwardableEmailMessage,
        env: ::worker::Env,
        _ctx: ::worker::worker_sys::Context,
    ) -> Result<JsValue, JsValue> {
        super::handle_email(message, env)
            .await
            .map(|()| JsValue::UNDEFINED)
            .map_err(|e| ::worker::js_sys::Error::new(&e.to_string()).into())
    }
}

/// Docs seção 6.1. `Err` é falha temporária (o remetente tenta de novo);
/// `set_reject` é permanente e só vale para destinatário inválido.
async fn handle_email(message: ForwardableEmailMessage, env: Env) -> Result<()> {
    let domains = configured_domains(&env)?;
    let bucket = env.bucket("MAIL")?;
    let queue = env.queue("INGEST")?;
    let envelope = Envelope {
        from: message.from(),
        to: message.to(),
        size: message.raw_size() as u64,
    };

    // Teste em produção do que o remetente recebe numa falha temporária.
    // Só existe com `wrangler deploy --var MAILBOX_TEST_TEMPFAIL:<endereço>`.
    if let Ok(addr) = env.var("MAILBOX_TEST_TEMPFAIL") {
        if addr.to_string().eq_ignore_ascii_case(&envelope.to) {
            console_error!(
                r#"{{"event":"ingest","outcome":"test_tempfail","to":"{}"}}"#,
                envelope.to
            );
            return Err(Error::RustError("falha temporária de teste".into()));
        }
    }

    let outcome = IngestEmail {
        clock: &WorkerClock,
        ids: &CryptoIds,
        store: &EmailRawStore {
            message: &message,
            bucket: &bucket,
        },
        queue: &QueueIngest { queue: &queue },
        domains: &domains,
    }
    .run(&envelope)
    .await;

    let log = |outcome: &str, extra: serde_json::Value| {
        let mut entry = json!({ "event": "ingest", "outcome": outcome, "to": envelope.to, "size": envelope.size });
        if let (Some(e), Some(x)) = (entry.as_object_mut(), extra.as_object()) {
            e.extend(x.clone());
        }
        entry.to_string()
    };
    match outcome {
        IngestOutcome::Accepted { raw_key } => {
            console_log!(
                "{}",
                log("accepted", json!({ "raw_key": raw_key.as_str() }))
            );
            Ok(())
        }
        IngestOutcome::AcceptedNotQueued { raw_key, error } => {
            // Já está no R2: aceitar. A reconciliação (seção 6.6) enfileira.
            console_error!(
                "{}",
                log(
                    "accepted_not_queued",
                    json!({ "raw_key": raw_key.as_str(), "error": error.0 })
                )
            );
            Ok(())
        }
        IngestOutcome::Rejected { reason } => {
            console_log!("{}", log("rejected", json!({ "reason": reason })));
            message.set_reject(reason);
            Ok(())
        }
        IngestOutcome::TemporaryFailure { error } => {
            console_error!("{}", log("temporary_failure", json!({ "error": error.0 })));
            Err(Error::RustError(format!("ingest: {error}")))
        }
    }
}

/// `MAILBOX_DOMAINS` vem do `instance.json` (gerado no `wrangler.toml`).
/// Ausente ou inválido é erro de configuração: falha temporária, nunca rejeição.
fn configured_domains(env: &Env) -> Result<Vec<DomainName>> {
    env.var("MAILBOX_DOMAINS")?
        .to_string()
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|d| {
            DomainName::parse(d).map_err(|e| Error::RustError(format!("MAILBOX_DOMAINS: {e}")))
        })
        .collect()
}

// Fase 4 implementa. Até lá registra o que chegou e devolve para a fila
// (acaba na DLQ), sem descartar: o `.eml` está no R2 para reprocessar.
// Nunca devolver `Err` daqui: a macro `#[event(queue)]` também vira `panic!`.
#[event(queue)]
async fn queue(batch: MessageBatch<serde_json::Value>, _env: Env, _ctx: Context) -> Result<()> {
    match batch.messages() {
        Ok(msgs) => {
            for msg in msgs {
                console_log!(
                    "{}",
                    json!({ "event": "queue_stub", "queue": batch.queue(), "job": msg.body() })
                );
            }
        }
        Err(e) => console_error!(
            "{}",
            json!({ "event": "queue_stub", "queue": batch.queue(), "error": e.to_string() })
        ),
    }
    batch.retry_all();
    Ok(())
}
