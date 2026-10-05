# Fluxo de ponta a ponta do mailbox: configurar a instância, criar os recursos
# na Cloudflare, testar, migrar o D1 e publicar o Worker.
#
#   just up --domain exemplo.com.br --label Exemplo --color '#2563EB'
#   just up          # sem args: usa o mailbox/instance.json atual
#
# Cada passo também roda sozinho (`just --list`). Rodar de novo é seguro:
# recursos que já existem não são recriados.

set shell := ["bash", "-euo", "pipefail", "-c"]
# Args chegam como "$@": preserva aspas (ex.: --color '#2563EB').
set positional-arguments

mailbox := justfile_directory() / "mailbox"
# Nomes lidos a cada chamada, porque `configure` pode ter acabado de mudá-los.
env := "set -a; source '" + mailbox / "instance.env" + "'; set +a"

default:
    @just --list --unsorted

# Tudo, na ordem. Args vão para o setup_instance.py (veja `just configure --help`).
up *ARGS: doctor
    just configure "$@"
    just login
    just bootstrap
    just deploy

# Confere as ferramentas do host
doctor:
    #!/usr/bin/env bash
    set -uo pipefail
    falta=0
    ok()   { printf '  ok     %s\n' "$1"; }
    erro() { printf '  FALTA  %s  →  %s\n' "$1" "$2"; falta=1; }
    for cmd in cargo rustup node npx python3 pkg-config; do
        command -v "$cmd" >/dev/null && ok "$cmd" || erro "$cmd" "instale $cmd"
    done
    pkg-config --exists openssl 2>/dev/null && ok "libssl-dev" \
        || erro "libssl-dev" "sudo apt install -y pkg-config libssl-dev"
    rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown && ok "wasm32" \
        || erro "wasm32-unknown-unknown" "just setup"
    command -v worker-build >/dev/null && ok "worker-build" || erro "worker-build" "just setup"
    exit $falta

# Instala target wasm32 e worker-build (uma vez)
setup:
    cd "{{ mailbox }}" && just setup

# Grava a instância e gera wrangler.toml, seed e instance.env
configure *ARGS:
    python3 scripts/setup_instance.py "$@"

# Login na Cloudflare (abre o navegador só se ainda não houver sessão)
login:
    cd "{{ mailbox }}" && (npx wrangler whoami --json >/dev/null 2>&1 || npx wrangler login)
    cd "{{ mailbox }}" && npx wrangler whoami

# Cria R2, D1 e filas que ainda não existem e grava o id real do D1
bootstrap:
    #!/usr/bin/env bash
    set -euo pipefail
    {{ env }}
    cd "{{ mailbox }}"
    w() { npx wrangler "$@"; }
    existe() { "$@" >/dev/null 2>&1; }

    if existe w r2 bucket info "$MAILBOX_BUCKET"; then echo "R2 $MAILBOX_BUCKET: já existe"
    else w r2 bucket create "$MAILBOX_BUCKET"; fi

    if existe w d1 info "$MAILBOX_DATABASE" --json; then echo "D1 $MAILBOX_DATABASE: já existe"
    else w d1 create "$MAILBOX_DATABASE"; fi

    for q in "$MAILBOX_QUEUE" "$MAILBOX_DLQ"; do
        if existe w queues info "$q"; then echo "fila $q: já existe"
        else w queues create "$q"; fi
    done

    id=$(w d1 list --json 2>/dev/null | python3 -c '
    import json, sys
    name = sys.argv[1]
    print(next(d["uuid"] for d in json.load(sys.stdin) if d["name"] == name))
    ' "$MAILBOX_DATABASE")
    if [ "$id" != "$MAILBOX_DATABASE_ID" ]; then
        cd "{{ justfile_directory() }}" && python3 scripts/setup_instance.py --database-id "$id"
    else
        echo "D1 id: $id"
    fi

# fmt, clippy, testes, regra de dependência, build wasm, --check da instância
ci:
    cd "{{ mailbox }}" && just ci

# Migrações no D1 local
migrate-local:
    cd "{{ mailbox }}" && just db-local

# Migrações no D1 remoto
migrate-remote:
    {{ env }} && cd "{{ mailbox }}" && npx wrangler d1 migrations apply "$MAILBOX_DATABASE" --remote

# Deploy exige o id real do D1 (o placeholder só serve para local)
_require-db-id:
    {{ env }} && [ "$MAILBOX_DATABASE_ID" != "00000000-0000-0000-0000-000000000000" ] \
        || { echo "D1 ainda com id placeholder: rode \`just bootstrap\` antes." >&2; exit 1; }

# CI + migração remota + publicação + teste de fumaça em /api/ping
deploy: _require-db-id ci migrate-remote
    #!/usr/bin/env bash
    set -euo pipefail
    {{ env }}
    cd "{{ mailbox }}"
    log=$(mktemp)
    npx wrangler deploy 2>&1 | tee "$log"
    url=$(grep -oE 'https://[a-zA-Z0-9.-]+\.workers\.dev' "$log" | head -1 || true)
    rm -f "$log"
    if [ -z "$url" ]; then
        echo "deploy feito, mas não achei a URL workers.dev na saída; pulei o teste de fumaça."; exit 0
    fi
    # Subdomínio novo pode levar alguns minutos para ter certificado.
    for i in $(seq 1 12); do
        if [ "$(curl -fsS "$url/api/ping" 2>/dev/null)" = "pong" ]; then
            echo "ok: $url/api/ping respondeu pong"; exit 0
        fi
        echo "aguardando $url ($i/12)..."; sleep 10
    done
    echo "deploy feito, mas $url/api/ping não respondeu pong em 2 min." >&2; exit 1

# Liga o Email Routing nos domínios da instância. ATENÇÃO: substitui os registros MX.
email-enable:
    #!/usr/bin/env bash
    set -euo pipefail
    {{ env }}
    cd "{{ mailbox }}"
    for d in ${MAILBOX_DOMAINS//,/ }; do npx wrangler email routing enable "$d"; done

# Manda um endereço específico para o Worker (validar antes do catch-all)
email-route ADDRESS:
    {{ env }} && cd "{{ mailbox }}" && npx wrangler email routing rules create "${1#*@}" \
        --name "mailbox: $1" --match-type literal --match-field to --match-value "$1" \
        --action-type worker --action-value "$MAILBOX_WORKER"

# Manda todo endereço dos domínios da instância para o Worker
email-catchall:
    #!/usr/bin/env bash
    set -euo pipefail
    {{ env }}
    cd "{{ mailbox }}"
    for d in ${MAILBOX_DOMAINS//,/ }; do
        npx wrangler email routing rules update "$d" catch-all --name "mailbox: catch-all" \
            --action-type worker --action-value "$MAILBOX_WORKER"
    done

# Estado do Email Routing e das regras
email-status:
    #!/usr/bin/env bash
    set -euo pipefail
    {{ env }}
    cd "{{ mailbox }}"
    for d in ${MAILBOX_DOMAINS//,/ }; do
        npx wrangler email routing settings "$d"
        npx wrangler email routing rules list "$d"
    done

# Sobe local com wrangler dev
dev:
    cd "{{ mailbox }}" && just dev
