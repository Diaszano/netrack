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
- **Notificações e Integrações (8 canais × 8 eventos)**:
  - Canais: **Discord**, **Telegram**, **Pushover**, **Gotify**, **ntfy**, **Webhook genérico**, **InfluxDB v2** e **Healthchecks**.
  - Eventos: `speedtest.started`, `speedtest.finished`, `speedtest.failed`, `speedtest.degraded`, `speedtest.recovered`, `minute.passed`, `config.updated`, `expected.updated`.
  - `speedtest.degraded` dispara uma vez por queda (quando o teste anterior não estava lento) e `speedtest.recovered` quando a velocidade volta ao normal.
  - Segredos das integrações voltam mascarados como `***` nas respostas da API; enviar `***` em uma atualização mantém o valor armazenado.
- **Observabilidade e Dados**:
  - Endpoint **Prometheus** (`/api/prometheus/metrics`) com suporte a Basic Auth.
  - Métricas das últimas 24h: `netrack_download_avg_24h_mbps`, `netrack_upload_avg_24h_mbps`, `netrack_ping_avg_24h_ms` e `netrack_tests_24h{result="ok"|"failed"}`.
  - Exportação/importação em **JSON** e **CSV**, além de importação compatível com **MySpeed**.
  - Imagem **Open Graph** dinâmica (`/api/opengraph/image`) com o último resultado.
- **Multi-instância (Nós Remotos)**:
  - Cadastro de múltiplos nós `netrack` e alternância transparente na interface via proxy reverso com streaming (`/api/nodes/{id}/proxy/*`).
  - O nó principal faz login no nó remoto e usa o token de sessão; a senha do nó não viaja nas requisições do proxy.

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

## OpenTelemetry (OTLP)

Exporta **logs, traces e métricas** para um OpenTelemetry Collector (ou qualquer backend OTLP/HTTP). Fica desligado até você definir um endpoint; sem ele os logs continuam só no stdout.

| Variável | Descrição |
|---|---|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | Endpoint base (ex.: `http://otel-collector:4318`); liga os três sinais. O SDK acrescenta `/v1/traces`, `/v1/metrics` e `/v1/logs` |
| `OTEL_EXPORTER_OTLP_{TRACES,METRICS,LOGS}_ENDPOINT` | Endpoint de um sinal específico (liga só ele) |
| `OTEL_{TRACES,METRICS,LOGS}_EXPORTER=none` | Desliga um sinal |
| `OTEL_SDK_DISABLED=true` | Desliga tudo |
| `OTEL_SERVICE_NAME` | Nome do serviço (padrão `netrack`) |
| `OTEL_EXPORTER_OTLP_HEADERS`, `OTEL_EXPORTER_OTLP_TIMEOUT`, `OTEL_RESOURCE_ATTRIBUTES` | Padrão OpenTelemetry (autenticação, timeout, atributos do recurso) |

Só há suporte a OTLP sobre HTTP/protobuf (porta `4318`); `OTEL_EXPORTER_OTLP_PROTOCOL=grpc` não é suportado.

- **Traces:** um span por requisição HTTP (`GET /api/...`, continua o `traceparent` recebido; `/healthz` fica de fora), por teste (`speedtest.run`) e por tentativa do provedor (`speedtest.attempt`).
- **Logs:** todos os eventos do `tracing` (respeitando `RUST_LOG`), já correlacionados ao trace/span ativo.
- **Métricas:** `netrack.speedtest.runs` (por resultado), `netrack.speedtest.degraded`, `netrack.speedtest.duration`, `netrack.speedtest.{download,upload,ping,jitter}` (último teste) e `http.server.request.duration`. As métricas HTTP cobrem só as rotas da API (a UI estática não entra).

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
- `GET /api/data/stats` (inclui a lista de backups em `backups`)
- `GET /api/data/backups/{nome}` (baixa um backup diário, ex.: `netrack-2026-11-21.db`)
- `GET /api/data/export?format=json|csv[&from=AAAA-MM-DD][&to=AAAA-MM-DD]` (período em datas locais, ambas inclusivas)
- `POST /api/data/import`
- `POST /api/data/reset` (apaga o histórico e restaura a config padrão; a senha é mantida)
- `GET /api/nodes`, `POST /api/nodes`
- `PUT /api/nodes/{id}`, `DELETE /api/nodes/{id}`
- `ANY /api/nodes/{id}/proxy/{*path}`

## Benchmarks

Alvo: Orange Pi Zero 3 (4 núcleos, 4 GB). Medido em 2026-11-24 com o commit `aa1c994`, provedor Cloudflare (POA).

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
| Ping (ms), netrack | 12,71 | 12,62 | 12,06 | ✅ ±5 ms do Cloudflare |
| Ping (ms), speed.cloudflare.com | 15,6 | 15,4 | — | |
| Download (Mbps), netrack | 217,07 | 218,50 | 218,19 | ✅ ±10% do Cloudflare |
| Download (Mbps), speed.cloudflare.com | 212 | 212 | — | |
| Upload (Mbps), netrack | 260,32 | 260,81 | 261,52 | ✅ ±10% do Cloudflare (ver nota) |
| Upload (Mbps), speed.cloudflare.com | 138 | 123 | — | |
| Upload (Mbps), `curl` paralelo no Pi | 240,60 (8 conexões) | 234,44 (16 conexões) | — | |
| Pico de CPU do processo (`top`) | 40,0% | 44,0% | 58,8% | ✅ < 80% de um núcleo |

| Item | Resultado | Aceite |
|---|---|---|
| RSS em repouso | 6,6–7,4 MB | ✅ < 30 MB |
| `statistics` com 1 ano | 0,38 s (17.520 testes sintéticos, 1 a cada 30 min) | anotar tempo |

Nota sobre o upload: o speed.cloudflare.com no navegador e o `curl` com uma
conexão (91–153 Mbps) não saturam o link, então não servem de referência para
o netrack, que usa até 32 conexões. A referência usada foi o tempo total de
8 e 16 uploads simultâneos de 25 MB para `https://speed.cloudflare.com/__up`
feitos do próprio Pi, que inclui a confirmação do servidor. O netrack ficou
cerca de 8% acima dela, dentro do aceite, mas consistente com uma pequena
superestimação pela contagem na entrega ao cliente HTTP (passo 5).

O navegador rodou num Mac na mesma rede, não num PC cabeado.
