'use client'

import { useEffect, useMemo, useState } from 'react'
import { Menu, Paperclip, Search, Star } from 'lucide-react'
import { useDomains, useMessages } from '@/lib/api/queries'
import { ApiRequestError, type ListItem } from '@/lib/api/types'
import { cn } from '@/lib/utils'
import { FOLDER_LABELS, formatListDate } from './format'
import type { NavState } from './nav'
import { EmptyState, ErrorState, ListSkeleton } from './states'

interface Props {
  nav: NavState
  onOpen: (id: string) => void
  onMenu: () => void
  /** Itens carregados, para os atalhos de teclado. */
  onItems: (ids: string[]) => void
}

export function MessageList({ nav, onOpen, onMenu, onItems }: Props) {
  const [search, setSearch] = useState('')
  const [query, setQuery] = useState('')
  useEffect(() => {
    const t = setTimeout(() => setQuery(search.trim()), 300)
    return () => clearTimeout(t)
  }, [search])

  const messages = useMessages(nav.folder, nav.domain || undefined, query || undefined)
  const domains = useDomains()
  const colors = useMemo(
    () => new Map((domains.data?.items ?? []).map((d) => [d.name, d.color])),
    [domains.data],
  )
  const items = useMemo(() => messages.data?.pages.flatMap((p) => p.items) ?? [], [messages.data])
  useEffect(() => onItems(items.map((i) => i.id)), [items, onItems])

  const domainLabel = domains.data?.items.find((d) => d.name === nav.domain)?.label
  const heading = domainLabel ? `${domainLabel} · ${FOLDER_LABELS[nav.folder]}` : FOLDER_LABELS[nav.folder]

  return (
    <section
      aria-label="Mensagens"
      className={cn('flex min-w-0 flex-1 flex-col border-r border-border lg:max-w-[540px]', nav.id && 'hidden lg:flex')}
    >
      <header className="flex h-16 shrink-0 items-center gap-3 border-b border-border px-4">
        <button aria-label="Abrir menu" onClick={onMenu} className="rounded-lg p-2 text-muted-foreground hover:bg-accent md:hidden">
          <Menu className="size-5" />
        </button>
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-base font-semibold tracking-tight">{query ? `Busca: “${query}”` : heading}</h1>
          {nav.domain && <p className="truncate text-xs text-muted-foreground">{nav.domain}</p>}
        </div>
      </header>

      <div className="border-b border-border px-4 py-2.5">
        <label className="flex items-center gap-2 rounded-lg bg-muted/70 px-3 py-2">
          <Search className="size-4 text-muted-foreground" aria-hidden="true" />
          <input
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Buscar mensagens"
            aria-label="Buscar mensagens"
            className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
          />
        </label>
      </div>

      <div
        className="min-h-0 flex-1 overflow-y-auto"
        onScroll={(e) => {
          const el = e.currentTarget
          if (el.scrollHeight - el.scrollTop - el.clientHeight < 200 && messages.hasNextPage && !messages.isFetchingNextPage) {
            void messages.fetchNextPage()
          }
        }}
      >
        {messages.isLoading ? (
          <ListSkeleton />
        ) : messages.isError ? (
          <ErrorState
            message={messages.error instanceof ApiRequestError ? messages.error.message : undefined}
            onRetry={() => void messages.refetch()}
          />
        ) : items.length === 0 ? (
          <EmptyState folder={nav.folder} />
        ) : (
          <ul>
            {items.map((item) => (
              <MessageRow
                key={item.id}
                item={item}
                active={nav.id === item.id}
                domainColor={nav.domain ? undefined : colors.get(item.domain)}
                onOpen={() => onOpen(item.id)}
              />
            ))}
          </ul>
        )}
        {messages.isFetchingNextPage && <ListSkeleton count={2} />}
        {!messages.hasNextPage && items.length > 0 && (
          <p className="py-5 text-center text-xs text-muted-foreground">Fim da lista</p>
        )}
      </div>
    </section>
  )
}

function MessageRow({ item, active, domainColor, onOpen }: {
  item: ListItem
  active: boolean
  /** Só na caixa unificada: etiqueta na cor do domínio. */
  domainColor?: string
  onOpen: () => void
}) {
  const sender = item.from?.name || item.from?.addr || '(sem remetente)'
  return (
    <li className="list-none">
      <button
        onClick={onOpen}
        aria-current={active ? 'true' : undefined}
        data-message-id={item.id}
        className={cn(
          'flex h-16 w-full items-center gap-3 border-b border-border/70 px-4 text-left transition-colors',
          active ? 'bg-accent' : 'hover:bg-accent/50',
        )}
      >
        <span className="flex w-2 shrink-0 justify-center" aria-hidden="true">
          {!item.is_read && <span className="size-2 rounded-full bg-primary" />}
        </span>
        <span className="min-w-0 flex-1">
          <span className="flex items-baseline justify-between gap-3">
            <span className={cn('truncate text-sm', !item.is_read && 'font-semibold')}>{sender}</span>
            <time dateTime={new Date(item.received_at).toISOString()} className="shrink-0 text-[11px] text-muted-foreground">
              {formatListDate(item.received_at)}
            </time>
          </span>
          <span className="mt-0.5 flex min-w-0 items-center gap-2">
            <span className={cn('truncate text-sm', item.is_read ? 'text-muted-foreground' : 'font-medium')}>
              {item.subject || '(sem assunto)'}
            </span>
            {item.snippet && <span className="hidden truncate text-xs text-muted-foreground sm:inline">— {item.snippet}</span>}
          </span>
        </span>
        <span className="flex shrink-0 items-center gap-1.5">
          {domainColor && (
            <span
              style={{ '--domain': domainColor } as React.CSSProperties}
              className="rounded border border-[var(--domain)]/50 bg-[var(--domain)]/10 px-1.5 py-0.5 text-[10px] font-medium text-foreground"
            >
              {item.domain}
            </span>
          )}
          {item.processing === 'partial' && (
            <span title="Email grande: sem corpo processado; baixe o .eml" className="rounded bg-muted px-1.5 py-0.5 text-[10px] font-medium uppercase text-muted-foreground">
              grande
            </span>
          )}
          {item.has_attachments && <Paperclip className="size-3.5 text-muted-foreground" aria-label="Com anexo" />}
          {item.is_starred && <Star className="size-3.5 fill-star text-star" aria-label="Com estrela" />}
          {!item.is_read && <span className="sr-only">Não lida</span>}
        </span>
      </button>
    </li>
  )
}
