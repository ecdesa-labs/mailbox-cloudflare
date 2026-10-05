-- Fase 5: lista da caixa unificada (sem filtro de domínio) e paginação por
-- cursor (received_at DESC, id DESC). Só mensagens visíveis.
CREATE INDEX idx_messages_folder ON messages(folder, received_at DESC, id DESC)
  WHERE deleted_at IS NULL;
CREATE INDEX idx_messages_domain_folder ON messages(domain_id, folder, received_at DESC, id DESC)
  WHERE deleted_at IS NULL;
-- Substituído pelo índice parcial acima.
DROP INDEX idx_messages_list;
