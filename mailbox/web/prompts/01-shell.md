# v0 · Fase 1 do front: shell, design system e `lib/api`

> Cole tudo abaixo da linha no v0.dev. Os tipos vêm de
> `mailbox/worker/openapi.json`; se a API mudar, atualize-os aqui antes.

---

Crie o front de uma **caixa de email pessoal** (um único usuário, vários
domínios, caixa unificada com visão por domínio). Nesta fase quero só o
**shell**, o **design system** e a camada **`lib/api`** com modo mock. Lista
completa, leitor, threads e ações vêm em fases seguintes; aqui elas aparecem
como estrutura e placeholders.

## Regras fixas (não negociáveis)

1. **Next.js com `output: 'export'`** (site estático servido pelo mesmo Worker
   da API). Sem Server Actions, sem route handlers, sem `next/image` remoto, sem
   middleware, sem `cookies()`/`headers()`.
2. **Uma única página em `/`.** O estado de navegação fica na URL como query
   string: `?folder=inbox&domain=exemplo.com.br&id=<id da mensagem>`. Nada de
   rotas dinâmicas (`/m/[id]`): o servidor devolve o `index.html` para qualquer
   caminho, e rota dinâmica num export estático quebraria.
3. **Todo dado vem de `lib/api/`**: cliente tipado, URL base **relativa**
   (`/api`), `fetch` com `credentials: 'same-origin'`. **Modo mock** com as
   fixtures abaixo quando `NEXT_PUBLIC_MOCK=1` (mesmas funções, mesmos tipos,
   com uma latência artificial de 300–600 ms para os estados de carregando
   aparecerem).
4. **TanStack Query** para cache, e `useInfiniteQuery` para a lista paginada por
   cursor (`next_cursor`; `null` = acabou).
5. **Tailwind + shadcn/ui.** Tema claro e escuro (padrão: o do sistema, com
   alternância manual que fica salva).
6. **Os tipos TypeScript abaixo são o contrato.** Não invente campos, não
   renomeie, não mude `null` para `undefined`.
7. **Mutações** (`PATCH`, `POST`, `DELETE`) sempre com
   `Content-Type: application/json` e na mesma origem: a API recusa o resto
   (proteção CSRF). Já deixe isso no cliente, mesmo sem tela que use ainda.
8. **Autenticação é da borda (Cloudflare Access):** o front não tem login. Se a
   API responder **401**, mostre um aviso "Sua sessão expirou" com o botão
   "Entrar de novo" que faz `window.location.reload()`.
9. Datas chegam em **Unix ms** (UTC). Mostre no fuso e no idioma do navegador
   (`pt-BR` como padrão): hoje = só a hora; este ano = dia e mês; antes = data
   completa.
10. Interface em **português do Brasil**.

## Contrato (tipos)

```ts
// lib/api/types.ts — gerado de openapi.json. Não editar à mão.
export type Id = string; // ULID, 26 caracteres
export type UnixMs = number;
export type Folder = "inbox" | "archive" | "trash" | "spam" | "sent";

export interface Person { name: string | null; addr: string }

export interface Me { email: string }

export interface Identity { id: Id; address: string; label: string | null; is_default_from: boolean }

export interface Domain {
  id: Id;
  name: string;      // "exemplo.com.br"
  label: string;     // nome curto exibido
  color: string;     // cor do domínio, ex.: "#2563EB"
  unread: number;    // não lidos na caixa de entrada
  identities: Identity[];
}

export interface ListItem {
  id: Id;
  thread_id: string | null;
  domain: string;              // nome do domínio
  identity: string | null;     // endereço pelo qual o email entrou
  from: Person | null;
  subject: string | null;
  snippet: string | null;
  received_at: UnixMs;
  is_read: boolean;
  is_starred: boolean;
  has_attachments: boolean;
  folder: Folder;
  processing: "full" | "partial"; // partial = email grande: sem corpo; oferecer o .eml
}

export interface Page { items: ListItem[]; next_cursor: string | null }

export interface Attachment {
  id: Id; filename: string | null; content_type: string; size: number;
  content_id: string | null; is_inline: boolean; url: string;
}

export interface MessageDetail extends ListItem {
  to: Person[];
  cc: Person[];
  date: UnixMs | null;          // header Date
  message_id: string | null;
  envelope_to: string;
  size: number;
  text: string | null;
  html: string | null;          // já sanitizado no servidor
  has_remote_images: boolean;
  auth: { spf: string | null; dkim: string | null; dmarc: string | null };
  attachments: Attachment[];
  raw_url: string;              // .eml original
}

export interface Thread { id: Id; subject: string | null; messages: ListItem[] }

export interface MessagePatch { read?: boolean | null; starred?: boolean | null; folder?: Folder | null }

export type RuleAction = "move" | "archive" | "mark_read" | "reject";
export interface Rule {
  id: Id; priority: number; match_address: string; action: RuleAction;
  folder: Folder | null; enabled: boolean;
}

export interface ApiError {
  error: {
    code: "bad_request" | "unauthorized" | "forbidden" | "not_found"
        | "unsupported_media_type" | "internal" | "unavailable";
    message: string;
  };
}
```

Endpoints que o cliente deve expor (assinaturas já prontas, mesmo os que esta
fase ainda não usa):

```
GET    /api/me                               -> Me
GET    /api/domains                          -> { items: Domain[] }
GET    /api/messages?folder=&domain=&q=&cursor=&limit=  -> Page
         (sem q: uma pasta, padrão inbox; com q e sem folder: todas as pastas)
GET    /api/messages/:id?remote_images=0|1   -> MessageDetail
GET    /api/threads/:id                      -> Thread
PATCH  /api/messages/:id   (MessagePatch)    -> ListItem
POST   /api/messages/bulk  ({ ids: Id[], patch: MessagePatch }) -> { updated: number }
GET    /api/rules                            -> { items: Rule[] }
POST   /api/rules          ({ match_address, action, folder?, priority?, enabled? }) -> Rule (201)
PATCH  /api/rules/:id      (campos parciais) -> Rule
DELETE /api/rules/:id                        -> 204
```

Erros: corpo `ApiError`. O cliente lança um erro tipado com `status`, `code` e
`message`, que as telas usam para os estados de erro.

## O que construir nesta fase

### Layout de 3 painéis (desktop)

- **Barra lateral** (largura fixa, recolhível):
  - Topo: nome do produto e o email de `/api/me`.
  - **Caixa unificada**: Entrada (com total de não lidos de todos os domínios),
    Arquivo, Lixeira, Spam, Enviados.
  - **Uma seção por domínio**, com um ponto ou faixa na **cor do domínio**
    (`Domain.color`), o `label` e o contador de não lidos; dentro dela, as
    pastas daquele domínio. Escolher um domínio filtra a lista (`domain=`).
  - Rodapé: link "Regras" (placeholder) e alternância de tema.
- **Lista** (painel do meio): cabeçalho com o nome da pasta/domínio e um campo
  de busca (só visual nesta fase). Itens usando `ListItem` com: remetente (nome
  ou endereço), assunto, snippet, data relativa, marca de não lido (negrito +
  ponto), estrela, clipe de anexo, **etiqueta com a cor do domínio** quando
  estiver na caixa unificada, e uma etiqueta discreta "grande" quando
  `processing === "partial"`. Rolagem infinita já ligada ao `useInfiniteQuery`.
- **Leitor** (painel da direita): por enquanto só cabeçalho (assunto,
  remetente, para, data) e um placeholder para o corpo. Vazio quando não há `id`
  na URL ("Selecione uma mensagem").

### Estados (em todos os painéis)

- **Carregando**: skeletons no formato do conteúdo.
- **Vazio**: mensagem amigável por pasta ("Nenhuma mensagem na lixeira").
- **Erro**: mensagem + botão "Tentar de novo"; 401 vira o aviso de sessão.

### Design system

- Tokens em CSS variables (cores, raio, espaçamento), tema claro e escuro.
- Neutro e legível, denso como um cliente de email (linhas de ~64 px na lista).
- A cor de cada domínio vem da API em tempo de execução: use-a como
  `style={{ '--domain': domain.color }}` e nunca como classe Tailwind gerada.
- Acessível: foco visível, navegação por teclado entre os painéis, contraste AA
  nos dois temas, `aria-label` nos botões de ícone.

### Responsivo

- Abaixo de 1024 px: lista e leitor alternam (um painel por vez, com "voltar").
- Abaixo de 768 px: barra lateral vira gaveta.

## Fixtures do modo mock

`lib/api/fixtures.ts` com **dois domínios** e pelo menos estas mensagens
(distribuídas entre os dois, com datas de hoje, desta semana e de meses atrás,
o bastante para ter 2 páginas com `limit=20`):

```ts
// IDs fictícios no formato ULID (26 caracteres), gerados por número.
const fakeId = (n: number): Id => "01" + String(n).padStart(24, "0");

export const domains: Domain[] = [
  { id: fakeId(1), name: "exemplo.com.br", label: "Exemplo",
    color: "#2563EB", unread: 3,
    identities: [
      { id: fakeId(101), address: "contato@exemplo.com.br", label: "Contato", is_default_from: true },
      { id: fakeId(102), address: "financeiro@exemplo.com.br", label: null, is_default_from: false },
    ] },
  { id: fakeId(2), name: "pessoal.dev", label: "Pessoal",
    color: "#E5484D", unread: 1,
    identities: [
      { id: fakeId(201), address: "github@pessoal.dev", label: "GitHub", is_default_from: false },
    ] },
];
// Mensagens, anexos e threads também usam fakeId (ex.: fakeId(1000 + i)).
```

Mensagens obrigatórias (gere o resto com variações realistas em português):

1. **Texto puro**, não lido: "Orçamento de outubro", de "João da Silva
   <joao@cliente.com.br>", para `contato@exemplo.com.br`; `auth` todo `"pass"`.
2. **Resposta na mesma thread** (mesmo `thread_id`): "Re: Orçamento de outubro".
3. **HTML com imagem inline**: "Newsletter com logo", com
   `html` contendo `<img src="/api/attachments/<id>">` e um anexo `image/png`
   `is_inline: true`.
4. **Com imagens remotas bloqueadas**: `has_remote_images: true` e
   `html` com `<img data-remote-src="https://...">`.
5. **Anexo grande**: "Planilha do trimestre", com um PDF de 18 MB nos
   `attachments`.
6. **Modo leve**: `processing: "partial"`, `text: null`, `html: null`,
   `has_attachments: false`, assunto "Backup completo (anexos)".
7. **Com estrela**, já lida, em `pessoal.dev`: "[pessoal/mailbox] CI passou",
   de "GitHub <noreply@github.com>", para `github@pessoal.dev`.
8. Uma em cada pasta: `archive`, `trash`, `spam` (spam com `auth.spf: "fail"`).
9. Uma com `from: null` e `subject: null` (exibir "(sem remetente)" e
   "(sem assunto)").

No modo mock, o filtro por `folder`/`domain` e a paginação por cursor devem
funcionar de verdade sobre as fixtures, e as mutações devem alterar o estado em
memória (para as próximas fases).

## Fora desta fase

Leitor com corpo/HTML/anexos, threads, ações (marcar, mover, lote), atalhos de
teclado, busca funcionando, tela de regras e compor. Deixe os pontos de
extensão óbvios, mas não implemente.

## Entregue

- `lib/api/{types.ts,client.ts,fixtures.ts,mock.ts,queries.ts}`
- Componentes do shell, da lista e do cabeçalho do leitor
- `next.config` com `output: 'export'` e `images: { unoptimized: true }`
- Um `README` curto: `NEXT_PUBLIC_MOCK=1 npm run dev` para rodar com fixtures
  e `npm run build` gerando `out/`.
