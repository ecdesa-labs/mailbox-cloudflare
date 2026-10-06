'use client'

import { MutationCache, QueryCache, QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { ThemeProvider } from 'next-themes'
import { useState } from 'react'
import { ApiRequestError } from '@/lib/api/types'

const isUnauthorized = (e: unknown) => e instanceof ApiRequestError && e.status === 401

export function Providers({ children }: { children: React.ReactNode }) {
  // Qualquer 401 (consulta ou mutação) abre um único aviso por cima do app.
  const [expired, setExpired] = useState(false)
  const [queryClient] = useState(
    () =>
      new QueryClient({
        queryCache: new QueryCache({ onError: (e) => isUnauthorized(e) && setExpired(true) }),
        mutationCache: new MutationCache({ onError: (e) => isUnauthorized(e) && setExpired(true) }),
        defaultOptions: {
          queries: {
            staleTime: 30_000,
            refetchOnWindowFocus: false,
            // Só erro de rede ou 5xx, no máximo 2 vezes. 4xx não melhora repetindo.
            retry: (failures, e) =>
              failures < 2 && (!(e instanceof ApiRequestError) || e.status >= 500),
          },
        },
      }),
  )
  return (
    <QueryClientProvider client={queryClient}>
      <ThemeProvider attribute="class" defaultTheme="system" enableSystem storageKey="mailbox-theme" disableTransitionOnChange>
        {children}
        {expired && <SessionExpired />}
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

export default Providers
