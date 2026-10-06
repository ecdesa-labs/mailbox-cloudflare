#!/usr/bin/env python3
"""Testes de contrato da API de leitura (docs seção 7, critério da Fase 5).

Sobe o Worker de verdade com `wrangler dev` e estado local isolado (D1, R2 e
filas simulados), injeta as fixtures do `mime` pelo Email Routing simulado,
espera a fila processar e confere cada endpoint.

    just contract
"""

from __future__ import annotations

import base64
import re
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
        data = json.loads(body)
        SPEC.validate("get", path, status, data)
        return status, data

    def send(self, method: str, path: str, body=None, headers: dict | None = None) -> tuple[int, dict]:
        """Mutação com JSON (como o front faz). `headers` sobrescreve."""
        h = {"Content-Type": "application/json"} | (headers or {})
        data = None if body is None else (body if isinstance(body, bytes) else json.dumps(body).encode())
        status, _, raw = self.request(path, data, method, h)
        out = json.loads(raw) if raw else {}
        if raw:
            SPEC.validate(method.lower(), path, status, out)
        return status, out


class Spec:
    """Validador mínimo de JSON Schema (o subconjunto que o openapi.json
    usa): toda resposta JSON do teste é conferida contra a especificação."""

    def __init__(self, doc: dict) -> None:
        self.doc = doc
        self.checked = 0
        self.violations: list[str] = []

    def resolve(self, schema: dict) -> dict:
        while "$ref" in schema:
            node = self.doc
            for part in schema["$ref"].lstrip("#/").split("/"):
                node = node[part]
            schema = node
        return schema

    def errors(self, schema: dict, v, at: str = "$") -> list[str]:
        schema = self.resolve(schema)
        if "allOf" in schema:
            # allOf de objetos: une campos (additionalProperties vale para a união).
            merged: dict = {"type": "object", "properties": {}, "required": [], "additionalProperties": False}
            for part in schema["allOf"]:
                part = self.resolve(part)
                merged["properties"] |= part.get("properties", {})
                merged["required"] += part.get("required", [])
            return self.errors(merged, v, at)
        if "oneOf" in schema:
            ok = [s for s in schema["oneOf"] if not self.errors(s, v, at)]
            return [] if len(ok) == 1 else [f"{at}: {len(ok)} opções de oneOf servem"]
        out = []
        types = schema.get("type")
        if types:
            types = [types] if isinstance(types, str) else types
            pytype = {dict: "object", list: "array", str: "string", bool: "boolean", type(None): "null"}.get(type(v))
            if isinstance(v, int) and not isinstance(v, bool):
                pytype = "integer"
            elif isinstance(v, float):
                pytype = "integer" if v.is_integer() else "number"
            if pytype not in types and not (pytype == "integer" and "number" in types):
                return [f"{at}: tipo {pytype}, esperado {types}"]
        if "enum" in schema and v not in schema["enum"]:
            out.append(f"{at}: {v!r} fora de {schema['enum']}")
        if "const" in schema and v != schema["const"]:
            out.append(f"{at}: {v!r} != {schema['const']!r}")
        if isinstance(v, str) and "pattern" in schema and not re.match(schema["pattern"], v):
            out.append(f"{at}: {v!r} não casa {schema['pattern']}")
        if isinstance(v, dict):
            props = schema.get("properties", {})
            for k in schema.get("required", []):
                if k not in v:
                    out.append(f"{at}: falta {k}")
            for k, val in v.items():
                if k in props:
                    out += self.errors(props[k], val, f"{at}.{k}")
                elif schema.get("additionalProperties") is False:
                    out.append(f"{at}: campo fora da especificação: {k}")
        if isinstance(v, list) and "items" in schema:
            for i, item in enumerate(v):
                out += self.errors(schema["items"], item, f"{at}[{i}]")
        return out

    def template(self, path: str) -> str | None:
        segs = urllib.parse.urlsplit(path).path.split("/")
        for tpl in self.doc["paths"]:
            t = tpl.split("/")
            if len(t) == len(segs) and all(a == b or a.startswith("{") for a, b in zip(t, segs)):
                # Prefere rota estática (ex.: /bulk) à com parâmetro.
                if "{" not in tpl or not any(
                    o.split("/") == segs for o in self.doc["paths"] if "{" not in o
                ):
                    return tpl
        return None

    def validate(self, method: str, path: str, status: int, data) -> None:
        if not urllib.parse.urlsplit(path).path.startswith("/api/"):
            return
        tpl = self.template(path)
        if tpl is None and status == 404:
            return  # rota inexistente de propósito (teste do 404 genérico)
        op = self.doc["paths"].get(tpl, {}).get(method) if tpl else None
        resp = (op or {}).get("responses", {}).get(str(status))
        if resp is None:
            self.violations.append(f"{method.upper()} {path} -> {status}: resposta não documentada")
            return
        schema = self.resolve(resp).get("content", {}).get("application/json", {}).get("schema")
        if schema is None:
            return
        self.checked += 1
        for e in self.errors(schema, data):
            self.violations.append(f"{method.upper()} {path} -> {status}: {e}")


SPEC = Spec(json.loads((MAILBOX / "worker/openapi.json").read_text()))


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
        verify_send(api, state / "dev.log")
        verify_retention(api, state, port)
        verify_spec(api)
        verify_logs(state / "dev.log")
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


def verify_send(api: Api, log: Path) -> None:
    """Fase 8. Local, o binding send_email do wrangler não entrega: só
    registra a mensagem no log."""
    print("POST /api/messages (envio)")
    ident = next(i for d in api.json("/api/domains")[1]["items"] for i in d["identities"] if i["address"] == f"contato@{DOMAIN}")
    inbox = api.json("/api/messages?limit=100")[1]["items"]
    orig = next(i for i in inbox if i["subject"] == "Orçamento de outubro")
    body = {
        "in_reply_to": orig["id"], "from_identity_id": ident["id"],
        "to": [{"name": "João da Silva", "addr": "joao@cliente.com.br"}], "cc": ["maria@cliente.com.br"],
        "subject": "Re: Orçamento de outubro", "text": "Obrigado, João.\n\nRecebido.",
    }
    s, r = api.send("POST", "/api/messages", body)
    check(s == 201 and r.get("stored") == "indexed", f"resposta enviada e indexada na hora ({s} {r})")
    if s != 201:
        return
    check(r["thread_id"] == orig["thread_id"], "resposta na thread da original")
    t = api.json(f"/api/threads/{orig['thread_id']}")[1]["messages"]
    mine = next((m for m in t if m["id"] == r["id"]), None)
    check(mine is not None and mine["folder"] == "sent" and mine["is_read"] is True, "na thread, em enviados e lida")
    d = api.json(f"/api/messages/{r['id']}")[1]
    check(d["from"]["addr"] == f"contato@{DOMAIN}" and d["identity"] == f"contato@{DOMAIN}", "From = identidade escolhida")
    check([p["addr"] for p in d["to"]] == ["joao@cliente.com.br"] and [p["addr"] for p in d["cc"]] == ["maria@cliente.com.br"], "To e Cc gravados")
    eml = api.request(d["raw_url"])[2]
    check(b"In-Reply-To: <CAF+texto.001@cliente.com.br>" in eml, ".eml da cópia com In-Reply-To da original")
    check(any(i["id"] == r["id"] for i in api.json("/api/messages?folder=sent")[1]["items"]), "lista de enviados")
    check(all(i["id"] != r["id"] for i in api.json("/api/messages?limit=100")[1]["items"]), "fora da caixa de entrada")
    check("Obrigado" in (api.json("/api/messages?q=obrigado")[1]["items"] or [{}])[0].get("snippet", ""), "busca acha o enviado")

    s, novo = api.send("POST", "/api/messages", {"from_identity_id": ident["id"], "to": ["alguem@exemplo.com"], "subject": "Assunto novo", "text": "Oi"})
    check(s == 201 and novo["thread_id"] not in (None, orig["thread_id"]), "mensagem nova abre thread própria")

    base = {"from_identity_id": ident["id"], "to": ["a@b.com"], "subject": "x", "text": "x"}
    check(api.send("POST", "/api/messages", base | {"to": []})[0] == 400, "sem destinatário -> 400")
    check(api.send("POST", "/api/messages", base | {"bcc": ["c@d.com"]})[0] == 400, "campo desconhecido -> 400")
    check(api.send("POST", "/api/messages", base | {"to": [f"a{i}@b.com" for i in range(51)]})[0] == 400, "mais de 50 destinatários -> 400")
    check(api.send("POST", "/api/messages", base | {"from_identity_id": "01M44NRD000000000000000099"})[0] == 404, "identidade desconhecida -> 404")
    check(api.send("POST", "/api/messages", base | {"in_reply_to": "01M44NRD000000000000000099"})[0] == 404, "original desconhecida -> 404")
    check(api.send("POST", "/api/messages", base, {"Content-Type": "text/plain"})[0] == 415, "sem JSON -> 415")
    check(api.send("POST", "/api/messages", base, {"Origin": "https://evil.example"})[0] == 403, "outra origem -> 403")


def d1_local(state: Path, sql: str) -> list[dict]:
    out = subprocess.run(
        ["npx", "wrangler", "d1", "execute", "mailbox", "--local", "--persist-to", str(state), "--json", "--command", sql],
        cwd=MAILBOX, check=True, capture_output=True, text=True,
    ).stdout  # fmt: skip
    return json.loads(out)[0]["results"]


def run_cron(api: Api, log: Path) -> None:
    before = log.read_text(errors="replace").count('"event":"reconcile"')
    status, _, body = api.request("/cdn-cgi/local/scheduled?cron=" + urllib.parse.quote("*/10 * * * *"))
    if status != 200:
        raise SystemExit(f"não consegui disparar o cron ({status}): {body[:200]!r}")
    for _ in range(30):
        if log.read_text(errors="replace").count('"event":"reconcile"') > before:
            time.sleep(1)  # a retenção roda logo depois da reconciliação
            return
        time.sleep(0.5)
    raise SystemExit("o cron não registrou a reconciliação no log")


def verify_retention(api: Api, state: Path, port: int) -> None:
    print("retenção (lixeira 30 dias)")
    items = api.json("/api/messages?limit=100")[1]["items"]
    by = {i["subject"]: i for i in items}
    velho, novo = by["Orçamento de outubro"], by["Re: [acmeco/mailbox] CI passou"]
    for m in (velho, novo):
        api.send("PATCH", f"/api/messages/{m['id']}", {"folder": "trash"})
    row = d1_local(state, f"SELECT folder_changed_at FROM messages WHERE id = '{velho['id']}'")[0]
    check(row["folder_changed_at"] and abs(row["folder_changed_at"] / 1000 - time.time()) < 120, "mover para a lixeira grava quando")
    raw_url = api.json(f"/api/messages/{velho['id']}")[1]["raw_url"]
    thread_before = api.json(f"/api/threads/{velho['thread_id']}")[1]["messages"]
    # Simula 31 dias na lixeira só para a primeira.
    d1_local(state, f"UPDATE messages SET folder_changed_at = folder_changed_at - 31 * 86400000 WHERE id = '{velho['id']}'")
    run_cron(api, state / "dev.log")
    check(api.json(f"/api/messages/{velho['id']}")[0] == 404, "vencida foi apagada")
    check(api.request(raw_url)[0] == 404, "o .eml dela também (não volta pela reconciliação)")
    check(api.json(f"/api/messages/{novo['id']}")[0] == 200, "recém-movida para a lixeira fica")
    t = api.json(f"/api/threads/{velho['thread_id']}")[1]
    check([m["id"] for m in t["messages"]] == [m["id"] for m in thread_before if m["id"] != velho["id"]], "thread perdeu só a mensagem apagada")
    check(api.json("/api/messages?q=s%C3%ADmbolos")[1]["items"] == [], "some da busca (FTS)")


def verify_spec(api: Api) -> None:
    print("OpenAPI")
    s, doc = api.json("/api/openapi.json")
    check(s == 200 and doc.get("openapi", "").startswith("3.1"), "/api/openapi.json publicado (3.1)")
    src = (MAILBOX / "worker/src/api.rs").read_text()
    routes = {(m, re.sub(r":(\w+)", r"{\1}", p)) for m, p in re.findall(r'\.(get|post|patch|delete)(?:_async)?\(\s*"(/api/[^"*]+)"', src)}
    documented = {(m, p) for p, ops in doc["paths"].items() for m in ops if m in ("get", "post", "patch", "delete")}
    check(routes == documented, f"rotas do código = rotas documentadas ({len(routes)})"
          + ("" if routes == documented else f" | só no código: {routes - documented} | só na spec: {documented - routes}"))
    check(SPEC.checked > 50, f"{SPEC.checked} respostas JSON validadas contra o schema")
    for v in SPEC.violations[:10]:
        print("        " + v)
    check(not SPEC.violations, f"nenhuma resposta fora da especificação ({len(SPEC.violations)} violações)")


def verify_logs(log: Path) -> None:
    print("logs estruturados")
    lines = [l for l in log.read_text(errors="replace").splitlines() if '"event"' in l]
    parsed = []
    for l in lines:
        i = l.find("{")
        try:
            parsed.append(json.loads(re.sub(r"\x1b\[[0-9;]*m", "", l[i:])))
        except json.JSONDecodeError:
            pass
    events = {p.get("event") for p in parsed}
    check({"ingest", "process", "send"} <= events, f"eventos no log: {sorted(e for e in events if e)}")
    check(all(p.get("level") in ("info", "warn", "error") for p in parsed), "todo evento tem level")

if __name__ == "__main__":
    sys.exit(main())
