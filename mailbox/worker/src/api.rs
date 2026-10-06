//! API de leitura (docs seção 7). JSON, datas em Unix ms, cursor opaco.
//! Erros: `{ "error": { "code", "message" } }` com status coerente.

use mailbox_adapters_cf::CryptoIds;
use mailbox_adapters_cf::{CfMailer, D1Index, JwksVerifier, QueueIngest, R2Blobs, WorkerClock};
use mailbox_application::{
    apply_rule_patch, authenticate, content_disposition, is_inline_safe, list_page, rule_from_json,
    rule_json, AccessConfig, AttachmentMeta, AuthError, BlobStore, BulkPatch, Clock, DomainSummary,
    IdGenerator, Identity, ListItem, ListQuery, MailWriter, MessageDetail, MessagePatch, Person,
    ProcessMessage, QueryError, ReadModel, RuleStore, SendError, SendMessage, SendRequest, Stored,
    ThreadView,
};
use mailbox_domain::{AttachmentId, MessageId, RuleId, ThreadId};
use mailbox_mime::{restore_remote_images, REMOTE_SRC_ATTR};
use serde_json::{json, Value};
use worker::*;

/// Especificação da API (Fase 9). O `just contract` confere que ela bate
/// com as rotas abaixo e com o JSON que a API devolve.
const OPENAPI: &str = include_str!("../openapi.json");

/// `identity` já validada em `authorize` (ou `None` só no `/api/ping`).
pub fn router(identity: Option<Identity>) -> Router<'static, Option<Identity>> {
    Router::with_data(identity)
        .get("/api/ping", |_, _| Response::ok("pong"))
        .get("/api/openapi.json", |_, _| {
            let mut r = Response::ok(OPENAPI)?;
            r.headers_mut()
                .set("Content-Type", "application/json; charset=utf-8")?;
            Ok(r)
        })
        .get("/api/me", |_, ctx| match &ctx.data {
            Some(i) => Response::from_json(&json!({ "email": i.email })),
            None => error_response(401, "unauthorized", "sem identidade"),
        })
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
        .patch_async("/api/messages/:id", |req, ctx| async move {
            handle(patch_message(req, &ctx.env, ctx.param("id")).await)
        })
        .post_async("/api/messages/bulk", |req, ctx| async move {
            handle(bulk(req, &ctx.env).await)
        })
        .post_async("/api/messages", |req, ctx| async move {
            handle(send_message(req, &ctx.env).await)
        })
        .get_async("/api/rules", |_, ctx| async move {
            handle(list_rules(&ctx.env).await)
        })
        .post_async("/api/rules", |req, ctx| async move {
            handle(create_rule(req, &ctx.env).await)
        })
        .patch_async("/api/rules/:id", |req, ctx| async move {
            handle(update_rule(req, &ctx.env, ctx.param("id")).await)
        })
        .delete_async("/api/rules/:id", |_, ctx| async move {
            handle(delete_rule(&ctx.env, ctx.param("id")).await)
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

impl From<SendError> for ApiError {
    fn from(e: SendError) -> Self {
        let (status, code) = match &e {
            SendError::Invalid(q) => return q.clone().into(),
            SendError::NotFound(_) => (404, "not_found"),
            SendError::Rejected { .. } => (422, "rejected"),
            SendError::RateLimited(_) => (429, "rate_limited"),
            SendError::Unavailable(_) => (502, "unavailable"),
        };
        Self {
            status,
            code,
            message: e.to_string(),
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
                // O detalhe do erro só existe aqui; a métrica da rota sai no `fetch`.
                crate::obs::log(
                    crate::obs::Level::Error,
                    "api_error",
                    &json!({ "status": e.status, "error": e.message }),
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

// ---------------------------------------------------------------- auth ----

/// Docs seção 6.5. Sem configuração do Access, nega tudo (falha fechada).
/// O bypass só vale com `MAILBOX_AUTH_BYPASS=1` **e** host local: em
/// produção a variável sozinha não abre nada.
pub async fn authorize(req: &Request, env: &Env) -> std::result::Result<Identity, Response> {
    let url = req.url().map_err(|_| unauthorized("url inválida"))?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    let var = |name: &str| env.var(name).map(|v| v.to_string()).unwrap_or_default();
    if local && var("MAILBOX_AUTH_BYPASS") == "1" {
        return Ok(Identity {
            email: "dev@localhost".into(),
        });
    }

    let Some(cfg) = AccessConfig::new(
        &var("MAILBOX_ACCESS_TEAM"),
        &var("MAILBOX_ACCESS_AUD"),
        &var("MAILBOX_ALLOWED_EMAILS"),
    ) else {
        crate::obs::event(
            env,
            crate::obs::Level::Error,
            "auth",
            json!({ "outcome": "not_configured", "error": "MAILBOX_ACCESS_TEAM/MAILBOX_ACCESS_AUD ausentes" }),
        );
        return Err(
            error_response(503, "unavailable", "autenticação não configurada")
                .unwrap_or_else(|_| Response::empty().unwrap().with_status(503)),
        );
    };
    let token = req.headers().get("Cf-Access-Jwt-Assertion").ok().flatten();
    let certs = cfg.certs_url();
    match authenticate(
        &cfg,
        &JwksVerifier { certs_url: &certs },
        token.as_deref(),
        WorkerClock.now(),
    )
    .await
    {
        Ok(i) => Ok(i),
        Err(AuthError::Keys(e)) => {
            crate::obs::event(
                env,
                crate::obs::Level::Error,
                "auth",
                json!({ "outcome": "keys_unavailable", "error": e.0 }),
            );
            Err(
                error_response(503, "unavailable", "chaves do Access indisponíveis")
                    .unwrap_or_else(|_| Response::empty().unwrap().with_status(503)),
            )
        }
        Err(e) => {
            crate::obs::event(
                env,
                crate::obs::Level::Warn,
                "auth",
                json!({ "outcome": "denied", "reason": e.to_string() }),
            );
            Err(unauthorized("acesso negado"))
        }
    }
}

/// Mensagem genérica: o motivo exato fica só no log.
fn unauthorized(message: &str) -> Response {
    error_response(401, "unauthorized", message)
        .unwrap_or_else(|_| Response::empty().unwrap().with_status(401))
}

// ------------------------------------------------------------- escrita ----

/// CSRF: o Access autentica por cookie, que o navegador manda junto com
/// requisições de outros sites. Mutação só com `Content-Type:
/// application/json` (formulário entre sites não consegue sem pré-checagem
/// de CORS, que recusamos) e, quando o navegador informa, mesma origem.
pub fn csrf_check(req: &Request) -> Option<Response> {
    let method = req.method();
    if matches!(method, Method::Get | Method::Head | Method::Options) {
        return None;
    }
    let header = |name: &str| req.headers().get(name).ok().flatten().unwrap_or_default();
    if header("Sec-Fetch-Site") == "cross-site" {
        return error_response(403, "forbidden", "requisição de outro site").ok();
    }
    let origin = header("Origin");
    if !origin.is_empty() {
        let own = req
            .url()
            .ok()
            .map(|u| u.origin().ascii_serialization())
            .unwrap_or_default();
        if origin != own {
            return error_response(403, "forbidden", "origem diferente da API").ok();
        }
    }
    let has_body = matches!(method, Method::Post | Method::Patch | Method::Put);
    let ct = header("Content-Type").to_ascii_lowercase();
    if has_body && !ct.starts_with("application/json") {
        return error_response(
            415,
            "unsupported_media_type",
            "use Content-Type: application/json",
        )
        .ok();
    }
    None
}

async fn json_body(req: &mut Request) -> std::result::Result<Value, ApiError> {
    req.json::<Value>()
        .await
        .map_err(|_| ApiError::bad_request("corpo não é JSON válido".into()))
}

async fn patch_message(
    mut req: Request,
    env: &Env,
    id: Option<&String>,
) -> std::result::Result<Response, ApiError> {
    let id: MessageId = parse_id(id, MessageId::parse, "mensagem")?;
    let patch = MessagePatch::from_json(&json_body(&mut req).await?)?;
    let db = env.d1("DB")?;
    let index = D1Index { db: &db };
    if !index.patch_message(id, &patch).await? {
        return Err(ApiError::not_found("mensagem"));
    }
    let m = index
        .message(id)
        .await?
        .ok_or_else(|| ApiError::not_found("mensagem"))?;
    json_ok(&item_json(&m.item))
}

async fn bulk(mut req: Request, env: &Env) -> std::result::Result<Response, ApiError> {
    let b = BulkPatch::from_json(&json_body(&mut req).await?)?;
    let db = env.d1("DB")?;
    let updated = D1Index { db: &db }.patch_many(&b.ids, &b.patch).await?;
    json_ok(&json!({ "updated": updated }))
}

async fn list_rules(env: &Env) -> std::result::Result<Response, ApiError> {
    let db = env.d1("DB")?;
    let rules = D1Index { db: &db }.list_rules().await?;
    json_ok(&json!({ "items": rules.iter().map(rule_json).collect::<Vec<_>>() }))
}

async fn create_rule(mut req: Request, env: &Env) -> std::result::Result<Response, ApiError> {
    let rule = rule_from_json(
        CryptoIds.rule_id(WorkerClock.now()),
        &json_body(&mut req).await?,
    )?;
    let db = env.d1("DB")?;
    D1Index { db: &db }.put_rule(&rule).await?;
    Ok(json_ok(&rule_json(&rule))?.with_status(201))
}

async fn update_rule(
    mut req: Request,
    env: &Env,
    id: Option<&String>,
) -> std::result::Result<Response, ApiError> {
    let id: RuleId = parse_id(id, RuleId::parse, "regra")?;
    let body = json_body(&mut req).await?;
    let db = env.d1("DB")?;
    let store = D1Index { db: &db };
    let current = store
        .get_rule(id)
        .await?
        .ok_or_else(|| ApiError::not_found("regra"))?;
    let rule = apply_rule_patch(current, &body)?;
    store.put_rule(&rule).await?;
    json_ok(&rule_json(&rule))
}

async fn delete_rule(env: &Env, id: Option<&String>) -> std::result::Result<Response, ApiError> {
    let id: RuleId = parse_id(id, RuleId::parse, "regra")?;
    let db = env.d1("DB")?;
    let deleted = D1Index { db: &db }.delete_rule(id).await?;
    if !deleted {
        return Err(ApiError::not_found("regra"));
    }
    Ok(Response::empty()?.with_status(204))
}

/// Envio e resposta (docs seção 6.4, Fase 8). Responde 201 assim que a
/// mensagem sai; o destino da cópia (índice, fila ou perdida) só vai para
/// o log, porque repetir o pedido mandaria o email de novo.
async fn send_message(mut req: Request, env: &Env) -> std::result::Result<Response, ApiError> {
    let request = SendRequest::from_json(&json_body(&mut req).await?)?;
    let binding = env.send_email("MAILER").map_err(|_| ApiError {
        status: 503,
        code: "unavailable",
        message: "envio não configurado (binding MAILER)".into(),
    })?;
    let (db, bucket, queue) = (env.d1("DB")?, env.bucket("MAIL")?, env.queue("INGEST")?);
    let index = D1Index { db: &db };
    let blobs = R2Blobs { bucket: &bucket };
    let process = ProcessMessage {
        blobs: &blobs,
        index: &index,
        clock: &WorkerClock,
        ids: &CryptoIds,
        full_max_bytes: crate::var_u64(env, "MAILBOX_FULL_MAX_BYTES", 512 * 1024),
        max_attachments: crate::var_u64(env, "MAILBOX_MAX_ATTACHMENTS", 30) as usize,
    };
    let uc = SendMessage {
        mailer: &CfMailer { binding: &binding },
        index: &index,
        raw: &blobs,
        queue: &QueueIngest { queue: &queue },
        process: &process,
    };
    let reply = request.in_reply_to.is_some();
    let out = match uc.run(&request).await {
        Ok(out) => out,
        Err(e) => {
            let level = match e {
                SendError::Invalid(_) | SendError::NotFound(_) => crate::obs::Level::Warn,
                _ => crate::obs::Level::Error,
            };
            // Sem endereços no log (docs seção 9 do log de decisões).
            crate::obs::event(
                env,
                level,
                "send",
                json!({ "outcome": "failed", "reply": reply, "error": e.to_string() }),
            );
            return Err(e.into());
        }
    };
    let (level, stored, detail) = match &out.stored {
        Stored::Indexed => (crate::obs::Level::Info, "indexed", None),
        Stored::Pending(why) => (crate::obs::Level::Warn, "pending", Some(why.as_str())),
        Stored::Lost(why) => (crate::obs::Level::Error, "lost", Some(why.as_str())),
    };
    crate::obs::event(
        env,
        level,
        "send",
        json!({
            "outcome": "sent", "reply": reply, "id": out.id.to_string(),
            "provider_id": out.provider_id, "stored": stored, "error": detail,
            "recipients": request.to.len() + request.cc.len(),
        }),
    );
    Ok(json_ok(&json!({
        "id": out.id.to_string(),
        "thread_id": out.thread_id.map(|t| t.to_string()),
        "stored": stored,
    }))?
    .with_status(201))
}
