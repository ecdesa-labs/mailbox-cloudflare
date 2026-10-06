'use client'

import { useEffect, useState } from 'react'
import { attachmentDataUrl } from '@/lib/api/client'

/**
 * HTML do email (já sanitizado no servidor) num iframe isolado:
 *
 * - `sandbox` sem `allow-scripts` nem `allow-same-origin`: nenhum script roda
 *   e o conteúdo não tem acesso à sessão nem à API. Links abrem em nova aba
 *   (`allow-popups-to-escape-sandbox`), fora do sandbox.
 * - CSP dentro do documento: nada carrega da rede, exceto imagens `https:`
 *   quando o usuário libera as remotas. Uma segunda camada caso algo escape da
 *   sanitização (pixels de rastreamento continuam bloqueados).
 * - Imagens inline (`/api/attachments/...`) viram `data:` aqui, pela origem
 *   do app: o iframe isolado não teria o cookie do Access para buscá-las.
 * - Fundo branco sempre: emails são feitos para fundo claro.
 */
export function EmailFrame({ html, remoteImages, title }: { html: string; remoteImages: boolean; title: string }) {
  const [doc, setDoc] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    void buildDocument(html, remoteImages).then((d) => {
      if (!cancelled) setDoc(d)
    })
    return () => {
      cancelled = true
    }
  }, [html, remoteImages])

  if (doc === null) {
    return <div className="h-full min-h-64 animate-pulse rounded-lg bg-muted" aria-hidden="true" />
  }
  return (
    <iframe
      title={title}
      srcDoc={doc}
      sandbox="allow-popups allow-popups-to-escape-sandbox"
      referrerPolicy="no-referrer"
      className="h-full min-h-64 w-full rounded-lg border border-border bg-white"
    />
  )
}

async function buildDocument(html: string, remoteImages: boolean): Promise<string> {
  // DOMParser não executa scripts nem carrega recursos.
  const parsed = new DOMParser().parseFromString(html, 'text/html')

  const inline = [...parsed.querySelectorAll<HTMLImageElement>('img[src^="/api/attachments/"]')]
  const urls = [...new Set(inline.map((img) => img.getAttribute('src') ?? ''))]
  const data = new Map(await Promise.all(urls.map(async (u) => [u, await attachmentDataUrl(u)] as const)))
  for (const img of inline) {
    const d = data.get(img.getAttribute('src') ?? '')
    if (d) img.setAttribute('src', d)
    else img.removeAttribute('src')
  }

  const csp = [
    "default-src 'none'",
    `img-src data:${remoteImages ? ' https:' : ''}`,
    "style-src 'unsafe-inline'",
    'font-src data:',
  ].join('; ')

  return `<!doctype html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="${csp}">
<meta name="referrer" content="no-referrer">
<base target="_blank">
<style>
  html { background: #fff; color: #111; color-scheme: light; }
  body { margin: 16px; font: 14px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif; overflow-wrap: anywhere; }
  img { max-width: 100%; height: auto; }
  table { max-width: 100%; }
  pre { white-space: pre-wrap; }
</style></head><body>${parsed.body.innerHTML}</body></html>`
}
