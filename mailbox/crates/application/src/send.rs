//! Envio e resposta (docs seção 6.4, Fase 8). `POST /api/messages`.
//!
//! O `.eml` que guardamos é montado aqui; o Email Sending monta e assina o
//! que de fato sai (o `Message-ID` e o `Date` dele são do serviço). A cópia
//! vai para o R2 como qualquer email recebido e passa pelo mesmo
//! processamento, marcada como enviada: thread, busca, reprocessamento e
//! reconciliação valem para os dois lados.

use std::fmt;

use mailbox_domain::{EmailAddress, IdentityId, Mailbox, MessageId, RawKey, ThreadId, UnixMs};
use mailbox_mime::{OutAddress, Outgoing};
use serde_json::{Map, Value};

use crate::ports::{
    BlobStore, Clock, IdGenerator, IngestJob, IngestQueue, MailIndex, Mailer, MailerError,
    OutgoingMail, PortError, RawWriter, SendIndex,
};
use crate::process::ProcessMessage;
use crate::read::QueryError;

/// Limite do Email Sending: `to` + `cc` + `bcc` por mensagem.
pub const MAX_RECIPIENTS: usize = 50;
/// RFC 5322 (e limite do Email Sending).
pub const MAX_SUBJECT_CHARS: usize = 998;
/// Limite do Email Sending é 5 MiB por mensagem montada; folga para os
/// cabeçalhos que o serviço adiciona.
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024 + 512 * 1024;
/// Limite do Email Sending por valor de cabeçalho.
const MAX_HEADER_BYTES: usize = 2048;

/// `{ in_reply_to?, from_identity_id, to[], cc?, subject, text, html? }`.
/// Destinatário: `"a@b.com"` ou `{ "name"?, "addr" }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendRequest {
    pub in_reply_to: Option<MessageId>,
    pub from_identity_id: IdentityId,
    pub to: Vec<Mailbox>,
    pub cc: Vec<Mailbox>,
    pub subject: String,
    pub text: String,
    pub html: Option<String>,
}

fn invalid(param: &'static str, message: impl Into<String>) -> QueryError {
    QueryError::Invalid {
        param,
        message: message.into(),
    }
}

/// Texto que vai num cabeçalho: sem quebras de linha nem controles.
fn header_text(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}

fn recipients(o: &Map<String, Value>, key: &'static str) -> Result<Vec<Mailbox>, QueryError> {
    let items = match o.get(key) {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(a)) => a,
        Some(_) => return Err(invalid(key, "esperada uma lista")),
    };
    let mut out: Vec<Mailbox> = Vec::with_capacity(items.len());
    for item in items {
        let (name, addr) = match item {
            Value::String(s) => (None, s.as_str()),
            Value::Object(p) => {
                if let Some(k) = p.keys().find(|k| *k != "name" && *k != "addr") {
                    return Err(invalid(key, format!("campo desconhecido: {k}")));
                }
                let name = match p.get("name") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(n)) => Some(header_text(n)).filter(|n| !n.is_empty()),
                    Some(_) => return Err(invalid(key, "name: esperado texto")),
                };
                let addr = p
                    .get("addr")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid(key, "addr obrigatório"))?;
                (name, addr)
            }
            _ => return Err(invalid(key, "esperado endereço ou {name, addr}")),
        };
        let addr = EmailAddress::parse(addr.trim())
            .map_err(|_| invalid(key, format!("endereço inválido: {addr}")))?;
        if !out.iter().any(|m| m.addr == addr) {
            out.push(Mailbox { name, addr });
        }
    }
    Ok(out)
}

impl SendRequest {
    pub fn from_json(v: &Value) -> Result<Self, QueryError> {
        let o = v
            .as_object()
            .ok_or_else(|| invalid("body", "esperado um objeto JSON"))?;
        const KEYS: [&str; 7] = [
            "in_reply_to",
            "from_identity_id",
            "to",
            "cc",
            "subject",
            "text",
            "html",
        ];
        if let Some(k) = o.keys().find(|k| !KEYS.contains(&k.as_str())) {
            return Err(invalid("body", format!("campo desconhecido: {k}")));
        }

        let in_reply_to = match o.get("in_reply_to") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) => Some(
                MessageId::parse(s)
                    .map_err(|_| invalid("in_reply_to", "id de mensagem inválido"))?,
            ),
            Some(_) => return Err(invalid("in_reply_to", "esperado texto")),
        };
        let from_identity_id = o
            .get("from_identity_id")
            .and_then(Value::as_str)
            .and_then(|s| IdentityId::parse(s).ok())
            .ok_or_else(|| invalid("from_identity_id", "id de identidade obrigatório"))?;

        let to = recipients(o, "to")?;
        let mut cc = recipients(o, "cc")?;
        cc.retain(|c| !to.iter().any(|t| t.addr == c.addr));
        if to.is_empty() {
            return Err(invalid("to", "ao menos um destinatário"));
        }
        if to.len() + cc.len() > MAX_RECIPIENTS {
            return Err(invalid(
                "to",
                format!("no máximo {MAX_RECIPIENTS} destinatários (to + cc)"),
            ));
        }

        let subject = match o.get("subject") {
            Some(Value::String(s)) => header_text(s),
            _ => return Err(invalid("subject", "esperado texto (pode ser vazio)")),
        };
        if subject.chars().count() > MAX_SUBJECT_CHARS {
            return Err(invalid(
                "subject",
                format!("no máximo {MAX_SUBJECT_CHARS} caracteres"),
            ));
        }
        let text = match o.get("text") {
            Some(Value::String(s)) => s.clone(),
            _ => return Err(invalid("text", "esperado texto")),
        };
        let html = match o.get("html") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if s.trim().is_empty() => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(_) => return Err(invalid("html", "esperado texto")),
        };
        if text.trim().is_empty() && html.is_none() {
            return Err(invalid("text", "mensagem vazia"));
        }

        Ok(Self {
            in_reply_to,
            from_identity_id,
            to,
            cc,
            subject,
            text,
            html,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    Invalid(QueryError),
    NotFound(&'static str),
    /// Recusada pelo serviço de envio (não saiu).
    Rejected {
        code: String,
        message: String,
    },
    RateLimited(String),
    /// Não se sabe se saiu (o serviço falhou sem código) ou o índice falhou
    /// antes do envio (não saiu).
    Unavailable(String),
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(e) => write!(f, "{e}"),
            Self::NotFound(what) => write!(f, "{what} não encontrada"),
            Self::Rejected { code, message } => write!(f, "{code}: {message}"),
            Self::RateLimited(m) | Self::Unavailable(m) => f.write_str(m),
        }
    }
}

impl From<PortError> for SendError {
    fn from(e: PortError) -> Self {
        Self::Unavailable(e.0)
    }
}

impl From<MailerError> for SendError {
    fn from(e: MailerError) -> Self {
        match e {
            MailerError::Rejected { code, message } => Self::Rejected { code, message },
            MailerError::RateLimited(m) => Self::RateLimited(m),
            MailerError::Unavailable(m) => Self::Unavailable(m),
        }
    }
}

/// Onde a cópia do enviado foi parar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stored {
    /// No R2 e no índice: já aparece na lista e na thread.
    Indexed,
    /// No R2; o índice falhou e o job foi para a fila (ou fica para a
    /// reconciliação).
    Pending(String),
    /// Saiu, mas a cópia não foi gravada. Só no log.
    Lost(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendOutcome {
    pub id: MessageId,
    pub thread_id: Option<ThreadId>,
    /// Id que o Email Sending deu à mensagem.
    pub provider_id: String,
    pub stored: Stored,
}

/// `References` da resposta: as da original + o `Message-ID` dela. Se não
/// couber no limite do cabeçalho, fica a primeira (raiz da conversa) e as
/// mais recentes, como a RFC 5322 sugere.
pub fn reply_references(original: &[String], message_id: Option<&str>) -> Vec<String> {
    let mut all: Vec<String> = original.to_vec();
    if let Some(m) = message_id {
        if !all.iter().any(|r| r == m) {
            all.push(m.to_owned());
        }
    }
    let size = |refs: &[String]| refs.iter().map(|r| r.len() + 3).sum::<usize>();
    while all.len() > 2 && size(&all) > MAX_HEADER_BYTES {
        all.remove(1);
    }
    all
}

fn out_address(m: &Mailbox) -> OutAddress {
    OutAddress {
        name: m.name.clone(),
        address: m.addr.to_string(),
    }
}

/// O id que o serviço devolve vira o `Message-ID` da cópia quando tem cara
/// de um; senão, um nosso no domínio do remetente.
fn copy_message_id(provider_id: &str, id: MessageId, sender: &EmailAddress) -> String {
    let p = provider_id
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>');
    if p.contains('@') && !p.contains(char::is_whitespace) {
        p.to_owned()
    } else {
        format!("{id}@{}", sender.domain_str())
    }
}

pub struct SendMessage<'a, M, S, W, Q, B, I, C, G> {
    pub mailer: &'a M,
    pub index: &'a S,
    pub raw: &'a W,
    pub queue: &'a Q,
    /// Indexa a cópia na hora; se falhar, vai para a fila.
    pub process: &'a ProcessMessage<'a, B, I, C, G>,
}

impl<M, S, W, Q, B, I, C, G> SendMessage<'_, M, S, W, Q, B, I, C, G>
where
    M: Mailer,
    S: SendIndex,
    W: RawWriter,
    Q: IngestQueue,
    B: BlobStore,
    I: MailIndex,
    C: Clock,
    G: IdGenerator,
{
    pub async fn run(&self, req: &SendRequest) -> Result<SendOutcome, SendError> {
        let sender = self
            .index
            .sender(req.from_identity_id)
            .await?
            .ok_or(SendError::NotFound("identidade"))?;

        let (in_reply_to, references, reply_thread) = match req.in_reply_to {
            None => (None, Vec::new(), None),
            Some(id) => {
                let t = self
                    .index
                    .reply_target(id)
                    .await?
                    .ok_or(SendError::NotFound("mensagem original"))?;
                // Docs seção 6.4: uma resposta nunca sai de outro domínio.
                if t.domain != sender.domain {
                    return Err(SendError::Invalid(invalid(
                        "from_identity_id",
                        format!("a resposta sai pelo domínio da original ({})", t.domain),
                    )));
                }
                let refs = reply_references(&t.references, t.message_id_hdr.as_deref());
                (t.message_id_hdr, refs, t.thread_id)
            }
        };

        let from = Mailbox {
            name: sender.name.clone(),
            addr: sender.address.clone(),
        };
        let mail = OutgoingMail {
            from,
            to: req.to.clone(),
            cc: req.cc.clone(),
            subject: req.subject.clone(),
            text: req.text.clone(),
            html: req.html.clone(),
            in_reply_to,
            references,
        };

        let clock = self.process.clock;
        let now = clock.now();
        let id = self.process.ids.message_id(now);
        let raw_key = RawKey::new(&sender.domain, id);
        // Mede antes de enviar: o que passa daqui o serviço recusaria.
        let mut outgoing = to_outgoing(&mail, id, now, &format!("{id}@{}", sender.domain));
        if mailbox_mime::build(&outgoing).len() > MAX_MESSAGE_BYTES {
            return Err(SendError::Invalid(invalid(
                "text",
                "mensagem grande demais (limite do envio: 5 MiB)",
            )));
        }

        let provider_id = self.mailer.send(&mail).await?;

        // Daqui em diante a mensagem já saiu: nada pode virar erro.
        outgoing.message_id = copy_message_id(&provider_id, id, &sender.address);
        let eml = mailbox_mime::build(&outgoing);
        let job = IngestJob {
            raw_key: raw_key.to_string(),
            domain: sender.domain.to_string(),
            envelope_from: sender.address.to_string(),
            // Pelo endereço do envelope o processamento acha a identidade.
            envelope_to: sender.address.to_string(),
            received_at: now.as_i64(),
            size: eml.len() as u64,
            sent: true,
        };
        let (stored, thread_id) = match self.raw.write(&job, &eml).await {
            Err(e) => (Stored::Lost(e.0), reply_thread),
            Ok(()) => match self.process.run(&job).await {
                Ok(_) => (Stored::Indexed, self.thread_of(id, reply_thread).await),
                Err(e) => {
                    let why = e.to_string();
                    match self.queue.enqueue(&job).await {
                        Ok(()) => (Stored::Pending(why), reply_thread),
                        // A reconciliação acha o `.eml` no R2.
                        Err(q) => (Stored::Pending(format!("{why}; fila: {q}")), reply_thread),
                    }
                }
            },
        };

        Ok(SendOutcome {
            id,
            thread_id,
            provider_id,
            stored,
        })
    }

    async fn thread_of(&self, id: MessageId, fallback: Option<ThreadId>) -> Option<ThreadId> {
        match self.process.index.existing(id).await {
            Ok(Some(e)) => e.thread_id.or(fallback),
            _ => fallback,
        }
    }
}

fn to_outgoing(mail: &OutgoingMail, id: MessageId, now: UnixMs, message_id: &str) -> Outgoing {
    Outgoing {
        message_id: message_id.to_owned(),
        date_ms: now.as_i64(),
        from: out_address(&mail.from),
        to: mail.to.iter().map(out_address).collect(),
        cc: mail.cc.iter().map(out_address).collect(),
        subject: mail.subject.clone(),
        in_reply_to: mail.in_reply_to.clone(),
        references: mail.references.clone(),
        text: mail.text.clone(),
        html: mail.html.clone(),
        boundary: format!("mbx_{id}"),
    }
}

#[cfg(test)]
mod tests;
