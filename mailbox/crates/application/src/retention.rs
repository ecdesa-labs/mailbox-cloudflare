//! Retenção (Fase 9): apaga de vez lixeira, spam e mensagens ocultas por
//! regra depois de N dias. Rodado pelo cron, com orçamento por execução.
//!
//! Ordem importa: primeiro o R2 (o `.eml` inclusive), depois o índice. Se o
//! `.eml` ficasse, a reconciliação (seção 6.6) veria um raw sem mensagem e
//! a mensagem apagada voltaria. Se o R2 apagar e o D1 falhar, a linha fica
//! e a próxima execução tenta de novo (apagar no R2 é idempotente).

use mailbox_domain::{MessageId, UnixMs};

use crate::ports::{Clock, PortError};

const DAY_MS: i64 = 86_400_000;

/// Prazos em dias. `0` = nunca apaga.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub trash_days: u32,
    pub spam_days: u32,
    /// Mensagens ocultas por regra `reject`.
    pub hidden_days: u32,
}

/// Limites de corte (Unix ms). `None` = categoria desligada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cutoffs {
    pub trash: Option<UnixMs>,
    pub spam: Option<UnixMs>,
    pub hidden: Option<UnixMs>,
}

impl RetentionPolicy {
    pub fn cutoffs(&self, now: UnixMs) -> Cutoffs {
        let cut = |days: u32| {
            (days > 0).then(|| {
                UnixMs::new((now.as_i64() - i64::from(days) * DAY_MS).max(0)).expect("não negativo")
            })
        };
        Cutoffs {
            trash: cut(self.trash_days),
            spam: cut(self.spam_days),
            hidden: cut(self.hidden_days),
        }
    }
}

/// Mensagem vencida, com todos os objetos dela no R2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expired {
    pub id: MessageId,
    /// `.eml`, HTML e anexos.
    pub blob_keys: Vec<String>,
}

#[allow(async_fn_in_trait)]
pub trait RetentionStore {
    async fn expired(&self, cutoffs: &Cutoffs, limit: usize) -> Result<Vec<Expired>, PortError>;
    /// Apaga as linhas (anexos em cascata, FTS por trigger) e acerta as threads.
    async fn purge(&self, ids: &[MessageId]) -> Result<(), PortError>;
}

#[allow(async_fn_in_trait)]
pub trait BlobPurge {
    /// Apaga de uma vez (até 1000 chaves por chamada no R2).
    async fn delete_many(&self, keys: &[String]) -> Result<(), PortError>;
}

pub struct Retention<'a, S, B, C> {
    pub store: &'a S,
    pub blobs: &'a B,
    pub clock: &'a C,
    pub policy: RetentionPolicy,
    /// Mensagens por execução.
    pub batch: usize,
}

impl<S: RetentionStore, B: BlobPurge, C: Clock> Retention<'_, S, B, C> {
    /// Quantas mensagens foram apagadas.
    pub async fn run(&self) -> Result<usize, PortError> {
        let cutoffs = self.policy.cutoffs(self.clock.now());
        if cutoffs.trash.is_none() && cutoffs.spam.is_none() && cutoffs.hidden.is_none() {
            return Ok(0);
        }
        let expired = self.store.expired(&cutoffs, self.batch).await?;
        if expired.is_empty() {
            return Ok(0);
        }
        let keys: Vec<String> = expired
            .iter()
            .flat_map(|e| e.blob_keys.iter().cloned())
            .collect();
        for chunk in keys.chunks(1000) {
            self.blobs.delete_many(chunk).await?;
        }
        let ids: Vec<MessageId> = expired.iter().map(|e| e.id).collect();
        self.store.purge(&ids).await?;
        Ok(ids.len())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
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

    const NOW: i64 = 100 * DAY_MS;

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> UnixMs {
            UnixMs::new(NOW).unwrap()
        }
    }

    /// (id, pasta, desde quando, oculta desde)
    type Row = (MessageId, &'static str, i64, Option<i64>);

    #[derive(Default)]
    struct Fake {
        rows: RefCell<Vec<Row>>,
        deleted_blobs: RefCell<Vec<String>>,
        order: RefCell<Vec<&'static str>>,
        fail_purge: bool,
    }

    impl RetentionStore for Fake {
        async fn expired(&self, c: &Cutoffs, limit: usize) -> Result<Vec<Expired>, PortError> {
            let lt = |t: i64, cut: Option<UnixMs>| cut.is_some_and(|c| t < c.as_i64());
            Ok(self
                .rows
                .borrow()
                .iter()
                .filter(|(_, folder, since, hidden)| match (*folder, hidden) {
                    (_, Some(h)) => lt(*h, c.hidden),
                    ("trash", None) => lt(*since, c.trash),
                    ("spam", None) => lt(*since, c.spam),
                    _ => false,
                })
                .take(limit)
                .map(|(id, ..)| Expired {
                    id: *id,
                    blob_keys: vec![format!("raw/{id}"), format!("att/{id}/0")],
                })
                .collect())
        }
        async fn purge(&self, ids: &[MessageId]) -> Result<(), PortError> {
            self.order.borrow_mut().push("d1");
            if self.fail_purge {
                return Err(PortError("d1 fora".into()));
            }
            self.rows.borrow_mut().retain(|r| !ids.contains(&r.0));
            Ok(())
        }
    }

    impl BlobPurge for Fake {
        async fn delete_many(&self, keys: &[String]) -> Result<(), PortError> {
            self.order.borrow_mut().push("r2");
            self.deleted_blobs.borrow_mut().extend(keys.iter().cloned());
            Ok(())
        }
    }

    fn id(n: u128) -> MessageId {
        MessageId::from_parts(UnixMs::new(0).unwrap(), n)
    }

    fn run(f: &Fake, policy: RetentionPolicy, batch: usize) -> Result<usize, PortError> {
        block_on(
            Retention {
                store: f,
                blobs: f,
                clock: &FixedClock,
                policy,
                batch,
            }
            .run(),
        )
    }

    const P30: RetentionPolicy = RetentionPolicy {
        trash_days: 30,
        spam_days: 30,
        hidden_days: 30,
    };

    #[test]
    fn apaga_so_o_vencido_e_r2_antes_do_d1() {
        let f = Fake::default();
        *f.rows.borrow_mut() = vec![
            (id(1), "trash", NOW - 31 * DAY_MS, None),      // vencida
            (id(2), "trash", NOW - 29 * DAY_MS, None),      // ainda não
            (id(3), "spam", NOW - 40 * DAY_MS, None),       // vencida
            (id(4), "inbox", NOW - 400 * DAY_MS, None),     // entrada nunca expira
            (id(5), "inbox", NOW, Some(NOW - 31 * DAY_MS)), // oculta por regra, vencida
            (id(6), "archive", NOW - 400 * DAY_MS, None),
        ];
        assert_eq!(run(&f, P30, 50), Ok(3));
        let left: Vec<_> = f.rows.borrow().iter().map(|r| r.0).collect();
        assert_eq!(left, [id(2), id(4), id(6)]);
        assert!(
            f.deleted_blobs.borrow().contains(&format!("raw/{}", id(1))),
            "o .eml vai junto"
        );
        assert_eq!(*f.order.borrow(), ["r2", "d1"]);
    }

    #[test]
    fn zero_dias_desliga() {
        let f = Fake::default();
        *f.rows.borrow_mut() = vec![(id(1), "trash", 0, None), (id(2), "spam", 0, None)];
        let p = RetentionPolicy {
            trash_days: 0,
            spam_days: 30,
            hidden_days: 0,
        };
        assert_eq!(run(&f, p, 50), Ok(1));
        assert_eq!(f.rows.borrow()[0].0, id(1), "lixeira ficou");
        let off = RetentionPolicy {
            trash_days: 0,
            spam_days: 0,
            hidden_days: 0,
        };
        assert_eq!(run(&f, off, 50), Ok(0));
        assert!(f.order.borrow().len() == 2, "desligado nem consulta");
    }

    #[test]
    fn respeita_o_lote() {
        let f = Fake::default();
        *f.rows.borrow_mut() = (1..=5).map(|n| (id(n), "trash", 0, None)).collect();
        assert_eq!(run(&f, P30, 2), Ok(2));
        assert_eq!(f.rows.borrow().len(), 3);
    }

    #[test]
    fn d1_falhou_tenta_de_novo_depois() {
        let f = Fake {
            fail_purge: true,
            ..Default::default()
        };
        *f.rows.borrow_mut() = vec![(id(1), "trash", 0, None)];
        assert!(run(&f, P30, 50).is_err());
        assert_eq!(
            f.rows.borrow().len(),
            1,
            "linha continua para a próxima execução"
        );
    }

    #[test]
    fn cortes() {
        let c = P30.cutoffs(UnixMs::new(NOW).unwrap());
        assert_eq!(c.trash, Some(UnixMs::new(70 * DAY_MS).unwrap()));
        let c = RetentionPolicy {
            trash_days: 1000,
            spam_days: 0,
            hidden_days: 0,
        }
        .cutoffs(UnixMs::new(NOW).unwrap());
        assert_eq!(
            (c.trash, c.spam),
            (Some(UnixMs::new(0).unwrap()), None),
            "não fica negativo"
        );
    }
}
