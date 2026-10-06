'use client'

import { useEffect, useRef, useState } from 'react'
import {
  AlertTriangle, Archive, ChevronDown, ChevronLeft, Download, File, FileImage, ImageOff, Inbox, Mail,
  MailOpen, Reply, ReplyAll, ShieldAlert, Star, Trash2, type LucideIcon,
} from 'lucide-react'
import { useMessage, useThread } from '@/lib/api/queries'
import { ApiRequestError, type Attachment, type ListItem, type MessageDetail, type MessagePatch, type Person, type Thread } from '@/lib/api/types'
import { cn } from '@/lib/utils'
import type { ComposeMode } from './compose'
import { EmailFrame } from './email-frame'
import { FOLDER_LABELS, formatBytes, formatFullDate, formatListDate } from './format'
import { ErrorState, ReaderSkeleton } from './states'

type Act = (items: ListItem[], patch: MessagePatch, opts?: { undoable?: boolean }) => void
type OnReply = (mode: Exclude<ComposeMode, 'new'>, original: MessageDetail) => void

/** "Responder a todos" só faz sentido com mais de um destinatário. */
function manyRecipients(d: MessageDetail): boolean {
  return d.to.length + d.cc.length > 1
}

function who(p: Person | null | undefined): string {
  if (!p) return '(sem remetente)'
  return p.name ? `${p.name} <${p.addr}>` : p.addr
}

export function Reader({ id, onBack, onAct, onReply }: { id: string; onBack: () => void; onAct: Act; onReply: OnReply }) {
  const message = useMessage(id || null)
  const data = message.data
  // Conversa: só quando a thread tem mais de uma mensagem.
  const thread = useThread(data?.thread_id ?? null)
  const conversation = thread.data && thread.data.messages.length > 1 ? thread.data : null

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
        {data && <Toolbar detail={data} onAct={onAct} onReply={onReply} />}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {message.isLoading ? (
          <ReaderSkeleton />
        ) : message.isError ? (
          <ErrorState
            message={message.error instanceof ApiRequestError ? message.error.message : undefined}
            onRetry={() => void message.refetch()}
          />
        ) : conversation ? (
          // Mensagem nova na conversa (ex.: a resposta enviada) remonta com ela aberta.
          <Conversation key={`${conversation.id}:${id}:${conversation.messages.length}`} thread={conversation} openId={id} onAct={onAct} />
        ) : data ? (
          <MessageBody id={id} onAct={onAct} />
        ) : null}
        {data && !message.isError && (
          <div className="flex gap-2 px-5 pb-6 sm:px-6">
            <button
              onClick={() => onReply('reply', data)}
              className="inline-flex h-9 items-center gap-2 rounded-lg border border-border px-3 text-sm hover:bg-accent"
            >
              <Reply className="size-4" aria-hidden="true" /> Responder
            </button>
            {manyRecipients(data) && (
              <button
                onClick={() => onReply('reply-all', data)}
                className="inline-flex h-9 items-center gap-2 rounded-lg border border-border px-3 text-sm hover:bg-accent"
              >
                <ReplyAll className="size-4" aria-hidden="true" /> Responder a todos
              </button>
            )}
          </div>
        )}
      </div>
    </section>
  )
}

/** Conversa, mais antiga primeiro. Abertas: a selecionada e a mais recente. */
function Conversation({ thread, openId, onAct }: { thread: Thread; openId: string; onAct: Act }) {
  const last = thread.messages[thread.messages.length - 1]?.id
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set([openId, last].filter(Boolean) as string[]))
  const toggle = (mid: string) =>
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(mid)) next.delete(mid)
      else next.add(mid)
      return next
    })
  const allOpen = expanded.size === thread.messages.length

  // A mensagem selecionada fica visível ao abrir a conversa.
  const openRef = useRef<HTMLLIElement>(null)
  useEffect(() => {
    openRef.current?.scrollIntoView({ block: 'start' })
  }, [])

  return (
    <div className="p-5 sm:p-6">
      <div className="mb-4 flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 className="text-xl font-semibold tracking-tight first-letter:uppercase">{thread.subject || '(sem assunto)'}</h2>
          <p className="mt-0.5 text-xs text-muted-foreground">{thread.messages.length} mensagens nesta conversa</p>
        </div>
        <button
          onClick={() => setExpanded(allOpen ? new Set([openId]) : new Set(thread.messages.map((m) => m.id)))}
          className="shrink-0 rounded-md px-2 py-1 text-xs text-muted-foreground hover:bg-accent"
        >
          {allOpen ? 'Recolher' : 'Expandir todas'}
        </button>
      </div>
      <ol className="flex flex-col gap-2">
        {thread.messages.map((m) => {
          const isOpen = expanded.has(m.id)
          return (
            <li
              key={m.id}
              ref={m.id === openId ? openRef : undefined}
              className={cn('rounded-xl border', m.id === openId ? 'border-primary/40' : 'border-border')}
            >
              <button
                onClick={() => toggle(m.id)}
                aria-expanded={isOpen}
                className="flex w-full items-center gap-3 px-4 py-3 text-left"
              >
                <span className="flex w-2 shrink-0 justify-center" aria-hidden="true">
                  {!m.is_read && <span className="size-2 rounded-full bg-primary" />}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="flex items-baseline justify-between gap-3">
                    <span className={cn('truncate text-sm', !m.is_read && 'font-semibold')}>
                      {m.from?.name || m.from?.addr || '(sem remetente)'}
                    </span>
                    <time className="shrink-0 text-[11px] text-muted-foreground">{formatListDate(m.received_at)}</time>
                  </span>
                  {!isOpen && <span className="block truncate text-xs text-muted-foreground">{m.snippet || m.subject}</span>}
                </span>
                <ChevronDown className={cn('size-4 shrink-0 text-muted-foreground transition-transform', isOpen && 'rotate-180')} aria-hidden="true" />
              </button>
              {isOpen && (
                <div className="border-t border-border">
                  <MessageBody id={m.id} onAct={onAct} compact />
                </div>
              )}
            </li>
          )
        })}
      </ol>
    </div>
  )
}

/**
 * Uma mensagem: carrega o detalhe, marca como lida ao aparecer e controla as
 * imagens remotas dela. `compact` = dentro de uma conversa (sem título).
 */
function MessageBody({ id, onAct, compact = false }: { id: string; onAct: Act; compact?: boolean }) {
  const [remote, setRemote] = useState(false)
  const message = useMessage(id, remote)
  const data = message.data

  const marked = useRef(false)
  useEffect(() => {
    if (data && !data.is_read && !marked.current) {
      marked.current = true
      onAct([data], { read: true })
    }
  }, [data, onAct])

  if (message.isLoading) return <ReaderSkeleton />
  if (message.isError || !data) {
    return (
      <ErrorState
        message={message.error instanceof ApiRequestError ? message.error.message : undefined}
        onRetry={() => void message.refetch()}
      />
    )
  }
  return <Body detail={data} remote={remote} onLoadRemote={() => setRemote(true)} compact={compact} />
}

function Toolbar({ detail, onAct, onReply }: { detail: MessageDetail; onAct: Act; onReply: OnReply }) {
  const moves: { to: MessagePatch['folder']; label: string; icon: LucideIcon }[] = [
    { to: 'inbox', label: 'Mover para a entrada', icon: Inbox },
    { to: 'archive', label: 'Arquivar', icon: Archive },
    { to: 'spam', label: 'Marcar como spam', icon: ShieldAlert },
    { to: 'trash', label: 'Mover para a lixeira', icon: Trash2 },
  ]
  return (
    <div className="ml-auto flex items-center gap-0.5 lg:ml-0">
      <ToolButton icon={Reply} label="Responder" onClick={() => onReply('reply', detail)} />
      {manyRecipients(detail) && <ToolButton icon={ReplyAll} label="Responder a todos" onClick={() => onReply('reply-all', detail)} />}
      <span className="mx-1 h-5 w-px bg-border" aria-hidden="true" />
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

function Body({ detail, remote, onLoadRemote, compact }: {
  detail: MessageDetail
  remote: boolean
  onLoadRemote: () => void
  compact: boolean
}) {
  const [view, setView] = useState<'html' | 'text'>('html')
  const hasHtml = Boolean(detail.html)
  const showHtml = hasHtml && view === 'html'
  const warning = authWarning(detail)
  const visibleAttachments = detail.attachments.filter((a) => !a.is_inline || !detail.html?.includes(a.url))

  return (
    <article className={cn('flex flex-col gap-4', compact ? 'p-4' : 'min-h-full p-5 sm:p-6')}>
      <header>
        {!compact && (
          <>
            <p className="text-xs font-medium text-muted-foreground">
              {detail.domain} · {FOLDER_LABELS[detail.folder]}
            </p>
            <h2 className="mt-1 text-xl font-semibold tracking-tight">{detail.subject || '(sem assunto)'}</h2>
          </>
        )}
        <dl className={cn('grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-sm', !compact && 'mt-3')}>
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
          {/* Num enviado, a identidade é o próprio remetente. */}
          {detail.folder !== 'sent' && detail.identity && detail.identity !== detail.to[0]?.addr && (
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

          {/* Na conversa, altura fixa só para o iframe; texto ocupa o que precisa. */}
          <div className={cn(compact ? showHtml && 'h-[60vh] min-h-64' : 'min-h-64 flex-1')}>
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
