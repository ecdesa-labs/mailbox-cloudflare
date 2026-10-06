'use client'

import { MutationCache, QueryCache, QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { ThemeProvider } from 'next-themes'
import { useState } from 'react'
import { ApiRequestError } from '@/lib/api/types'

const isUnauthorized = (e: unknown) => e instanceof ApiRequestError && e.status === 401
/** Login do Access válido, mas o email não está na lista da caixa. */
const notAllowed = (e: unknown) => (e instanceof ApiRequestError && e.code === 'not_allowed' ? e : null)

export function Providers({ children }: { children: React.ReactNode }) {
  // Qualquer 401 (consulta ou mutação) abre um único aviso por cima do app;
  // email fora da lista abre outro, que explica (não é sessão expirada).
  const [expired, setExpired] = useState(false)
  const [denied, setDenied] = useState<string | null>(null)
  const [queryClient] = useState(() => {
    const onError = (e: unknown) => {
      const n = notAllowed(e)
      if (n) setDenied(n.email ?? '')
      else if (isUnauthorized(e)) setExpired(true)
    }
    return new QueryClient({
      queryCache: new QueryCache({ onError }),
      mutationCache: new MutationCache({ onError }),
      defaultOptions: {
        queries: {
          staleTime: 30_000,
          refetchOnWindowFocus: false,
          // Só erro de rede ou 5xx, no máximo 2 vezes. 4xx não melhora repetindo.
          retry: (failures, e) =>
            failures < 2 && (!(e instanceof ApiRequestError) || e.status >= 500),
        },
      },
    })
  })
  return (
    <QueryClientProvider client={queryClient}>
      <ThemeProvider attribute="class" defaultTheme="system" enableSystem storageKey="mailbox-theme" disableTransitionOnChange>
        {children}
        {denied !== null ? <NotAllowed email={denied} /> : expired && <SessionExpired />}
      </ThemeProvider>
    </QueryClientProvider>
  )
}

function SessionExpired() {
  return (
    <div role="alertdialog" aria-modal="true" aria-labelledby="session-title" className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
      <div className="w-full max-w-sm rounded-xl border border-border bg-card p-6 text-card-foreground shadow-xl">
        <h2 id="session-title" className="text-base font-semibold">Sua sessão expirou</h2>
        <p className="mt-2 text-sm text-muted-foreground">Entre de novo para continuar de onde parou.</p>
        <button
          autoFocus
          onClick={() => window.location.reload()}
          className="mt-5 w-full rounded-lg bg-primary px-3 py-2 text-sm font-medium text-primary-foreground hover:opacity-90"
        >
          Entrar de novo
        </button>
      </div>
    </div>
  )
}

/** O Access deixou entrar, mas a caixa só aceita os emails de MAILBOX_ALLOWED_EMAILS. */
function NotAllowed({ email }: { email: string }) {
  return (
    <div role="alertdialog" aria-modal="true" aria-labelledby="denied-title" className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
      <div className="w-full max-w-sm rounded-xl border border-border bg-card p-6 text-card-foreground shadow-xl">
        <h2 id="denied-title" className="text-base font-semibold">Este email não tem acesso a esta caixa</h2>
        <p className="mt-2 text-sm text-muted-foreground">
          {email ? (
            <>
              Você entrou como <strong className="font-medium text-foreground">{email}</strong>, que não está na lista de
              emails permitidos.
            </>
          ) : (
            'O email com que você entrou não está na lista de emails permitidos.'
          )}{' '}
          Entre com a conta autorizada ou peça para incluir esse email na configuração da caixa.
        </p>
        <a
          autoFocus
          href="/cdn-cgi/access/logout"
          className="mt-5 block w-full rounded-lg bg-primary px-3 py-2 text-center text-sm font-medium text-primary-foreground hover:opacity-90"
        >
          Entrar com outra conta
        </a>
      </div>
    </div>
  )
}

export default Providers
