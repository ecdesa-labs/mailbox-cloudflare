//! Consultas de leitura da API no D1 (docs seção 7).

use mailbox_application::{
    AttachmentMeta, DomainSummary, IdentitySummary, ListItem, ListQuery, MessageDetail, Person,
    PortError, ReadModel, ThreadView,
};
use mailbox_domain::{AttachmentId, Folder, MessageId, Processing, ThreadId, UnixMs};
use serde::Deserialize;
use wasm_bindgen::JsValue;

use crate::D1Index;

/// Colunas do item da lista; `m` = messages, `d` = domains, `i` = identities.
const ITEM_COLUMNS: &str =
    "m.id, m.thread_id, d.name AS domain, i.address AS identity, m.from_addr, \
    m.from_name, m.subject, m.snippet, m.received_at, m.is_read, m.is_starred, m.has_attachments, \
    m.folder, m.processing";
const ITEM_FROM: &str =
    "FROM messages m JOIN domains d ON d.id = m.domain_id LEFT JOIN identities i ON i.id = m.identity_id";

#[derive(Deserialize)]
struct ItemRow {
    id: String,
    thread_id: Option<String>,
    domain: String,
    identity: Option<String>,
    from_addr: Option<String>,
    from_name: Option<String>,
    subject: Option<String>,
    snippet: Option<String>,
    received_at: f64,
    is_read: f64,
    is_starred: f64,
    has_attachments: f64,
    folder: String,
    processing: String,
}

impl ItemRow {
    fn into_item(self) -> Option<ListItem> {
        Some(ListItem {
            id: MessageId::parse(&self.id).ok()?,
            thread_id: self.thread_id.and_then(|t| ThreadId::parse(&t).ok()),
            domain: self.domain,
            identity: self.identity,
            from: self.from_addr.map(|addr| Person {
                name: self.from_name,
                addr,
            }),
            subject: self.subject,
            snippet: self.snippet,
            received_at: UnixMs::new(self.received_at as i64).ok()?,
            is_read: self.is_read != 0.0,
            is_starred: self.is_starred != 0.0,
            has_attachments: self.has_attachments != 0.0,
            folder: self.folder.parse::<Folder>().ok()?,
            processing: if self.processing == "partial" {
                Processing::Partial
            } else {
                Processing::Full
            },
        })
    }
}

#[derive(Deserialize)]
struct AttachmentRow {
    id: String,
    message_id: String,
    filename: Option<String>,
    content_type: Option<String>,
    size_bytes: f64,
    content_id: Option<String>,
    is_inline: f64,
    r2_key: String,
}

impl AttachmentRow {
    fn into_meta(self) -> Option<AttachmentMeta> {
        Some(AttachmentMeta {
            id: AttachmentId::parse(&self.id).ok()?,
            message_id: MessageId::parse(&self.message_id).ok()?,
            filename: self.filename,
            content_type: self
                .content_type
                .unwrap_or_else(|| "application/octet-stream".into()),
            size_bytes: self.size_bytes as u64,
            content_id: self.content_id,
            is_inline: self.is_inline != 0.0,
            r2_key: self.r2_key,
        })
    }
}

const ATTACHMENT_COLUMNS: &str =
    "id, message_id, filename, content_type, size_bytes, content_id, is_inline, r2_key";

/// `to_addrs`/`cc_addrs` gravados como `[{"name":..,"addr":..}]`.
fn people(json: Option<&str>) -> Vec<Person> {
    #[derive(Deserialize)]
    struct P {
        name: Option<String>,
        addr: String,
    }
    json.and_then(|j| serde_json::from_str::<Vec<P>>(j).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|p| Person {
            name: p.name,
            addr: p.addr,
        })
        .collect()
}

fn s(v: &str) -> JsValue {
    JsValue::from_str(v)
}
fn n(v: i64) -> JsValue {
    JsValue::from_f64(v as f64)
}
fn err(ctx: &'static str) -> impl Fn(worker::Error) -> PortError {
    move |e| PortError(format!("d1 {ctx}: {e}"))
}

impl D1Index<'_> {
    pub(crate) async fn query<T: for<'de> Deserialize<'de>>(
        &self,
        ctx: &'static str,
        sql: &str,
        args: &[JsValue],
    ) -> Result<Vec<T>, PortError> {
        self.db
            .prepare(sql)
            .bind(args)
            .map_err(err(ctx))?
            .all()
            .await
            .map_err(err(ctx))?
            .results::<T>()
            .map_err(err(ctx))
    }
}

impl ReadModel for D1Index<'_> {
    async fn list(&self, q: &ListQuery, limit: u32) -> Result<Vec<ListItem>, PortError> {
        let mut sql = format!("SELECT {ITEM_COLUMNS} {ITEM_FROM} WHERE m.deleted_at IS NULL");
        let mut args = Vec::new();
        if let Some(f) = q.folder {
            sql.push_str(" AND m.folder = ?");
            args.push(s(f.as_str()));
        }
        // `search` já vem montado por `fts_query`: só termos entre aspas.
        if let Some(expr) = &q.search {
            sql.push_str(
                " AND m.seq IN (SELECT rowid FROM messages_fts WHERE messages_fts MATCH ?)",
            );
            args.push(s(expr));
        }
        if let Some(d) = &q.domain {
            sql.push_str(" AND d.name = ?");
            args.push(s(d.as_str()));
        }
        if let Some(c) = &q.after {
            sql.push_str(" AND (m.received_at < ? OR (m.received_at = ? AND m.id < ?))");
            args.extend([n(c.received_at), n(c.received_at), s(&c.id.to_string())]);
        }
        sql.push_str(" ORDER BY m.received_at DESC, m.id DESC LIMIT ?");
        args.push(n(limit as i64));
        let rows: Vec<ItemRow> = self.query("list", &sql, &args).await?;
        Ok(rows.into_iter().filter_map(ItemRow::into_item).collect())
    }

    async fn message(&self, id: MessageId) -> Result<Option<MessageDetail>, PortError> {
        #[derive(Deserialize)]
        struct Row {
            #[serde(flatten)]
            item: ItemRow,
            to_addrs: Option<String>,
            cc_addrs: Option<String>,
            date_hdr: Option<f64>,
            message_id_hdr: Option<String>,
            envelope_to: String,
            size_bytes: f64,
            text_body: Option<String>,
            html_key: Option<String>,
            raw_key: String,
            spf: Option<String>,
            dkim: Option<String>,
            dmarc: Option<String>,
        }
        let sql = format!(
            "SELECT {ITEM_COLUMNS}, m.to_addrs, m.cc_addrs, m.date_hdr, m.message_id_hdr, m.envelope_to, \
             m.size_bytes, m.text_body, m.html_key, m.raw_key, m.spf, m.dkim, m.dmarc \
             {ITEM_FROM} WHERE m.id = ? AND m.deleted_at IS NULL"
        );
        let id_s = id.to_string();
        let Some(r) = self
            .query::<Row>("message", &sql, &[s(&id_s)])
            .await?
            .into_iter()
            .next()
        else {
            return Ok(None);
        };
        let attachments: Vec<AttachmentRow> = self
            .query(
                "attachments",
                &format!("SELECT {ATTACHMENT_COLUMNS} FROM attachments WHERE message_id = ? ORDER BY r2_key"),
                &[s(&id_s)],
            )
            .await?;
        let Some(item) = r.item.into_item() else {
            return Ok(None);
        };
        Ok(Some(MessageDetail {
            item,
            to: people(r.to_addrs.as_deref()),
            cc: people(r.cc_addrs.as_deref()),
            date_hdr: r.date_hdr.and_then(|d| UnixMs::new(d as i64).ok()),
            message_id_hdr: r.message_id_hdr,
            envelope_to: r.envelope_to,
            size_bytes: r.size_bytes as u64,
            text_body: r.text_body,
            html_key: r.html_key,
            raw_key: r.raw_key,
            spf: r.spf,
            dkim: r.dkim,
            dmarc: r.dmarc,
            attachments: attachments
                .into_iter()
                .filter_map(AttachmentRow::into_meta)
                .collect(),
        }))
    }

    async fn thread(&self, id: ThreadId) -> Result<Option<ThreadView>, PortError> {
        #[derive(Deserialize)]
        struct T {
            subject_norm: Option<String>,
        }
        let id_s = id.to_string();
        let Some(t) = self
            .query::<T>(
                "thread",
                "SELECT subject_norm FROM threads WHERE id = ?",
                &[s(&id_s)],
            )
            .await?
            .into_iter()
            .next()
        else {
            return Ok(None);
        };
        let rows: Vec<ItemRow> = self
            .query(
                "thread_messages",
                &format!(
                    "SELECT {ITEM_COLUMNS} {ITEM_FROM} WHERE m.thread_id = ? AND m.deleted_at IS NULL \
                     ORDER BY m.received_at, m.id"
                ),
                &[s(&id_s)],
            )
            .await?;
        Ok(Some(ThreadView {
            id,
            subject_norm: t.subject_norm,
            messages: rows.into_iter().filter_map(ItemRow::into_item).collect(),
        }))
    }

    async fn attachment(&self, id: AttachmentId) -> Result<Option<AttachmentMeta>, PortError> {
        // Anexo de mensagem oculta/excluída não é servido.
        let rows: Vec<AttachmentRow> = self
            .query(
                "attachment",
                "SELECT a.id, a.message_id, a.filename, a.content_type, a.size_bytes, a.content_id, \
                   a.is_inline, a.r2_key \
                 FROM attachments a JOIN messages m ON m.id = a.message_id \
                 WHERE a.id = ? AND m.deleted_at IS NULL",
                &[s(&id.to_string())],
            )
            .await?;
        Ok(rows.into_iter().next().and_then(AttachmentRow::into_meta))
    }

    async fn domains(&self) -> Result<Vec<DomainSummary>, PortError> {
        #[derive(Deserialize)]
        struct D {
            id: String,
            name: String,
            label: String,
            color: String,
            unread: f64,
        }
        #[derive(Deserialize)]
        struct I {
            id: String,
            domain_id: String,
            address: String,
            label: Option<String>,
            is_default_from: f64,
        }
        let domains: Vec<D> = self
            .query(
                "domains",
                "SELECT d.id, d.name, d.label, d.color, \
                   (SELECT count(*) FROM messages m WHERE m.domain_id = d.id AND m.folder = 'inbox' \
                      AND m.is_read = 0 AND m.deleted_at IS NULL) AS unread \
                 FROM domains d ORDER BY d.name",
                &[],
            )
            .await?;
        let identities: Vec<I> = self
            .query(
                "identities",
                "SELECT id, domain_id, address, label, is_default_from FROM identities ORDER BY address",
                &[],
            )
            .await?;
        Ok(domains
            .into_iter()
            .map(|d| DomainSummary {
                identities: identities
                    .iter()
                    .filter(|i| i.domain_id == d.id)
                    .map(|i| IdentitySummary {
                        id: i.id.clone(),
                        address: i.address.clone(),
                        label: i.label.clone(),
                        is_default_from: i.is_default_from != 0.0,
                    })
                    .collect(),
                id: d.id,
                name: d.name,
                label: d.label,
                color: d.color,
                unread: d.unread as u64,
            })
            .collect())
    }
}
