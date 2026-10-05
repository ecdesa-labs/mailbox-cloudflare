//! Chaves do R2 (docs seção 5.1). Prefixo por domínio para facilitar
//! retenção e remoção por domínio.
//!
//! ```text
//! {domain}/raw/{yyyy}/{mm}/{message_id}.eml
//! {domain}/att/{message_id}/{ordinal}
//! {domain}/html/{message_id}.html
//! ```

use std::fmt;

use crate::{DomainError, DomainName, MessageId};

/// Chave do `.eml` bruto, a fonte da verdade. Ano e mês vêm do timestamp
/// do próprio `MessageId`, então a chave é derivável só do domínio + id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RawKey {
    key: String,
    domain: DomainName,
    message_id: MessageId,
}

impl RawKey {
    pub fn new(domain: &DomainName, message_id: MessageId) -> Self {
        let (y, m) = message_id.timestamp().year_month();
        Self {
            key: format!("{domain}/raw/{y:04}/{m:02}/{message_id}.eml"),
            domain: domain.clone(),
            message_id,
        }
    }

    /// Lê uma chave que veio de fora (payload da fila, comando de
    /// reprocessamento). Só aceita a forma que `new` produziria.
    pub fn parse(s: &str) -> Result<Self, DomainError> {
        let err = || DomainError::InvalidId(s.to_owned());
        let parts: Vec<&str> = s.split('/').collect();
        let [domain, "raw", _, _, file] = parts[..] else {
            return Err(err());
        };
        let id = file.strip_suffix(".eml").ok_or_else(err)?;
        let domain = DomainName::parse(domain).map_err(|_| err())?;
        let id = MessageId::parse(id).map_err(|_| err())?;
        let key = Self::new(&domain, id);
        if key.key != s {
            return Err(err());
        }
        Ok(key)
    }

    pub fn as_str(&self) -> &str {
        &self.key
    }

    pub fn domain(&self) -> &DomainName {
        &self.domain
    }

    pub fn message_id(&self) -> MessageId {
        self.message_id
    }
}

impl fmt::Display for RawKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.key)
    }
}

/// Chave de um anexo, pela posição dele na mensagem.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AttachmentKey(String);

impl AttachmentKey {
    pub fn new(domain: &DomainName, message_id: MessageId, ordinal: u32) -> Self {
        Self(format!("{domain}/att/{message_id}/{ordinal}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AttachmentKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Chave do HTML sanitizado, com `cid:` já reescrito.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HtmlKey(String);

impl HtmlKey {
    pub fn new(domain: &DomainName, message_id: MessageId) -> Self {
        Self(format!("{domain}/html/{message_id}.html"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HtmlKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnixMs;

    fn fixture() -> (DomainName, MessageId) {
        let d = DomainName::parse("pessoal.dev").unwrap();
        let id = MessageId::from_parts(UnixMs::new(1_791_158_400_000).unwrap(), 1);
        (d, id)
    }

    #[test]
    fn layout() {
        let (d, id) = fixture();
        assert_eq!(
            RawKey::new(&d, id).as_str(),
            "pessoal.dev/raw/2026/10/01M44NRD000000000000000001.eml"
        );
        assert_eq!(
            AttachmentKey::new(&d, id, 0).as_str(),
            "pessoal.dev/att/01M44NRD000000000000000001/0"
        );
        assert_eq!(
            HtmlKey::new(&d, id).as_str(),
            "pessoal.dev/html/01M44NRD000000000000000001.html"
        );
    }

    #[test]
    fn mes_com_zero_a_esquerda() {
        let d = DomainName::parse("acmeco.com.br").unwrap();
        let id = MessageId::from_parts(UnixMs::new(0).unwrap(), 0);
        assert!(RawKey::new(&d, id)
            .as_str()
            .starts_with("acmeco.com.br/raw/1970/01/"));
    }

    #[test]
    fn raw_parse_ida_e_volta() {
        let (d, id) = fixture();
        let k = RawKey::new(&d, id);
        let lida = RawKey::parse(k.as_str()).unwrap();
        assert_eq!(lida, k);
        assert_eq!(lida.domain(), &d);
        assert_eq!(lida.message_id(), id);
    }

    #[test]
    fn raw_parse_recusa() {
        for s in [
            "",
            "pessoal.dev/raw/2026/10/01M44NRD000000000000000001",
            "pessoal.dev/raw/2026/09/01M44NRD000000000000000001.eml", // mês não bate
            "pessoal.dev/att/2026/10/01M44NRD000000000000000001.eml",
            "Pessoal.dev/raw/2026/10/01M44NRD000000000000000001.eml", // não canônica
            "../raw/2026/10/01M44NRD000000000000000001.eml",
            "pessoal.dev/raw/2026/10/x/01M44NRD000000000000000001.eml",
        ] {
            assert!(RawKey::parse(s).is_err(), "{s}");
        }
    }
}
