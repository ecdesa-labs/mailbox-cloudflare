//! Entidades. Espelham as tabelas da migração `0001_init.sql`, com tipos
//! de domínio no lugar de strings soltas.

use crate::{
    AttachmentId, AttachmentKey, AuthResults, DomainId, DomainName, EmailAddress, Folder, HtmlKey,
    IdentityId, Mailbox, MessageId, RawKey, ThreadId, UnixMs,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Domain {
    pub id: DomainId,
    pub name: DomainName,
    pub label: String,
    /// Token de cor para o front.
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub id: IdentityId,
    pub domain_id: DomainId,
    pub address: EmailAddress,
    pub label: Option<String>,
    pub is_default_from: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    pub id: ThreadId,
    pub subject_norm: Option<String>,
    pub last_message_at: UnixMs,
    pub message_count: u32,
}

impl Thread {
    pub fn new(id: ThreadId, subject_norm: Option<String>, first_at: UnixMs) -> Self {
        Self {
            id,
            subject_norm,
            last_message_at: first_at,
            message_count: 1,
        }
    }

    /// Conta uma mensagem nova. Mensagem atrasada não faz a thread voltar no tempo.
    pub fn add_message(&mut self, at: UnixMs) {
        self.message_count += 1;
        self.last_message_at = self.last_message_at.max(at);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: MessageId,
    pub raw_key: RawKey,
    pub domain_id: DomainId,
    pub identity_id: Option<IdentityId>,
    pub thread_id: Option<ThreadId>,
    /// `None` para bounce (`MAIL FROM:<>`).
    pub envelope_from: Option<EmailAddress>,
    /// Endereço pelo qual entrou (não o header To).
    pub envelope_to: EmailAddress,
    pub message_id_hdr: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub from: Option<Mailbox>,
    pub to: Vec<Mailbox>,
    pub cc: Vec<Mailbox>,
    pub subject: Option<String>,
    pub snippet: Option<String>,
    /// Já cortado em `TEXT_BODY_MAX_BYTES`.
    pub text_body: Option<String>,
    pub html_key: Option<HtmlKey>,
    pub has_attachments: bool,
    pub size_bytes: u64,
    pub received_at: UnixMs,
    pub date_hdr: Option<UnixMs>,
    pub folder: Folder,
    pub is_read: bool,
    pub is_starred: bool,
    pub auth: AuthResults,
    pub deleted_at: Option<UnixMs>,
}

impl Message {
    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub id: AttachmentId,
    pub message_id: MessageId,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub size_bytes: u64,
    pub r2_key: AttachmentKey,
    /// Para reescrever `cid:xxx` no HTML.
    pub content_id: Option<String>,
    pub is_inline: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: i64) -> UnixMs {
        UnixMs::new(ms).unwrap()
    }

    #[test]
    fn thread_conta_e_avanca() {
        let id = ThreadId::from_parts(at(0), 1);
        let mut t = Thread::new(id, Some("fatura".into()), at(100));
        t.add_message(at(300));
        t.add_message(at(200)); // chegou atrasada
        assert_eq!(t.message_count, 3);
        assert_eq!(t.last_message_at, at(300));
    }
}
