'use client'

import { useCallback, useMemo } from 'react'
import { usePathname, useRouter, useSearchParams } from 'next/navigation'
import type { Folder } from '@/lib/api/types'

export const FOLDERS: Folder[] = ['inbox', 'archive', 'trash', 'spam', 'sent']

export interface NavState {
  folder: Folder
  /** A pasta veio da URL (numa busca, sem pasta explícita = todas as pastas). */
  folderExplicit: boolean
  /** '' = caixa unificada */
  domain: string
  /** '' = nenhuma mensagem aberta */
  id: string
  /** '' = sem busca */
  q: string
}

function isFolder(v: string | null): v is Folder {
  return v !== null && (FOLDERS as string[]).includes(v)
}

/**
 * Estado de navegação na query string (`?folder=&domain=&id=`): recarregar a
 * página e o botão voltar do navegador mantêm o que estava aberto. Página
 * única em `/` (export estático; sem rotas dinâmicas).
 */
export function useNav() {
  const router = useRouter()
  const pathname = usePathname()
  const params = useSearchParams()

  const state = useMemo<NavState>(
    () => ({
      folder: isFolder(params.get('folder')) ? (params.get('folder') as Folder) : 'inbox',
      folderExplicit: isFolder(params.get('folder')),
      domain: params.get('domain') ?? '',
      id: params.get('id') ?? '',
      q: params.get('q') ?? '',
    }),
    [params],
  )

  /** `replace` para trocar de mensagem na mesma lista sem encher o histórico. */
  const navigate = useCallback(
    (next: Partial<NavState>, opts: { replace?: boolean } = {}) => {
      const merged = { ...state, ...next }
      // Escolher pasta torna a pasta explícita (inclusive a entrada).
      const explicit = next.folder !== undefined ? true : merged.folderExplicit
      const q = new URLSearchParams()
      if (explicit && (merged.folder !== 'inbox' || merged.q)) q.set('folder', merged.folder)
      if (merged.domain) q.set('domain', merged.domain)
      if (merged.q) q.set('q', merged.q)
      if (merged.id) q.set('id', merged.id)
      const url = q.size ? `${pathname}?${q}` : pathname
      if (opts.replace) router.replace(url, { scroll: false })
      else router.push(url, { scroll: false })
    },
    [state, pathname, router],
  )

  return { state, navigate }
}
