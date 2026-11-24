# Desenvolvimento

[← Documentação](../../README.pt-BR.md) · [English](../en/development.md)

## Stack

- **Backend**: Rust (edição 2024) com Tokio, Axum, rusqlite (SQLite embutido), reqwest (rustls) e
  croner. Binário único, sem dependências em tempo de execução.
- **Frontend**: módulos ES em JavaScript puro, CSS e gráficos SVG feitos à mão; sem framework, sem
  bundler, sem dependências npm. Os arquivos de `web/` são embutidos no binário em tempo de
  compilação pelo `build.rs`.

## Estrutura

| Caminho | Conteúdo |
|---|---|
| `src/main.rs`, `src/lib.rs` | Inicialização e montagem |
| `src/api/` | Roteador HTTP, endpoints de teste de velocidade, configuração e sistema |
| `src/auth/` | Hash de senha, sessões, bloqueio, middleware de autenticação |
| `src/providers/` | Provedores Cloudflare, LibreSpeed e Ookla |
| `src/measure.rs` | Motor de vazão paralela e estatísticas de latência |
| `src/runner.rs`, `src/scheduler.rs` | Execução de testes, agendamento cron, manutenção diária |
| `src/db/` | Acesso ao SQLite, migrações, estatísticas, backups |
| `src/integrations/` | Despachante de notificações e os canais |
| `src/nodes/` | Nós remotos e o proxy |
| `src/data_transfer/` | Importação e exportação |
| `src/prometheus.rs`, `src/opengraph.rs`, `src/telemetry.rs` | Métricas, imagem OG, OpenTelemetry |
| `web/` | Interface web (`app.js`, `views/`, `locales/`, `styles.css`, fontes) |
| `web/tests/` | Testes unitários do frontend |
| `tests/` | Testes de integração em Rust |

## Rodando localmente

Instale a versão do Rust fixada em `rust-toolchain.toml` (o rustup a detecta automaticamente; o
`mise.toml` fixa a mesma versão para quem usa o [mise](https://mise.jdx.dev)).

```bash
NETRACK_DATA_DIR=./data cargo run
# http://localhost:5216
```

Mudanças em `web/` exigem recompilar (`cargo run` de novo), já que os arquivos são embutidos em
tempo de compilação. Variáveis úteis durante o desenvolvimento: `RUST_LOG=debug`,
`RUN_TEST_ON_STARTUP=true`, `TZ=America/Sao_Paulo`.

Para ter dados realistas sem esperar, importe um arquivo JSON ou CSV em **Ajustes → Importar
arquivo** (veja [Dados](data.md#importação) para o formato).

## Testes e verificações

Os mesmos comandos que a CI executa:

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
node --test web/tests/*.test.js     # Node 22+, sem precisar de npm install
```

## Traduções

Os textos da interface ficam em `web/locales/<code>.json`. Para adicionar um idioma, copie
`en.json`, traduza os valores, defina `_meta.name` (mostrado no seletor de idioma) e recompile.
Chaves ausentes usam o português como fallback.

## Integração contínua

Todo pull request executa:

- **Rust & Web UI Tests**: os comandos acima.
- **Multi-Arch Docker Build Check**: gera a imagem para `amd64` e `arm64`.
- **cargo audit** (também semanalmente): vulnerabilidades conhecidas nas dependências.
- **PR checks**: nome da branch, título do PR e mensagens de commit (abaixo).

A `main` é protegida: CI OK e as verificações de nome da branch, título do PR e mensagens de
commit precisam passar, o histórico é linear e as conversas precisam estar resolvidas.

## Contribuindo

- **Nomes de branch**: `<type>/<kebab-case-description>`, com o tipo sendo um de `feat`, `fix`,
  `refactor`, `docs`, `test`, `chore`, `hotfix` (ex.: `fix/upload-byte-counting`).
- **Commits e títulos de PR** seguem o [Conventional Commits](https://www.conventionalcommits.org/):
  `type(scope): description`, com o tipo sendo um de `feat`, `fix`, `docs`, `style`, `refactor`,
  `perf`, `test`, `build`, `ci`, `chore`, `revert`, e no máximo 72 caracteres depois dos dois
  pontos.
- Os pull requests são mesclados com squash, então o título do PR vira o commit na `main` e
  alimenta o changelog.

## Releases

As releases são automatizadas com o [release-please](https://github.com/googleapis/release-please):

1. Mesclar na `main` atualiza um PR de release que incrementa a versão em
   `Cargo.toml`/`Cargo.lock` e atualiza o `CHANGELOG.md` a partir das mensagens Conventional
   Commit (`feat` → minor, `fix` → patch).
2. Mesclar o PR de release cria a tag Git e a release no GitHub.
3. O workflow de release então gera a imagem multiarquitetura e a publica em
   `ghcr.io/diaszano/netrack` com as tags `<version>`, `<major>.<minor>` e `latest` (mais
   `<major>` a partir da 1.0).

O PR de release é aberto por um bot. Para que a CI rode nele, o repositório precisa de um secret
`RELEASE_PLEASE_TOKEN`: um personal access token fine-grained para este repositório com acesso de
leitura/escrita a **Contents**, **Pull requests** e **Issues**. Sem ele, o PR de release usa o
`GITHUB_TOKEN` padrão, que não dispara workflows, e as verificações obrigatórias nunca rodam.
