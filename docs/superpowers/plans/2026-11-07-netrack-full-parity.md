# netrack Full Parity (Plan 3 of 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete full feature parity with MySpeed: password protection (read-only vs full auth), on-demand Ookla CLI provider with license consent, notifications/integrations engine, Prometheus metrics, multi-node proxy, JSON/CSV data import/export (with MySpeed format support), Open Graph dynamic image, preview mode, and GitHub version checking.

**Architecture:** Extend the existing single-binary architecture with new SQLite tables (`integrations`, `nodes`, `auth_sessions`), background event subscriber for notifications, lazy on-demand runner for external Ookla CLI binaries stored under `<data_dir>/bin/`, basic auth/bearer cookie session middleware for Axum, and proxy client for remote netrack nodes. The embedded web UI is extended to wire up the existing seams for Nodes, Integrations, and new modals/settings.

**Tech Stack:** Rust 1.99.1 (`axum 0.8`, `tokio`, `rusqlite`, `reqwest`, `argon2` or `password-hash`, `resvg`/`image` or pure SVG-to-PNG for Open Graph). Vanilla ES2024 frontend. Node test runner (`node --test`) + `cargo test`.

**Spec:** `docs/superpowers/specs/2026-11-06-netrack-design.md` (Sections 5.2, 5.6–5.13)

---

## Global Constraints

- Single binary; no external processes bundled in the Docker image.
- Ookla CLI (`speedtest`) is **never** bundled: downloaded at runtime to `<data_dir>/bin/` only after explicit user license acceptance.
- Timezone: all logs, cron triggers, and time displays remain in the server's configured `TZ`.
- Password protection supports two levels:
  - `none`: public access.
  - `public`: read-only public access; running tests or modifying config requires password.
  - `protected`: everything requires password authentication (login screen).
  - Passwords hashed with `argon2`.
- Data import: supports native netrack JSON/CSV and MySpeed v1 JSON export.
- Remote Nodes: passwords for remote instances stored reversibly (encrypted/plain with clear warning) to allow the local instance to authenticate on behalf of the browser without CORS.
- Preview mode: `PREVIEW_MODE=true` env var displays configurable banner (`PREVIEW_MESSAGE`) and disables mutations.

---

## Review Focus

1. **Password Authentication & Middleware Leakage** — When `password_mode=protected`, no unauthenticated requests reach data endpoints (`/api/speedtests/*`, `/api/config/*`, `/api/system/*`), except `/api/auth/login`, `/healthz`, and public static assets. When `password_mode=public`, read endpoints work but mutating endpoints (`POST /api/speedtests/run`, `PATCH /api/config/*`, etc.) return 401.
2. **On-demand Ookla CLI Sandbox & Security** — License acceptance must be recorded in config before download. Binary download URL must be strictly pinned to official Ookla domains. Process execution must have strict timeout (60s) and bounded memory.
3. **Integration Event Notification Delivery Failure** — External webhook or notification targets (Discord, Telegram, etc.) failing or timing out must never block or fail the speedtest run. Failures are logged and surfaced in integration status.
4. **MySpeed Import Schema Mapping** — Importing an export from MySpeed must gracefully map fields (`download`, `upload`, `ping`, `jitter`, dates) and convert units accurately without corrupting existing history.
5. **Multi-Node Reverse Proxy SSRF Defense** — Registering a node URL must validate that it is an HTTP/HTTPS address and proxy requests safely without leaking server internals.

---

## File Structure

```
src/auth/mod.rs                 # password hashing, session tokens, axum middleware
src/auth/routes.rs              # /api/auth/login, /logout, /status
src/providers/ookla.rs          # on-demand downloader, license consent, CLI executor
src/integrations/mod.rs         # dispatcher subscribing to Runner broadcast events
src/integrations/channels/      # discord, telegram, pushover, gotify, ntfy, webhook, influxdb, healthchecks
src/integrations/routes.rs      # /api/integrations CRUD and test ping
src/prometheus.rs               # /api/prometheus/metrics exporter
src/nodes/mod.rs                # multi-node DB repo and reverse proxy client
src/nodes/routes.rs             # /api/nodes CRUD and proxy routing
src/data_transfer/mod.rs        # export/import JSON, CSV, MySpeed format converter
src/data_transfer/routes.rs     # /api/data/export, /import, /reset
src/opengraph.rs                # /api/opengraph/image dynamic PNG/SVG generator
src/version.rs                  # GitHub release version checker
web/views/auth.js               # login modal / screen
web/views/integrations.js       # full integrations management UI
web/views/nodes.js              # remote nodes management UI and multi-node switcher
web/views/data.js               # import/export/reset tab in Ajustes
```

---

### Task 1: Authentication & Password Protection Middleware

**Files:**
- Create: `src/auth/mod.rs`, `src/auth/routes.rs`
- Modify: `src/db/migrations.rs`, `src/api/mod.rs`, `src/db/config.rs`
- Test: `tests/auth.rs`

**Interfaces:**
- Migration: adds `password_hash`, `password_mode` (`none` | `public` | `protected`) to config or separate auth table, plus `sessions` table.
- Axum middleware: checks cookie/header for valid session token. Rejects unauthenticated requests with 401 JSON error based on `password_mode`.
- Routes: `POST /api/auth/login`, `POST /api/auth/logout`, `GET /api/auth/status`, `POST /api/auth/password` (change/remove password).

- [ ] **Step 1: Write failing integration tests** in `tests/auth.rs` verifying public, read-only, and fully protected modes.
- [ ] **Step 2: Run** `cargo test --test auth` → FAIL.
- [ ] **Step 3: Implement** password hashing (`argon2`), session store, middleware and auth endpoints.
- [ ] **Step 4: Run** `cargo test --test auth` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(auth): password protection levels, sessions and middleware"`

---

### Task 2: On-Demand Ookla CLI Provider & License Consent Flow

**Files:**
- Create: `src/providers/ookla.rs`
- Modify: `src/providers/mod.rs`, `src/api/system.rs`
- Test: `tests/ookla.rs`

**Interfaces:**
- `GET /api/system/ookla/status`: returns `{ installed: bool, consented: bool, arch: String }`.
- `POST /api/system/ookla/consent`: records user license acceptance (`ookla_license_agreed=true`).
- `POST /api/system/ookla/install`: downloads official Ookla CLI tarball for detected architecture (`x86_64` or `aarch64`) to `<data_dir>/bin/speedtest`, sets executable permissions.
- `GET /api/system/servers/ookla`: lists nearby Ookla servers via `speedtest -L --format=json`.
- `impl SpeedProvider for Ookla`: executes `speedtest -f json-pretty` with local interface binding and timeout.

- [ ] **Step 1: Write failing tests** for license requirement, download unpacking (mocked), and output parsing.
- [ ] **Step 2: Run** `cargo test --test ookla` → FAIL.
- [ ] **Step 3: Implement** Ookla downloader, license flow, and CLI JSON parser.
- [ ] **Step 4: Run** `cargo test --test ookla` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(providers): on-demand ookla cli download and execution"`

---

### Task 3: Notifications & Integrations Engine

**Files:**
- Create: `src/integrations/mod.rs`, `src/integrations/channels/*.rs`, `src/integrations/routes.rs`
- Modify: `src/db/migrations.rs`, `src/api/mod.rs`, `src/main.rs`
- Test: `tests/integrations.rs`

**Interfaces:**
- Migration: creates `integrations` table (`id`, `name`, `channel_type`, `config_json`, `events_mask`, `enabled`).
- Background worker: subscribes to `Runner::subscribe()` events + heartbeat tick; formats payloads and sends HTTP requests concurrently without stalling the runner.
- Channels: Discord (embed), Telegram (markdown), Pushover, Gotify, ntfy, Webhook (JSON payload), InfluxDB (line protocol), Healthchecks (ping start/fail/success).
- Endpoints: `GET /api/integrations`, `POST /api/integrations`, `PUT /api/integrations/{id}`, `DELETE /api/integrations/{id}`, `POST /api/integrations/{id}/test`.

- [ ] **Step 1: Write failing tests** in `tests/integrations.rs` simulating runner events and verifying dispatched HTTP requests against a local mock server.
- [ ] **Step 2: Run** `cargo test --test integrations` → FAIL.
- [ ] **Step 3: Implement** integration dispatcher and channel implementations.
- [ ] **Step 4: Run** `cargo test --test integrations` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(integrations): notification dispatcher and channel handlers"`

---

### Task 4: Prometheus Metrics Exporter

**Files:**
- Create: `src/prometheus.rs`
- Modify: `src/api/mod.rs`
- Test: `tests/prometheus.rs`

**Interfaces:**
- Route: `GET /api/prometheus/metrics`.
- Exposes standard prometheus text format metrics:
  - `netrack_ping_latency_ms`
  - `netrack_jitter_ms`
  - `netrack_download_speed_mbps`
  - `netrack_upload_speed_mbps`
  - `netrack_test_duration_seconds`
  - `netrack_last_test_timestamp_seconds`
  - Labels: `server_id`, `server_name`, `provider`.
- Protected by HTTP Basic Auth if password protection is enabled.

- [ ] **Step 1: Write failing tests** for metric output format and basic auth check.
- [ ] **Step 2: Run** `cargo test --test prometheus` → FAIL.
- [ ] **Step 3: Implement** Prometheus metrics formatter.
- [ ] **Step 4: Run** `cargo test --test prometheus` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(prometheus): prometheus metrics endpoint with basic auth"`

---

### Task 5: Data Transfer (Export, Import MySpeed, Database Reset)

**Files:**
- Create: `src/data_transfer/mod.rs`, `src/data_transfer/routes.rs`
- Modify: `src/api/mod.rs`
- Test: `tests/data_transfer.rs`

**Interfaces:**
- `GET /api/data/export?format=json|csv`: streams entire speedtest history.
- `POST /api/data/import`: accepts JSON file (netrack or MySpeed format); detects format, validates records, bulk-inserts into SQLite.
- `GET /api/data/stats`: returns DB file size in bytes and row count.
- `POST /api/data/reset`: clears tests history and restores default config (requires authentication).

- [ ] **Step 1: Write failing tests** for CSV export, JSON round-trip, MySpeed v1 JSON import parsing, and reset.
- [ ] **Step 2: Run** `cargo test --test data_transfer` → FAIL.
- [ ] **Step 3: Implement** data export streaming, MySpeed record mapper, and reset endpoints.
- [ ] **Step 4: Run** `cargo test --test data_transfer` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(data): export, import with myspeed compatibility and database reset"`

---

### Task 6: Multi-Node Management & Transparent Proxy

**Files:**
- Create: `src/nodes/mod.rs`, `src/nodes/routes.rs`
- Modify: `src/db/migrations.rs`, `src/api/mod.rs`
- Test: `tests/nodes.rs`

**Interfaces:**
- Migration: creates `nodes` table (`id`, `name`, `url`, `password_encrypted`, `created_at`).
- `GET /api/nodes`, `POST /api/nodes`, `PATCH /api/nodes/{id}`, `DELETE /api/nodes/{id}`.
- `GET /api/nodes/{id}/status`: queries remote node `/api/system/status` and `/api/speedtests?limit=1`.
- `ANY /api/nodes/{id}/proxy/*`: forwards request to remote node, appending credentials, streaming response back.

- [ ] **Step 1: Write failing tests** for node registration and proxy forwarding against mock instances.
- [ ] **Step 2: Run** `cargo test --test nodes` → FAIL.
- [ ] **Step 3: Implement** nodes repository and proxy handler.
- [ ] **Step 4: Run** `cargo test --test nodes` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(nodes): multi-instance node management and reverse proxy"`

---

### Task 7: Open Graph Image, Preview Mode, and Version Check

**Files:**
- Create: `src/opengraph.rs`, `src/version.rs`
- Modify: `src/api/mod.rs`, `src/settings.rs`
- Test: `tests/og_version.rs`

**Interfaces:**
- `GET /api/opengraph/image`: generates PNG or dynamic SVG with netrack logo, latest speed metrics, and timestamp.
- Settings/Env: `PREVIEW_MODE=true` and `PREVIEW_MESSAGE`.
- Version: periodically queries GitHub API (`https://api.github.com/repos/Diaszano/netrack/releases/latest`) in background, surfaces update availability in `/api/system/status`.

- [ ] **Step 1: Write failing tests** for image generation endpoint, preview mode mutation blocker, and version checker.
- [ ] **Step 2: Run** `cargo test --test og_version` → FAIL.
- [ ] **Step 3: Implement** image generator, preview middleware, and version check.
- [ ] **Step 4: Run** `cargo test --test og_version` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(system): opengraph image, preview mode and version check"`

---

### Task 8: Web UI Extension for Full Parity Features

**Files:**
- Create: `web/views/auth.js`, `web/views/integrations.js`, `web/views/nodes.js`
- Modify: `web/app.js`, `web/views/settings.js`, `web/views/servers.js`, `web/locales/pt-BR.json`, `web/locales/en.json`, `web/styles.css`
- Test: `web/tests/plan3_views.test.js`

**Interfaces:**
- Auth: login modal when required, password configuration card in Settings.
- Ookla: consent dialog when Ookla selected in Servers view, status indicator for binary installation.
- Integrations: full CRUD view replacing the seam placeholder in `web/views/seams.js`.
- Nodes: node switcher and cards replacing the seam placeholder in `web/views/seams.js`.
- Data: Import/Export/Reset section in Settings view.
- Update banner: subtle notification when new version is available.

- [ ] **Step 1: Write failing tests** in `web/tests/plan3_views.test.js`.
- [ ] **Step 2: Run** `node --test web/tests/plan3_views.test.js` → FAIL.
- [ ] **Step 3: Implement** all UI additions and locale updates.
- [ ] **Step 4: Run** `node --test web/tests/*.test.js && cargo test` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(web): wire auth, ookla consent, integrations, nodes and data views"`
