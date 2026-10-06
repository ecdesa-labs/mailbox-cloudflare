# mailbox-cloudflare

🌐 [Português (Brasil)](README.pt-BR.md)

A personal mailbox that runs entirely on Cloudflare: it receives, stores,
indexes, sends and replies to email for one or more of your domains, with no
mail server of your own and no paid mailbox provider.

> **Status:** in development. It receives, processes and indexes real email
> (R2 + D1, with threads and search), and has an API and a web app to read,
> organize, write and reply (see [Roadmap](#roadmap)).

> Independent project, not affiliated with Cloudflare, Inc. "Cloudflare" is a
> trademark of Cloudflare, Inc. and appears in the name only to indicate the
> platform.

## Goals

- **Single owner.** One person, several domains, a unified inbox with a per-domain view.
- **Cloudflare only.** Email Routing delivers each message to a Worker; R2 stores
  the files; D1 stores the index; Access guards access; Email Sending sends.
  Receiving fits the free plan at personal volume; sending requires the
  Workers Paid plan.
- **The raw `.eml` is the source of truth.** Everything in the database is
  derived from it and can be reprocessed at any time.
- **Reusable.** Domains, colors and resource names live in a single instance
  file. Anyone can deploy their own mailbox with one command.

## How it works

```
Email → Email Routing (per-address route or catch-all)
          → Worker [email]  stores the .eml in R2 and enqueues it
          → Queue mail-ingest (+ DLQ)
          → Worker [queue]  parses, stores attachments and sanitized HTML in R2, indexes in D1

Browser → Cloudflare Access → Worker [fetch]
            ├─ /api/*  → D1 / R2
            └─ /*      → static web app (Workers Assets)
```

A single Worker, written in Rust ([workers-rs](https://github.com/cloudflare/workers-rs)),
with three handlers:

| Handler | Role |
|---|---|
| `email` | Receives from Email Routing. Only stores the `.eml` and enqueues it: fast, no parsing. |
| `queue` | Does the heavy lifting: MIME, attachments, HTML sanitization, threads, rules. Repeated failures go to the DLQ; the `.eml` stays in R2 for reprocessing. |
| `fetch` | JSON API under `/api/*` and the static web app everywhere else, on the same origin (no CORS). |

**Storage**

- **R2**: `{domain}/raw/{yyyy}/{mm}/{id}.eml`, attachments under `{domain}/att/…`
  and sanitized HTML under `{domain}/html/…`. The per-domain prefix makes
  retention and removal easy.
- **D1** (SQLite): domains, identities, threads, messages, attachments, routing
  rules and full-text search (FTS5, kept up to date by triggers).

**Security**

- Email HTML is treated as hostile input: sanitized during processing, shown in
  an `<iframe sandbox>` without scripts, and with remote images blocked by
  default (no tracking).
- All access goes through Cloudflare Access; the Worker validates the JWT.

## Usage

### Requirements

- A Cloudflare account with the domain(s) already added.
- Linux or macOS with [Rust](https://rustup.rs), Node.js (for `npx wrangler`),
  Python 3, [just](https://github.com/casey/just) and, on Debian/Ubuntu:

  ```bash
  sudo apt install -y pkg-config libssl-dev
  ```

> **Warning:** enabling Email Routing on a domain **replaces its MX records**.
> If the domain already receives email at another provider, migrate those
> mailboxes first.

### From zero to deploy

```bash
just setup    # once: wasm32 target and worker-build
just doctor   # checks for missing tools
just up --domain example.com --label Example --color '#2563EB'
```

`just up` runs, in order:

1. **configure**: saves the instance and generates the configuration files;
2. **login**: opens the Cloudflare login if there is no session yet;
3. **bootstrap**: creates the R2 bucket, the D1 database and the queues that
   don't exist yet, and saves the D1 id;
4. **deploy**: runs CI, applies the migrations to the remote D1, publishes the
   Worker and tests `GET /api/ping`.

Running it again is safe: nothing is recreated. Without arguments, `just up`
uses the instance that is already configured.

### Receiving email

After deploying, enable Email Routing and validate with one address before
sending the whole domain to the Worker:

```bash
just email-enable                    # replaces the domain's MX records
just email-route test@example.com    # only this address goes to the Worker
just email-catchall                  # after validating: every address
```

If the domain has a **Null MX** (`MX 0 .`) or an SPF of `v=spf1 -all` (a
"domain without email" setup), `email-enable` is refused: remove the Null MX
and change the SPF to `v=spf1 include:_spf.mx.cloudflare.net ~all` in the
dashboard first.

### Sending email

Requires the **Workers Paid** plan (US$ 5/month, with 3,000 emails per month
included). The Worker already ships with the `send_email` binding (`MAILER`);
what's left is verifying the domain in **Email Sending**:

1. In the dashboard: **Compute → Email Service → Email Sending → Onboard
   Domain** and pick the domain. Cloudflare creates the DKIM record
   (`cf-bounce._domainkey`) and the SPF/MX records for the `cf-bounce`
   subdomain (the envelope and bounce domain).
2. Check that the domain's DMARC is still what you want (e.g. `p=reject`):
   DKIM signs as the domain itself, so alignment passes.

The sender is always an identity of the domain. An identity is created when an
address receives its first email, or with:

```bash
just email-address new@example.com "Display name"
```

which creates the address's route in Email Routing (so replies arrive) and the
identity (so it shows up in **From**). Without a catch-all, only addresses
with a route receive email: run this command before sending from a new
address. Running it again is safe; the name only changes if you pass one. A
reply is sent from the identity that received the original and never from
another domain. A copy of what was sent is kept in R2 and in **Sent**, in the
same thread.

Without a verified domain, sending is refused (`422`, code
`E_SENDER_NOT_VERIFIED`).

### Protecting with Cloudflare Access

The API only answers requests that went through Access; until it's configured,
it denies everything.

1. In the dashboard: **Workers & Pages → mailbox → Access → Protect this Worker
   behind Access**. Choose the policy (e.g. only your Cloudflare account).
2. In **Zero Trust → Access → Applications**, open the application that was
   created and copy the **team domain** and the **Application Audience (AUD)
   Tag**.
3. Save them to the instance and deploy:

   ```bash
   just configure --access-team yourteam.cloudflareaccess.com --access-aud <AUD> --allowed-email you@example.com
   just deploy
   ```

4. Open `https://<worker>.<account>.workers.dev/api/me` in the browser: after
   logging in, it shows your email.

To develop locally without Access, copy `mailbox/.dev.vars.example` to
`mailbox/.dev.vars` (only works on `localhost`).

### Several domains

`--label` and `--color` follow the order of the `--domain` flags:

```bash
just configure \
  --domain example.com --label Example --color '#2563EB' \
  --domain other.dev   --label Other   --color '#E5484D'
```

Once the seed has been applied to a D1 database, a new domain comes in through
a new migration.

### Resource names

Defaults: Worker and R2 `mailbox`, D1 `mailbox`, queues `mail-ingest` and
`mail-ingest-dlq`. To run more than one instance in the same account, change
the names:

```bash
just configure --worker inbox --bucket inbox --database inbox --queue inbox-in --dlq inbox-dlq
```

All options: `just configure --help`.

### Commands

| Command | What it does |
|---|---|
| `just up [args]` | Everything, end to end |
| `just doctor` | Checks the host's tools |
| `just configure [args]` | Configures the instance |
| `just login` | Logs in to Cloudflare |
| `just bootstrap` | Creates the missing remote resources |
| `just ci` | fmt, clippy, tests, dependency rule, wasm build, instance check |
| `just migrate-local` / `just migrate-remote` | D1 migrations |
| `just deploy` | CI + remote migration + publish + smoke test |
| `just dev` | Runs locally with `wrangler dev` (simulated D1, R2 and queues) |
| `just contract` | API contract tests against a local Worker, D1, R2 and queues |
| `just front-dev` | Web app locally with fixtures (no API), on `localhost:3000` |
| `just front-build` | Builds the static web app into `mailbox/web/out` (`deploy` already does it) |
| `just email-enable` | Enables Email Routing on the instance's domains (replaces MX) |
| `just email-route <address>` | Sends one specific address to the Worker |
| `just email-address <address> ["name"]` | Route + identity: enables a new address to receive and send |
| `just email-catchall` | Sends every address of the domains to the Worker |
| `just email-status` | Email Routing status and rules |
| `just reprocess --key <raw_key> \| --failed \| --all` | Requests reprocessing (the cron runs it within 10 min) |
| `just failures` | Processing failures and pending requests |

## The instance

Everything that changes from one person to another lives in
`mailbox/instance.json`, which **is not committed**: it holds your D1 id, your
Access team and application, and your domains. The versioned template is
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
    { "id": "01M…", "name": "example.com", "label": "Example", "color": "#2563EB" }
  ]
}
```

No need to copy it by hand: the first `just configure --domain ...` creates
yours.

[`scripts/setup_instance.py`](scripts/setup_instance.py) updates that file and
generates, from [`mailbox/templates/`](mailbox/templates):

- `mailbox/wrangler.toml`
- `mailbox/migrations/0002_seed_domains.sql`
- `mailbox/instance.env` (read by the `justfile`s)

These three are not committed either. Don't edit them by hand: `just ci` fails
if they drift from `instance.json`. To change something in `wrangler.toml`,
edit the template. None of them holds secrets (the API token lives in the
wrangler login, outside the project), but they do identify your account.

## Operations

- **Retention:** trash and spam are deleted for good (including the `.eml`) 30
  days after landing there; messages hidden by a `reject` rule, 30 days after
  being hidden. Adjust in the template's `[vars]`
  (`MAILBOX_RETENTION_*_DAYS`; `0` = never). Runs in the same cron as
  reconciliation.
- **Logs:** every event is JSON with `level` and `event` (`ingest`, `process`,
  `send`, `dlq`, `reconcile`, `retention`, `auth`, `api_error`). See them in
  **Workers & Pages → mailbox → Logs** or with `npx wrangler tail`.
- **Metrics (optional):** Analytics Engine, dataset `<worker>_events`. Off by
  default: the account must enable Analytics Engine in the dashboard first
  (otherwise the deploy fails with code 10089). Then run `just configure
  --metrics on` and `just deploy`. Each data point has `index1` = event,
  `blob1..3` = event, outcome and domain, `double1` = size in bytes and
  `double2` = count. Example (Analytics Engine SQL API):

  ```sql
  SELECT blob1 AS event, blob2 AS outcome, SUM(_sample_interval * double2) AS total
  FROM mailbox_events WHERE timestamp > NOW() - INTERVAL '1' DAY
  GROUP BY event, outcome ORDER BY total DESC
  ```

- **API:** the OpenAPI 3.1 spec is served at `/api/openapi.json` (source:
  [`mailbox/worker/openapi.json`](mailbox/worker/openapi.json)). `just contract`
  checks that it covers every route and that every response matches the
  schema.

## Repository layout

```
mailbox-cloudflare/
  justfile              end-to-end workflow
  scripts/              setup_instance.py, email_address.py, contract_test.py and tests
  mailbox/              the product
    crates/
      domain/           entities, value objects and rules; pure, tested natively
      application/      use cases and ports (traits)
      mime/             .eml → parsed message, HTML sanitization; pure
      adapters-cf/      D1, R2, Queue, Mailer; wasm32 only
    worker/             the three handlers and adapter wiring
    migrations/         D1 SQL
    templates/          base for the files generated from the instance
    web/                web app (static Next.js; v0 prompts in web/prompts/)
    instance.example.json  instance template (your instance.json is not committed)
    docs/               local notes (not committed)
  mailbox-spike/        proof of concept that validated the platform
```

Hexagonal architecture: `domain` ← `application` ← `adapters-cf` ← `worker`.
`domain`, `application` and `mime` don't depend on `worker` (`just ci` checks
it), so they are tested natively, without Cloudflare.

## Roadmap

| Phase | Deliverable | Status |
|---|---|---|
| 0 | Scaffold: workspace, wrangler, CI | ✅ |
| 1 | Domain, migrations, value objects; instance configuration | ✅ |
| 2 | `mime` crate: parsing, sanitization, `cid:` and remote images | ✅ |
| 3 | `email` handler: receiving for real | ✅ |
| 4 | `queue` handler: processing, idempotent, DLQ, light mode, reconciliation | ✅ |
| 5 | Read API with cursor pagination | ✅ |
| 6 | Authentication with Cloudflare Access | ✅ |
| 7 | Search, flags, folders, bulk actions, routing rules | ✅ |
| 8 | Reply and send (Cloudflare Email Sending) | ✅ |
| 9 | Retention, metrics, structured logs, OpenAPI | ✅ |

### Web app

Static Next.js in `mailbox/web`, served by the same Worker.

| Phase | Deliverable | Status |
|---|---|---|
| 1 | Shell, design system, `lib/api` with mock mode | ✅ |
| 2 | List: search, bulk selection, actions with undo | ✅ |
| 3 | Reader: isolated HTML, on-demand remote images, attachments | ✅ |
| 4 | Threads: grouped conversation, collapsible messages | ✅ |
| 5 | Keyboard shortcuts and help (`?`) | ✅ |
| 6 | Compose and reply | ✅ |
| 7 | Routing rules screen | |
| 8 | Mobile polish | |

### Future improvements

- [x] **Enable metrics in Analytics Engine** (as defined in Phase 9). The
  product must be enabled on the account before the first deploy with the
  binding (otherwise: code 10089). Activation may take a few minutes to take
  effect; then run `just configure --metrics on` and `just deploy`.

- [ ] **Multibyte Asian charsets** (Shift_JIS, ISO-2022-JP, GB2312, GBK, Big5,
  EUC-KR). Today they become text with replacement characters and the message
  is flagged `has_encoding_problem`. Path: enable `mail-parser`'s
  `full_encoding` feature (pulls in `encoding_rs`), measure the `.wasm` size
  increase and add goldens with those charsets.
- [ ] **CSS in `<style>` blocks.** Today `<style>` is removed entirely
  (`ammonia` doesn't sanitize stylesheets), so marketing emails lose their
  responsive layout; inline styles (`style=""`) are kept. Path: sanitize the
  stylesheet with `cssparser` (same property allowlist without `url()`, no
  `@import`, `@font-face` or selectors that escape the email) or inline the
  rules during processing and run them through the current filter. Either
  way, CSS must not load anything from outside.

## Known limitations

- Email Routing accepts messages up to 25 MiB.
- Email Sending (public beta): up to 5 MiB per message and 50 recipients
  (To + Cc); the daily quota starts conservative and grows with use. The
  recipient's `Message-ID` is generated by Cloudflare; the copy in Sent uses
  the id the service returns. Sending doesn't support attachments yet.
- Receiving and storing don't depend on size (the `.eml` is streamed to R2).
  Processing does: email above a threshold goes to **light mode** (sender,
  subject and date; the `.eml` is available for download), and email with too
  many attachments has its text indexed but attachments and HTML only in the
  `.eml`. The thresholds follow the instance's plan (`just configure --plan
  free|paid`):

  | | Free | Paid |
  |---|---|---|
  | Light mode above | 512 KiB (10 ms of CPU) | 16 MiB (128 MB of memory) |
  | Max attachments processed | 30 | 200 |
  | Reconciliation per run | 50 | 500 |
  | Retention per run | 50 | 500 |

  On Paid, the measured peak wasm memory was ~2.5x the `.eml` size; the
  `process` event includes `wasm_mib` for recalibration.
- On the Free plan, queue and DLQ messages expire after 24 h; reprocessing
  always starts from the `.eml` in R2.
- `wrangler d1 export` doesn't export virtual tables (the FTS5 search). To
  export the database, drop `messages_fts`, export and recreate it. The real
  backup is the R2 bucket with the `.eml` files.
