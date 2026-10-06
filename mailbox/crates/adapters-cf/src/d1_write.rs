//! Alterações da API no D1 (Fase 7): flags, pastas, lote e regras.

use mailbox_application::{
    Cutoffs, Expired, MailWriter, MessagePatch, PortError, RetentionStore, RuleStore,
};
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
/// Mudar de pasta grava quando (a retenção da lixeira/spam conta daí). No
/// `SET`, todas as expressões leem o valor antigo de `folder`.
const PATCH_SET: &str = "UPDATE messages SET is_read = COALESCE(?1, is_read), \
    is_starred = COALESCE(?2, is_starred), \
    folder_changed_at = CASE WHEN ?3 IS NOT NULL AND ?3 <> folder THEN ?4 ELSE folder_changed_at END, \
    folder = COALESCE(?3, folder)";
/// Primeiro parâmetro depois dos 4 do `SET`.
const FIRST_ID_PARAM: usize = 5;

fn patch_args(p: &MessagePatch) -> Vec<JsValue> {
    vec![
        opt_bool(p.read),
        opt_bool(p.starred),
        p.folder.map(|f| s(f.as_str())).unwrap_or(JsValue::NULL),
        JsValue::from_f64(worker::Date::now().as_millis() as f64),
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
        let sql = format!("{PATCH_SET} WHERE id = ?{FIRST_ID_PARAM} AND deleted_at IS NULL");
        Ok(self.changes("patch_message", &sql, &args).await? > 0)
    }

    async fn patch_many(&self, ids: &[MessageId], patch: &MessagePatch) -> Result<u64, PortError> {
        if ids.is_empty() {
            return Ok(0);
        }
        // 4 parâmetros do SET + até 96 ids por statement (limite de 100).
        let mut total = 0;
        for chunk in ids.chunks(96) {
            let mut args = patch_args(patch);
            args.extend(chunk.iter().map(|i| s(&i.to_string())));
            let params: Vec<String> = (0..chunk.len())
                .map(|i| format!("?{}", FIRST_ID_PARAM + i))
                .collect();
            let sql = format!(
                "{PATCH_SET} WHERE deleted_at IS NULL AND id IN ({})",
                params.join(",")
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

impl RetentionStore for D1Index<'_> {
    async fn expired(&self, c: &Cutoffs, limit: usize) -> Result<Vec<Expired>, PortError> {
        #[derive(Deserialize)]
        struct Row {
            id: String,
            raw_key: String,
            html_key: Option<String>,
            att_keys: Option<String>,
        }
        let cut = |c: Option<mailbox_domain::UnixMs>| {
            c.map(|t| JsValue::from_f64(t.as_i64() as f64))
                .unwrap_or(JsValue::NULL)
        };
        let rows: Vec<Row> = self
            .db
            .prepare(
                "SELECT m.id, m.raw_key, m.html_key, \
                   (SELECT group_concat(a.r2_key, char(10)) FROM attachments a WHERE a.message_id = m.id) AS att_keys \
                 FROM messages m WHERE \
                   (?1 IS NOT NULL AND m.deleted_at IS NULL AND m.folder = 'trash' \
                      AND COALESCE(m.folder_changed_at, m.received_at) < ?1) \
                   OR (?2 IS NOT NULL AND m.deleted_at IS NULL AND m.folder = 'spam' \
                      AND COALESCE(m.folder_changed_at, m.received_at) < ?2) \
                   OR (?3 IS NOT NULL AND m.deleted_at IS NOT NULL AND m.deleted_at < ?3) \
                 ORDER BY m.received_at LIMIT ?4",
            )
            .bind(&[cut(c.trash), cut(c.spam), cut(c.hidden), JsValue::from_f64(limit as f64)])
            .map_err(err("expired"))?
            .all()
            .await
            .map_err(err("expired"))?
            .results()
            .map_err(err("expired"))?;
        Ok(rows
            .into_iter()
            .filter_map(|r| {
                let mut blob_keys = vec![r.raw_key];
                blob_keys.extend(r.html_key);
                blob_keys.extend(
                    r.att_keys
                        .iter()
                        .flat_map(|k| k.split('\n').map(str::to_owned)),
                );
                Some(Expired {
                    id: MessageId::parse(&r.id).ok()?,
                    blob_keys,
                })
            })
            .collect())
    }

    async fn purge(&self, ids: &[MessageId]) -> Result<(), PortError> {
        #[derive(Deserialize)]
        struct T {
            thread_id: String,
        }
        for chunk in ids.chunks(100) {
            let ph = vec!["?"; chunk.len()].join(",");
            let args: Vec<JsValue> = chunk.iter().map(|i| s(&i.to_string())).collect();
            // Threads afetadas, lidas antes de apagar.
            let threads: Vec<T> = self
                .db
                .prepare(format!(
                    "SELECT DISTINCT thread_id FROM messages WHERE id IN ({ph}) AND thread_id IS NOT NULL"
                ))
                .bind(&args)
                .map_err(err("purge"))?
                .all()
                .await
                .map_err(err("purge"))?
                .results()
                .map_err(err("purge"))?;
            // Anexos saem em cascata (FK) e o FTS pelo trigger. Threads:
            // recalcula as afetadas e remove as que ficaram vazias. Tudo numa
            // transação (`batch`).
            let mut batch = vec![self
                .db
                .prepare(format!("DELETE FROM messages WHERE id IN ({ph})"))
                .bind(&args)
                .map_err(err("purge"))?];
            if !threads.is_empty() {
                let tph = vec!["?"; threads.len()].join(",");
                let targs: Vec<JsValue> = threads.iter().map(|t| s(&t.thread_id)).collect();
                batch.push(
                    self.db
                        .prepare(format!(
                            "UPDATE threads SET \
                               message_count = (SELECT count(*) FROM messages m WHERE m.thread_id = threads.id AND m.deleted_at IS NULL), \
                               last_message_at = COALESCE((SELECT max(received_at) FROM messages m WHERE m.thread_id = threads.id), last_message_at) \
                             WHERE id IN ({tph})"
                        ))
                        .bind(&targs)
                        .map_err(err("purge"))?,
                );
                batch.push(
                    self.db
                        .prepare(format!(
                            "DELETE FROM threads WHERE id IN ({tph}) \
                               AND NOT EXISTS (SELECT 1 FROM messages m WHERE m.thread_id = threads.id)"
                        ))
                        .bind(&targs)
                        .map_err(err("purge"))?,
                );
            }
            self.db.batch(batch).await.map_err(err("purge"))?;
        }
        Ok(())
    }
}
