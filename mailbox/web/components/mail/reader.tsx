'use client'

import { ChevronLeft, FileText, Mail } from 'lucide-react'
import { useMessage } from '@/lib/api/queries'
import { ApiRequestError, type MessageDetail, type Person } from '@/lib/api/types'
import { formatFullDate } from './format'
import { ErrorState, ReaderSkeleton } from './states'

function who(p: Person | null | undefined): string {
  if (!p) return '(sem remetente)'
  return p.name ? `${p.name} <${p.addr}>` : p.addr
}

/** Nesta fase: só cabeçalho. Corpo, HTML e anexos entram na fase 3 do front. */
export function Reader({ id, onBack }: { id: string; onBack: () => void }) {
  const message = useMessage(id || null)

  if (!id) {
    return (
      <section aria-label="Leitor" className="hidden min-w-0 flex-1 items-center justify-center bg-muted/20 lg:flex">
        <div className="text-center">
          <div className="mx-auto mb-4 flex size-14 items-center justify-center rounded-2xl bg-muted">
            <Mail className="size-6 text-muted-foreground" aria-hidden="true" />
          </div>
          <p className="text-sm font-medium">Selecione uma mensagem</p>
          <p className="mt-1 text-xs text-muted-foreground">Use ↑ e ↓ para navegar pela lista.</p>
        </div>
      </section>
    )
  }

  return (
    <section aria-label="Leitor" className="flex min-w-0 flex-1 flex-col">
      <div className="flex h-16 shrink-0 items-center gap-2 border-b border-border px-4 lg:hidden">
        <button onClick={onBack} className="flex items-center gap-1 rounded-lg p-2 text-sm text-muted-foreground hover:bg-accent">
          <ChevronLeft className="size-4" aria-hidden="true" /> Voltar
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {message.isLoading ? (
          <ReaderSkeleton />
        ) : message.isError ? (
          <ErrorState
            message={message.error instanceof ApiRequestError ? message.error.message : undefined}
            onRetry={() => void message.refetch()}
          />
        ) : message.data ? (
          <ReaderHeader detail={message.data} />
        ) : null}
      </div>
    </section>
  )
}

function ReaderHeader({ detail }: { detail: MessageDetail }) {
  return (
    <article className="p-6">
      <p className="text-xs font-medium text-muted-foreground">{detail.domain}</p>
      <h2 className="mt-1 text-xl font-semibold tracking-tight">{detail.subject || '(sem assunto)'}</h2>
      <dl className="mt-4 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-sm">
        <dt className="text-muted-foreground">De</dt>
        <dd className="truncate">{who(detail.from)}</dd>
        <dt className="text-muted-foreground">Para</dt>
        <dd className="truncate">{detail.to.length ? detail.to.map(who).join(', ') : detail.envelope_to}</dd>
        {detail.cc.length > 0 && (
          <>
            <dt className="text-muted-foreground">Cc</dt>
            <dd className="truncate">{detail.cc.map(who).join(', ')}</dd>
          </>
        )}
        <dt className="text-muted-foreground">Data</dt>
        <dd>{formatFullDate(detail.date ?? detail.received_at)}</dd>
      </dl>
      <div className="mt-6 rounded-xl border border-dashed border-border bg-muted/20 p-6 text-sm text-muted-foreground">
        <FileText className="mb-2 size-5" aria-hidden="true" />
        {detail.processing === 'partial'
          ? 'Este email é grande demais para ser processado; o conteúdo fica disponível no .eml original.'
          : 'O corpo da mensagem será exibido aqui na próxima fase.'}
      </div>
    </article>
  )
}
