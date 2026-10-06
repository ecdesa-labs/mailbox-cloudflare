'use client'

import { useEffect, useState } from 'react'
import { useTheme } from 'next-themes'
import {
  Archive, ChevronDown, ChevronLeft, ChevronRight, Inbox, Laptop, Moon, SquarePen, Send,
  Settings2, ShieldAlert, Sun, Trash2, X, type LucideIcon,
} from 'lucide-react'
import { useDomains, useMe } from '@/lib/api/queries'
import type { Domain, Folder } from '@/lib/api/types'
import { cn } from '@/lib/utils'
import { FOLDER_LABELS } from './format'
import { FOLDERS, type NavState } from './nav'

const FOLDER_ICONS: Record<Folder, LucideIcon> = {
  inbox: Inbox,
  archive: Archive,
  trash: Trash2,
  spam: ShieldAlert,
  sent: Send,
}

interface Props {
  nav: NavState
  onNavigate: (next: Partial<NavState>) => void
  collapsed: boolean
  onCollapse: () => void
  /** Gaveta aberta (telas pequenas). */
  open: boolean
  onClose: () => void
  onCompose: () => void
}

export function Sidebar({ nav, onNavigate, collapsed, onCollapse, open, onClose, onCompose }: Props) {
  const me = useMe()
  const domains = useDomains()
  const items = domains.data?.items ?? []
  const totalUnread = items.reduce((sum, d) => sum + d.unread, 0)
  const go = (next: Partial<NavState>) => {
    onNavigate({ ...next, id: '' })
    onClose()
  }

  return (
    <>
      {open && (
        <button aria-label="Fechar menu" className="fixed inset-0 z-30 bg-black/40 md:hidden" onClick={onClose} />
      )}
      <aside
        aria-label="Navegação"
        className={cn(
          'fixed inset-y-0 left-0 z-40 flex w-[264px] shrink-0 flex-col border-r border-sidebar-border bg-sidebar text-sidebar-foreground transition-transform md:relative md:translate-x-0',
          collapsed && 'md:w-[68px]',
          open ? 'translate-x-0' : '-translate-x-full',
        )}
      >
        <div className="flex h-16 items-center gap-3 border-b border-sidebar-border px-4">
          <div className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-primary text-primary-foreground">
            <Inbox className="size-4" aria-hidden="true" />
          </div>
          {!collapsed && (
            <div className="min-w-0">
              <p className="font-semibold tracking-tight">Mailbox</p>
              <p className="truncate text-xs text-muted-foreground">{me.data?.email ?? 'Carregando...'}</p>
            </div>
          )}
          <button onClick={onClose} aria-label="Fechar menu" className="ml-auto rounded-lg p-2 text-muted-foreground hover:bg-sidebar-accent md:hidden">
            <X className="size-4" />
          </button>
        </div>

        <div className="px-2 pt-4">
          <button
            onClick={() => {
              onCompose()
              onClose()
            }}
            aria-label="Escrever"
            title="Escrever"
            className={cn(
              'flex h-10 w-full items-center gap-2 rounded-lg bg-primary text-sm font-medium text-primary-foreground hover:bg-primary/90',
              collapsed ? 'justify-center' : 'px-3',
            )}
          >
            <SquarePen className="size-4 shrink-0" aria-hidden="true" />
            {!collapsed && 'Escrever'}
          </button>
        </div>

        <nav className="flex-1 overflow-y-auto px-2 py-4">
          <Section title="Caixa unificada" collapsed={collapsed}>
            {FOLDERS.map((f) => (
              <NavItem
                key={f}
                icon={FOLDER_ICONS[f]}
                label={FOLDER_LABELS[f]}
                count={f === 'inbox' ? totalUnread : undefined}
                active={!nav.domain && nav.folder === f}
                collapsed={collapsed}
                onClick={() => go({ folder: f, domain: '' })}
              />
            ))}
          </Section>

          <Section title="Domínios" collapsed={collapsed}>
            {domains.isLoading ? (
              <div className="flex flex-col gap-2 px-3" aria-hidden="true">
                <div className="h-8 animate-pulse rounded-lg bg-sidebar-accent" />
                <div className="h-8 animate-pulse rounded-lg bg-sidebar-accent" />
              </div>
            ) : (
              items.map((d) => <DomainGroup key={d.id} domain={d} nav={nav} collapsed={collapsed} onNavigate={go} />)
            )}
          </Section>
        </nav>

        <div className="border-t border-sidebar-border p-2">
          <NavItem icon={Settings2} label="Regras" collapsed={collapsed} active={false} disabled onClick={() => {}} badge="Em breve" />
          <div className="mt-1 flex items-center gap-1">
            <ThemeToggle collapsed={collapsed} />
            <button
              onClick={onCollapse}
              aria-label={collapsed ? 'Expandir barra lateral' : 'Recolher barra lateral'}
              className="hidden rounded-lg p-2.5 text-muted-foreground hover:bg-sidebar-accent md:block"
            >
              {collapsed ? <ChevronRight className="size-4" /> : <ChevronLeft className="size-4" />}
            </button>
          </div>
        </div>
      </aside>
    </>
  )
}

function Section({ title, collapsed, children }: { title: string; collapsed: boolean; children: React.ReactNode }) {
  return (
    <div className="mb-5">
      {!collapsed && (
        <p className="mb-1.5 px-3 text-[10px] font-semibold uppercase tracking-[0.14em] text-muted-foreground">{title}</p>
      )}
      <ul className="flex flex-col gap-0.5">{children}</ul>
    </div>
  )
}

function NavItem({
  icon: Icon, label, count, active, collapsed, onClick, disabled, badge, dense,
}: {
  icon: LucideIcon
  label: string
  count?: number
  active: boolean
  collapsed: boolean
  onClick: () => void
  disabled?: boolean
  badge?: string
  dense?: boolean
}) {
  return (
    <li className="list-none">
      <button
        onClick={onClick}
        disabled={disabled}
        aria-current={active ? 'page' : undefined}
        aria-label={collapsed ? label : undefined}
        title={collapsed ? label : undefined}
        className={cn(
          'flex w-full items-center gap-3 rounded-lg px-3 text-sm transition-colors disabled:cursor-default disabled:opacity-60',
          dense ? 'py-1.5' : 'py-2',
          active
            ? 'bg-sidebar-accent font-medium text-sidebar-accent-foreground'
            : 'text-muted-foreground hover:bg-sidebar-accent/70 hover:text-sidebar-foreground',
        )}
      >
        <Icon className="size-4 shrink-0" aria-hidden="true" />
        {!collapsed && (
          <>
            <span className="truncate">{label}</span>
            {count ? <span className="ml-auto text-xs font-semibold tabular-nums">{count}</span> : null}
            {badge ? <span className="ml-auto rounded-full bg-sidebar-accent px-1.5 py-0.5 text-[10px]">{badge}</span> : null}
          </>
        )}
      </button>
    </li>
  )
}

/** Um domínio, na cor dele, com as próprias pastas (recolhíveis). */
function DomainGroup({ domain, nav, collapsed, onNavigate }: {
  domain: Domain
  nav: NavState
  collapsed: boolean
  onNavigate: (next: Partial<NavState>) => void
}) {
  const isActive = nav.domain === domain.name
  const [expanded, setExpanded] = useState(isActive)
  useEffect(() => {
    if (isActive) setExpanded(true)
  }, [isActive])

  return (
    <li className="list-none" style={{ '--domain': domain.color } as React.CSSProperties}>
      <div className="flex items-center">
        <button
          onClick={() => onNavigate({ folder: 'inbox', domain: domain.name })}
          aria-current={isActive && nav.folder === 'inbox' ? 'page' : undefined}
          aria-label={collapsed ? domain.label : undefined}
          title={collapsed ? domain.label : undefined}
          className={cn(
            'flex min-w-0 flex-1 items-center gap-3 rounded-lg px-3 py-2 text-sm',
            isActive ? 'font-medium text-sidebar-foreground' : 'text-muted-foreground hover:bg-sidebar-accent/70',
          )}
        >
          <span className="size-2.5 shrink-0 rounded-full bg-[var(--domain)]" aria-hidden="true" />
          {!collapsed && (
            <>
              <span className="truncate">{domain.label}</span>
              {domain.unread ? <span className="ml-auto text-xs font-semibold tabular-nums">{domain.unread}</span> : null}
            </>
          )}
        </button>
        {!collapsed && (
          <button
            onClick={() => setExpanded((e) => !e)}
            aria-expanded={expanded}
            aria-label={`${expanded ? 'Recolher' : 'Expandir'} pastas de ${domain.label}`}
            className="rounded-md p-1.5 text-muted-foreground hover:bg-sidebar-accent"
          >
            <ChevronDown className={cn('size-3.5 transition-transform', !expanded && '-rotate-90')} />
          </button>
        )}
      </div>
      {!collapsed && expanded && (
        <ul className="ml-[18px] border-l-2 border-[var(--domain)]/40 pl-2">
          {FOLDERS.map((f) => (
            <NavItem
              key={f}
              dense
              icon={FOLDER_ICONS[f]}
              label={FOLDER_LABELS[f]}
              count={f === 'inbox' ? domain.unread : undefined}
              active={isActive && nav.folder === f}
              collapsed={false}
              onClick={() => onNavigate({ folder: f, domain: domain.name })}
            />
          ))}
        </ul>
      )}
    </li>
  )
}

/** Sistema → claro → escuro. Salvo pelo next-themes (sem piscar ao carregar). */
function ThemeToggle({ collapsed }: { collapsed: boolean }) {
  const { theme, setTheme } = useTheme()
  const [mounted, setMounted] = useState(false)
  useEffect(() => setMounted(true), [])
  const current = mounted ? (theme ?? 'system') : 'system'
  const next = current === 'system' ? 'light' : current === 'light' ? 'dark' : 'system'
  const label = { system: 'Tema do sistema', light: 'Tema claro', dark: 'Tema escuro' }[current] ?? 'Tema'
  const Icon = current === 'light' ? Sun : current === 'dark' ? Moon : Laptop
  return (
    <button
      onClick={() => setTheme(next)}
      aria-label={`${label} (trocar)`}
      title={collapsed ? label : undefined}
      className="flex flex-1 items-center gap-3 rounded-lg px-3 py-2 text-sm text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-foreground"
    >
      <Icon className="size-4" aria-hidden="true" />
      {!collapsed && label}
    </button>
  )
}
