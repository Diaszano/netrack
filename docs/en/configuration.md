# Configuration

[← Documentation](../../README.md) · [Português](../pt-BR/configuration.md)

netrack has two kinds of configuration:

- **Environment variables**, read once at startup: port, data directory, time zone, proxy and
  cookie behavior, OpenTelemetry.
- **Runtime settings**, stored in the database and changed from the web UI (**Settings** and
  **Servers** pages) or the API (`PATCH /api/config/{key}`). They take effect immediately, no
  restart needed.

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `NETRACK_PORT` | `5216` | HTTP port. Listens on `0.0.0.0` and `[::]`. |
| `NETRACK_DATA_DIR` | `/data` | Directory for the SQLite database, daily backups and the optional Ookla binary. |
| `TZ` | `UTC` | IANA time zone (e.g. `America/Sao_Paulo`) used for the cron schedule, the 03:00 maintenance job, statistics (hour of day, weekday, date ranges) and backup file names. Empty means `UTC`. |
| `RUN_TEST_ON_STARTUP` | `false` | Run one test as soon as the server starts. |
| `NETRACK_TRUST_PROXY` | `false` | Use the last `X-Forwarded-For` value as the client IP for the login lockout. Enable only behind a trusted reverse proxy. |
| `NETRACK_SECURE_COOKIE` | `false` | Add `Secure` to the session cookie. Enable only when users reach netrack over HTTPS, otherwise browsers drop the cookie and login stops working. |
| `PREVIEW_MODE` | `false` | Demo mode: every change through the API is refused with `403` and the UI shows a banner. See [Security](security.md#preview-mode). |
| `PREVIEW_MESSAGE` | *(Portuguese default)* | Text of the preview banner and of the `403` error. |
| `RUST_LOG` | `info` | Log level/filter (`warn`, `info`, `debug`, `netrack=debug,tower_http=info`, ...). |
| `OTEL_*` | *(unset)* | OpenTelemetry export. See [Observability](observability.md#opentelemetry). |

Boolean variables accept only `true`/`1` and `false`/`0`; any other value (e.g. `yes`) stops
netrack at startup with an error, so typos don't go unnoticed.

> Changing `TZ` is safe: on the next start netrack recomputes the stored local hour and weekday
> of every test, so statistics and the heatmap follow the new zone.

## Runtime settings

All values are strings in the API (`{"value": "..."}`), and `GET /api/config` returns them in
camelCase (`expectedDownload`, ...). Each section title says where in the UI the settings are
edited.

### Schedule (Settings → Schedule)

| Key | Default | Valid values | Meaning |
|---|---|---|---|
| `cron` | `0 * * * *` | Cron expression with 5 fields (6 or 7 with seconds/years), or nicknames like `@hourly`, `@daily` | When automatic tests run, in `TZ`. |
| `schedule_offset` | `true` | `true`/`false` | Add a random delay to each run, between 0 and the smaller of 5 minutes and a quarter of the interval. Keeps many installations from hitting the test servers at the same second. |
| `paused` | `false` | `true`/`false` | Stop automatic tests. Manual tests still work. Same as the pause/continue API. |
| `retention_days` | `365` | 1–10000 | Tests older than this are deleted by the daily 03:00 job. |
| `backup_keep` | `7` | 0–30 | Number of daily backups to keep. `0` disables new backups (existing files are left alone). |

Cron examples:

| Expression | Runs |
|---|---|
| `0 * * * *` | Every hour |
| `*/30 * * * *` | Every 30 minutes |
| `0 */4 * * *` | Every 4 hours |
| `0 8-23 * * *` | Hourly from 08:00 to 23:00 |
| `@daily` | Once a day at midnight |

If a scheduled run comes up while another test is still running, it's skipped. An invalid
expression is reported in the logs and retried every minute.

### Internet plan (Settings → Internet plan)

These values feed the gauges ("% of plan") and the slowness alerts.

| Key | Default | Valid values | Meaning |
|---|---|---|---|
| `expected_mode` | `auto` | `auto`, `manual` | Where the expected values come from. |
| `expected_download` | `100` | number ≥ 0 (Mbps) | Contracted download speed. `0` disables download slowness checks. |
| `expected_upload` | `50` | number ≥ 0 (Mbps) | Contracted upload speed. `0` disables upload slowness checks. |
| `expected_ping` | `25` | number ≥ 0 (ms) | Reference latency, shown on the ping gauge. |
| `degraded_threshold_pct` | `70` | 1–100 | A test is *slow* when download **or** upload is below this percentage of the expected value. Ping is not considered. |

- **`manual`**: you type your plan's speeds; netrack never changes them.
- **`auto`**: after every successful test, once there are at least 10 successful tests, netrack
  sets expected download and upload to the **highest** and ping to the **lowest** value seen in
  the last 10 successful tests. The fields are read-only in the UI. Use it when you don't know
  your plan, but note that "% of plan" is then relative to recent best results, not to what you
  pay for.

### Provider (Servers page)

| Key | Default | Valid values | Meaning |
|---|---|---|---|
| `provider` | `cloudflare` | `cloudflare`, `librespeed`, `ookla` | Which engine runs the tests. |
| `libre_server_id` | `auto` | `auto` or a server id | LibreSpeed server from the public list. `auto` pings every listed server (2 s timeout) and picks the fastest to answer. |
| `libre_url` | `none` | `none` or an `http(s)://` URL | Your own LibreSpeed instance. Overrides `libre_server_id`. netrack uses its `garbage.php` and `empty.php` endpoints. |
| `ookla_server_id` | `auto` | `auto` or a server id | Ookla server (list comes from `speedtest -L`). `auto` lets the CLI choose. |
| `ookla_license_agreed` | `false` | `true`/`false` | Set by the consent button; required before the CLI can be installed. |
| `interface` | `auto` | `auto` or an interface name (e.g. `eth0`) | Bind test traffic to this interface's IPv4 address (or its first address if it has none). An interface that no longer exists falls back silently to the OS default; the Servers page flags it. |

How the providers measure:

- **Cloudflare** and **LibreSpeed** use netrack's own engine: 20 latency samples (ping = median,
  jitter = mean difference between consecutive samples), then 10 s of download and 10 s of
  upload. Each phase starts with 4 parallel connections and adds 4 more (up to 32) while the
  throughput keeps growing during a 2 s warm-up; only the traffic after the warm-up counts.
- **Ookla** runs the official CLI (`speedtest --format=json-pretty`) with a 60 s timeout. The
  CLI is not bundled: on the Servers page, **Accept terms & install Ookla CLI** records your
  consent to Ookla's EULA, privacy policy and terms of use, then downloads version 1.2.0 for
  `x86_64` or `aarch64` into `/data/bin/speedtest`.

A failed attempt is retried once before the test is recorded as failed.

### Language (Settings → Language)

The UI language is a per-browser preference (stored in `localStorage`), not a server setting.
**Auto** picks the first browser language that matches a shipped locale (`en`, `pt-BR`) and
falls back to `pt-BR`.

Adding a language means adding `web/locales/<code>.json` with a `_meta.name` entry and rebuilding:
locale files are embedded at compile time.

### Security (Settings → Security & Access)

`password_mode` and the password itself can't be changed through `PATCH /api/config`; use the
Security form or `POST /api/auth/password`. See [Security](security.md).

## Changing settings through the API

```bash
curl -X PATCH http://localhost:5216/api/config/cron \
  -H 'content-type: application/json' \
  -d '{"value":"*/30 * * * *"}'
```

Returns `204` on success or `400` with `{"error": "..."}` for an unknown key or invalid value.
Every change fires the `config.updated` integration event. With a password set, send the
session cookie or `Authorization: Bearer <token>` (see [HTTP API](api.md#authentication)).
