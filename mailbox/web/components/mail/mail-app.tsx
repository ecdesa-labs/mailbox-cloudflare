'use client'

import { useCallback, useEffect, useRef, useState } from 'react'
import { MessageList } from './message-list'
import { useNav } from './nav'
import { Reader } from './reader'
import { Sidebar } from './sidebar'

/**
 * Shell de 3 painéis. Atalhos (fora de campos de texto): ↑/↓ ou k/j trocam a
 * mensagem aberta, Esc fecha o leitor. Abaixo de 1024 px, lista e leitor
 * alternam; abaixo de 768 px, a barra lateral vira gaveta.
 */
export function MailApp() {
  const { state: nav, navigate } = useNav()
  const [collapsed, setCollapsed] = useState(false)
  const [drawer, setDrawer] = useState(false)
  const ids = useRef<string[]>([])
  const setIds = useCallback((next: string[]) => {
    ids.current = next
  }, [])

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const t = e.target as HTMLElement | null
      if (t && (t.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName))) return
      if (e.metaKey || e.ctrlKey || e.altKey) return
      const list = ids.current
      const i = nav.id ? list.indexOf(nav.id) : -1
      if (e.key === 'ArrowDown' || e.key === 'j') {
        const next = list[Math.min(i + 1, list.length - 1)]
        if (next && next !== nav.id) {
          e.preventDefault()
          navigate({ id: next }, { replace: true })
        }
      } else if (e.key === 'ArrowUp' || e.key === 'k') {
        const prev = list[Math.max(i - 1, 0)]
        if (prev && prev !== nav.id) {
          e.preventDefault()
          navigate({ id: prev }, { replace: true })
        }
      } else if (e.key === 'Escape' && nav.id) {
        navigate({ id: '' })
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [nav.id, navigate])

  // A linha aberta fica visível quando o teclado troca de mensagem.
  useEffect(() => {
    if (nav.id) document.querySelector(`[data-message-id="${nav.id}"]`)?.scrollIntoView({ block: 'nearest' })
  }, [nav.id])

  return (
    <div className="flex h-dvh min-h-[560px] overflow-hidden">
      <Sidebar
        nav={nav}
        onNavigate={navigate}
        collapsed={collapsed}
        onCollapse={() => setCollapsed((c) => !c)}
        open={drawer}
        onClose={() => setDrawer(false)}
      />
      <main className="flex min-w-0 flex-1">
        <MessageList
          nav={nav}
          onOpen={(id) => navigate({ id }, { replace: Boolean(nav.id) })}
          onMenu={() => setDrawer(true)}
          onItems={setIds}
        />
        <Reader id={nav.id} onBack={() => navigate({ id: '' })} />
      </main>
    </div>
  )
}
