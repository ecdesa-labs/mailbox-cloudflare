//! Lado de leitura da API (docs seção 7): consultas, paginação por cursor e
//! validação de parâmetros. Sem HTTP nem JSON aqui; o `worker` serializa.

use std::fmt;

use mailbox_domain::{AttachmentId, DomainName, Folder, MessageId, Processing, ThreadId, UnixMs};

use crate::ports::PortError;

pub const DEFAULT_LIMIT: u32 = 50;
pub const MAX_LIMIT: u32 = 100;

/// Posição na lista (ordem `received_at DESC, id DESC`). Opaco para o
/// cliente: `<received_at>.<id>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub received_at: i64,
    pub id: MessageId,
}

impl Cursor {
    pub fn encode(&self) -> String {
        format!("{}.{}", self.received_at, self.id)
    }

    pub fn decode(s: &str) -> Option<Self> {
        let (at, id) = s.split_once('.')?;
        Some(Self {
            received_at: at.parse().ok().filter(|v: &i64| *v >= 0)?,
            id: MessageId::parse(id).ok()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListQuery {
    pub folder: Folder,
    /// `None` = caixa unificada.
    pub domain: Option<DomainName>,
    pub limit: u32,
    pub after: Option<Cursor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    Invalid {
        param: &'static str,
        message: String,
    },
    /// Existe no contrato, mas entra numa fase futura.
    NotImplemented { param: &'static str, phase: u8 },
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid { param, message } => write!(f, "{param}: {message}"),
            Self::NotImplemented { param, phase } => {
                write!(f, "{param}: disponível na fase {phase}")
            }
        }
    }
}

impl ListQuery {
    /// `GET /api/messages?folder=&domain=&q=&cursor=&limit=`. Parâmetro
    /// desconhecido é ignorado; valor inválido é erro (nada de adivinhar).
    pub fn from_params<'a>(
        params: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, QueryError> {
        let mut q = Self {
            folder: Folder::Inbox,
            domain: None,
            limit: DEFAULT_LIMIT,
            after: None,
        };
        let invalid = |param, message: &str| QueryError::Invalid {
            param,
            message: message.to_owned(),
        };
        for (k, v) in params {
            match k {
                "folder" => {
                    q.folder = v
                        .parse()
                        .map_err(|_| invalid("folder", "pasta desconhecida"))?
                }
                "domain" if v.is_empty() => q.domain = None,
                "domain" => {
                    q.domain = Some(
                        DomainName::parse(v).map_err(|_| invalid("domain", "domínio inválido"))?,
                    )
                }
                "limit" => {
                    q.limit = v
                        .parse()
                        .ok()
                        .filter(|n| (1..=MAX_LIMIT).contains(n))
                        .ok_or_else(|| invalid("limit", &format!("entre 1 e {MAX_LIMIT}")))?
                }
                "cursor" if v.is_empty() => q.after = None,
                "cursor" => {
                    q.after = Some(
                        Cursor::decode(v).ok_or_else(|| invalid("cursor", "cursor inválido"))?,
                    )
                }
                "q" if !v.trim().is_empty() => {
                    return Err(QueryError::NotImplemented {
                        param: "q",
                        phase: 7,
                    })
                }
                _ => {}
            }
        }
        Ok(q)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub name: Option<String>,
    pub addr: String,
}

/// Item da lista (`GET /api/messages`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    pub id: MessageId,
    pub thread_id: Option<ThreadId>,
    pub domain: String,
    /// Endereço pelo qual entrou.
    pub identity: Option<String>,
    pub from: Option<Person>,
    pub subject: Option<String>,
    pub snippet: Option<String>,
    pub received_at: UnixMs,
    pub is_read: bool,
    pub is_starred: bool,
    pub has_attachments: bool,
    pub folder: Folder,
    /// `partial` = modo leve: o front oferece o `.eml` para download.
    pub processing: Processing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub items: Vec<ListItem>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentMeta {
    pub id: AttachmentId,
    pub message_id: MessageId,
    pub filename: Option<String>,
    pub content_type: String,
    pub size_bytes: u64,
    pub content_id: Option<String>,
    pub is_inline: bool,
    pub r2_key: String,
}

/// Detalhe (`GET /api/messages/:id`). O HTML vem do R2 à parte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageDetail {
    pub item: ListItem,
    pub to: Vec<Person>,
    pub cc: Vec<Person>,
    pub date_hdr: Option<UnixMs>,
    pub message_id_hdr: Option<String>,
    pub envelope_to: String,
    pub size_bytes: u64,
    pub text_body: Option<String>,
    pub html_key: Option<String>,
    pub raw_key: String,
    pub spf: Option<String>,
    pub dkim: Option<String>,
    pub dmarc: Option<String>,
    pub attachments: Vec<AttachmentMeta>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentitySummary {
    pub id: String,
    pub address: String,
    pub label: Option<String>,
    pub is_default_from: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainSummary {
    pub id: String,
    pub name: String,
    pub label: String,
    pub color: String,
    /// Não lidos na caixa de entrada.
    pub unread: u64,
    pub identities: Vec<IdentitySummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadView {
    pub id: ThreadId,
    pub subject_norm: Option<String>,
    /// Mais antiga primeiro.
    pub messages: Vec<ListItem>,
}

/// Consultas de leitura (D1).
#[allow(async_fn_in_trait)]
pub trait ReadModel {
    /// Até `limit` itens depois de `after`, na ordem da lista.
    async fn list(&self, q: &ListQuery, limit: u32) -> Result<Vec<ListItem>, PortError>;
    async fn message(&self, id: MessageId) -> Result<Option<MessageDetail>, PortError>;
    async fn thread(&self, id: ThreadId) -> Result<Option<ThreadView>, PortError>;
    async fn attachment(&self, id: AttachmentId) -> Result<Option<AttachmentMeta>, PortError>;
    async fn domains(&self) -> Result<Vec<DomainSummary>, PortError>;
}

/// Uma página: pede um item a mais para saber se há próxima.
pub async fn list_page<R: ReadModel>(read: &R, q: &ListQuery) -> Result<Page, PortError> {
    let mut items = read.list(q, q.limit + 1).await?;
    let next_cursor = if items.len() > q.limit as usize {
        items.truncate(q.limit as usize);
        items.last().map(|i| {
            Cursor {
                received_at: i.received_at.as_i64(),
                id: i.id,
            }
            .encode()
        })
    } else {
        None
    };
    Ok(Page { items, next_cursor })
}

/// Tipos de imagem que o navegador pode exibir direto. Qualquer outro
/// anexo é servido como download (HTML ou SVG na mesma origem da API
/// executaria script com a sessão do dono).
pub fn is_inline_safe(content_type: &str) -> bool {
    let base = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    matches!(
        base.as_str(),
        "image/png" | "image/jpeg" | "image/gif" | "image/webp"
    )
}

/// `Content-Disposition` com nome seguro: ASCII sem aspas nem controle no
/// `filename`, nome original em `filename*` (RFC 6266/5987).
pub fn content_disposition(inline: bool, filename: Option<&str>) -> String {
    let kind = if inline { "inline" } else { "attachment" };
    let Some(name) = filename.map(str::trim).filter(|n| !n.is_empty()) else {
        return kind.to_owned();
    };
    let ascii: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    format!("{kind}; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

#[cfg(test)]
mod tests;
