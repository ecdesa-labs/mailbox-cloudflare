use std::fmt;
use std::str::FromStr;

use crate::DomainError;

/// Nome de domínio em ASCII minúsculo, sem ponto final. Domínios
/// internacionalizados entram na forma punycode (`xn--`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DomainName(String);

impl DomainName {
    pub fn parse(s: &str) -> Result<Self, DomainError> {
        let err = || DomainError::InvalidDomainName(s.to_owned());
        let name = s.trim().trim_end_matches('.').to_ascii_lowercase();
        if name.is_empty() || name.len() > 253 {
            return Err(err());
        }
        let labels: Vec<&str> = name.split('.').collect();
        if labels.len() < 2 {
            return Err(err());
        }
        for label in labels {
            let ok = (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
            if !ok {
                return Err(err());
            }
        }
        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DomainName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for DomainName {
    type Err = DomainError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Endereço de email normalizado em minúsculas, inclusive a parte local.
/// Tecnicamente a parte local diferencia maiúsculas, mas na prática nenhum
/// provedor faz isso, e casar identidades e regras fica previsível.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EmailAddress {
    full: String,
    at: usize,
}

impl EmailAddress {
    pub fn parse(s: &str) -> Result<Self, DomainError> {
        let err = || DomainError::InvalidEmailAddress(s.to_owned());
        let trimmed = s.trim();
        // Aceita a forma de envelope `<a@b>`.
        let trimmed = trimmed
            .strip_prefix('<')
            .and_then(|t| t.strip_suffix('>'))
            .unwrap_or(trimmed);
        let (local, domain) = trimmed.rsplit_once('@').ok_or_else(err)?;
        let local_ok = (1..=64).contains(&local.len())
            && !local.contains('@')
            && local
                .chars()
                .all(|c| !c.is_whitespace() && !c.is_control() && !matches!(c, '<' | '>'));
        if !local_ok {
            return Err(err());
        }
        let domain = DomainName::parse(domain).map_err(|_| err())?;
        let local = local.to_lowercase();
        let full = format!("{local}@{domain}");
        if full.len() > 254 {
            return Err(err());
        }
        Ok(Self {
            at: local.len(),
            full,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.full
    }

    pub fn local_part(&self) -> &str {
        &self.full[..self.at]
    }

    /// Parte do domínio. Já validada na construção.
    pub fn domain(&self) -> DomainName {
        DomainName(self.domain_str().to_owned())
    }

    pub fn domain_str(&self) -> &str {
        &self.full[self.at + 1..]
    }
}

impl fmt::Display for EmailAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.full)
    }
}

impl FromStr for EmailAddress {
    type Err = DomainError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Endereço com nome de exibição, como aparece em From/To/Cc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mailbox {
    pub name: Option<String>,
    pub addr: EmailAddress,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dominio_normaliza() {
        let d = DomainName::parse(" Pessoal.DEV. ").unwrap();
        assert_eq!(d.as_str(), "pessoal.dev");
        assert_eq!(
            DomainName::parse("acmeco.com.br").unwrap().as_str(),
            "acmeco.com.br"
        );
        assert!(DomainName::parse("xn--bcher-kva.example").is_ok());
    }

    #[test]
    fn dominio_recusa() {
        let longo = "a".repeat(64) + ".dev";
        for s in [
            "",
            "localhost",
            "-a.dev",
            "a-.dev",
            "a..dev",
            "a_b.dev",
            "bücher.de",
            longo.as_str(),
        ] {
            assert!(DomainName::parse(s).is_err(), "{s}");
        }
    }

    #[test]
    fn endereco_normaliza() {
        let a = EmailAddress::parse("<GitHub@Pessoal.dev>").unwrap();
        assert_eq!(a.as_str(), "github@pessoal.dev");
        assert_eq!(a.local_part(), "github");
        assert_eq!(a.domain_str(), "pessoal.dev");
        assert_eq!(a.domain(), DomainName::parse("pessoal.dev").unwrap());
    }

    #[test]
    fn endereco_aceita_mais() {
        let a = EmailAddress::parse("ctf+2026@pessoal.dev").unwrap();
        assert_eq!(a.local_part(), "ctf+2026");
    }

    #[test]
    fn endereco_recusa() {
        let longo = "a".repeat(65) + "@pessoal.dev";
        for s in [
            "",
            "sem-arroba",
            "@pessoal.dev",
            "a@",
            "a b@pessoal.dev",
            "a@b@pessoal.dev",
            "a@localhost",
            "a\n@pessoal.dev",
            longo.as_str(),
        ] {
            assert!(EmailAddress::parse(s).is_err(), "{s:?}");
        }
    }
}
