'use client'

import { useEffect, useRef, useState } from 'react'
import {
  AlertTriangle, Archive, ChevronLeft, Download, File, FileImage, ImageOff, Inbox, Mail,
  MailOpen, ShieldAlert, Star, Trash2, type LucideIcon,
} from 'lucide-react'
import { useMessage } from '@/lib/api/queries'
import { ApiRequestError, type Attachment, type ListItem, type MessageDetail, type MessagePatch, type Person } from '@/lib/api/types'
import { cn } from '@/lib/utils'
import { EmailFrame } from './email-frame'
import { FOLDER_LABELS, formatBytes, formatFullDate } from './format'
import { ErrorState, ReaderSkeleton } from './states'

type Act = (items: ListItem[], patch: MessagePatch, opts?: { undoable?: boolean }) => void

function who(p: Person | null | undefined): string {
  if (!p) return '(sem remetente)'
  return p.name ? `${p.name} <${p.addr}>` : p.addr
}

export function Reader({ id, onBack, onAct }: { id: string; onBack: () => void; onAct: Act }) {
  // Imagens remotas: liberadas só para a mensagem em que o usuário pediu.
  const [remoteFor, setRemoteFor] = useState<string | null>(null)
  const remote = remoteFor === id
  const message = useMessage(id || null, remote)

  // Abrir marca como lida (uma vez por mensagem aberta).
  const marked = useRef<string | null>(null)
  const data = message.data
  useEffect(() => {
    if (data && data.id === id && !data.is_read && marked.current !== id) {
      marked.current = id
      onAct([data], { read: true })
    }
  }, [data, id, onAct])

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
      <div className="flex h-16 shrink-0 items-center gap-1 border-b border-border px-3">
        <button onClick={onBack} className="flex items-center gap-1 rounded-lg p-2 text-sm text-muted-foreground hover:bg-accent lg:hidden">
          <ChevronLeft className="size-4" aria-hidden="true" /> Voltar
        </button>
        {data && <Toolbar detail={data} onAct={onAct} />}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {message.isLoading ? (
          <ReaderSkeleton />
        ) : message.isError ? (
          <ErrorState
            message={message.error instanceof ApiRequestError ? message.error.message : undefined}
            onRetry={() => void message.refetch()}
          />
        ) : data ? (
          <Body detail={data} remote={remote} onLoadRemote={() => setRemoteFor(id)} />
        ) : null}
      </div>
    </section>
  )
}

function Toolbar({ detail, onAct }: { detail: MessageDetail; onAct: Act }) {
  const moves: { to: MessagePatch['folder']; label: string; icon: LucideIcon }[] = [
    { to: 'inbox', label: 'Mover para a entrada', icon: Inbox },
    { to: 'archive', label: 'Arquivar', icon: Archive },
    { to: 'spam', label: 'Marcar como spam', icon: ShieldAlert },
    { to: 'trash', label: 'Mover para a lixeira', icon: Trash2 },
  ]
  return (
    <div className="ml-auto flex items-center gap-0.5 lg:ml-0">
      {moves
        .filter((m) => m.to !== detail.folder && !(detail.folder === 'sent' && m.to === 'inbox'))
        .map((m) => (
          <ToolButton key={m.to} icon={m.icon} label={m.label} onClick={() => onAct([detail], { folder: m.to }, { undoable: true })} />
        ))}
      <ToolButton
        icon={detail.is_read ? Mail : MailOpen}
        label={detail.is_read ? 'Marcar como não lida' : 'Marcar como lida'}
        onClick={() => onAct([detail], { read: !detail.is_read })}
      />
      <ToolButton
        icon={Star}
        label={detail.is_starred ? 'Tirar estrela' : 'Marcar com estrela'}
        pressed={detail.is_starred}
        onClick={() => onAct([detail], { starred: !detail.is_starred })}
      />
      <a
        href={detail.raw_url}
        download
        className="rounded-md p-2 text-muted-foreground hover:bg-accent hover:text-foreground"
        aria-label="Baixar o .eml original"
        title="Baixar o .eml original"
      >
        <Download className="size-4" aria-hidden="true" />
      </a>
    </div>
  )
}

function ToolButton({ icon: Icon, label, onClick, pressed }: { icon: LucideIcon; label: string; onClick: () => void; pressed?: boolean }) {
  return (
    <button
      onClick={onClick}
      aria-label={label}
      aria-pressed={pressed}
      title={label}
      className="rounded-md p-2 text-muted-foreground hover:bg-accent hover:text-foreground"
    >
      <Icon className={cn('size-4', pressed && 'fill-star text-star')} aria-hidden="true" />
    </button>
  )
}

/** SPF/DKIM/DMARC falhando: o remetente pode ter sido forjado. */
function authWarning(d: MessageDetail): string | null {
  const fail = (v: string | null) => v === 'fail' || v === 'softfail' || v === 'permerror'
  if (fail(d.auth.dmarc)) return 'O domínio do remetente não confirmou este email (DMARC falhou). Pode ser falsificado.'
  if (fail(d.auth.spf) && fail(d.auth.dkim ?? null)) return 'Não foi possível confirmar o remetente (SPF e DKIM falharam).'
  if (fail(d.auth.spf)) return 'O servidor que enviou este email não está autorizado pelo domínio do remetente (SPF falhou).'
  return null
}

function Body({ detail, remote, onLoadRemote }: { detail: MessageDetail; remote: boolean; onLoadRemote: () => void }) {
  const [view, setView] = useState<'html' | 'text'>('html')
  const hasHtml = Boolean(detail.html)
  const showHtml = hasHtml && view === 'html'
  const warning = authWarning(detail)
  const visibleAttachments = detail.attachments.filter((a) => !a.is_inline || !detail.html?.includes(a.url))

  return (
    <article className="flex min-h-full flex-col gap-4 p-5 sm:p-6">
      <header>
        <p className="text-xs font-medium text-muted-foreground">
          {detail.domain} · {FOLDER_LABELS[detail.folder]}
        </p>
        <h2 className="mt-1 text-xl font-semibold tracking-tight">{detail.subject || '(sem assunto)'}</h2>
        <dl className="mt-3 grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-sm">
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
          {detail.identity && detail.identity !== detail.to[0]?.addr && (
            <>
              <dt className="text-muted-foreground">Recebido por</dt>
              <dd className="truncate">{detail.identity}</dd>
            </>
          )}
        </dl>
      </header>

      {warning && (
        <Banner tone="danger" icon={AlertTriangle}>
          {warning}
        </Banner>
      )}

      {detail.processing === 'partial' ? (
        <Banner tone="info" icon={File}>
          Este email é grande demais para ser processado{detail.size > 0 ? ` (${formatBytes(detail.size)})` : ''}. O
          conteúdo e os anexos estão no arquivo original.{' '}
          <a href={detail.raw_url} download className="font-medium underline underline-offset-2">
            Baixar o .eml
          </a>
        </Banner>
      ) : (
        <>
          {detail.has_remote_images && showHtml && !remote && (
            <Banner tone="muted" icon={ImageOff}>
              Imagens externas foram bloqueadas para que o remetente não saiba quando você abriu este email.{' '}
              <button onClick={onLoadRemote} className="font-medium underline underline-offset-2">
                Carregar imagens
              </button>
            </Banner>
          )}

          {hasHtml && detail.text && (
            <div role="tablist" aria-label="Formato" className="flex w-fit gap-1 rounded-lg bg-muted p-1 text-xs">
              {(['html', 'text'] as const).map((v) => (
                <button
                  key={v}
                  role="tab"
                  aria-selected={view === v}
                  onClick={() => setView(v)}
                  className={cn('rounded-md px-2.5 py-1', view === v ? 'bg-background font-medium shadow-sm' : 'text-muted-foreground')}
                >
                  {v === 'html' ? 'Formatado' : 'Texto'}
                </button>
              ))}
            </div>
          )}

          <div className="min-h-64 flex-1">
            {showHtml ? (
              <EmailFrame html={detail.html!} remoteImages={remote} title={`Conteúdo: ${detail.subject ?? 'sem assunto'}`} />
            ) : detail.text ? (
              <pre className="whitespace-pre-wrap break-words font-sans text-sm leading-relaxed">{detail.text}</pre>
            ) : (
              <p className="text-sm text-muted-foreground">(mensagem sem texto)</p>
            )}
          </div>

          {visibleAttachments.length > 0 && <Attachments items={visibleAttachments} />}
        </>
      )}
    </article>
  )
}

function Banner({ tone, icon: Icon, children }: { tone: 'danger' | 'info' | 'muted'; icon: LucideIcon; children: React.ReactNode }) {
  return (
    <div
      role={tone === 'danger' ? 'alert' : 'note'}
      className={cn(
        'flex items-start gap-2.5 rounded-lg border px-3 py-2.5 text-sm',
        tone === 'danger' && 'border-destructive/40 bg-destructive/10',
        tone === 'info' && 'border-primary/30 bg-primary/5',
        tone === 'muted' && 'border-border bg-muted/50',
      )}
    >
      <Icon className={cn('mt-0.5 size-4 shrink-0', tone === 'danger' ? 'text-destructive' : 'text-muted-foreground')} aria-hidden="true" />
      <p>{children}</p>
    </div>
  )
}

function Attachments({ items }: { items: Attachment[] }) {
  return (
    <section aria-label="Anexos" className="border-t border-border pt-4">
      <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
        {items.length} {items.length === 1 ? 'anexo' : 'anexos'}
      </h3>
      <ul className="grid gap-2 sm:grid-cols-2">
        {items.map((a) => {
          const isImage = a.content_type.startsWith('image/')
          const Icon = isImage ? FileImage : File
          return (
            <li key={a.id}>
              <a
                href={a.url}
                // Imagens simples abrem no navegador; o resto a API já força download.
                target={isImage ? '_blank' : undefined}
                rel="noopener noreferrer"
                download={isImage ? undefined : (a.filename ?? true)}
                className="flex items-center gap-3 rounded-lg border border-border p-2.5 hover:bg-accent"
              >
                <Icon className="size-5 shrink-0 text-muted-foreground" aria-hidden="true" />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm">{a.filename || '(sem nome)'}</span>
                  <span className="block text-xs text-muted-foreground">
                    {formatBytes(a.size)} · {a.content_type}
                  </span>
                </span>
                <Download className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
              </a>
            </li>
          )
        })}
      </ul>
    </section>
  )
}
