'use client'

import { useEffect, useRef } from 'react'
import { X } from 'lucide-react'
import { SHORTCUT_GROUPS } from './shortcuts'

/** Lista dos atalhos (`?`). Esc, `?` ou clique fora fecham. */
export function ShortcutsHelp({ onClose }: { onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => ref.current?.focus(), [])

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4" onClick={onClose}>
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-labelledby="shortcuts-title"
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === 'Escape' || e.key === '?') {
            e.preventDefault()
            e.stopPropagation()
            onClose()
          }
        }}
        className="max-h-[85vh] w-full max-w-2xl overflow-y-auto rounded-xl border border-border bg-background shadow-2xl outline-none"
      >
        <header className="sticky top-0 flex items-center border-b border-border bg-background px-5 py-3">
          <h2 id="shortcuts-title" className="text-sm font-semibold">
            Atalhos de teclado
          </h2>
          <button onClick={onClose} aria-label="Fechar" className="ml-auto rounded-md p-2 text-muted-foreground hover:bg-accent hover:text-foreground">
            <X className="size-4" aria-hidden="true" />
          </button>
        </header>
        <div className="grid gap-x-8 gap-y-5 p-5 sm:grid-cols-2">
          {SHORTCUT_GROUPS.map((g) => (
            <section key={g.title}>
              <h3 className="mb-2 text-[11px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">{g.title}</h3>
              <dl className="flex flex-col gap-1.5 text-sm">
                {g.items.map((s) => (
                  <div key={s.label.join('+') + s.description} className="flex items-center justify-between gap-4">
                    <dt>{s.description}</dt>
                    <dd className="flex shrink-0 gap-1">
                      {s.label.map((k, i) => (
                        <kbd key={i} className="min-w-6 rounded border border-border bg-muted px-1.5 py-0.5 text-center font-mono text-xs">
                          {k}
                        </kbd>
                      ))}
                    </dd>
                  </div>
                ))}
              </dl>
            </section>
          ))}
        </div>
        <p className="border-t border-border px-5 py-3 text-xs text-muted-foreground">
          Os atalhos não valem enquanto você digita num campo. No compositor, Ctrl+Enter envia e Esc fecha.
        </p>
      </div>
    </div>
  )
}
