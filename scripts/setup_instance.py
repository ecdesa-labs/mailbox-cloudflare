#!/usr/bin/env python3
"""Configura a instância do mailbox: domínios e nomes dos recursos na Cloudflare.

`mailbox/instance.json` é a fonte da verdade da instância (fora do git: tem IDs
da sua conta). O modelo versionado é `mailbox/instance.example.json`. Este script atualiza
o `instance.json` com os args e gera, a partir de `mailbox/templates/`, estes
arquivos dentro de `mailbox/`:

    wrangler.toml
    migrations/0002_seed_domains.sql
    instance.env                      (lido pelo justfile)

Exemplos (rodar da raiz do repositório):

    # Instância nova, um domínio
    python3 scripts/setup_instance.py --domain exemplo.com.br --label Exemplo --color '#2563EB'

    # Dois domínios: --label e --color seguem a ordem dos --domain
    python3 scripts/setup_instance.py \\
        --domain a.com --label A --color '#E8B923' \\
        --domain b.dev --label B --color '#E5484D'

    # Só gravar o id do D1 depois do `just remote-bootstrap`
    python3 scripts/setup_instance.py --database-id 0f2c...

    # CI: falha se algum arquivo gerado foi editado à mão
    python3 scripts/setup_instance.py --check

Só biblioteca padrão.
"""

from __future__ import annotations

import argparse
import json
import re
import secrets
import sys
import time
from pathlib import Path
from string import Template

# O script fica na raiz do repositório; a instância é do componente mailbox/.
ROOT = Path(__file__).resolve().parent.parent / "mailbox"
INSTANCE = "instance.json"

DEFAULTS = {
    "worker": "mailbox",
    "bucket": "mailbox",
    "database": "mailbox",
    # Placeholder: o D1 local aceita; deploy exige o id real.
    "database_id": "00000000-0000-0000-0000-000000000000",
    "queue": "mail-ingest",
    "dlq": "mail-ingest-dlq",
    # Cloudflare Access (docs seção 6.5). Vazio = Worker nega toda a API.
    "access_team": "",
    "access_aud": "",
    "allowed_emails": [],
    # Analytics Engine (Fase 9). Desligado por padrão: a conta precisa ativar
    # o produto no painel antes, senão o deploy falha (código 10089).
    "metrics": False,
}
RESOURCE_KEYS = ("worker", "bucket", "database", "queue", "dlq")

# Nomes de Worker, bucket R2, D1 e filas: minúsculas, dígitos e hífen, 3 a 63.
RESOURCE_RE = re.compile(r"^[a-z0-9][a-z0-9-]{1,61}[a-z0-9]$")
UUID_RE = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
LABEL_RE = re.compile(r"^(?!-)[a-z0-9-]{1,63}(?<!-)$")
COLOR_RE = re.compile(r"^#(?:[0-9A-Fa-f]{3}|[0-9A-Fa-f]{6})$")
ULID_RE = re.compile(r"^[0-7][0-9A-HJKMNP-TV-Z]{25}$")
ACCESS_TEAM_RE = re.compile(r"^[a-z0-9][a-z0-9-]*\.cloudflareaccess\.com$")
ACCESS_AUD_RE = re.compile(r"^[0-9a-f]{32,128}$")
EMAIL_RE = re.compile(r"^[^@\s,]+@[^@\s,]+\.[^@\s,]+$")
CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"

GENERATED = "GERADO por scripts/setup_instance.py a partir de {src}. Não edite aqui."


class ConfigError(Exception):
    pass


# --- validação -------------------------------------------------------------


def normalize_domain(name: str) -> str:
    """Mesmas regras do `DomainName` em crates/domain: ASCII minúsculo, punycode para IDN."""
    d = name.strip().rstrip(".").lower()
    labels = d.split(".")
    if not d or len(d) > 253 or len(labels) < 2 or not all(LABEL_RE.match(l) for l in labels):
        raise ConfigError(f"domínio inválido: {name!r} (IDN precisa estar em punycode, xn--)")
    return d


def validate(cfg: dict) -> None:
    for key in RESOURCE_KEYS:
        if not RESOURCE_RE.match(cfg[key]):
            raise ConfigError(
                f"{key} inválido: {cfg[key]!r} (minúsculas, dígitos e hífen, 3 a 63 caracteres)"
            )
    if cfg["queue"] == cfg["dlq"]:
        raise ConfigError("queue e dlq precisam ter nomes diferentes")
    if not UUID_RE.match(cfg["database_id"]):
        raise ConfigError(f"database_id inválido: {cfg['database_id']!r} (UUID)")
    if cfg["access_team"] and not ACCESS_TEAM_RE.match(cfg["access_team"]):
        raise ConfigError(f"access_team inválido: {cfg['access_team']!r} (ex.: acmeco.cloudflareaccess.com)")
    if cfg["access_aud"] and not ACCESS_AUD_RE.match(cfg["access_aud"]):
        raise ConfigError(f"access_aud inválido: {cfg['access_aud']!r} (Application Audience Tag, hexadecimal)")
    if not isinstance(cfg["metrics"], bool):
        raise ConfigError("metrics precisa ser true ou false")
    if bool(cfg["access_team"]) != bool(cfg["access_aud"]):
        raise ConfigError("access_team e access_aud andam juntos")
    for e in cfg["allowed_emails"]:
        if not EMAIL_RE.match(e):
            raise ConfigError(f"email inválido em allowed_emails: {e!r}")
    domains = cfg["domains"]
    if not domains:
        raise ConfigError("informe ao menos um --domain")
    names = [d["name"] for d in domains]
    if len(set(names)) != len(names):
        raise ConfigError("domínio repetido")
    for d in domains:
        if normalize_domain(d["name"]) != d["name"]:
            raise ConfigError(f"domínio não normalizado: {d['name']!r}")
        if not d["label"].strip():
            raise ConfigError(f"label vazio para {d['name']}")
        if not COLOR_RE.match(d["color"]):
            raise ConfigError(f"cor inválida para {d['name']}: {d['color']!r} (use #RGB ou #RRGGBB)")
        if not ULID_RE.match(d["id"]):
            raise ConfigError(f"id inválido para {d['name']}: {d['id']!r} (ULID)")


# --- ULID --------------------------------------------------------------------


def ulid(timestamp_ms: int, random80: int) -> str:
    value = (timestamp_ms << 80) | (random80 & ((1 << 80) - 1))
    out = []
    for _ in range(26):
        out.append(CROCKFORD[value & 31])
        value >>= 5
    return "".join(reversed(out))


def new_ulid() -> str:
    return ulid(int(time.time() * 1000), secrets.randbits(80))


# --- config ------------------------------------------------------------------


def load(root: Path) -> dict:
    path = root / INSTANCE
    if not path.exists():
        return {**DEFAULTS, "domains": []}
    return {**DEFAULTS, **json.loads(path.read_text(encoding="utf-8"))}



def apply_args(cfg: dict, args: argparse.Namespace) -> dict:
    cfg = {**cfg}
    for key in RESOURCE_KEYS:
        value = getattr(args, key)
        if value is not None:
            cfg[key] = value
    if args.database_id is not None:
        cfg["database_id"] = args.database_id.lower()
    if args.access_team is not None:
        cfg["access_team"] = args.access_team.strip().lower().removeprefix("https://").rstrip("/")
    if args.access_aud is not None:
        cfg["access_aud"] = args.access_aud.strip().lower()
    if args.metrics is not None:
        cfg["metrics"] = args.metrics == "on"
    if args.allowed_email:
        cfg["allowed_emails"] = [e.strip().lower() for e in args.allowed_email if e.strip()]

    if args.domain:
        for flag, values in (("--label", args.label), ("--color", args.color)):
            if len(values) != len(args.domain):
                raise ConfigError(
                    f"{flag} aparece {len(values)} vez(es); precisa de um por --domain ({len(args.domain)})"
                )
        # Reaproveita o id de domínio que já existe: a migração de seed
        # pode ter sido aplicada, e trocar o id quebraria as referências.
        known = {d["name"]: d["id"] for d in cfg["domains"]}
        domains = []
        for name, label, color in zip(args.domain, args.label, args.color):
            name = normalize_domain(name)
            domains.append(
                {"id": known.get(name) or new_ulid(), "name": name, "label": label.strip(), "color": color.upper()}
            )
        cfg["domains"] = domains
    elif args.label or args.color:
        raise ConfigError("--label e --color só valem junto com --domain")
    return cfg


# --- geração -----------------------------------------------------------------


def sql_str(s: str) -> str:
    return "'" + s.replace("'", "''") + "'"


def render(root: Path, cfg: dict) -> dict[Path, str]:
    tmpl = root / "templates"

    wrangler = Template((tmpl / "wrangler.toml.tmpl").read_text(encoding="utf-8")).substitute(
        {k: cfg[k] for k in (*RESOURCE_KEYS, "database_id", "access_team", "access_aud")},
        domains=",".join(d["name"] for d in cfg["domains"]),
        allowed_emails=",".join(cfg["allowed_emails"]),
        metrics_block=(
            "# Métricas (Fase 9). Free: 100 mil pontos por dia.\n"
            "[[analytics_engine_datasets]]\n"
            'binding = "METRICS"\n'
            f'dataset = "{cfg["worker"]}_events"\n\n'
            if cfg["metrics"]
            else "# Métricas desligadas (`just configure --metrics on` depois de ativar o\n"
            "# Analytics Engine no painel). Os logs estruturados funcionam sem elas.\n\n"
        ),
    )

    rows = ",\n".join(
        f"  ({sql_str(d['id'])}, {sql_str(d['name'])}, {sql_str(d['label'])}, {sql_str(d['color'])})"
        for d in cfg["domains"]
    )
    seed = Template((tmpl / "0002_seed_domains.sql.tmpl").read_text(encoding="utf-8")).substitute(rows=rows)

    env = "".join(
        f"MAILBOX_{k.upper()}={cfg[k]}\n" for k in (*RESOURCE_KEYS, "database_id")
    ) + "MAILBOX_DOMAINS=" + ",".join(d["name"] for d in cfg["domains"]) + "\n"

    return {
        root / "wrangler.toml": f"# {GENERATED.format(src='templates/wrangler.toml.tmpl')}\n" + wrangler,
        root / "migrations" / "0002_seed_domains.sql": f"-- {GENERATED.format(src='templates/0002_seed_domains.sql.tmpl')}\n"
        + seed,
        root / "instance.env": f"# {GENERATED.format(src=INSTANCE)}\n" + env,
    }


def dump(cfg: dict) -> str:
    ordered = {k: cfg[k] for k in (*RESOURCE_KEYS[:3], "database_id", *RESOURCE_KEYS[3:])}
    ordered |= {k: cfg[k] for k in ("access_team", "access_aud", "allowed_emails", "metrics")}
    ordered["domains"] = cfg["domains"]
    return json.dumps(ordered, indent=2, ensure_ascii=False) + "\n"


# --- CLI ---------------------------------------------------------------------


def parse_args(argv: list[str]) -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description="Configura a instância do mailbox (domínios e recursos da Cloudflare).",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__.split("Exemplos", 1)[1],
    )
    g = p.add_argument_group("domínios (repetíveis; --label e --color seguem a ordem dos --domain)")
    g.add_argument("--domain", action="append", default=[], help="ex.: exemplo.com.br")
    g.add_argument("--label", action="append", default=[], help="nome curto exibido no front")
    g.add_argument("--color", action="append", default=[], help="cor do domínio no front, #RRGGBB")

    r = p.add_argument_group("recursos da Cloudflare (padrão: valores atuais do instance.json)")
    r.add_argument("--worker", help=f"nome do Worker (padrão novo: {DEFAULTS['worker']})")
    r.add_argument("--bucket", help="nome do bucket R2")
    r.add_argument("--database", help="nome do banco D1")
    r.add_argument("--database-id", help="id do D1 impresso por `wrangler d1 create`")
    r.add_argument("--queue", help="fila de ingestão")
    r.add_argument("--dlq", help="fila de mensagens mortas")

    a = p.add_argument_group("Cloudflare Access (Zero Trust > Access > Applications)")
    a.add_argument("--access-team", help="team domain, ex.: acmeco.cloudflareaccess.com")
    a.add_argument("--access-aud", help="Application Audience (AUD) Tag")
    a.add_argument("--allowed-email", action="append", default=[], help="email permitido (repetível; vazio = quem a política do Access deixar)")

    p.add_argument("--metrics", choices=["on", "off"], help="métricas no Analytics Engine (ative o produto no painel antes)")

    p.add_argument("--check", action="store_true", help="não escreve; falha se os gerados estão desatualizados")
    p.add_argument("--root", type=Path, default=ROOT, help=argparse.SUPPRESS)
    return p.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    root = args.root
    try:
        cfg = load(root)
        if args.check:
            validate(cfg)
            stale = [
                str(path.relative_to(root))
                for path, content in render(root, cfg).items()
                if not path.exists() or path.read_text(encoding="utf-8") != content
            ]
            if stale:
                print("desatualizados (rode scripts/setup_instance.py):", ", ".join(stale), file=sys.stderr)
                return 1
            return 0

        old_ids = {d["name"]: d["id"] for d in cfg["domains"]}
        cfg = apply_args(cfg, args)
        validate(cfg)
    except ConfigError as e:
        print(f"erro: {e}", file=sys.stderr)
        return 2

    (root / INSTANCE).write_text(dump(cfg), encoding="utf-8")
    for path, content in render(root, cfg).items():
        path.write_text(content, encoding="utf-8")
        print(f"gerado: {path.relative_to(root)}")

    if old_ids and {d["name"]: d["id"] for d in cfg["domains"]} != old_ids:
        print(
            "\natenção: os domínios mudaram. Se a migração 0002 já foi aplicada em algum D1,\n"
            "ela não roda de novo: crie uma migração nova para incluir ou remover domínios.",
            file=sys.stderr,
        )
    if cfg["database_id"] == DEFAULTS["database_id"]:
        print("\nD1 com id placeholder: serve para local. Para deploy rode `just remote-bootstrap`\n"
              "e depois este script com --database-id.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
