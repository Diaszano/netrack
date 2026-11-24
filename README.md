<div align="center">

# netrack

**English** · [Português (Brasil)](README.pt-BR.md)

[![CI](https://img.shields.io/github/actions/workflow/status/Diaszano/netrack/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/Diaszano/netrack/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Diaszano/netrack?style=flat-square)](https://github.com/Diaszano/netrack/releases)
[![Platforms](https://img.shields.io/badge/Docker-amd64%20%7C%20arm64-2496ED?style=flat-square&logo=docker&logoColor=white)](https://github.com/Diaszano/netrack/pkgs/container/netrack)
[![License: MIT](https://img.shields.io/badge/License-MIT-green?style=flat-square)](LICENSE)

</div>

Self-hosted internet speed monitor written in Rust, with a responsive web UI, JSON HTTP API, and Prometheus/OpenTelemetry observability in a single static binary.

Runs scheduled speed tests (**Cloudflare**, **LibreSpeed**, or **Ookla Speedtest CLI**), stores history in SQLite, and ships as a ~11 MB `scratch` image for `linux/amd64` and `linux/arm64` — using under 8 MB of RAM at rest on an Orange Pi Zero 3.

![Overview](docs/images/en/overview.png)

## Features

- **Three providers:** native Cloudflare and LibreSpeed (public list, closest server or your own
  instance) engines, plus the Ookla Speedtest CLI, downloaded on demand after you accept its license.
- **Scheduling:** any cron expression, evaluated in your time zone, with an optional random offset
  to avoid everybody testing at `:00`. Pause and resume from the UI.
- **Web UI:** gauges against your plan, 7-day chart, weekday × hour heatmap, statistics for any
  period (availability, averages, std dev, consistency, per-server breakdown), full history.
  English and Portuguese, desktop and mobile. No CDN: fonts and charts are embedded.
- **Alerts:** Discord, Telegram, Pushover, Gotify, ntfy, generic webhook, InfluxDB and
  Healthchecks, on 8 events, including *slow* (`speedtest.degraded`) and *back to normal*
  (`speedtest.recovered`).
- **Observability:** Prometheus metrics, OpenTelemetry (OTLP/HTTP) logs, traces and metrics, and a
  dynamic Open Graph image with the latest result.
- **Multiple sites:** register other netrack instances as nodes and switch between them from one UI.
- **Your data:** JSON/CSV export and import (including MySpeed exports), daily SQLite backups with
  rotation, configurable retention.
- **Access control:** no password, public read-only, or fully protected (Argon2id, login lockout).

## Quick start

```bash
mkdir -p data && sudo chown 65534:65534 data
docker run -d --name netrack --network host \
  -e TZ=UTC \
  -v "$PWD/data:/data" \
  --restart unless-stopped \
  ghcr.io/diaszano/netrack:latest
```

Open `http://<host>:5216` (or check `curl http://localhost:5216/healthz`).

- Set `TZ` to your IANA time zone (for example, `America/Sao_Paulo` or `Europe/London`).
- The container runs as UID `65534` (`nobody`), so a bind-mounted `./data` directory must be owned by `65534:65534` (a named Docker volume works without `chown`).
- `--network host` lets netrack measure the host's real network interface on Linux. With bridge networking (or Docker Desktop), replace `--network host` with `-p 5216:5216`.

See [Installation](docs/en/installation.md) for Docker Compose, building from source, and running behind a reverse proxy.

## Screenshots

| Statistics | History |
|---|---|
| ![Statistics](docs/images/en/statistics.png) | ![History](docs/images/en/history.png) |
| **Servers** | **Integrations** |
| ![Servers](docs/images/en/servers.png) | ![Integrations](docs/images/en/integrations.png) |
| **Nodes** | **Settings** |
| ![Nodes](docs/images/en/nodes.png) | ![Settings](docs/images/en/settings.png) |

<p align="center"><img src="docs/images/en/mobile-overview.png" alt="Mobile overview" width="300"></p>

## Documentation

| Guide | What's in it |
|---|---|
| [Installation](docs/en/installation.md) | Docker, Compose, building from source, reverse proxy, upgrades |
| [Configuration](docs/en/configuration.md) | Environment variables and every runtime setting |
| [User guide](docs/en/user-guide.md) | Tour of every page in the web UI |
| [Integrations](docs/en/integrations.md) | Notification channels, events and payloads |
| [Security](docs/en/security.md) | Password modes, sessions, lockout, preview mode |
| [Data](docs/en/data.md) | Backups, retention, export, import, reset |
| [Multiple nodes](docs/en/nodes.md) | Monitoring several sites from one UI |
| [Observability](docs/en/observability.md) | Prometheus, OpenTelemetry, Open Graph image |
| [HTTP API](docs/en/api.md) | Every endpoint with examples |
| [Development](docs/en/development.md) | Building, testing, CI, releases, contributing |
| [Benchmarks](docs/en/benchmarks.md) | Accuracy and resource usage on an Orange Pi Zero 3 |
| [Changelog](CHANGELOG.md) | Release history and notable changes |

## Contributing

Bug reports, pull requests, and translations are welcome. See [Development](docs/en/development.md) for local setup, tests, and CI checks.

## License

Licensed under the [MIT License](LICENSE).
