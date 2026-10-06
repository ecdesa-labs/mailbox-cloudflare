//! Consultas do envio (docs seção 6.4) no D1.

use mailbox_application::{PortError, ReplyTarget, SendIndex, Sender};
use mailbox_domain::{DomainName, EmailAddress, IdentityId, MessageId, ThreadId};
use serde::Deserialize;
use wasm_bindgen::JsValue;

use crate::D1Index;

#[derive(Deserialize)]
struct SenderRow {
    address: String,
    label: Option<String>,
    domain: String,
}

#[derive(Deserialize)]
struct TargetRow {
    domain: String,
    message_id_hdr: Option<String>,
    references_hdr: Option<String>,
    thread_id: Option<String>,
}

impl SendIndex for D1Index<'_> {
    async fn sender(&self, id: IdentityId) -> Result<Option<Sender>, PortError> {
        let rows: Vec<SenderRow> = self
            .query(
                "sender",
                "SELECT i.address, i.label, d.name AS domain \
                 FROM identities i JOIN domains d ON d.id = i.domain_id WHERE i.id = ?",
                &[JsValue::from_str(&id.to_string())],
            )
            .await?;
        let Some(r) = rows.into_iter().next() else {
            return Ok(None);
        };
        let bad = |e: mailbox_domain::DomainError| PortError(format!("identidade {id}: {e}"));
        Ok(Some(Sender {
            address: EmailAddress::parse(&r.address).map_err(bad)?,
            name: r.label.filter(|l| !l.trim().is_empty()),
            domain: DomainName::parse(&r.domain).map_err(bad)?,
        }))
    }

    async fn reply_target(&self, id: MessageId) -> Result<Option<ReplyTarget>, PortError> {
        let rows: Vec<TargetRow> = self
            .query(
                "reply_target",
                "SELECT d.name AS domain, m.message_id_hdr, m.references_hdr, m.thread_id \
                 FROM messages m JOIN domains d ON d.id = m.domain_id \
                 WHERE m.id = ? AND m.deleted_at IS NULL",
                &[JsValue::from_str(&id.to_string())],
            )
            .await?;
        let Some(r) = rows.into_iter().next() else {
            return Ok(None);
        };
        Ok(Some(ReplyTarget {
            domain: DomainName::parse(&r.domain)
                .map_err(|e| PortError(format!("mensagem {id}: {e}")))?,
            message_id_hdr: r.message_id_hdr,
            // Gravado como lista JSON (d1.rs).
            references: r
                .references_hdr
                .and_then(|j| serde_json::from_str(&j).ok())
                .unwrap_or_default(),
            thread_id: r.thread_id.and_then(|t| ThreadId::parse(&t).ok()),
        }))
    }
}
