#!/usr/bin/env python3
"""Testes de contrato da API de leitura (docs seção 7, critério da Fase 5).

Sobe o Worker de verdade com `wrangler dev` e estado local isolado (D1, R2 e
filas simulados), injeta as fixtures do `mime` pelo Email Routing simulado,
espera a fila processar e confere cada endpoint.

    just contract
"""

from __future__ import annotations

import json
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MAILBOX = ROOT / "mailbox"
FIXTURES = MAILBOX / "crates/mime/tests/fixtures"

ITEM_KEYS = {
    "id", "thread_id", "domain", "identity", "from", "subject", "snippet", "received_at",
    "is_read", "is_starred", "has_attachments", "folder", "processing",
}  # fmt: skip
DETAIL_KEYS = ITEM_KEYS | {
    "to", "cc", "date", "message_id", "envelope_to", "size", "text", "html",
    "has_remote_images", "auth", "attachments", "raw_url",
}  # fmt: skip

REPLY = (
    b"Message-ID: <resp.1@cliente.com.br>\r\nIn-Reply-To: <CAF+texto.001@cliente.com.br>\r\n"
    b"From: Joao <joao@cliente.com.br>\r\nTo: contato@acmeco.com.br\r\nSubject: Re: Orcamento\r\n\r\nPode ser.\r\n"
)

# (arquivo ou bytes, envelope_to)
EMAILS = [
    ("texto_puro.eml", "contato@acmeco.com.br"),
    ("multipart_alternative.eml", "dev@acmeco.com.br"),
    ("imagem_inline.eml", "marketing@acmeco.com.br"),
    ("rfc822_aninhado.eml", "contato@acmeco.com.br"),
    # charset_quebrado.eml fica de fora: o simulador local recusa charset
    # desconhecido antes do Worker. Coberto pelos goldens do `mime`.
    (REPLY, "contato@acmeco.com.br"),
]

failures: list[str] = []


def check(cond: bool, what: str) -> None:
    print(("  ok    " if cond else "  FALHA ") + what)
    if not cond:
        failures.append(what)


class Api:
    def __init__(self, port: int) -> None:
        self.base = f"http://127.0.0.1:{port}"

    def request(self, path: str, data: bytes | None = None) -> tuple[int, dict[str, str], bytes]:
        req = urllib.request.Request(self.base + path, data=data, method="POST" if data else "GET")
        try:
            with urllib.request.urlopen(req, timeout=30) as r:
                return r.status, {k.lower(): v for k, v in r.headers.items()}, r.read()
        except urllib.error.HTTPError as e:
            return e.code, {k.lower(): v for k, v in e.headers.items()}, e.read()

    def json(self, path: str) -> tuple[int, dict]:
        status, _, body = self.request(path)
        return status, json.loads(body)


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def run(cmd: list[str], **kw) -> None:
    subprocess.run(cmd, cwd=MAILBOX, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, **kw)


def main() -> int:
    state = Path(tempfile.mkdtemp(prefix="mailbox-contract-"))
    port = free_port()
    log = open(state / "dev.log", "w")
    server = None
    try:
        print("preparando D1 local e subindo o Worker...")
        run(["npx", "wrangler", "d1", "migrations", "apply", "mailbox", "--local", "--persist-to", str(state)])
        server = subprocess.Popen(
            ["npx", "wrangler", "dev", "--port", str(port), "--persist-to", str(state)],
            cwd=MAILBOX, stdout=log, stderr=subprocess.STDOUT,
        )  # fmt: skip
        api = Api(port)
        for _ in range(180):
            try:
                if api.request("/api/ping")[0] == 200:
                    break
            except OSError:
                pass
            time.sleep(1)
        else:
            raise SystemExit("o Worker não subiu (veja dev.log)")

        raws: dict[str, bytes] = {}
        for src, to in EMAILS:
            raw = src if isinstance(src, bytes) else (FIXTURES / src).read_bytes()
            q = urllib.parse.urlencode({"from": "cliente@exemplo.com", "to": to})
            status, _, body = api.request(f"/cdn-cgi/local/email?{q}", raw)
            if status != 200:
                raise SystemExit(f"email rejeitado ({status}): {body[:200]!r}")
            raws[src if isinstance(src, str) else "resposta"] = raw

        for _ in range(60):
            status, page = api.json("/api/messages?limit=100")
            if status == 200 and len(page["items"]) >= len(EMAILS):
                break
            time.sleep(1)
        else:
            raise SystemExit(f"a fila não processou tudo a tempo: {page}")

        verify(api, raws)
    finally:
        if server:
            server.terminate()
            try:
                server.wait(10)
            except subprocess.TimeoutExpired:
                server.kill()
        log.close()
        if failures:
            print(f"\nlog do wrangler: {state / 'dev.log'}")
        else:
            shutil.rmtree(state, ignore_errors=True)

    print(f"\n{'FALHOU: ' + str(len(failures)) if failures else 'tudo ok'}")
    return 1 if failures else 0


def verify(api: Api, raws: dict[str, bytes]) -> None:
    print("GET /api/messages")
    status, page = api.json("/api/messages?limit=100")
    items = page["items"]
    check(status == 200 and len(items) == len(EMAILS), f"{len(EMAILS)} mensagens na caixa de entrada")
    check(all(set(i) == ITEM_KEYS for i in items), "itens com exatamente os campos do contrato")
    keys = [(i["received_at"], i["id"]) for i in items]
    check(keys == sorted(keys, reverse=True), "ordem: mais recente primeiro")
    check(page["next_cursor"] is None, "sem próxima página quando cabe tudo")
    by_subject = {i["subject"]: i for i in items}

    print("paginação por cursor")
    seen, cursor, pages = [], None, 0
    while True:
        path = "/api/messages?limit=2" + (f"&cursor={urllib.parse.quote(cursor)}" if cursor else "")
        status, p = api.json(path)
        seen += [i["id"] for i in p["items"]]
        pages += 1
        cursor = p["next_cursor"]
        if not cursor or pages > 10:
            break
    check(seen == [i["id"] for i in items], f"limit=2 percorre tudo em {pages} páginas, sem repetir nem pular")

    print("filtros e erros")
    check(api.json("/api/messages?domain=acmeco.com.br")[1]["items"] == items[:50], "filtro por domínio")
    check(api.json("/api/messages?folder=archive")[1]["items"] == [], "pasta vazia")
    for path, code, status in [
        ("/api/messages?folder=rascunho", "bad_request", 400),
        ("/api/messages?limit=0", "bad_request", 400),
        ("/api/messages?cursor=lixo", "bad_request", 400),
        ("/api/messages?q=fatura", "not_implemented", 501),
        ("/api/messages/lixo", "not_found", 404),
        ("/api/messages/01M44NRD000000000000000099", "not_found", 404),
        ("/api/threads/01M44NRD000000000000000099", "not_found", 404),
        ("/api/attachments/01M44NRD000000000000000099", "not_found", 404),
        ("/api/nada", "not_found", 404),
    ]:
        s, body = api.json(path)
        check(s == status and body.get("error", {}).get("code") == code, f"{path} -> {status} {code}")

    print("GET /api/domains")
    status, doms = api.json("/api/domains")
    d = doms["items"][0] if doms.get("items") else {}
    check(status == 200 and len(doms["items"]) == 1 and d["name"] == "acmeco.com.br", "um domínio: acmeco.com.br")
    check(d.get("unread") == len(EMAILS), "não lidos = mensagens novas")
    addrs = {i["address"] for i in d.get("identities", [])}
    check(addrs == {"contato@acmeco.com.br", "dev@acmeco.com.br", "marketing@acmeco.com.br"}, "identidades criadas")

    print("GET /api/messages/:id (imagem inline)")
    nl = by_subject["Newsletter com logo"]
    status, m = api.json(f"/api/messages/{nl['id']}")
    check(status == 200 and set(m) == DETAIL_KEYS, "detalhe com os campos do contrato")
    att = m["attachments"][0] if m["attachments"] else {}
    check(att.get("url") and f'src="{att["url"]}"' in (m["html"] or ""), "cid: virou a URL do anexo no HTML")
    check(m["has_remote_images"] is False and m["identity"] == "marketing@acmeco.com.br", "sem imagens remotas")

    print("GET /api/attachments/:id")
    s, h, body = api.request(att["url"])
    check(s == 200 and body.startswith(b"\x89PNG") and len(body) == att["size"], "bytes do PNG")
    check(h.get("content-type") == "image/png" and h.get("content-disposition", "").startswith("inline"), "imagem exibível inline")
    check(h.get("x-content-type-options") == "nosniff" and "sandbox" in h.get("content-security-policy", ""), "nosniff + CSP sandbox")

    print("imagens remotas")
    gh = by_subject["Re: [acmeco/mailbox] CI passou"]
    _, m = api.json(f"/api/messages/{gh['id']}")
    check(m["has_remote_images"] and 'data-remote-src="https://tracker.example' in m["html"], "bloqueadas por padrão")
    check("<script" not in m["html"] and "javascript:" not in m["html"], "HTML sanitizado")
    _, m1 = api.json(f"/api/messages/{gh['id']}?remote_images=1")
    check('src="https://tracker.example' in m1["html"], "remote_images=1 libera")
    check(api.json(f"/api/messages/{gh['id']}?remote_images=2")[0] == 400, "remote_images=2 -> 400")

    print("anexo perigoso e .eml")
    fwd = by_subject["Fwd: Proposta comercial"]
    _, m = api.json(f"/api/messages/{fwd['id']}")
    s, h, body = api.request(m["attachments"][0]["url"])
    check(h.get("content-type") == "application/octet-stream" and h.get("content-disposition", "").startswith("attachment"), "message/rfc822 vira download")
    s, h, body = api.request(m["raw_url"])
    check(s == 200 and body == raws["rfc822_aninhado.eml"], ".eml original byte a byte")
    check(h.get("content-disposition", "").startswith("attachment"), ".eml sempre como download")

    print("GET /api/threads/:id")
    orc = by_subject["Orçamento de outubro"]
    s, t = api.json(f"/api/threads/{orc['thread_id']}")
    subjects = [x["subject"] for x in t.get("messages", [])]
    check(s == 200 and subjects == ["Orçamento de outubro", "Re: Orcamento"], "resposta na mesma thread, mais antiga primeiro")


if __name__ == "__main__":
    sys.exit(main())
