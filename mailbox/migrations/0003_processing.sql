-- Fase 4: modo leve, falhas de processamento e pedidos de reprocessamento
-- (docs seções 6.2 e 6.6).

-- full = corpo, anexos, HTML e busca; partial = só headers (modo leve).
ALTER TABLE messages ADD COLUMN processing TEXT NOT NULL DEFAULT 'full'
  CHECK (processing IN ('full', 'partial'));

-- Achar a thread de mensagens que chegaram antes da mensagem que elas respondem.
CREATE INDEX idx_messages_in_reply_to ON messages(in_reply_to);

-- Uma linha por .eml que falhou. Some quando o processamento dá certo.
-- `gave_up_at` preenchido = esgotou as tentativas (veio da DLQ); a
-- reconciliação não reenfileira até alguém pedir `just reprocess --failed`.
CREATE TABLE ingest_failures (
  raw_key    TEXT PRIMARY KEY,
  attempts   INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  updated_at INTEGER NOT NULL,
  gave_up_at INTEGER
);

-- Gravado por `just reprocess` (wrangler d1 execute); executado pelo cron.
CREATE TABLE reprocess_requests (
  id         INTEGER PRIMARY KEY,
  mode       TEXT NOT NULL CHECK (mode IN ('key', 'failed', 'all')),
  raw_key    TEXT,                       -- só no modo 'key'
  cursor     TEXT,                       -- paginação do modo 'all'
  enqueued   INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  done_at    INTEGER,
  CHECK ((mode = 'key') = (raw_key IS NOT NULL))
);
CREATE INDEX idx_reprocess_pending ON reprocess_requests(done_at, id);
