use std::fmt;

/// Erro de construção de value object. Carrega a entrada recusada para log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    InvalidId(String),
    InvalidDomainName(String),
    InvalidEmailAddress(String),
    InvalidFolder(String),
    InvalidMatchPattern(String),
    InvalidRuleAction(String),
    InvalidTimestamp(i64),
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId(v) => write!(f, "id inválido: {v:?}"),
            Self::InvalidDomainName(v) => write!(f, "domínio inválido: {v:?}"),
            Self::InvalidEmailAddress(v) => write!(f, "endereço inválido: {v:?}"),
            Self::InvalidFolder(v) => write!(f, "pasta inválida: {v:?}"),
            Self::InvalidMatchPattern(v) => write!(f, "padrão de regra inválido: {v:?}"),
            Self::InvalidRuleAction(v) => write!(f, "ação de regra inválida: {v:?}"),
            Self::InvalidTimestamp(v) => write!(f, "timestamp inválido: {v}"),
        }
    }
}

impl std::error::Error for DomainError {}
