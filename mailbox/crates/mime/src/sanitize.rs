//! Sanitização do HTML de email (docs seção 6.2, passo 3).
//!
//! Duas etapas:
//! 1. `ammonia` remove tudo que não está na lista permitida: scripts, iframes,
//!    forms, handlers de evento, `<style>`, CSS que pode carregar URL.
//! 2. Uma passada pelas tags resolve as imagens (`cid:` → URL do anexo; remota →
//!    `data-remote-src`, bloqueada) e corta links com esquema perigoso.
//!    O `ammonia` não renomeia atributos, por isso esta etapa existe.
//!
//! O resultado ainda deve ser exibido em `<iframe sandbox srcdoc>` sem
//! `allow-scripts` (docs seção 6.3): sanitizar é uma camada, não a única.

use std::borrow::Cow;
use std::cell::RefCell;
use std::sync::OnceLock;

use ammonia::{Builder, UrlRelative};
use html5ever::tokenizer::{
    BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use html5ever::{ns, Attribute, LocalName, QualName};

/// Atributo que guarda a URL de uma imagem remota bloqueada.
pub const REMOTE_SRC_ATTR: &str = "data-remote-src";

/// Propriedades CSS permitidas em `style`. Nenhuma aceita `url()`, então
/// CSS não consegue carregar nada de fora (rastreamento) mesmo com escapes.
/// `background` é convertido para `background-color` antes (ver `rewrite_style`).
const STYLE_PROPERTIES: &[&str] = &[
    "background-color",
    "border",
    "border-bottom",
    "border-bottom-color",
    "border-bottom-style",
    "border-bottom-width",
    "border-collapse",
    "border-color",
    "border-left",
    "border-left-color",
    "border-left-style",
    "border-left-width",
    "border-radius",
    "border-right",
    "border-right-color",
    "border-right-style",
    "border-right-width",
    "border-spacing",
    "border-style",
    "border-top",
    "border-top-color",
    "border-top-style",
    "border-top-width",
    "border-width",
    "color",
    "direction",
    "display",
    "font",
    "font-family",
    "font-size",
    "font-style",
    "font-variant",
    "font-weight",
    "height",
    "letter-spacing",
    "line-height",
    "margin",
    "margin-bottom",
    "margin-left",
    "margin-right",
    "margin-top",
    "max-height",
    "max-width",
    "min-height",
    "min-width",
    "overflow-wrap",
    "padding",
    "padding-bottom",
    "padding-left",
    "padding-right",
    "padding-top",
    "table-layout",
    "text-align",
    "text-decoration",
    "text-indent",
    "text-transform",
    "vertical-align",
    "white-space",
    "width",
    "word-break",
    "word-spacing",
]; // fmt: skip

/// Tags removidas junto com todo o conteúdo (não só a marcação).
const DROP_WITH_CONTENT: &[&str] = &[
    "script", "style", "title", "noscript", "iframe", "frame", "frameset", "object", "embed",
    "applet", "template", "svg", "math", "head",
]; // fmt: skip

fn builder() -> &'static Builder<'static> {
    static BUILDER: OnceLock<Builder<'static>> = OnceLock::new();
    BUILDER.get_or_init(|| {
        let mut b = Builder::default();
        b.add_tags(["font"])
            .clean_content_tags(DROP_WITH_CONTENT.iter().copied().collect())
            .add_generic_attributes(["dir", "style"])
            .add_tag_attributes("img", ["align", "border"])
            .add_tag_attributes(
                "table",
                [
                    "align",
                    "bgcolor",
                    "border",
                    "cellpadding",
                    "cellspacing",
                    "width",
                ],
            )
            .add_tag_attributes("tr", ["align", "bgcolor", "valign"])
            .add_tag_attributes(
                "td",
                ["align", "bgcolor", "height", "nowrap", "valign", "width"],
            )
            .add_tag_attributes(
                "th",
                ["align", "bgcolor", "height", "nowrap", "valign", "width"],
            )
            .add_tag_attributes("col", ["width"])
            .add_tag_attributes("font", ["color", "face", "size"])
            .add_tag_attributes("div", ["align"])
            .add_tag_attributes("p", ["align"])
            // cid e data só sobrevivem em <img src>; a etapa 2 corta o resto.
            .url_schemes(["http", "https", "mailto", "tel", "cid", "data"].into())
            // URL relativa apontaria para a nossa própria origem (/api/...).
            .url_relative(UrlRelative::Deny)
            .link_rel(Some("noopener noreferrer"))
            .set_tag_attribute_value("a", "target", "_blank")
            .filter_style_properties(STYLE_PROPERTIES.iter().copied().collect())
            .attribute_filter(|_, attr, value| match attr {
                "style" => Some(rewrite_style(value)),
                _ => Some(value.into()),
            });
        b
    })
}

/// `background: #fff` é muito comum em email e seria perdido (a propriedade
/// aceita `url()`). Vira `background-color` quando não tem URL nem escape.
fn rewrite_style(style: &str) -> Cow<'_, str> {
    if !style.to_ascii_lowercase().contains("background") {
        return style.into();
    }
    let out: Vec<String> = style
        .split(';')
        .map(|decl| match decl.split_once(':') {
            Some((name, value)) if name.trim().eq_ignore_ascii_case("background") => {
                let v = value.to_ascii_lowercase();
                if v.contains("url") || v.contains('\\') || v.contains("image") {
                    String::new()
                } else {
                    format!("background-color:{value}")
                }
            }
            _ => decl.to_owned(),
        })
        .collect();
    out.join(";").into()
}

/// Sanitiza HTML de email. `resolve_cid` recebe o Content-ID (sem `cid:`)
/// e devolve a URL do anexo, ou `None` para remover a imagem.
pub fn sanitize_html(html: &str, resolve_cid: impl Fn(&str) -> Option<String>) -> String {
    let cleaned = builder().clean(html).to_string();
    rewrite_tags(&cleaned, |el, attrs| match el {
        "img" => rewrite_img(attrs, &resolve_cid),
        "a" | "area" => {
            if let Some(i) = find(attrs, "href") {
                if !has_scheme(&attrs[i].value, &["http", "https", "mailto", "tel"]) {
                    attrs.remove(i);
                }
            }
        }
        _ => {}
    })
}

/// Versão com imagens remotas liberadas (`?remote_images=1`, docs seção 6.3):
/// `data-remote-src` volta a ser `src`.
pub fn restore_remote_images(sanitized: &str) -> String {
    rewrite_tags(sanitized, |el, attrs| {
        if el != "img" {
            return;
        }
        let Some(i) = find(attrs, REMOTE_SRC_ATTR) else {
            return;
        };
        let url = attrs.remove(i).value;
        if has_scheme(&url, &["http", "https"]) {
            attrs.retain(|a| &*a.name.local != "src");
            attrs.push(attr("src", &url));
        }
    })
}

fn rewrite_img(attrs: &mut Vec<Attribute>, resolve_cid: &impl Fn(&str) -> Option<String>) {
    let Some(i) = find(attrs, "src") else { return };
    let src = attrs.remove(i).value.to_string();
    let lower = src.trim().to_ascii_lowercase();

    if let Some(cid) = lower.strip_prefix("cid:").map(|_| &src.trim()[4..]) {
        if let Some(url) = resolve_cid(&percent_decode(cid)) {
            attrs.push(attr("src", &url));
        }
    } else if lower.starts_with("http://") || lower.starts_with("https://") {
        attrs.push(attr(REMOTE_SRC_ATTR, src.trim()));
    } else if [
        "data:image/png",
        "data:image/jpeg",
        "data:image/jpg",
        "data:image/gif",
        "data:image/webp",
    ]
    .iter()
    .any(|p| lower.starts_with(p))
    {
        attrs.push(attr("src", &src));
    }
    // Qualquer outra coisa: a imagem fica sem src.
}

/// RFC 2392: o Content-ID dentro de `cid:` pode vir com %XX.
fn percent_decode(s: &str) -> String {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn has_scheme(url: &str, allowed: &[&str]) -> bool {
    url.trim()
        .split_once(':')
        .is_some_and(|(scheme, _)| allowed.iter().any(|a| scheme.eq_ignore_ascii_case(a)))
}

fn find(attrs: &[Attribute], name: &str) -> Option<usize> {
    attrs.iter().position(|a| &*a.name.local == name)
}

fn attr(name: &str, value: &str) -> Attribute {
    Attribute {
        name: QualName::new(None, ns!(), LocalName::from(name)),
        value: value.into(),
    }
}

/// Reescreve atributos tag a tag e serializa de volta. Roda sobre a saída do
/// `ammonia`, que já é HTML bem formado e sem elementos de texto cru
/// (script/style/textarea), então todo texto é texto comum.
fn rewrite_tags(html: &str, f: impl FnMut(&str, &mut Vec<Attribute>)) -> String {
    struct Sink<F> {
        out: RefCell<String>,
        f: RefCell<F>,
    }

    impl<F: FnMut(&str, &mut Vec<Attribute>)> TokenSink for Sink<F> {
        type Handle = ();

        fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
            let mut out = self.out.borrow_mut();
            match token {
                Token::TagToken(mut tag) => match tag.kind {
                    TagKind::StartTag => {
                        (self.f.borrow_mut())(&tag.name, &mut tag.attrs);
                        out.push('<');
                        out.push_str(&tag.name);
                        for a in &tag.attrs {
                            out.push(' ');
                            out.push_str(&a.name.local);
                            out.push_str("=\"");
                            escape(&mut out, &a.value, true);
                            out.push('"');
                        }
                        out.push('>');
                    }
                    TagKind::EndTag => {
                        out.push_str("</");
                        out.push_str(&tag.name);
                        out.push('>');
                    }
                },
                Token::CharacterTokens(text) => escape(&mut out, &text, false),
                // O ammonia já removeu comentários e doctype.
                _ => {}
            }
            TokenSinkResult::Continue
        }
    }

    let sink = Sink {
        out: RefCell::new(String::with_capacity(html.len())),
        f: RefCell::new(f),
    };
    let tokenizer = Tokenizer::new(sink, TokenizerOpts::default());
    let input = BufferQueue::default();
    input.push_back(html.into());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.out.take()
}

/// Mesmo escape do serializador do html5ever.
fn escape(out: &mut String, s: &str, attr_mode: bool) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            '"' if attr_mode => out.push_str("&quot;"),
            '<' if !attr_mode => out.push_str("&lt;"),
            '>' if !attr_mode => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(html: &str) -> String {
        sanitize_html(html, |cid| Some(format!("/api/attachments/{cid}")))
    }

    #[test]
    fn remove_script_iframe_form_e_handlers() {
        let out = clean(
            r#"<p onclick="x()">oi</p><script>alert(1)</script><iframe src="https://x"></iframe>
               <form action="https://x"><input name=a><button>ok</button></form><style>p{}</style>"#,
        );
        assert!(!out.contains("script") && !out.contains("alert"), "{out}");
        assert!(!out.contains("iframe") && !out.contains("onclick"), "{out}");
        assert!(!out.contains("<form") && !out.contains("<input"), "{out}");
        assert!(!out.contains("<style") && !out.contains("p{}"), "{out}");
        assert!(out.contains("<p>oi</p>"), "{out}");
    }

    #[test]
    fn imagem_remota_fica_bloqueada() {
        let out = clean(r#"<img src="https://tracker.example/p.gif" alt="x">"#);
        assert_eq!(
            out,
            r#"<img alt="x" data-remote-src="https://tracker.example/p.gif">"#
        );
    }

    #[test]
    fn restaura_imagem_remota() {
        let out = restore_remote_images(&clean(r#"<img src="https://img.example/a.png">"#));
        assert_eq!(out, r#"<img src="https://img.example/a.png">"#);
    }

    #[test]
    fn restaurar_nao_aceita_esquema_estranho() {
        let out = restore_remote_images(r#"<img data-remote-src="javascript:alert(1)">"#);
        assert_eq!(out, "<img>");
    }

    #[test]
    fn cid_vira_url_do_anexo() {
        let out = clean(r#"<img src="cid:logo%40acmeco">"#);
        assert_eq!(out, r#"<img src="/api/attachments/logo@acmeco">"#);
    }

    #[test]
    fn cid_sem_anexo_some() {
        let out = sanitize_html(r#"<img src="cid:nada" alt="x">"#, |_| None);
        assert_eq!(out, r#"<img alt="x">"#);
    }

    #[test]
    fn data_uri_so_imagem() {
        assert!(clean(r#"<img src="data:image/png;base64,AAAA">"#).contains("src=\"data:image/png"));
        assert_eq!(
            clean(r#"<img src="data:image/svg+xml;base64,AAAA">"#),
            "<img>"
        );
        assert_eq!(clean(r#"<img src="data:text/html,<script>">"#), "<img>");
    }

    #[test]
    fn links() {
        let out = clean(r#"<a href="https://acmeco.com.br">a</a>"#);
        assert_eq!(
            out,
            r#"<a href="https://acmeco.com.br" target="_blank" rel="noopener noreferrer">a</a>"#
        );
        for href in [
            "javascript:alert(1)",
            "data:text/html,x",
            "cid:x",
            "/api/messages",
            "vbscript:x",
        ] {
            let out = clean(&format!(r#"<a href="{href}">a</a>"#));
            assert!(!out.contains("href"), "{href}: {out}");
        }
    }

    #[test]
    fn css_nao_carrega_url() {
        let out = clean(
            r#"<p style="color:red;background-image:url(https://t.example/a);background:url(https://t.example/b)">x</p>
               <p style="b\61ckground-image:url(https://t.example/c)">y</p>
               <div style="background:#fff;list-style-image:url(https://t.example/d)">z</div>"#,
        );
        assert!(!out.contains("t.example"), "{out}");
        assert!(out.contains("color:red"), "{out}");
        assert!(out.contains("background-color:#fff"), "{out}");
    }

    #[test]
    fn mantem_layout_de_tabela() {
        let out = clean(
            r##"<table width="600" bgcolor="#eee"><tr><td align="center" valign="top">x</td></tr></table>"##,
        );
        assert!(
            out.contains(r#"width="600""#) && out.contains(r##"bgcolor="#eee""##),
            "{out}"
        );
        assert!(
            out.contains(r#"align="center""#) && out.contains(r#"valign="top""#),
            "{out}"
        );
    }

    #[test]
    fn remove_conteudo_de_head_e_title() {
        let out = clean("<html><head><title>T</title><meta http-equiv=refresh content=0></head><body><p>b</p></body></html>");
        assert_eq!(out, "<p>b</p>");
    }

    #[test]
    fn percent() {
        assert_eq!(percent_decode("a%40b%2"), "a@b%2");
        assert_eq!(percent_decode("%zz"), "%zz");
    }
}
