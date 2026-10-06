'use client'

import { Suspense } from 'react'
import { MailApp } from '@/components/mail/mail-app'

// `useSearchParams` precisa de Suspense no export estático.
export default function Page() {
  return (
    <Suspense>
      <MailApp />
    </Suspense>
  )
}
