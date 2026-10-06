//! Processamento de um `.eml` da fila (docs seção 6.2): parse, anexos e
//! HTML no R2, thread, regras e índice. Idempotente: o `id` da mensagem vem
//! do `raw_key`, e anexos e thread nova têm IDs derivados dele.

use std::fmt;

use mailbox_domain::{
    normalize_subject, snippet, truncate_text_body, Attachment, AttachmentId, AttachmentKey,
    AuthResults, AuthVerdict, EmailAddress, Folder, HtmlKey, Mailbox, Message, MessageId,
    Processing, RawKey, Rule, Thread, ThreadId, UnixMs,
};
use mailbox_mime::ParsedMessage;

use crate::ports::{
    BlobStore, Clock, FailureLog, IdGenerator, IngestJob, MailIndex, MessageRecord, PortError,
};

/// Quem escreve o `Authentication-Results` e o `Received-SPF` em que
/// confiamos. Conferido num email real (2026-10-05): a Cloudflare adiciona
/// os dela acima dos que vêm com o remetente, então vale o primeiro com
/// este `authserv_id`/`receiver`.
pub const TRUSTED_AUTHSERV_ID: &str = "mx.cloudflare.net";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessError {
    /// Tentar de novo não resolve (chave inválida, raw ausente, domínio sem
    /// cadastro). Registrar a falha e não reenfileirar.
    Permanent(String),
    /// Infraestrutura (D1/R2). Tentar de novo.
    Transient(PortError),
}

impl fmt::Display for ProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Permanent(m) => write!(f, "permanente: {m}"),
            Self::Transient(e) => write!(f, "temporária: {e}"),
        }
    }
}

impl From<PortError> for ProcessError {
    fn from(e: PortError) -> Self {
        Self::Transient(e)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutcome {
    pub message_id: MessageId,
    pub processing: Processing,
    pub reprocessed: bool,
    /// Uma regra `reject` casou: a mensagem fica no índice, mas oculta.
    pub hidden_by_rule: bool,
}

pub struct ProcessMessage<'a, B, I, C, G> {
    pub blobs: &'a B,
    pub index: &'a I,
    pub clock: &'a C,
    pub ids: &'a G,
    /// Acima disso, modo leve (só headers).
    pub full_max_bytes: u64,
    /// Acima disso, anexos e HTML ficam só no `.eml` (texto ainda é
    /// indexado). Cada anexo é uma escrita no R2, e o plano Free permite 50
    /// sub-requisições por invocação.
    pub max_attachments: usize,
}

impl<B, I, C, G> ProcessMessage<'_, B, I, C, G>
where
    B: BlobStore,
    I: MailIndex,
    C: Clock,
    G: IdGenerator,
{
    pub async fn run(&self, job: &IngestJob) -> Result<ProcessOutcome, ProcessError> {
        let permanent = |m: String| ProcessError::Permanent(m);

        let raw_key = RawKey::parse(&job.raw_key).map_err(|e| permanent(e.to_string()))?;
        let domain = raw_key.domain();
        let domain_id = self
            .index
            .domain_id(domain)
            .await?
            .ok_or_else(|| permanent(format!("domínio sem cadastro: {domain}")))?;
        let envelope_to =
            EmailAddress::parse(&job.envelope_to).map_err(|e| permanent(e.to_string()))?;
        let raw = self
            .blobs
            .get(raw_key.as_str())
            .await?
            .ok_or_else(|| permanent(format!("raw ausente: {raw_key}")))?;

        let mut processing = Processing::for_size(raw.len() as u64, self.full_max_bytes);
        let parsed = match processing {
            Processing::Full => mailbox_mime::parse(&raw),
            Processing::Partial => mailbox_mime::parse_headers(&raw),
        }
        .map_err(|e| permanent(e.to_string()))?;
        if parsed.attachments.len() > self.max_attachments {
            processing = Processing::Partial;
        }

        let id = raw_key.message_id();
        let existing = self.index.existing(id).await?;
        let is_new = existing.is_none();
        let thread_id = match existing.and_then(|e| e.thread_id) {
            Some(t) => t,
            None => self.resolve_thread(id, &parsed).await?,
        };

        let now = self.clock.now();
        let identity_id = self
            .index
            .ensure_identity(domain_id, &envelope_to, self.ids.identity_id(now))
            .await?;

        // Regras só na primeira vez (reprocessar não desfaz o que o usuário
        // mudou) e só no que chega: o que enviamos vai para `sent`.
        let rules = if is_new && !job.sent {
            Rule::evaluate(&self.index.rules().await?, &envelope_to)
        } else {
            Default::default()
        };

        // Oculta por regra ou modo leve: nada de anexos e HTML no R2.
        let (attachments, html_key) = if rules.reject || processing == Processing::Partial {
            (Vec::new(), None)
        } else {
            self.store_bodies(&raw_key, &parsed).await?
        };

        let received_at = UnixMs::new(job.received_at).unwrap_or(id.timestamp());
        let text_body = parsed
            .text
            .as_deref()
            .map(|t| truncate_text_body(t).to_owned());
        let message = Message {
            id,
            raw_key: raw_key.clone(),
            domain_id,
            identity_id: Some(identity_id),
            thread_id: Some(thread_id),
            envelope_from: EmailAddress::parse(&job.envelope_from).ok(),
            envelope_to,
            message_id_hdr: parsed.message_id.clone(),
            in_reply_to: parsed.in_reply_to.first().cloned(),
            references: parsed.references.clone(),
            from: parsed.from.as_ref().and_then(mailbox),
            to: parsed.to.iter().filter_map(mailbox).collect(),
            cc: parsed.cc.iter().filter_map(mailbox).collect(),
            subject: parsed.subject.clone(),
            snippet: parsed.text.as_deref().and_then(snippet),
            text_body,
            html_key,
            has_attachments: !attachments.is_empty(),
            size_bytes: raw.len() as u64,
            received_at,
            date_hdr: parsed.date_ms.and_then(|ms| UnixMs::new(ms).ok()),
            folder: if job.sent {
                Folder::Sent
            } else {
                rules.folder.unwrap_or(Folder::Inbox)
            },
            is_read: job.sent || rules.mark_read,
            is_starred: false,
            // Num enviado, um `Authentication-Results` só poderia ter vindo de
            // quem escreveu a mensagem: não vale nada.
            auth: if job.sent {
                AuthResults::default()
            } else {
                trusted_auth(&parsed)
            },
            deleted_at: rules.reject.then_some(now),
        };
        let thread = Thread::new(
            thread_id,
            parsed.subject.as_deref().and_then(normalize_subject),
            received_at,
        );

        self.index
            .save(&MessageRecord {
                message,
                processing,
                attachments,
                thread,
                is_new,
            })
            .await?;

        Ok(ProcessOutcome {
            message_id: id,
            processing,
            reprocessed: !is_new,
            hidden_by_rule: rules.reject,
        })
    }

    /// `In-Reply-To`/`References` contra mensagens conhecidas; depois,
    /// respostas que chegaram antes desta; senão, thread nova.
    async fn resolve_thread(
        &self,
        id: MessageId,
        parsed: &ParsedMessage<'_>,
    ) -> Result<ThreadId, PortError> {
        let refs: Vec<String> = parsed
            .in_reply_to
            .iter()
            .chain(parsed.references.iter().rev())
            .cloned()
            .collect();
        if !refs.is_empty() {
            if let Some(t) = self.index.thread_of(&refs).await? {
                return Ok(t);
            }
        }
        if let Some(mid) = &parsed.message_id {
            if let Some(t) = self.index.thread_of_reply_to(mid).await? {
                return Ok(t);
            }
        }
        Ok(ThreadId::rooted_at(id))
    }

    /// Anexos e HTML sanitizado no R2. Chaves e IDs derivados da mensagem:
    /// reprocessar sobrescreve os mesmos objetos.
    async fn store_bodies(
        &self,
        raw_key: &RawKey,
        parsed: &ParsedMessage<'_>,
    ) -> Result<(Vec<Attachment>, Option<HtmlKey>), PortError> {
        let id = raw_key.message_id();
        let domain = raw_key.domain();
        let mut attachments = Vec::with_capacity(parsed.attachments.len());
        for a in &parsed.attachments {
            let key = AttachmentKey::new(domain, id, a.ordinal);
            self.blobs
                .put(key.as_str(), &a.data, &a.content_type)
                .await?;
            attachments.push(Attachment {
                id: AttachmentId::for_message(id, a.ordinal),
                message_id: id,
                filename: a.filename.clone(),
                content_type: Some(a.content_type.clone()),
                size_bytes: a.data.len() as u64,
                r2_key: key,
                content_id: a.content_id.clone(),
                is_inline: a.is_inline,
            });
        }

        let html_key = match &parsed.html {
            Some(html) => {
                let clean = mailbox_mime::sanitize_html(html, |cid| {
                    attachments
                        .iter()
                        .find(|a| a.content_id.as_deref() == Some(cid))
                        .map(|a| format!("/api/attachments/{}", a.id))
                });
                let key = HtmlKey::new(domain, id);
                self.blobs
                    .put(key.as_str(), clean.as_bytes(), "text/html; charset=utf-8")
                    .await?;
                Some(key)
            }
            None => None,
        };
        Ok((attachments, html_key))
    }
}

/// O que o consumer da fila faz com a mensagem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    Ack,
    Retry,
}

/// Processa um job e registra a falha: temporária vira nova tentativa
/// (até a DLQ); permanente desiste na hora. Nunca perde o `.eml`, que
/// continua no R2.
pub async fn handle_job<B, I, C, G, F>(
    uc: &ProcessMessage<'_, B, I, C, G>,
    failures: &F,
    job: &IngestJob,
) -> (Delivery, Result<ProcessOutcome, ProcessError>)
where
    B: BlobStore,
    I: MailIndex,
    C: Clock,
    G: IdGenerator,
    F: FailureLog,
{
    let result = uc.run(job).await;
    let now = uc.clock.now();
    let delivery = match &result {
        Ok(_) => Delivery::Ack,
        Err(ProcessError::Permanent(m)) => {
            // Se nem registrar der, tenta de novo para não sumir em silêncio.
            match failures.give_up(&job.raw_key, Some(m), now).await {
                Ok(()) => Delivery::Ack,
                Err(_) => Delivery::Retry,
            }
        }
        Err(ProcessError::Transient(e)) => {
            let _ = failures.record(&job.raw_key, &e.0, now).await;
            Delivery::Retry
        }
    };
    (delivery, result)
}

fn mailbox(a: &mailbox_mime::Address) -> Option<Mailbox> {
    Some(Mailbox {
        name: a.name.clone(),
        addr: EmailAddress::parse(&a.address).ok()?,
    })
}

/// DKIM e DMARC do primeiro `Authentication-Results` da Cloudflare. SPF do
/// primeiro `Received-SPF` dela (o do `Authentication-Results` é o do HELO);
/// sem ele, cai no do `Authentication-Results`.
fn trusted_auth(p: &ParsedMessage<'_>) -> AuthResults {
    let ar = p
        .auth_results
        .iter()
        .find(|a| a.authserv_id == TRUSTED_AUTHSERV_ID);
    let spf = p
        .received_spf
        .iter()
        .find(|r| r.receiver.as_deref() == Some(TRUSTED_AUTHSERV_ID))
        .map(|r| r.verdict.as_str())
        .or_else(|| ar.and_then(|a| a.spf.as_deref()));
    let verdict = |v: Option<&str>| v.and_then(AuthVerdict::parse);
    AuthResults {
        spf: verdict(spf),
        dkim: verdict(ar.and_then(|a| a.dkim.as_deref())),
        dmarc: verdict(ar.and_then(|a| a.dmarc.as_deref())),
    }
}

#[cfg(test)]
mod tests;
