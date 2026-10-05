use std::collections::HashMap;

use mailbox_application::{IngestJob, PortError, RawMailStore};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use worker::{Bucket, ForwardableEmailMessage, HttpMetadata};

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
