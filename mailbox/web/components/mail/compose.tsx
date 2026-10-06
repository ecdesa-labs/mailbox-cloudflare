'use client'

import { useEffect, useMemo, useRef, useState } from 'react'
import { AlertTriangle, Loader2, Send, Trash2, X } from 'lucide-react'
import { useDomains, useSendMessage } from '@/lib/api/queries'
import { ApiRequestError, type Domain, type Identity, type MessageDetail, type Person, type SendResult } from '@/lib/api/types'
import { cn } from '@/lib/utils'
import { formatFullDate } from './format'

export type ComposeMode = 'new' | 'reply' | 'reply-all'

export interface ComposeInit {
  mode: ComposeMode
  /** Mensagem respondida (modos de resposta). */
  original?: MessageDetail
  /** Domínio que estava na tela (mensagem nova): sugere o remetente. */
  domain?: string
}

interface Option extends Identity {
  domain: Domain
}

const EMAIL = /^[^\s@<>,;]+@[^\s@<>,;]+\.[^\s@<>,;]+$/

function display(p: Person): string {
  return p.name ? `${p.name.replace(/[<>,;"]/g, '')} <${p.addr}>` : p.addr
}

/** "Nome <a@b.com>, c@d.com" → lista; inválidos à parte. */
export function parseRecipients(input: string): { people: Person[]; invalid: string[] } {
  const people: Person[] = []
  const invalid: string[] = []
  for (const raw of input.split(/[,;\n]/)) {
    const token = raw.trim()
    if (!token) continue
    const m = token.match(/^(.*?)\s*<([^<>]+)>$/)
    const name = m ? m[1].replace(/^"|"$/g, '').trim() || null : null
    const addr = (m ? m[2] : token).trim()
    if (!EMAIL.test(addr)) invalid.push(token)
    else if (!people.some((p) => p.addr.toLowerCase() === addr.toLowerCase())) people.push({ name, addr })
  }
  return { people, invalid }
}

function replySubject(subject: string | null): string {
  const s = subject?.trim() ?? ''
  return /^(re|res)\s*:/i.test(s) ? s : `Re: ${s}`.trim()
}

/** Texto citado, como os clientes de email fazem. */
function quote(d: MessageDetail): string {
  const who = d.from ? (d.from.name ?? d.from.addr) : 'alguém'
  const when = formatFullDate(d.date ?? d.received_at)
  const body = (d.text ?? '').replace(/\s+$/, '')
  if (!body) return ''
  return `\n\nEm ${when}, ${who} escreveu:\n${body.split('\n').map((l) => `> ${l}`).join('\n')}\n`
}

/** Estado inicial dos campos para cada modo. */
function initial(init: ComposeInit, options: Option[]) {
  const own = new Set(options.map((o) => o.address.toLowerCase()))
  const d = init.original
  if (init.mode !== 'new' && d) {
    // Docs seção 6.4: a resposta sai pela identidade que recebeu a original.
    const same = options.filter((o) => o.domain.name === d.domain)
    const received = d.folder === 'sent' ? d.from?.addr : d.identity
    const from =
      same.find((o) => o.address === received) ?? same.find((o) => o.is_default_from) ?? same[0]
    // Respondendo a algo que eu mesmo enviei: para quem eu tinha mandado.
    const to: Person[] = d.folder === 'sent' ? d.to : d.from ? [d.from] : []
    const toSet = new Set(to.map((p) => p.addr.toLowerCase()))
    const cc =
      init.mode === 'reply-all'
        ? [...d.to, ...d.cc].filter((p) => !own.has(p.addr.toLowerCase()) && !toSet.has(p.addr.toLowerCase()))
        : []
    return {
      fromId: from?.id ?? '',
      to: to.map(display).join(', '),
      cc: cc.map(display).join(', '),
      subject: replySubject(d.subject),
      text: quote(d),
    }
  }
  const inDomain = options.filter((o) => o.domain.name === init.domain)
  const from =
    inDomain.find((o) => o.is_default_from) ??
    inDomain[0] ??
    options.find((o) => o.is_default_from) ??
    options[0]
  return { fromId: from?.id ?? '', to: '', cc: '', subject: '', text: '' }
}

/**
 * Compor ou responder. Painel no canto em telas largas, tela cheia no
 * celular. Ctrl/⌘+Enter envia; Esc fecha (pede confirmação se houver texto).
 */
export function Composer(props: { init: ComposeInit; onClose: () => void; onSent: (result: SendResult) => void }) {
  const domains = useDomains()
  const options = useMemo<Option[]>(
    () => (domains.data?.items ?? []).flatMap((d) => d.identities.map((i) => ({ ...i, domain: d }))),
    [domains.data],
  )
  // O formulário nasce com as identidades: o remetente e o Cc dependem delas.
  if (options.length === 0) {
    return (
      <section role="dialog" aria-label="Nova mensagem" className="fixed inset-0 z-50 flex items-center justify-center gap-2 bg-background text-sm text-muted-foreground lg:inset-auto lg:bottom-0 lg:right-6 lg:h-40 lg:w-[580px] lg:rounded-t-xl lg:border lg:shadow-2xl">
        {domains.isError ? (
          <>
            Não foi possível carregar as identidades.
            <button onClick={props.onClose} className="underline">Fechar</button>
          </>
        ) : (
          <>
            <Loader2 className="size-4 animate-spin" aria-hidden="true" /> Carregando identidades...
          </>
        )}
      </section>
    )
  }
  return <ComposerForm {...props} options={options} />
}

function ComposerForm({
  init,
  options,
  onClose,
  onSent,
}: {
  init: ComposeInit
  options: Option[]
  onClose: () => void
  onSent: (result: SendResult) => void
}) {
  const send = useSendMessage()
  const reply = init.mode !== 'new' && init.original ? init.original : null
  // Resposta: só identidades do domínio da original (o servidor recusa as outras).
  const allowed = reply ? options.filter((o) => o.domain.name === reply.domain) : options

  const [form, setForm] = useState(() => initial(init, options))
  const [showCc, setShowCc] = useState(() => form.cc !== '')
  const [error, setError] = useState<{ text: string; uncertain?: boolean } | null>(null)
  const [start] = useState(form)

  const from = options.find((o) => o.id === form.fromId)
  const dirty = form.text !== start.text || form.to !== start.to || form.subject !== start.subject
  const close = () => {
    if (send.isPending) return
    if (dirty && !window.confirm('Descartar esta mensagem?')) return
    onClose()
  }

  const toRef = useRef<HTMLInputElement>(null)
  const textRef = useRef<HTMLTextAreaElement>(null)
  useEffect(() => {
    if (form.to) {
      textRef.current?.focus()
      textRef.current?.setSelectionRange(0, 0)
    } else toRef.current?.focus()
  }, [])

  async function submit() {
    if (send.isPending) return
    setError(null)
    const to = parseRecipients(form.to)
    const cc = parseRecipients(showCc ? form.cc : '')
    const invalid = [...to.invalid, ...cc.invalid]
    if (!from) return setError({ text: 'Escolha o remetente.' })
    if (invalid.length) return setError({ text: `Endereço inválido: ${invalid.join(', ')}` })
    if (to.people.length === 0) return setError({ text: 'Informe ao menos um destinatário.' })
    if (to.people.length + cc.people.length > 50) return setError({ text: 'No máximo 50 destinatários (Para + Cc).' })
    if (!form.text.trim()) return setError({ text: 'A mensagem está vazia.' })
    try {
      const result = await send.mutateAsync({
        in_reply_to: reply?.id ?? null,
        from_identity_id: from.id,
        to: to.people,
        cc: cc.people,
        subject: form.subject,
        text: form.text,
      })
      onSent(result)
    } catch (e) {
      const err = e instanceof ApiRequestError ? e : null
      if (err?.code === 'rejected') setError({ text: `O serviço de envio recusou a mensagem. ${err.message}` })
      else if (err?.code === 'rate_limited') setError({ text: 'Limite de envios atingido. Tente mais tarde.' })
      else if (err && err.status < 500 && err.status !== 0) setError({ text: err.message })
      else {
        // 502 ou rede: pode ter saído. Reenviar às cegas duplicaria o email.
        setError({
          uncertain: true,
          text: 'Não deu para confirmar se a mensagem saiu. Confira em Enviados antes de tentar de novo.',
        })
      }
    }
  }

  const title = init.mode === 'new' ? 'Nova mensagem' : init.mode === 'reply' ? 'Responder' : 'Responder a todos'
  // Mensagem nova saindo por um domínio diferente do que está na tela.
  const otherDomain = init.mode === 'new' && init.domain && from && from.domain.name !== init.domain

  return (
    <>
      <div className="fixed inset-0 z-40 bg-black/30 lg:hidden" aria-hidden="true" />
      <section
        role="dialog"
        aria-label={title}
        onKeyDown={(e) => {
          if (e.key === 'Escape') {
            e.stopPropagation()
            close()
          } else if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
            e.preventDefault()
            void submit()
          }
        }}
        className="fixed inset-0 z-50 flex flex-col bg-background lg:inset-auto lg:bottom-0 lg:right-6 lg:h-[min(640px,85vh)] lg:w-[580px] lg:rounded-t-xl lg:border lg:border-b-0 lg:border-border lg:shadow-2xl"
        style={from ? ({ '--domain': from.domain.color } as React.CSSProperties) : undefined}
      >
        <header className="flex h-12 shrink-0 items-center gap-2 border-b border-border pl-4 pr-2 lg:rounded-t-xl lg:bg-muted/40">
          <h2 className="text-sm font-semibold">{title}</h2>
          <button onClick={close} aria-label="Fechar" title="Fechar (Esc)" className="ml-auto rounded-md p-2 text-muted-foreground hover:bg-accent hover:text-foreground">
            <X className="size-4" aria-hidden="true" />
          </button>
        </header>

        <div className="flex shrink-0 flex-col border-b border-border text-sm">
          <Row label="De" htmlFor="compose-from" accent>
            <select
              id="compose-from"
              value={form.fromId}
              onChange={(e) => setForm({ ...form, fromId: e.target.value })}
              disabled={send.isPending}
              className="min-w-0 flex-1 truncate bg-transparent py-2.5 outline-none"
            >
              {allowed.map((o) => (
                <option key={o.id} value={o.id}>
                  {o.label ? `${o.label} <${o.address}>` : o.address}
                </option>
              ))}
            </select>
            {from && (
              <span
                className="shrink-0 rounded-full px-2 py-0.5 text-[11px] font-semibold text-white"
                style={{ backgroundColor: from.domain.color }}
                title={`Sai por ${from.domain.name}`}
              >
                {from.domain.label}
              </span>
            )}
          </Row>
          <Row label="Para" htmlFor="compose-to">
            <input
              ref={toRef}
              id="compose-to"
              value={form.to}
              onChange={(e) => setForm({ ...form, to: e.target.value })}
              disabled={send.isPending}
              placeholder="nome@exemplo.com, Nome <outro@exemplo.com>"
              autoComplete="off"
              className="min-w-0 flex-1 bg-transparent py-2.5 outline-none placeholder:text-muted-foreground/60"
            />
            {!showCc && (
              <button onClick={() => setShowCc(true)} className="shrink-0 rounded px-1.5 py-0.5 text-xs text-muted-foreground hover:bg-accent">
                Cc
              </button>
            )}
          </Row>
          {showCc && (
            <Row label="Cc" htmlFor="compose-cc">
              <input
                id="compose-cc"
                value={form.cc}
                onChange={(e) => setForm({ ...form, cc: e.target.value })}
                disabled={send.isPending}
                autoComplete="off"
                className="min-w-0 flex-1 bg-transparent py-2.5 outline-none"
              />
            </Row>
          )}
          <Row label="Assunto" htmlFor="compose-subject">
            <input
              id="compose-subject"
              value={form.subject}
              onChange={(e) => setForm({ ...form, subject: e.target.value })}
              disabled={send.isPending}
              maxLength={998}
              className="min-w-0 flex-1 bg-transparent py-2.5 outline-none"
            />
          </Row>
        </div>

        {otherDomain && (
          <p role="note" className="flex shrink-0 items-center gap-2 border-b border-border bg-amber-500/10 px-4 py-2 text-xs text-amber-700 dark:text-amber-400">
            <AlertTriangle className="size-3.5 shrink-0" aria-hidden="true" />
            Você está vendo {init.domain}, mas esta mensagem sai por <strong>{from.domain.name}</strong>.
          </p>
        )}

        <textarea
          ref={textRef}
          aria-label="Mensagem"
          value={form.text}
          onChange={(e) => setForm({ ...form, text: e.target.value })}
          disabled={send.isPending}
          className="min-h-0 flex-1 resize-none bg-transparent px-4 py-3 text-sm leading-relaxed outline-none"
        />

        {error && (
          <p
            role="alert"
            className={cn(
              'mx-4 mb-2 flex items-start gap-2 rounded-lg px-3 py-2 text-xs',
              error.uncertain ? 'bg-amber-500/10 text-amber-700 dark:text-amber-400' : 'bg-destructive/10 text-destructive',
            )}
          >
            <AlertTriangle className="mt-px size-3.5 shrink-0" aria-hidden="true" />
            {error.text}
          </p>
        )}

        <footer className="flex shrink-0 items-center gap-2 border-t border-border px-4 py-3">
          <button
            onClick={() => void submit()}
            disabled={send.isPending}
            className="inline-flex h-9 items-center gap-2 rounded-lg bg-primary px-4 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-60"
          >
            {send.isPending ? <Loader2 className="size-4 animate-spin" aria-hidden="true" /> : <Send className="size-4" aria-hidden="true" />}
            {send.isPending ? 'Enviando...' : 'Enviar'}
          </button>
          <span className="hidden text-xs text-muted-foreground sm:inline">Ctrl+Enter</span>
          <button
            onClick={close}
            disabled={send.isPending}
            aria-label="Descartar"
            title="Descartar"
            className="ml-auto rounded-md p-2 text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-60"
          >
            <Trash2 className="size-4" aria-hidden="true" />
          </button>
        </footer>
      </section>
    </>
  )
}

function Row({ label, htmlFor, accent, children }: { label: string; htmlFor: string; accent?: boolean; children: React.ReactNode }) {
  return (
    <div
      className={cn(
        'flex items-center gap-3 border-b border-border/60 px-4 last:border-b-0',
        // Barra na cor do domínio do remetente: deixa claro por onde sai.
        accent && 'border-l-4 border-l-[var(--domain)] pl-3',
      )}
    >
      <label htmlFor={htmlFor} className="w-14 shrink-0 text-xs text-muted-foreground">
        {label}
      </label>
      {children}
    </div>
  )
}
