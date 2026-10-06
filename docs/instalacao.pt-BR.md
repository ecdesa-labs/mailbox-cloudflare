# Guia de instalação

🌐 [English](installation.en.md) · [Español](instalacion.es.md) · [Voltar ao README](../README.pt-BR.md)

Este guia leva você do zero até uma caixa de email funcionando no seu próprio
domínio, rodando na sua conta da Cloudflare. Ele não supõe que você conheça o
projeto: cada passo diz **o que fazer**, **onde** (no painel da Cloudflare ou
no terminal) e **por quê**.

Os passos estão em ordem. Alguns dependem dos anteriores, então siga a
sequência.

## Antes de começar

**O que você vai ter no fim**

- Um endereço como `https://mailbox.<seu-subdominio>.workers.dev`, protegido por
  login, onde você lê, organiza e (opcionalmente) envia emails.
- Endereços `algo@seudominio.com` que entregam nessa caixa.

**Quanto custa**

| Para | Plano | Custo |
|---|---|---|
| Receber e ler emails | Workers Free | Grátis para volume pessoal |
| Também enviar e responder | Workers Paid | US$ 5/mês (inclui 3.000 envios/mês) |

O R2 (onde os emails ficam guardados) e o Zero Trust (o login) pedem um
cartão de crédito na ativação, mesmo nos planos gratuitos. Nada é cobrado
enquanto você estiver dentro das cotas grátis.

**Quanto tempo leva:** cerca de uma hora na primeira vez, a maior parte
esperando instalações e a primeira compilação.

**Atenção se o domínio já tem email:** ligar o recebimento troca os registros
MX do domínio. Se hoje você recebe email desse domínio em outro lugar (Google
Workspace, Zoho, a hospedagem do site…), esse lugar para de receber. Migre as
caixas antes, ou use um domínio que ainda não tem email.

---

## Parte 1: no painel da Cloudflare

Estas etapas são feitas por você no navegador, em
[dash.cloudflare.com](https://dash.cloudflare.com). Elas ativam os serviços
que o projeto usa; sem elas, os comandos das próximas partes falham.

### 1.1 Crie a conta e ative a verificação em duas etapas

Crie uma conta em [dash.cloudflare.com](https://dash.cloudflare.com/sign-up)
(ou entre na sua). Em **Perfil → Autenticação**, ative a verificação em duas
etapas.

> **Por quê:** essa conta vai guardar todos os seus emails, e o login dela
> também protege a caixa (Parte 4).

### 1.2 Coloque o domínio na Cloudflare

No painel, clique em **Adicionar → Domínio existente** (ou "Add a domain"),
digite o domínio e escolha o plano **Free**. A Cloudflare mostra dois
*nameservers* (algo como `ana.ns.cloudflare.com`). Entre no site onde você
comprou o domínio (Registro.br, GoDaddy, Hostinger…) e troque os nameservers
pelos da Cloudflare. A ativação leva de minutos a algumas horas; o painel
avisa quando o domínio fica **Ativo**.

> **Por quê:** o recebimento de email (Email Routing) só funciona com o DNS do
> domínio na Cloudflare.

Se o domínio já estava na Cloudflare, pule este passo.

### 1.3 Ative o R2

No menu, abra **Armazenamento e bancos de dados → R2 → Visão geral**
(*Storage & databases → R2 → Overview*) e conclua a ativação (o
*checkout* do plano do R2, que é gratuito até 10 GB).

> **Por quê:** o R2 guarda cada email original (`.eml`), os anexos e o HTML. É
> o "arquivo" da sua caixa. Sem ativá-lo, a criação do bucket falha.

### 1.4 Escolha o seu subdomínio `workers.dev`

Abra **Workers & Pages**. Se for a primeira vez, o painel pede para escolher
um subdomínio; senão, ao lado de **Your subdomain**, clique em **Change**.
Escolha um nome curto (ex.: o nome da sua empresa).

> **Por quê:** é o endereço onde a caixa vai ficar:
> `https://mailbox.<subdominio>.workers.dev`.

### 1.5 Ative o Zero Trust (o login da caixa)

No menu, abra **Zero Trust**. Na primeira vez, ele pede:

1. Um **nome de time** (*team name*): um identificador seu, como `minhaempresa`.
   Ele vira o endereço de login `minhaempresa.cloudflareaccess.com`.
2. Um plano: escolha **Free** (até 50 usuários). Ele pede um cartão, mas não
   cobra.

> **Por quê:** o Cloudflare Access (parte do Zero Trust) fica na frente da
> caixa e só deixa entrar quem você autorizar. O projeto recusa todo acesso
> enquanto ele não estiver configurado.

### 1.6 (Só se for enviar emails) Assine o Workers Paid

Em **Workers & Pages**, abra a opção de plano (**Plans** ou **Usage model**) e
escolha **Workers Paid**.

> **Por quê:** o envio de emails (Email Sending) para qualquer destinatário só
> existe no plano pago. Para só receber, o plano gratuito basta; você pode
> assinar depois.

---

## Parte 2: prepare o computador

Daqui em diante você vai usar o terminal (Linux ou macOS). No Windows, use o
[WSL](https://learn.microsoft.com/windows/wsl/install) com Ubuntu e siga as
instruções de Linux.

### 2.1 Instale as ferramentas

| Ferramenta | Para quê |
|---|---|
| git | baixar o projeto |
| Rust (rustup) | compilar o Worker, que é escrito em Rust |
| Node.js (versão LTS) | rodar o `wrangler`, a ferramenta oficial da Cloudflare, e compilar o front |
| Python 3 | os scripts de configuração |
| just | os atalhos de comando (`just deploy`, `just up`…) |
| pkg-config e OpenSSL (headers) | dependências de compilação |

**Ubuntu / Debian:**

```bash
sudo apt update
sudo apt install -y git python3 pkg-config libssl-dev build-essential curl just
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # Rust; aceite o padrão
```

Para o Node.js, use a versão LTS de [nodejs.org](https://nodejs.org) ou do
[nvm](https://github.com/nvm-sh/nvm). Se o seu Ubuntu não tiver o pacote
`just`, veja outras formas em [just.systems](https://just.systems/man/en/packages.html).

**macOS** (com [Homebrew](https://brew.sh)):

```bash
brew install git python node just pkg-config openssl
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Feche e abra o terminal depois de instalar o Rust.

### 2.2 Baixe o projeto e prepare a compilação

```bash
git clone <endereço do repositório> mailbox-cloudflare
cd mailbox-cloudflare
just setup    # instala o alvo WebAssembly do Rust e o worker-build (uma vez)
just doctor   # confere se falta alguma coisa
```

O `just doctor` lista cada ferramenta com `ok` ou `FALTA`, e diz como
resolver o que falta. Só siga quando estiver tudo `ok`.

> **Por quê:** o Worker roda como WebAssembly dentro da Cloudflare; o `setup`
> instala o que transforma o código Rust nesse formato.

Todos os comandos `just` a seguir são rodados nesta pasta
(`mailbox-cloudflare`).

---

## Parte 3: configure e publique

### 3.1 Diga ao projeto qual é o seu domínio

```bash
just configure --domain seudominio.com.br --label "Minha Empresa" --color '#2563EB'
```

- `--label`: o nome curto que aparece na barra lateral da caixa.
- `--color`: a cor do domínio na caixa (hexadecimal).
- Se você assinou o Workers Paid (1.6), acrescente `--plan paid`: com mais
  CPU e memória disponíveis, a caixa processa por completo emails bem maiores.

> **Por quê:** tudo o que é seu (domínio, nomes, ids) fica num arquivo local,
> `mailbox/instance.json`, que não vai para o git. Este comando cria esse
> arquivo e gera a configuração do Worker a partir dele.

### 3.2 Conecte o terminal à sua conta

```bash
just login
```

Abre o navegador para você autorizar o `wrangler` a mexer na sua conta. No fim,
o terminal mostra o email e o id da conta.

### 3.3 Crie os recursos na Cloudflare

```bash
just bootstrap
```

Cria o bucket do R2 (os arquivos), o banco D1 (o índice: pastas, threads,
busca) e as duas filas (processamento e falhas), e grava o id do banco na sua
configuração. Rodar de novo não recria nada.

### 3.4 Publique

```bash
just deploy
```

Roda os testes, compila o front e o Worker, cria as tabelas no banco e
publica. A primeira vez leva alguns minutos. No fim aparece:

```
ok: https://mailbox.<subdominio>.workers.dev está no ar
```

Se você abrir esse endereço agora, a caixa ainda não deixa entrar: ela recusa
tudo até o login estar configurado (Parte 4). Isso é proposital.

> **Atalho:** `just up --domain ... --label ... --color ...` faz 3.1 a 3.4 de
> uma vez. Os passos separados ajudam a entender e a achar o problema se algo
> falhar.

---

## Parte 4: proteja a caixa com login

### 4.1 Ligue o Access no Worker (painel)

1. Abra **Workers & Pages** e clique no Worker **mailbox**.
2. Abra a aba **Domains**.
3. Na linha do `workers.dev`, clique em **Enable Cloudflare Access**. O
   painel cria uma política que libera os membros da sua conta da
   Cloudflare (você). Para liberar outras pessoas ou trocar o jeito de
   entrar, ajuste depois em **Zero Trust → Access**.
4. A janela que aparece mostra dois valores. **Copie os dois:**
   - o **team domain** (`https://minhaempresa.cloudflareaccess.com`);
   - o **AUD** (*Application Audience tag*, um texto longo em hexadecimal).

### 4.2 Passe esses valores para o projeto e publique de novo

```bash
just configure --access-team minhaempresa.cloudflareaccess.com --access-aud <AUD copiado> --allowed-email <email da sua conta Cloudflare>
just deploy
```

**Qual email vai em `--allowed-email`:** o mesmo com que você entra no painel
da Cloudflare (o email da conta). É ele que o Access informa à caixa depois do
login. Para conferir, rode `just login`: o terminal mostra *"logged in …
associated with the email …"*. No painel, ele também aparece no menu do perfil
(canto superior direito).

- É opcional: sem ele, vale só a política do Access. Com ele, a caixa recusa
  qualquer login que não seja desse email, mesmo que a política do Access
  deixe passar. É uma segunda trava.
- Se o email não bater com o da conta, **você fica de fora**: o login do
  Access funciona, mas a caixa mostra "Este email não tem acesso a esta
  caixa", com o email com que você entrou. Corrija com o `just configure`
  acima e `just deploy`.
- Se um dia liberar outra pessoa na política do Access, inclua o email dela
  também, repetindo a opção: `--allowed-email voce@… --allowed-email ela@…`.

> **Por quê:** o Access pede o login antes de chegar à caixa, e o Worker
> confere de novo, sozinho, se o login é válido e veio do seu Access. Se uma
> das duas pontas não estiver certa, nada abre.

### 4.3 Entre na caixa

Abra `https://mailbox.<subdominio>.workers.dev`. A Cloudflare pede para você
se identificar (com a conta da Cloudflare, pela política criada em 4.1).
Depois disso, a caixa aparece, ainda vazia.

---

## Parte 5: receba emails

### 5.1 Ligue o recebimento no domínio

```bash
just email-enable
```

Liga o Email Routing e troca os registros MX do domínio pelos da Cloudflare.
(No painel, o equivalente é **Compute → Email Service → Email Routing →
Onboard Domain**.)

Se o comando for recusado por causa de um **Null MX** (`MX 0 .`) ou de um SPF
`v=spf1 -all` (configuração de "domínio que não usa email"), apague esse MX e
troque esse SPF por `v=spf1 include:_spf.mx.cloudflare.net ~all` em **DNS**,
no painel do domínio, e rode de novo.

### 5.2 Crie o primeiro endereço

```bash
just email-address contato@seudominio.com.br "Minha Empresa"
```

Cria duas coisas: a **rota**, que manda os emails desse endereço para a sua
caixa, e a **identidade**, que faz o endereço aparecer no campo **De** quando
você escreve. O texto entre aspas é o nome que os destinatários veem.

> **Por quê:** o domínio fica fechado. Só entra email para endereços que você
> criou, o que evita spam em endereços inventados (`admin@`, `vendas@`…).
> Repita o comando para cada endereço novo.
>
> **Alternativa:** `just email-catchall` faz *qualquer* endereço do domínio
> chegar na caixa, sem criar um por um. É mais prático, mas qualquer
> endereço inventado por spammers também passa a entrar.

### 5.3 Teste

De outro email (Gmail, por exemplo), mande uma mensagem para o endereço que
você criou. Em até um minuto ela aparece na caixa de entrada.

---

## Parte 6: envie emails (opcional, plano pago)

Requer o Workers Paid (passo 1.6).

### 6.1 Verifique o domínio para envio (painel)

1. Abra **Compute → Email Service → Email Sending**.
2. Clique em **Onboard Domain** e escolha o seu domínio.
3. A Cloudflare cria sozinha os registros de autenticação: DKIM
   (`cf-bounce._domainkey`) e SPF/MX do subdomínio `cf-bounce`.
4. Em **DNS**, confira se o registro `_dmarc` do domínio continua como você
   quer. Se não existir, um bom começo é `v=DMARC1; p=quarantine;`.

> **Por quê:** sem essas assinaturas, Gmail e Outlook tratam seus emails como
> suspeitos ou recusam. Com elas, os emails saem autenticados (SPF, DKIM e
> DMARC aprovados).

### 6.2 Se ainda não fez, ajuste o plano no projeto

```bash
just configure --plan paid
just deploy
```

### 6.3 Teste

Na caixa, clique em **Escrever**, escolha o seu endereço em **De** e mande
para o seu Gmail. No Gmail, abra a mensagem, clique nos três pontos e depois
em **Mostrar original**: SPF, DKIM e DMARC devem aparecer como **PASS**.
Responda do Gmail; a resposta cai na mesma conversa da caixa.

---

## Dia a dia

| Quero… | Faça |
|---|---|
| Um endereço novo (`compras@`, `github@`…) | `just email-address novo@seudominio.com.br "Nome"` |
| Atualizar o projeto | `git pull` e depois `just deploy` |
| Ver os atalhos de teclado | Na caixa, aperte `?` |
| Ver os logs em tempo real | `cd mailbox && npx wrangler tail` |
| Ver emails que falharam ao processar | `just failures` |
| Reprocessar tudo (ex.: depois de uma atualização) | `just reprocess --all` |

Lixeira e spam são apagados de vez 30 dias depois de irem para lá.

## Problemas comuns

| Sintoma | Causa provável e solução |
|---|---|
| `just bootstrap` falha ao criar o bucket | O R2 não foi ativado (passo 1.3). |
| A caixa abre, mas mostra erro ao carregar (e `/api/me` responde "autenticação não configurada") | Falta o passo 4.2 (team e AUD), ou o deploy depois dele. |
| Logo depois de entrar, a caixa mostra "Este email não tem acesso a esta caixa" | O `--allowed-email` não é o email da conta Cloudflare (veja 4.2). O aviso mostra com que email você entrou. |
| O login do Access não aparece | O Access não foi ligado no Worker (passo 4.1). |
| Email enviado para o domínio não chega | O endereço não tem rota: rode `just email-address` para ele. Confira com `just email-status`. |
| `email-enable` recusado | Null MX ou SPF `-all` no domínio (veja 5.1). |
| Envio recusado com `E_SENDER_NOT_VERIFIED` | O domínio não foi verificado no Email Sending (passo 6.1). |
| Envio recusado em geral | Confira se o plano é Workers Paid (passo 1.6). |
| `just deploy` falha com código 10089 | As métricas foram ligadas (`--metrics on`) sem ativar o Analytics Engine no painel. Ative-o ou rode `just configure --metrics off`. |
| `just deploy` falha com código 7403 | Costuma ser passageiro. Confira a sessão com `cd mailbox && npx wrangler whoami` e rode de novo. |
