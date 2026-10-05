-- Schema inicial (docs seção 5.2).
-- IDs: ULID em texto. Tempo: inteiros em Unix ms (UTC). Booleanos: 0/1.
-- D1 aplica foreign keys por padrão (equivale a PRAGMA foreign_keys = on).

CREATE TABLE domains (
  id    TEXT PRIMARY KEY,
  name  TEXT NOT NULL UNIQUE,          -- acmeco.com.br
  label TEXT NOT NULL,                 -- "ACMECO"
  color TEXT NOT NULL                  -- token de cor para o front
);

CREATE TABLE identities (
  id              TEXT PRIMARY KEY,
  domain_id       TEXT NOT NULL REFERENCES domains(id),
  address         TEXT NOT NULL UNIQUE,  -- contato@acmeco.com.br
  label           TEXT,
  is_default_from INTEGER NOT NULL DEFAULT 0 CHECK (is_default_from IN (0, 1))
);
CREATE INDEX idx_identities_domain ON identities(domain_id);

CREATE TABLE threads (
  id              TEXT PRIMARY KEY,
  subject_norm    TEXT,
  last_message_at INTEGER NOT NULL,
  message_count   INTEGER NOT NULL DEFAULT 1
);

-- `seq` é o rowid declarado: estável mesmo após VACUUM. Só o FTS usa;
-- todo o resto (API, R2, front) usa `id`.
CREATE TABLE messages (
  seq             INTEGER PRIMARY KEY,
  id              TEXT NOT NULL UNIQUE,
  raw_key         TEXT NOT NULL UNIQUE,  -- garante idempotência
  domain_id       TEXT NOT NULL REFERENCES domains(id),
  identity_id     TEXT REFERENCES identities(id),
  thread_id       TEXT REFERENCES threads(id),
  envelope_from   TEXT,
  envelope_to     TEXT NOT NULL,         -- endereço pelo qual entrou (não o header To)
  message_id_hdr  TEXT,
  in_reply_to     TEXT,
  references_hdr  TEXT,
  from_addr       TEXT,
  from_name       TEXT,
  to_addrs        TEXT,                  -- JSON
  cc_addrs        TEXT,                  -- JSON
  subject         TEXT,
  snippet         TEXT,
  text_body       TEXT,                  -- truncado em 64 KB; o completo vive no R2
  html_key        TEXT,
  has_attachments INTEGER NOT NULL DEFAULT 0 CHECK (has_attachments IN (0, 1)),
  size_bytes      INTEGER NOT NULL,
  received_at     INTEGER NOT NULL,
  date_hdr        INTEGER,
  folder          TEXT NOT NULL DEFAULT 'inbox'
                  CHECK (folder IN ('inbox', 'archive', 'trash', 'spam', 'sent')),
  is_read         INTEGER NOT NULL DEFAULT 0 CHECK (is_read IN (0, 1)),
  is_starred      INTEGER NOT NULL DEFAULT 0 CHECK (is_starred IN (0, 1)),
  spf             TEXT,
  dkim            TEXT,
  dmarc           TEXT,
  deleted_at      INTEGER
);
CREATE INDEX idx_messages_list   ON messages(domain_id, folder, received_at DESC);
CREATE INDEX idx_messages_thread ON messages(thread_id, received_at);
CREATE INDEX idx_messages_msgid  ON messages(message_id_hdr);

CREATE TABLE attachments (
  id           TEXT PRIMARY KEY,
  message_id   TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  filename     TEXT,
  content_type TEXT,
  size_bytes   INTEGER NOT NULL,
  r2_key       TEXT NOT NULL,
  content_id   TEXT,
  is_inline    INTEGER NOT NULL DEFAULT 0 CHECK (is_inline IN (0, 1))
);
CREATE INDEX idx_attachments_message ON attachments(message_id);

CREATE TABLE rules (
  id            TEXT PRIMARY KEY,
  priority      INTEGER NOT NULL,
  match_address TEXT NOT NULL,           -- *@pessoal.dev, contato@acmeco.com.br
  action        TEXT NOT NULL CHECK (action IN ('move', 'archive', 'mark_read', 'reject')),
  params        TEXT,                    -- JSON
  enabled       INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
);

-- Busca por texto. Conteúdo externo: o texto vive em `messages`; o índice
-- é mantido pelos triggers abaixo, nunca pelo código.
-- Atenção: `wrangler d1 export` não suporta tabelas virtuais. Para exportar,
-- apague `messages_fts`, exporte e recrie (o .eml no R2 continua sendo o backup).
CREATE VIRTUAL TABLE messages_fts USING fts5(
  subject, from_addr, text_body,
  content='messages', content_rowid='seq'
);

CREATE TRIGGER messages_fts_ai AFTER INSERT ON messages BEGIN
  INSERT INTO messages_fts(rowid, subject, from_addr, text_body)
  VALUES (new.seq, new.subject, new.from_addr, new.text_body);
END;

CREATE TRIGGER messages_fts_ad AFTER DELETE ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, subject, from_addr, text_body)
  VALUES ('delete', old.seq, old.subject, old.from_addr, old.text_body);
END;

CREATE TRIGGER messages_fts_au AFTER UPDATE OF subject, from_addr, text_body ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, subject, from_addr, text_body)
  VALUES ('delete', old.seq, old.subject, old.from_addr, old.text_body);
  INSERT INTO messages_fts(rowid, subject, from_addr, text_body)
  VALUES (new.seq, new.subject, new.from_addr, new.text_body);
END;
