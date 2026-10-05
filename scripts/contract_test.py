#!/usr/bin/env python3
"""Testes de contrato da API de leitura (docs seção 7, critério da Fase 5).

Sobe o Worker de verdade com `wrangler dev` e estado local isolado (D1, R2 e
filas simulados), injeta as fixtures do `mime` pelo Email Routing simulado,
espera a fila processar e confere cada endpoint.

    just contract
"""

from __future__ import annotations

import base64
import http.server
import json
import shutil
import threading
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

AUD = "c0ffee" * 8
OWNER = "dono@acmeco.com.br"

failures: list[str] = []


def b64u(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


class FakeAccess:
    """Chave RSA real (openssl) + JWKS no formato do /cdn-cgi/access/certs."""

    def __init__(self, workdir: Path) -> None:
        self.key = workdir / "access.pem"
        subprocess.run(["openssl", "genrsa", "-out", str(self.key), "2048"], check=True, capture_output=True)
        mod = subprocess.run(["openssl", "rsa", "-in", str(self.key), "-noout", "-modulus"],
                             check=True, capture_output=True, text=True).stdout  # fmt: skip
        n = bytes.fromhex(mod.strip().split("=", 1)[1])
        jwks = json.dumps({"keys": [{"kid": "k1", "kty": "RSA", "alg": "RS256", "n": b64u(n), "e": "AQAB"}]}).encode()

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self) -> None:
                ok = self.path == "/cdn-cgi/access/certs"
                self.send_response(200 if ok else 404)
                self.send_header("content-type", "application/json")
                self.end_headers()
                self.wfile.write(jwks if ok else b"{}")

            def log_message(self, *a) -> None:
                pass

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.issuer = f"http://127.0.0.1:{self.server.server_port}"

    def token(self, *, alg: str = "RS256", kid: str = "k1", sign: bool = True, **claims) -> str:
        now = int(time.time())
        body = {"iss": self.issuer, "aud": [AUD], "exp": now + 600, "nbf": now - 5, "type": "app", "email": OWNER}
        body |= claims
        head = b64u(json.dumps({"alg": alg, "kid": kid}).encode())
        payload = b64u(json.dumps(body).encode())
        signing_input = f"{head}.{payload}".encode()
        sig = b""
        if sign:
            sig = subprocess.run(["openssl", "dgst", "-sha256", "-sign", str(self.key)],
                                 input=signing_input, check=True, capture_output=True).stdout  # fmt: skip
        return f"{head}.{payload}.{b64u(sig)}"


def check(cond: bool, what: str) -> None:
    print(("  ok    " if cond else "  FALHA ") + what)
    if not cond:
        failures.append(what)


class Api:
    def __init__(self, port: int, token: str | None = None) -> None:
        self.base = f"http://127.0.0.1:{port}"
        self.token = token

    def as_(self, token: str | None) -> "Api":
        api = Api(0, token)
        api.base = self.base
        return api

    def request(self, path: str, data: bytes | None = None) -> tuple[int, dict[str, str], bytes]:
        headers = {"Cf-Access-Jwt-Assertion": self.token} if self.token else {}
        req = urllib.request.Request(self.base + path, data=data, method="POST" if data else "GET", headers=headers)
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
        print("preparando D1 local, Access falso e subindo o Worker...")
        access = FakeAccess(state)
        run(["npx", "wrangler", "d1", "migrations", "apply", "mailbox", "--local", "--persist-to", str(state)])
        server = subprocess.Popen(
            ["npx", "wrangler", "dev", "--port", str(port), "--persist-to", str(state),
             "--var", f"MAILBOX_ACCESS_TEAM:{access.issuer}", "--var", f"MAILBOX_ACCESS_AUD:{AUD}",
             "--var", f"MAILBOX_ALLOWED_EMAILS:{OWNER}", "--var", "MAILBOX_AUTH_BYPASS:0"],
            cwd=MAILBOX, stdout=log, stderr=subprocess.STDOUT,
        )  # fmt: skip
        api = Api(port, access.token())
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

        verify_auth(api, access)
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


def verify_auth(api: Api, access: FakeAccess) -> None:
    print("autenticação (Access)")
    check(api.as_(None).request("/api/ping")[0] == 200, "/api/ping é público")
    s, me = api.json("/api/me")
    check(s == 200 and me == {"email": OWNER}, "token válido -> 200 e /api/me com o email")
    tampered = access.token()
    head, payload, sig = tampered.split(".")
    forged = b64u(json.dumps({"iss": access.issuer, "aud": [AUD], "exp": int(time.time()) + 600, "email": "intruso@x.com"}).encode())
    bad = {
        "sem token": None,
        "lixo": "abc",
        "expirado": access.token(exp=int(time.time()) - 3600),
        "aud de outra aplicação": access.token(aud=["outra"]),
        "iss de outro time": access.token(iss="https://outro.cloudflareaccess.com"),
        "type org": access.token(type="org"),
        "email fora da lista": access.token(email="intruso@x.com"),
        "payload adulterado": f"{head}.{forged}.{sig}",
        "alg none": access.token(alg="none", sign=False),
        "kid desconhecido": access.token(kid="k9"),
    }
    for what, token in bad.items():
        for path in ("/api/me", "/api/messages"):
            s, body = api.as_(token).json(path)
            check(s == 401 and body.get("error", {}).get("code") == "unauthorized", f"{what}: {path} -> 401")


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
