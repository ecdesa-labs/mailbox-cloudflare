//! Validação do JWT do Cloudflare Access (docs seção 6.5). A assinatura
//! RS256 é verificada por uma porta (WebCrypto no Worker); o resto é puro.
//!
//! Defesa em profundidade: o Access já barra quem não passou pela política
//! na borda. Aqui conferimos que o token é do nosso time (`iss`), da nossa
//! aplicação (`aud`), válido no tempo e, se configurado, de um email permitido.

use std::fmt;

use mailbox_domain::UnixMs;
use serde_json::Value;

use crate::ports::PortError;

/// Tolerância de relógio para `exp`/`nbf`.
const CLOCK_SKEW_MS: i64 = 60_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessConfig {
    /// `https://<time>.cloudflareaccess.com` (sem barra final). É o `iss`
    /// esperado e a base de `/cdn-cgi/access/certs`.
    pub issuer: String,
    /// Application Audience (AUD) Tag.
    pub aud: String,
    /// Vazio = qualquer identidade que a política do Access deixou passar.
    pub allowed_emails: Vec<String>,
}

impl AccessConfig {
    /// `team` aceita `acmeco.cloudflareaccess.com` ou uma URL completa
    /// (`http://127.0.0.1:9999` nos testes de contrato).
    pub fn new(team: &str, aud: &str, allowed_emails: &str) -> Option<Self> {
        let team = team.trim().trim_end_matches('/');
        let aud = aud.trim();
        if team.is_empty() || aud.is_empty() {
            return None;
        }
        let issuer = if team.contains("://") {
            team.to_owned()
        } else {
            format!("https://{team}")
        };
        Some(Self {
            issuer,
            aud: aud.to_owned(),
            allowed_emails: allowed_emails
                .split(',')
                .map(|e| e.trim().to_ascii_lowercase())
                .filter(|e| !e.is_empty())
                .collect(),
        })
    }

    pub fn certs_url(&self) -> String {
        format!("{}/cdn-cgi/access/certs", self.issuer)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    Missing,
    Malformed(&'static str),
    Algorithm(String),
    UnknownKey(String),
    BadSignature,
    Expired,
    NotYetValid,
    WrongAudience,
    WrongIssuer,
    WrongType(String),
    NotAllowed(String),
    /// Não deu para buscar as chaves: é falha nossa, não do cliente.
    Keys(PortError),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("token ausente"),
            Self::Malformed(w) => write!(f, "token malformado: {w}"),
            Self::Algorithm(a) => write!(f, "algoritmo não aceito: {a}"),
            Self::UnknownKey(k) => write!(f, "chave desconhecida: {k}"),
            Self::BadSignature => f.write_str("assinatura inválida"),
            Self::Expired => f.write_str("token expirado"),
            Self::NotYetValid => f.write_str("token ainda não válido"),
            Self::WrongAudience => f.write_str("aud de outra aplicação"),
            Self::WrongIssuer => f.write_str("iss de outro time"),
            Self::WrongType(t) => write!(f, "tipo de token não aceito: {t}"),
            Self::NotAllowed(e) => write!(f, "email sem permissão: {e}"),
            Self::Keys(e) => write!(f, "chaves do Access indisponíveis: {e}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    UnknownKey,
    Port(PortError),
}

/// Verifica RS256 com a chave `kid` do time.
#[allow(async_fn_in_trait)]
pub trait SignatureVerifier {
    async fn verify_rs256(
        &self,
        kid: &str,
        signing_input: &[u8],
        signature: &[u8],
    ) -> Result<bool, VerifyError>;
}

pub async fn authenticate<V: SignatureVerifier>(
    cfg: &AccessConfig,
    verifier: &V,
    token: Option<&str>,
    now: UnixMs,
) -> Result<Identity, AuthError> {
    let token = token
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or(AuthError::Missing)?;
    let mut parts = token.split('.');
    let (Some(h), Some(p), Some(sig), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(AuthError::Malformed("precisa de 3 partes"));
    };
    let header = json_part(h, "header")?;
    let claims = json_part(p, "payload")?;
    let signature = base64url_decode(sig).ok_or(AuthError::Malformed("assinatura"))?;

    // Só RS256: nada de `none` nem HS256 (que usaria a chave pública como segredo).
    let alg = header
        .get("alg")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if alg != "RS256" {
        return Err(AuthError::Algorithm(alg.to_owned()));
    }
    let kid = header
        .get("kid")
        .and_then(Value::as_str)
        .ok_or(AuthError::Malformed("sem kid"))?;

    let signing_input = &token[..h.len() + 1 + p.len()];
    match verifier
        .verify_rs256(kid, signing_input.as_bytes(), &signature)
        .await
    {
        Ok(true) => {}
        Ok(false) => return Err(AuthError::BadSignature),
        Err(VerifyError::UnknownKey) => return Err(AuthError::UnknownKey(kid.to_owned())),
        Err(VerifyError::Port(e)) => return Err(AuthError::Keys(e)),
    }

    // Claims só depois da assinatura: antes disso o payload é de qualquer um.
    if claims.get("iss").and_then(Value::as_str) != Some(cfg.issuer.as_str()) {
        return Err(AuthError::WrongIssuer);
    }
    let aud_ok = match claims.get("aud") {
        Some(Value::String(a)) => a == &cfg.aud,
        Some(Value::Array(list)) => list.iter().any(|a| a.as_str() == Some(cfg.aud.as_str())),
        _ => false,
    };
    if !aud_ok {
        return Err(AuthError::WrongAudience);
    }
    if let Some(t) = claims.get("type").and_then(Value::as_str) {
        if t != "app" {
            return Err(AuthError::WrongType(t.to_owned()));
        }
    }
    let now_ms = now.as_i64();
    let exp_ms = claims
        .get("exp")
        .and_then(Value::as_i64)
        .ok_or(AuthError::Malformed("sem exp"))?
        * 1000;
    if now_ms > exp_ms + CLOCK_SKEW_MS {
        return Err(AuthError::Expired);
    }
    if let Some(nbf) = claims.get("nbf").and_then(Value::as_i64) {
        if now_ms + CLOCK_SKEW_MS < nbf * 1000 {
            return Err(AuthError::NotYetValid);
        }
    }

    let email = claims
        .get("email")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
        .ok_or(AuthError::Malformed("sem email"))?;
    if !cfg.allowed_emails.is_empty() && !cfg.allowed_emails.contains(&email) {
        return Err(AuthError::NotAllowed(email));
    }
    Ok(Identity { email })
}

fn json_part(part: &str, what: &'static str) -> Result<Value, AuthError> {
    let bytes = base64url_decode(part).ok_or(AuthError::Malformed(what))?;
    let v: Value = serde_json::from_slice(&bytes).map_err(|_| AuthError::Malformed(what))?;
    v.is_object().then_some(v).ok_or(AuthError::Malformed(what))
}

/// Base64url sem padding (RFC 7515). `None` se tiver caractere inválido.
pub fn base64url_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        } as u32)
    };
    if s.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for chunk in s.as_bytes().chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        let bytes = n.to_be_bytes();
        out.extend_from_slice(&bytes[1..chunk.len()]);
    }
    Some(out)
}

#[cfg(test)]
mod tests;
