# HTTP API

[← Documentation](../../README.md) · [Português](../pt-BR/api.md)

Everything the web UI does goes through this JSON API, so anything you can click you can also
script. Base URL: `http://<host>:5216`.

- Request and response bodies are JSON (`content-type: application/json`).
- Errors return a non-2xx status with `{"error": "message"}`.
- Timestamps are Unix **milliseconds**; speeds in Mbps, ping and jitter in ms, durations in
  seconds.
- In [preview mode](security.md#preview-mode), every non-GET request returns `403`.

## Authentication

Which routes need login depends on the protection mode; see [Security](security.md#protection-modes).
Log in once and reuse the token:

```bash
TOKEN=$(curl -s -X POST http://localhost:5216/api/auth/login \
  -H 'content-type: application/json' -d '{"password":"secret"}' | jq -r .token)

curl -H "Authorization: Bearer $TOKEN" http://localhost:5216/api/integrations
```

The login also sets the `netrack_session` cookie, which browsers use automatically.

| Method & path | Description |
|---|---|
| `GET /api/auth/status` | `{mode, authenticated}`: the protection mode and whether the caller is logged in. |
| `POST /api/auth/login` | Body `{"password"}`. Returns `{token, expiresAt}` and sets the cookie. `401` on a wrong password, `429` while locked out. |
| `POST /api/auth/logout` | Ends the current session. |
| `POST /api/auth/password` | Body `{password?, mode?, current_password?}`. Sets or removes the password and mode. |

## Health

| Method & path | Description |
|---|---|
| `GET /healthz` | `ok`. Always open; use it for container health checks. |

## Speed tests

| Method & path | Description |
|---|---|
| `GET /api/speedtests?limit=30&before=<id>` | Tests, newest first. `limit` 1–100 (default 30). `before` returns tests with an id lower than it (pagination). |
| `GET /api/speedtests/{id}` | One test, `404` if it doesn't exist. |
| `DELETE /api/speedtests/{id}` | Deletes a test (`204`). |
| `POST /api/speedtests/run` | Starts a manual test: `202 {"status":"started"}`, or `409` if one is already running. |
| `GET /api/speedtests/status` | `{running, paused, nextRun}`; `nextRun` is a timestamp or `null` when paused. |
| `POST /api/speedtests/pause` | Pauses the schedule. |
| `POST /api/speedtests/continue` | Resumes the schedule. |
| `GET /api/speedtests/statistics?from=YYYY-MM-DD&to=YYYY-MM-DD` | Statistics for a local date range (both required, inclusive). |
| `GET /api/speedtests/events` | Live events (Server-Sent Events). |

A test object:

```json
{
  "id": 1441,
  "createdAt": 1795553449536,
  "kind": "custom",
  "provider": "cloudflare",
  "serverId": "POA",
  "serverName": "Cloudflare · POA",
  "serverHost": "speed.cloudflare.com",
  "resultId": null,
  "ping": 12.4,
  "jitter": 1.8,
  "download": 214.3,
  "upload": 231.9,
  "duration": 21.6,
  "error": null
}
```

`kind` is `auto` (scheduled or startup) or `custom` (manual). A failed test has `error` set and
null metrics. `resultId` is the Ookla result id, when there is one.

### Running a test and waiting for it

```bash
curl -s -X POST http://localhost:5216/api/speedtests/run
while curl -s http://localhost:5216/api/speedtests/status | grep -q '"running":true'; do sleep 2; done
curl -s 'http://localhost:5216/api/speedtests?limit=1'
```

### Statistics

```bash
curl -s 'http://localhost:5216/api/speedtests/statistics?from=2026-11-01&to=2026-11-30'
```

```json
{
  "total": 720, "failed": 3, "availability": 99.6,
  "download": { "avg": 202.1, "min": 104.2, "max": 229.0, "stdDev": 27.2, "consistency": 86.5 },
  "upload":   { "...": "same shape" },
  "ping":     { "...": "same shape" },
  "jitter":   { "...": "same shape" },
  "hourly":   [{ "hour": 0, "download": 210.4, "upload": 230.1, "ping": 12.1, "jitter": 1.5, "count": 30 }],
  "heatmap":  [{ "weekday": 0, "hour": 21, "download": 141.7, "count": 4 }],
  "series":   [{ "t": 1795484729472, "download": 215.3, "upload": 261.5, "ping": 14.7 }],
  "byServer": [{ "provider": "cloudflare", "serverName": "Cloudflare · POA", "total": 600,
                 "failed": 2, "download": 201.0, "upload": 222.0, "ping": 13.7 }]
}
```

- Averages and the like use successful tests only; `availability` is the percentage of tests
  that succeeded (100 when there are none).
- `consistency` is `100 − stdDev / avg × 100`; `stdDev` is the population standard deviation.
- `hourly` always has 24 entries; `heatmap` only has cells with data (`weekday` 0 = Monday).
- `series` has at most 300 points; longer ranges are averaged into equal time buckets and `t`
  is each bucket's start.

### Live events

`GET /api/speedtests/events` is a Server-Sent Events stream. Each message's `data` is JSON with
a `type`:

```
data: {"type":"started","kind":"custom"}
data: {"type":"finished","id":1442,"metrics":{"ping":12.4,"jitter":1.8,"download":214.3,"upload":231.9}}
data: {"type":"degraded","id":1442,"metrics":{...},"expected_download":250.0,"expected_upload":250.0,"threshold_pct":70}
data: {"type":"recovered","id":1443,"metrics":{...}}
data: {"type":"failed","id":1444,"error":"all streams failed: ..."}
data: {"type":"expectedUpdated","ping":11.2,"download":229.0,"upload":251.0}
```

```bash
curl -N http://localhost:5216/api/speedtests/events
```

## Configuration

| Method & path | Description |
|---|---|
| `GET /api/config` | All runtime settings (camelCase keys). |
| `PATCH /api/config/{key}` | Body `{"value":"..."}` with the snake_case key. `204`, or `400` for an unknown key or invalid value. |

See [Configuration](configuration.md#runtime-settings) for every key.

## System

| Method & path | Description |
|---|---|
| `GET /api/system/status` | `{version, latest_version, update_available, preview_mode, preview_message, uptimeSecs, cpuPercent, rssBytes, tz}`. CPU and memory are `null` outside Linux. |
| `GET /api/system/interfaces` | Network interfaces and their addresses. |
| `GET /api/system/servers/librespeed` | Public LibreSpeed server list. |
| `GET /api/system/servers/ookla` | Ookla servers (`speedtest -L`), when the CLI is installed. |
| `GET /api/system/ookla/status` | `{installed, consented, arch}`. |
| `POST /api/system/ookla/consent` | Records acceptance of Ookla's license terms. |
| `POST /api/system/ookla/install` | Downloads the CLI (requires consent; `x86_64`/`aarch64` only). |

## Integrations

| Method & path | Description |
|---|---|
| `GET /api/integrations` | All integrations, secrets masked. |
| `POST /api/integrations` | Creates one (`201`). |
| `PUT /api/integrations/{id}` | Replaces one; `***` keeps a stored secret. |
| `DELETE /api/integrations/{id}` | Deletes one. |
| `POST /api/integrations/{id}/test` | Sends a test notification: `200 {success, message}` or `502`. |

Body and channel fields: [Integrations](integrations.md).

## Data

| Method & path | Description |
|---|---|
| `GET /api/data/stats` | `{rowCount, dbSizeBytes, backups: [{name, sizeBytes}]}`. |
| `GET /api/data/export?format=json\|csv&from=&to=` | Download the history (dates optional, local, inclusive). |
| `POST /api/data/import` | Body: the file contents (netrack JSON/CSV or MySpeed JSON). Returns `{imported, skipped}`. |
| `GET /api/data/backups/{name}` | Download a daily backup, e.g. `netrack-2026-11-24.db`. |
| `POST /api/data/reset` | Deletes all tests and restores default settings (password kept). |

Details: [Data](data.md).

## Nodes

| Method & path | Description |
|---|---|
| `GET /api/nodes` | Remote nodes (`{id, name, url, hasPassword, createdAt}`). |
| `POST /api/nodes` | Body `{name, url, password?}`. |
| `GET /api/nodes/{id}` | One node. |
| `PUT` / `PATCH /api/nodes/{id}` | Update; `"password": null` or `""` clears it, omitting it keeps it. |
| `DELETE /api/nodes/{id}` | Remove. |
| `GET /api/nodes/{id}/status` | `{online, version, latestTest, error}`, queried live. |
| `ANY /api/nodes/{id}/proxy/{path}` | Forwards to `<node>/api/{path}`. |

Details: [Multiple nodes](nodes.md).

## Metrics and images

| Method & path | Description |
|---|---|
| `GET /api/prometheus/metrics` | Prometheus text format. See [Observability](observability.md#prometheus). |
| `GET /api/opengraph/image` | 1200×630 SVG card with the latest result. |
