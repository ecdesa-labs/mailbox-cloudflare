//! Testes golden (critério de aceite da Fase 2): cada `fixtures/*.eml` é
//! parseado e sanitizado, e o resultado é comparado com `fixtures/*.golden`.
//!
//! Para regenerar depois de uma mudança intencional (e revisar o diff!):
//!
//!     UPDATE_GOLDEN=1 cargo test -p mailbox-mime --test golden

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use mailbox_mime::{parse, restore_remote_images, sanitize_html, ParsedMessage};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Representação estável e legível de tudo que o crate produz.
fn render(p: &ParsedMessage<'_>) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "message_id: {:?}", p.message_id);
    let _ = writeln!(s, "in_reply_to: {:?}", p.in_reply_to);
    let _ = writeln!(s, "references: {:?}", p.references);
    let _ = writeln!(s, "from: {:?}", p.from);
    let _ = writeln!(s, "to: {:?}", p.to);
    let _ = writeln!(s, "cc: {:?}", p.cc);
    let _ = writeln!(s, "reply_to: {:?}", p.reply_to);
    let _ = writeln!(s, "subject: {:?}", p.subject);
    let _ = writeln!(s, "date_ms: {:?}", p.date_ms);
    let _ = writeln!(s, "has_encoding_problem: {}", p.has_encoding_problem);
    let _ = writeln!(s, "auth_results:");
    for a in &p.auth_results {
        let _ = writeln!(s, "  {a:?}");
    }
    let _ = writeln!(s, "received_spf:");
    for r in &p.received_spf {
        let _ = writeln!(s, "  {r:?}");
    }
    let _ = writeln!(s, "attachments:");
    for a in &p.attachments {
        let head: String = a.data.iter().take(24).map(|&b| b as char).collect();
        let _ = writeln!(
            s,
            "  #{} filename={:?} type={} cid={:?} inline={} size={} head={:?}",
            a.ordinal,
            a.filename,
            a.content_type,
            a.content_id,
            a.is_inline,
            a.data.len(),
            head
        );
    }
    let _ = writeln!(s, "--- text\n{}", p.text.as_deref().unwrap_or("<nenhum>"));

    let sanitized = p.html.as_deref().map(|h| {
        sanitize_html(h, |cid| {
            p.attachments
                .iter()
                .find(|a| a.content_id.as_deref() == Some(cid))
                .map(|a| format!("/api/attachments/{}", a.ordinal))
        })
    });
    let _ = writeln!(
        s,
        "--- html sanitizado\n{}",
        sanitized.as_deref().unwrap_or("<nenhum>")
    );
    if let Some(h) = &sanitized {
        let restored = restore_remote_images(h);
        if &restored != h {
            let _ = writeln!(s, "--- html com imagens remotas\n{restored}");
        }
    }
    s
}

#[test]
fn golden() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut emls: Vec<_> = fs::read_dir(fixtures())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "eml"))
        .collect();
    emls.sort();
    assert!(emls.len() >= 6, "fixtures sumiram: {emls:?}");

    let mut divergentes = Vec::new();
    for eml in emls {
        let raw = fs::read(&eml).unwrap();
        let parsed = parse(&raw).unwrap_or_else(|e| panic!("{}: {e}", eml.display()));
        let got = render(&parsed);
        let golden = eml.with_extension("golden");
        if update {
            fs::write(&golden, &got).unwrap();
            continue;
        }
        let want = fs::read_to_string(&golden)
            .unwrap_or_else(|_| panic!("falta {} (rode com UPDATE_GOLDEN=1)", golden.display()));
        if got != want {
            divergentes.push(format!(
                "== {}\n--- esperado\n{want}\n--- obtido\n{got}",
                eml.file_name().unwrap().to_string_lossy()
            ));
        }
    }
    assert!(divergentes.is_empty(), "{}", divergentes.join("\n"));
}

/// Anexo grande gerado na hora (não vale a pena versionar megabytes).
/// O Email Routing aceita até 25 MiB; 15 MiB em base64 dá ~20 MiB de .eml.
#[test]
fn anexo_grande() {
    const SIZE: usize = 15 * 1024 * 1024;
    // Bytes pseudoaleatórios determinísticos (xorshift), para não comprimir trivialmente.
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let data: Vec<u8> = (0..SIZE)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect();

    let mut eml = String::from(
        "Message-ID: <grande@acmeco.com.br>\r\nFrom: a@acmeco.com.br\r\nTo: b@acmeco.com.br\r\n\
         Subject: Anexo grande\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"g\"\r\n\r\n\
         --g\r\nContent-Type: text/plain\r\n\r\nveja o anexo\r\n\
         --g\r\nContent-Type: application/octet-stream; name=\"dados.bin\"\r\n\
         Content-Transfer-Encoding: base64\r\nContent-Disposition: attachment; filename=\"dados.bin\"\r\n\r\n",
    );
    eml.push_str(&base64_lines(&data));
    eml.push_str("--g--\r\n");
    assert!(eml.len() > 20 * 1024 * 1024, "{}", eml.len());

    let parsed = parse(eml.as_bytes()).unwrap();
    assert_eq!(parsed.text.as_deref(), Some("veja o anexo"));
    let [a] = &parsed.attachments[..] else {
        panic!("{:?}", parsed.attachments.len())
    };
    assert_eq!(a.filename.as_deref(), Some("dados.bin"));
    assert_eq!(a.content_type, "application/octet-stream");
    assert!(a.data[..] == data[..], "conteúdo do anexo diverge");
}

fn base64_lines(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len() * 4 / 3 + data.len() / 38);
    for (i, c) in data.chunks(3).enumerate() {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
        if (i + 1) % 19 == 0 {
            out.push_str("\r\n");
        }
    }
    out.push_str("\r\n");
    out
}
