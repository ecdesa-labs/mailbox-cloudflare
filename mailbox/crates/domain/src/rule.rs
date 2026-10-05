//! Regras de roteamento por endereço de entrada (`envelope_to`).

use std::fmt;
use std::str::FromStr;

use crate::{DomainError, DomainName, EmailAddress, Folder, RuleId};

/// Padrão da coluna `rules.match_address`: `*@pessoal.dev` (qualquer
/// endereço do domínio) ou `contato@acmeco.com.br` (endereço exato).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MatchPattern {
    AnyAt(DomainName),
    Exact(EmailAddress),
}

impl MatchPattern {
    pub fn parse(s: &str) -> Result<Self, DomainError> {
        let err = || DomainError::InvalidMatchPattern(s.to_owned());
        match s.trim().strip_prefix("*@") {
            Some(domain) => DomainName::parse(domain)
                .map(Self::AnyAt)
                .map_err(|_| err()),
            None if s.contains('*') => Err(err()),
            None => EmailAddress::parse(s).map(Self::Exact).map_err(|_| err()),
        }
    }

    pub fn matches(&self, addr: &EmailAddress) -> bool {
        match self {
            Self::AnyAt(d) => addr.domain_str() == d.as_str(),
            Self::Exact(a) => a == addr,
        }
    }
}

impl fmt::Display for MatchPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AnyAt(d) => write!(f, "*@{d}"),
            Self::Exact(a) => write!(f, "{a}"),
        }
    }
}

impl FromStr for MatchPattern {
    type Err = DomainError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Ação da regra. Na tabela: `action` + `params` (JSON); o adaptador lê o
/// JSON e entrega a pasta já tipada em `from_parts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuleAction {
    Move(Folder),
    Archive,
    MarkRead,
    /// Rejeição silenciosa: a mensagem é descartada no processamento.
    Reject,
}

impl RuleAction {
    pub fn from_parts(action: &str, folder: Option<Folder>) -> Result<Self, DomainError> {
        match (action, folder) {
            ("move", Some(f)) => Ok(Self::Move(f)),
            ("archive", None) => Ok(Self::Archive),
            ("mark_read", None) => Ok(Self::MarkRead),
            ("reject", None) => Ok(Self::Reject),
            _ => Err(DomainError::InvalidRuleAction(action.to_owned())),
        }
    }

    /// Valor da coluna `rules.action`.
    pub fn kind(self) -> &'static str {
        match self {
            Self::Move(_) => "move",
            Self::Archive => "archive",
            Self::MarkRead => "mark_read",
            Self::Reject => "reject",
        }
    }

    pub fn folder(self) -> Option<Folder> {
        match self {
            Self::Move(f) => Some(f),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: RuleId,
    /// Menor primeiro.
    pub priority: i64,
    pub pattern: MatchPattern,
    pub action: RuleAction,
    pub enabled: bool,
}

/// Efeito combinado das regras sobre uma mensagem nova.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RuleOutcome {
    pub reject: bool,
    /// `None` = mantém a pasta padrão (`inbox`).
    pub folder: Option<Folder>,
    pub mark_read: bool,
}

impl Rule {
    /// Aplica as regras habilitadas que casam com `to`, em ordem de
    /// prioridade (empate: `id`, para ser determinístico).
    /// `reject` encerra tudo; a primeira regra que define pasta vence;
    /// `mark_read` soma.
    pub fn evaluate(rules: &[Rule], to: &EmailAddress) -> RuleOutcome {
        let mut matching: Vec<&Rule> = rules
            .iter()
            .filter(|r| r.enabled && r.pattern.matches(to))
            .collect();
        matching.sort_by_key(|r| (r.priority, r.id));

        let mut out = RuleOutcome::default();
        for rule in matching {
            match rule.action {
                RuleAction::Reject => {
                    return RuleOutcome {
                        reject: true,
                        ..RuleOutcome::default()
                    }
                }
                RuleAction::MarkRead => out.mark_read = true,
                RuleAction::Archive => {
                    out.folder.get_or_insert(Folder::Archive);
                }
                RuleAction::Move(f) => {
                    out.folder.get_or_insert(f);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnixMs;

    fn addr(s: &str) -> EmailAddress {
        EmailAddress::parse(s).unwrap()
    }

    fn rule(n: u128, priority: i64, pattern: &str, action: RuleAction) -> Rule {
        Rule {
            id: RuleId::from_parts(UnixMs::new(0).unwrap(), n),
            priority,
            pattern: MatchPattern::parse(pattern).unwrap(),
            action,
            enabled: true,
        }
    }

    #[test]
    fn padrao_parse() {
        assert_eq!(
            MatchPattern::parse("*@Pessoal.dev").unwrap().to_string(),
            "*@pessoal.dev"
        );
        assert_eq!(
            MatchPattern::parse("Contato@acmeco.com.br")
                .unwrap()
                .to_string(),
            "contato@acmeco.com.br"
        );
        for s in ["*", "*@", "a*@pessoal.dev", "*@*.dev", "x"] {
            assert!(MatchPattern::parse(s).is_err(), "{s}");
        }
    }

    #[test]
    fn padrao_casa() {
        let any = MatchPattern::parse("*@pessoal.dev").unwrap();
        assert!(any.matches(&addr("github@pessoal.dev")));
        assert!(!any.matches(&addr("github@acmeco.com.br")));
        assert!(!any.matches(&addr("x@sub.pessoal.dev")));

        let exact = MatchPattern::parse("ctf@pessoal.dev").unwrap();
        assert!(exact.matches(&addr("CTF@pessoal.dev")));
        assert!(!exact.matches(&addr("github@pessoal.dev")));
    }

    #[test]
    fn acao_from_parts() {
        assert_eq!(
            RuleAction::from_parts("move", Some(Folder::Spam)).unwrap(),
            RuleAction::Move(Folder::Spam)
        );
        assert!(RuleAction::from_parts("move", None).is_err());
        assert!(RuleAction::from_parts("archive", Some(Folder::Spam)).is_err());
        assert!(RuleAction::from_parts("delete", None).is_err());
        for a in [
            RuleAction::Archive,
            RuleAction::MarkRead,
            RuleAction::Reject,
        ] {
            assert_eq!(RuleAction::from_parts(a.kind(), None).unwrap(), a);
        }
    }

    #[test]
    fn sem_regras_nada_muda() {
        let out = Rule::evaluate(&[], &addr("a@pessoal.dev"));
        assert_eq!(out, RuleOutcome::default());
    }

    #[test]
    fn reject_vence_tudo() {
        let rules = [
            rule(1, 0, "*@pessoal.dev", RuleAction::MarkRead),
            rule(2, 10, "vazou@pessoal.dev", RuleAction::Reject),
        ];
        assert!(Rule::evaluate(&rules, &addr("vazou@pessoal.dev")).reject);
        assert!(!Rule::evaluate(&rules, &addr("ok@pessoal.dev")).reject);
    }

    #[test]
    fn primeira_pasta_vence_e_mark_read_soma() {
        let rules = [
            rule(1, 20, "*@pessoal.dev", RuleAction::Archive),
            rule(2, 10, "ctf@pessoal.dev", RuleAction::Move(Folder::Spam)),
            rule(3, 30, "*@pessoal.dev", RuleAction::MarkRead),
        ];
        let out = Rule::evaluate(&rules, &addr("ctf@pessoal.dev"));
        assert_eq!(out.folder, Some(Folder::Spam));
        assert!(out.mark_read);
        assert!(!out.reject);
    }

    #[test]
    fn desabilitada_e_ignorada() {
        let mut r = rule(1, 0, "*@pessoal.dev", RuleAction::Reject);
        r.enabled = false;
        assert!(!Rule::evaluate(&[r], &addr("a@pessoal.dev")).reject);
    }

    #[test]
    fn empate_de_prioridade_decide_por_id() {
        let rules = [
            rule(2, 0, "*@pessoal.dev", RuleAction::Archive),
            rule(1, 0, "*@pessoal.dev", RuleAction::Move(Folder::Spam)),
        ];
        let out = Rule::evaluate(&rules, &addr("a@pessoal.dev"));
        assert_eq!(out.folder, Some(Folder::Spam));
    }
}
