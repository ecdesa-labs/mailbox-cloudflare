//! Mensagem enviada -> `.eml` (Fase 8). É a cópia que fica no R2 como fonte
//! da verdade; o Email Sending monta e assina a versão que de fato sai.
//!
//! No wasm, `SystemTime::now()` entra em pânico. O `mail-builder` só o usa
//! quando falta `Date`, `Message-ID` ou o boundary de um multipart, então
//! quem chama sempre passa os três.

use std::borrow::Cow;

use mail_builder::headers::address::Address as MbAddress;
use mail_builder::headers::content_type::ContentType;
use mail_builder::headers::date::Date;
use mail_builder::mime::{BodyPart, MimePart};
use mail_builder::MessageBuilder;

/// Um endereço com nome opcional, como vai no header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutAddress {
    pub name: Option<String>,
    pub address: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    /// Sem `<>`.
    pub message_id: String,
    pub date_ms: i64,
    pub from: OutAddress,
    pub to: Vec<OutAddress>,
    pub cc: Vec<OutAddress>,
    pub subject: String,
    /// Sem `<>`.
    pub in_reply_to: Option<String>,
    /// Sem `<>`, do mais antigo para o mais novo.
    pub references: Vec<String>,
    pub text: String,
    pub html: Option<String>,
    /// Só usado com HTML (multipart/alternative). Precisa ser único e não
    /// aparecer no corpo: derive do id da mensagem.
    pub boundary: String,
}

fn address(a: &OutAddress) -> MbAddress<'_> {
    MbAddress::new_address(a.name.as_deref(), a.address.as_str())
}

fn list(items: &[OutAddress]) -> MbAddress<'_> {
    MbAddress::new_list(items.iter().map(address).collect())
}

pub fn build(m: &Outgoing) -> Vec<u8> {
    let mut b = MessageBuilder::new()
        .message_id(m.message_id.as_str())
        .date(Date::new(m.date_ms.div_euclid(1000)))
        .from(address(&m.from))
        .to(list(&m.to))
        .subject(m.subject.as_str());
    if !m.cc.is_empty() {
        b = b.cc(list(&m.cc));
    }
    if let Some(r) = &m.in_reply_to {
        b = b.in_reply_to(r.as_str());
    }
    if !m.references.is_empty() {
        b = b.references(m.references.as_slice());
    }
    let text = MimePart::new("text/plain", BodyPart::Text(Cow::Borrowed(m.text.as_str())));
    b = match &m.html {
        None => b.body(text),
        Some(html) => b.body(MimePart::new(
            ContentType::new("multipart/alternative").attribute("boundary", m.boundary.as_str()),
            vec![
                text,
                MimePart::new("text/html", BodyPart::Text(Cow::Borrowed(html.as_str()))),
            ],
        )),
    };
    // Escrever num Vec não falha.
    b.write_to_vec().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Outgoing {
        Outgoing {
            message_id: "01M44NRD000000000000000001@acmeco.com.br".into(),
            date_ms: 1_791_158_400_000,
            from: OutAddress {
                name: Some("Contato Acme".into()),
                address: "contato@acmeco.com.br".into(),
            },
            to: vec![OutAddress {
                name: Some("João Ninguém".into()),
                address: "joao@exemplo.com".into(),
            }],
            cc: vec![],
            subject: "Re: Orçamento de outubro".into(),
            in_reply_to: Some("orig@exemplo.com".into()),
            references: vec!["raiz@exemplo.com".into(), "orig@exemplo.com".into()],
            text: "Olá, João.\n\nSegue a resposta, com acentuação: ção.\n".into(),
            html: None,
            boundary: "mbx_01M44NRD000000000000000001".into(),
        }
    }

    #[test]
    fn texto_puro_ida_e_volta_pelo_parser() {
        let eml = build(&base());
        let p = crate::parse(&eml).unwrap();
        assert_eq!(
            p.message_id.as_deref(),
            Some("01M44NRD000000000000000001@acmeco.com.br")
        );
        assert_eq!(p.subject.as_deref(), Some("Re: Orçamento de outubro"));
        assert_eq!(p.from.as_ref().unwrap().address, "contato@acmeco.com.br");
        assert_eq!(
            p.from.as_ref().unwrap().name.as_deref(),
            Some("Contato Acme")
        );
        assert_eq!(p.to[0].name.as_deref(), Some("João Ninguém"));
        assert!(p.cc.is_empty());
        assert_eq!(p.in_reply_to, ["orig@exemplo.com"]);
        assert_eq!(p.references, ["raiz@exemplo.com", "orig@exemplo.com"]);
        assert_eq!(p.date_ms, Some(1_791_158_400_000));
        assert!(p.text.as_deref().unwrap().contains("acentuação: ção"));
        assert!(p.html.is_none());
    }

    #[test]
    fn com_html_vira_alternative_com_o_boundary_dado() {
        let mut m = base();
        m.html = Some("<p>Olá, <b>João</b></p>".into());
        m.cc = vec![OutAddress {
            name: None,
            address: "maria@exemplo.com".into(),
        }];
        let eml = build(&m);
        let raw = String::from_utf8_lossy(&eml);
        assert!(
            raw.contains("boundary=\"mbx_01M44NRD000000000000000001\""),
            "{raw}"
        );
        let p = crate::parse(&eml).unwrap();
        assert!(p.html.as_deref().unwrap().contains("<b>João</b>"));
        assert!(p.text.as_deref().unwrap().contains("Segue a resposta"));
        assert_eq!(p.cc[0].address, "maria@exemplo.com");
    }

    #[test]
    fn sem_resposta_nao_tem_in_reply_to() {
        let mut m = base();
        m.in_reply_to = None;
        m.references = vec![];
        let raw = String::from_utf8(build(&m)).unwrap();
        assert!(!raw.contains("In-Reply-To"));
        assert!(!raw.contains("References"));
    }
}
