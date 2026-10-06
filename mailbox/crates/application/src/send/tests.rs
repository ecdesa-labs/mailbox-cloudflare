use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use mailbox_domain::{DomainName, Folder};
use serde_json::json;

use super::*;
use crate::fakes::*;
use crate::ports::{ReplyTarget, Sender};

const IDENTITY: &str = "01M44NRD0000000000000000A1";
const OUTRA: &str = "01M44NRD0000000000000000A2";

fn identity() -> IdentityId {
    IdentityId::parse(IDENTITY).unwrap()
}

fn req(v: Value) -> Result<SendRequest, QueryError> {
    SendRequest::from_json(&v)
}

fn minimal() -> Value {
    json!({ "from_identity_id": IDENTITY, "to": ["joao@exemplo.com"], "subject": "Oi", "text": "Olá" })
}

// ------------------------------------------------------------ pedido ----

#[test]
fn pedido_minimo() {
    let r = req(minimal()).unwrap();
    assert_eq!(r.from_identity_id, identity());
    assert_eq!(r.to[0].addr.as_str(), "joao@exemplo.com");
    assert!(r.cc.is_empty() && r.in_reply_to.is_none() && r.html.is_none());
}

#[test]
fn destinatario_como_objeto_e_cc_sem_repetir_o_to() {
    let r = req(json!({
        "from_identity_id": IDENTITY,
        "to": [{ "name": "João\r\nBcc: x@y.com", "addr": "Joao@Exemplo.com" }, "joao@exemplo.com"],
        "cc": ["JOAO@exemplo.com", "maria@exemplo.com"],
        "subject": "Linha 1\r\nBcc: intruso@x.com",
        "text": "corpo",
    }))
    .unwrap();
    assert_eq!(r.to.len(), 1, "repetido no to some");
    assert_eq!(r.to[0].name.as_deref(), Some("João  Bcc: x@y.com"));
    assert_eq!(r.cc.len(), 1);
    assert_eq!(r.cc[0].addr.as_str(), "maria@exemplo.com");
    assert!(!r.subject.contains('\n') && !r.subject.contains('\r'));
}

#[test]
fn pedidos_invalidos() {
    let casos = [
        json!([]),
        json!({ "from_identity_id": IDENTITY, "to": [], "subject": "", "text": "x" }),
        json!({ "from_identity_id": IDENTITY, "to": ["não é email"], "subject": "", "text": "x" }),
        json!({ "from_identity_id": IDENTITY, "to": ["a@b.com"], "text": "x" }),
        json!({ "from_identity_id": IDENTITY, "to": ["a@b.com"], "subject": "", "text": "  " }),
        json!({ "from_identity_id": "x", "to": ["a@b.com"], "subject": "", "text": "x" }),
        json!({ "from_identity_id": IDENTITY, "to": ["a@b.com"], "subject": "", "text": "x", "bcc": [] }),
        json!({ "from_identity_id": IDENTITY, "to": ["a@b.com"], "subject": "", "text": "x", "in_reply_to": "x" }),
        json!({ "from_identity_id": IDENTITY, "to": [{ "addr": "a@b.com", "x": 1 }], "subject": "", "text": "x" }),
        json!({ "from_identity_id": IDENTITY, "to": ["a@b.com"], "subject": "a".repeat(999), "text": "x" }),
    ];
    for c in casos {
        assert!(req(c.clone()).is_err(), "deveria recusar: {c}");
    }
}

#[test]
fn no_maximo_50_destinatarios() {
    let to: Vec<String> = (0..30).map(|i| format!("a{i}@x.com")).collect();
    let cc: Vec<String> = (0..20).map(|i| format!("c{i}@x.com")).collect();
    let mut v =
        json!({ "from_identity_id": IDENTITY, "to": to, "cc": cc, "subject": "", "text": "x" });
    assert!(req(v.clone()).is_ok());
    v["cc"].as_array_mut().unwrap().push(json!("c99@x.com"));
    assert!(req(v).is_err());
}

#[test]
fn html_sem_texto_vale() {
    let mut v = minimal();
    v["text"] = json!("");
    v["html"] = json!("<p>oi</p>");
    assert!(req(v).is_ok());
}

#[test]
fn references_da_resposta() {
    let refs = vec!["raiz@x".to_string(), "meio@x".to_string()];
    assert_eq!(
        reply_references(&refs, Some("orig@x")),
        ["raiz@x", "meio@x", "orig@x"]
    );
    assert_eq!(
        reply_references(&refs, Some("meio@x")),
        ["raiz@x", "meio@x"]
    );
    assert_eq!(reply_references(&[], None), Vec::<String>::new());

    // Não cabe em 2048 bytes: fica a raiz e as mais novas.
    let longas: Vec<String> = (0..100).map(|i| format!("{i:040}@exemplo.com")).collect();
    let r = reply_references(&longas, Some("orig@x"));
    assert_eq!(r[0], longas[0]);
    assert_eq!(r.last().unwrap(), "orig@x");
    assert!(r.iter().map(|x| x.len() + 3).sum::<usize>() <= 2048);
}

// ------------------------------------------------------------- fluxo ----

#[derive(Default)]
struct FakeSendIndex {
    senders: HashMap<IdentityId, Sender>,
    targets: HashMap<MessageId, ReplyTarget>,
}
impl SendIndex for FakeSendIndex {
    async fn sender(&self, id: IdentityId) -> Result<Option<Sender>, PortError> {
        Ok(self.senders.get(&id).cloned())
    }
    async fn reply_target(&self, id: MessageId) -> Result<Option<ReplyTarget>, PortError> {
        Ok(self.targets.get(&id).cloned())
    }
}

struct FakeMailer {
    sent: RefCell<Vec<OutgoingMail>>,
    result: Result<String, MailerError>,
}
impl FakeMailer {
    fn ok() -> Self {
        Self {
            sent: RefCell::new(vec![]),
            result: Ok("<cf.0001@acmeco.com.br>".into()),
        }
    }
}
impl Mailer for FakeMailer {
    async fn send(&self, mail: &OutgoingMail) -> Result<String, MailerError> {
        if self.result.is_ok() {
            self.sent.borrow_mut().push(mail.clone());
        }
        self.result.clone()
    }
}

/// Grava no mesmo R2 fake que o processamento lê.
struct FakeRaw<'a> {
    blobs: &'a MemBlobs,
    jobs: RefCell<Vec<IngestJob>>,
    fail: bool,
}
impl RawWriter for FakeRaw<'_> {
    async fn write(&self, job: &IngestJob, eml: &[u8]) -> Result<(), PortError> {
        if self.fail {
            return Err(PortError("r2 fora".into()));
        }
        self.jobs.borrow_mut().push(job.clone());
        self.blobs.put(&job.raw_key, eml, "message/rfc822").await
    }
}

#[derive(Default)]
struct FakeQueue(RefCell<Vec<IngestJob>>);
impl IngestQueue for FakeQueue {
    async fn enqueue(&self, job: &IngestJob) -> Result<(), PortError> {
        self.0.borrow_mut().push(job.clone());
        Ok(())
    }
    async fn enqueue_batch(&self, jobs: &[IngestJob]) -> Result<(), PortError> {
        self.0.borrow_mut().extend_from_slice(jobs);
        Ok(())
    }
}

fn sender(addr: &str) -> Sender {
    let addr = EmailAddress::parse(addr).unwrap();
    Sender {
        domain: addr.domain(),
        name: Some("Contato Acme".into()),
        address: addr,
    }
}

struct World<'a> {
    blobs: &'a MemBlobs,
    index: &'a FakeIndex,
    send_index: FakeSendIndex,
    mailer: FakeMailer,
    raw_fail: bool,
}

impl<'a> World<'a> {
    fn new(blobs: &'a MemBlobs, index: &'a FakeIndex) -> Self {
        let mut send_index = FakeSendIndex::default();
        send_index
            .senders
            .insert(identity(), sender("contato@acmeco.com.br"));
        send_index.senders.insert(
            IdentityId::parse(OUTRA).unwrap(),
            sender("contato@outra.dev"),
        );
        Self {
            blobs,
            index,
            send_index,
            mailer: FakeMailer::ok(),
            raw_fail: false,
        }
    }

    fn send(
        &self,
        r: &SendRequest,
    ) -> (
        Result<SendOutcome, SendError>,
        Vec<IngestJob>,
        Vec<IngestJob>,
    ) {
        let process = ProcessMessage {
            blobs: self.blobs,
            index: self.index,
            clock: &FixedClock,
            ids: &SeqIds(Cell::new(500)),
            full_max_bytes: 25 * 1024 * 1024,
            max_attachments: 30,
        };
        let raw = FakeRaw {
            blobs: self.blobs,
            jobs: RefCell::new(vec![]),
            fail: self.raw_fail,
        };
        let queue = FakeQueue::default();
        let uc = SendMessage {
            mailer: &self.mailer,
            index: &self.send_index,
            raw: &raw,
            queue: &queue,
            process: &process,
        };
        let out = block_on(uc.run(r));
        (out, raw.jobs.into_inner(), queue.0.into_inner())
    }
}

#[test]
fn envio_novo_sai_e_fica_em_enviados() {
    let (blobs, index) = (MemBlobs::default(), FakeIndex::new());
    let w = World::new(&blobs, &index);
    let (out, written, queued) = w.send(&req(minimal()).unwrap());
    let out = out.unwrap();

    let mail = &w.mailer.sent.borrow()[0];
    assert_eq!(mail.from.addr.as_str(), "contato@acmeco.com.br");
    assert_eq!(mail.from.name.as_deref(), Some("Contato Acme"));
    assert!(mail.in_reply_to.is_none() && mail.references.is_empty());

    assert_eq!(out.stored, Stored::Indexed);
    assert_eq!(out.provider_id, "<cf.0001@acmeco.com.br>");
    assert!(queued.is_empty());
    let job = &written[0];
    assert!(job.sent);
    assert_eq!(job.envelope_to, "contato@acmeco.com.br");
    assert!(job.raw_key.starts_with("acmeco.com.br/raw/"));

    let r = index.get(out.id);
    assert_eq!(r.message.folder, Folder::Sent);
    assert!(r.message.is_read);
    assert_eq!(
        r.message.message_id_hdr.as_deref(),
        Some("cf.0001@acmeco.com.br")
    );
    assert_eq!(r.message.subject.as_deref(), Some("Oi"));
    assert_eq!(r.message.to[0].addr.as_str(), "joao@exemplo.com");
    assert_eq!(out.thread_id, Some(ThreadId::rooted_at(out.id)));
}

#[test]
fn resposta_entra_na_thread_da_original() {
    let (blobs, index) = (MemBlobs::default(), FakeIndex::new());
    // A original chega pelo caminho normal.
    let domain = DomainName::parse("acmeco.com.br").unwrap();
    let orig_id = MessageId::from_parts(UnixMs::new(NOW).unwrap(), 1);
    let key = RawKey::new(&domain, orig_id);
    let texto = include_bytes!("../../../mime/tests/fixtures/texto_puro.eml");
    block_on(blobs.put(key.as_str(), texto, "message/rfc822")).unwrap();
    let process = ProcessMessage {
        blobs: &blobs,
        index: &index,
        clock: &FixedClock,
        ids: &SeqIds(Cell::new(100)),
        full_max_bytes: 25 * 1024 * 1024,
        max_attachments: 30,
    };
    block_on(process.run(&IngestJob {
        raw_key: key.to_string(),
        domain: "acmeco.com.br".into(),
        envelope_from: "cliente@exemplo.com".into(),
        envelope_to: "contato@acmeco.com.br".into(),
        received_at: NOW,
        size: texto.len() as u64,
        sent: false,
    }))
    .unwrap();
    let orig = index.get(orig_id);

    let mut w = World::new(&blobs, &index);
    w.send_index.targets.insert(
        orig_id,
        ReplyTarget {
            domain,
            message_id_hdr: orig.message.message_id_hdr.clone(),
            references: vec!["raiz@cliente.com.br".into()],
            thread_id: orig.message.thread_id,
        },
    );
    let mut v = minimal();
    v["in_reply_to"] = json!(orig_id.to_string());
    let (out, _, _) = w.send(&req(v).unwrap());
    let out = out.unwrap();

    let mail = &w.mailer.sent.borrow()[0];
    assert_eq!(
        mail.in_reply_to.as_deref(),
        Some("CAF+texto.001@cliente.com.br")
    );
    assert_eq!(
        mail.references,
        ["raiz@cliente.com.br", "CAF+texto.001@cliente.com.br"]
    );
    assert_eq!(out.thread_id, orig.message.thread_id);
    let r = index.get(out.id);
    assert_eq!(r.message.thread_id, orig.message.thread_id);
    assert_eq!(r.message.folder, Folder::Sent);
}

#[test]
fn resposta_nunca_sai_de_outro_dominio() {
    let (blobs, index) = (MemBlobs::default(), FakeIndex::new());
    let mut w = World::new(&blobs, &index);
    let orig_id = MessageId::from_parts(UnixMs::new(NOW).unwrap(), 1);
    w.send_index.targets.insert(
        orig_id,
        ReplyTarget {
            domain: DomainName::parse("acmeco.com.br").unwrap(),
            message_id_hdr: Some("x@y".into()),
            references: vec![],
            thread_id: None,
        },
    );
    let mut v = minimal();
    v["in_reply_to"] = json!(orig_id.to_string());
    v["from_identity_id"] = json!(OUTRA);
    let (out, written, _) = w.send(&req(v).unwrap());
    assert!(matches!(out, Err(SendError::Invalid(_))), "{out:?}");
    assert!(w.mailer.sent.borrow().is_empty() && written.is_empty());
}

#[test]
fn identidade_ou_original_desconhecida() {
    let (blobs, index) = (MemBlobs::default(), FakeIndex::new());
    let w = World::new(&blobs, &index);
    let mut v = minimal();
    v["from_identity_id"] = json!("01M44NRD0000000000000000ZZ");
    assert_eq!(
        w.send(&req(v).unwrap()).0,
        Err(SendError::NotFound("identidade"))
    );
    let mut v = minimal();
    v["in_reply_to"] = json!("01M44NRD0000000000000000ZZ");
    assert_eq!(
        w.send(&req(v).unwrap()).0,
        Err(SendError::NotFound("mensagem original"))
    );
    assert!(w.mailer.sent.borrow().is_empty());
}

#[test]
fn recusa_do_servico_nao_grava_nada() {
    let (blobs, index) = (MemBlobs::default(), FakeIndex::new());
    let mut w = World::new(&blobs, &index);
    w.mailer.result = Err(MailerError::Rejected {
        code: "E_SENDER_NOT_VERIFIED".into(),
        message: "domínio não verificado".into(),
    });
    let (out, written, _) = w.send(&req(minimal()).unwrap());
    assert!(
        matches!(out, Err(SendError::Rejected { code, .. }) if code == "E_SENDER_NOT_VERIFIED")
    );
    assert!(written.is_empty() && index.saved.borrow().is_empty());
}

#[test]
fn mensagem_grande_demais_nem_sai() {
    let (blobs, index) = (MemBlobs::default(), FakeIndex::new());
    let w = World::new(&blobs, &index);
    let mut v = minimal();
    v["text"] = json!("x".repeat(MAX_MESSAGE_BYTES));
    assert!(matches!(
        w.send(&req(v).unwrap()).0,
        Err(SendError::Invalid(_))
    ));
    assert!(w.mailer.sent.borrow().is_empty());
}

#[test]
fn saiu_mas_r2_falhou_nao_vira_erro() {
    let (blobs, index) = (MemBlobs::default(), FakeIndex::new());
    let mut w = World::new(&blobs, &index);
    w.raw_fail = true;
    let (out, _, _) = w.send(&req(minimal()).unwrap());
    assert!(matches!(out.unwrap().stored, Stored::Lost(_)));
    assert_eq!(w.mailer.sent.borrow().len(), 1);
}

#[test]
fn indice_falhou_vai_para_a_fila() {
    // Domínio sem cadastro no índice: o processamento falha depois do envio.
    let (blobs, index) = (MemBlobs::default(), FakeIndex::new());
    let mut w = World::new(&blobs, &index);
    w.send_index
        .senders
        .insert(identity(), sender("contato@semcadastro.dev"));
    let (out, written, queued) = w.send(&req(minimal()).unwrap());
    let out = out.unwrap();
    assert!(matches!(out.stored, Stored::Pending(_)));
    assert_eq!(queued, written);
    assert!(queued[0].sent);
}

#[test]
fn id_do_servico_sem_arroba_vira_message_id_nosso() {
    let (blobs, index) = (MemBlobs::default(), FakeIndex::new());
    let mut w = World::new(&blobs, &index);
    w.mailer.result = Ok("0f9a7c".into());
    let (out, _, _) = w.send(&req(minimal()).unwrap());
    let out = out.unwrap();
    assert_eq!(
        index.get(out.id).message.message_id_hdr,
        Some(format!("{}@acmeco.com.br", out.id))
    );
}
