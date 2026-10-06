import type { Folder } from '@/lib/api/types'

export const FOLDER_LABELS: Record<Folder, string> = {
  inbox: 'Entrada',
  archive: 'Arquivo',
  trash: 'Lixeira',
  spam: 'Spam',
  sent: 'Enviados',
}

export const EMPTY_LABELS: Record<Folder, string> = {
  inbox: 'Nenhuma mensagem na entrada',
  archive: 'Nenhuma mensagem no arquivo',
  trash: 'Nenhuma mensagem na lixeira',
  spam: 'Nenhuma mensagem no spam',
  sent: 'Nenhuma mensagem enviada',
}

function locale(): string {
  return typeof navigator !== 'undefined' && navigator.language ? navigator.language : 'pt-BR'
}

/** Hoje = hora; este ano = dia e mês; antes = data completa. Fuso do navegador. */
export function formatListDate(ms: number, now = new Date()): string {
  const d = new Date(ms)
  const loc = locale()
  if (d.toDateString() === now.toDateString()) {
    return d.toLocaleTimeString(loc, { hour: '2-digit', minute: '2-digit' })
  }
  if (d.getFullYear() === now.getFullYear()) {
    return d.toLocaleDateString(loc, { day: '2-digit', month: 'short' }).replace('.', '')
  }
  return d.toLocaleDateString(loc, { day: '2-digit', month: '2-digit', year: 'numeric' })
}

/** Data completa para o cabeçalho do leitor. */
export function formatFullDate(ms: number): string {
  return new Date(ms).toLocaleString(locale(), { dateStyle: 'full', timeStyle: 'short' })
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  const units = ['KB', 'MB', 'GB']
  let v = n / 1024
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  return `${v.toLocaleString(undefined, { maximumFractionDigits: v < 10 ? 1 : 0 })} ${units[i]}`
}
