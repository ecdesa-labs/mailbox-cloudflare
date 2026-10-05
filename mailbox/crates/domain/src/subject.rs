/// Prefixos de resposta/encaminhamento comuns (pt, en, es, fr, de, nórdicos).
const PREFIXES: [&str; 10] = [
    "re", "fw", "fwd", "enc", "res", "rv", "tr", "aw", "wg", "sv",
];

/// Assunto normalizado para agrupar threads (`threads.subject_norm`):
/// sem prefixos `Re:`/`Fwd:`/`Enc:` (também `Re[2]:` e repetidos),
/// espaços colapsados e minúsculas. `None` se nada sobrar.
pub fn normalize_subject(subject: &str) -> Option<String> {
    let mut s = subject.trim();
    while let Some(rest) = strip_prefix(s) {
        s = rest.trim_start();
    }
    let norm = s
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    (!norm.is_empty()).then_some(norm)
}

fn strip_prefix(s: &str) -> Option<&str> {
    let word_end = s
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(s.len());
    let word = &s[..word_end];
    if !PREFIXES.iter().any(|p| word.eq_ignore_ascii_case(p)) {
        return None;
    }
    let mut rest = s[word_end..].trim_start();
    // Contador opcional: Re[2]: ou Re(2):
    if let Some(r) = rest.strip_prefix('[').or_else(|| rest.strip_prefix('(')) {
        let close = r.find([']', ')'])?;
        if !r[..close].bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        rest = r[close + 1..].trim_start();
    }
    rest.strip_prefix(':').or_else(|| rest.strip_prefix('：'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_prefixos() {
        let cases = [
            ("Re: Fatura", "fatura"),
            ("RE: re: Fwd: Fatura", "fatura"),
            ("Enc: Res: Proposta ACMECO", "proposta acmeco"),
            ("Re[2]: Fatura", "fatura"),
            ("Re (3): Fatura", "fatura"),
            ("AW: WG: Rechnung", "rechnung"),
            ("  Fatura   de   outubro ", "fatura de outubro"),
        ];
        for (entrada, esperado) in cases {
            assert_eq!(
                normalize_subject(entrada).as_deref(),
                Some(esperado),
                "{entrada}"
            );
        }
    }

    #[test]
    fn nao_remove_o_que_nao_e_prefixo() {
        assert_eq!(normalize_subject("Recibo").as_deref(), Some("recibo"));
        assert_eq!(normalize_subject("Re fatura").as_deref(), Some("re fatura"));
        assert_eq!(
            normalize_subject("[ctf] Re: flag").as_deref(),
            Some("[ctf] re: flag")
        );
        assert_eq!(normalize_subject("Re[x]: a").as_deref(), Some("re[x]: a"));
    }

    #[test]
    fn vazio_vira_none() {
        assert_eq!(normalize_subject(""), None);
        assert_eq!(normalize_subject("Re: "), None);
        assert_eq!(normalize_subject("Re: Fwd:"), None);
    }
}
