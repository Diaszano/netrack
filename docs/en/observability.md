# Observability

[← Documentation](../../README.md) · [Português](../pt-BR/observability.md)

## Prometheus

`GET /api/prometheus/metrics` exposes gauges in the Prometheus text format.

**Latest test.** Labelled with `provider`, `server_id` and `server_name`. Only present once at
least one test exists. If the latest test failed, the values are `0` and `netrack_test_success`
is `0`.

| Metric | Unit |
|---|---|
| `netrack_download_speed_mbps` | Mbps |
| `netrack_upload_speed_mbps` | Mbps |
| `netrack_ping_latency_ms` | ms |
| `netrack_jitter_ms` | ms |
| `netrack_test_duration_seconds` | s |
| `netrack_last_test_timestamp_seconds` | Unix time |
| `netrack_test_success` | 1 or 0 |

**Last 24 hours.** No labels; averages use successful tests only.

| Metric | Unit |
|---|---|
| `netrack_download_avg_24h_mbps` | Mbps |
| `netrack_upload_avg_24h_mbps` | Mbps |
| `netrack_ping_avg_24h_ms` | ms |
| `netrack_tests_24h{result="ok"\|"failed"}` | count (only present when there were tests) |

### Authentication

- Protection mode **Disabled**: open.
- **Public** or **Protected**: needs either a session (cookie or `Bearer` token) or HTTP Basic
  Auth with **any username** and the **netrack password**. Wrong passwords count toward the
  [login lockout](security.md#login-lockout). Failures get `401` with
  `WWW-Authenticate: Basic realm="netrack"`.

### Scrape configuration

```yaml
scrape_configs:
  - job_name: netrack
    metrics_path: /api/prometheus/metrics
    scrape_interval: 1m
    static_configs:
      - targets: ["netrack.lan:5216"]
    basic_auth:          # only with a password set
      username: prometheus
      password: <netrack password>
```

Example alert, firing when the last 24 h download average drops below 100 Mbps:

```yaml
- alert: NetrackSlowDownload
  expr: netrack_download_avg_24h_mbps < 100
  for: 1h
```

## OpenTelemetry

netrack exports **logs, traces and metrics** over OTLP/HTTP (protobuf, port `4318`) to an
OpenTelemetry Collector or any compatible backend. It's off until you set an endpoint; without
one, logs only go to stdout.

| Variable | Description |
|---|---|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | Base endpoint (e.g. `http://otel-collector:4318`); enables all three signals. `/v1/traces`, `/v1/metrics` and `/v1/logs` are appended. |
| `OTEL_EXPORTER_OTLP_{TRACES,METRICS,LOGS}_ENDPOINT` | Endpoint for one signal only (enables just that one). |
| `OTEL_{TRACES,METRICS,LOGS}_EXPORTER=none` | Disable one signal. |
| `OTEL_SDK_DISABLED=true` | Disable everything. |
| `OTEL_SERVICE_NAME` | Service name (default `netrack`). |
| `OTEL_EXPORTER_OTLP_HEADERS`, `OTEL_EXPORTER_OTLP_TIMEOUT`, `OTEL_RESOURCE_ATTRIBUTES` | Standard OpenTelemetry settings (auth headers, timeout, resource attributes). |

gRPC (`OTEL_EXPORTER_OTLP_PROTOCOL=grpc`) is not supported.

What's exported:

- **Traces**: one span per HTTP request (continuing an incoming `traceparent`; `/healthz` is
  excluded), one per test (`speedtest.run`) and one per provider attempt (`speedtest.attempt`).
- **Logs**: every log event (filtered by `RUST_LOG`), correlated with the active trace and span.
- **Metrics**: `netrack.speedtest.runs` (by result), `netrack.speedtest.degraded`,
  `netrack.speedtest.duration`, `netrack.speedtest.{download,upload,ping,jitter}` (latest test)
  and `http.server.request.duration` (API routes only, not static files).

Compose example:

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

Logs go to stdout (`docker logs netrack`). Set `RUST_LOG=debug` for more detail, or a filter
such as `RUST_LOG=netrack=debug,tower_http=info`.

## Open Graph image

`GET /api/opengraph/image` returns a 1200×630 **SVG** card with the latest successful test:
download, upload, ping, the server (or provider) and the time, in the server's time zone. With no
tests yet it shows `--` and "Awaiting first test run". It's cached for 60 seconds.

Use it wherever an always-current image of your connection is handy: a status page, a
dashboard, a README badge (`![speed](https://netrack.example.com/api/opengraph/image)`). The web
UI doesn't add it to its own `<meta>` tags. In **Protected** mode the image requires login. Some
chat apps only accept PNG/JPEG link previews and ignore SVG.
