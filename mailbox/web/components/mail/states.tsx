import { Inbox, ShieldAlert } from 'lucide-react'
import type { Folder } from '@/lib/api/types'
import { EMPTY_LABELS } from './format'

export function ListSkeleton({ count = 6 }: { count?: number }) {
  return (
    <div aria-hidden="true">
      {Array.from({ length: count }).map((_, i) => (
        <div key={i} className="flex h-16 items-center gap-3 border-b border-border/70 px-5">
          <div className="size-2 rounded-full bg-muted" />
          <div className="flex flex-1 flex-col gap-2">
            <div className="h-3.5 w-2/5 animate-pulse rounded bg-muted" />
            <div className="h-3 w-4/5 animate-pulse rounded bg-muted" />
          </div>
        </div>
      ))}
    </div>
  )
}

export function ReaderSkeleton() {
  return (
    <div aria-hidden="true" className="flex flex-col gap-3 p-6">
      <div className="h-5 w-1/2 animate-pulse rounded bg-muted" />
      <div className="h-4 w-1/3 animate-pulse rounded bg-muted" />
      <div className="mt-4 h-40 animate-pulse rounded-xl bg-muted" />
    </div>
  )
}

export function EmptyState({ folder }: { folder: Folder }) {
  return (
    <div className="flex h-full min-h-72 flex-col items-center justify-center px-8 text-center">
      <div className="mb-4 flex size-12 items-center justify-center rounded-xl bg-muted">
        <Inbox className="size-5 text-muted-foreground" aria-hidden="true" />
      </div>
      <p className="text-sm font-medium">{EMPTY_LABELS[folder]}</p>
      <p className="mt-1 text-xs text-muted-foreground">Tudo certo por aqui.</p>
    </div>
  )
}

/** Erro genérico. O 401 não chega aqui: vira o aviso de sessão expirada. */
export function ErrorState({ message, onRetry }: { message?: string; onRetry: () => void }) {
  return (
    <div role="alert" className="flex h-full min-h-72 flex-col items-center justify-center px-8 text-center">
      <div className="mb-4 flex size-12 items-center justify-center rounded-xl bg-destructive/10">
        <ShieldAlert className="size-5 text-destructive" aria-hidden="true" />
      </div>
      <p className="text-sm font-medium">Não foi possível carregar</p>
      <p className="mt-1 max-w-xs text-xs text-muted-foreground">
        {message ?? 'Verifique sua conexão e tente de novo.'}
      </p>
      <button
        onClick={onRetry}
        className="mt-4 rounded-lg bg-primary px-3 py-2 text-xs font-medium text-primary-foreground hover:opacity-90"
      >
        Tentar de novo
      </button>
    </div>
  )
}
