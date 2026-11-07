# netrack Web UI + i18n (Plan 2 of 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Embed the complete "C · Painel denso" web interface and `pt-BR`/`en` internationalization into the `netrack` binary so the user can monitor, run, chart, and configure speed tests from any browser with live SSE updates and zero external CDN dependencies.

**Architecture:** `build.rs` scans `web/` at compile time, generates an embedded asset table plus a `/locales/index.json` catalog from `web/locales/*.json`, and compiles everything into the Rust binary (`src/web.rs`). The frontend is a fast, zero-framework ES module single-page app (`web/*.js`, `web/views/*.js`) with pure SVG chart/gauge renderers (`web/charts.js`) and `Intl` timezone-aware formatters (`web/format.js`) tested directly with Node's built-in `node --test` runner. Axum serves embedded static files and falls back to `index.html` on SPA navigation routes while keeping JSON `404`/`405` responses on `/api/*`.

**Tech Stack:** Rust 1.99.1 (`axum 0.8`, `build.rs` codegen), Vanilla ES2024 modules, CSS custom properties + Grid/Flexbox, inline SVG charts/gauges, embedded `.woff2` fonts (`Space Grotesk`, `Inter`, `JetBrains Mono`). Tests: `cargo test` (`tower::ServiceExt::oneshot`) for Rust HTTP/asset routing and `node --test` for JS modules.

**Spec:** `docs/superpowers/specs/2026-11-06-netrack-design.md` (Section 6 + UI for Sections 5.1–5.5, 5.13)

**Out of scope (Plan 3):** Ookla CLI install/consent flow, notification integrations CRUD, Prometheus endpoint, password auth screen/middleware, multi-node proxy CRUD, JSON/CSV import-export, Open Graph image, preview mode, GitHub release check. This plan leaves clean sidebar views (`Nós`, `Integrações`) and state hooks for Plan 3 to extend.

## Global Constraints

- Single binary; all HTML, CSS, JS, JSON locale files, and `.woff2` fonts are embedded at compile time with zero external CDN requests (works offline).
- Visual direction **"C · Painel denso"** (Penpot Page 1, board `C · Painel denso`):
  - Palette: background `#0E1020`, sidebar `#13162A`, active nav `#22264A`, card `#171A31`, border `#262A47`, text `#E8E9F5`, secondary `#8A8DB0`, download `#8B7CFF`, upload `#3FD6C2` (also `ok` badge and online dot), ping `#FF8FB1` (also `falha` badge).
  - Typography: `Space Grotesk` (600, titles and gauge numbers), `Inter` (400/500/600, UI copy), `JetBrains Mono` (400, table numbers, chart axes, percentages, server stats).
  - Sidebar navigation in exact order: `Visão geral`, `Estatísticas`, `Histórico`, `Servidores`, `Nós`, `Integrações`, `Ajustes`, with the server status card (`online`, `CPU X% · RAM Y MB`) pinned to the footer.
- Internationalization (`pt-BR` and `en` included): all UI strings live in `web/locales/<lang>.json` keyed by string; adding a new language requires only creating a new `web/locales/<lang>.json` file. Active locale follows `localStorage["netrack.locale"]` → `navigator.languages` → `"pt-BR"`, with a selector in `Ajustes`.
- All dates, times, hourly buckets, heatmap axes, and `from`/`to` statistics queries use the server's timezone (`tz` from `GET /api/system/status`), formatted in the active locale.
- Responsive layout: 220px fixed sidebar + multi-column cards on desktop (`≥ 900px`); top bar/drawer + stacked cards on mobile (`< 900px`).
- Live updates without page reload via `EventSource("/api/speedtests/events")`.
- Repo rules: branch names `<type>/<kebab-case>`; commit messages must NOT contain AI co-author trailers.

## Review Focus

1. **Empty database (0 speedtests run yet)** — Overview, Statistics, and History must render clean `"—"` placeholders and empty-state messages without `NaN`, `Infinity`, division-by-zero (`0 / 0`), or broken SVG path coordinates. → Task 3 `gauge_and_charts_handle_empty_and_zero_expected` + Task 5 `overview_and_stats_render_when_history_is_empty`.
2. **Timezone mismatch between browser and server** — A browser in `UTC` or `America/New_York` viewing a server with `TZ=America/Sao_Paulo` must display all table timestamps, chart tooltips, and default `from`/`to` date-range queries in the server's `tz`, never the browser's local timezone. → Task 2 `formatters_and_presets_use_server_tz`.
3. **XSS via untrusted provider/server/error strings** — A custom LibreSpeed server name, host, or failure error containing `<script>` or quotes is rendered into HTML/SVG; all dynamic strings must be HTML-escaped before insertion. → Task 5 `history_and_recent_escape_html_in_server_and_error`.
4. **Config key casing and validation errors on Save** — `GET /api/config` returns camelCase (`scheduleOffset`, `retentionDays`, `expectedMode`, `libreServerId`, `libreUrl`), whereas `PATCH /api/config/{key}` requires snake_case keys (`schedule_offset`, `retention_days`, `expected_mode`, `libre_server_id`, `libre_url`) and string values (`"true"`, `"365"`, `"auto"`, `"none"`), returning HTTP 400 `{ "error": "..." }` on invalid input; the UI must map keys accurately and surface the 400 error message without losing form state. → Task 6 `config_patch_maps_camel_to_snake_and_surfaces_400`.
5. **Unknown `/api/...` routes vs SPA fallback** — Adding the web UI fallback must not turn unknown `/api/...` paths or bad asset URLs into `200 index.html`; `/api/nope` must still return `404 { "error": "not found" }` and `GET /missing.js` must return `404`, while SPA routes (`GET /`, `GET /statistics`, `GET /settings`) return `index.html`. → Task 1 `spa_routes_serve_html_while_unknown_api_and_assets_404`.

---

## File Structure

```
build.rs                       # walks web/, embeds assets + generates /locales/index.json
Dockerfile                     # copies build.rs and web/ into builder stage
src/lib.rs                     # adds pub mod web
src/web.rs                     # embedded asset lookup + Axum handler + SPA fallback rules
src/api/mod.rs                 # wires web::fallback into router while keeping /api/* 404 JSON
tests/web_assets.rs            # integration tests for asset serving, headers, locales, SPA vs API 404
web/index.html                 # app shell (sidebar, topbar, main view mount, toast/banner)
web/styles.css                 # "C · Painel denso" theme, @font-face, desktop grid + mobile responsive
web/fonts/                     # embedded woff2 subsets: Space Grotesk 600, Inter 400/500/600, JetBrains Mono 400
web/locales/pt-BR.json         # Brazilian Portuguese translation catalog
web/locales/en.json            # English translation catalog
web/i18n.js                    # locale resolution, translator t(key, vars), HTML escaping
web/format.js                  # Intl number/date/time/bytes/plan-% formatters in server TZ
web/charts.js                  # pure SVG generators: gauge card, speed line/area chart, heatmap, hourly bars
web/api.js                     # fetch wrappers for /api/* + CONFIG_KEY_MAP (camelCase -> snake_case)
web/state.js                   # app store + SSE RunEvent reducer
web/views/overview.js          # Visão geral view (3 gauges, 7d speed chart, heatmap, recent table)
web/views/statistics.js        # Estatísticas view (date range/presets, summaries, series, hourly, heatmap)
web/views/history.js           # Histórico view (cursor pagination, delete row, status/error details)
web/views/servers.js           # Servidores view (provider, LibreSpeed server/custom URL, network interface)
web/views/settings.js          # Ajustes view (cron, offset, pause, retention, expected plan, language)
web/views/seams.js             # Nós (local server status card + Plan 3 seam) & Integrações empty state
web/app.js                     # browser entrypoint: router, SSE lifecycle, 15s system status poll
web/tests/*.test.js            # Node built-in test runner suites (node --test)
```

---

### Task 1: Embedded asset pipeline, fonts, and SPA/API routing

**Files:**
- Create: `build.rs`, `src/web.rs`, `web/index.html`, `web/styles.css`, `web/app.js`, `web/locales/pt-BR.json`, `web/locales/en.json`, `web/fonts/space-grotesk-latin.woff2`, `web/fonts/inter-latin.woff2`, `web/fonts/jetbrains-mono-latin.woff2`
- Modify: `src/lib.rs`, `src/api/mod.rs`, `Dockerfile`
- Test: `tests/web_assets.rs`

**Interfaces:**
- Consumes: `crate::api::{AppState, ApiError, router}`.
- Produces:
  - `build.rs`: walks `web/` (ignoring `web/tests/`), emits `cargo:rerun-if-changed=web`, writes `$OUT_DIR/web_assets.rs` with `pub static ASSETS: &[(&str, &str, &[u8])]` (`(url_path, content_type, bytes)`) and synthesizes `("/locales/index.json", "application/json; charset=utf-8", ...)` by reading `"_meta": { "name": "..." }` from every `web/locales/<code >.json` sorted by `code`.
  - `src/web.rs`: `pub fn get_asset(path: &str) -> Option<(&'static str, &'static [u8])>` and `pub(crate) async fn serve(uri: axum::http::Uri, method: axum::http::Method) -> axum::response::Response`.
  - Routing rules in `web::serve`:
    1. Any path starting with `/api/` or equal to `/api` returns `ApiError::new(StatusCode::NOT_FOUND, "not found")` (`application/json`).
    2. Only `GET` and `HEAD` are allowed; other methods return `ApiError::new(StatusCode::METHOD_NOT_ALLOWED, "method not allowed")`.
    3. Exact match in `ASSETS` (e.g. `/styles.css`, `/app.js`, `/locales/pt-BR.json`, `/locales/index.json`, `/fonts/space-grotesk-latin.woff2`) returns `200` with `content-type` and `cache-control` (`public, max-age=31536000, immutable` for `/fonts/*`, `no-cache` for HTML/JS/CSS/JSON).
    4. Any other path containing a `.` in its last segment (e.g. `/missing.js`, `/fonts/nope.woff2`) returns `404` JSON error.
    5. SPA paths without an extension (`/`, `/overview`, `/statistics`, `/history`, `/servers`, `/nodes`, `/integrations`, `/settings`) return `index.html` (`200`, `text/html; charset=utf-8`).

- [ ] **Step 1: Write failing tests** in `tests/web_assets.rs`:
  - `root_and_spa_routes_serve_index_html`: `GET /`, `GET /statistics`, `GET /settings` → `200`, `content-type` starts with `text/html`, body contains `<div id="app">`.
  - `static_assets_and_fonts_served_with_mime_types`: `GET /styles.css` → `text/css`, `GET /app.js` → `text/javascript`, `GET /fonts/space-grotesk-latin.woff2` → `font/woff2` with `immutable` cache header and non-empty body.
  - `locales_index_lists_discovered_languages`: `GET /locales/index.json` → JSON array containing `{ "code": "en", "name": "English" }` and `{ "code": "pt-BR", "name": "Português (Brasil)" }`.
  - `spa_routes_serve_html_while_unknown_api_and_assets_404`: `GET /api/nope` → `404` `application/json` `{ "error": "not found" }`; `GET /missing.js` → `404`; `POST /` → `405` `application/json`.
- [ ] **Step 2: Run** `cargo test --test web_assets` → FAIL (module `web` does not exist).
- [ ] **Step 3: Implement** `build.rs`, `src/web.rs`, wire `fallback(web::serve)` in `src/api/mod.rs`, add initial `web/index.html`, `web/styles.css`, `web/app.js`, minimal `web/locales/{pt-BR,en}.json` with `_meta.name`, bundle the 3 `.woff2` font files in `web/fonts/`, and update `Dockerfile` to copy `build.rs` and `web/`.
- [ ] **Step 4: Run** `cargo test --test web_assets && cargo test --test api` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(web): embed static assets, fonts and locale index"`

---

### Task 2: i18n engine, complete `pt-BR`/`en` catalogs, and timezone formatters

**Files:**
- Create: `web/i18n.js`, `web/format.js`, `web/tests/i18n_format.test.js`
- Modify: `web/locales/pt-BR.json`, `web/locales/en.json`
- Test: `web/tests/i18n_format.test.js`

**Interfaces:**
- Produces in `web/i18n.js`:
  - `export function escapeHtml(str)` — escapes `&`, `<`, `>`, `"`, `'` (coerces non-strings safely).
  - `export function resolveLocale({ saved, navigatorLanguages = [], available = ["pt-BR", "en"], defaultLocale = "pt-BR" }) -> string` — returns `saved` if in `available` (and not `"auto"`), else first `navigatorLanguages` entry matching an `available` code (exact case-insensitive match first, then primary subtag e.g. `"pt-PT"` → `"pt-BR"`, `"en-US"` → `"en"`), else `defaultLocale`.
  - `export function createTranslator(dict, fallbackDict = {}) -> (key: string, vars?: Record<string, string | number>) => string` — looks up `dict[key] ?? fallbackDict[key] ?? key` and replaces `{name}` placeholders with `String(vars[name])`.
- Produces in `web/format.js`:
  - `export function formatNumber(value, locale, decimals = 0) -> string` — returns `"—"` if `value == null || !Number.isFinite(value)`, else `Intl.NumberFormat(locale, { minimumFractionDigits: decimals, maximumFractionDigits: decimals }).format(value)`.
  - `export function formatMetric(value, locale, metric /* "download"|"upload"|"ping"|"jitter" */) -> string` — `0` decimals for integer-like or `≥ 100` Mbps/ms on gauges, `1` decimal for jitter (`1,8` in `pt-BR`), `2` decimals in detailed tables when needed.
  - `export function formatTime(ms, locale, tz) -> string` — `HH:MM` in `tz` (`hour: "2-digit", minute: "2-digit", hour12: false, timeZone: tz`).
  - `export function formatDateTime(ms, locale, tz) -> string` — short date + `HH:MM` in `tz`.
  - `export function formatDateInput(ms, tz) -> string` — `YYYY-MM-DD` calendar date of `ms` in `tz` (using `Intl.DateTimeFormat("en-CA", { timeZone: tz, year: "numeric", month: "2-digit", day: "2-digit" })`).
  - `export function dateRangeForPreset(preset /* "24h"|"7d"|"30d"|"365d" */, nowMs, tz) -> { from: string, to: string }` — `to` is today in `tz`; `from` is `to` minus `0` (`24h` = today..today or yesterday..today: `1`, `6`, `29`, `364` calendar days in `tz`).
  - `export function formatBytes(bytes, locale) -> string` — `"—"` on null, else `X MB` / `X,X GB`.
  - `export function computePlanProgress(actual, expected, metric /* "download"|"upload"|"ping" */) -> { percent: number | null, ratio: number }` — for `download`/`upload`: if `!Number.isFinite(actual) || !Number.isFinite(expected) || expected <= 0` returns `{ percent: null, ratio: 0 }`, else `percent = Math.round((actual / expected) * 100)` and `ratio = Math.min(1, Math.max(0, actual / expected))`. For `ping`: if `actual <= 0 || expected <= 0` returns `{ percent: null, ratio: 0 }`, else `ratio = Math.min(1, Math.max(0, expected / actual))`.

- [ ] **Step 1: Write failing tests** in `web/tests/i18n_format.test.js`:
  - `locale_catalogs_have_identical_keys`: reads `web/locales/pt-BR.json` and `web/locales/en.json`, asserts identical sorted key lists and non-empty string values.
  - `resolve_locale_priority_and_subtag_fallback`: `saved: "en"` → `"en"`; `saved: "auto", navigatorLanguages: ["pt-PT", "en-US"]` → `"pt-BR"`; `navigatorLanguages: ["fr-FR"]` → `"pt-BR"`.
  - `translator_interpolates_and_falls_back`: `t("overview.planPercent", { pct: 98 })` → `"98% do plano"` in `pt-BR` and `"98% of plan"` in `en`.
  - `formatters_and_presets_use_server_tz`: for `2026-03-10T01:30:00Z` (`1773106200000`), `formatTime(ms, "pt-BR", "America/Sao_Paulo")` === `"22:30"` and `formatDateInput(ms, "America/Sao_Paulo")` === `"2026-03-09"`, regardless of host OS TZ; `formatNumber(1.8, "pt-BR", 1)` === `"1,8"`; `formatNumber(null, "pt-BR", 1)` === `"—"`.
  - `plan_progress_handles_zero_and_null_expected`: `computePlanProgress(587, 600, "download")` → `{ percent: 98, ratio: 587/600 }`; `computePlanProgress(587, 0, "download")` → `{ percent: null, ratio: 0 }`.
- [ ] **Step 2: Run** `node --test web/tests/i18n_format.test.js` → FAIL.
- [ ] **Step 3: Implement** `web/i18n.js`, `web/format.js`, and populate the complete key catalogs in `web/locales/pt-BR.json` and `web/locales/en.json`.
- [ ] **Step 4: Run** `node --test web/tests/i18n_format.test.js` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(i18n): locale resolution, pt-BR/en catalogs and server-tz formatters"`

---

### Task 3: SVG gauges, speed area chart, weekday×hour heatmap, and hourly chart

**Files:**
- Create: `web/charts.js`, `web/tests/charts.test.js`
- Test: `web/tests/charts.test.js`

**Interfaces:**
- Consumes: `escapeHtml` (`web/i18n.js`), `formatNumber`, `formatTime`, `formatDateTime`, `computePlanProgress` (`web/format.js`).
- Produces in `web/charts.js`:
  - `export function renderGaugeCard({ metric /* "download"|"upload"|"ping" */, value, unit, color, expected, avg, min, max, jitter, locale, t }) -> string`:
    - Renders `.gauge-card` with a `120×104` SVG 240° arc gauge (`#262A47` track `stroke-width="10" stroke-linecap="round"`, value path `stroke="${color}"` with `stroke-dasharray` proportional to `ratio`), centered big number (`Space Grotesk` 32px, `"—"` when `value == null`) and `unit` (`Mbps` / `ms`), and right-hand metadata: metric title, subtitle (`{pct}% do plano` for download/upload or `jitter {val}` for ping), `méd {avg}`, and `mín {min}` (download/upload) or `máx {max}` (ping).
  - `export function renderSpeedChart(series, { locale, tz, t }) -> string`:
    - Takes `SeriesPoint[]` (`{ t, download, upload, ping }`). Renders `<svg viewBox="0 0 612 210" class="speed-chart">` with 4 horizontal grid lines (`#262A47`), download area (`fill="#8B7CFF" fill-opacity="0.15"`) + stroke (`stroke="#8B7CFF" stroke-width="2"`), upload area (`fill="#3FD6C2" fill-opacity="0.12"`) + stroke (`stroke="#3FD6C2" stroke-width="2"`), X-axis time labels in `tz`, and `.chart-point` circles with `data-t`, `data-dl`, `data-ul`, `data-ping` for hover tooltips. When `series.length === 0`, renders a `.chart-empty` message inside the card. When `series.length === 1`, renders a single horizontal sample with visible dots and valid finite SVG coordinates (no `NaN`).
  - `export function renderHeatmap(cells, { locale, t }) -> string`:
    - Takes `HeatCell[]` (`{ weekday: 0..6, hour: 0..23, download, count }`). Renders 7 weekday rows (`0=Mon..6=Sun`, labels from `t("weekdays.short")` → `["S","T","Q","Q","S","S","D"]` in `pt-BR`, `["M","T","W","T","F","S","S"]` in `en`) × 24 hour cells (`00h..23h`, bottom axis labels `00h`, `06h`, `12h`, `18h`). Populated cells get `background: rgba(139, 124, 255, <0.18..1.0>)` scaled by `cell.download / maxDownload`; empty cells get `#22264A`.
  - `export function renderHourlyChart(hourly, { locale, t }) -> string`:
    - Takes `HourBucket[24]` (`{ hour: 0..23, download, upload, ping, jitter, count }`). Renders a 24-group SVG bar chart comparing average `download` (`#8B7CFF`) and `upload` (`#3FD6C2`) per hour of day.

- [ ] **Step 1: Write failing tests** in `web/tests/charts.test.js`:
  - `gauge_card_renders_penpot_metrics_and_plan_pct`: `renderGaugeCard({ metric: "download", value: 587, unit: "Mbps", color: "#8B7CFF", expected: 600, avg: 552, min: 371, locale: "pt-BR", t })` contains `"587"`, `"Mbps"`, `"98% do plano"`, `"méd 552"`, `"mín 371"`, and `#8B7CFF`.
  - `ping_gauge_renders_jitter_and_max`: `renderGaugeCard({ metric: "ping", value: 11, unit: "ms", color: "#FF8FB1", expected: 25, avg: 14, max: 31, jitter: 1.8, locale: "pt-BR", t })` contains `"jitter 1,8"`, `"méd 14"`, `"máx 31"`.
  - `gauge_and_charts_handle_empty_and_zero_expected`: `renderGaugeCard` with `value: null, expected: 0`, `renderSpeedChart([], ...)`, `renderSpeedChart([singlePoint], ...)`, `renderHeatmap([], ...)`, `renderHourlyChart(empty24, ...)` never output `"NaN"` or `"Infinity"`.
  - `heatmap_maps_weekday_hour_and_scales_opacity`: `renderHeatmap([{ weekday: 0, hour: 14, download: 500, count: 3 }], ...)` marks `(0, 14)` with `500` Mbps in its tooltip and renders 168 (`7 × 24`) cells.
- [ ] **Step 2: Run** `node --test web/tests/charts.test.js` → FAIL.
- [ ] **Step 3: Implement** `web/charts.js`.
- [ ] **Step 4: Run** `node --test web/tests/charts.test.js` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(web): svg gauge, speed area chart, heatmap and hourly chart"`

---

### Task 4: API client, SSE live state reducer, and "C · Painel denso" shell styling

**Files:**
- Create: `web/api.js`, `web/state.js`, `web/tests/state.test.js`
- Modify: `web/index.html`, `web/styles.css`
- Test: `web/tests/state.test.js`

**Interfaces:**
- Produces in `web/api.js`:
  - `export class ApiClientError extends Error { constructor(status, message) }`
  - `export const CONFIG_KEY_MAP = { provider: "provider", cron: "cron", scheduleOffset: "schedule_offset", retentionDays: "retention_days", expectedMode: "expected_mode", expectedPing: "expected_ping", expectedDownload: "expected_download", expectedUpload: "expected_upload", libreServerId: "libre_server_id", libreUrl: "libre_url", interface: "interface", paused: "paused" }`
  - `export function serializeConfigValue(key, val) -> string` — converts `null`/`""` on `libreServerId`/`interface` to `"auto"`, `null`/`""` on `libreUrl` to `"none"`, booleans to `"true"`/`"false"`, numbers/strings to `String(val)`.
  - `export function createApiClient(fetchImpl = globalThis.fetch)` with methods: `getStatus()`, `runSpeedtest()`, `pause()`, `resume()`, `listSpeedtests({ before, limit } = {})`, `deleteSpeedtest(id)`, `getStatistics(from, to)`, `getConfig()`, `patchConfig(fieldOrKey, rawValue)`, `getSystemStatus()`, `getInterfaces()`, `getLibreServers()`, `getLocales()`, `getLocaleDict(code)`.
- Produces in `web/state.js`:
  - `export function createInitialState() -> AppState` (`route: "overview"`, `locale: "pt-BR"`, `status: { running: false, paused: false, nextRun: null }`, `system: { version: "—", uptimeSecs: 0, cpuPercent: null, rssBytes: null, tz: "UTC" }`, `config: null`, `recentTests: []`, `overviewStats: null`, `statsRange: { preset: "7d", from: "", to: "" }`, `statsData: null`, `history: { rows: [], hasMore: false, loading: false }`, `servers: { interfaces: [], libreServers: [], libreError: null }`, `banner: null`).
  - `export function applyRunEvent(state, event)` — pure state transition for `RunEvent`:
    - `{ type: "started" }` → returns `{ nextState: { ...state, status: { ...state.status, running: true } }, shouldRefresh: false }`.
    - `{ type: "finished", id, metrics }` → returns `{ nextState: { ...state, status: { ...state.status, running: false } }, shouldRefresh: true }`.
    - `{ type: "failed", id, error }` → returns `{ nextState: { ...state, status: { ...state.status, running: false }, banner: { kind: "error", text: error } }, shouldRefresh: true }`.
    - `{ type: "expectedUpdated", ping, download, upload }` → updates `state.config.expectedPing/expectedDownload/expectedUpload` if `state.config` is loaded, `shouldRefresh: false`.
  - `export function parseRoute(pathname) -> "overview"|"statistics"|"history"|"servers"|"nodes"|"integrations"|"settings"` (defaults unknown paths to `"overview"`).

- [ ] **Step 1: Write failing tests** in `web/tests/state.test.js`:
  - `apply_run_event_transitions_running_and_triggers_refresh`: `started` sets `running: true` (`shouldRefresh: false`); `finished` sets `running: false` (`shouldRefresh: true`); `failed` sets `running: false`, populates `banner`, and sets `shouldRefresh: true`; `expectedUpdated` updates `config.expectedDownload/Upload/Ping` in place.
  - `serialize_config_value_and_key_map`: `CONFIG_KEY_MAP.scheduleOffset === "schedule_offset"`, `serializeConfigValue("libreServerId", null) === "auto"`, `serializeConfigValue("libreUrl", "") === "none"`, `serializeConfigValue("scheduleOffset", true) === "true"`.
  - `api_client_parses_json_error_on_400_and_409`: mock `fetchImpl` returning `409` `{ "error": "test already running" }` rejects with `ApiClientError` where `err.status === 409` and `err.message === "test already running"`.
- [ ] **Step 2: Run** `node --test web/tests/state.test.js` → FAIL.
- [ ] **Step 3: Implement** `web/api.js`, `web/state.js`, and complete `web/index.html` + `web/styles.css` with the full `"C · Painel denso"` layout, sidebar, mobile drawer, cards, tables, badges, and form controls.
- [ ] **Step 4: Run** `node --test web/tests/state.test.js` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(web): api client, sse state reducer and dense panel layout"`

---

### Task 5: Overview, Statistics, and History views

**Files:**
- Create: `web/views/overview.js`, `web/views/statistics.js`, `web/views/history.js`, `web/tests/views_data.test.js`
- Test: `web/tests/views_data.test.js`

**Interfaces:**
- Consumes: `renderGaugeCard`, `renderSpeedChart`, `renderHeatmap`, `renderHourlyChart` (`web/charts.js`), `formatNumber`, `formatTime`, `formatDateTime`, ` escapeHtml` (`web/format.js`, `web/i18n.js`).
- Produces:
  - `export function renderOverview(state, { t }) -> string` in `web/views/overview.js`:
    - Header with `Visão geral` title, pause/next-run status pill, and `▶ Testar agora` button (`data-action="run-test"`, disabled when `state.status.running`).
    - 3 gauge cards (`Download`, `Upload`, `Ping`) populated from the newest non-failed row in `state.recentTests` (or `null` if none), `state.config.expected*`, and 7-day `state.overviewStats`.
    - Middle grid: `Velocidade · 7 dias` (`renderSpeedChart`) + `Download por dia e hora` (`renderHeatmap`).
    - Bottom card: `Testes recentes` table (up to 10 rows from `state.recentTests`, columns: time in `state.system.tz`, download, upload, ping, provider/server, badge `ok` in `#3FD6C2` or `falha` in `#FF8FB1` with escaped error `title` attribute; failed rows display `—` for metrics).
  - `export function renderStatistics(state, { t }) -> string` in `web/views/statistics.js`:
    - Period bar: preset buttons (`24h`, `7d`, `30d`, `365d` with `data-action="stats-preset"`) + `<input type="date" name="from">` and `<input type="date" name="to">`.
    - KPI summary strip: `total`, `failed`, `availability%`.
    - 4 metric summary cards (`Download`, `Upload`, `Ping`, `Jitter`) displaying `avg`, `min`, `max`, `stdDev`, and `consistency%`.
    - Charts grid: period speed chart (`renderSpeedChart`), 24h average by hour (`renderHourlyChart`), and weekday×hour heatmap (`renderHeatmap`).
  - `export function renderHistory(state, { t }) -> string` in `web/views/history.js`:
    - Table of `state.history.rows` with columns: Data/Hora (in `state.system.tz`), Tipo (`auto`/`custom`), Provedor & Servidor (`provider`, `serverName`, `serverHost`), Ping, Jitter, Download, Upload, Duração (`duration` s), Status (`ok`/`falha` + escaped error text), and delete button (`data-action="delete-test" data-id="${row.id}"`).
    - Pagination footer: `Carregar mais` button (`data-action="load-more-history"`) shown when `state.history.hasMore` is true.

- [ ] **Step 1: Write failing tests** in `web/tests/views_data.test.js`:
  - `overview_renders_gauges_chart_heatmap_and_recent_rows`: with 1 ok row (`download: 587, upload: 274, ping: 11, jitter: 1.8, provider: "cloudflare"`) and 1 failed row (`error: "timeout"`), `renderOverview` outputs `"587"`, `"274"`, `"11"`, `"ok"`, `"falha"`, and `"—"`.
  - `overview_and_stats_render_when_history_is_empty`: with `recentTests: []` and `overviewStats.total === 0`, `renderOverview` and `renderStatistics` render clean empty states without `"NaN"` or `"undefined"`.
  - `history_and_recent_escape_html_in_server_and_error`: a row with `serverName: "<img src=x onerror=alert(1)>"` and `error: "<script>evil()</script>"` has `&lt;img` and `&lt;script&gt;` in both `renderOverview` and `renderHistory`, never raw `<script>`.
  - `history_shows_load_more_only_when_has_more`: `hasMore: true` renders `data-action="load-more-history"`; `hasMore: false` omits it.
- [ ] **Step 2: Run** `node --test web/tests/views_data.test.js` → FAIL.
- [ ] **Step 3: Implement** `web/views/overview.js`, `web/views/statistics.js`, and `web/views/history.js`.
- [ ] **Step 4: Run** `node --test web/tests/views_data.test.js` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(web): overview, statistics and history views"`

---

### Task 6: Servers, Settings, Nodes/Integrations seams, and full app wiring

**Files:**
- Create: `web/views/servers.js`, `web/views/settings.js`, `web/views/seams.js`, `web/tests/views_config.test.js`
- Modify: `web/app.js`, `tests/web_assets.rs`
- Test: `web/tests/views_config.test.js`, `tests/web_assets.rs`

**Interfaces:**
- Consumes: `createApiClient`, `CONFIG_KEY_MAP`, `serializeConfigValue` (`web/api.js`), `createInitialState`, `applyRunEvent`, `parseRoute` (`web/state.js`), all views (`web/views/*.js`).
- Produces:
  - `export function renderServers(state, { t }) -> string` in `web/views/servers.js`:
    - Provider selection cards (`cloudflare`, `librespeed`).
    - LibreSpeed section (enabled/highlighted when `provider === "librespeed"`): public server dropdown (`auto` + `state.servers.libreServers` items `${id} — ${name}`), custom URL input (`libreUrl`), and graceful warning banner if `state.servers.libreError` is set (e.g. offline/502).
    - Network interface selector (`auto` + `state.servers.interfaces` items `${name} (${addrs.join(", ")})`).
  - `export function renderSettings(state, { t, locales }) -> string` in `web/views/settings.js`:
    - Agendamento card: `cron` input, `scheduleOffset` checkbox, `paused` toggle, `retentionDays` number input (`1..10000`).
    - Plano / Valores esperados card: `expectedMode` (`auto` / `manual`), `expectedDownload` (Mbps), `expectedUpload` (Mbps), `expectedPing` (ms) inputs (editable when `manual`, read-only preview when `auto`).
    - Idioma card: `<select name="locale">` with `auto` + every entry from `locales` (`/locales/index.json`).
  - `export function renderNodes(state, { t }) -> string` and `export function renderIntegrations(state, { t }) -> string` in `web/views/seams.js`:
    - `renderNodes`: renders the local instance status card (online dot, version, timezone, uptime, CPU %, RAM MB) and a note/seam for remote nodes (Plan 3).
    - `renderIntegrations`: renders the integrations empty state card ready for Plan 3.
  - `export async function saveConfigDiff(prevConfig, formValues, apiClient) -> { ok: boolean, error?: string }` in `web/views/settings.js`:
    - Diffs changed fields, calls `apiClient.patchConfig(field, value)` sequentially using `CONFIG_KEY_MAP` + `serializeConfigValue`, and catches `ApiClientError` to return `{ ok: false, error: err.message }`.
  - `web/app.js`: mounts sidebar + active view, binds delegated click/submit/change/hover events (including chart point tooltip), opens `EventSource("/api/speedtests/events")`, refreshes data on `finished`/`failed`, and updates the sidebar footer (`orangepi · online` / `netrack · online`, `CPU X% · RAM Y MB`) every 15s.

- [ ] **Step 1: Write failing tests** in `web/tests/views_config.test.js`:
  - `servers_view_renders_providers_interfaces_and_libre_list`: renders Cloudflare/LibreSpeed options, interfaces `eth0 (192.168.1.10)`, and LibreSpeed servers.
  - `settings_view_renders_cron_retention_expected_and_locales`: renders `0 * * * *`, `365`, `100`, `50`, `25`, and language options `pt-BR` and `en`.
  - `config_patch_maps_camel_to_snake_and_surfaces_400`: `saveConfigDiff` with changed `scheduleOffset: false` and `cron: "bad"` calls `PATCH /api/config/schedule_offset` with `{"value":"false"}` then `PATCH /api/config/cron` with `{"value":"bad"}`, and returns `{ ok: false, error: "invalid value for cron: ..." }` when the API responds `400`.
- [ ] **Step 2: Run** `node --test web/tests/views_config.test.js` → FAIL.
- [ ] **Step 3: Implement** `web/views/servers.js`, `web/views/settings.js`, `web/views/seams.js`, and `web/app.js`; add an assertion in `tests/web_assets.rs` that all ES module imports in `web/*.js` and `web/views/*.js` resolve to embedded assets in `get_asset(...)`.
- [ ] **Step 4: Run** `node --test web/tests/*.test.js && cargo test` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(web): servers, settings, sidebar seams and live app wiring"`

---

## Self-review notes

- **Spec coverage:**
  - Section 6 visual direction `"C · Painel denso"` (palette `#0E1020`/`#13162A`/`#171A31`/`#262A47`/`#E8E9F5`/`#8A8DB0`, metric colors `#8B7CFF`/`#3FD6C2`/`#FF8FB1`, embedded `Space Grotesk` / `Inter` / `JetBrains Mono` fonts, 7 sidebar items + footer server status card, responsive mobile layout) → Tasks 1, 3, 4, 5, 6.
  - Section 6 Overview (3 circular gauge cards with plan % and avg/min/max, speed line+area chart, weekday×hour heatmap, recent tests table with `ok`/`falha` badges) → Tasks 3, 5.
  - Section 6 i18n (`pt-BR` and `en` key catalogs, adding a language = adding one JSON file, browser language detection + selector in Ajustes, server-TZ dates and locale numbers) → Tasks 1, 2, 6.
  - Section 6 live updates via SSE without page reload → Tasks 4, 6.
  - Sections 5.1–5.5 & 5.13 UI screens (Estatísticas with date range/consistency/hourly/heatmap/300-pt series, Histórico with cursor pagination & delete, Servidores with provider/LibreSpeed/interface selection, Ajustes with cron/offset/pause/retention/expected plan) → Tasks 5, 6.
- **Deferred to Plan 3:** Ookla CLI download/consent, notifications/integrations CRUD, Prometheus `/api/prometheus/metrics`, password auth, multi-node proxy CRUD, JSON/CSV import-export, Open Graph image, preview mode, GitHub release check.
