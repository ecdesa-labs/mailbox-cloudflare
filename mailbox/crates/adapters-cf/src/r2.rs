use std::collections::HashMap;

use mailbox_application::{
    BlobStore, IngestJob, PortError, RawListing, RawMailStore, RawObject, RawPage,
};
use mailbox_domain::{RawKey, UnixMs};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use worker::{Bucket, ForwardableEmailMessage, HttpMetadata, Include};

// `FixedLengthStream` do runtime: TransformStream com tamanho declarado.
// O R2 só aceita stream de tamanho conhecido; o do email não tem.
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = web_sys::TransformStream, js_name = FixedLengthStream)]
    type JsFixedLengthStream;
    #[wasm_bindgen(constructor, js_class = "FixedLengthStream")]
    fn new(length: f64) -> JsFixedLengthStream;
}

/// Grava o `.eml` direto do stream do email para o R2: os bytes nunca
/// entram no wasm (docs seção 6.1). Nunca usar `raw_bytes()` aqui.
pub struct EmailRawStore<'a> {
    pub message: &'a ForwardableEmailMessage,
    pub bucket: &'a Bucket,
}

impl RawMailStore for EmailRawStore<'_> {
    async fn store(&self, job: &IngestJob) -> Result<(), PortError> {
        let fls = JsFixedLengthStream::new(self.message.raw_size());
        let pipe = self.message.raw().pipe_to(&fls.writable());

        let put = self
            .bucket
            .put(&job.raw_key, fls.readable())
            .http_metadata(HttpMetadata {
                content_type: Some("message/rfc822".into()),
                ..Default::default()
            })
            .custom_metadata(envelope_metadata(job))
            .execute()
            .await;
        let piped = JsFuture::from(pipe).await;

        put.map_err(|e| PortError(format!("r2 put {}: {e}", job.raw_key)))?;
        piped.map_err(|e| PortError(format!("stream do email: {e:?}")))?;
        Ok(())
    }
}

/// O envelope não está no `.eml`; vai no objeto para a reconciliação
/// (docs seção 6.6) refazer o job só a partir do R2.
fn envelope_metadata(job: &IngestJob) -> HashMap<String, String> {
    HashMap::from([
        ("envelope-from".into(), job.envelope_from.clone()),
        ("envelope-to".into(), job.envelope_to.clone()),
        ("received-at".into(), job.received_at.to_string()),
    ])
}

/// Objetos do R2 por chave (raw, anexos, HTML).
pub struct R2Blobs<'a> {
    pub bucket: &'a Bucket,
}

impl BlobStore for R2Blobs<'_> {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, PortError> {
        let err = |e: worker::Error| PortError(format!("r2 get {key}: {e}"));
        let Some(obj) = self.bucket.get(key).execute().await.map_err(err)? else {
            return Ok(None);
        };
        match obj.body() {
            Some(body) => body.bytes().await.map(Some).map_err(err),
            None => Ok(Some(Vec::new())),
        }
    }

    async fn put(&self, key: &str, data: &[u8], content_type: &str) -> Result<(), PortError> {
        self.bucket
            .put(key, data.to_vec())
            .http_metadata(HttpMetadata {
                content_type: Some(content_type.into()),
                ..Default::default()
            })
            .execute()
            .await
            .map(|_| ())
            .map_err(|e| PortError(format!("r2 put {key}: {e}")))
    }
}

impl RawListing for R2Blobs<'_> {
    async fn list(
        &self,
        prefix: &str,
        cursor: Option<&str>,
        with_jobs: bool,
    ) -> Result<RawPage, PortError> {
        // Com metadados, páginas menores: ler os metadados custa CPU no wasm.
        let mut req = self
            .bucket
            .list()
            .prefix(prefix)
            .limit(if with_jobs { 100 } else { 1000 });
        if let Some(c) = cursor {
            req = req.cursor(c);
        }
        if with_jobs {
            req = req.include(vec![Include::CustomMetadata]);
        }
        let page = req
            .execute()
            .await
            .map_err(|e| PortError(format!("r2 list {prefix}: {e}")))?;
        let objects = page
            .objects()
            .into_iter()
            .filter(|o| o.key().ends_with(".eml"))
            .map(|o| {
                let key = o.key();
                let job = with_jobs
                    .then(|| {
                        o.custom_metadata()
                            .ok()
                            .and_then(|m| job_from_metadata(&key, o.size(), &m))
                    })
                    .flatten();
                RawObject {
                    uploaded_at: UnixMs::new(o.uploaded().as_millis() as i64)
                        .unwrap_or(UnixMs::new(0).unwrap()),
                    key,
                    job,
                }
            })
            .collect();
        Ok(RawPage {
            objects,
            cursor: page.truncated().then(|| page.cursor()).flatten(),
        })
    }

    async fn job_for(&self, key: &str) -> Result<Option<IngestJob>, PortError> {
        let Some(obj) = self
            .bucket
            .head(key)
            .await
            .map_err(|e| PortError(format!("r2 head {key}: {e}")))?
        else {
            return Ok(None);
        };
        let meta = obj.custom_metadata().unwrap_or_default();
        Ok(job_from_metadata(key, obj.size(), &meta))
    }
}

/// Refaz o job da fila a partir dos metadados gravados no ingest. Sem
/// `envelope-to` não dá para saber por qual endereço o email entrou.
fn job_from_metadata(key: &str, size: u64, meta: &HashMap<String, String>) -> Option<IngestJob> {
    let raw_key = RawKey::parse(key).ok()?;
    let envelope_to = meta.get("envelope-to")?.clone();
    let received_at = meta
        .get("received-at")
        .and_then(|v| v.parse().ok())
        .unwrap_or(raw_key.message_id().timestamp().as_i64());
    Some(IngestJob {
        raw_key: key.to_owned(),
        domain: raw_key.domain().to_string(),
        envelope_from: meta.get("envelope-from").cloned().unwrap_or_default(),
        envelope_to,
        received_at,
        size,
    })
}
