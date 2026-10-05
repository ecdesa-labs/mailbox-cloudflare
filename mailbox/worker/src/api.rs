//! API de leitura (docs seção 7). JSON, datas em Unix ms, cursor opaco.
//! Erros: `{ "error": { "code", "message" } }` com status coerente.

use mailbox_adapters_cf::{D1Index, R2Blobs};
use mailbox_application::{
    content_disposition, is_inline_safe, list_page, AttachmentMeta, BlobStore, DomainSummary,
    ListItem, ListQuery, MessageDetail, Person, QueryError, ReadModel, ThreadView,
};
use mailbox_domain::{AttachmentId, MessageId, ThreadId};
use mailbox_mime::{restore_remote_images, REMOTE_SRC_ATTR};
use serde_json::{json, Value};
use worker::*;

pub fn router() -> Router<'static, ()> {
    Router::new()
        .get("/api/ping", |_, _| Response::ok("pong"))
        .get_async("/api/domains", |_, ctx| async move {
            handle(domains(&ctx.env).await)
        })
        .get_async("/api/messages", |req, ctx| async move {
            handle(messages(&req, &ctx.env).await)
        })
        .get_async("/api/messages/:id", |req, ctx| async move {
            handle(message(&req, &ctx.env, ctx.param("id")).await)
        })
        .get_async("/api/messages/:id/raw", |_, ctx| async move {
            handle(raw(&ctx.env, ctx.param("id")).await)
        })
        .get_async("/api/threads/:id", |_, ctx| async move {
            handle(thread(&ctx.env, ctx.param("id")).await)
        })
        .get_async("/api/attachments/:id", |_, ctx| async move {
            handle(attachment(&ctx.env, ctx.param("id")).await)
        })
        .or_else_any_method("/api/*rest", |_, _| {
            error_response(404, "not_found", "rota desconhecida")
        })
}

/// Erro da API: vira JSON com status.
pub struct ApiError {
    status: u16,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn not_found(what: &str) -> Self {
        Self {
            status: 404,
            code: "not_found",
            message: format!("{what} não encontrado"),
        }
    }
    fn bad_request(message: String) -> Self {
        Self {
            status: 400,
            code: "bad_request",
            message,
        }
    }
}

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        Self {
            status: 500,
            code: "internal",
            message: e.to_string(),
        }
    }
}

impl From<mailbox_application::PortError> for ApiError {
    fn from(e: mailbox_application::PortError) -> Self {
        Self {
            status: 503,
            code: "unavailable",
            message: e.0,
        }
    }
}

impl From<QueryError> for ApiError {
    fn from(e: QueryError) -> Self {
        match e {
            QueryError::Invalid { .. } => Self::bad_request(e.to_string()),
            QueryError::NotImplemented { .. } => Self {
                status: 501,
                code: "not_implemented",
                message: e.to_string(),
            },
        }
    }
}

fn error_response(status: u16, code: &str, message: &str) -> Result<Response> {
    Ok(
        Response::from_json(&json!({ "error": { "code": code, "message": message } }))?
            .with_status(status),
    )
}

fn handle(result: std::result::Result<Response, ApiError>) -> Result<Response> {
    match result {
        Ok(r) => Ok(r),
        Err(e) => {
            if e.status >= 500 {
                console_error!(
                    "{}",
                    json!({ "event": "api", "status": e.status, "error": e.message })
                );
            }
            error_response(e.status, e.code, &e.message)
        }
    }
}

/// JSON sem cache compartilhado: é caixa de email pessoal.
fn json_ok(v: &Value) -> std::result::Result<Response, ApiError> {
    let mut r = Response::from_json(v)?;
    r.headers_mut().set("Cache-Control", "private, no-store")?;
    Ok(r)
}

fn parse_id<T>(
    raw: Option<&String>,
    parse: impl Fn(&str) -> std::result::Result<T, mailbox_domain::DomainError>,
    what: &str,
) -> std::result::Result<T, ApiError> {
    raw.and_then(|r| parse(r).ok())
        .ok_or_else(|| ApiError::not_found(what))
}

// ---------------------------------------------------------------- JSON ----

fn person(p: &Person) -> Value {
    json!({ "name": p.name, "addr": p.addr })
}

fn item_json(i: &ListItem) -> Value {
    json!({
        "id": i.id.to_string(),
        "thread_id": i.thread_id.map(|t| t.to_string()),
        "domain": i.domain,
        "identity": i.identity,
        "from": i.from.as_ref().map(person),
        "subject": i.subject,
        "snippet": i.snippet,
        "received_at": i.received_at.as_i64(),
        "is_read": i.is_read,
        "is_starred": i.is_starred,
        "has_attachments": i.has_attachments,
        "folder": i.folder.as_str(),
        "processing": i.processing.as_str(),
    })
}

fn attachment_json(a: &AttachmentMeta) -> Value {
    json!({
        "id": a.id.to_string(),
        "filename": a.filename,
        "content_type": a.content_type,
        "size": a.size_bytes,
        "content_id": a.content_id,
        "is_inline": a.is_inline,
        "url": format!("/api/attachments/{}", a.id),
    })
}

fn domain_json(d: &DomainSummary) -> Value {
    json!({
        "id": d.id,
        "name": d.name,
        "label": d.label,
        "color": d.color,
        "unread": d.unread,
        "identities": d.identities.iter().map(|i| json!({
            "id": i.id, "address": i.address, "label": i.label, "is_default_from": i.is_default_from,
        })).collect::<Vec<_>>(),
    })
}

fn detail_json(m: &MessageDetail, html: Option<String>) -> Value {
    let mut v = item_json(&m.item);
    let has_remote_images = html.as_deref().is_some_and(|h| h.contains(REMOTE_SRC_ATTR));
    let extra = json!({
        "to": m.to.iter().map(person).collect::<Vec<_>>(),
        "cc": m.cc.iter().map(person).collect::<Vec<_>>(),
        "date": m.date_hdr.map(|d| d.as_i64()),
        "message_id": m.message_id_hdr,
        "envelope_to": m.envelope_to,
        "size": m.size_bytes,
        "text": m.text_body,
        "html": html,
        "has_remote_images": has_remote_images,
        "auth": { "spf": m.spf, "dkim": m.dkim, "dmarc": m.dmarc },
        "attachments": m.attachments.iter().map(attachment_json).collect::<Vec<_>>(),
        "raw_url": format!("/api/messages/{}/raw", m.item.id),
    });
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    v
}

fn thread_json(t: &ThreadView) -> Value {
    json!({
        "id": t.id.to_string(),
        "subject": t.subject_norm,
        "messages": t.messages.iter().map(item_json).collect::<Vec<_>>(),
    })
}

// ------------------------------------------------------------ handlers ----

async fn domains(env: &Env) -> std::result::Result<Response, ApiError> {
    let db = env.d1("DB")?;
    let list = D1Index { db: &db }.domains().await?;
    json_ok(&json!({ "items": list.iter().map(domain_json).collect::<Vec<_>>() }))
}

async fn messages(req: &Request, env: &Env) -> std::result::Result<Response, ApiError> {
    let url = req.url()?;
    let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    let q = ListQuery::from_params(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;
    let db = env.d1("DB")?;
    let page = list_page(&D1Index { db: &db }, &q).await?;
    json_ok(&json!({
        "items": page.items.iter().map(item_json).collect::<Vec<_>>(),
        "next_cursor": page.next_cursor,
    }))
}

async fn message(
    req: &Request,
    env: &Env,
    id: Option<&String>,
) -> std::result::Result<Response, ApiError> {
    let id: MessageId = parse_id(id, MessageId::parse, "mensagem")?;
    let remote_images = match req
        .url()?
        .query_pairs()
        .find(|(k, _)| k == "remote_images")
        .map(|(_, v)| v.into_owned())
    {
        None => false,
        Some(v) if v == "0" => false,
        Some(v) if v == "1" => true,
        Some(_) => return Err(ApiError::bad_request("remote_images: 0 ou 1".into())),
    };
    let db = env.d1("DB")?;
    let m = D1Index { db: &db }
        .message(id)
        .await?
        .ok_or_else(|| ApiError::not_found("mensagem"))?;
    let html = match &m.html_key {
        Some(key) => {
            let bucket = env.bucket("MAIL")?;
            R2Blobs { bucket: &bucket }
                .get(key)
                .await?
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .map(|h| {
                    if remote_images {
                        restore_remote_images(&h)
                    } else {
                        h
                    }
                })
        }
        None => None,
    };
    json_ok(&detail_json(&m, html))
}

async fn thread(env: &Env, id: Option<&String>) -> std::result::Result<Response, ApiError> {
    let id: ThreadId = parse_id(id, ThreadId::parse, "thread")?;
    let db = env.d1("DB")?;
    let t = D1Index { db: &db }
        .thread(id)
        .await?
        .ok_or_else(|| ApiError::not_found("thread"))?;
    json_ok(&thread_json(&t))
}

/// Conteúdo de terceiros servido na mesma origem da API: nunca pode virar
/// documento ativo. `nosniff` + CSP `sandbox` em tudo; fora imagens simples,
/// download forçado.
fn untrusted_headers(r: &mut Response, content_type: &str, disposition: &str) -> Result<()> {
    let h = r.headers_mut();
    h.set("Content-Type", content_type)?;
    h.set("Content-Disposition", disposition)?;
    h.set("X-Content-Type-Options", "nosniff")?;
    h.set(
        "Content-Security-Policy",
        "default-src 'none'; img-src 'self'; style-src 'unsafe-inline'; sandbox",
    )?;
    h.set("Cache-Control", "private, max-age=86400, immutable")?;
    Ok(())
}

/// Corpo do R2 direto para a resposta, sem passar pelo wasm.
async fn stream_object(env: &Env, key: &str) -> std::result::Result<Response, ApiError> {
    let bucket = env.bucket("MAIL")?;
    let obj = bucket
        .get(key)
        .execute()
        .await?
        .ok_or_else(|| ApiError::not_found("arquivo"))?;
    let body = obj
        .body()
        .ok_or_else(|| ApiError::not_found("arquivo"))?
        .response_body()?;
    Ok(Response::from_body(body)?)
}

async fn attachment(env: &Env, id: Option<&String>) -> std::result::Result<Response, ApiError> {
    let id: AttachmentId = parse_id(id, AttachmentId::parse, "anexo")?;
    let db = env.d1("DB")?;
    let a = D1Index { db: &db }
        .attachment(id)
        .await?
        .ok_or_else(|| ApiError::not_found("anexo"))?;
    let inline = is_inline_safe(&a.content_type);
    let content_type = if inline {
        a.content_type.as_str()
    } else {
        "application/octet-stream"
    };
    let mut r = stream_object(env, &a.r2_key).await?;
    untrusted_headers(
        &mut r,
        content_type,
        &content_disposition(inline, a.filename.as_deref()),
    )?;
    Ok(r)
}

/// `.eml` original (sempre download). Essencial para mensagens em modo leve.
async fn raw(env: &Env, id: Option<&String>) -> std::result::Result<Response, ApiError> {
    let id: MessageId = parse_id(id, MessageId::parse, "mensagem")?;
    let db = env.d1("DB")?;
    let m = D1Index { db: &db }
        .message(id)
        .await?
        .ok_or_else(|| ApiError::not_found("mensagem"))?;
    let mut r = stream_object(env, &m.raw_key).await?;
    let name = format!("{}.eml", m.item.id);
    untrusted_headers(
        &mut r,
        "message/rfc822",
        &content_disposition(false, Some(&name)),
    )?;
    Ok(r)
}
