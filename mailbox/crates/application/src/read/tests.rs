use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use super::*;

fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = pin!(f);
    match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("fake não deveria ficar pendente"),
    }
}

fn id(n: u128) -> MessageId {
    MessageId::from_parts(UnixMs::new(1_791_158_400_000).unwrap(), n)
}

#[test]
fn cursor_ida_e_volta() {
    let c = Cursor {
        received_at: 1_791_158_400_123,
        id: id(7),
    };
    assert_eq!(Cursor::decode(&c.encode()), Some(c));
    for bad in [
        "",
        "x",
        "1.",
        ".01M44NRD000000000000000007",
        "-1.01M44NRD000000000000000007",
        "1.lixo",
    ] {
        assert_eq!(Cursor::decode(bad), None, "{bad}");
    }
}

#[test]
fn parametros_padrao() {
    let q = ListQuery::from_params([]).unwrap();
    assert_eq!(
        (q.folder, q.domain, q.limit, q.after),
        (Folder::Inbox, None, DEFAULT_LIMIT, None)
    );
}

#[test]
fn parametros_validos() {
    let c = Cursor {
        received_at: 5,
        id: id(1),
    }
    .encode();
    let q = ListQuery::from_params([
        ("folder", "archive"),
        ("domain", "ACMECO.com.br"),
        ("limit", "10"),
        ("cursor", c.as_str()),
        ("q", "  "),
        ("outro", "ignorado"),
    ])
    .unwrap();
    assert_eq!(q.folder, Folder::Archive);
    assert_eq!(q.domain.unwrap().as_str(), "acmeco.com.br");
    assert_eq!(q.limit, 10);
    assert_eq!(q.after.unwrap().received_at, 5);
}

#[test]
fn parametros_invalidos() {
    for (k, v) in [
        ("folder", "drafts"),
        ("limit", "0"),
        ("limit", "101"),
        ("limit", "x"),
        ("cursor", "lixo"),
        ("domain", "localhost"),
    ] {
        assert!(
            matches!(
                ListQuery::from_params([(k, v)]),
                Err(QueryError::Invalid { .. })
            ),
            "{k}={v}"
        );
    }
    assert_eq!(
        ListQuery::from_params([("q", "fatura")]),
        Err(QueryError::NotImplemented {
            param: "q",
            phase: 7
        })
    );
}

struct Fixed(Vec<ListItem>);
impl ReadModel for Fixed {
    async fn list(&self, _q: &ListQuery, limit: u32) -> Result<Vec<ListItem>, PortError> {
        Ok(self.0.iter().take(limit as usize).cloned().collect())
    }
    async fn message(&self, _: MessageId) -> Result<Option<MessageDetail>, PortError> {
        Ok(None)
    }
    async fn thread(&self, _: ThreadId) -> Result<Option<ThreadView>, PortError> {
        Ok(None)
    }
    async fn attachment(&self, _: AttachmentId) -> Result<Option<AttachmentMeta>, PortError> {
        Ok(None)
    }
    async fn domains(&self) -> Result<Vec<DomainSummary>, PortError> {
        Ok(vec![])
    }
}

fn item(n: u128) -> ListItem {
    ListItem {
        id: id(n),
        thread_id: None,
        domain: "acmeco.com.br".into(),
        identity: None,
        from: None,
        subject: None,
        snippet: None,
        received_at: UnixMs::new(1000 - n as i64).unwrap(),
        is_read: false,
        is_starred: false,
        has_attachments: false,
        folder: Folder::Inbox,
        processing: Processing::Full,
    }
}

#[test]
fn pagina_com_e_sem_proxima() {
    let read = Fixed((1..=3).map(item).collect());
    let q = |limit| ListQuery {
        limit,
        ..ListQuery::from_params([]).unwrap()
    };

    let p = block_on(list_page(&read, &q(2))).unwrap();
    assert_eq!(p.items.len(), 2);
    let c = Cursor::decode(p.next_cursor.as_deref().unwrap()).unwrap();
    assert_eq!(
        (c.received_at, c.id),
        (998, id(2)),
        "cursor aponta para o último item da página"
    );

    let p = block_on(list_page(&read, &q(3))).unwrap();
    assert_eq!((p.items.len(), p.next_cursor), (3, None));
}

#[test]
fn so_imagens_simples_sao_exibidas() {
    for ok in ["image/png", "IMAGE/JPEG", "image/gif; name=x", "image/webp"] {
        assert!(is_inline_safe(ok), "{ok}");
    }
    for no in [
        "image/svg+xml",
        "text/html",
        "application/pdf",
        "",
        "image/png-ish",
    ] {
        assert!(!is_inline_safe(no), "{no}");
    }
}

#[test]
fn content_disposition_seguro() {
    assert_eq!(content_disposition(false, None), "attachment");
    assert_eq!(
        content_disposition(true, Some("logo.png")),
        "inline; filename=\"logo.png\"; filename*=UTF-8''logo.png"
    );
    let h = content_disposition(false, Some("orçamento \"final\".pdf"));
    assert_eq!(h, "attachment; filename=\"or_amento _final_.pdf\"; filename*=UTF-8''or%C3%A7amento%20%22final%22.pdf");
    // Quebra de linha no nome não pode injetar header.
    let h = content_disposition(false, Some("a\r\nSet-Cookie: x=1"));
    assert!(!h.contains('\r') && !h.contains('\n'), "{h}");
}
