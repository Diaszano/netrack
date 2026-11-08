# netrack

Monitor de velocidade de internet auto-hospedado, escrito em Rust. Executa testes de velocidade (**Cloudflare**, **LibreSpeed** e **Ookla Speedtest CLI**) em intervalos agendados, armazena o histórico em SQLite e serve uma interface web responsiva (**Painel denso**, bilíngue `pt-BR`/`en`) junto de uma API HTTP JSON e exportador Prometheus em um único binário estático (`scratch`, ~11 MB) para `linux/amd64` e `linux/arm64`.

## Funcionalidades

- **3 Provedores de Teste**:
  - **Cloudflare** (`speed.cloudflare.com`) nativo em Rust.
  - **LibreSpeed** com lista pública de servidores ou URL customizada.
  - **Ookla Speedtest CLI** baixado sob demanda em `/data/bin/speedtest` apenas após aceite explícito da licença na interface ou API.
- **Interface Web Embutida (`rust-embed`)**:
  - Zero dependências de CDN em tempo de execução (fontes *Space Grotesk*, *Plus Jakarta Sans* e *JetBrains Mono* embutidas).
  - Gráficos SVG nativos (medidores radiais, histórico de linha/área, mapa de calor semanal 7×24 e médias por hora).
  - Internacionalização automática (`pt-BR` e `en`) extensível via arquivos JSON em `web/locales/`.
- **Segurança e Modos de Acesso**:
  - Níveis de proteção por senha (Argon2id + sessões em SQLite): `none` (sem senha), `public` (leitura pública, alterações protegidas) e `protected` (tudo exige login).
- **Notificações e Integrações (8 canais × 7 eventos)**:
  - Canais: **Discord**, **Telegram**, **Pushover**, **Gotify**, **ntfy**, **Webhook genérico**, **InfluxDB v2** e **Healthchecks**.
  - Eventos: `speedtest.started`, `speedtest.finished`, `speedtest.failed`, `speedtest.degraded`, `minute.passed`, `config.updated`, `expected.updated`.
- **Observabilidade e Dados**:
  - Endpoint **Prometheus** (`/api/prometheus/metrics`) com suporte a Basic Auth.
  - Exportação/importação em **JSON** e **CSV**, além de importação compatível com **MySpeed**.
  - Imagem **Open Graph** dinâmica (`/api/opengraph/image`) com o último resultado.
- **Multi-instância (Nós Remotos)**:
  - Cadastro de múltiplos nós `netrack` e alternância transparente na interface via proxy reverso com streaming (`/api/nodes/{id}/proxy/*`).

## Como rodar

```bash
docker build -t netrack:dev .
mkdir -p data && sudo chown 65534:65534 data
docker compose -f docker-compose.example.yml up -d
curl http://localhost:5216/healthz   # ok
```

Use `docker-compose.example.yml` como base. Ele usa `network_mode: host` para que o teste meça a interface real do host.

O container roda como UID 65534 (`nobody`). O diretório montado em `/data` precisa ser gravável por esse UID (`chown 65534:65534 ./data`). Com volume nomeado do Docker, as permissões já vêm corretas da imagem.

## Variáveis de ambiente

| Variável | Padrão | Descrição |
|---|---|---|
| `NETRACK_PORT` | `5216` | Porta HTTP (escuta em `0.0.0.0` e `[::]`) |
| `NETRACK_DATA_DIR` | `/data` | Diretório do banco SQLite, binário opcional do Ookla e backups diários (`backups/netrack-AAAA-MM-DD.db`, quantidade em `backup_keep`) |
| `NETRACK_TRUST_PROXY` | `false` | Usa o último IP de `X-Forwarded-For` (o anexado pelo proxy) como IP do cliente no bloqueio de login (5 falhas em 15 min). Ative só atrás de um proxy reverso confiável |
| `NETRACK_SECURE_COOKIE` | `false` | Adiciona `Secure` ao cookie de sessão (login e logout). Ative só se o acesso for por HTTPS, senão o navegador descarta o cookie |
| `TZ` | `UTC` | Fuso horário usado no agendamento e estatísticas (ex.: `America/Sao_Paulo`); vazio = `UTC` |
| `RUN_TEST_ON_STARTUP` | `false` | Executa um teste imediatamente ao iniciar |
| `PREVIEW_MODE` | `false` | Modo demonstração: bloqueia mutações (`403 Forbidden`) e exibe aviso na UI |
| `PREVIEW_MESSAGE` | *(padrão)* | Mensagem customizada exibida no banner quando `PREVIEW_MODE=true` |
| `RUST_LOG` | `info` | Nível de log (`info`, `debug`, ...) |

## Portas

- `5216/tcp`: Interface Web e API HTTP (sem `network_mode: host`, publique com `-p 5216:5216`).

## API HTTP

### Testes de Velocidade e Configuração
- `GET /healthz`
- `GET /api/speedtests`
- `POST /api/speedtests/run`
- `GET /api/speedtests/status`
- `POST /api/speedtests/pause`
- `POST /api/speedtests/continue`
- `GET /api/speedtests/statistics`
- `GET /api/speedtests/events` (SSE)
- `GET /api/speedtests/{id}`, `DELETE /api/speedtests/{id}`
- `GET /api/config`, `PATCH /api/config/{key}`

### Sistema e Provedores
- `GET /api/system/status`
- `GET /api/system/interfaces`
- `GET /api/system/servers/librespeed`
- `GET /api/system/servers/ookla`
- `GET /api/system/ookla/status`
- `POST /api/system/ookla/consent`
- `POST /api/system/ookla/install`
- `GET /api/opengraph/image`

### Autenticação
- `GET /api/auth/status`
- `POST /api/auth/login`
- `POST /api/auth/logout`
- `POST /api/auth/password`

### Integrações, Métricas, Dados e Multi-Nós
- `GET /api/integrations`, `POST /api/integrations`
- `PUT /api/integrations/{id}`, `DELETE /api/integrations/{id}`
- `POST /api/integrations/{id}/test`
- `GET /api/prometheus/metrics`
- `GET /api/data/stats`
- `GET /api/data/export?format=json|csv`
- `POST /api/data/import`
- `DELETE /api/data/reset`
- `GET /api/nodes`, `POST /api/nodes`
- `PUT /api/nodes/{id}`, `DELETE /api/nodes/{id}`
- `ANY /api/nodes/{id}/proxy/{*path}`

## Benchmarks

Alvo: Orange Pi Zero 3. Resultados **pendentes** (requer o hardware).

Procedimento de aceite:

1. Gere a imagem multi-arquitetura (requer `docker buildx`):
   `docker buildx build --platform linux/arm64,linux/amd64 -t netrack:dev .`
2. Suba o netrack no Pi com `network_mode: host` (compose acima).
3. Execute 3 testes: `curl -XPOST http://<pi>:5216/api/speedtests/run`, aguarde
   terminar (`/api/speedtests/status`) e anote ping, download e upload de
   `GET /api/speedtests`.
4. Compare com https://speed.cloudflare.com em um navegador num PC cabeado, na
   mesma rede e momento. Aceite: download e upload dentro de ±10%; ping dentro
   de ±5 ms.
5. Upload: confira com atenção. O netrack conta os bytes de upload quando são
   entregues ao cliente HTTP, não quando o servidor confirma o recebimento; um
   upload consistentemente acima do Cloudflare indica contagem superestimada e
   precisa ser corrigido antes de aceitar.
6. Durante um teste, rode `top` no Pi e anote o pico de CPU do processo
   `netrack`. Aceite: < 80% de UM núcleo (no `top`, 100% = 1 núcleo; o Pi Zero 3
   tem 4 núcleos, então o máximo possível é 400%).
7. Em repouso, leia `rssBytes` em `GET /api/system/status`. Aceite: < 30 MB.
8. Meça `time curl -s 'http://<pi>:5216/api/speedtests/statistics?from=AAAA-MM-DD&to=AAAA-MM-DD'`
   com histórico de 1 ano e anote o tempo.

| Item | Execução 1 | Execução 2 | Execução 3 | Aceite |
|---|---|---|---|---|
| Ping (ms), netrack | pendente | pendente | pendente | ±5 ms do Cloudflare |
| Ping (ms), speed.cloudflare.com | pendente | pendente | pendente | |
| Download (Mbps), netrack | pendente | pendente | pendente | ±10% do Cloudflare |
| Download (Mbps), speed.cloudflare.com | pendente | pendente | pendente | |
| Upload (Mbps), netrack | pendente | pendente | pendente | ±10% do Cloudflare |
| Upload (Mbps), speed.cloudflare.com | pendente | pendente | pendente | |
| Pico de CPU do processo (`top`) | pendente | pendente | pendente | < 80% de um núcleo |

| Item | Resultado | Aceite |
|---|---|---|
| RSS em repouso | pendente | < 30 MB |
| `statistics` com 1 ano | pendente | anotar tempo |
