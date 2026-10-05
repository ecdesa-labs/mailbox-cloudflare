//! Casca: três handlers + montagem dos adaptadores.
use mailbox_adapters_cf::{
    CryptoIds, D1Index, EmailRawStore, QueueIngest, R2Blobs, WireIngestJob, WorkerClock,
};
use mailbox_application::{
    handle_job, Clock, Delivery, Envelope, FailureLog, IngestEmail, IngestJob, IngestOutcome,
    ProcessMessage, Reconcile,
};
use mailbox_domain::DomainName;
use serde_json::json;
use worker::*;

mod api;

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    api::router().run(req, env).await
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

/// Consumer de `mail-ingest` (docs seção 6.2) e da DLQ (seção 6.6), pelo
/// nome da fila. Nunca devolve `Err`: a macro `#[event(queue)]` transforma
/// `Err` em `panic!`. Cada mensagem é confirmada ou reenviada sozinha.
#[event(queue)]
async fn queue(batch: MessageBatch<serde_json::Value>, env: Env, _ctx: Context) -> Result<()> {
    let messages = match batch.messages() {
        Ok(m) => m,
        Err(e) => {
            console_error!(
                "{}",
                json!({ "event": "queue", "queue": batch.queue(), "error": e.to_string() })
            );
            batch.retry_all();
            return Ok(());
        }
    };
    let dlq = env
        .var("MAILBOX_DLQ")
        .map(|v| v.to_string())
        .unwrap_or_default();
    let (Ok(db), Ok(bucket)) = (env.d1("DB"), env.bucket("MAIL")) else {
        console_error!(r#"{{"event":"queue","error":"bindings DB/MAIL ausentes"}}"#);
        batch.retry_all();
        return Ok(());
    };
    let index = D1Index { db: &db };
    let blobs = R2Blobs { bucket: &bucket };
    let process = ProcessMessage {
        blobs: &blobs,
        index: &index,
        clock: &WorkerClock,
        ids: &CryptoIds,
        full_max_bytes: var_u64(&env, "MAILBOX_FULL_MAX_BYTES", 512 * 1024),
        max_attachments: var_u64(&env, "MAILBOX_MAX_ATTACHMENTS", 30) as usize,
    };

    for msg in messages {
        // Corpo que não é job (ex.: lixo de teste) não tem conserto: descarta.
        let job: IngestJob = match serde_json::from_value::<WireIngestJob>(msg.body().clone()) {
            Ok(w) => w.into(),
            Err(e) => {
                console_error!(
                    "{}",
                    json!({ "event": "queue", "outcome": "invalid_job", "error": e.to_string(), "body": msg.body() })
                );
                msg.ack();
                continue;
            }
        };

        if batch.queue() == dlq {
            // Esgotou as tentativas: registra e libera. O .eml segue no R2.
            match index.give_up(&job.raw_key, None, WorkerClock.now()).await {
                Ok(()) => msg.ack(),
                Err(e) => {
                    console_error!(
                        "{}",
                        json!({ "event": "dlq", "raw_key": job.raw_key, "error": e.0 })
                    );
                    msg.retry();
                }
            }
            console_log!(
                "{}",
                json!({ "event": "dlq", "outcome": "gave_up", "raw_key": job.raw_key })
            );
            continue;
        }

        let (delivery, result) = handle_job(&process, &index, &job).await;
        let entry = match &result {
            Ok(o) => json!({
                "event": "process", "outcome": "ok", "raw_key": job.raw_key, "size": job.size,
                "processing": o.processing.as_str(), "reprocessed": o.reprocessed, "hidden_by_rule": o.hidden_by_rule,
            }),
            Err(e) => {
                json!({ "event": "process", "outcome": "error", "raw_key": job.raw_key, "size": job.size, "error": e.to_string() })
            }
        };
        match delivery {
            Delivery::Ack => {
                console_log!("{entry}");
                msg.ack();
            }
            Delivery::Retry => {
                console_error!("{entry}");
                msg.retry();
            }
        }
    }
    Ok(())
}

/// Reconciliação e pedidos de `just reprocess` (docs seção 6.6).
#[event(scheduled)]
async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    let result = async {
        let domains = configured_domains(&env)?;
        let db = env.d1("DB")?;
        let bucket = env.bucket("MAIL")?;
        let queue = env.queue("INGEST")?;
        Reconcile {
            listing: &R2Blobs { bucket: &bucket },
            index: &D1Index { db: &db },
            queue: &QueueIngest { queue: &queue },
            clock: &WorkerClock,
            domains: &domains,
            min_age_ms: 15 * 60 * 1000,
            budget: var_u64(&env, "MAILBOX_RECONCILE_BUDGET", 50) as usize,
        }
        .run()
        .await
        .map_err(|e| Error::RustError(e.0))
    }
    .await;
    match result {
        Ok(r) => console_log!(
            "{}",
            json!({ "event": "reconcile", "missing": r.missing, "requested": r.requested, "requests_done": r.requests_done })
        ),
        Err(e) => console_error!(
            "{}",
            json!({ "event": "reconcile", "error": e.to_string() })
        ),
    }
}

fn var_u64(env: &Env, name: &str, default: u64) -> u64 {
    env.var(name)
        .ok()
        .and_then(|v| v.to_string().parse().ok())
        .unwrap_or(default)
}
