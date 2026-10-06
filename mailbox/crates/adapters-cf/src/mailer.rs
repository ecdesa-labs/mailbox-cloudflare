//! Cloudflare Email Sending pelo binding `send_email` (docs seção 6.4).
//! Erros vêm como `Error` do JS com `code` (`E_SENDER_NOT_VERIFIED`,
//! `E_RATE_LIMIT_EXCEEDED`...), conferido na documentação em 2026-10-06.

use js_sys::{Array, JsString, Object, Reflect};
use mailbox_application::{Mailer, MailerError, OutgoingMail};
use mailbox_domain::Mailbox;
use wasm_bindgen::{JsCast, JsValue};
use worker::{EmailAddress, SendEmail, SendEmailBuilder};

pub struct CfMailer<'a> {
    pub binding: &'a SendEmail,
}

fn address(m: &Mailbox) -> EmailAddress {
    EmailAddress::new(m.name.as_deref().unwrap_or(""), m.addr.as_str())
}

fn list(items: &[Mailbox]) -> Array {
    items.iter().map(|m| JsValue::from(address(m))).collect()
}

fn set(target: &JsValue, key: &str, value: &JsValue) -> Result<(), MailerError> {
    Reflect::set(target, &JsValue::from_str(key), value)
        .map(|_| ())
        .map_err(|e| MailerError::Unavailable(format!("montar envio ({key}): {e:?}")))
}

fn classify(e: &JsValue) -> MailerError {
    let field = |k: &str| {
        Reflect::get(e, &JsValue::from_str(k))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default()
    };
    let (code, message) = (field("code"), field("message"));
    let message = if message.is_empty() {
        format!("{e:?}")
    } else {
        message
    };
    match code.as_str() {
        "" => MailerError::Unavailable(message),
        c if c.contains("RATE_LIMIT") || c.contains("QUOTA") => {
            MailerError::RateLimited(format!("{c}: {message}"))
        }
        // Erro interno do serviço: não se sabe se saiu.
        c if c.contains("INTERNAL") => MailerError::Unavailable(format!("{c}: {message}")),
        _ => MailerError::Rejected { code, message },
    }
}

impl Mailer for CfMailer<'_> {
    async fn send(&self, mail: &OutgoingMail) -> Result<String, MailerError> {
        // `to`/`cc` como `{name, email}` para levar o nome (o builder do
        // workers-rs só aceita lista de strings).
        let builder: SendEmailBuilder = SendEmailBuilder::builder_with_email_address_and_str(
            &address(&mail.from),
            "",
            &mail.subject,
        )
        .build();
        set(&builder, "to", &list(&mail.to))?;
        if !mail.cc.is_empty() {
            set(&builder, "cc", &list(&mail.cc))?;
        }
        if !mail.text.is_empty() {
            builder.set_text(&mail.text);
        }
        if let Some(html) = &mail.html {
            builder.set_html(html);
        }

        let headers = Object::new();
        if let Some(r) = &mail.in_reply_to {
            set(&headers, "In-Reply-To", &format!("<{r}>").into())?;
        }
        if !mail.references.is_empty() {
            let refs: Vec<String> = mail.references.iter().map(|r| format!("<{r}>")).collect();
            set(&headers, "References", &refs.join(" ").into())?;
        }
        if Object::keys(&headers).length() > 0 {
            builder.set_headers(headers.unchecked_ref::<Object<JsString>>());
        }

        self.binding
            .send_with_builder(&builder)
            .await
            .map(|r| r.message_id())
            .map_err(|e| classify(&e.into()))
    }
}
