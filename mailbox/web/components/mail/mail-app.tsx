'use client'

import { useCallback, useEffect, useRef, useState } from 'react'
import type { ListItem, MessagePatch } from '@/lib/api/types'
import { MessageList } from './message-list'
import { useNav } from './nav'
import { Reader } from './reader'
import { Sidebar } from './sidebar'
import { Toast } from './toast'
import { useMessageActions, type Undo } from './use-actions'

/**
 * Shell de 3 painéis. Atalhos (fora de campos de texto): ↑/↓ ou k/j trocam a
 * mensagem aberta, x seleciona a aberta, / vai para a busca, Esc fecha o
 * leitor. Abaixo de 1024 px, lista e leitor alternam; abaixo de 768 px, a
 * barra lateral vira gaveta.
 */
export function MailApp() {
  const { state: nav, navigate } = useNav()
  const [collapsed, setCollapsed] = useState(false)
  const [drawer, setDrawer] = useState(false)
  const [notice, setNotice] = useState<Undo | null>(null)
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const items = useRef<ListItem[]>([])
  const setItems = useCallback((next: ListItem[]) => {
    items.current = next
  }, [])
  const act = useMessageActions(setNotice)

  const onAct = useCallback(
    (list: ListItem[], patch: MessagePatch, opts?: { undoable?: boolean }) => {
      // Se a mensagem aberta sai da lista, fecha o leitor.
      if (patch.folder && nav.id && list.some((m) => m.id === nav.id)) navigate({ id: '' }, { replace: true })
      void act(list, patch, opts).catch(() => {})
    },
    [act, nav.id, navigate],
  )

  // Seleção vale para a lista atual; trocar de lista limpa.
  const listKey = `${nav.folder}|${nav.folderExplicit}|${nav.domain}|${nav.q}`
  useEffect(() => setSelected(new Set()), [listKey])

  const select = useCallback((ids: string[], on: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev)
      for (const id of ids) on ? next.add(id) : next.delete(id)
      return next
    })
  }, [])

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const t = e.target as HTMLElement | null
      if (t && (t.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName))) return
      if (e.metaKey || e.ctrlKey || e.altKey) return
      const ids = items.current.map((m) => m.id)
      const i = nav.id ? ids.indexOf(nav.id) : -1
      if (e.key === 'ArrowDown' || e.key === 'j') {
        const next = ids[Math.min(i + 1, ids.length - 1)]
        if (next && next !== nav.id) {
          e.preventDefault()
          navigate({ id: next }, { replace: true })
        }
      } else if (e.key === 'ArrowUp' || e.key === 'k') {
        const prev = ids[Math.max(i - 1, 0)]
        if (prev && prev !== nav.id) {
          e.preventDefault()
          navigate({ id: prev }, { replace: true })
        }
      } else if (e.key === 'x' && nav.id) {
        select([nav.id], !selected.has(nav.id))
      } else if (e.key === '/') {
        e.preventDefault()
        document.getElementById('mail-search')?.focus()
      } else if (e.key === 'Escape' && nav.id) {
        navigate({ id: '' })
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [nav.id, navigate, select, selected])

  // A linha aberta fica visível quando o teclado troca de mensagem.
  useEffect(() => {
    if (nav.id) document.querySelector(`[data-message-id="${nav.id}"]`)?.scrollIntoView({ block: 'nearest' })
  }, [nav.id])

  return (
    <div className="flex h-dvh min-h-[560px] overflow-hidden">
      <Sidebar
        nav={nav}
        onNavigate={(next) => navigate({ ...next, q: '' })}
        collapsed={collapsed}
        onCollapse={() => setCollapsed((c) => !c)}
        open={drawer}
        onClose={() => setDrawer(false)}
      />
      <main className="flex min-w-0 flex-1">
        <MessageList
          nav={nav}
          onNavigate={navigate}
          onOpen={(id) => navigate({ id }, { replace: Boolean(nav.id) })}
          onMenu={() => setDrawer(true)}
          onItems={setItems}
          selected={selected}
          onSelect={select}
          onClearSelection={() => setSelected(new Set())}
          onAct={onAct}
        />
        <Reader id={nav.id} onBack={() => navigate({ id: '' })} onAct={onAct} />
      </main>
      {notice && <Toast notice={notice} onClose={() => setNotice(null)} />}
    </div>
  )
}
