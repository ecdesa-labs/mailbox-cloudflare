# mailbox-cloudflare

🌐 [English](README.en.md)

Caixa de email pessoal que roda inteira na Cloudflare: recebe, guarda, indexa,
envia e responde emails de um ou mais domínios seus, sem servidor de email
próprio e sem provedor pago de caixa postal.

> **Estado:** em desenvolvimento. Recebe, processa e indexa emails de
> verdade (R2 + D1, com threads e busca), tem API e front para ler,
> organizar, escrever e responder (veja [Roadmap](#roadmap)).

> Projeto independente, sem vínculo com a Cloudflare, Inc. "Cloudflare" é
> marca da Cloudflare, Inc. e aparece no nome só para indicar a plataforma.

## Proposta

- **Uso próprio.** Um único dono, vários domínios, caixa unificada com visão por domínio.
- **Só Cloudflare.** Email Routing entrega cada mensagem a um Worker; R2 guarda os
  arquivos; D1 guarda o índice; Access protege o acesso; Email Sending envia.
  Receber cabe no plano gratuito para volume pessoal; enviar exige o plano
  Workers Paid.
- **O `.eml` bruto é a fonte da verdade.** Tudo o que está no banco é derivado
  dele e pode ser reprocessado a qualquer momento.
- **Reutilizável.** Domínios, cores e nomes dos recursos ficam num único arquivo
  de instância. Qualquer pessoa pode subir o próprio mailbox com um comando.

## Como funciona

```
Email → Email Routing (rota por endereço ou catch-all)
          → Worker [email]  grava o .eml no R2 e enfileira
          → Queue mail-ingest (+ DLQ)
          → Worker [queue]  parseia, grava anexos e HTML sanitizado no R2, indexa no D1

Browser → Cloudflare Access → Worker [fetch]
            ├─ /api/*  → D1 / R2
            └─ /*      → front estático (Workers Assets)
```

Um único Worker, escrito em Rust ([workers-rs](https://github.com/cloudflare/workers-rs)),
com três handlers:

| Handler | Papel |
|---|---|
| `email` | Recebe do Email Routing. Só grava o `.eml` e enfileira: rápido e sem parsing. |
| `queue` | Faz o trabalho pesado: MIME, anexos, sanitização de HTML, threads, regras. Falhas repetidas vão para a DLQ; o `.eml` continua no R2 para reprocessar. |
| `fetch` | API JSON em `/api/*` e o front estático no resto, na mesma origem (sem CORS). |

**Armazenamento**

- **R2**: `{domínio}/raw/{aaaa}/{mm}/{id}.eml`, anexos em `{domínio}/att/…` e
  HTML sanitizado em `{domínio}/html/…`. O prefixo por domínio facilita retenção
  e remoção.
- **D1** (SQLite): domínios, identidades, threads, mensagens, anexos, regras de
  roteamento e busca por texto (FTS5, mantida por triggers).

**Segurança**

- HTML de email é tratado como entrada hostil: sanitizado no processamento,
  exibido em `<iframe sandbox>` sem scripts e com imagens remotas bloqueadas por
  padrão (evita rastreamento).
- Todo acesso passa pelo Cloudflare Access; o Worker valida o JWT.

## Como usar

**Para instalar, siga o [guia de instalação](docs/instalacao.pt-BR.md).** Ele
vai do zero (conta e domínio na Cloudflare) até a caixa recebendo e enviando,
em ordem, com o que fazer no painel da Cloudflare e no terminal, e o porquê de
cada passo.

O resumo abaixo é referência para quem já instalou.

### Vários domínios

`--label` e `--color` seguem a ordem dos `--domain`:

```bash
just configure \
  --domain exemplo.com.br --label Exemplo --color '#2563EB' \
  --domain outro.dev      --label Outro   --color '#E5484D'
```

Depois que o seed já foi aplicado num D1, domínio novo entra por uma migração nova.

### Nomes dos recursos

Por padrão: Worker e R2 `mailbox`, D1 `mailbox`, filas `mail-ingest` e
`mail-ingest-dlq`. Para ter mais de uma instância na mesma conta, troque os nomes:

```bash
just configure --worker caixa --bucket caixa --database caixa --queue caixa-in --dlq caixa-dlq
```

Todas as opções: `just configure --help`.

### Comandos

| Comando | O que faz |
|---|---|
| `just up [args]` | Tudo, de ponta a ponta |
| `just doctor` | Confere as ferramentas do host |
| `just configure [args]` | Configura a instância |
| `just login` | Login na Cloudflare |
| `just bootstrap` | Cria os recursos remotos que faltam |
| `just ci` | fmt, clippy, testes, regra de dependência, build wasm, checagem da instância |
| `just migrate-local` / `just migrate-remote` | Migrações do D1 |
| `just deploy` | CI + migração remota + publicação + teste de fumaça |
| `just dev` | Sobe local com `wrangler dev` (D1, R2 e filas simulados) |
| `just contract` | Testes de contrato da API com Worker, D1, R2 e filas locais |
| `just front-dev` | Front local com fixtures (sem API), em `localhost:3000` |
| `just front-build` | Gera o front estático em `mailbox/web/out` (o `deploy` já faz) |
| `just email-enable` | Liga o Email Routing nos domínios da instância (substitui os MX) |
| `just email-route <endereço>` | Manda um endereço específico para o Worker |
| `just email-address <endereço> ["nome"]` | Rota + identidade: libera um endereço novo para receber e enviar |
| `just email-catchall` | Manda todos os endereços dos domínios para o Worker |
| `just email-status` | Estado do Email Routing e das regras |
| `just reprocess --key <raw_key> \| --failed \| --all` | Pede reprocessamento (o cron executa em até 10 min) |
| `just failures` | Falhas de processamento e pedidos pendentes |

## A instância

Tudo o que muda de uma pessoa para outra fica em `mailbox/instance.json`,
que **não vai para o git**: tem o id do seu D1, o time e a aplicação do Access e
os seus domínios. O modelo versionado é
[`mailbox/instance.example.json`](mailbox/instance.example.json):

```json
{
  "worker": "mailbox",
  "bucket": "mailbox",
  "database": "mailbox",
  "database_id": "00000000-0000-0000-0000-000000000000",
  "queue": "mail-ingest",
  "dlq": "mail-ingest-dlq",
  "access_team": "",
  "access_aud": "",
  "allowed_emails": [],
  "metrics": false,
  "plan": "free",
  "domains": [
    { "id": "01M…", "name": "exemplo.com.br", "label": "Exemplo", "color": "#2563EB" }
  ]
}
```

Não precisa copiar à mão: o primeiro `just configure --domain ...` cria o seu.

O [`scripts/setup_instance.py`](scripts/setup_instance.py) atualiza esse arquivo
e gera, a partir de [`mailbox/templates/`](mailbox/templates):

- `mailbox/wrangler.toml`
- `mailbox/migrations/0002_seed_domains.sql`
- `mailbox/instance.env` (lido pelos `justfile`s)

Os três também ficam fora do git. Não edite à mão: o `just ci` falha se eles
divergirem do `instance.json`. Para mudar algo no `wrangler.toml`, edite o
template. Nenhum deles guarda segredo (token de API fica no login do
wrangler, fora do projeto), mas identificam a sua conta.

## Operação

- **Retenção:** lixeira e spam são apagados de vez (com o `.eml`) 30 dias depois
  de irem para lá; mensagens ocultas por regra `reject`, 30 dias depois de
  ocultadas. Ajuste em `[vars]` do template (`MAILBOX_RETENTION_*_DAYS`; `0` =
  nunca). Roda no mesmo cron da reconciliação.
- **Logs:** todo evento sai em JSON com `level` e `event` (`ingest`, `process`,
  `send`, `dlq`, `reconcile`, `retention`, `auth`, `api_error`). Veja em **Workers &
  Pages → mailbox → Logs** ou com `npx wrangler tail`.
- **Métricas (opcional):** Analytics Engine, dataset `<worker>_events`. Vêm
  desligadas: a conta precisa ativar o Analytics Engine no painel antes (senão o
  deploy falha com o código 10089). Depois, `just configure --metrics on` e
  `just deploy`. Cada ponto tem
  `index1` = evento, `blob1..3` = evento, resultado e domínio, `double1` =
  tamanho em bytes e `double2` = contagem. Exemplo (SQL API do Analytics Engine):

  ```sql
  SELECT blob1 AS evento, blob2 AS resultado, SUM(_sample_interval * double2) AS total
  FROM mailbox_events WHERE timestamp > NOW() - INTERVAL '1' DAY
  GROUP BY evento, resultado ORDER BY total DESC
  ```

- **API:** a especificação OpenAPI 3.1 fica em `/api/openapi.json` (fonte:
  [`mailbox/worker/openapi.json`](mailbox/worker/openapi.json)). O `just contract`
  confere que ela cobre todas as rotas e que toda resposta bate com o schema.

## Estrutura do repositório

```
mailbox-cloudflare/
  justfile              fluxo de ponta a ponta
  docs/                 guia de instalação (pt-BR e en)
  scripts/              setup_instance.py, email_address.py, contract_test.py e testes
  mailbox/              o produto
    crates/
      domain/           entidades, value objects e regras; puro, testado nativo
      application/      casos de uso e portas (traits)
      mime/             .eml → mensagem parseada, sanitização de HTML; puro
      adapters-cf/      D1, R2, Queue, Mailer; só wasm32
    worker/             os três handlers e a montagem dos adaptadores
    migrations/         SQL do D1
    templates/          base dos arquivos gerados pela instância
    web/                front (Next.js estático; prompts do v0 em web/prompts/)
    instance.example.json  modelo da instância (o seu instance.json fica fora do git)
    docs/               notas locais (fora do git)
  mailbox-spike/        prova de conceito que validou a plataforma
```

Arquitetura hexagonal: `domain` ← `application` ← `adapters-cf` ← `worker`.
`domain`, `application` e `mime` não dependem do `worker` (o `just ci` verifica)
e por isso são testados nativamente, sem Cloudflare.

## Roadmap

| Fase | Entrega | Estado |
|---|---|---|
| 0 | Scaffold: workspace, wrangler, CI | ✅ |
| 1 | Domínio, migrações, value objects; configuração da instância | ✅ |
| 2 | Crate `mime`: parsing, sanitização, `cid:` e imagens remotas | ✅ |
| 3 | Handler `email`: receber de verdade | ✅ |
| 4 | Handler `queue`: processar, idempotente, DLQ, modo leve, reconciliação | ✅ |
| 5 | API de leitura com paginação por cursor | ✅ |
| 6 | Autenticação com Cloudflare Access | ✅ |
| 7 | Busca, flags, pastas, ações em lote, regras de roteamento | ✅ |
| 8 | Responder e enviar (Cloudflare Email Sending) | ✅ |
| 9 | Retenção, métricas, logs estruturados, OpenAPI | ✅ |

### Front

Next.js estático em `mailbox/web`, servido pelo mesmo Worker.

| Fase | Entrega | Estado |
|---|---|---|
| 1 | Shell, design system, `lib/api` com modo mock | ✅ |
| 2 | Lista: busca, seleção em lote, ações com desfazer | ✅ |
| 3 | Leitor: HTML isolado, imagens remotas sob demanda, anexos | ✅ |
| 4 | Threads: conversa agrupada, mensagens recolhíveis | ✅ |
| 5 | Atalhos de teclado e ajuda (`?`) | ✅ |
| 6 | Compor e responder | ✅ |
| 7 | Tela de regras de roteamento | |
| 8 | Polimento para celular | |

### Melhorias futuras

- [x] **Ligar as métricas no Analytics Engine** (como definido na Fase 9).
  O produto precisa ser ativado na conta antes do primeiro deploy com o
  binding (senão: código 10089). A ativação pode levar alguns minutos para
  valer; depois, `just configure --metrics on` e `just deploy`.

- [ ] **Charsets asiáticos multibyte** (Shift_JIS, ISO-2022-JP, GB2312, GBK,
  Big5, EUC-KR). Hoje viram texto com caracteres de substituição e a mensagem
  sai com `has_encoding_problem`. Caminho: ligar o recurso `full_encoding` do
  `mail-parser` (puxa o `encoding_rs`), medir o aumento do `.wasm` e adicionar
  goldens com esses charsets.
- [ ] **CSS em blocos `<style>`.** Hoje o `<style>` é removido inteiro (o
  `ammonia` não sanitiza folhas de estilo), então emails de marketing perdem o
  layout responsivo; os estilos inline (`style=""`) são mantidos. Caminho:
  sanitizar a folha com `cssparser` (mesma lista de propriedades sem `url()`,
  sem `@import`, `@font-face` nem seletores que escapem do email) ou converter
  as regras em estilos inline no processamento e passar pelo filtro atual.
  Nos dois casos, CSS não pode carregar nada de fora.

## Limitações conhecidas

- Email Routing aceita mensagens de até 25 MiB.
- Email Sending (beta público): até 5 MiB por mensagem e 50 destinatários
  (Para + Cc); a cota diária começa conservadora e cresce com o uso. O
  `Message-ID` de quem recebe é gerado pela Cloudflare; a cópia em Enviados
  usa o id que o serviço devolve. Envio ainda sem anexos.
- Receber e gravar não depende do tamanho (o `.eml` vai por stream para o
  R2). Processar sim: emails acima de um limiar vão para o **modo leve**
  (remetente, assunto e data; o `.eml` fica para download), e emails com
  anexos demais têm o texto indexado mas anexos e HTML só no `.eml`. Os
  limiares seguem o plano da instância (`just configure --plan free|paid`):

  | | Free | Paid |
  |---|---|---|
  | Modo leve acima de | 512 KiB (10 ms de CPU) | 16 MiB (128 MB de memória) |
  | Máximo de anexos processados | 30 | 200 |
  | Reconciliação por execução | 50 | 500 |
  | Retenção por execução | 50 | 500 |

  No Paid, o pico de memória do wasm medido foi ~2,5x o tamanho do `.eml`; o
  evento `process` traz `wasm_mib` para recalibrar.
- No plano Free, mensagens da fila e da DLQ expiram em 24 h; o reprocessamento
  sempre parte do `.eml` no R2.
- `wrangler d1 export` não exporta tabelas virtuais (a busca FTS5). Para
  exportar o banco, apague `messages_fts`, exporte e recrie. O backup real é o
  bucket R2 com os `.eml`.
