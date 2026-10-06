'use client'

import { useCallback, useEffect, useRef, useState } from 'react'
import { useMessage } from '@/lib/api/queries'
import type { Folder, ListItem, MessagePatch, SendResult } from '@/lib/api/types'
import { Composer, type ComposeInit } from './compose'
import { MessageList } from './message-list'
import { useNav } from './nav'
import { Reader } from './reader'
import { GO_TO, isTyping, normalizeKey, shortcutFor } from './shortcuts'
import { ShortcutsHelp } from './shortcuts-help'
import { Sidebar } from './sidebar'
import { Toast } from './toast'
import { useMessageActions, type Undo } from './use-actions'

/** Depois de `g`, quanto tempo a segunda tecla ainda vale. */
const GO_TO_WINDOW_MS = 1500

/**
 * Shell de 3 painéis com os atalhos de teclado (tabela em shortcuts.ts; `?`
 * mostra a ajuda). Abaixo de 1024 px, lista e leitor alternam; abaixo de
 * 768 px, a barra lateral vira gaveta.
 */
export function MailApp() {
  const { state: nav, navigate } = useNav()
  const [collapsed, setCollapsed] = useState(false)
  const [drawer, setDrawer] = useState(false)
  const [notice, setNotice] = useState<Undo | null>(null)
  const [help, setHelp] = useState(false)
  const [compose, setCompose] = useState<(ComposeInit & { key: number }) | null>(null)
  const openCompose = useCallback((init: ComposeInit) => setCompose({ ...init, key: Date.now() }), [])
  const onSent = useCallback((r: SendResult) => {
    setCompose(null)
    setNotice({
      text:
        r.stored === 'indexed'
          ? 'Mensagem enviada'
          : r.stored === 'pending'
            ? 'Mensagem enviada. Ela aparece em Enviados em instantes.'
            : 'Mensagem enviada, mas a cópia não foi salva em Enviados.',
    })
  }, [])
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

  // Detalhe da aberta (mesmo cache do leitor): responder pelo teclado.
  const open = useMessage(nav.id || null)

  // O handler é registrado uma vez e lê o estado mais recente daqui.
  const latest = useRef({ nav, selected, notice, compose, help, detail: open.data })
  latest.current = { nav, selected, notice, compose, help, detail: open.data }
  const goPending = useRef(0)
  // Mensagem aberta segundo o teclado: a URL (router do Next) atualiza
  // depois, e um `j` seguido de `x` rápido não pode agir na anterior.
  const openId = useRef(nav.id)
  useEffect(() => {
    openId.current = nav.id
  }, [nav.id])
  const openByKey = useCallback(
    (id: string) => {
      openId.current = id
      navigate({ id }, { replace: true })
    },
    [navigate],
  )

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const s = latest.current
      // Compositor e ajuda tratam as próprias teclas.
      if (s.compose || s.help || isTyping(e.target)) return
      if (e.metaKey || e.ctrlKey || e.altKey) return
      const key = normalizeKey(e)

      if (goPending.current > Date.now()) {
        goPending.current = 0
        const folder = GO_TO[key]
        if (folder) {
          e.preventDefault()
          navigate({ folder, id: '', q: '' })
        }
        return
      }
      if (key === 'g') {
        goPending.current = Date.now() + GO_TO_WINDOW_MS
        return
      }

      const action = shortcutFor(key)
      if (!action) return
      const current = openId.current
      const ids = items.current.map((m) => m.id)
      const i = current ? ids.indexOf(current) : -1
      // Seleção, se houver; senão, a mensagem aberta.
      const targets = (): ListItem[] => {
        const chosen = items.current.filter((m) => s.selected.has(m.id))
        if (chosen.length) return chosen
        const open = items.current.find((m) => m.id === current) ?? (s.detail?.id === current ? s.detail : undefined)
        return open ? [open] : []
      }
      const patch = (list: ListItem[], p: MessagePatch) => {
        if (!list.length) return
        e.preventDefault()
        void act(list, p).catch(() => {})
      }
      const move = (folder: Folder) => {
        const list = targets().filter((m) => m.folder !== folder)
        if (!list.length) return
        e.preventDefault()
        // A aberta saiu da lista: abre a próxima que ficou (ou a anterior).
        if (current && list.some((m) => m.id === current)) {
          const gone = new Set(list.map((m) => m.id))
          const next =
            ids.slice(i + 1).find((id) => !gone.has(id)) ?? ids.slice(0, Math.max(i, 0)).reverse().find((id) => !gone.has(id))
          openByKey(next ?? '')
        }
        if (s.selected.size) setSelected(new Set())
        void act(list, { folder }, { undoable: true }).catch(() => {})
      }

      switch (action) {
        case 'next':
        case 'prev': {
          const to = ids[action === 'next' ? Math.min(i + 1, ids.length - 1) : Math.max(i - 1, 0)]
          if (to && to !== current) {
            e.preventDefault()
            openByKey(to)
          }
          break
        }
        case 'close':
          if (current) {
            openId.current = ''
            navigate({ id: '' })
          }
          else if (s.selected.size) setSelected(new Set())
          break
        case 'search':
          e.preventDefault()
          document.getElementById('mail-search')?.focus()
          break
        case 'select':
          if (current) select([current], !s.selected.has(current))
          break
        case 'archive':
          move('archive')
          break
        case 'trash':
          move('trash')
          break
        case 'spam':
          move('spam')
          break
        case 'star': {
          const list = targets()
          patch(list, { starred: !list.every((m) => m.is_starred) })
          break
        }
        case 'read':
          patch(targets().filter((m) => !m.is_read), { read: true })
          break
        case 'unread':
          patch(targets().filter((m) => m.is_read), { read: false })
          break
        case 'undo':
          if (s.notice?.undo) {
            s.notice.undo()
            setNotice(null)
          }
          break
        case 'compose':
          e.preventDefault()
          openCompose({ mode: 'new', domain: s.nav.domain || undefined })
          break
        case 'reply':
        case 'reply-all':
          if (s.detail && s.detail.id === current) {
            e.preventDefault()
            openCompose({ mode: action, original: s.detail })
          }
          break
        case 'help':
          e.preventDefault()
          setHelp(true)
          break
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [act, navigate, openByKey, openCompose, select])

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
        onCompose={() => openCompose({ mode: 'new', domain: nav.domain || undefined })}
        onHelp={() => setHelp(true)}
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
        <Reader id={nav.id} onBack={() => navigate({ id: '' })} onAct={onAct} onReply={(mode, original) => openCompose({ mode, original })} />
      </main>
      {compose && <Composer key={compose.key} init={compose} onClose={() => setCompose(null)} onSent={onSent} />}
      {notice && <Toast notice={notice} onClose={() => setNotice(null)} />}
      {help && <ShortcutsHelp onClose={() => setHelp(false)} />}
    </div>
  )
}
