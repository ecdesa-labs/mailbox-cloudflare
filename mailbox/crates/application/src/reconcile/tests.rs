use std::cell::RefCell;
use std::collections::HashSet;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use mailbox_domain::{DomainName, MessageId, RawKey};

use super::*;
use crate::ports::{RawObject, RawPage};

fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = pin!(f);
    match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("fake não deveria ficar pendente"),
    }
}

const NOW: i64 = 1_791_158_400_000; // 2026-10-05T00:00Z
const MIN_AGE: i64 = 15 * 60 * 1000;

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> UnixMs {
        UnixMs::new(NOW).unwrap()
    }
}

fn domain() -> DomainName {
    DomainName::parse("acmeco.com.br").unwrap()
}

/// Chave raw com o instante do próprio id (como o ingest gera).
fn key(at: i64, n: u128) -> String {
    RawKey::new(
        &domain(),
        MessageId::from_parts(UnixMs::new(at).unwrap(), n),
    )
    .to_string()
}

fn job_of(k: &str) -> IngestJob {
    IngestJob {
        raw_key: k.into(),
        domain: "acmeco.com.br".into(),
        envelope_from: "a@x.com".into(),
        envelope_to: "contato@acmeco.com.br".into(),
        received_at: 0,
        size: 1,
    }
}

/// R2 fake: páginas de 2 objetos, em ordem de chave.
#[derive(Default)]
struct FakeListing {
    objects: Vec<(String, i64)>,
    heads: RefCell<usize>,
}
impl RawListing for FakeListing {
    async fn list(
        &self,
        prefix: &str,
        cursor: Option<&str>,
        with_jobs: bool,
    ) -> Result<RawPage, PortError> {
        let mut all: Vec<_> = self
            .objects
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .collect();
        all.sort();
        let start: usize = cursor.map(|c| c.parse().unwrap()).unwrap_or(0);
        let page: Vec<_> = all.iter().skip(start).take(2).collect();
        let next = start + page.len();
        Ok(RawPage {
            objects: page
                .into_iter()
                .map(|(k, t)| RawObject {
                    key: k.clone(),
                    uploaded_at: UnixMs::new(*t).unwrap(),
                    job: with_jobs.then(|| job_of(k)),
                })
                .collect(),
            cursor: (next < all.len()).then(|| next.to_string()),
        })
    }
    async fn job_for(&self, key: &str) -> Result<Option<IngestJob>, PortError> {
        *self.heads.borrow_mut() += 1;
        Ok(self
            .objects
            .iter()
            .any(|(k, _)| k == key)
            .then(|| job_of(key)))
    }
}

/// (id, cursor, enfileirados, concluído)
type Update = (i64, Option<String>, usize, bool);

#[derive(Default)]
struct FakeIndex {
    handled: HashSet<String>,
    requests: RefCell<Vec<ReprocessRequest>>,
    given_up: RefCell<Vec<String>>,
    cleared: RefCell<Vec<String>>,
    updates: RefCell<Vec<Update>>,
}
impl ReconcileIndex for FakeIndex {
    async fn handled_with_prefix(&self, prefix: &str) -> Result<HashSet<String>, PortError> {
        Ok(self
            .handled
            .iter()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect())
    }
    async fn pending_requests(&self) -> Result<Vec<ReprocessRequest>, PortError> {
        Ok(self.requests.borrow().clone())
    }
    async fn update_request(
        &self,
        id: i64,
        cursor: Option<&str>,
        enqueued: usize,
        done: bool,
    ) -> Result<(), PortError> {
        self.updates
            .borrow_mut()
            .push((id, cursor.map(str::to_owned), enqueued, done));
        Ok(())
    }
    async fn given_up(&self, limit: usize) -> Result<Vec<String>, PortError> {
        Ok(self.given_up.borrow().iter().take(limit).cloned().collect())
    }
    async fn clear_failures(&self, keys: &[String]) -> Result<(), PortError> {
        self.cleared.borrow_mut().extend(keys.iter().cloned());
        Ok(())
    }
}

#[derive(Default)]
struct FakeQueue {
    batches: RefCell<Vec<Vec<IngestJob>>>,
}
impl FakeQueue {
    fn keys(&self) -> Vec<String> {
        self.batches
            .borrow()
            .iter()
            .flatten()
            .map(|j| j.raw_key.clone())
            .collect()
    }
}
impl IngestQueue for FakeQueue {
    async fn enqueue(&self, job: &IngestJob) -> Result<(), PortError> {
        self.batches.borrow_mut().push(vec![job.clone()]);
        Ok(())
    }
    async fn enqueue_batch(&self, jobs: &[IngestJob]) -> Result<(), PortError> {
        self.batches.borrow_mut().push(jobs.to_vec());
        Ok(())
    }
}

fn run(
    listing: &FakeListing,
    index: &FakeIndex,
    queue: &FakeQueue,
    budget: usize,
) -> ReconcileReport {
    let domains = [domain()];
    block_on(
        Reconcile {
            listing,
            index,
            queue,
            clock: &FixedClock,
            domains: &domains,
            min_age_ms: MIN_AGE,
            budget,
        }
        .run(),
    )
    .unwrap()
}

#[test]
fn enfileira_so_o_que_falta_e_ja_passou_da_idade() {
    let antigo = key(NOW - 3_600_000, 1); // outubro, 1 h atrás
    let processado = key(NOW - 3_600_000, 2);
    let novo = key(NOW - 60_000, 3); // 1 min: ainda é da fila normal
    let setembro = key(NOW - 10 * 86_400_000, 4); // mês anterior
    let listing = FakeListing {
        objects: vec![
            (antigo.clone(), NOW - 3_600_000),
            (processado.clone(), NOW - 3_600_000),
            (novo.clone(), NOW - 60_000),
            (setembro.clone(), NOW - 10 * 86_400_000),
        ],
        ..Default::default()
    };
    let index = FakeIndex {
        handled: HashSet::from([processado]),
        ..Default::default()
    };
    let queue = FakeQueue::default();
    let r = run(&listing, &index, &queue, 50);
    let mut got = queue.keys();
    got.sort();
    let mut want = vec![antigo, setembro];
    want.sort();
    assert_eq!(got, want);
    assert_eq!(r.missing, 2);
    assert_eq!(queue.batches.borrow().len(), 1, "um sendBatch só");
}

#[test]
fn respeita_o_orcamento() {
    let objects: Vec<_> = (1..=5)
        .map(|n| (key(NOW - 3_600_000, n), NOW - 3_600_000))
        .collect();
    let listing = FakeListing {
        objects,
        ..Default::default()
    };
    let (index, queue) = (FakeIndex::default(), FakeQueue::default());
    let r = run(&listing, &index, &queue, 3);
    assert_eq!(r.missing, 3);
    assert_eq!(*listing.heads.borrow(), 3);
}

#[test]
fn pedido_por_chave_ignora_idade_e_limpa_falha() {
    let k = key(NOW - 60_000, 1);
    let listing = FakeListing {
        objects: vec![(k.clone(), NOW - 60_000)],
        ..Default::default()
    };
    let index = FakeIndex {
        handled: HashSet::from([k.clone()]),
        requests: RefCell::new(vec![ReprocessRequest {
            id: 7,
            mode: ReprocessMode::Key,
            raw_key: Some(k.clone()),
            cursor: None,
        }]),
        ..Default::default()
    };
    let queue = FakeQueue::default();
    let r = run(&listing, &index, &queue, 50);
    assert_eq!(queue.keys(), std::slice::from_ref(&k));
    assert_eq!((r.requested, r.missing, r.requests_done), (1, 0, 1));
    assert_eq!(*index.cleared.borrow(), [k]);
    assert_eq!(*index.updates.borrow(), [(7, None, 1, true)]);
}

#[test]
fn pedido_failed_reenfileira_as_desistidas() {
    let (a, b) = (key(NOW - 3_600_000, 1), key(NOW - 3_600_000, 2));
    let listing = FakeListing {
        objects: vec![(a.clone(), NOW - 3_600_000), (b.clone(), NOW - 3_600_000)],
        ..Default::default()
    };
    let index = FakeIndex {
        handled: HashSet::from([a.clone(), b.clone()]),
        requests: RefCell::new(vec![ReprocessRequest {
            id: 1,
            mode: ReprocessMode::Failed,
            raw_key: None,
            cursor: None,
        }]),
        given_up: RefCell::new(vec![a.clone(), b.clone()]),
        ..Default::default()
    };
    let queue = FakeQueue::default();
    run(&listing, &index, &queue, 50);
    assert_eq!(queue.keys(), [a.clone(), b.clone()]);
    assert_eq!(*index.cleared.borrow(), [a, b]);
    assert_eq!(*index.updates.borrow(), [(1, None, 2, true)]);
}

#[test]
fn pedido_all_pagina_e_guarda_cursor() {
    let objects: Vec<_> = (1..=5)
        .map(|n| (key(NOW - 3_600_000, n), NOW - 3_600_000))
        .collect();
    let listing = FakeListing {
        objects: objects.clone(),
        ..Default::default()
    };
    let all: HashSet<String> = objects.iter().map(|(k, _)| k.clone()).collect();
    let index = FakeIndex {
        handled: all.clone(),
        requests: RefCell::new(vec![ReprocessRequest {
            id: 3,
            mode: ReprocessMode::All,
            raw_key: None,
            cursor: None,
        }]),
        ..Default::default()
    };
    let queue = FakeQueue::default();
    // Orçamento 3: duas páginas de 2 cabem (4 jobs), a terceira fica para depois.
    run(&listing, &index, &queue, 3);
    assert_eq!(queue.keys().len(), 4);
    assert_eq!(
        *listing.heads.borrow(),
        0,
        "modo all usa os metadados da listagem"
    );
    let (id, cursor, n, done) = index.updates.borrow()[0].clone();
    assert_eq!((id, n, done), (3, 4, false));
    assert_eq!(cursor.as_deref(), Some("0\t4"));

    // Próxima execução continua do cursor e termina.
    index.requests.borrow_mut()[0].cursor = cursor;
    let queue2 = FakeQueue::default();
    run(&listing, &index, &queue2, 3);
    assert_eq!(queue2.keys().len(), 1);
    assert_eq!(index.updates.borrow()[1], (3, None, 1, true));
}

#[test]
fn prefixos_do_mes_atual_e_anterior() {
    let jan = UnixMs::new(1_767_225_600_000).unwrap(); // 2026-01-01
    assert_eq!(
        recent_month_prefixes(&domain(), jan),
        [
            "acmeco.com.br/raw/2026/01/".to_string(),
            "acmeco.com.br/raw/2025/12/".to_string()
        ]
    );
}
