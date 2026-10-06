'use client'

import { useEffect, useMemo, useState } from 'react'
import {
  Archive, Inbox, Mail, MailOpen, Menu, Paperclip, Search, ShieldAlert, Star, Trash2, X,
  type LucideIcon,
} from 'lucide-react'
import { useDomains, useMessages } from '@/lib/api/queries'
import { ApiRequestError, type Folder, type ListItem, type MessagePatch } from '@/lib/api/types'
import { cn } from '@/lib/utils'
import { FOLDER_LABELS, formatListDate } from './format'
import type { NavState } from './nav'
import { EmptyState, ErrorState, ListSkeleton } from './states'

interface Props {
  nav: NavState
  onNavigate: (next: Partial<NavState>, opts?: { replace?: boolean }) => void
  onOpen: (id: string) => void
  onMenu: () => void
  /** Itens carregados, para os atalhos de teclado e a seleção. */
  onItems: (items: ListItem[]) => void
  selected: Set<string>
  onSelect: (ids: string[], on: boolean) => void
  onClearSelection: () => void
  onAct: (items: ListItem[], patch: MessagePatch, opts?: { undoable?: boolean }) => void
}

export function MessageList(props: Props) {
  const { nav, onNavigate, onOpen, onMenu, onItems, selected, onSelect, onClearSelection, onAct } = props

  // Busca: o campo responde na hora; a URL (e a API) só depois de 300 ms.
  const [search, setSearch] = useState(nav.q)
  useEffect(() => setSearch(nav.q), [nav.q])
  useEffect(() => {
    const value = search.trim()
    if (value === nav.q) return
    const t = setTimeout(() => onNavigate({ q: value, id: '' }, { replace: true }), 300)
    return () => clearTimeout(t)
  }, [search, nav.q, onNavigate])

  // Numa busca sem pasta escolhida, a API olha todas as pastas.
  const folderFilter = nav.q && !nav.folderExplicit ? undefined : nav.folder
  const messages = useMessages(folderFilter, nav.domain || undefined, nav.q || undefined)
  const domains = useDomains()
  const colors = useMemo(
    () => new Map((domains.data?.items ?? []).map((d) => [d.name, d.color])),
    [domains.data],
  )
  const items = useMemo(() => messages.data?.pages.flatMap((p) => p.items) ?? [], [messages.data])
  useEffect(() => onItems(items), [items, onItems])

  const domainLabel = domains.data?.items.find((d) => d.name === nav.domain)?.label
  const place = domainLabel ? `${domainLabel} · ${FOLDER_LABELS[nav.folder]}` : FOLDER_LABELS[nav.folder]
  const chosen = items.filter((i) => selected.has(i.id))
  const allChosen = items.length > 0 && chosen.length === items.length

  return (
    <section
      aria-label="Mensagens"
      className={cn('flex min-w-0 flex-1 flex-col border-r border-border lg:max-w-[560px]', nav.id && 'hidden lg:flex')}
    >
      <header className="flex h-16 shrink-0 items-center gap-3 border-b border-border px-4">
        <button aria-label="Abrir menu" onClick={onMenu} className="rounded-lg p-2 text-muted-foreground hover:bg-accent md:hidden">
          <Menu className="size-5" />
        </button>
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-base font-semibold tracking-tight">{nav.q ? `Busca: “${nav.q}”` : place}</h1>
          <p className="truncate text-xs text-muted-foreground">
            {nav.q ? (folderFilter ? `em ${place}` : `em todas as pastas${domainLabel ? ` de ${domainLabel}` : ''}`) : nav.domain}
          </p>
        </div>
      </header>

      <div className="border-b border-border px-4 py-2.5">
        <label className="flex items-center gap-2 rounded-lg bg-muted/70 px-3 py-2">
          <Search className="size-4 text-muted-foreground" aria-hidden="true" />
          <input
            id="mail-search"
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => e.key === 'Escape' && setSearch('')}
            placeholder="Buscar por assunto, remetente ou texto"
            aria-label="Buscar mensagens"
            className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground [&::-webkit-search-cancel-button]:hidden"
          />
          {search && (
            <button onClick={() => setSearch('')} aria-label="Limpar busca" className="rounded p-0.5 text-muted-foreground hover:text-foreground">
              <X className="size-4" />
            </button>
          )}
        </label>
      </div>

      {items.length > 0 && (
        <BulkBar
          folder={nav.q && !nav.folderExplicit ? undefined : nav.folder}
          count={chosen.length}
          total={items.length}
          all={allChosen}
          onToggleAll={() => onSelect(items.map((i) => i.id), !allChosen)}
          onClear={onClearSelection}
          chosen={chosen}
          onAct={(patch, undoable) => {
            onAct(chosen, patch, { undoable })
            if (patch.folder) onClearSelection()
          }}
        />
      )}

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
          nav.q ? <NoResults q={nav.q} /> : <EmptyState folder={nav.folder} />
        ) : (
          <ul>
            {items.map((item) => (
              <MessageRow
                key={item.id}
                item={item}
                active={nav.id === item.id}
                checked={selected.has(item.id)}
                selecting={selected.size > 0}
                domainColor={nav.domain ? undefined : colors.get(item.domain)}
                onOpen={() => onOpen(item.id)}
                onCheck={(on) => onSelect([item.id], on)}
                onAct={(patch, undoable) => onAct([item], patch, { undoable })}
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

function NoResults({ q }: { q: string }) {
  return (
    <div className="flex h-full min-h-72 flex-col items-center justify-center px-8 text-center">
      <div className="mb-4 flex size-12 items-center justify-center rounded-xl bg-muted">
        <Search className="size-5 text-muted-foreground" aria-hidden="true" />
      </div>
      <p className="text-sm font-medium">Nada encontrado para “{q}”</p>
      <p className="mt-1 text-xs text-muted-foreground">A busca considera assunto, remetente e texto, sem diferenciar acentos.</p>
    </div>
  )
}

/** Destinos de "mover" a partir de cada pasta. */
function moves(folder: Folder | undefined): { to: Folder; label: string; icon: LucideIcon }[] {
  const all: { to: Folder; label: string; icon: LucideIcon }[] = [
    { to: 'inbox', label: 'Mover para a entrada', icon: Inbox },
    { to: 'archive', label: 'Arquivar', icon: Archive },
    { to: 'spam', label: 'Marcar como spam', icon: ShieldAlert },
    { to: 'trash', label: 'Mover para a lixeira', icon: Trash2 },
  ]
  return all.filter((m) => m.to !== folder && !(folder === 'sent' && m.to === 'inbox'))
}

function BulkBar({ folder, count, total, all, onToggleAll, onClear, chosen, onAct }: {
  folder: Folder | undefined
  count: number
  total: number
  all: boolean
  onToggleAll: () => void
  onClear: () => void
  chosen: ListItem[]
  onAct: (patch: MessagePatch, undoable?: boolean) => void
}) {
  const anyUnread = chosen.some((m) => !m.is_read)
  const allStarred = chosen.length > 0 && chosen.every((m) => m.is_starred)
  return (
    <div className="flex h-11 shrink-0 items-center gap-1 border-b border-border px-3">
      <input
        type="checkbox"
        checked={all}
        ref={(el) => {
          if (el) el.indeterminate = count > 0 && !all
        }}
        onChange={onToggleAll}
        aria-label={all ? 'Desmarcar todas' : `Selecionar as ${total} carregadas`}
        className="mx-1.5 size-4 accent-primary"
      />
      {count === 0 ? (
        <span className="text-xs text-muted-foreground">Selecionar</span>
      ) : (
        <>
          <span className="mr-1 text-xs font-medium tabular-nums" aria-live="polite">
            {count} {count === 1 ? 'selecionada' : 'selecionadas'}
          </span>
          <IconAction
            icon={anyUnread ? MailOpen : Mail}
            label={anyUnread ? 'Marcar como lidas' : 'Marcar como não lidas'}
            onClick={() => onAct({ read: anyUnread })}
          />
          <IconAction
            icon={Star}
            label={allStarred ? 'Tirar estrela' : 'Marcar com estrela'}
            onClick={() => onAct({ starred: !allStarred })}
          />
          {moves(folder).map((m) => (
            <IconAction key={m.to} icon={m.icon} label={m.label} onClick={() => onAct({ folder: m.to }, true)} />
          ))}
          <button onClick={onClear} className="ml-auto rounded-md px-2 py-1 text-xs text-muted-foreground hover:bg-accent">
            Limpar
          </button>
        </>
      )}
    </div>
  )
}

function IconAction({ icon: Icon, label, onClick, className }: { icon: LucideIcon; label: string; onClick: () => void; className?: string }) {
  return (
    <button
      onClick={(e) => {
        e.stopPropagation()
        onClick()
      }}
      aria-label={label}
      title={label}
      className={cn('rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground', className)}
    >
      <Icon className="size-4" aria-hidden="true" />
    </button>
  )
}

function MessageRow({ item, active, checked, selecting, domainColor, onOpen, onCheck, onAct }: {
  item: ListItem
  active: boolean
  checked: boolean
  /** Há alguma selecionada: as caixas ficam sempre visíveis. */
  selecting: boolean
  /** Só na caixa unificada: etiqueta na cor do domínio. */
  domainColor?: string
  onOpen: () => void
  onCheck: (on: boolean) => void
  onAct: (patch: MessagePatch, undoable?: boolean) => void
}) {
  const sender = item.from?.name || item.from?.addr || '(sem remetente)'
  return (
    <li
      className={cn(
        'group relative flex h-16 items-center border-b border-border/70 transition-colors',
        active ? 'bg-accent' : checked ? 'bg-primary/5' : 'hover:bg-accent/50',
      )}
    >
      <label className="flex h-full w-10 shrink-0 cursor-pointer items-center justify-center">
        <input
          type="checkbox"
          checked={checked}
          onChange={(e) => onCheck(e.target.checked)}
          aria-label={`Selecionar: ${item.subject || '(sem assunto)'}`}
          className={cn(
            'size-4 accent-primary transition-opacity',
            selecting || checked ? 'opacity-100' : 'opacity-0 focus:opacity-100 group-hover:opacity-100',
          )}
        />
      </label>
      <button
        onClick={onOpen}
        aria-current={active ? 'true' : undefined}
        data-message-id={item.id}
        className="flex h-full min-w-0 flex-1 items-center gap-3 pr-3 text-left"
      >
        <span className="flex w-2 shrink-0 justify-center" aria-hidden="true">
          {!item.is_read && <span className="size-2 rounded-full bg-primary" />}
        </span>
        <span className="min-w-0 flex-1">
          <span className="flex items-baseline justify-between gap-3">
            <span className={cn('truncate text-sm', !item.is_read && 'font-semibold')}>{sender}</span>
            <time
              dateTime={new Date(item.received_at).toISOString()}
              className="shrink-0 text-[11px] text-muted-foreground group-hover:invisible group-focus-within:invisible"
            >
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
          {!item.is_read && <span className="sr-only">Não lida</span>}
        </span>
      </button>
      {/* Estrela sempre visível; arquivar e lixeira aparecem no hover/foco. */}
      <div className="flex shrink-0 items-center pr-2">
        <span className="absolute right-12 top-1.5 hidden items-center gap-0.5 rounded-md bg-background/95 px-0.5 shadow-sm group-hover:flex group-focus-within:flex">
          {item.folder !== 'archive' && <IconAction icon={Archive} label="Arquivar" onClick={() => onAct({ folder: 'archive' }, true)} />}
          {item.folder !== 'trash' && <IconAction icon={Trash2} label="Mover para a lixeira" onClick={() => onAct({ folder: 'trash' }, true)} />}
          <IconAction
            icon={item.is_read ? Mail : MailOpen}
            label={item.is_read ? 'Marcar como não lida' : 'Marcar como lida'}
            onClick={() => onAct({ read: !item.is_read })}
          />
        </span>
        <button
          onClick={() => onAct({ starred: !item.is_starred })}
          aria-label={item.is_starred ? 'Tirar estrela' : 'Marcar com estrela'}
          aria-pressed={item.is_starred}
          className="rounded-md p-1.5 hover:bg-accent"
        >
          <Star className={cn('size-4', item.is_starred ? 'fill-star text-star' : 'text-muted-foreground/50 hover:text-muted-foreground')} />
        </button>
      </div>
    </li>
  )
}
