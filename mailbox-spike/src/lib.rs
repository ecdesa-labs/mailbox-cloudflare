//! Spike descartável: prova que fetch + email + queue convivem no mesmo Worker
//! em Rust, que R2 e Queue funcionam, e que mail-parser e ammonia rodam em WASM.
//! Não faz parte do workspace final (docs seção 4.1).

use mail_parser::MessageParser;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use worker::*;

// `FixedLengthStream` do runtime (TransformStream com tamanho declarado).
// O R2 só aceita stream com tamanho conhecido; o do email não tem.
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = web_sys::TransformStream, js_name = FixedLengthStream)]
    type JsFixedLengthStream;
    #[wasm_bindgen(constructor, js_class = "FixedLengthStream")]
    fn new(length: f64) -> JsFixedLengthStream;
}

const KNOWN_DOMAINS: &[&str] = &["acmeco.com.br", "pessoal.dev"];

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IngestJob {
    pub raw_key: String,
    pub domain: String,
    pub envelope_from: String,
    pub envelope_to: String,
    pub received_at: u64,
    pub size: usize,
}

/// Placeholder do spike. O ULID de verdade entra na Fase 1 (crate `domain`).
fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{:013}-{n:06}", Date::now().as_millis())
}

// ---------------------------------------------------------------- fetch ----

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        .get("/api/ping", |_, _| Response::ok("pong"))
        .get_async("/api/spike/list", |_, ctx| async move {
            let bucket = ctx.env.bucket("MAIL")?;
            let listing = bucket.list().execute().await?;
            let items: Vec<serde_json::Value> = listing
                .objects()
                .iter()
                .map(|o| serde_json::json!({ "key": o.key(), "size": o.size() }))
                .collect();
            Response::from_json(&items)
        })
        // Enfileira um job cujo raw_key não existe: o consumer deve falhar,
        // tentar de novo e, esgotadas as tentativas, mandar para a DLQ.
        .post_async("/api/spike/poison", |_, ctx| async move {
            let job = IngestJob {
                raw_key: "pessoal.dev/raw/inexistente.eml".into(),
                domain: "pessoal.dev".into(),
                envelope_from: "x@y.com".into(),
                envelope_to: "ctf@pessoal.dev".into(),
                received_at: Date::now().as_millis(),
                size: 0,
            };
            ctx.env.queue("INGEST")?.send(job).await?;
            Response::ok("poison enfileirado")
        })
        .run(req, env)
        .await
}

// ---------------------------------------------------------------- email ----

#[event(email)]
async fn email(message: ForwardableEmailMessage, env: Env, _ctx: Context) -> Result<()> {
    let envelope_to = message.to();
    let envelope_from = message.from();
    let domain = envelope_to
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();

    // Domínio desconhecido: rejeição permanente (docs 6.1, passo 1).
    if !KNOWN_DOMAINS.contains(&domain.as_str()) {
        message.set_reject("unknown domain");
        return Ok(());
    }

    let id = new_id();
    let raw_key = format!("{domain}/raw/{id}.eml");

    // Item 7: `stream@` grava por stream (os bytes não entram no wasm);
    // qualquer outro endereço usa o buffer completo, como antes.
    // Se qualquer `?` abaixo falhar, o handler devolve Err. Verificar no
    // ambiente real se isso vira erro temporário (4xx) para o remetente.
    let size = message.raw_size() as usize;
    let bucket = env.bucket("MAIL")?;
    if envelope_to.starts_with("stream@") {
        let fls = JsFixedLengthStream::new(size as f64);
        let pipe = message.raw().pipe_to(&fls.writable());
        bucket.put(&raw_key, fls.readable()).execute().await?;
        JsFuture::from(pipe).await?;
    } else {
        let bytes = message.raw_bytes().await?;
        bucket.put(&raw_key, bytes).execute().await?;
    }

    let job = IngestJob {
        raw_key: raw_key.clone(),
        domain,
        envelope_from,
        envelope_to,
        received_at: Date::now().as_millis(),
        size,
    };
    env.queue("INGEST")?.send(job).await?;

    console_log!("email gravado: {raw_key} ({size} bytes)");
    Ok(())
}

// ---------------------------------------------------------------- queue ----

#[event(queue)]
async fn queue(batch: MessageBatch<IngestJob>, env: Env, _ctx: Context) -> Result<()> {
    let bucket = env.bucket("MAIL")?;
    for msg in batch.messages()? {
        let job = msg.body().clone();
        match process(&bucket, &job).await {
            Ok(()) => msg.ack(),
            Err(e) => {
                console_error!("falha ao processar {}: {e}", job.raw_key);
                msg.retry();
            }
        }
    }
    Ok(())
}

async fn process(bucket: &Bucket, job: &IngestJob) -> Result<()> {
    let obj = bucket
        .get(&job.raw_key)
        .execute()
        .await?
        .ok_or_else(|| Error::RustError(format!("raw ausente: {}", job.raw_key)))?;
    let bytes = obj
        .body()
        .ok_or_else(|| Error::RustError("objeto sem corpo".into()))?
        .bytes()
        .await?;

    let parsed = MessageParser::default()
        .parse(&bytes)
        .ok_or_else(|| Error::RustError("mime inválido".into()))?;

    let subject = parsed.subject().unwrap_or("(sem assunto)").to_string();
    let html_clean = parsed
        .body_html(0)
        .map(|h| ammonia::clean(&h))
        .unwrap_or_default();

    let file = job.raw_key.rsplit('/').next().unwrap_or("x");
    let marker_key = format!("{}/spike/{file}.txt", job.domain);
    let summary = format!(
        "subject={subject}\nhtml_sanitized_bytes={}\nattachments={}\n---\n{html_clean}\n",
        html_clean.len(),
        parsed.attachment_count()
    );
    bucket.put(&marker_key, summary.into_bytes()).execute().await?;

    console_log!("processado: {} -> {marker_key}", job.raw_key);
    Ok(())
}