# mailbox

Componente principal: o Worker em Rust. Visão geral e uso no [README da raiz](../README.pt-BR.md) ([English](../README.en.md)).

    sudo apt install -y pkg-config libssl-dev   # uma vez
    just setup                                   # uma vez
    just ci                                      # tudo que o CI roda
    just dev                                     # wrangler dev

## Instância

Domínios e nomes dos recursos na Cloudflare ficam em `instance.json` (fora do
git; modelo em `instance.example.json`).
`wrangler.toml`, `migrations/0002_seed_domains.sql` e `instance.env` são
gerados a partir dele e de `templates/`; não edite esses três à mão.
O script fica na raiz do repositório:

    cd ..
    python3 scripts/setup_instance.py --domain exemplo.com.br --label Exemplo --color '#2563EB'
    python3 scripts/setup_instance.py --help
