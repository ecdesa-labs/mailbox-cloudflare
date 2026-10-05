use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use serde_json::json;

use super::*;

fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = pin!(f);
    match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("fake não deveria ficar pendente"),
    }
}

fn b64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..=c.len() {
            out.push(T[(n >> (18 - 6 * i)) as usize & 63] as char);
        }
    }
    out
}

/// "Assinatura" fake: `ok:<kid>` só vale para a chave `k1`.
struct Fake;
impl SignatureVerifier for Fake {
    async fn verify_rs256(
        &self,
        kid: &str,
        _input: &[u8],
        sig: &[u8],
    ) -> Result<bool, VerifyError> {
        match kid {
            "k1" => Ok(sig == b"ok:k1"),
            "fora" => Err(VerifyError::Port(PortError("certs fora".into()))),
            _ => Err(VerifyError::UnknownKey),
        }
    }
}

const NOW: i64 = 1_791_158_400_000;
const ISS: &str = "https://acmeco.cloudflareaccess.com";
const AUD: &str = "aud123";

fn cfg(allowed: &str) -> AccessConfig {
    AccessConfig::new("acmeco.cloudflareaccess.com", AUD, allowed).unwrap()
}

fn token(header: serde_json::Value, claims: serde_json::Value, sig: &[u8]) -> String {
    format!(
        "{}.{}.{}",
        b64(header.to_string().as_bytes()),
        b64(claims.to_string().as_bytes()),
        b64(sig)
    )
}

fn good_claims() -> serde_json::Value {
    json!({ "iss": ISS, "aud": [AUD], "exp": NOW / 1000 + 600, "nbf": NOW / 1000 - 10, "type": "app", "email": "Rosa@ACMECO.com.br" })
}

fn good_header() -> serde_json::Value {
    json!({ "alg": "RS256", "kid": "k1" })
}

fn auth(cfg: &AccessConfig, t: Option<&str>) -> Result<Identity, AuthError> {
    block_on(authenticate(cfg, &Fake, t, UnixMs::new(NOW).unwrap()))
}

fn with(f: impl FnOnce(&mut serde_json::Value)) -> String {
    let mut c = good_claims();
    f(&mut c);
    token(good_header(), c, b"ok:k1")
}

#[test]
fn token_valido() {
    let t = token(good_header(), good_claims(), b"ok:k1");
    assert_eq!(
        auth(&cfg(""), Some(&t)),
        Ok(Identity {
            email: "rosa@acmeco.com.br".into()
        })
    );
    assert!(auth(&cfg("rosa@acmeco.com.br"), Some(&t)).is_ok());
    // aud como string também vale
    assert!(auth(&cfg(""), Some(&with(|c| c["aud"] = json!(AUD)))).is_ok());
}

/// Token (ou ausência) e o erro esperado.
type Case = (Option<String>, fn(&AuthError) -> bool);

#[test]
fn recusa() {
    let cases: Vec<Case> = vec![
        (None, |e| *e == AuthError::Missing),
        (Some("".into()), |e| *e == AuthError::Missing),
        (Some("a.b".into()), |e| matches!(e, AuthError::Malformed(_))),
        (Some("@@.@@.@@".into()), |e| {
            matches!(e, AuthError::Malformed(_))
        }),
        (Some(token(good_header(), good_claims(), b"forjada")), |e| {
            *e == AuthError::BadSignature
        }),
        (
            Some(token(
                json!({"alg": "none", "kid": "k1"}),
                good_claims(),
                b"",
            )),
            |e| matches!(e, AuthError::Algorithm(_)),
        ),
        (
            Some(token(
                json!({"alg": "HS256", "kid": "k1"}),
                good_claims(),
                b"ok:k1",
            )),
            |e| matches!(e, AuthError::Algorithm(_)),
        ),
        (
            Some(token(
                json!({"alg": "RS256", "kid": "k9"}),
                good_claims(),
                b"ok:k1",
            )),
            |e| matches!(e, AuthError::UnknownKey(_)),
        ),
        (
            Some(token(
                json!({"alg": "RS256", "kid": "fora"}),
                good_claims(),
                b"ok:k1",
            )),
            |e| matches!(e, AuthError::Keys(_)),
        ),
        (
            Some(with(|c| {
                c["iss"] = json!("https://outro.cloudflareaccess.com")
            })),
            |e| *e == AuthError::WrongIssuer,
        ),
        (Some(with(|c| c["aud"] = json!(["outra"]))), |e| {
            *e == AuthError::WrongAudience
        }),
        (Some(with(|c| c["type"] = json!("org"))), |e| {
            matches!(e, AuthError::WrongType(_))
        }),
        (Some(with(|c| c["exp"] = json!(NOW / 1000 - 120))), |e| {
            *e == AuthError::Expired
        }),
        (Some(with(|c| c["nbf"] = json!(NOW / 1000 + 120))), |e| {
            *e == AuthError::NotYetValid
        }),
        (
            Some(with(|c| {
                c.as_object_mut().unwrap().remove("email");
            })),
            |e| matches!(e, AuthError::Malformed(_)),
        ),
    ];
    for (t, expected) in cases {
        let r = auth(&cfg(""), t.as_deref());
        assert!(r.as_ref().is_err_and(expected), "{t:?} -> {r:?}");
    }
}

#[test]
fn email_fora_da_lista() {
    let t = token(good_header(), good_claims(), b"ok:k1");
    assert_eq!(
        auth(&cfg("outra@acmeco.com.br"), Some(&t)),
        Err(AuthError::NotAllowed("rosa@acmeco.com.br".into()))
    );
}

#[test]
fn tolerancia_de_relogio() {
    assert!(auth(&cfg(""), Some(&with(|c| c["exp"] = json!(NOW / 1000 - 30)))).is_ok());
}

#[test]
fn config() {
    assert_eq!(AccessConfig::new("", "x", ""), None);
    assert_eq!(AccessConfig::new("t.cloudflareaccess.com", " ", ""), None);
    let c = AccessConfig::new("http://127.0.0.1:9/", "x", " A@b.com , ").unwrap();
    assert_eq!(c.issuer, "http://127.0.0.1:9");
    assert_eq!(c.certs_url(), "http://127.0.0.1:9/cdn-cgi/access/certs");
    assert_eq!(c.allowed_emails, ["a@b.com"]);
}

#[test]
fn base64url() {
    assert_eq!(base64url_decode("aGVsbG8").unwrap(), b"hello");
    assert_eq!(base64url_decode("-_8").unwrap(), [0xfb, 0xff]);
    assert_eq!(base64url_decode("aGVsbG8=").unwrap(), b"hello");
    assert!(base64url_decode("a+b/").is_none());
    assert!(base64url_decode("a").is_none());
    for n in 0..20u8 {
        let bytes: Vec<u8> = (0..n).map(|i| i.wrapping_mul(37)).collect();
        assert_eq!(base64url_decode(&b64(&bytes)).unwrap(), bytes);
    }
}
