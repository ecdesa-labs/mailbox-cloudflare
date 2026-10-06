use std::cell::Cell;

use mailbox_domain::{DomainName, MatchPattern, RuleAction, RuleId};

use super::*;
use crate::fakes::*;

/// Coloca um .eml no R2 fake e devolve o job como o ingest produziria.
fn job(blobs: &MemBlobs, n: u128, to: &str, eml: &[u8]) -> IngestJob {
    let domain = DomainName::parse("acmeco.com.br").unwrap();
    let key = RawKey::new(&domain, MessageId::from_parts(UnixMs::new(NOW).unwrap(), n));
    blobs
        .objects
        .borrow_mut()
        .insert(key.to_string(), (eml.to_vec(), "message/rfc822".into()));
    IngestJob {
        raw_key: key.to_string(),
        domain: "acmeco.com.br".into(),
        envelope_from: "cliente@exemplo.com".into(),
        envelope_to: to.into(),
        received_at: NOW + n as i64,
        size: eml.len() as u64,
        sent: false,
    }
}

fn run(
    blobs: &MemBlobs,
    index: &FakeIndex,
    job: &IngestJob,
    full_max: u64,
) -> Result<ProcessOutcome, ProcessError> {
    let uc = ProcessMessage {
        blobs,
        index,
        clock: &FixedClock,
        ids: &SeqIds(Cell::new(1000)),
        full_max_bytes: full_max,
        max_attachments: 30,
    };
    block_on(uc.run(job))
}

const BIG: u64 = 25 * 1024 * 1024;
const INLINE: &[u8] = include_bytes!("../../../mime/tests/fixtures/imagem_inline.eml");
const TEXTO: &[u8] = include_bytes!("../../../mime/tests/fixtures/texto_puro.eml");
const ALT: &[u8] = include_bytes!("../../../mime/tests/fixtures/multipart_alternative.eml");

#[test]
fn completo_grava_anexo_html_e_indice() {
    let (blobs, index) = (MemBlobs::default(), FakeIndex::new());
    let j = job(&blobs, 1, "contato@acmeco.com.br", INLINE);
    let out = run(&blobs, &index, &j, BIG).unwrap();
    assert_eq!(out.processing, Processing::Full);
    assert!(!out.reprocessed && !out.hidden_by_rule);

    let r = index.get(out.message_id);
    let att_id = AttachmentId::for_message(out.message_id, 0);
    assert_eq!(r.attachments.len(), 1);
    assert_eq!(r.attachments[0].id, att_id);
    assert!(r.message.has_attachments);
    assert_eq!(
        r.message.thread_id,
        Some(ThreadId::rooted_at(out.message_id))
    );
    assert_eq!(r.message.folder, Folder::Inbox);
    assert_eq!(r.message.subject.as_deref(), Some("Newsletter com logo"));
    assert_eq!(
        r.thread.subject_norm.as_deref(),
        Some("newsletter com logo")
    );

    let att_key = r.attachments[0].r2_key.as_str();
    assert_eq!(blobs.objects.borrow()[att_key].1, "image/png");
    let html = blobs.text(r.message.html_key.as_ref().unwrap().as_str());
    assert!(
        html.contains(&format!(r#"src="/api/attachments/{att_id}""#)),
        "{html}"
    );
}

#[test]
fn reprocessar_mantem_ids_e_nao_duplica() {
    let (blobs, index) = (MemBlobs::default(), FakeIndex::new());
    let j = job(&blobs, 1, "contato@acmeco.com.br", INLINE);
    let a = run(&blobs, &index, &j, BIG).unwrap();
    let primeiro = index.get(a.message_id);
    let b = run(&blobs, &index, &j, BIG).unwrap();
    assert!(b.reprocessed);
    assert_eq!(a.message_id, b.message_id);
    assert_eq!(index.saved.borrow().len(), 1);
    let segundo = index.get(b.message_id);
    assert_eq!(primeiro.attachments, segundo.attachments);
    assert_eq!(primeiro.message.thread_id, segundo.message.thread_id);
    assert_eq!(primeiro.message.identity_id, segundo.message.identity_id);
    assert!(!segundo.is_new);
}

#[test]
fn grande_vai_para_o_modo_leve() {
    let (blobs, index) = (MemBlobs::default(), FakeIndex::new());
    let j = job(&blobs, 1, "contato@acmeco.com.br", INLINE);
    let antes = blobs.objects.borrow().len();
    let out = run(&blobs, &index, &j, 100).unwrap();
    assert_eq!(out.processing, Processing::Partial);
    let r = index.get(out.message_id);
    assert_eq!(r.message.subject.as_deref(), Some("Newsletter com logo"));
    assert_eq!((r.message.text_body, r.message.html_key), (None, None));
    assert!(r.attachments.is_empty());
    assert_eq!(
        blobs.objects.borrow().len(),
        antes,
        "modo leve não grava nada no R2"
    );
}

#[test]
fn resposta_entra_na_thread_da_original() {
    let (blobs, index) = (MemBlobs::default(), FakeIndex::new());
    let original = run(
        &blobs,
        &index,
        &job(&blobs, 1, "contato@acmeco.com.br", TEXTO),
        BIG,
    )
    .unwrap();
    let resposta =
        b"Message-ID: <r1@acmeco.com.br>\r\nIn-Reply-To: <CAF+texto.001@cliente.com.br>\r\n\
                     Subject: Re: Or\xc3\xa7amento\r\n\r\nok";
    let r = run(
        &blobs,
        &index,
        &job(&blobs, 2, "contato@acmeco.com.br", resposta),
        BIG,
    )
    .unwrap();
    assert_eq!(
        index.get(r.message_id).message.thread_id,
        Some(ThreadId::rooted_at(original.message_id))
    );
}

#[test]
fn original_que_chega_depois_entra_na_thread_da_resposta() {
    let (blobs, index) = (MemBlobs::default(), FakeIndex::new());
    let resposta = b"Message-ID: <r1@acmeco.com.br>\r\nIn-Reply-To: <CAF+texto.001@cliente.com.br>\r\nSubject: Re: x\r\n\r\nok";
    let r = run(
        &blobs,
        &index,
        &job(&blobs, 1, "contato@acmeco.com.br", resposta),
        BIG,
    )
    .unwrap();
    let o = run(
        &blobs,
        &index,
        &job(&blobs, 2, "contato@acmeco.com.br", TEXTO),
        BIG,
    )
    .unwrap();
    assert_eq!(
        index.get(o.message_id).message.thread_id,
        Some(ThreadId::rooted_at(r.message_id))
    );
}

#[test]
fn regras_so_na_primeira_vez() {
    let blobs = MemBlobs::default();
    let mut index = FakeIndex::new();
    let rule = |n, pattern: &str, action| Rule {
        id: RuleId::from_parts(UnixMs::new(0).unwrap(), n),
        priority: 0,
        pattern: MatchPattern::parse(pattern).unwrap(),
        action,
        enabled: true,
    };
    index.rules = vec![
        rule(1, "*@acmeco.com.br", RuleAction::Move(Folder::Archive)),
        rule(2, "*@acmeco.com.br", RuleAction::MarkRead),
        rule(3, "vazou@acmeco.com.br", RuleAction::Reject),
    ];

    let a = run(
        &blobs,
        &index,
        &job(&blobs, 1, "contato@acmeco.com.br", ALT),
        BIG,
    )
    .unwrap();
    let r = index.get(a.message_id);
    assert_eq!(
        (r.message.folder, r.message.is_read),
        (Folder::Archive, true)
    );

    let v = run(
        &blobs,
        &index,
        &job(&blobs, 2, "vazou@acmeco.com.br", INLINE),
        BIG,
    )
    .unwrap();
    assert!(v.hidden_by_rule);
    let r = index.get(v.message_id);
    assert_eq!(r.message.deleted_at, Some(UnixMs::new(NOW).unwrap()));
    assert!(r.attachments.is_empty() && r.message.html_key.is_none());

    // Reprocessar não reaplica regras (o adaptador preserva pasta/lido).
    let again = run(
        &blobs,
        &index,
        &job(&blobs, 1, "contato@acmeco.com.br", ALT),
        BIG,
    )
    .unwrap();
    let r = index.get(again.message_id);
    assert!(!r.is_new);
    assert_eq!(
        (r.message.folder, r.message.is_read),
        (Folder::Inbox, false)
    );
}

#[test]
fn autenticacao_confia_so_na_cloudflare() {
    let (blobs, index) = (MemBlobs::default(), FakeIndex::new());
    let eml = b"Received-SPF: softfail (mx.cloudflare.net: ...) receiver=mx.cloudflare.net; client-ip=1.2.3.4\r\n\
                Authentication-Results: mx.cloudflare.net; dkim=pass header.d=x; dmarc=fail; spf=none smtp.helo=x\r\n\
                Authentication-Results: mx.cloudflare.net; dkim=fail; dmarc=pass; spf=pass\r\n\
                Received-SPF: pass receiver=mx.cloudflare.net\r\n\
                Message-ID: <auth@x>\r\nSubject: s\r\n\r\nx";
    let out = run(
        &blobs,
        &index,
        &job(&blobs, 1, "contato@acmeco.com.br", eml),
        BIG,
    )
    .unwrap();
    let auth = index.get(out.message_id).message.auth;
    assert_eq!(
        auth.spf,
        Some(AuthVerdict::SoftFail),
        "Received-SPF de cima, não o do HELO"
    );
    assert_eq!(auth.dkim, Some(AuthVerdict::Pass));
    assert_eq!(auth.dmarc, Some(AuthVerdict::Fail));

    let forjado = b"Authentication-Results: mx.google.com; dkim=pass; dmarc=pass; spf=pass\r\nSubject: s\r\n\r\nx";
    let out = run(
        &blobs,
        &index,
        &job(&blobs, 2, "contato@acmeco.com.br", forjado),
        BIG,
    )
    .unwrap();
    assert_eq!(
        index.get(out.message_id).message.auth,
        AuthResults::default()
    );
}

#[test]
fn mesma_identidade_para_o_mesmo_endereco() {
    let (blobs, index) = (MemBlobs::default(), FakeIndex::new());
    let a = run(
        &blobs,
        &index,
        &job(&blobs, 1, "Contato@acmeco.com.br", TEXTO),
        BIG,
    )
    .unwrap();
    let b = run(
        &blobs,
        &index,
        &job(&blobs, 2, "contato@acmeco.com.br", ALT),
        BIG,
    )
    .unwrap();
    assert_eq!(
        index.get(a.message_id).message.identity_id,
        index.get(b.message_id).message.identity_id
    );
}

#[test]
fn erros_permanentes() {
    let (blobs, index) = (MemBlobs::default(), FakeIndex::new());
    let mut j = job(&blobs, 1, "contato@acmeco.com.br", TEXTO);

    let bad = IngestJob {
        raw_key: "lixo".into(),
        ..j.clone()
    };
    assert!(matches!(
        run(&blobs, &index, &bad, BIG),
        Err(ProcessError::Permanent(_))
    ));

    let outro = DomainName::parse("outro.dev").unwrap();
    let k = RawKey::new(&outro, MessageId::from_parts(UnixMs::new(NOW).unwrap(), 9));
    let sem_dominio = IngestJob {
        raw_key: k.to_string(),
        ..j.clone()
    };
    assert!(
        matches!(run(&blobs, &index, &sem_dominio, BIG), Err(ProcessError::Permanent(m)) if m.contains("domínio"))
    );

    j.raw_key = RawKey::new(
        &index.domain.0,
        MessageId::from_parts(UnixMs::new(NOW).unwrap(), 77),
    )
    .to_string();
    assert!(
        matches!(run(&blobs, &index, &j, BIG), Err(ProcessError::Permanent(m)) if m.contains("raw ausente"))
    );
}

#[test]
fn r2_fora_e_temporario() {
    let blobs = MemBlobs::with("x", b"");
    let j = job(&blobs, 1, "contato@acmeco.com.br", TEXTO);
    let blobs = MemBlobs {
        fail_get: true,
        ..MemBlobs::default()
    };
    assert!(matches!(
        run(&blobs, &FakeIndex::new(), &j, BIG),
        Err(ProcessError::Transient(_))
    ));
}

#[test]
fn muitos_anexos_indexa_texto_e_deixa_anexos_no_eml() {
    let (blobs, index) = (MemBlobs::default(), FakeIndex::new());
    let mut eml = String::from("Message-ID: <muitos@x>\r\nSubject: muitos\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\ncorpo\r\n");
    for i in 0..31 {
        eml.push_str(&format!("--b\r\nContent-Type: application/octet-stream; name=\"a{i}.bin\"\r\nContent-Disposition: attachment\r\n\r\nx\r\n"));
    }
    eml.push_str("--b--\r\n");
    let antes = {
        let j = job(&blobs, 1, "contato@acmeco.com.br", eml.as_bytes());
        let n = blobs.objects.borrow().len();
        (j, n)
    };
    let out = run(&blobs, &index, &antes.0, BIG).unwrap();
    assert_eq!(out.processing, Processing::Partial);
    let r = index.get(out.message_id);
    assert_eq!(r.message.text_body.as_deref(), Some("corpo"));
    assert!(r.attachments.is_empty());
    assert_eq!(blobs.objects.borrow().len(), antes.1);
}
