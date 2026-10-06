#!/usr/bin/env python3
"""Libera um endereço novo de um domínio da instância: rota no Email Routing
(para receber) e identidade no D1 (para aparecer no `De` do compositor).

O domínio fica fechado (sem catch-all): só entra email para endereços com
rota. A rota é criada antes da identidade, então um endereço nunca aparece
para envio sem poder receber a resposta.

    just email-address novo@exemplo.com.br
    just email-address novo@exemplo.com.br "Nome de exibição"

Rodar de novo é seguro: rota existente não é duplicada; o nome só muda se
for informado. Só biblioteca padrão.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from setup_instance import ROOT, ConfigError, load, new_ulid, sql_str  # noqa: E402

# Parte local: minúsculas (o mailbox normaliza endereços assim), dígitos e
# . _ + -; sem começar/terminar com separador nem ".." (RFC 5321, subconjunto).
LOCAL_RE = re.compile(r"^[a-z0-9](?:[a-z0-9_+-]|\.(?!\.)){0,62}(?<![._+-])$")
MAX_LABEL = 64


def parse_address(raw: str, domains: list[str]) -> tuple[str, str]:
    """Devolve (endereço normalizado, domínio). Domínio precisa ser da instância."""
    addr = raw.strip().lower()
    local, sep, domain = addr.partition("@")
    if not sep or not LOCAL_RE.match(local):
        raise ConfigError(f"endereço inválido: {raw!r} (ex.: contato@{domains[0] if domains else 'exemplo.com.br'})")
    if domain not in domains:
        raise ConfigError(f"{domain!r} não é domínio da instância ({', '.join(domains)})")
    return addr, domain


def clean_label(raw: str | None) -> str | None:
    if raw is None:
        return None
    label = " ".join(raw.split())
    if not label:
        return None
    if len(label) > MAX_LABEL or any(ord(c) < 32 for c in label):
        raise ConfigError(f"nome inválido (até {MAX_LABEL} caracteres, sem controles)")
    return label


def identity_sql(identity_id: str, domain_id: str, address: str, label: str | None) -> str:
    """Cria a identidade; se já existe (ex.: recebeu email antes), só atualiza o nome."""
    return (
        "INSERT INTO identities (id, domain_id, address, label) VALUES "
        f"({sql_str(identity_id)}, {sql_str(domain_id)}, {sql_str(address)}, {sql_str(label) if label else 'NULL'}) "
        "ON CONFLICT(address) DO UPDATE SET label = COALESCE(excluded.label, identities.label)"
    )


def has_route(rules_output: str, address: str) -> bool:
    """Saída do `wrangler email routing rules list`: linhas `Matchers: to:<endereço>`."""
    return any(line.split(":", 1)[-1].strip() == f"to:{address}" for line in rules_output.splitlines() if "Matchers:" in line)


def wrangler(*args: str, capture: bool = False) -> str:
    out = subprocess.run(["npx", "wrangler", *args], cwd=ROOT, check=True, text=True, capture_output=capture)
    return out.stdout if capture else ""


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description="Libera um endereço novo (rota + identidade).")
    p.add_argument("address", help="ex.: novo@exemplo.com.br")
    p.add_argument("label", nargs="?", help="nome de exibição no From (opcional)")
    p.add_argument("--local", action="store_true", help="só a identidade, no D1 local (testes)")
    args = p.parse_args(sys.argv[1:] if argv is None else argv)

    try:
        cfg = load(ROOT)
        by_name = {d["name"]: d["id"] for d in cfg["domains"]}
        address, domain = parse_address(args.address, list(by_name))
        label = clean_label(args.label)
    except ConfigError as e:
        print(f"erro: {e}", file=sys.stderr)
        return 2

    try:
        if args.local:
            print("--local: rota ignorada (o Email Routing local não tem regras)")
        elif has_route(wrangler("email", "routing", "rules", "list", domain, capture=True), address):
            print(f"rota: {address} já vai para o Worker")
        else:
            wrangler(
                "email", "routing", "rules", "create", domain,
                "--name", f"mailbox: {address}",
                "--match-type", "literal", "--match-field", "to", "--match-value", address,
                "--action-type", "worker", "--action-value", cfg["worker"],
            )  # fmt: skip
            print(f"rota: {address} -> Worker {cfg['worker']}")

        sql = identity_sql(new_ulid(), by_name[domain], address, label)
        wrangler("d1", "execute", cfg["database"], "--local" if args.local else "--remote", "--command", sql, capture=True)
        print(f"identidade: {address}" + (f" ({label})" if label else "") + " disponível no De")
    except subprocess.CalledProcessError as e:
        print(f"erro: wrangler falhou ({e.returncode}); nada foi gravado depois desse passo", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
