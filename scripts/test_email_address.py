"""Testes do email_address.py (só as partes puras; nada chama o wrangler)."""

import unittest

from email_address import clean_label, has_route, identity_sql, parse_address
from setup_instance import ConfigError

DOMAINS = ["exemplo.com.br", "pessoal.dev"]


class EnderecoTest(unittest.TestCase):
    def test_normaliza(self):
        self.assertEqual(parse_address(" Novo.Servico+x@Exemplo.com.br ", DOMAINS), ("novo.servico+x@exemplo.com.br", "exemplo.com.br"))

    def test_recusa(self):
        for raw in ["semarroba", "@exemplo.com.br", "a..b@exemplo.com.br", ".a@exemplo.com.br", "a.@exemplo.com.br",
                    "a b@exemplo.com.br", "a'@exemplo.com.br", "a@outro.com", "a@exemplo.com.br.evil", "x" * 65 + "@pessoal.dev"]:
            with self.subTest(raw=raw), self.assertRaises(ConfigError):
                parse_address(raw, DOMAINS)

    def test_nome(self):
        self.assertIsNone(clean_label(None))
        self.assertIsNone(clean_label("   "))
        self.assertEqual(clean_label("  Fulano   de  Tal "), "Fulano de Tal")
        with self.assertRaises(ConfigError):
            clean_label("a\x00b")
        with self.assertRaises(ConfigError):
            clean_label("x" * 65)

    def test_sql_escapa_e_nao_apaga_nome(self):
        sql = identity_sql("01M44NRD0000000000000000A9", "01M44NRD000000000000000001", "novo@exemplo.com.br", "D'Ávila")
        self.assertIn("'D''Ávila'", sql)
        self.assertIn("ON CONFLICT(address) DO UPDATE SET label = COALESCE(excluded.label, identities.label)", sql)
        self.assertIn(", NULL)", identity_sql("01M44NRD0000000000000000A9", "01M44NRD000000000000000001", "a@exemplo.com.br", None))

    def test_rota_existente(self):
        out = "Rule: 1\n  Name:     mailbox: a@exemplo.com.br\n  Matchers: to:a@exemplo.com.br\n  Actions:  worker:mailbox\n"
        self.assertTrue(has_route(out, "a@exemplo.com.br"))
        self.assertFalse(has_route(out, "b@exemplo.com.br"))
        self.assertFalse(has_route(out, "a@exemplo.com"))


if __name__ == "__main__":
    unittest.main()
