//! Recebimento de email (docs seção 6.1): grava o `.eml` e enfileira.
//! Nada de parsing aqui.

use mailbox_domain::{DomainName, EmailAddress, RawKey};

use crate::ports::{Clock, IdGenerator, IngestJob, IngestQueue, PortError, RawMailStore};

/// O que o Email Routing entrega antes de qualquer byte do corpo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub from: String,
    pub to: String,
    pub size: u64,
}

/// Decisão para o handler. Só `Rejected` vira `setReject` (permanente).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestOutcome {
    /// Gravado e enfileirado.
    Accepted { raw_key: RawKey },
    /// Gravado, mas a fila falhou. O email está salvo: aceitar e logar; a
    /// reconciliação (seção 6.6) enfileira depois.
    AcceptedNotQueued { raw_key: RawKey, error: PortError },
    /// Destinatário que esta instância não atende. Rejeição permanente.
    Rejected { reason: &'static str },
    /// Nada foi salvo. O handler devolve erro (nunca `setReject`, que
    /// perderia o email) para o remetente tentar de novo.
    TemporaryFailure { error: PortError },
}

pub struct IngestEmail<'a, C, G, S, Q> {
    pub clock: &'a C,
    pub ids: &'a G,
    pub store: &'a S,
    pub queue: &'a Q,
    /// Domínios atendidos pela instância (gerado do `instance.json`).
    pub domains: &'a [DomainName],
}

impl<C, G, S, Q> IngestEmail<'_, C, G, S, Q>
where
    C: Clock,
    G: IdGenerator,
    S: RawMailStore,
    Q: IngestQueue,
{
    pub async fn run(&self, envelope: &Envelope) -> IngestOutcome {
        let Ok(to) = EmailAddress::parse(&envelope.to) else {
            return IngestOutcome::Rejected {
                reason: "invalid recipient",
            };
        };
        let Some(domain) = self.domains.iter().find(|d| d.as_str() == to.domain_str()) else {
            return IngestOutcome::Rejected {
                reason: "unknown domain",
            };
        };

        let received_at = self.clock.now();
        let id = self.ids.message_id(received_at);
        let raw_key = RawKey::new(domain, id);

        let job = IngestJob {
            raw_key: raw_key.to_string(),
            domain: domain.to_string(),
            envelope_from: envelope.from.trim().to_owned(),
            envelope_to: to.to_string(),
            received_at: received_at.as_i64(),
            size: envelope.size,
        };

        if let Err(error) = self.store.store(&job).await {
            return IngestOutcome::TemporaryFailure { error };
        }

        match self.queue.enqueue(&job).await {
            Ok(()) => IngestOutcome::Accepted { raw_key },
            Err(error) => IngestOutcome::AcceptedNotQueued { raw_key, error },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use mailbox_domain::{MessageId, UnixMs};

    use super::*;

    /// Os fakes respondem na hora; basta um poll.
    fn block_on<F: Future>(f: F) -> F::Output {
        let mut f = pin!(f);
        match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("fake não deveria ficar pendente"),
        }
    }

    const NOW: i64 = 1_791_158_400_000; // 2026-10-05T00:00Z

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> UnixMs {
            UnixMs::new(NOW).unwrap()
        }
    }

    struct SeqIds(Cell<u128>);
    impl IdGenerator for SeqIds {
        fn message_id(&self, at: UnixMs) -> MessageId {
            let n = self.0.get() + 1;
            self.0.set(n);
            MessageId::from_parts(at, n)
        }
        fn identity_id(&self, at: UnixMs) -> mailbox_domain::IdentityId {
            mailbox_domain::IdentityId::from_parts(at, 0)
        }
    }

    #[derive(Default)]
    struct FakeStore {
        fail: bool,
        stored: RefCell<Vec<String>>,
    }
    impl RawMailStore for FakeStore {
        async fn store(&self, job: &IngestJob) -> Result<(), PortError> {
            if self.fail {
                return Err(PortError("r2 fora".into()));
            }
            self.stored.borrow_mut().push(job.raw_key.clone());
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeQueue {
        fail: bool,
        jobs: RefCell<Vec<IngestJob>>,
    }
    impl IngestQueue for FakeQueue {
        async fn enqueue(&self, job: &IngestJob) -> Result<(), PortError> {
            if self.fail {
                return Err(PortError("fila fora".into()));
            }
            self.jobs.borrow_mut().push(job.clone());
            Ok(())
        }
        async fn enqueue_batch(&self, jobs: &[IngestJob]) -> Result<(), PortError> {
            self.jobs.borrow_mut().extend(jobs.iter().cloned());
            Ok(())
        }
    }

    fn domains() -> Vec<DomainName> {
        vec![DomainName::parse("acmeco.com.br").unwrap()]
    }

    fn envelope(to: &str) -> Envelope {
        Envelope {
            from: "Cliente@Exemplo.com".into(),
            to: to.into(),
            size: 4321,
        }
    }

    fn run(store: &FakeStore, queue: &FakeQueue, env: &Envelope) -> IngestOutcome {
        let domains = domains();
        let uc = IngestEmail {
            clock: &FixedClock,
            ids: &SeqIds(Cell::new(0)),
            store,
            queue,
            domains: &domains,
        };
        block_on(uc.run(env))
    }

    #[test]
    fn grava_e_enfileira() {
        let (store, queue) = (FakeStore::default(), FakeQueue::default());
        let out = run(&store, &queue, &envelope("Contato@ACMECO.com.br"));

        let key = "acmeco.com.br/raw/2026/10/01M44NRD000000000000000001.eml";
        assert!(
            matches!(&out, IngestOutcome::Accepted { raw_key } if raw_key.as_str() == key),
            "{out:?}"
        );
        assert_eq!(*store.stored.borrow(), [key]);
        assert_eq!(
            *queue.jobs.borrow(),
            [IngestJob {
                raw_key: key.into(),
                domain: "acmeco.com.br".into(),
                envelope_from: "Cliente@Exemplo.com".into(),
                envelope_to: "contato@acmeco.com.br".into(),
                received_at: NOW,
                size: 4321,
            }]
        );
    }

    #[test]
    fn dominio_desconhecido_rejeita_sem_gravar() {
        let (store, queue) = (FakeStore::default(), FakeQueue::default());
        for to in ["x@outro.dev", "x@sub.acmeco.com.br"] {
            let out = run(&store, &queue, &envelope(to));
            assert_eq!(
                out,
                IngestOutcome::Rejected {
                    reason: "unknown domain"
                },
                "{to}"
            );
        }
        assert!(store.stored.borrow().is_empty());
        assert!(queue.jobs.borrow().is_empty());
    }

    #[test]
    fn destinatario_invalido_rejeita() {
        let (store, queue) = (FakeStore::default(), FakeQueue::default());
        let out = run(&store, &queue, &envelope("sem-arroba"));
        assert_eq!(
            out,
            IngestOutcome::Rejected {
                reason: "invalid recipient"
            }
        );
        assert!(store.stored.borrow().is_empty());
    }

    #[test]
    fn falha_no_r2_e_temporaria_e_nao_enfileira() {
        let store = FakeStore {
            fail: true,
            ..Default::default()
        };
        let queue = FakeQueue::default();
        let out = run(&store, &queue, &envelope("contato@acmeco.com.br"));
        assert_eq!(
            out,
            IngestOutcome::TemporaryFailure {
                error: PortError("r2 fora".into())
            }
        );
        assert!(queue.jobs.borrow().is_empty());
    }

    #[test]
    fn falha_na_fila_aceita_porque_ja_gravou() {
        let store = FakeStore::default();
        let queue = FakeQueue {
            fail: true,
            ..Default::default()
        };
        let out = run(&store, &queue, &envelope("contato@acmeco.com.br"));
        assert!(
            matches!(out, IngestOutcome::AcceptedNotQueued { .. }),
            "{out:?}"
        );
        assert_eq!(store.stored.borrow().len(), 1);
    }

    #[test]
    fn bounce_sem_remetente() {
        let (store, queue) = (FakeStore::default(), FakeQueue::default());
        let env = Envelope {
            from: "".into(),
            ..envelope("contato@acmeco.com.br")
        };
        run(&store, &queue, &env);
        assert_eq!(queue.jobs.borrow()[0].envelope_from, "");
    }
}
