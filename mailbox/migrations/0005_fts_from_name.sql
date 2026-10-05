-- Fase 7: a busca também acha pelo nome do remetente ("João", "GitHub").
-- FTS5 não aceita ALTER para colunas novas: recria o índice e os triggers
-- e reconstrói a partir de `messages` (fonte do conteúdo externo).

DROP TRIGGER messages_fts_ai;
DROP TRIGGER messages_fts_ad;
DROP TRIGGER messages_fts_au;
DROP TABLE messages_fts;

CREATE VIRTUAL TABLE messages_fts USING fts5(
  subject, from_name, from_addr, text_body,
  content='messages', content_rowid='seq'
);

CREATE TRIGGER messages_fts_ai AFTER INSERT ON messages BEGIN
  INSERT INTO messages_fts(rowid, subject, from_name, from_addr, text_body)
  VALUES (new.seq, new.subject, new.from_name, new.from_addr, new.text_body);
END;

CREATE TRIGGER messages_fts_ad AFTER DELETE ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, subject, from_name, from_addr, text_body)
  VALUES ('delete', old.seq, old.subject, old.from_name, old.from_addr, old.text_body);
END;

CREATE TRIGGER messages_fts_au AFTER UPDATE OF subject, from_name, from_addr, text_body ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, subject, from_name, from_addr, text_body)
  VALUES ('delete', old.seq, old.subject, old.from_name, old.from_addr, old.text_body);
  INSERT INTO messages_fts(rowid, subject, from_name, from_addr, text_body)
  VALUES (new.seq, new.subject, new.from_name, new.from_addr, new.text_body);
END;

INSERT INTO messages_fts(messages_fts) VALUES ('rebuild');
