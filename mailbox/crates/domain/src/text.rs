//! Cortes de texto para o D1. O limite do D1 é 2 MB por linha/string
//! (conferido em 2026-10-05); o corpo completo vive no R2.

/// Tamanho máximo de `messages.text_body`, em bytes.
pub const TEXT_BODY_MAX_BYTES: usize = 64 * 1024;

/// Tamanho máximo de `messages.snippet`, em caracteres.
pub const SNIPPET_MAX_CHARS: usize = 200;

/// Corta em no máximo `TEXT_BODY_MAX_BYTES` sem quebrar um caractere UTF-8.
pub fn truncate_text_body(text: &str) -> &str {
    truncate_bytes(text, TEXT_BODY_MAX_BYTES)
}

fn truncate_bytes(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Prévia da lista: espaços colapsados, até `SNIPPET_MAX_CHARS` caracteres,
/// com `…` quando corta. `None` se não sobrar texto.
pub fn snippet(text: &str) -> Option<String> {
    let mut out = String::new();
    let mut count = 0;
    for word in text.split_whitespace() {
        let sep = usize::from(!out.is_empty());
        let len = word.chars().count();
        if count + sep + len > SNIPPET_MAX_CHARS {
            let room = SNIPPET_MAX_CHARS.saturating_sub(count + sep + 1);
            if room > 0 {
                if sep == 1 {
                    out.push(' ');
                }
                out.extend(word.chars().take(room));
            }
            out.push('…');
            return Some(out);
        }
        if sep == 1 {
            out.push(' ');
        }
        out.push_str(word);
        count += sep + len;
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpo_curto_intacto() {
        assert_eq!(truncate_text_body("oi"), "oi");
    }

    #[test]
    fn corpo_corta_em_fronteira_utf8() {
        // "é" ocupa 2 bytes; o limite cai no meio do último.
        let s = "a".repeat(TEXT_BODY_MAX_BYTES - 1) + "é";
        let t = truncate_text_body(&s);
        assert_eq!(t.len(), TEXT_BODY_MAX_BYTES - 1);
        assert!(t.is_char_boundary(t.len()));
    }

    #[test]
    fn snippet_colapsa_espacos() {
        assert_eq!(
            snippet("  Olá,\r\n\r\n  tudo\tbem?  ").as_deref(),
            Some("Olá, tudo bem?")
        );
    }

    #[test]
    fn snippet_vazio() {
        assert_eq!(snippet(""), None);
        assert_eq!(snippet(" \n\t "), None);
    }

    #[test]
    fn snippet_corta_com_reticencias() {
        let s = "palavra ".repeat(100);
        let p = snippet(&s).unwrap();
        assert!(p.ends_with('…'));
        assert_eq!(p.chars().count(), SNIPPET_MAX_CHARS);
    }

    #[test]
    fn snippet_palavra_gigante() {
        let s = "ç".repeat(1000);
        let p = snippet(&s).unwrap();
        assert_eq!(p.chars().count(), SNIPPET_MAX_CHARS);
        assert!(p.ends_with('…'));
    }

    #[test]
    fn snippet_exato_no_limite_nao_corta() {
        let s = "x".repeat(SNIPPET_MAX_CHARS);
        assert_eq!(snippet(&s).unwrap(), s);
    }
}
