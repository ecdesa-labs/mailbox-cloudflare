"""Testes do setup_instance.py. Rodam com `just check-instance`."""

import json
import shutil
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
from pathlib import Path

import setup_instance as si

REPO = Path(__file__).resolve().parent.parent / "mailbox"


def run(root: Path, *argv: str) -> tuple[int, str]:
    err = StringIO()
    with redirect_stdout(StringIO()), redirect_stderr(err):
        code = si.main([*argv, "--root", str(root)])
    return code, err.getvalue()


class InstanciaNova(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        shutil.copytree(REPO / "templates", self.root / "templates")
        (self.root / "migrations").mkdir()

    def tearDown(self):
        shutil.rmtree(self.root)

    def cfg(self) -> dict:
        return json.loads((self.root / "instance.json").read_text())

    def test_gera_tudo_com_padroes(self):
        code, _ = run(self.root, "--domain", "Exemplo.COM.br", "--label", "Exemplo", "--color", "#2563eb")
        self.assertEqual(code, 0)
        cfg = self.cfg()
        self.assertEqual(cfg["worker"], "mailbox")
        self.assertEqual(cfg["database_id"], si.DEFAULTS["database_id"])
        [d] = cfg["domains"]
        self.assertEqual((d["name"], d["label"], d["color"]), ("exemplo.com.br", "Exemplo", "#2563EB"))
        self.assertRegex(d["id"], si.ULID_RE)

        seed = (self.root / "migrations/0002_seed_domains.sql").read_text()
        self.assertIn(f"('{d['id']}', 'exemplo.com.br', 'Exemplo', '#2563EB');", seed)
        self.assertIn('bucket_name = "mailbox"', (self.root / "wrangler.toml").read_text())
        self.assertIn("MAILBOX_DATABASE=mailbox\n", (self.root / "instance.env").read_text())
        self.assertIn("MAILBOX_DOMAINS=exemplo.com.br\n", (self.root / "instance.env").read_text())
        self.assertEqual(run(self.root, "--check")[0], 0)

    def test_varios_dominios_e_recursos(self):
        code, _ = run(
            self.root,
            "--domain", "a.com", "--label", "A", "--color", "#111",
            "--domain", "b.dev", "--label", "B", "--color", "#222222",
            "--worker", "caixa", "--queue", "caixa-in", "--dlq", "caixa-dlq",
        )  # fmt: skip
        self.assertEqual(code, 0)
        self.assertEqual([d["name"] for d in self.cfg()["domains"]], ["a.com", "b.dev"])
        wrangler = (self.root / "wrangler.toml").read_text()
        self.assertIn('name = "caixa"', wrangler)
        self.assertIn('dead_letter_queue = "caixa-dlq"', wrangler)
        self.assertIn('MAILBOX_DOMAINS = "a.com,b.dev"', wrangler)

    def test_rodar_de_novo_preserva_id_e_atualiza_so_o_pedido(self):
        run(self.root, "--domain", "a.com", "--label", "A", "--color", "#111")
        antes = self.cfg()
        code, _ = run(self.root, "--database-id", "0F2C1B3A-0000-4000-8000-000000000001")
        self.assertEqual(code, 0)
        depois = self.cfg()
        self.assertEqual(depois["database_id"], "0f2c1b3a-0000-4000-8000-000000000001")
        self.assertEqual(depois["domains"], antes["domains"])

        run(self.root, "--domain", "a.com", "--label", "Outro", "--color", "#333")
        self.assertEqual(self.cfg()["domains"][0]["id"], antes["domains"][0]["id"])

    def test_access(self):
        aud = "a" * 64
        code, _ = run(self.root, "--domain", "a.com", "--label", "A", "--color", "#111",
                      "--access-team", "https://Acmeco.cloudflareaccess.com/", "--access-aud", aud.upper(),
                      "--allowed-email", "Rosa@a.com")  # fmt: skip
        self.assertEqual(code, 0)
        cfg = self.cfg()
        self.assertEqual((cfg["access_team"], cfg["access_aud"], cfg["allowed_emails"]),
                         ("acmeco.cloudflareaccess.com", aud, ["rosa@a.com"]))  # fmt: skip
        w = (self.root / "wrangler.toml").read_text()
        self.assertIn('MAILBOX_ACCESS_TEAM = "acmeco.cloudflareaccess.com"', w)
        self.assertIn('MAILBOX_ALLOWED_EMAILS = "rosa@a.com"', w)
        for argv, msg in [
            (["--access-team", "acmeco.com"], "access_team inválido"),
            (["--access-aud", ""], "andam juntos"),
            (["--access-aud", "nao-hex"], "access_aud inválido"),
            (["--allowed-email", "sem-arroba"], "email inválido"),
        ]:
            with self.subTest(argv=argv):
                self.assertEqual(run(self.root, *argv)[0], 2)

    def test_metricas(self):
        run(self.root, "--domain", "a.com", "--label", "A", "--color", "#111")
        w = (self.root / "wrangler.toml").read_text()
        self.assertNotIn("analytics_engine_datasets", w, "desligado por padrão")
        self.assertEqual(run(self.root, "--metrics", "on")[0], 0)
        w = (self.root / "wrangler.toml").read_text()
        self.assertIn('dataset = "mailbox_events"', w)
        self.assertIs(self.cfg()["metrics"], True)

    def test_plano(self):
        run(self.root, "--domain", "a.com", "--label", "A", "--color", "#111")
        w = (self.root / "wrangler.toml").read_text()
        self.assertIn('MAILBOX_FULL_MAX_BYTES = "524288"', w, "free por padrão")
        self.assertEqual(run(self.root, "--plan", "paid")[0], 0)
        w = (self.root / "wrangler.toml").read_text()
        self.assertIn('MAILBOX_FULL_MAX_BYTES = "16777216"', w)
        self.assertIn('MAILBOX_MAX_ATTACHMENTS = "200"', w)
        self.assertEqual(self.cfg()["plan"], "paid")
        with self.assertRaises(SystemExit):  # o argparse recusa
            run(self.root, "--plan", "gold")

    def test_escapa_aspas_no_sql(self):
        run(self.root, "--domain", "a.com", "--label", "D'Ávila", "--color", "#111")
        seed = (self.root / "migrations/0002_seed_domains.sql").read_text()
        self.assertIn("'D''Ávila'", seed)

    def test_check_pega_edicao_a_mao(self):
        run(self.root, "--domain", "a.com", "--label", "A", "--color", "#111")
        (self.root / "wrangler.toml").write_text("editado")
        code, err = run(self.root, "--check")
        self.assertEqual(code, 1)
        self.assertIn("wrangler.toml", err)

    def test_recusa(self):
        casos = [
            ([], "ao menos um --domain"),
            (["--domain", "localhost", "--label", "x", "--color", "#111"], "domínio inválido"),
            (["--domain", "bücher.de", "--label", "x", "--color", "#111"], "punycode"),
            (["--domain", "a.com", "--label", "x", "--color", "red"], "cor inválida"),
            (["--domain", "a.com", "--color", "#111"], "--label aparece 0"),
            (["--domain", "a.com", "--label", " ", "--color", "#111"], "label vazio"),
            (["--domain", "a.com", "--label", "x", "--color", "#111",
              "--domain", "A.com", "--label", "y", "--color", "#222"], "repetido"),  # fmt: skip
            (["--domain", "a.com", "--label", "x", "--color", "#111", "--bucket", "Meu_Bucket"], "bucket inválido"),
            (["--domain", "a.com", "--label", "x", "--color", "#111", "--queue", "q-1", "--dlq", "q-1"], "diferentes"),
            (["--domain", "a.com", "--label", "x", "--color", "#111", "--database-id", "abc"], "database_id"),
            (["--label", "x"], "só valem junto"),
        ]
        for argv, msg in casos:
            with self.subTest(argv=argv):
                code, err = run(self.root, *argv)
                self.assertEqual(code, 2)
                self.assertIn(msg, err)
                self.assertFalse((self.root / "instance.json").exists())


class Ulid(unittest.TestCase):
    def test_bate_com_o_crate_rust(self):
        # Mesmo valor do teste `id::tests::ida_e_volta` em crates/domain.
        self.assertEqual(si.ulid(1_791_158_400_000, 1), "01M44NRD000000000000000001")

    def test_novo_e_valido_e_ordenado(self):
        a = si.ulid(1, (1 << 80) - 1)
        b = si.ulid(2, 0)
        self.assertLess(a, b)
        self.assertRegex(si.new_ulid(), si.ULID_RE)


class RepositorioAtual(unittest.TestCase):
    def test_instance_json_do_repo_e_valido(self):
        si.validate(si.load(REPO))


if __name__ == "__main__":
    unittest.main()
