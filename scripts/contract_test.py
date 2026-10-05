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
# Domínio da instância local (o handler de email só aceita os dela).
_instance = MAILBOX / "instance.json"
DOMAIN = json.loads((_instance if _instance.exists() else MAILBOX / "instance.example.json").read_text())["domains"][0]["name"]

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
    b"From: Joao <joao@cliente.com.br>\r\nTo: contato@exemplo.com.br\r\nSubject: Re: Orcamento\r\n\r\nPode ser.\r\n"
)

# (arquivo ou bytes, envelope_to)
EMAILS = [
    ("texto_puro.eml", f"contato@{DOMAIN}"),
    ("multipart_alternative.eml", f"dev@{DOMAIN}"),
    ("imagem_inline.eml", f"marketing@{DOMAIN}"),
    ("rfc822_aninhado.eml", f"contato@{DOMAIN}"),
    # charset_quebrado.eml fica de fora: o simulador local recusa charset
    # desconhecido antes do Worker. Coberto pelos goldens do `mime`.
    (REPLY, f"contato@{DOMAIN}"),
]

AUD = "c0ffee" * 8
OWNER = f"dono@{DOMAIN}"

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

    def request(
        self, path: str, data: bytes | None = None, method: str | None = None, headers: dict | None = None
    ) -> tuple[int, dict[str, str], bytes]:
        h = {"Cf-Access-Jwt-Assertion": self.token} if self.token else {}
        h |= headers or {}
        # Acentos na query (ex.: q=orçamento) viram %XX como o navegador faria.
        path = urllib.parse.quote(path, safe="/?&=%:+,;@*")
        req = urllib.request.Request(self.base + path, data=data, method=method or ("POST" if data else "GET"), headers=h)
        try:
            with urllib.request.urlopen(req, timeout=30) as r:
                return r.status, {k.lower(): v for k, v in r.headers.items()}, r.read()
        except urllib.error.HTTPError as e:
            return e.code, {k.lower(): v for k, v in e.headers.items()}, e.read()

    def json(self, path: str) -> tuple[int, dict]:
        status, _, body = self.request(path)
        return status, json.loads(body)

    def send(self, method: str, path: str, body=None, headers: dict | None = None) -> tuple[int, dict]:
        """Mutação com JSON (como o front faz). `headers` sobrescreve."""
        h = {"Content-Type": "application/json"} | (headers or {})
        data = None if body is None else (body if isinstance(body, bytes) else json.dumps(body).encode())
        status, _, raw = self.request(path, data, method, h)
        return status, (json.loads(raw) if raw else {})


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
        verify_writes(api)
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
    check(api.json(f"/api/messages?domain={DOMAIN}")[1]["items"] == items[:50], "filtro por domínio")
    check(api.json("/api/messages?folder=archive")[1]["items"] == [], "pasta vazia")
    for path, code, status in [
        ("/api/messages?folder=rascunho", "bad_request", 400),
        ("/api/messages?limit=0", "bad_request", 400),
        ("/api/messages?cursor=lixo", "bad_request", 400),
        ("/api/messages?q=%21%21%21", "bad_request", 400),
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
    check(status == 200 and len(doms["items"]) == 1 and d["name"] == f"{DOMAIN}", f"um domínio: {DOMAIN}")
    check(d.get("unread") == len(EMAILS), "não lidos = mensagens novas")
    addrs = {i["address"] for i in d.get("identities", [])}
    check(addrs == {f"contato@{DOMAIN}", f"dev@{DOMAIN}", f"marketing@{DOMAIN}"}, "identidades criadas")

    print("GET /api/messages/:id (imagem inline)")
    nl = by_subject["Newsletter com logo"]
    status, m = api.json(f"/api/messages/{nl['id']}")
    check(status == 200 and set(m) == DETAIL_KEYS, "detalhe com os campos do contrato")
    att = m["attachments"][0] if m["attachments"] else {}
    check(att.get("url") and f'src="{att["url"]}"' in (m["html"] or ""), "cid: virou a URL do anexo no HTML")
    check(m["has_remote_images"] is False and m["identity"] == f"marketing@{DOMAIN}", "sem imagens remotas")

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



def wait_for(api: Api, path: str, pred, what: str, tries: int = 30):
    for _ in range(tries):
        s, body = api.json(path)
        if s == 200 and pred(body):
            return body
        time.sleep(1)
    check(False, what)
    return None


def verify_writes(api: Api) -> None:
    def subjects(path: str) -> list[str]:
        return sorted(i["subject"] for i in api.json(path)[1].get("items", []))

    print("busca (q)")
    check(subjects("/api/messages?q=orçamento") == ["Orçamento de outubro", "Re: Orcamento"], "assunto, sem diferenciar acento")
    check("Orçamento de outubro" in subjects("/api/messages?q=jo%C3%A3o"), "nome do remetente (João)")
    check(subjects("/api/messages?q=s%C3%ADmbolos") == ["Orçamento de outubro"], "palavra do corpo")
    check(subjects("/api/messages?q=github") == ["Re: [acmeco/mailbox] CI passou"], "endereço do remetente")
    check(subjects("/api/messages?q=orc+outub") == ["Orçamento de outubro"], "prefixo e termos combinados (E)")
    check(subjects("/api/messages?q=inexistentexyz") == [], "sem resultado")
    s, _ = api.json("/api/messages?q=" + urllib.parse.quote('" OR * NEAR( x:y ^'))
    check(s == 200, "sintaxe do FTS5 no texto não quebra a busca")
    check(api.json("/api/messages?q=" + urllib.parse.quote("!!! *"))[0] == 400, "só pontuação -> 400")

    print("PATCH /api/messages/:id")
    items = api.json("/api/messages?limit=100")[1]["items"]
    by = {i["subject"]: i for i in items}
    unread0 = api.json("/api/domains")[1]["items"][0]["unread"]
    nl = by["Newsletter com logo"]
    s, it = api.send("PATCH", f"/api/messages/{nl['id']}", {"read": True, "starred": True})
    check(s == 200 and it["is_read"] and it["is_starred"], "lido + estrela devolve o item atualizado")
    check(api.json("/api/domains")[1]["items"][0]["unread"] == unread0 - 1, "contador de não lidos cai")
    s, it = api.send("PATCH", f"/api/messages/{nl['id']}", {"folder": "archive"})
    check(s == 200 and it["folder"] == "archive", "mover para arquivo")
    check("Newsletter com logo" not in subjects("/api/messages") and subjects("/api/messages?folder=archive") == ["Newsletter com logo"], "some da entrada, aparece no arquivo")
    check("Newsletter com logo" in subjects("/api/messages?q=newsletter"), "busca sem pasta acha em qualquer pasta")
    check(subjects("/api/messages?q=newsletter&folder=inbox") == [], "busca com pasta respeita a pasta")
    for body, status in [({}, 400), ({"read": "sim"}, 400), ({"folder": "rascunho"}, 400), ({"lido": True}, 400)]:
        check(api.send("PATCH", f"/api/messages/{nl['id']}", body)[0] == status, f"PATCH {body} -> {status}")
    check(api.send("PATCH", "/api/messages/01M44NRD000000000000000099", {"read": True})[0] == 404, "mensagem inexistente -> 404")

    print("CSRF")
    path = f"/api/messages/{nl['id']}"
    check(api.send("PATCH", path, b'{"read":false}', {"Content-Type": "text/plain"})[0] == 415, "text/plain (formulário) -> 415")
    check(api.send("PATCH", path, {"read": False}, {"Origin": "https://malicioso.example"})[0] == 403, "Origin de outro site -> 403")
    check(api.send("PATCH", path, {"read": False}, {"Sec-Fetch-Site": "cross-site"})[0] == 403, "Sec-Fetch-Site cross-site -> 403")
    check(api.send("PATCH", path, {"read": False}, {"Origin": api.base})[0] == 200, "mesma origem passa")

    print("POST /api/messages/bulk")
    ids = [i["id"] for i in items][:3]
    s, r = api.send("POST", "/api/messages/bulk", {"ids": ids, "patch": {"read": True}})
    check(s == 200 and r == {"updated": 3}, "3 marcadas como lidas")
    unread = {i["id"]: i["is_read"] for i in api.json("/api/messages?limit=100&q=")[1]["items"]}
    check(all(unread.get(i, True) for i in ids), "refletiu na lista")
    check(api.send("POST", "/api/messages/bulk", {"ids": ids * 40, "patch": {"read": True}})[0] == 200, "ids repetidos contam uma vez")
    check(api.send("POST", "/api/messages/bulk", {"ids": [], "patch": {"read": True}})[0] == 400, "lista vazia -> 400")

    print("regras")
    s, rule = api.send("POST", "/api/rules", {"match_address": f"promo@{DOMAIN}", "action": "move", "folder": "archive", "priority": 1})
    check(s == 201 and rule.get("folder") == "archive", "criar regra move -> 201")
    s, rule2 = api.send("POST", "/api/rules", {"match_address": f"*@{DOMAIN}", "action": "mark_read", "priority": 2})
    check(s == 201, "criar regra mark_read")
    check(len(api.json("/api/rules")[1]["items"]) == 2, "listar regras")
    check(api.send("POST", "/api/rules", {"match_address": "*", "action": "archive"})[0] == 400, "padrão inválido -> 400")
    check(api.send("POST", "/api/rules", {"match_address": f"*@{DOMAIN}", "action": "move"})[0] == 400, "move sem pasta -> 400")
    s, r2 = api.send("PATCH", f"/api/rules/{rule2['id']}", {"enabled": False})
    check(s == 200 and r2["enabled"] is False and r2["action"] == "mark_read", "desabilitar mantém o resto")
    s, r2 = api.send("PATCH", f"/api/rules/{rule2['id']}", {"enabled": True})

    raw = b"Message-ID: <promo.1@loja.com>\r\nFrom: Loja <promo@loja.com>\r\nTo: promo@x.com\r\nSubject: Ofertas da semana\r\n\r\nDescontos.\r\n"
    q = urllib.parse.urlencode({"from": "promo@loja.com", "to": f"promo@{DOMAIN}"})
    api.request(f"/cdn-cgi/local/email?{q}", raw)
    got = wait_for(api, "/api/messages?q=ofertas", lambda b: len(b["items"]) == 1, "email novo processado")
    if got:
        it = got["items"][0]
        check(it["folder"] == "archive" and it["is_read"] is True, "regras aplicadas no email novo (arquivo + lido)")

    check(api.send("DELETE", f"/api/rules/{rule['id']}", None)[0] == 204, "apagar regra -> 204")
    check(api.send("DELETE", f"/api/rules/{rule['id']}", None)[0] == 404, "apagar de novo -> 404")
    check(api.send("PATCH", "/api/rules/01M44NRD000000000000000099", {"enabled": False})[0] == 404, "regra inexistente -> 404")

if __name__ == "__main__":
    sys.exit(main())
