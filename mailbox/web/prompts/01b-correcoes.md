# v0 · Fase 1 do front: correções da revisão

> Cole no mesmo chat do v0 que gerou a fase 1, só o que está abaixo da linha.

---

Revisei o código da fase 1. Os tipos em `lib/api/types.ts` estão certos (não
mexa neles). Corrija os pontos abaixo, na ordem. São requisitos do prompt
original que ficaram de fora.

## Bloqueadores

1. **Export estático.** Em `next.config.mjs`, adicione `output: 'export'` e
   **remova `typescript.ignoreBuildErrors`**. O build precisa gerar `out/` e
   falhar se houver erro de tipo. Corrija os erros de tipo que aparecerem.
2. **Mock só quando pedido.** Em `lib/api/client.ts`, o mock está ligado por
   padrão (`NEXT_PUBLIC_MOCK !== '0'`). Inverta: mock **apenas** quando
   `process.env.NEXT_PUBLIC_MOCK === '1'`. Sem a variável, o cliente fala com a
   API real.
3. **Sem analytics de terceiros.** Remova `@vercel/analytics` do
   `package.json` e o `<Analytics />` do `layout.tsx`. É uma caixa de email
   privada, servida pela Cloudflare (não pela Vercel); nenhum script de
   terceiros pode carregar nela.
4. **Estado na URL.** Pasta, domínio e mensagem selecionada estão em
   `useState`. Leve para a query string (`?folder=&domain=&id=`), lendo com
   `useSearchParams` e escrevendo com `router.push` (`router.replace` para
   trocar de mensagem dentro da mesma lista). Recarregar a página e o botão
   voltar do navegador precisam manter o que estava aberto. Lembre de envolver
   o componente que usa `useSearchParams` em `<Suspense>` (exigência do export
   estático).

## Importantes

5. **Um só layout, com Tailwind + shadcn.** Hoje existem duas versões do shell:
   `app/page.tsx` (CSS próprio em `globals.css`, tudo numa linha) e
   `components/mail-shell.tsx` (Tailwind, não usado). Fique só com uma, em
   Tailwind + componentes shadcn, dividida em componentes legíveis
   (`Sidebar`, `MessageList`, `MessageRow`, `ReaderHeader`, estados). Apague o
   CSS próprio do `globals.css` (fica só tema/tokens) e o arquivo não usado.
6. **Um só `QueryClient`.** Use o `Providers` de `app/providers.tsx` no
   `layout.tsx` e remova o `new QueryClient()` de `page.tsx`. Não repita
   requisições que falharam com 4xx (`retry` só para erro de rede e 5xx, no
   máximo 2 vezes).
7. **Tema.** Padrão = tema do sistema (`prefers-color-scheme`); a alternância
   manual (claro / escuro / sistema) fica salva em `localStorage` e é aplicada
   antes da pintura (sem piscar o tema errado ao carregar). Pode usar
   `next-themes`.
8. **Sessão expirada.** O cliente já detecta 401, mas a tela mostra o erro
   genérico. Qualquer 401 deve mostrar o aviso "Sua sessão expirou" com o botão
   "Entrar de novo" (`window.location.reload()`), uma vez, por cima do app.
9. **Erro tipado de verdade.** Troque o `throw { status, ...data }` por uma
   classe `class ApiRequestError extends Error { status; code; }`. Se a
   resposta de erro não for JSON (ex.: página HTML de 502), não quebre no
   `res.json()`: use `code: 'unavailable'` e uma mensagem genérica.
10. **Sem `any`.** `createRule`, `patchRule` e o `Reader` usam `any`. Adicione
    em `types.ts`:

    ```ts
    export interface RuleInput { match_address: string; action: RuleAction;
      folder?: Folder; priority?: number; enabled?: boolean }
    export type RulePatch = Partial<RuleInput>;
    ```

    e tipe o `Reader` com `MessageDetail`.

## Mock fiel à API

11. Sem `q`, a API devolve **uma pasta** (padrão `inbox`); com `q` e sem
    `folder`, **todas as pastas**. Hoje o mock sem `folder` devolve tudo; faça
    igual à API.
12. Os contadores `unread` dos domínios devem ser **calculados** a partir do
    estado em memória (não lidos na entrada daquele domínio), para refletir
    as mutações.
13. `getMessage` de uma mensagem sem detalhe nas fixtures deve devolver
    `raw_url: "/api/messages/<id>/raw"` (não string vazia), e o mock de
    `createRule` deve gerar um id no formato ULID.

## Detalhes

14. Na barra lateral, cada domínio mostra só "Entrada". Dentro de cada seção de
    domínio, mostre as pastas daquele domínio (Entrada com contador, Arquivo,
    Lixeira, Spam, Enviados), recolhíveis.
15. Formate datas com o idioma do navegador (`navigator.language`, `pt-BR` como
    reserva), não fixo em `pt-BR`.
16. Item ativo da lista e da barra lateral com `aria-current`; atalho de teclado
    para ir da lista ao leitor e voltar (setas sobem/descem na lista, `Enter`
    abre, `Esc` volta).
17. Limpeza: remova `const _unused = null`, `generator: 'v0.app'` do metadata,
    as imagens `placeholder*` não usadas de `public/` e renomeie o pacote de
    `my-project` para `mailbox-web`.

Ao terminar, confirme que `npm run build` gera a pasta `out/` sem erros de tipo
e que `NEXT_PUBLIC_MOCK=1 npm run dev` continua funcionando com as fixtures.
