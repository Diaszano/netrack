# Observabilidade

[← Documentação](../../README.pt-BR.md) · [English](../en/observability.md)

## Prometheus

`GET /api/prometheus/metrics` expõe gauges no formato de texto do Prometheus.

**Último teste.** Com os labels `provider`, `server_id` e `server_name`. Só aparecem quando existe
pelo menos um teste. Se o último teste falhou, os valores são `0` e `netrack_test_success` é `0`.

| Métrica | Unidade |
|---|---|
| `netrack_download_speed_mbps` | Mbps |
| `netrack_upload_speed_mbps` | Mbps |
| `netrack_ping_latency_ms` | ms |
| `netrack_jitter_ms` | ms |
| `netrack_test_duration_seconds` | s |
| `netrack_last_test_timestamp_seconds` | tempo Unix |
| `netrack_test_success` | 1 ou 0 |

**Últimas 24 horas.** Sem labels; as médias usam só testes bem-sucedidos.

| Métrica | Unidade |
|---|---|
| `netrack_download_avg_24h_mbps` | Mbps |
| `netrack_upload_avg_24h_mbps` | Mbps |
| `netrack_ping_avg_24h_ms` | ms |
| `netrack_tests_24h{result="ok"\|"failed"}` | contagem (só aparece quando houve testes) |

### Autenticação

- Modo de proteção **Desativado**: aberto.
- **Público** ou **Protegido**: exige uma sessão (cookie ou token `Bearer`) ou HTTP Basic Auth
  com **qualquer nome de usuário** e a **senha do netrack**. Senhas erradas contam para o
  [bloqueio de login](security.md#bloqueio-de-login). Falhas recebem `401` com
  `WWW-Authenticate: Basic realm="netrack"`.

### Configuração de scrape

```yaml
scrape_configs:
  - job_name: netrack
    metrics_path: /api/prometheus/metrics
    scrape_interval: 1m
    static_configs:
      - targets: ["netrack.lan:5216"]
    basic_auth:          # só com senha definida
      username: prometheus
      password: <senha do netrack>
```

Exemplo de alerta, que dispara quando a média de download das últimas 24 h cai abaixo de
100 Mbps:

```yaml
- alert: NetrackSlowDownload
  expr: netrack_download_avg_24h_mbps < 100
  for: 1h
```

## OpenTelemetry

O netrack exporta **logs, traces e métricas** via OTLP/HTTP (protobuf, porta `4318`) para um
OpenTelemetry Collector ou qualquer backend compatível. Fica desligado até você definir um
endpoint; sem ele, os logs vão só para o stdout.

| Variável | Descrição |
|---|---|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | Endpoint base (ex.: `http://otel-collector:4318`); ativa os três sinais. `/v1/traces`, `/v1/metrics` e `/v1/logs` são acrescentados. |
| `OTEL_EXPORTER_OTLP_{TRACES,METRICS,LOGS}_ENDPOINT` | Endpoint de um único sinal (ativa só esse). |
| `OTEL_{TRACES,METRICS,LOGS}_EXPORTER=none` | Desativa um sinal. |
| `OTEL_SDK_DISABLED=true` | Desativa tudo. |
| `OTEL_SERVICE_NAME` | Nome do serviço (padrão `netrack`). |
| `OTEL_EXPORTER_OTLP_HEADERS`, `OTEL_EXPORTER_OTLP_TIMEOUT`, `OTEL_RESOURCE_ATTRIBUTES` | Configurações padrão do OpenTelemetry (cabeçalhos de autenticação, timeout, atributos de recurso). |

gRPC (`OTEL_EXPORTER_OTLP_PROTOCOL=grpc`) não é suportado.

O que é exportado:

- **Traces**: um span por requisição HTTP (dando continuidade a um `traceparent` recebido;
  `/healthz` é excluído), um por teste (`speedtest.run`) e um por tentativa do provedor
  (`speedtest.attempt`).
- **Logs**: todo evento de log (filtrado por `RUST_LOG`), correlacionado com o trace e o span
  ativos.
- **Métricas**: `netrack.speedtest.runs` (por resultado), `netrack.speedtest.degraded`,
  `netrack.speedtest.duration`, `netrack.speedtest.{download,upload,ping,jitter}` (último teste)
  e `http.server.request.duration` (só rotas da API, não arquivos estáticos).

Exemplo com Compose:

```yaml
services:
  netrack:
    image: ghcr.io/diaszano/netrack:latest
    network_mode: host
    environment:
      - TZ=America/Sao_Paulo
      - OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318
      - OTEL_RESOURCE_ATTRIBUTES=deployment.environment=home
    volumes:
      - ./data:/data
```

## Logs

Os logs vão para o stdout (`docker logs netrack`). Defina `RUST_LOG=debug` para mais detalhes, ou
um filtro como `RUST_LOG=netrack=debug,tower_http=info`.

## Imagem Open Graph

`GET /api/opengraph/image` retorna um card **SVG** de 1200×630 com o último teste bem-sucedido:
download, upload, ping, o servidor (ou provedor) e o horário, no fuso horário do servidor. Sem
testes ainda, mostra `--` e "Awaiting first test run". Fica em cache por 60 segundos.

Use-a onde for útil ter uma imagem sempre atualizada da sua conexão: uma página de status, um
painel, um badge no README (`![speed](https://netrack.example.com/api/opengraph/image)`). A
interface web não a adiciona às próprias tags `<meta>`. No modo **Protegido**, a imagem exige
login. Alguns apps de chat só aceitam PNG/JPEG nas prévias de link e ignoram SVG.
