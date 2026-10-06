'use client'

import { useCallback, useMemo } from 'react'
import { usePathname, useRouter, useSearchParams } from 'next/navigation'
import type { Folder } from '@/lib/api/types'

export const FOLDERS: Folder[] = ['inbox', 'archive', 'trash', 'spam', 'sent']

export interface NavState {
  folder: Folder
  /** '' = caixa unificada */
  domain: string
  /** '' = nenhuma mensagem aberta */
  id: string
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
      domain: params.get('domain') ?? '',
      id: params.get('id') ?? '',
    }),
    [params],
  )

  /** `replace` para trocar de mensagem na mesma lista sem encher o histórico. */
  const navigate = useCallback(
    (next: Partial<NavState>, opts: { replace?: boolean } = {}) => {
      const merged = { ...state, ...next }
      const q = new URLSearchParams()
      if (merged.folder !== 'inbox') q.set('folder', merged.folder)
      if (merged.domain) q.set('domain', merged.domain)
      if (merged.id) q.set('id', merged.id)
      const url = q.size ? `${pathname}?${q}` : pathname
      if (opts.replace) router.replace(url, { scroll: false })
      else router.push(url, { scroll: false })
    },
    [state, pathname, router],
  )

  return { state, navigate }
}
