import type { Folder } from '@/lib/api/types'

/**
 * Atalhos de teclado. Esta tabela é a fonte da verdade: o tratamento das
 * teclas (mail-app.tsx) e a ajuda (`?`) leem daqui.
 */
export type ShortcutAction =
  | 'next'
  | 'prev'
  | 'close'
  | 'search'
  | 'select'
  | 'archive'
  | 'trash'
  | 'spam'
  | 'star'
  | 'read'
  | 'unread'
  | 'undo'
  | 'compose'
  | 'reply'
  | 'reply-all'
  | 'help'
  /** Sequência `g` + letra (GO_TO); tratada à parte. */
  | 'goto'

export interface Shortcut {
  /** Valores de `KeyboardEvent.key` que disparam. */
  keys: string[]
  /** Como aparece na ajuda. */
  label: string[]
  action: ShortcutAction
  description: string
}

export const SHORTCUT_GROUPS: { title: string; items: Shortcut[] }[] = [
  {
    title: 'Navegar',
    items: [
      { keys: ['j', 'ArrowDown'], label: ['j', '↓'], action: 'next', description: 'Próxima mensagem' },
      { keys: ['k', 'ArrowUp'], label: ['k', '↑'], action: 'prev', description: 'Mensagem anterior' },
      { keys: ['Escape'], label: ['Esc'], action: 'close', description: 'Fechar a mensagem ou limpar a seleção' },
      { keys: ['/'], label: ['/'], action: 'search', description: 'Buscar' },
    ],
  },
  {
    title: 'Agir (na seleção ou na mensagem aberta)',
    items: [
      { keys: ['x'], label: ['x'], action: 'select', description: 'Selecionar a mensagem aberta' },
      { keys: ['e'], label: ['e'], action: 'archive', description: 'Arquivar' },
      { keys: ['#'], label: ['#'], action: 'trash', description: 'Mover para a lixeira' },
      { keys: ['!'], label: ['!'], action: 'spam', description: 'Marcar como spam' },
      { keys: ['s'], label: ['s'], action: 'star', description: 'Estrela (liga/desliga)' },
      { keys: ['I'], label: ['Shift', 'I'], action: 'read', description: 'Marcar como lida' },
      { keys: ['U'], label: ['Shift', 'U'], action: 'unread', description: 'Marcar como não lida' },
      { keys: ['z'], label: ['z'], action: 'undo', description: 'Desfazer a última mudança de pasta' },
    ],
  },
  {
    title: 'Escrever',
    items: [
      { keys: ['c'], label: ['c'], action: 'compose', description: 'Nova mensagem' },
      { keys: ['r'], label: ['r'], action: 'reply', description: 'Responder' },
      { keys: ['a'], label: ['a'], action: 'reply-all', description: 'Responder a todos' },
    ],
  },
  {
    title: 'Ir para (g, depois a letra)',
    items: [
      { keys: [], label: ['g', 'i'], action: 'goto', description: 'Entrada' },
      { keys: [], label: ['g', 'a'], action: 'goto', description: 'Arquivo' },
      { keys: [], label: ['g', 't'], action: 'goto', description: 'Enviados' },
      { keys: [], label: ['g', 's'], action: 'goto', description: 'Spam' },
      { keys: [], label: ['g', 'l'], action: 'goto', description: 'Lixeira' },
    ],
  },
  {
    title: 'Ajuda',
    items: [{ keys: ['?'], label: ['?'], action: 'help', description: 'Mostrar e esconder esta lista' }],
  },
]

/** Segunda tecla depois de `g`. */
export const GO_TO: Record<string, Folder> = {
  i: 'inbox',
  a: 'archive',
  t: 'sent',
  s: 'spam',
  l: 'trash',
}

const BY_KEY = new Map<string, ShortcutAction>(
  SHORTCUT_GROUPS.flatMap((g) => g.items.flatMap((s) => s.keys.map((k) => [k, s.action] as const))),
)

/**
 * Letras pelo Shift, não pela caixa: com Caps Lock, `e` chega como `E`;
 * alguns ambientes mandam `u` + Shift em vez de `U`.
 */
export function normalizeKey(e: Pick<KeyboardEvent, 'key' | 'shiftKey'>): string {
  if (!/^[a-z]$/i.test(e.key)) return e.key
  return e.shiftKey ? e.key.toUpperCase() : e.key.toLowerCase()
}

export function shortcutFor(key: string): ShortcutAction | undefined {
  return BY_KEY.get(key)
}

/** Tecla vinda de um campo de texto não é atalho. */
export function isTyping(target: EventTarget | null): boolean {
  const t = target as HTMLElement | null
  return !!t && (t.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName))
}
