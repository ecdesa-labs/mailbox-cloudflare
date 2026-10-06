-- Fase 9: retenção. A lixeira e o spam contam a partir de quando a mensagem
-- foi para lá (NULL = desde que chegou, ex.: regra que manda para spam).
ALTER TABLE messages ADD COLUMN folder_changed_at INTEGER;

CREATE INDEX idx_messages_retention
  ON messages(folder, COALESCE(folder_changed_at, received_at))
  WHERE folder IN ('trash', 'spam');

CREATE INDEX idx_messages_hidden ON messages(deleted_at) WHERE deleted_at IS NOT NULL;
