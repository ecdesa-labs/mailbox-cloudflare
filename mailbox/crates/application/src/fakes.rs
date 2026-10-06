//! Fakes das portas, compartilhados pelos testes dos casos de uso.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use mailbox_domain::{
    DomainId, DomainName, EmailAddress, IdentityId, MessageId, Rule, ThreadId, UnixMs,
};

use crate::ports::{
    BlobStore, Clock, ExistingMessage, IdGenerator, MailIndex, MessageRecord, PortError,
};

pub(crate) fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = pin!(f);
    match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("fake não deveria ficar pendente"),
    }
}

pub(crate) const NOW: i64 = 1_791_158_400_000;

pub(crate) struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> UnixMs {
        UnixMs::new(NOW).unwrap()
    }
}

pub(crate) struct SeqIds(pub(crate) Cell<u128>);
impl IdGenerator for SeqIds {
    fn message_id(&self, at: UnixMs) -> MessageId {
        self.0.set(self.0.get() + 1);
        MessageId::from_parts(at, self.0.get())
    }
    fn identity_id(&self, at: UnixMs) -> IdentityId {
        self.0.set(self.0.get() + 1);
        IdentityId::from_parts(at, self.0.get())
    }
    fn rule_id(&self, at: UnixMs) -> mailbox_domain::RuleId {
        mailbox_domain::RuleId::from_parts(at, 0)
    }
}

#[derive(Default)]
pub(crate) struct MemBlobs {
    pub(crate) objects: RefCell<HashMap<String, (Vec<u8>, String)>>,
    pub(crate) fail_get: bool,
}
impl MemBlobs {
    pub(crate) fn with(key: &str, data: &[u8]) -> Self {
        let b = Self::default();
        b.objects
            .borrow_mut()
            .insert(key.into(), (data.to_vec(), "message/rfc822".into()));
        b
    }
    pub(crate) fn text(&self, key: &str) -> String {
        String::from_utf8(self.objects.borrow()[key].0.clone()).unwrap()
    }
}
impl BlobStore for MemBlobs {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, PortError> {
        if self.fail_get {
            return Err(PortError("r2 fora".into()));
        }
        Ok(self.objects.borrow().get(key).map(|(d, _)| d.clone()))
    }
    async fn put(&self, key: &str, data: &[u8], content_type: &str) -> Result<(), PortError> {
        self.objects
            .borrow_mut()
            .insert(key.into(), (data.to_vec(), content_type.into()));
        Ok(())
    }
}

pub(crate) struct FakeIndex {
    pub(crate) domain: (DomainName, DomainId),
    pub(crate) rules: Vec<Rule>,
    pub(crate) saved: RefCell<Vec<MessageRecord>>,
    pub(crate) identities: RefCell<HashMap<String, IdentityId>>,
}
impl FakeIndex {
    pub(crate) fn new() -> Self {
        Self {
            domain: (
                DomainName::parse("acmeco.com.br").unwrap(),
                DomainId::parse("01M44NRD000000000000000001").unwrap(),
            ),
            rules: vec![],
            saved: RefCell::new(vec![]),
            identities: RefCell::new(HashMap::new()),
        }
    }
    pub(crate) fn get(&self, id: MessageId) -> MessageRecord {
        self.saved
            .borrow()
            .iter()
            .find(|r| r.message.id == id)
            .cloned()
            .expect("salvo")
    }
}
impl MailIndex for FakeIndex {
    async fn domain_id(&self, name: &DomainName) -> Result<Option<DomainId>, PortError> {
        Ok((name == &self.domain.0).then_some(self.domain.1))
    }
    async fn existing(&self, id: MessageId) -> Result<Option<ExistingMessage>, PortError> {
        Ok(self
            .saved
            .borrow()
            .iter()
            .find(|r| r.message.id == id)
            .map(|r| ExistingMessage {
                thread_id: r.message.thread_id,
            }))
    }
    async fn thread_of(&self, ids: &[String]) -> Result<Option<ThreadId>, PortError> {
        Ok(self
            .saved
            .borrow()
            .iter()
            .find(|r| {
                r.message
                    .message_id_hdr
                    .as_ref()
                    .is_some_and(|m| ids.contains(m))
            })
            .and_then(|r| r.message.thread_id))
    }
    async fn thread_of_reply_to(&self, mid: &str) -> Result<Option<ThreadId>, PortError> {
        Ok(self
            .saved
            .borrow()
            .iter()
            .find(|r| r.message.in_reply_to.as_deref() == Some(mid))
            .and_then(|r| r.message.thread_id))
    }
    async fn rules(&self) -> Result<Vec<Rule>, PortError> {
        Ok(self.rules.clone())
    }
    async fn ensure_identity(
        &self,
        _domain_id: DomainId,
        address: &EmailAddress,
        new_id: IdentityId,
    ) -> Result<IdentityId, PortError> {
        Ok(*self
            .identities
            .borrow_mut()
            .entry(address.to_string())
            .or_insert(new_id))
    }
    async fn save(&self, record: &MessageRecord) -> Result<(), PortError> {
        let mut saved = self.saved.borrow_mut();
        saved.retain(|r| r.message.id != record.message.id);
        saved.push(record.clone());
        Ok(())
    }
}
