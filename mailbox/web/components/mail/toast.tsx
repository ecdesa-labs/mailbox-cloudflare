'use client'

import { useEffect } from 'react'
import { X } from 'lucide-react'
import type { Undo } from './use-actions'

/** Aviso no rodapé, com "Desfazer" quando a ação permite. Some em 10 s. */
export function Toast({ notice, onClose }: { notice: Undo; onClose: () => void }) {
  useEffect(() => {
    const t = setTimeout(onClose, 10_000)
    return () => clearTimeout(t)
  }, [notice, onClose])

  return (
    <div
      role="status"
      aria-live="polite"
      className="fixed bottom-4 left-1/2 z-50 flex -translate-x-1/2 items-center gap-3 rounded-lg bg-foreground px-4 py-2.5 text-sm text-background shadow-lg"
    >
      <span>{notice.text}</span>
      {notice.undo && (
        <button
          onClick={() => {
            notice.undo?.()
            onClose()
          }}
          className="font-semibold text-primary-foreground underline-offset-2 hover:underline dark:text-primary"
        >
          Desfazer
        </button>
      )}
      <button onClick={onClose} aria-label="Fechar aviso" className="opacity-70 hover:opacity-100">
        <X className="size-4" />
      </button>
    </div>
  )
}
