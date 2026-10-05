use std::fmt;

/// Resultado de SPF/DKIM/DMARC como aparece em `Authentication-Results`
/// (RFC 8601). Valor desconhecido vira `None` na leitura, não erro: o header
/// é escrito por terceiros.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthVerdict {
    Pass,
    Fail,
    SoftFail,
    Neutral,
    None,
    TempError,
    PermError,
    Policy,
}

impl AuthVerdict {
    const ALL: [AuthVerdict; 8] = [
        AuthVerdict::Pass,
        AuthVerdict::Fail,
        AuthVerdict::SoftFail,
        AuthVerdict::Neutral,
        AuthVerdict::None,
        AuthVerdict::TempError,
        AuthVerdict::PermError,
        AuthVerdict::Policy,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            AuthVerdict::Pass => "pass",
            AuthVerdict::Fail => "fail",
            AuthVerdict::SoftFail => "softfail",
            AuthVerdict::Neutral => "neutral",
            AuthVerdict::None => "none",
            AuthVerdict::TempError => "temperror",
            AuthVerdict::PermError => "permerror",
            AuthVerdict::Policy => "policy",
        }
    }

    /// Leitura tolerante: ignora caixa e espaços.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

impl fmt::Display for AuthVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Colunas `spf`, `dkim`, `dmarc`. `None` = header ausente ou ilegível.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AuthResults {
    pub spf: Option<AuthVerdict>,
    pub dkim: Option<AuthVerdict>,
    pub dmarc: Option<AuthVerdict>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ida_e_volta() {
        for v in AuthVerdict::ALL {
            assert_eq!(AuthVerdict::parse(v.as_str()), Some(v));
        }
    }

    #[test]
    fn tolerante() {
        assert_eq!(AuthVerdict::parse(" PASS "), Some(AuthVerdict::Pass));
        assert_eq!(AuthVerdict::parse("SoftFail"), Some(AuthVerdict::SoftFail));
        assert_eq!(AuthVerdict::parse("bogus"), None);
        assert_eq!(AuthVerdict::parse(""), None);
    }
}
