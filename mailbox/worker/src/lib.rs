//! Casca: três handlers + montagem dos adaptadores.
use mailbox_adapters_cf::{
    CryptoIds, D1Index, EmailRawStore, QueueIngest, R2Blobs, WireIngestJob, WorkerClock,
};
use mailbox_application::{
    handle_job, Clock, Delivery, Envelope, FailureLog, IngestEmail, IngestJob, IngestOutcome,
    ProcessMessage, Reconcile, Retention, RetentionPolicy,
};
use mailbox_domain::DomainName;
use obs::Level;
use serde_json::json;
use worker::*;

mod api;
mod obs;

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    // Só /api/* chega aqui (run_worker_first); o front estático é servido
    // direto pelos Assets, atrás do Access na borda.
    if let Some(resp) = api::csrf_check(&req) {
        return Ok(resp);
    }
    let path = req.path();
    let identity = if path == "/api/ping" {
        None
    } else {
        match api::authorize(&req, &env).await {
            Ok(i) => Some(i),
            Err(resp) => return Ok(resp),
        }
    };
    let method = req.method().to_string();
    let route = route_label(&path);
    let started = Date::now().as_millis();
    let resp = api::router(identity).run(req, env.clone()).await?;
    let status = resp.status_code();
    let fields = json!({ "method": method, "route": route, "status": status, "outcome": status.to_string(),
                         "ms": Date::now().as_millis().saturating_sub(started) });
    // Métrica de toda requisição; log só de erro do servidor.
    obs::metric(&env, "api", Level::Info, &fields);
    if status >= 500 {
        obs::log(Level::Error, "api", &fields);
    }
    Ok(resp)
}

/// Rota sem IDs (`/api/messages/01J.../raw` → `/api/messages/:id/raw`), para
/// agrupar métricas.
fn route_label(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            if seg.len() == 26 && seg.chars().all(|c| c.is_ascii_alphanumeric()) {
                ":id"
            } else {
                seg
            }
        })
        .collect::<Vec<_>>()
        .join("/")
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
            obs::event(
                &env,
                Level::Error,
                "ingest",
                json!({ "outcome": "test_tempfail", "to": envelope.to }),
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

    let domain = envelope
        .to
        .rsplit_once('@')
        .map(|(_, d)| d.to_ascii_lowercase())
        .unwrap_or_default();
    let emit = |level, outcome: &str, extra: serde_json::Value| {
        let mut f = json!({ "outcome": outcome, "to": envelope.to, "domain": domain, "size": envelope.size });
        if let (Some(e), Some(x)) = (f.as_object_mut(), extra.as_object()) {
            e.extend(x.clone());
        }
        obs::event(&env, level, "ingest", f);
    };
    match outcome {
        IngestOutcome::Accepted { raw_key } => {
            emit(
                Level::Info,
                "accepted",
                json!({ "raw_key": raw_key.as_str() }),
            );
            Ok(())
        }
        IngestOutcome::AcceptedNotQueued { raw_key, error } => {
            // Já está no R2: aceitar. A reconciliação (seção 6.6) enfileira.
            emit(
                Level::Error,
                "accepted_not_queued",
                json!({ "raw_key": raw_key.as_str(), "error": error.0 }),
            );
            Ok(())
        }
        IngestOutcome::Rejected { reason } => {
            emit(Level::Warn, "rejected", json!({ "reason": reason }));
            message.set_reject(reason);
            Ok(())
        }
        IngestOutcome::TemporaryFailure { error } => {
            emit(
                Level::Error,
                "temporary_failure",
                json!({ "error": error.0 }),
            );
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
            obs::event(
                &env,
                Level::Error,
                "queue",
                json!({ "queue": batch.queue(), "error": e.to_string() }),
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
        obs::event(
            &env,
            Level::Error,
            "queue",
            json!({ "error": "bindings DB/MAIL ausentes" }),
        );
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
                obs::event(
                    &env,
                    Level::Error,
                    "queue",
                    json!({ "outcome": "invalid_job", "error": e.to_string(), "body": msg.body() }),
                );
                msg.ack();
                continue;
            }
        };

        if batch.queue() == dlq {
            // Esgotou as tentativas: registra e libera. O .eml segue no R2.
            match index.give_up(&job.raw_key, None, WorkerClock.now()).await {
                Ok(()) => {
                    obs::event(
                        &env,
                        Level::Warn,
                        "dlq",
                        json!({ "outcome": "gave_up", "raw_key": job.raw_key, "domain": job.domain }),
                    );
                    msg.ack();
                }
                Err(e) => {
                    obs::event(
                        &env,
                        Level::Error,
                        "dlq",
                        json!({ "outcome": "error", "raw_key": job.raw_key, "domain": job.domain, "error": e.0 }),
                    );
                    msg.retry();
                }
            }
            continue;
        }

        let (delivery, result) = handle_job(&process, &index, &job).await;
        let fields = match &result {
            // `outcome` = modo de processamento, para a métrica separar full/partial.
            Ok(o) => json!({
                "outcome": o.processing.as_str(), "raw_key": job.raw_key, "domain": job.domain, "size": job.size,
                "reprocessed": o.reprocessed, "hidden_by_rule": o.hidden_by_rule,
            }),
            Err(e) => json!({
                "outcome": "error", "raw_key": job.raw_key, "domain": job.domain, "size": job.size, "error": e.to_string(),
            }),
        };
        match delivery {
            Delivery::Ack => {
                let level = if result.is_ok() {
                    Level::Info
                } else {
                    Level::Warn
                };
                obs::event(&env, level, "process", fields);
                msg.ack();
            }
            Delivery::Retry => {
                obs::event(&env, Level::Error, "process", fields);
                msg.retry();
            }
        }
    }
    Ok(())
}

/// Cron: reconciliação e pedidos de `just reprocess` (docs seção 6.6), depois
/// retenção (Fase 9). Uma falha não impede a outra.
#[event(scheduled)]
async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    let reconcile = async {
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
    match reconcile {
        Ok(r) => obs::event(
            &env,
            Level::Info,
            "reconcile",
            json!({
                "outcome": "ok", "missing": r.missing, "requested": r.requested, "requests_done": r.requests_done,
                "count": r.missing + r.requested,
            }),
        ),
        Err(e) => obs::event(
            &env,
            Level::Error,
            "reconcile",
            json!({ "outcome": "error", "error": e.to_string() }),
        ),
    }

    let retention = async {
        let db = env.d1("DB")?;
        let bucket = env.bucket("MAIL")?;
        let index = D1Index { db: &db };
        Retention {
            store: &index,
            blobs: &R2Blobs { bucket: &bucket },
            clock: &WorkerClock,
            policy: RetentionPolicy {
                trash_days: var_u64(&env, "MAILBOX_RETENTION_TRASH_DAYS", 30) as u32,
                spam_days: var_u64(&env, "MAILBOX_RETENTION_SPAM_DAYS", 30) as u32,
                hidden_days: var_u64(&env, "MAILBOX_RETENTION_HIDDEN_DAYS", 30) as u32,
            },
            batch: var_u64(&env, "MAILBOX_RETENTION_BATCH", 50) as usize,
        }
        .run()
        .await
        .map_err(|e| Error::RustError(e.0))
    }
    .await;
    match retention {
        Ok(0) => {}
        Ok(n) => obs::event(
            &env,
            Level::Info,
            "retention",
            json!({ "outcome": "purged", "count": n }),
        ),
        Err(e) => obs::event(
            &env,
            Level::Error,
            "retention",
            json!({ "outcome": "error", "error": e.to_string() }),
        ),
    }
}

fn var_u64(env: &Env, name: &str, default: u64) -> u64 {
    env.var(name)
        .ok()
        .and_then(|v| v.to_string().parse().ok())
        .unwrap_or(default)
}
