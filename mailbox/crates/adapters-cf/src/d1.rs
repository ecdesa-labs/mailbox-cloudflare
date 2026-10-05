//! Índice no D1: mensagens, threads, identidades, regras, falhas e pedidos
//! de reprocessamento. Limites do D1 (seção 5.2): 100 parâmetros por query.

use std::collections::HashSet;

use mailbox_application::{
    ExistingMessage, FailureLog, MailIndex, MessageRecord, PortError, ReconcileIndex,
    ReprocessMode, ReprocessRequest,
};
use mailbox_domain::{
    DomainId, DomainName, EmailAddress, Folder, IdentityId, Mailbox, MatchPattern, MessageId, Rule,
    RuleAction, RuleId, ThreadId, UnixMs,
};
use serde::Deserialize;
use serde_json::json;
use wasm_bindgen::JsValue;
use worker::{D1Database, D1PreparedStatement};

pub struct D1Index<'a> {
    pub db: &'a D1Database,
}

fn err(ctx: &str) -> impl Fn(worker::Error) -> PortError + '_ {
    move |e| PortError(format!("d1 {ctx}: {e}"))
}

fn s(v: &str) -> JsValue {
    JsValue::from_str(v)
}
fn os(v: Option<&str>) -> JsValue {
    v.map(JsValue::from_str).unwrap_or(JsValue::NULL)
}
fn n(v: i64) -> JsValue {
    JsValue::from_f64(v as f64)
}
fn on(v: Option<i64>) -> JsValue {
    v.map(n).unwrap_or(JsValue::NULL)
}
fn b(v: bool) -> JsValue {
    n(v as i64)
}
fn placeholders(count: usize) -> String {
    vec!["?"; count].join(",")
}

fn mailboxes_json(list: &[Mailbox]) -> String {
    json!(list
        .iter()
        .map(|m| json!({ "name": m.name, "addr": m.addr.as_str() }))
        .collect::<Vec<_>>())
    .to_string()
}

impl D1Index<'_> {
    fn stmt(&self, sql: &str, args: &[JsValue]) -> Result<D1PreparedStatement, PortError> {
        self.db.prepare(sql).bind(args).map_err(err(sql))
    }

    async fn column<T: for<'de> Deserialize<'de>>(
        &self,
        sql: &str,
        args: &[JsValue],
        col: &str,
    ) -> Result<Option<T>, PortError> {
        self.stmt(sql, args)?
            .first::<T>(Some(col))
            .await
            .map_err(err(sql))
    }

    async fn rows<T: for<'de> Deserialize<'de>>(
        &self,
        sql: &str,
        args: &[JsValue],
    ) -> Result<Vec<T>, PortError> {
        self.stmt(sql, args)?
            .all()
            .await
            .map_err(err(sql))?
            .results::<T>()
            .map_err(err(sql))
    }
}

impl MailIndex for D1Index<'_> {
    async fn domain_id(&self, name: &DomainName) -> Result<Option<DomainId>, PortError> {
        let id: Option<String> = self
            .column(
                "SELECT id FROM domains WHERE name = ?",
                &[s(name.as_str())],
                "id",
            )
            .await?;
        Ok(id.and_then(|i| DomainId::parse(&i).ok()))
    }

    async fn existing(&self, id: MessageId) -> Result<Option<ExistingMessage>, PortError> {
        #[derive(Deserialize)]
        struct Row {
            thread_id: Option<String>,
        }
        let rows: Vec<Row> = self
            .rows(
                "SELECT thread_id FROM messages WHERE id = ?",
                &[s(&id.to_string())],
            )
            .await?;
        Ok(rows.into_iter().next().map(|r| ExistingMessage {
            thread_id: r.thread_id.and_then(|t| ThreadId::parse(&t).ok()),
        }))
    }

    async fn thread_of(&self, message_ids: &[String]) -> Result<Option<ThreadId>, PortError> {
        // References pode ser enorme; as mais recentes bastam.
        let ids: Vec<JsValue> = message_ids.iter().take(50).map(|i| s(i)).collect();
        if ids.is_empty() {
            return Ok(None);
        }
        let sql = format!(
            "SELECT thread_id FROM messages WHERE message_id_hdr IN ({}) AND thread_id IS NOT NULL \
             ORDER BY received_at LIMIT 1",
            placeholders(ids.len())
        );
        let t: Option<String> = self.column(&sql, &ids, "thread_id").await?;
        Ok(t.and_then(|t| ThreadId::parse(&t).ok()))
    }

    async fn thread_of_reply_to(&self, message_id: &str) -> Result<Option<ThreadId>, PortError> {
        let t: Option<String> = self
            .column(
                "SELECT thread_id FROM messages WHERE in_reply_to = ? AND thread_id IS NOT NULL \
                 ORDER BY received_at LIMIT 1",
                &[s(message_id)],
                "thread_id",
            )
            .await?;
        Ok(t.and_then(|t| ThreadId::parse(&t).ok()))
    }

    async fn rules(&self) -> Result<Vec<Rule>, PortError> {
        #[derive(Deserialize)]
        struct Row {
            id: String,
            priority: i64,
            match_address: String,
            action: String,
            params: Option<String>,
        }
        let rows: Vec<Row> = self
            .rows(
                "SELECT id, priority, match_address, action, params FROM rules WHERE enabled = 1",
                &[],
            )
            .await?;
        // Regra malformada é ignorada (a tela de regras da Fase 7 valida antes).
        Ok(rows
            .into_iter()
            .filter_map(|r| {
                let folder = r
                    .params
                    .as_deref()
                    .and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok())
                    .and_then(|v| v.get("folder").and_then(|f| f.as_str()).map(str::to_owned))
                    .and_then(|f| f.parse::<Folder>().ok());
                Some(Rule {
                    id: RuleId::parse(&r.id).ok()?,
                    priority: r.priority,
                    pattern: MatchPattern::parse(&r.match_address).ok()?,
                    action: RuleAction::from_parts(&r.action, folder).ok()?,
                    enabled: true,
                })
            })
            .collect())
    }

    async fn ensure_identity(
        &self,
        domain_id: DomainId,
        address: &EmailAddress,
        new_id: IdentityId,
    ) -> Result<IdentityId, PortError> {
        let results = self
            .db
            .batch(vec![
                self.stmt(
                    "INSERT INTO identities (id, domain_id, address) VALUES (?, ?, ?) \
                     ON CONFLICT(address) DO NOTHING",
                    &[
                        s(&new_id.to_string()),
                        s(&domain_id.to_string()),
                        s(address.as_str()),
                    ],
                )?,
                self.stmt(
                    "SELECT id FROM identities WHERE address = ?",
                    &[s(address.as_str())],
                )?,
            ])
            .await
            .map_err(err("ensure_identity"))?;
        #[derive(Deserialize)]
        struct Row {
            id: String,
        }
        let row = results
            .get(1)
            .map(|r| r.results::<Row>())
            .transpose()
            .map_err(err("ensure_identity"))?
            .and_then(|v| v.into_iter().next())
            .ok_or_else(|| PortError("identidade sumiu depois do insert".into()))?;
        IdentityId::parse(&row.id).map_err(|e| PortError(e.to_string()))
    }

    async fn save(&self, r: &MessageRecord) -> Result<(), PortError> {
        let m = &r.message;
        let id = m.id.to_string();
        let thread_id = m.thread_id.map(|t| t.to_string());
        let mut batch = vec![
            self.stmt(
                "INSERT INTO threads (id, subject_norm, last_message_at, message_count) VALUES (?, ?, ?, 0) \
                 ON CONFLICT(id) DO NOTHING",
                &[s(&r.thread.id.to_string()), os(r.thread.subject_norm.as_deref()), n(r.thread.last_message_at.as_i64())],
            )?,
            // Num reprocessamento, pasta, lido, estrela e exclusão ficam como o usuário deixou.
            self.stmt(
                "INSERT INTO messages (id, raw_key, domain_id, identity_id, thread_id, envelope_from, envelope_to, \
                   message_id_hdr, in_reply_to, references_hdr, from_addr, from_name, to_addrs, cc_addrs, subject, \
                   snippet, text_body, html_key, has_attachments, size_bytes, received_at, date_hdr, folder, \
                   is_read, is_starred, spf, dkim, dmarc, deleted_at, processing) \
                 VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?) \
                 ON CONFLICT(id) DO UPDATE SET identity_id = excluded.identity_id, \
                   thread_id = excluded.thread_id, envelope_from = excluded.envelope_from, \
                   message_id_hdr = excluded.message_id_hdr, in_reply_to = excluded.in_reply_to, \
                   references_hdr = excluded.references_hdr, from_addr = excluded.from_addr, \
                   from_name = excluded.from_name, to_addrs = excluded.to_addrs, cc_addrs = excluded.cc_addrs, \
                   subject = excluded.subject, snippet = excluded.snippet, text_body = excluded.text_body, \
                   html_key = excluded.html_key, has_attachments = excluded.has_attachments, \
                   size_bytes = excluded.size_bytes, date_hdr = excluded.date_hdr, spf = excluded.spf, \
                   dkim = excluded.dkim, dmarc = excluded.dmarc, processing = excluded.processing",
                &[
                    s(&id),
                    s(m.raw_key.as_str()),
                    s(&m.domain_id.to_string()),
                    os(m.identity_id.map(|i| i.to_string()).as_deref()),
                    os(thread_id.as_deref()),
                    os(m.envelope_from.as_ref().map(|a| a.as_str())),
                    s(m.envelope_to.as_str()),
                    os(m.message_id_hdr.as_deref()),
                    os(m.in_reply_to.as_deref()),
                    s(&json!(m.references).to_string()),
                    os(m.from.as_ref().map(|f| f.addr.as_str())),
                    os(m.from.as_ref().and_then(|f| f.name.as_deref())),
                    s(&mailboxes_json(&m.to)),
                    s(&mailboxes_json(&m.cc)),
                    os(m.subject.as_deref()),
                    os(m.snippet.as_deref()),
                    os(m.text_body.as_deref()),
                    os(m.html_key.as_ref().map(|k| k.as_str())),
                    b(m.has_attachments),
                    n(m.size_bytes as i64),
                    n(m.received_at.as_i64()),
                    on(m.date_hdr.map(UnixMs::as_i64)),
                    s(m.folder.as_str()),
                    b(m.is_read),
                    b(m.is_starred),
                    os(m.auth.spf.map(|v| v.as_str())),
                    os(m.auth.dkim.map(|v| v.as_str())),
                    os(m.auth.dmarc.map(|v| v.as_str())),
                    on(m.deleted_at.map(UnixMs::as_i64)),
                    s(r.processing.as_str()),
                ],
            )?,
            self.stmt("DELETE FROM attachments WHERE message_id = ?", &[s(&id)])?,
        ];
        // 8 colunas por anexo: 12 por statement para ficar abaixo de 100 parâmetros.
        for chunk in r.attachments.chunks(12) {
            let sql = format!(
                "INSERT INTO attachments (id, message_id, filename, content_type, size_bytes, r2_key, content_id, is_inline) VALUES {}",
                vec!["(?,?,?,?,?,?,?,?)"; chunk.len()].join(",")
            );
            let args: Vec<JsValue> = chunk
                .iter()
                .flat_map(|a| {
                    [
                        s(&a.id.to_string()),
                        s(&id),
                        os(a.filename.as_deref()),
                        os(a.content_type.as_deref()),
                        n(a.size_bytes as i64),
                        s(a.r2_key.as_str()),
                        os(a.content_id.as_deref()),
                        b(a.is_inline),
                    ]
                })
                .collect();
            batch.push(self.stmt(&sql, &args)?);
        }
        if let Some(t) = &thread_id {
            batch.push(self.stmt(
                "UPDATE threads SET \
                   message_count = (SELECT count(*) FROM messages WHERE thread_id = ?1 AND deleted_at IS NULL), \
                   last_message_at = COALESCE((SELECT max(received_at) FROM messages WHERE thread_id = ?1 AND deleted_at IS NULL), last_message_at) \
                 WHERE id = ?1",
                &[s(t)],
            )?);
        }
        batch.push(self.stmt(
            "DELETE FROM ingest_failures WHERE raw_key = ?",
            &[s(m.raw_key.as_str())],
        )?);

        // `batch` é uma transação: ou grava tudo, ou nada.
        self.db.batch(batch).await.map_err(err("save"))?;
        Ok(())
    }
}

impl FailureLog for D1Index<'_> {
    async fn record(&self, raw_key: &str, error: &str, at: UnixMs) -> Result<(), PortError> {
        self.stmt(
            "INSERT INTO ingest_failures (raw_key, attempts, last_error, updated_at) VALUES (?, 1, ?, ?) \
             ON CONFLICT(raw_key) DO UPDATE SET attempts = attempts + 1, last_error = excluded.last_error, \
               updated_at = excluded.updated_at",
            &[s(raw_key), s(error), n(at.as_i64())],
        )?
        .run()
        .await
        .map_err(err("record_failure"))?;
        Ok(())
    }

    async fn give_up(
        &self,
        raw_key: &str,
        error: Option<&str>,
        at: UnixMs,
    ) -> Result<(), PortError> {
        self.stmt(
            "INSERT INTO ingest_failures (raw_key, attempts, last_error, updated_at, gave_up_at) VALUES (?, 1, ?, ?, ?) \
             ON CONFLICT(raw_key) DO UPDATE SET last_error = COALESCE(excluded.last_error, last_error), \
               updated_at = excluded.updated_at, gave_up_at = excluded.gave_up_at",
            &[s(raw_key), os(error), n(at.as_i64()), n(at.as_i64())],
        )?
        .run()
        .await
        .map_err(err("give_up"))?;
        Ok(())
    }
}

impl ReconcileIndex for D1Index<'_> {
    async fn handled_with_prefix(&self, prefix: &str) -> Result<HashSet<String>, PortError> {
        #[derive(Deserialize)]
        struct Row {
            raw_key: String,
        }
        // Faixa de prefixo usa o índice UNIQUE de raw_key (LIKE não usaria).
        let end = format!("{prefix}\u{10FFFF}");
        let rows: Vec<Row> = self
            .rows(
                "SELECT raw_key FROM messages WHERE raw_key >= ?1 AND raw_key < ?2 \
                 UNION SELECT raw_key FROM ingest_failures WHERE raw_key >= ?1 AND raw_key < ?2",
                &[s(prefix), s(&end)],
            )
            .await?;
        Ok(rows.into_iter().map(|r| r.raw_key).collect())
    }

    async fn pending_requests(&self) -> Result<Vec<ReprocessRequest>, PortError> {
        #[derive(Deserialize)]
        struct Row {
            id: f64,
            mode: String,
            raw_key: Option<String>,
            cursor: Option<String>,
        }
        let rows: Vec<Row> = self
            .rows(
                "SELECT id, mode, raw_key, cursor FROM reprocess_requests WHERE done_at IS NULL ORDER BY id LIMIT 10",
                &[],
            )
            .await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| {
                let mode = match r.mode.as_str() {
                    "key" => ReprocessMode::Key,
                    "failed" => ReprocessMode::Failed,
                    "all" => ReprocessMode::All,
                    _ => return None,
                };
                Some(ReprocessRequest {
                    id: r.id as i64,
                    mode,
                    raw_key: r.raw_key,
                    cursor: r.cursor,
                })
            })
            .collect())
    }

    async fn update_request(
        &self,
        id: i64,
        cursor: Option<&str>,
        enqueued: usize,
        done: bool,
    ) -> Result<(), PortError> {
        let done_at = done.then(|| worker::Date::now().as_millis() as i64);
        self.stmt(
            "UPDATE reprocess_requests SET cursor = ?, enqueued = enqueued + ?, done_at = ? WHERE id = ?",
            &[os(cursor), n(enqueued as i64), on(done_at), n(id)],
        )?
        .run()
        .await
        .map_err(err("update_request"))?;
        Ok(())
    }

    async fn given_up(&self, limit: usize) -> Result<Vec<String>, PortError> {
        #[derive(Deserialize)]
        struct Row {
            raw_key: String,
        }
        let rows: Vec<Row> = self
            .rows(
                "SELECT raw_key FROM ingest_failures WHERE gave_up_at IS NOT NULL ORDER BY updated_at LIMIT ?",
                &[n(limit as i64)],
            )
            .await?;
        Ok(rows.into_iter().map(|r| r.raw_key).collect())
    }

    async fn clear_failures(&self, raw_keys: &[String]) -> Result<(), PortError> {
        for chunk in raw_keys.chunks(100) {
            let sql = format!(
                "DELETE FROM ingest_failures WHERE raw_key IN ({})",
                placeholders(chunk.len())
            );
            let args: Vec<JsValue> = chunk.iter().map(|k| s(k)).collect();
            self.stmt(&sql, &args)?
                .run()
                .await
                .map_err(err("clear_failures"))?;
        }
        Ok(())
    }
}
