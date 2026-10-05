//! Reconciliação e reprocessamento (docs seção 6.6), rodado pelo cron.
//!
//! O R2 é a fonte da verdade e o processamento é idempotente, então
//! reenfileirar é sempre seguro. Cada execução tem um orçamento de jobs para
//! caber nas 50 sub-requisições e nos 10 ms de CPU do plano Free; o que não
//! couber fica para a próxima.

use mailbox_domain::{DomainName, UnixMs};

use crate::ports::{
    Clock, IngestJob, IngestQueue, PortError, RawListing, ReconcileIndex, ReprocessMode,
    ReprocessRequest,
};

/// Tamanho máximo de um `sendBatch`.
const QUEUE_BATCH: usize = 100;
/// Páginas de listagem por prefixo e execução (1000 objetos cada).
const MAX_PAGES: usize = 5;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    /// `.eml` no R2 sem linha no índice nem falha registrada.
    pub missing: usize,
    /// Enfileirados por pedidos de `just reprocess`.
    pub requested: usize,
    pub requests_done: usize,
}

pub struct Reconcile<'a, L, X, Q, C> {
    pub listing: &'a L,
    pub index: &'a X,
    pub queue: &'a Q,
    pub clock: &'a C,
    pub domains: &'a [DomainName],
    /// Objetos mais novos que isso ainda estão com a fila normal.
    pub min_age_ms: i64,
    /// Máximo de jobs enfileirados por execução.
    pub budget: usize,
}

/// Atualização de pedido, aplicada só depois de enfileirar.
struct RequestUpdate {
    id: i64,
    cursor: Option<String>,
    enqueued: usize,
    done: bool,
}

impl<L, X, Q, C> Reconcile<'_, L, X, Q, C>
where
    L: RawListing,
    X: ReconcileIndex,
    Q: IngestQueue,
    C: Clock,
{
    pub async fn run(&self) -> Result<ReconcileReport, PortError> {
        let mut jobs: Vec<IngestJob> = Vec::new();
        let mut updates: Vec<RequestUpdate> = Vec::new();
        let mut cleared: Vec<String> = Vec::new();

        // 1. Pedidos explícitos primeiro, em ordem.
        for req in self.index.pending_requests().await? {
            let room = self.budget.saturating_sub(jobs.len());
            if room == 0 {
                break;
            }
            let before = jobs.len();
            let (cursor, done) = match req.mode {
                ReprocessMode::Key => {
                    let key = req.raw_key.clone().unwrap_or_default();
                    jobs.extend(self.listing.job_for(&key).await?);
                    cleared.push(key);
                    (None, true)
                }
                ReprocessMode::Failed => {
                    let keys = self.index.given_up(room).await?;
                    for k in &keys {
                        jobs.extend(self.listing.job_for(k).await?);
                    }
                    let done = keys.len() < room;
                    cleared.extend(keys);
                    (None, done)
                }
                ReprocessMode::All => self.all_page(&req, room, &mut jobs).await?,
            };
            updates.push(RequestUpdate {
                id: req.id,
                cursor,
                enqueued: jobs.len() - before,
                done,
            });
        }
        let requested = jobs.len();

        // 2. O que está no R2 e o índice não conhece (ex.: R2 gravou e a fila falhou).
        let cutoff = self.clock.now().as_i64() - self.min_age_ms;
        'domains: for domain in self.domains {
            for prefix in recent_month_prefixes(domain, self.clock.now()) {
                if jobs.len() >= self.budget {
                    break 'domains;
                }
                let handled = self.index.handled_with_prefix(&prefix).await?;
                let mut cursor: Option<String> = None;
                for _ in 0..MAX_PAGES {
                    let page = self.listing.list(&prefix, cursor.as_deref(), false).await?;
                    for obj in page.objects {
                        if jobs.len() >= self.budget {
                            break 'domains;
                        }
                        if obj.uploaded_at.as_i64() <= cutoff && !handled.contains(&obj.key) {
                            jobs.extend(self.listing.job_for(&obj.key).await?);
                        }
                    }
                    match page.cursor {
                        Some(c) => cursor = Some(c),
                        None => break,
                    }
                }
            }
        }

        for chunk in jobs.chunks(QUEUE_BATCH) {
            self.queue.enqueue_batch(chunk).await?;
        }
        if !cleared.is_empty() {
            self.index.clear_failures(&cleared).await?;
        }
        for u in &updates {
            self.index
                .update_request(u.id, u.cursor.as_deref(), u.enqueued, u.done)
                .await?;
        }

        Ok(ReconcileReport {
            missing: jobs.len() - requested,
            requested,
            requests_done: updates.iter().filter(|u| u.done).count(),
        })
    }

    /// Modo `all`: uma página por vez, com cursor `<índice do domínio>\t<cursor do R2>`.
    async fn all_page(
        &self,
        req: &ReprocessRequest,
        room: usize,
        jobs: &mut Vec<IngestJob>,
    ) -> Result<(Option<String>, bool), PortError> {
        let (mut d, mut r2_cursor) = parse_all_cursor(req.cursor.as_deref());
        let start = jobs.len();
        while d < self.domains.len() && jobs.len() - start < room {
            let prefix = format!("{}/raw/", self.domains[d]);
            let page = self
                .listing
                .list(&prefix, r2_cursor.as_deref(), true)
                .await?;
            jobs.extend(page.objects.into_iter().filter_map(|o| o.job));
            match page.cursor {
                Some(c) => r2_cursor = Some(c),
                None => {
                    d += 1;
                    r2_cursor = None;
                }
            }
        }
        let done = d >= self.domains.len();
        let cursor = (!done).then(|| format!("{d}\t{}", r2_cursor.unwrap_or_default()));
        Ok((cursor, done))
    }
}

fn parse_all_cursor(c: Option<&str>) -> (usize, Option<String>) {
    let Some((d, r2)) = c.and_then(|c| c.split_once('\t')) else {
        return (0, None);
    };
    (
        d.parse().unwrap_or(0),
        (!r2.is_empty()).then(|| r2.to_owned()),
    )
}

/// `raw/` do mês atual e do anterior (o `.eml` de 23h59 do dia 31 ainda
/// pode estar pendente no dia 1).
fn recent_month_prefixes(domain: &DomainName, now: UnixMs) -> [String; 2] {
    let (y, m) = now.year_month();
    let (py, pm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
    [
        format!("{domain}/raw/{y:04}/{m:02}/"),
        format!("{domain}/raw/{py:04}/{pm:02}/"),
    ]
}

#[cfg(test)]
mod tests;
