use std::borrow::Cow;
use std::fmt;

use mail_parser::{HeaderValue, Message, MessageParser, MimeHeaders, PartType};

/// Mensagem já decodificada. Os anexos apontam para os bytes originais
/// sempre que possível (`Cow::Borrowed`): só o que veio em base64 ou
/// quoted-printable é copiado ao decodificar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMessage<'a> {
    /// Sem `<>`. `None` quando o header falta (acontece; a idempotência
    /// do sistema é pelo `raw_key`, não por este campo).
    pub message_id: Option<String>,
    pub in_reply_to: Vec<String>,
    pub references: Vec<String>,
    pub from: Option<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub subject: Option<String>,
    /// Header `Date` em Unix ms (UTC). `None` se ausente ou ilegível.
    pub date_ms: Option<i64>,
    /// Corpo em texto puro. Se a mensagem só tem HTML, vem convertido dele.
    pub text: Option<String>,
    /// HTML **cru**, ainda não sanitizado. Só existe se houver parte HTML de
    /// verdade (não é gerado a partir do texto).
    pub html: Option<String>,
    pub attachments: Vec<Attachment<'a>>,
    /// Todos os headers `Authentication-Results`, na ordem do arquivo (o
    /// primeiro é o mais recente). Qualquer remetente pode forjar um; quem
    /// decide em qual confiar é quem chama, pelo `authserv_id`.
    pub auth_results: Vec<AuthResultsHeader>,
    /// Alguma parte tinha charset ou codificação problemática; o texto
    /// pode ter caracteres de substituição.
    pub has_encoding_problem: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub name: Option<String>,
    /// Como veio no header, sem validar. Validar é papel do `domain`.
    pub address: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment<'a> {
    /// Posição entre os anexos (0, 1, 2...). Vira a chave no R2.
    pub ordinal: u32,
    pub filename: Option<String>,
    /// `tipo/subtipo` em minúsculas. Padrão `application/octet-stream`.
    pub content_type: String,
    /// Sem `<>`. Liga `<img src="cid:...">` a este anexo.
    pub content_id: Option<String>,
    pub is_inline: bool,
    pub data: Cow<'a, [u8]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuthResultsHeader {
    /// Quem fez a verificação (primeiro token do header), em minúsculas.
    pub authserv_id: String,
    pub spf: Option<String>,
    pub dkim: Option<String>,
    pub dmarc: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MimeError {
    /// Nada que se pareça com uma mensagem (vazio, só lixo).
    Unparseable,
}

impl fmt::Display for MimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unparseable => f.write_str("mensagem MIME ilegível"),
        }
    }
}

impl std::error::Error for MimeError {}

/// Lê um `.eml`. Tolerante: header quebrado vira `None`, charset ruim vira
/// texto com substituição. Só falha se não houver mensagem alguma.
pub fn parse(raw: &[u8]) -> Result<ParsedMessage<'_>, MimeError> {
    if raw.iter().all(u8::is_ascii_whitespace) {
        return Err(MimeError::Unparseable);
    }
    let mut msg = MessageParser::default()
        .parse(raw)
        .ok_or(MimeError::Unparseable)?;

    let text = join_bodies(&msg, &msg.text_body, Body::Text);
    let html = msg
        .html_body
        .iter()
        .any(|&i| {
            matches!(
                msg.parts.get(i as usize).map(|p| &p.body),
                Some(PartType::Html(_))
            )
        })
        .then(|| join_bodies(&msg, &msg.html_body, Body::Html))
        .flatten();

    let mut parsed = ParsedMessage {
        message_id: msg.message_id().map(clean_id).filter(|s| !s.is_empty()),
        in_reply_to: ids(msg.in_reply_to()),
        references: ids(msg.references()),
        from: msg
            .from()
            .and_then(|a| addresses(Some(a)).into_iter().next()),
        to: addresses(msg.to()),
        cc: addresses(msg.cc()),
        reply_to: addresses(msg.reply_to()),
        subject: msg.subject().map(str::to_owned),
        date_ms: msg
            .date()
            .map(|d| d.to_timestamp())
            .filter(|&s| s > 0)
            .and_then(|s| s.checked_mul(1000)),
        text,
        html,
        attachments: Vec::new(),
        auth_results: msg
            .headers()
            .iter()
            .filter(|h| h.name().eq_ignore_ascii_case("authentication-results"))
            .filter_map(|h| h.value().as_text().map(parse_auth_results))
            .collect(),
        has_encoding_problem: false,
    };
    // O mail-parser não marca UTF-8 inválido; o caractere de substituição denuncia.
    parsed.has_encoding_problem = msg.parts.iter().any(|p| p.is_encoding_problem)
        || [&parsed.text, &parsed.html, &parsed.subject]
            .into_iter()
            .flatten()
            .any(|s| s.contains('\u{FFFD}'));

    // Toma posse das partes para mover os bytes dos anexos sem copiar.
    let attachment_ids = std::mem::take(&mut msg.attachments);
    let mut parts: Vec<Option<_>> = std::mem::take(&mut msg.parts)
        .into_iter()
        .map(Some)
        .collect();
    for (ordinal, id) in attachment_ids.into_iter().enumerate() {
        let Some(part) = parts.get_mut(id as usize).and_then(Option::take) else {
            continue;
        };
        let content_type = part
            .content_type()
            .map(|ct| match ct.subtype() {
                Some(sub) => format!("{}/{}", ct.ctype(), sub),
                None => ct.ctype().to_owned(),
            })
            .map(|s| s.to_ascii_lowercase());
        let filename = part.attachment_name().map(str::to_owned);
        let content_id = part.content_id().map(clean_id).filter(|s| !s.is_empty());
        let disposition_inline = part.content_disposition().is_some_and(|cd| cd.is_inline());
        let is_inline = disposition_inline || matches!(part.body, PartType::InlineBinary(_));

        let (data, default_type): (Cow<'_, [u8]>, &str) = match part.body {
            PartType::Binary(b) | PartType::InlineBinary(b) => (b, "application/octet-stream"),
            PartType::Text(t) => (cow_str_to_bytes(t), "text/plain"),
            PartType::Html(t) => (cow_str_to_bytes(t), "text/html"),
            PartType::Message(m) => (nested_raw(m), "message/rfc822"),
            PartType::Multipart(_) => continue,
        };
        let content_type = content_type.unwrap_or_else(|| default_type.to_owned());
        let filename = filename
            .or_else(|| (content_type == "message/rfc822").then(|| "mensagem.eml".to_owned()));

        parsed.attachments.push(Attachment {
            ordinal: ordinal as u32,
            filename,
            content_type,
            content_id,
            is_inline,
            data,
        });
    }

    Ok(parsed)
}

#[derive(Clone, Copy)]
enum Body {
    Text,
    Html,
}

/// Junta as partes de corpo (mensagens multipart/mixed podem ter vários
/// pedaços de texto intercalados com imagens).
fn join_bodies(msg: &Message<'_>, ids: &[u32], kind: Body) -> Option<String> {
    let pieces: Vec<String> = ids
        .iter()
        .filter_map(|&i| msg.parts.get(i as usize))
        .filter_map(|p| match (&p.body, kind) {
            (PartType::Text(t), Body::Text) => Some(t.to_string()),
            (PartType::Html(h), Body::Text) => Some(mail_parser::decoders::html::html_to_text(h)),
            (PartType::Html(h), Body::Html) => Some(h.to_string()),
            (PartType::Text(t), Body::Html) => Some(mail_parser::decoders::html::text_to_html(t)),
            _ => None,
        })
        .collect();
    let sep = match kind {
        Body::Text => "\n",
        Body::Html => "\n<hr>\n",
    };
    let joined = pieces.join(sep);
    (!joined.trim().is_empty()).then_some(joined)
}

/// Bytes só da mensagem aninhada. O `raw_message` dela aponta para o buffer
/// do email externo inteiro; o trecho certo é o da primeira parte (é o que
/// `Message::raw_message()` faz, mas sem perder o empréstimo).
fn nested_raw(m: Message<'_>) -> Cow<'_, [u8]> {
    let range = m
        .parts
        .first()
        .map(|p| p.offset_header as usize..p.offset_end as usize)
        .unwrap_or(0..0);
    match m.raw_message {
        Cow::Borrowed(b) => Cow::Borrowed(b.get(range).unwrap_or_default()),
        Cow::Owned(v) => Cow::Owned(v.get(range).unwrap_or_default().to_vec()),
    }
}

fn cow_str_to_bytes(s: Cow<'_, str>) -> Cow<'_, [u8]> {
    match s {
        Cow::Borrowed(b) => Cow::Borrowed(b.as_bytes()),
        Cow::Owned(o) => Cow::Owned(o.into_bytes()),
    }
}

fn clean_id(s: &str) -> String {
    s.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim()
        .to_owned()
}

fn ids(v: &HeaderValue<'_>) -> Vec<String> {
    let raw: Vec<&str> = match v {
        HeaderValue::Text(t) => vec![t.as_ref()],
        HeaderValue::TextList(l) => l.iter().map(|s| s.as_ref()).collect(),
        _ => vec![],
    };
    raw.into_iter()
        .map(clean_id)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Achata listas e grupos. Endereço vazio (ex.: `undisclosed-recipients:;`) é descartado.
fn addresses(a: Option<&mail_parser::Address<'_>>) -> Vec<Address> {
    let Some(a) = a else { return vec![] };
    a.iter()
        .filter_map(|addr| {
            let address = addr.address()?.trim();
            (!address.is_empty()).then(|| Address {
                name: addr
                    .name()
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .map(str::to_owned),
                address: address.to_owned(),
            })
        })
        .collect()
}

/// RFC 8601, leitura tolerante:
/// `mx.cloudflare.net; spf=pass smtp.mailfrom=x; dkim=pass header.d=y; dmarc=pass`.
fn parse_auth_results(value: &str) -> AuthResultsHeader {
    let mut sections = value.split(';');
    let authserv_id = sections
        .next()
        .and_then(|s| s.split_whitespace().next())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut out = AuthResultsHeader {
        authserv_id,
        ..Default::default()
    };
    for section in sections {
        let Some((method, rest)) = section.trim().split_once('=') else {
            continue;
        };
        let verdict = rest
            .split(|c: char| c.is_whitespace() || c == '(')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if verdict.is_empty() {
            continue;
        }
        // Só o primeiro resultado de cada método (pode haver várias assinaturas DKIM).
        let slot = match method.trim().to_ascii_lowercase().as_str() {
            "spf" => &mut out.spf,
            "dkim" => &mut out.dkim,
            "dmarc" => &mut out.dmarc,
            _ => continue,
        };
        slot.get_or_insert(verdict);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_results_tolerante() {
        let a = parse_auth_results(
            "mx.cloudflare.net; dkim=pass header.d=github.com header.s=pf2023; \
             dkim=fail header.d=x; spf=SoftFail (sender IP) smtp.mailfrom=x; dmarc=pass header.from=github.com",
        );
        assert_eq!(a.authserv_id, "mx.cloudflare.net");
        assert_eq!(a.dkim.as_deref(), Some("pass"));
        assert_eq!(a.spf.as_deref(), Some("softfail"));
        assert_eq!(a.dmarc.as_deref(), Some("pass"));
    }

    #[test]
    fn auth_results_vazio() {
        let a = parse_auth_results("mx.example; none");
        assert_eq!(a.authserv_id, "mx.example");
        assert_eq!((a.spf, a.dkim, a.dmarc), (None, None, None));
    }

    #[test]
    fn vazio_falha() {
        assert_eq!(parse(b""), Err(MimeError::Unparseable));
        assert_eq!(parse(b" \r\n "), Err(MimeError::Unparseable));
    }

    #[test]
    fn clean_id_tira_colchetes() {
        assert_eq!(clean_id(" <a@b> "), "a@b");
    }
}
