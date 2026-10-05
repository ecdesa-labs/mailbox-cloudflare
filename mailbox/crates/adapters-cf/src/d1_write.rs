//! Alterações da API no D1 (Fase 7): flags, pastas, lote e regras.

use mailbox_application::{MailWriter, MessagePatch, PortError, RuleStore};
use mailbox_domain::{Folder, MatchPattern, MessageId, Rule, RuleAction, RuleId};
use serde::Deserialize;
use serde_json::json;
use wasm_bindgen::JsValue;

use crate::D1Index;

fn err(ctx: &'static str) -> impl Fn(worker::Error) -> PortError {
    move |e| PortError(format!("d1 {ctx}: {e}"))
}
fn s(v: &str) -> JsValue {
    JsValue::from_str(v)
}
fn opt_bool(v: Option<bool>) -> JsValue {
    v.map(|b| JsValue::from_f64(b as u8 as f64))
        .unwrap_or(JsValue::NULL)
}

/// `SET` comum ao patch e ao lote: campo ausente (`NULL`) mantém o valor.
const PATCH_SET: &str = "UPDATE messages SET is_read = COALESCE(?, is_read), \
    is_starred = COALESCE(?, is_starred), folder = COALESCE(?, folder)";

fn patch_args(p: &MessagePatch) -> Vec<JsValue> {
    vec![
        opt_bool(p.read),
        opt_bool(p.starred),
        p.folder.map(|f| s(f.as_str())).unwrap_or(JsValue::NULL),
    ]
}

impl D1Index<'_> {
    async fn changes(
        &self,
        ctx: &'static str,
        sql: &str,
        args: &[JsValue],
    ) -> Result<u64, PortError> {
        let r = self
            .db
            .prepare(sql)
            .bind(args)
            .map_err(err(ctx))?
            .run()
            .await
            .map_err(err(ctx))?;
        Ok(r.meta()
            .map_err(err(ctx))?
            .and_then(|m| m.changes)
            .unwrap_or(0) as u64)
    }
}

impl MailWriter for D1Index<'_> {
    async fn patch_message(&self, id: MessageId, patch: &MessagePatch) -> Result<bool, PortError> {
        let mut args = patch_args(patch);
        args.push(s(&id.to_string()));
        let sql = format!("{PATCH_SET} WHERE id = ? AND deleted_at IS NULL");
        Ok(self.changes("patch_message", &sql, &args).await? > 0)
    }

    async fn patch_many(&self, ids: &[MessageId], patch: &MessagePatch) -> Result<u64, PortError> {
        if ids.is_empty() {
            return Ok(0);
        }
        // 3 parâmetros do SET + até 97 ids por statement (limite de 100).
        let mut total = 0;
        for chunk in ids.chunks(97) {
            let mut args = patch_args(patch);
            args.extend(chunk.iter().map(|i| s(&i.to_string())));
            let sql = format!(
                "{PATCH_SET} WHERE deleted_at IS NULL AND id IN ({})",
                vec!["?"; chunk.len()].join(",")
            );
            total += self.changes("patch_many", &sql, &args).await?;
        }
        Ok(total)
    }
}

#[derive(Deserialize)]
struct RuleRow {
    id: String,
    priority: f64,
    match_address: String,
    action: String,
    params: Option<String>,
    enabled: f64,
}

impl RuleRow {
    fn into_rule(self) -> Option<Rule> {
        let folder = self
            .params
            .as_deref()
            .and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok())
            .and_then(|v| v.get("folder").and_then(|f| f.as_str()).map(str::to_owned))
            .and_then(|f| f.parse::<Folder>().ok());
        Some(Rule {
            id: RuleId::parse(&self.id).ok()?,
            priority: self.priority as i64,
            pattern: MatchPattern::parse(&self.match_address).ok()?,
            action: RuleAction::from_parts(&self.action, folder).ok()?,
            enabled: self.enabled != 0.0,
        })
    }
}

const RULE_COLUMNS: &str = "id, priority, match_address, action, params, enabled";

impl RuleStore for D1Index<'_> {
    async fn list_rules(&self) -> Result<Vec<Rule>, PortError> {
        let rows: Vec<RuleRow> = self
            .db
            .prepare(format!(
                "SELECT {RULE_COLUMNS} FROM rules ORDER BY priority, id"
            ))
            .all()
            .await
            .map_err(err("list_rules"))?
            .results()
            .map_err(err("list_rules"))?;
        Ok(rows.into_iter().filter_map(RuleRow::into_rule).collect())
    }

    async fn get_rule(&self, id: RuleId) -> Result<Option<Rule>, PortError> {
        let rows: Vec<RuleRow> = self
            .db
            .prepare(format!("SELECT {RULE_COLUMNS} FROM rules WHERE id = ?"))
            .bind(&[s(&id.to_string())])
            .map_err(err("get_rule"))?
            .all()
            .await
            .map_err(err("get_rule"))?
            .results()
            .map_err(err("get_rule"))?;
        Ok(rows.into_iter().next().and_then(RuleRow::into_rule))
    }

    async fn put_rule(&self, r: &Rule) -> Result<(), PortError> {
        let params = r
            .action
            .folder()
            .map(|f| json!({ "folder": f.as_str() }).to_string());
        self.changes(
            "put_rule",
            "INSERT INTO rules (id, priority, match_address, action, params, enabled) VALUES (?, ?, ?, ?, ?, ?) \
             ON CONFLICT(id) DO UPDATE SET priority = excluded.priority, match_address = excluded.match_address, \
               action = excluded.action, params = excluded.params, enabled = excluded.enabled",
            &[
                s(&r.id.to_string()),
                JsValue::from_f64(r.priority as f64),
                s(&r.pattern.to_string()),
                s(r.action.kind()),
                params.as_deref().map(s).unwrap_or(JsValue::NULL),
                opt_bool(Some(r.enabled)),
            ],
        )
        .await?;
        Ok(())
    }

    async fn delete_rule(&self, id: RuleId) -> Result<bool, PortError> {
        Ok(self
            .changes(
                "delete_rule",
                "DELETE FROM rules WHERE id = ?",
                &[s(&id.to_string())],
            )
            .await?
            > 0)
    }
}
