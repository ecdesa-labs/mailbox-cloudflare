# Installation guide

🌐 [Português (Brasil)](instalacao.pt-BR.md) · [Español](instalacion.es.md) · [Back to the README](../README.en.md)

This guide takes you from zero to a working mailbox on your own domain,
running on your own Cloudflare account. It doesn't assume you know the
project: each step says **what to do**, **where** (the Cloudflare dashboard or
the terminal) and **why**.

The steps are in order. Some depend on earlier ones, so follow the sequence.

## Before you start

**What you'll have at the end**

- An address like `https://mailbox.<your-subdomain>.workers.dev`, behind a
  login, where you read, organize and (optionally) send email.
- Addresses like `something@yourdomain.com` that deliver into that mailbox.

**What it costs**

| To | Plan | Cost |
|---|---|---|
| Receive and read email | Workers Free | Free at personal volume |
| Also send and reply | Workers Paid | US$ 5/month (includes 3,000 emails/month) |

R2 (where the email is stored) and Zero Trust (the login) ask for a credit
card when you enable them, even on the free plans. Nothing is charged while
you stay within the free quotas.

**How long it takes:** about an hour the first time, mostly waiting for
installs and the first build.

**If the domain already has email:** turning on receiving replaces the
domain's MX records. If you currently receive email for this domain somewhere
else (Google Workspace, Zoho, your web host…), that place stops receiving it.
Migrate those mailboxes first, or use a domain that has no email yet.

---

## Part 1: in the Cloudflare dashboard

You do these steps in the browser, at
[dash.cloudflare.com](https://dash.cloudflare.com). They enable the services
the project uses; without them, the commands in the next parts fail.

### 1.1 Create the account and turn on two-factor authentication

Create an account at [dash.cloudflare.com](https://dash.cloudflare.com/sign-up)
(or log in). Under **Profile → Authentication**, turn on two-factor
authentication.

> **Why:** this account will hold all your email, and its login also guards
> the mailbox (Part 4).

### 1.2 Put the domain on Cloudflare

In the dashboard, click **Add → Existing domain** (or "Add a domain"), type the
domain and choose the **Free** plan. Cloudflare shows two *nameservers*
(something like `ana.ns.cloudflare.com`). Go to the site where you bought the
domain (Namecheap, GoDaddy, your registrar…) and replace the nameservers with
Cloudflare's. Activation takes from minutes to a few hours; the dashboard
tells you when the domain is **Active**.

> **Why:** receiving email (Email Routing) only works when the domain's DNS is
> on Cloudflare.

If the domain was already on Cloudflare, skip this step.

### 1.3 Enable R2

In the menu, open **Storage & databases → R2 → Overview** and complete the
activation (the R2 plan checkout, which is free up to 10 GB).

> **Why:** R2 stores every original email (`.eml`), attachments and HTML. It's
> your mailbox's archive. Without enabling it, creating the bucket fails.

### 1.4 Choose your `workers.dev` subdomain

Open **Workers & Pages**. The first time, the dashboard asks you to pick a
subdomain; otherwise, next to **Your subdomain**, click **Change**. Pick a
short name (e.g. your company's name).

> **Why:** it's where the mailbox will live:
> `https://mailbox.<subdomain>.workers.dev`.

### 1.5 Enable Zero Trust (the mailbox login)

In the menu, open **Zero Trust**. The first time, it asks for:

1. A **team name**: an identifier of yours, like `mycompany`. It becomes the
   login address `mycompany.cloudflareaccess.com`.
2. A plan: choose **Free** (up to 50 users). It asks for a card but doesn't
   charge it.

> **Why:** Cloudflare Access (part of Zero Trust) sits in front of the mailbox
> and only lets in whoever you allow. The project refuses all access until
> it's configured.

### 1.6 (Only if you'll send email) Subscribe to Workers Paid

In **Workers & Pages**, open the plan option (**Plans** or **Usage model**)
and choose **Workers Paid**.

> **Why:** sending email (Email Sending) to any recipient only exists on the
> paid plan. To only receive, the free plan is enough; you can subscribe
> later.

---

## Part 2: prepare your computer

From here on you'll use the terminal (Linux or macOS). On Windows, use
[WSL](https://learn.microsoft.com/windows/wsl/install) with Ubuntu and follow
the Linux instructions.

### 2.1 Install the tools

| Tool | What for |
|---|---|
| git | download the project |
| Rust (rustup) | build the Worker, which is written in Rust |
| Node.js (LTS) | run `wrangler`, Cloudflare's official tool, and build the web app |
| Python 3 | the configuration scripts |
| just | the command shortcuts (`just deploy`, `just up`…) |
| pkg-config and OpenSSL headers | build dependencies |

**Ubuntu / Debian:**

```bash
sudo apt update
sudo apt install -y git python3 pkg-config libssl-dev build-essential curl just
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # Rust; accept the defaults
```

For Node.js, use the LTS version from [nodejs.org](https://nodejs.org) or
[nvm](https://github.com/nvm-sh/nvm). If your Ubuntu has no `just` package,
see other options at [just.systems](https://just.systems/man/en/packages.html).

**macOS** (with [Homebrew](https://brew.sh)):

```bash
brew install git python node just pkg-config openssl
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Close and reopen the terminal after installing Rust.

### 2.2 Download the project and prepare the build

```bash
git clone <repository address> mailbox-cloudflare
cd mailbox-cloudflare
just setup    # installs Rust's WebAssembly target and worker-build (once)
just doctor   # checks that nothing is missing
```

`just doctor` lists each tool as `ok` or `FALTA` (missing) and says how to fix
what's missing. Only move on when everything is `ok`.

> **Why:** the Worker runs as WebAssembly inside Cloudflare; `setup` installs
> what turns the Rust code into that format.

Run every `just` command below from this folder (`mailbox-cloudflare`).

---

## Part 3: configure and deploy

### 3.1 Tell the project which domain is yours

```bash
just configure --domain yourdomain.com --label "My Company" --color '#2563EB'
```

- `--label`: the short name shown in the mailbox sidebar.
- `--color`: the domain's color in the mailbox (hex).
- If you subscribed to Workers Paid (1.6), add `--plan paid`: with more CPU
  and memory available, the mailbox fully processes much larger emails.

> **Why:** everything that is yours (domain, names, ids) lives in a local
> file, `mailbox/instance.json`, which is not committed. This command creates
> that file and generates the Worker's configuration from it.

### 3.2 Connect the terminal to your account

```bash
just login
```

Opens the browser so you can authorize `wrangler` to manage your account. At
the end, the terminal shows the account's email and id.

### 3.3 Create the resources on Cloudflare

```bash
just bootstrap
```

Creates the R2 bucket (the files), the D1 database (the index: folders,
threads, search) and the two queues (processing and failures), and saves the
database id to your configuration. Running it again recreates nothing.

### 3.4 Deploy

```bash
just deploy
```

Runs the tests, builds the web app and the Worker, creates the database
tables and publishes. The first time takes a few minutes. At the end you see:

```
ok: https://mailbox.<subdomain>.workers.dev está no ar
```

If you open that address now, the mailbox won't let you in yet: it refuses
everything until the login is configured (Part 4). That's on purpose.

> **Shortcut:** `just up --domain ... --label ... --color ...` runs 3.1 to 3.4
> in one go. The separate steps help you understand and find the problem if
> something fails.

---

## Part 4: protect the mailbox with a login

### 4.1 Turn on Access for the Worker (dashboard)

1. Open **Workers & Pages** and click the **mailbox** Worker.
2. Open the **Domains** tab.
3. On the `workers.dev` row, click **Enable Cloudflare Access**. The dashboard
   creates a policy that lets in the members of your Cloudflare account (you).
   To let other people in or change how they sign in, adjust it later in
   **Zero Trust → Access**.
4. The dialog that appears shows two values. **Copy both:**
   - the **team domain** (`https://mycompany.cloudflareaccess.com`);
   - the **AUD** (*Application Audience tag*, a long hexadecimal string).

### 4.2 Give those values to the project and deploy again

```bash
just configure --access-team mycompany.cloudflareaccess.com --access-aud <copied AUD> --allowed-email <your Cloudflare account email>
just deploy
```

**Which email goes in `--allowed-email`:** the one you use to log in to the
Cloudflare dashboard (the account email). That's the email Access hands to
the mailbox after login. To check it, run `just login`: the terminal shows
*"logged in … associated with the email …"*. In the dashboard, it also
appears in the profile menu (top right corner).

- It's optional: without it, only the Access policy applies. With it, the
  mailbox refuses any login that isn't that email, even if the Access policy
  lets it through. It's a second lock.
- If the email doesn't match the account's, **you're locked out**: the Access
  login works, but the mailbox shows "Este email não tem acesso a esta caixa"
  (this email has no access to this mailbox), with the email you signed in
  with. Fix it with the `just configure` above and `just deploy`.
- If you ever let someone else in through the Access policy, add their email
  too by repeating the option: `--allowed-email you@… --allowed-email them@…`.

> **Why:** Access asks for the login before anyone reaches the mailbox, and
> the Worker independently checks that the login is valid and came from your
> Access. If either side is wrong, nothing opens.

### 4.3 Open the mailbox

Open `https://mailbox.<subdomain>.workers.dev`. Cloudflare asks you to identify
yourself (with your Cloudflare account, per the policy created in 4.1). After
that, the mailbox shows up, still empty.

---

## Part 5: receive email

### 5.1 Turn on receiving for the domain

```bash
just email-enable
```

Turns on Email Routing and replaces the domain's MX records with Cloudflare's.
(In the dashboard, the equivalent is **Compute → Email Service → Email Routing
→ Onboard Domain**.)

If the command is refused because of a **Null MX** (`MX 0 .`) or an SPF of
`v=spf1 -all` (a "domain that doesn't use email" setup), delete that MX and
change that SPF to `v=spf1 include:_spf.mx.cloudflare.net ~all` under **DNS**
in the domain's dashboard, then run it again.

### 5.2 Create the first address

```bash
just email-address contact@yourdomain.com "My Company"
```

Creates two things: the **route**, which sends that address's email to your
mailbox, and the **identity**, which makes the address show up in the
**From** field when you write. The quoted text is the name recipients see.

> **Why:** the domain stays closed. Only addresses you created receive email,
> which avoids spam to made-up addresses (`admin@`, `sales@`…). Run the
> command again for each new address.
>
> **Alternative:** `just email-catchall` makes *any* address on the domain
> reach the mailbox, without creating them one by one. It's more convenient,
> but any address spammers make up also gets in.

### 5.3 Test it

From another email account (Gmail, for example), send a message to the
address you created. Within a minute it shows up in the inbox.

---

## Part 6: send email (optional, paid plan)

Requires Workers Paid (step 1.6).

### 6.1 Verify the domain for sending (dashboard)

1. Open **Compute → Email Service → Email Sending**.
2. Click **Onboard Domain** and choose your domain.
3. Cloudflare creates the authentication records on its own: DKIM
   (`cf-bounce._domainkey`) and SPF/MX for the `cf-bounce` subdomain.
4. Under **DNS**, check that the domain's `_dmarc` record is still what you
   want. If there is none, a good start is `v=DMARC1; p=quarantine;`.

> **Why:** without these signatures, Gmail and Outlook treat your email as
> suspicious or refuse it. With them, your email goes out authenticated (SPF,
> DKIM and DMARC pass).

### 6.2 If you haven't yet, set the plan in the project

```bash
just configure --plan paid
just deploy
```

### 6.3 Test it

In the mailbox, click **Escrever** (Compose), pick your address under **De**
(From) and send it to your Gmail. In Gmail, open the message, click the three
dots, then **Show original**: SPF, DKIM and DMARC should show **PASS**. Reply
from Gmail; the reply lands in the same conversation in the mailbox.

---

## Day to day

| I want to… | Do |
|---|---|
| A new address (`purchases@`, `github@`…) | `just email-address new@yourdomain.com "Name"` |
| Update the project | `git pull`, then `just deploy` |
| See the keyboard shortcuts | In the mailbox, press `?` |
| Watch the logs live | `cd mailbox && npx wrangler tail` |
| See emails that failed to process | `just failures` |
| Reprocess everything (e.g. after an update) | `just reprocess --all` |

Trash and spam are deleted for good 30 days after landing there.

## Common problems

| Symptom | Likely cause and fix |
|---|---|
| `just bootstrap` fails to create the bucket | R2 wasn't enabled (step 1.3). |
| The mailbox opens but shows a loading error (and `/api/me` says "autenticação não configurada") | Step 4.2 is missing (team and AUD), or the deploy after it. |
| Right after signing in, the mailbox shows "Este email não tem acesso a esta caixa" (no access) | `--allowed-email` isn't your Cloudflare account email (see 4.2). The notice shows which email you signed in with. |
| The Access login doesn't show up | Access wasn't turned on for the Worker (step 4.1). |
| Email sent to the domain doesn't arrive | The address has no route: run `just email-address` for it. Check with `just email-status`. |
| `email-enable` is refused | Null MX or SPF `-all` on the domain (see 5.1). |
| Sending refused with `E_SENDER_NOT_VERIFIED` | The domain wasn't verified in Email Sending (step 6.1). |
| Sending refused in general | Check that the plan is Workers Paid (step 1.6). |
| `just deploy` fails with code 10089 | Metrics were turned on (`--metrics on`) without enabling Analytics Engine in the dashboard. Enable it or run `just configure --metrics off`. |
| `just deploy` fails with code 7403 | Usually transient. Check the session with `cd mailbox && npx wrangler whoami` and run it again. |

The mailbox interface is currently in Portuguese (Brazil).
