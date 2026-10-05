//! Lado de escrita da API (docs seção 7, Fase 7): flags, pastas, lote e
//! regras de roteamento. Validação estrita: campo desconhecido é erro.

use std::collections::BTreeSet;

use mailbox_domain::{Folder, MatchPattern, MessageId, Rule, RuleAction, RuleId};
use serde_json::{json, Map, Value};

use crate::ports::PortError;
use crate::read::QueryError;

/// Máximo de mensagens num `POST /api/messages/bulk`.
pub const MAX_BULK: usize = 100;
/// Prioridade de uma regra criada sem `priority`.
pub const DEFAULT_PRIORITY: i64 = 100;

fn invalid(param: &'static str, message: impl Into<String>) -> QueryError {
    QueryError::Invalid {
        param,
        message: message.into(),
    }
}

fn object<'a>(v: &'a Value, what: &'static str) -> Result<&'a Map<String, Value>, QueryError> {
    v.as_object()
        .ok_or_else(|| invalid(what, "esperado um objeto JSON"))
}

fn only_keys(
    o: &Map<String, Value>,
    what: &'static str,
    allowed: &[&str],
) -> Result<(), QueryError> {
    match o.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(invalid(what, format!("campo desconhecido: {k}"))),
        None => Ok(()),
    }
}

fn opt_bool(o: &Map<String, Value>, key: &'static str) -> Result<Option<bool>, QueryError> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(invalid(key, "esperado true ou false")),
    }
}

fn opt_folder(o: &Map<String, Value>) -> Result<Option<Folder>, QueryError> {
    match o.get("folder") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => s
            .parse()
            .map(Some)
            .map_err(|_| invalid("folder", "pasta desconhecida")),
        Some(_) => Err(invalid("folder", "esperado texto")),
    }
}

/// `PATCH /api/messages/:id` e o `patch` do lote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MessagePatch {
    pub read: Option<bool>,
    pub starred: Option<bool>,
    pub folder: Option<Folder>,
}

impl MessagePatch {
    pub fn from_json(v: &Value) -> Result<Self, QueryError> {
        let o = object(v, "patch")?;
        only_keys(o, "patch", &["read", "starred", "folder"])?;
        let p = Self {
            read: opt_bool(o, "read")?,
            starred: opt_bool(o, "starred")?,
            folder: opt_folder(o)?,
        };
        if p == Self::default() {
            return Err(invalid(
                "patch",
                "nada para mudar (read, starred ou folder)",
            ));
        }
        Ok(p)
    }
}

/// `POST /api/messages/bulk` `{ "ids": [...], "patch": {...} }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkPatch {
    pub ids: Vec<MessageId>,
    pub patch: MessagePatch,
}

impl BulkPatch {
    pub fn from_json(v: &Value) -> Result<Self, QueryError> {
        let o = object(v, "body")?;
        only_keys(o, "body", &["ids", "patch"])?;
        let ids = o
            .get("ids")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("ids", "esperada uma lista"))?;
        let mut unique = BTreeSet::new();
        for id in ids {
            let id = id
                .as_str()
                .and_then(|s| MessageId::parse(s).ok())
                .ok_or_else(|| invalid("ids", "id inválido"))?;
            unique.insert(id);
        }
        if unique.is_empty() || unique.len() > MAX_BULK {
            return Err(invalid("ids", format!("entre 1 e {MAX_BULK}")));
        }
        Ok(Self {
            ids: unique.into_iter().collect(),
            patch: MessagePatch::from_json(o.get("patch").unwrap_or(&Value::Null))?,
        })
    }
}

/// Monta uma regra nova (`POST /api/rules`).
/// `{ match_address, action, folder?, priority?, enabled? }`.
pub fn rule_from_json(id: RuleId, v: &Value) -> Result<Rule, QueryError> {
    let base = Rule {
        id,
        priority: DEFAULT_PRIORITY,
        pattern: MatchPattern::parse("*@exemplo.invalid").expect("padrão fixo válido"),
        action: RuleAction::Archive,
        enabled: true,
    };
    let o = object(v, "body")?;
    if !o.contains_key("match_address") || !o.contains_key("action") {
        return Err(invalid("body", "match_address e action são obrigatórios"));
    }
    apply_rule_patch(base, v)
}

/// Aplica uma alteração parcial (`PATCH /api/rules/:id`) e revalida tudo.
pub fn apply_rule_patch(mut rule: Rule, v: &Value) -> Result<Rule, QueryError> {
    let o = object(v, "body")?;
    only_keys(
        o,
        "body",
        &["match_address", "action", "folder", "priority", "enabled"],
    )?;
    if o.is_empty() {
        return Err(invalid("body", "nada para mudar"));
    }
    if let Some(m) = o.get("match_address") {
        let m = m
            .as_str()
            .ok_or_else(|| invalid("match_address", "esperado texto"))?;
        rule.pattern = MatchPattern::parse(m)
            .map_err(|_| invalid("match_address", "use *@dominio.com ou endereco@dominio.com"))?;
    }
    if let Some(p) = o.get("priority") {
        rule.priority = p
            .as_i64()
            .ok_or_else(|| invalid("priority", "esperado inteiro"))?;
    }
    if let Some(e) = opt_bool(o, "enabled")? {
        rule.enabled = e;
    }
    // `action` e `folder` andam juntos: só `move` tem pasta.
    let kind = match o.get("action") {
        Some(a) => a
            .as_str()
            .ok_or_else(|| invalid("action", "esperado texto"))?
            .to_owned(),
        None => rule.action.kind().to_owned(),
    };
    let folder = match o.get("folder") {
        Some(_) => opt_folder(o)?,
        None if o.contains_key("action") => None,
        None => rule.action.folder(),
    };
    rule.action = RuleAction::from_parts(&kind, folder)
        .map_err(|_| invalid("action", "move (com folder), archive, mark_read ou reject"))?;
    Ok(rule)
}

pub fn rule_json(r: &Rule) -> Value {
    json!({
        "id": r.id.to_string(),
        "priority": r.priority,
        "match_address": r.pattern.to_string(),
        "action": r.action.kind(),
        "folder": r.action.folder().map(|f| f.as_str()),
        "enabled": r.enabled,
    })
}

/// Alterações de mensagens (D1).
#[allow(async_fn_in_trait)]
pub trait MailWriter {
    /// `false` se a mensagem não existe (ou está oculta).
    async fn patch_message(&self, id: MessageId, patch: &MessagePatch) -> Result<bool, PortError>;
    /// Quantas foram alteradas.
    async fn patch_many(&self, ids: &[MessageId], patch: &MessagePatch) -> Result<u64, PortError>;
}

/// Regras de roteamento (D1), inclusive as desabilitadas.
#[allow(async_fn_in_trait)]
pub trait RuleStore {
    async fn list_rules(&self) -> Result<Vec<Rule>, PortError>;
    async fn get_rule(&self, id: RuleId) -> Result<Option<Rule>, PortError>;
    async fn put_rule(&self, rule: &Rule) -> Result<(), PortError>;
    async fn delete_rule(&self, id: RuleId) -> Result<bool, PortError>;
}

#[cfg(test)]
mod tests;
