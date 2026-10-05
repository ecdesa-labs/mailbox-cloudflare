//! Verificação RS256 do JWT do Access com o WebCrypto do runtime e as
//! chaves públicas do time (`/cdn-cgi/access/certs`).
//!
//! Cache por isolate: as chaves ficam importadas por até 1 h. `kid`
//! desconhecido força nova busca, no máximo uma vez por minuto (senão um
//! token com `kid` aleatório viraria uma busca por requisição).

use std::cell::RefCell;
use std::collections::HashMap;

use mailbox_application::{PortError, SignatureVerifier, VerifyError};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::CryptoKey;

const MAX_AGE_MS: u64 = 60 * 60 * 1000;
const MIN_REFRESH_MS: u64 = 60 * 1000;

#[derive(Default)]
struct KeyCache {
    url: String,
    keys: HashMap<String, CryptoKey>,
    fetched_at: u64,
}

thread_local! {
    static CACHE: RefCell<KeyCache> = RefCell::new(KeyCache::default());
}

pub struct JwksVerifier<'a> {
    pub certs_url: &'a str,
}

fn subtle() -> web_sys::SubtleCrypto {
    js_sys::Reflect::get(&js_sys::global(), &"crypto".into())
        .expect("globalThis.crypto existe no Workers")
        .unchecked_into::<web_sys::Crypto>()
        .subtle()
}

fn algorithm(with_hash: bool) -> js_sys::Object {
    let o = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&o, &"name".into(), &"RSASSA-PKCS1-v1_5".into());
    if with_hash {
        let _ = js_sys::Reflect::set(&o, &"hash".into(), &"SHA-256".into());
    }
    o
}

fn port(ctx: &str) -> impl Fn(JsValue) -> VerifyError + '_ {
    move |e| VerifyError::Port(PortError(format!("{ctx}: {e:?}")))
}

impl JwksVerifier<'_> {
    fn cached(&self, kid: &str, now: u64) -> Option<CryptoKey> {
        CACHE.with(|c| {
            let c = c.borrow();
            (c.url == self.certs_url && now.saturating_sub(c.fetched_at) < MAX_AGE_MS)
                .then(|| c.keys.get(kid).cloned())
                .flatten()
        })
    }

    fn may_refresh(&self, now: u64) -> bool {
        CACHE.with(|c| {
            let c = c.borrow();
            c.url != self.certs_url || now.saturating_sub(c.fetched_at) >= MIN_REFRESH_MS
        })
    }

    async fn refresh(&self, now: u64) -> Result<(), VerifyError> {
        let url = worker::Url::parse(self.certs_url)
            .map_err(|e| VerifyError::Port(PortError(format!("certs url: {e}"))))?;
        let mut resp = worker::Fetch::Url(url)
            .send()
            .await
            .map_err(|e| VerifyError::Port(PortError(format!("certs fetch: {e}"))))?;
        if resp.status_code() != 200 {
            return Err(VerifyError::Port(PortError(format!(
                "certs status {}",
                resp.status_code()
            ))));
        }
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| VerifyError::Port(PortError(format!("certs json: {e}"))))?;

        let mut keys = HashMap::new();
        for jwk in body
            .get("keys")
            .and_then(|k| k.as_array())
            .into_iter()
            .flatten()
        {
            let (Some(kid), Some("RSA")) = (
                jwk.get("kid").and_then(|v| v.as_str()),
                jwk.get("kty").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            // Só os campos públicos que o importKey precisa.
            let public = serde_json::json!({ "kty": "RSA", "n": jwk.get("n"), "e": jwk.get("e"), "alg": "RS256", "ext": true });
            let jwk_obj = js_sys::JSON::parse(&public.to_string()).map_err(port("jwk"))?;
            let usages = js_sys::Array::of1(&"verify".into());
            let promise = subtle()
                .import_key_with_object(
                    "jwk",
                    jwk_obj.unchecked_ref(),
                    &algorithm(true),
                    false,
                    &usages,
                )
                .map_err(port("importKey"))?;
            match JsFuture::from(promise).await {
                Ok(k) => {
                    keys.insert(kid.to_owned(), k.unchecked_into::<CryptoKey>());
                }
                Err(e) => worker::console_error!("chave {kid} do Access ignorada: {e:?}"),
            }
        }
        CACHE.with(|c| {
            *c.borrow_mut() = KeyCache {
                url: self.certs_url.to_owned(),
                keys,
                fetched_at: now,
            }
        });
        Ok(())
    }
}

impl SignatureVerifier for JwksVerifier<'_> {
    async fn verify_rs256(
        &self,
        kid: &str,
        signing_input: &[u8],
        signature: &[u8],
    ) -> Result<bool, VerifyError> {
        let now = worker::Date::now().as_millis();
        let key = match self.cached(kid, now) {
            Some(k) => k,
            None if self.may_refresh(now) => {
                self.refresh(now).await?;
                self.cached(kid, now).ok_or(VerifyError::UnknownKey)?
            }
            None => return Err(VerifyError::UnknownKey),
        };
        let promise = subtle()
            .verify_with_object_and_u8_array_and_u8_array(
                &algorithm(false),
                &key,
                signature,
                signing_input,
            )
            .map_err(port("verify"))?;
        let ok = JsFuture::from(promise).await.map_err(port("verify"))?;
        Ok(ok.as_bool().unwrap_or(false))
    }
}
