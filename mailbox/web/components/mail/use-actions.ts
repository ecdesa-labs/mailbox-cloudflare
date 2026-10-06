'use client'

import { useCallback } from 'react'
import { useQueryClient, type InfiniteData } from '@tanstack/react-query'
import { apiClient } from '@/lib/api/client'
import { queryKeys } from '@/lib/api/queries'
import type { Folder, ListItem, MessagePatch, Page, Thread } from '@/lib/api/types'
import { FOLDER_LABELS } from './format'

/** Limite do `POST /api/messages/bulk`. */
const BULK_MAX = 100

type ListKey = [string, string, { folder?: string; domain?: string; q?: string }]

export interface Undo {
  text: string
  undo?: () => void
}

/**
 * Ações sobre mensagens com atualização otimista: a lista muda na hora e
 * volta ao estado anterior se a API falhar. Mover entre pastas devolve um
 * "Desfazer" (que move cada mensagem de volta para a pasta de origem).
 */
export function useMessageActions(onNotice: (n: Undo) => void) {
  const qc = useQueryClient()

  const apply = useCallback(
    async (items: ListItem[], patch: MessagePatch, opts: { undoable?: boolean } = {}) => {
      if (items.length === 0) return
      const ids = items.map((i) => i.id)
      const idSet = new Set(ids)
      await qc.cancelQueries({ queryKey: queryKeys.messages() })
      const snapshot = qc.getQueriesData<InfiniteData<Page>>({ queryKey: queryKeys.messages() })

      // Otimista em todas as listas em cache.
      for (const [key, data] of snapshot) {
        if (!data?.pages) continue
        const filter = (key as unknown as ListKey)[2] ?? {}
        qc.setQueryData<InfiniteData<Page>>(key, {
          ...data,
          pages: data.pages.map((page) => ({
            ...page,
            items: page.items
              .map((m) =>
                idSet.has(m.id)
                  ? {
                      ...m,
                      is_read: patch.read ?? m.is_read,
                      is_starred: patch.starred ?? m.is_starred,
                      folder: patch.folder ?? m.folder,
                    }
                  : m,
              )
              // Lista de uma pasta: a mensagem que saiu dela some.
              .filter((m) => !(idSet.has(m.id) && filter.folder && m.folder !== filter.folder)),
          })),
        })
      }

      const threadSnapshot = qc.getQueriesData<Thread>({ queryKey: queryKeys.threads() })
      for (const [key, t] of threadSnapshot) {
        if (!t?.messages) continue
        qc.setQueryData<Thread>(key, {
          ...t,
          messages: t.messages.map((m) =>
            idSet.has(m.id)
              ? { ...m, is_read: patch.read ?? m.is_read, is_starred: patch.starred ?? m.is_starred, folder: patch.folder ?? m.folder }
              : m,
          ),
        })
      }

      try {
        for (let i = 0; i < ids.length; i += BULK_MAX) {
          await apiClient.bulkPatchMessages(ids.slice(i, i + BULK_MAX), patch)
        }
      } catch (e) {
        for (const [key, data] of snapshot) qc.setQueryData(key, data)
        for (const [key, data] of threadSnapshot) qc.setQueryData(key, data)
        onNotice({ text: 'Não foi possível concluir a ação. Nada foi alterado.' })
        throw e
      } finally {
        void qc.invalidateQueries({ queryKey: queryKeys.messages() })
        void qc.invalidateQueries({ queryKey: queryKeys.domains() })
        void qc.invalidateQueries({ queryKey: queryKeys.threads() })
      }

      if (patch.folder && opts.undoable) {
        // Pasta de origem de cada uma (numa busca, podem ser várias).
        const origin = new Map<Folder, ListItem[]>()
        for (const m of items) origin.set(m.folder, [...(origin.get(m.folder) ?? []), m])
        const n = items.length
        onNotice({
          text: `${n === 1 ? 'Mensagem movida' : `${n} mensagens movidas`} para ${FOLDER_LABELS[patch.folder]}`,
          undo: () => {
            for (const [folder, group] of origin) {
              void apply(group.map((m) => ({ ...m, folder: patch.folder! })), { folder })
            }
          },
        })
      }
    },
    [qc, onNotice],
  )

  return apply
}
