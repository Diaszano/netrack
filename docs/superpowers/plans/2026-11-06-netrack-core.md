# netrack Core (Plan 1 of 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A single Rust binary that runs scheduled Cloudflare/LibreSpeed speed tests natively, stores them in SQLite, computes statistics in the database and exposes everything through a JSON API, shipped as a small multi-arch Docker image.

**Architecture:** One crate (`netrack`, lib + bin). `tokio` runtime; `axum` HTTP API; `reqwest` (rustls) for measurements; `rusqlite` (bundled) behind a small `Db` handle that runs queries on `spawn_blocking`. A generic throughput engine (`measure`) drives N parallel HTTP streams and is shared by both providers. A `Runner` owns the "one test at a time" lock, retry and persistence; a `Scheduler` fires the runner from cron. Local hour/weekday are stored per row at insert time, so all hour/heatmap stats are plain SQL `GROUP BY`.

**Tech Stack:** Rust 1.99.1 (edition 2024), tokio, axum 0.8, reqwest 0.12 (rustls, stream), rusqlite 0.32 (bundled), croner, chrono + chrono-tz, serde/serde_json, tracing, if-addrs, futures, bytes, rand. Tests: tokio::test, tower::ServiceExt::oneshot, local axum mock servers.

**Spec:** `docs/superpowers/specs/2026-11-06-netrack-design.md`

**Out of scope (Plans 2 and 3):** web interface + i18n (Plan 2); Ookla, integrations/notifications, Prometheus, password, nodes, import/export, Open Graph, preview mode, version check (Plan 3). This plan must leave clean seams for them: the runner emits events on a `tokio::sync::broadcast` channel (Plan 3 integrations subscribe), and the router is built by one function Plan 2/3 extend.

## Global Constraints

- Single binary; nothing spawns external processes in this plan.
- Default port `5216` (env `NETRACK_PORT`), data dir `/data` (env `NETRACK_DATA_DIR`), DB file `<data>/netrack.db`.
- Timezone: one, the server's, from env `TZ` (IANA name, default `UTC`), resolved with `chrono-tz` (embedded tz data — the image has none).
- `RUN_TEST_ON_STARTUP=true` runs one test at boot.
- Config defaults (keys stored in `config` table): `provider=cloudflare`, `cron=0 * * * *`, `schedule_offset=true`, `retention_days=365` (max `10000`), `expected_mode=auto`, `expected_ping=25`, `expected_download=100`, `expected_upload=50`, `libre_server_id=auto`, `libre_url=none`, `interface=auto`, `paused=false`.
- Units: download/upload in Mbps (`f64`, 2 decimals), ping and jitter in ms (`f64`, 2 decimals), test duration in seconds (`f64`).
- Only one test runs at a time; a second request gets HTTP 409.
- Failed test: retry once; if it fails again, persist a row with `error` set and metrics null.
- Statistics never load the full history into memory; every period filter and aggregate is SQL with an index on `created_at`. Chart series return at most 300 points.
- Measurement must not be the bottleneck: ≥ 2.5 Gbps on loopback on x86 in release mode (ignored benchmark test), and saturate 1 Gbps on the Orange Pi Zero 3 (manual acceptance, Task 10).
- Docker: multi-arch `linux/arm64` + `linux/amd64`, final image from `scratch` (+ CA certs), example compose uses `network_mode: host`.
- Repo rules: branch names `<type>/<kebab-case>`; commit messages must NOT contain AI co-author trailers (a hook rejects them).

## Review Focus

1. **Provider unreachable / DNS failure / no internet** — the test must end with a stored failed row and a readable error within a bounded time (60 s total), never hang the runner lock. → Task 6 `runner_records_failure_after_one_retry` + Task 4 `download_times_out_on_stalled_server`.
2. **Very large history (100k rows)** — statistics for a 1-year range stay fast and series are capped at 300 points. → Task 7 `series_is_downsampled_to_300_points` + `stats_on_100k_rows_under_200ms` (ignored perf test).
3. **Invalid config values via API** (bad cron, retention 0 or > 10000, unknown provider, negative expected values) — rejected with 400 and old value kept. → Task 2 `config_rejects_invalid_values`.
4. **TZ change between runs / DST** — local hour/weekday must be recomputed for all rows on startup when `TZ` differs from the stored one. → Task 2 `local_fields_recomputed_when_tz_changes`.
5. **"Testar agora" while a scheduled test is running** — 409, no second test, no stuck "running" state after a panic in a provider. → Task 6 `concurrent_run_returns_busy` + `lock_released_when_provider_panics`.

---

## File Structure

```
Cargo.toml
rust-toolchain.toml            # channel = "1.99.1"
mise.toml                      # [tools] rust = "1.99.1"
src/main.rs                    # boot: config, db, runner, scheduler, http
src/lib.rs                     # pub mod declarations
src/settings.rs                # env settings (port, data dir, tz, run-on-startup)
src/db/mod.rs                  # Db handle, open + migrations
src/db/migrations.rs           # ordered SQL migrations
src/db/config.rs               # typed config get/set + validation
src/db/tests_repo.rs           # speedtest rows CRUD + retention
src/db/stats.rs                # statistics queries
src/measure.rs                 # generic parallel throughput engine + latency stats
src/providers/mod.rs           # SpeedProvider trait, Measurement, ProviderError, factory
src/providers/cloudflare.rs
src/providers/librespeed.rs
src/net.rs                     # interface detection, local address binding
src/runner.rs                  # single-run lock, retry, persist, events, expected values
src/scheduler.rs               # cron + offset, pause, retention job
src/system.rs                  # process CPU/RAM
src/api/mod.rs                 # router(AppState) -> Router
src/api/speedtests.rs
src/api/config.rs
src/api/system.rs
tests/support/mod.rs           # mock Cloudflare / LibreSpeed servers
tests/*.rs                     # integration tests per task
Dockerfile
docker-compose.example.yml
.dockerignore
```

---

### Task 1: Project scaffold and settings

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `mise.toml`, `src/main.rs`, `src/lib.rs`, `src/settings.rs`, `.gitignore` (`/target`, `*.db`)
- Test: `src/settings.rs` (unit tests)

**Interfaces:**
- Produces: `pub struct Settings { pub port: u16, pub data_dir: PathBuf, pub tz: chrono_tz::Tz, pub run_on_startup: bool }` and `impl Settings { pub fn from_env() -> anyhow::Result<Self>; pub fn from_map(vars: &HashMap<String,String>) -> anyhow::Result<Self> }`.

- [ ] **Step 1: Write failing tests** in `src/settings.rs`:
  - `defaults_when_env_empty`: `from_map(&{})` → port 5216, data_dir `/data`, tz `UTC`, run_on_startup false.
  - `reads_all_vars`: `NETRACK_PORT=8080, NETRACK_DATA_DIR=/tmp/x, TZ=America/Sao_Paulo, RUN_TEST_ON_STARTUP=true` → matching values, `tz == chrono_tz::America::Sao_Paulo`.
  - `rejects_bad_tz`: `TZ=Mars/Base` → `Err` whose message contains `TZ`.
- [ ] **Step 2: Run** `cargo test settings` → FAIL (does not compile).
- [ ] **Step 3: Implement** `Settings::from_map` / `from_env` (`from_env` collects `std::env::vars()` and delegates). `main.rs` loads settings, inits `tracing_subscriber` (env filter, default `info`) and logs them; nothing else yet.
- [ ] **Step 4: Run** `cargo test settings && cargo build` → PASS.
- [ ] **Step 5: Commit** `git commit -m "chore: scaffold netrack crate with env settings"`

---

### Task 2: Database, migrations and typed config

**Files:**
- Create: `src/db/mod.rs`, `src/db/migrations.rs`, `src/db/config.rs`
- Test: `tests/db_config.rs`

**Interfaces:**
- Produces:
  - `#[derive(Clone)] pub struct Db` with `pub async fn open(path: &Path, tz: Tz) -> anyhow::Result<Db>`, `pub async fn open_in_memory(tz: Tz) -> anyhow::Result<Db>`, and `pub async fn call<F, T>(&self, f: F) -> anyhow::Result<T> where F: FnOnce(&mut rusqlite::Connection) -> anyhow::Result<T> + Send + 'static, T: Send + 'static` (runs on `spawn_blocking`, single connection behind `Arc<Mutex<Connection>>`, WAL mode, `busy_timeout=5000`).
  - `pub fn tz(&self) -> Tz`.
  - In `db::config`: `pub enum Provider { Cloudflare, LibreSpeed }` (Plan 3 adds `Ookla`), `pub struct AppConfig { provider, cron: String, schedule_offset: bool, retention_days: u32, expected_mode: ExpectedMode /* Auto|Manual */, expected_ping: f64, expected_download: f64, expected_upload: f64, libre_server_id: Option<u32>, libre_url: Option<String>, interface: Option<String>, paused: bool }` (serde, camelCase), `pub async fn load(db: &Db) -> anyhow::Result<AppConfig>`, `pub async fn set(db: &Db, key: &str, value: &str) -> Result<(), ConfigError>`, `pub enum ConfigError { UnknownKey(String), Invalid { key: String, reason: String }, Db(anyhow::Error) }`.

Schema (migration 1):
```sql
CREATE TABLE config (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE speedtests (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at INTEGER NOT NULL,          -- unix ms UTC
  local_hour INTEGER NOT NULL,          -- 0..23 in server TZ
  local_weekday INTEGER NOT NULL,       -- 0=Mon..6=Sun in server TZ
  kind TEXT NOT NULL,                   -- 'auto' | 'custom'
  provider TEXT NOT NULL,
  server_id TEXT, server_name TEXT, server_host TEXT, result_id TEXT,
  ping REAL, jitter REAL, download REAL, upload REAL,
  duration REAL,
  error TEXT
);
CREATE INDEX speedtests_created_at ON speedtests(created_at);
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL); -- stores 'tz' and 'schema_version'
```
Migrations are an ordered `&[&str]`; `schema_version` in `meta` tracks the last applied. On open, if `meta.tz` differs from the given `tz`, recompute `local_hour/local_weekday` for every row in one transaction (rows processed in Rust with chrono-tz, batched updates), then store the new tz.

- [ ] **Step 1: Write failing tests** in `tests/db_config.rs`:
  - `defaults_loaded_on_fresh_db`: `load` returns provider Cloudflare, cron `0 * * * *`, retention 365, expected 25/100/50, paused false.
  - `set_persists_value`: `set("retention_days","30")` then `load().retention_days == 30`.
  - `config_rejects_invalid_values`: each of `("cron","nope")`, `("retention_days","0")`, `("retention_days","10001")`, `("provider","foo")`, `("expected_download","-1")`, `("libre_url","ftp://x")` → `ConfigError::Invalid`, and the stored value is unchanged; `("nope","1")` → `UnknownKey`.
  - `local_fields_recomputed_when_tz_changes`: open file DB in a tempdir with UTC, insert raw row `created_at = 2026-01-15T02:30:00Z` (`local_hour 2`), reopen with `America/Sao_Paulo` → row has `local_hour == 23`, `local_weekday == 2` (Wed, 14 Jan).
- [ ] **Step 2: Run** `cargo test --test db_config` → FAIL.
- [ ] **Step 3: Implement** `Db`, migrations, tz recompute, `config::{load,set}`. Validation: cron parsed with `croner::Cron::new(v).parse()`; bools `true|false`; numbers ≥ 0; retention 1..=10000; `libre_url` must be `http(s)://` or `none`; `interface` any string or `auto`.
- [ ] **Step 4: Run** `cargo test --test db_config` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(db): sqlite schema, migrations and validated config"`

---

### Task 3: Speedtest repository and retention

**Files:**
- Create: `src/db/tests_repo.rs`
- Test: `tests/db_tests_repo.rs`

**Interfaces:**
- Consumes: `Db` (Task 2).
- Produces:
  - `#[derive(Serialize, Clone)] pub struct SpeedtestRow { id: i64, created_at: i64, kind: TestKind, provider: String, server_id, server_name, server_host, result_id: Option<String>, ping, jitter, download, upload: Option<f64>, duration: Option<f64>, error: Option<String> }` (camelCase JSON).
  - `pub enum TestKind { Auto, Custom }`.
  - `pub struct NewSpeedtest { kind, provider: String, server_id, server_name, server_host, result_id: Option<String>, metrics: Result<Metrics, String>, duration: f64 }` where `pub struct Metrics { ping: f64, jitter: f64, download: f64, upload: f64 }`.
  - `pub async fn insert(db: &Db, t: NewSpeedtest, at: DateTime<Utc>) -> anyhow::Result<i64>` (computes local fields with `db.tz()`).
  - `pub async fn list(db: &Db, before_id: Option<i64>, limit: u32) -> anyhow::Result<Vec<SpeedtestRow>>` (newest first, limit clamped to 1..=100).
  - `pub async fn get(db, id) -> anyhow::Result<Option<SpeedtestRow>>`, `pub async fn delete(db, id) -> anyhow::Result<bool>`, `pub async fn delete_older_than(db, days: u32, now: DateTime<Utc>) -> anyhow::Result<u64>`, `pub async fn last_successful(db, n: u32) -> anyhow::Result<Vec<SpeedtestRow>>`.

- [ ] **Step 1: Write failing tests:**
  - `insert_and_get_roundtrip` (ok metrics rounded to 2 decimals: 587.123 → 587.12).
  - `failed_test_has_error_and_null_metrics`.
  - `list_paginates_newest_first`: insert 15, `list(None,10)` → ids 15..6; `list(Some(6),10)` → 5..1.
  - `delete_older_than_removes_only_old`: rows at now-400d, now-10d; `delete_older_than(365)` → 1 removed.
  - `last_successful_skips_failures`.
- [ ] **Step 2: Run** `cargo test --test db_tests_repo` → FAIL.
- [ ] **Step 3: Implement** the functions.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(db): speedtest repository and retention"`

---

### Task 4: Throughput engine

**Files:**
- Create: `src/measure.rs`, `tests/support/mod.rs` (a reusable mock server: `pub async fn spawn_mock() -> MockServer { base_url: String, ... }` serving `GET /down?bytes=N` streaming N zero bytes from a static 1 MiB buffer, `POST /up` that drains and discards the body, `GET /empty`; plus knobs `stall_after_bytes: Option<u64>` and `fail_status: Option<u16>`)
- Test: `tests/measure.rs`

**Interfaces:**
- Produces:
  - `pub struct ThroughputConfig { pub duration: Duration /* 10s */, pub warmup: Duration /* 2s */, pub initial_streams: usize /* 4 */, pub max_streams: usize /* 32 */, pub step: usize /* 4 */, pub sample_every: Duration /* 250ms */, pub stall_timeout: Duration /* 5s */ }` with `Default`.
  - `pub type StreamFactory = Arc<dyn Fn(Arc<AtomicU64>) -> BoxFuture<'static, Result<(), MeasureError>> + Send + Sync>` — each call performs one HTTP transfer, adding bytes to the counter as they flow; the engine re-invokes it in a loop per stream until time is up.
  - `pub async fn measure_throughput(cfg: &ThroughputConfig, factory: StreamFactory) -> Result<f64 /* Mbps */, MeasureError>`.
  - `pub struct LatencyStats { pub ping: f64, pub jitter: f64 }`, `pub fn latency_stats(samples_ms: &[f64]) -> Option<LatencyStats>` — ping = median, jitter = mean absolute difference of consecutive samples, `None` if fewer than 2.
  - `pub enum MeasureError { AllStreamsFailed(String), Stalled, Http(String) }` (`thiserror`).
  - `pub static UPLOAD_CHUNK: LazyLock<Bytes>` — 1 MiB of random bytes generated once; upload bodies are `Body::wrap_stream` repeating `UPLOAD_CHUNK.clone()` (no copies).

Engine algorithm (not determined by tests — implement exactly):
```
start initial_streams tasks; each loops: factory(counter).await, on Err count failure and retry after 100ms
every sample_every: read counter, compute rate over the last 1s window
  if elapsed < warmup and rate grew > 10% vs previous window and streams < max_streams: spawn `step` more streams
  if no bytes for stall_timeout: abort all, Err(Stalled)
at warmup: snapshot (t0, bytes0); at duration: snapshot (t1, bytes1); abort all streams
if bytes1 == bytes0 and every stream failed: Err(AllStreamsFailed(last error))
Mbps = (bytes1 - bytes0) * 8 / (t1 - t0).as_secs_f64() / 1e6, rounded to 2 decimals
```

- [ ] **Step 1: Write failing tests:**
  - `latency_stats_median_and_jitter`: `[10,12,11,30,12]` → ping 12.0, jitter (2+1+19+18)/4 = 10.0; `[5]` → None.
  - `download_against_mock_reports_positive_rate`: short cfg (duration 2s, warmup 500ms) with a reqwest-based download factory against `spawn_mock()` → `Ok(x)` with `x > 100.0`.
  - `upload_against_mock_reports_positive_rate`: same for POST `/up`.
  - `download_times_out_on_stalled_server`: mock with `stall_after_bytes=1_000_000`, `stall_timeout=1s` → `Err(Stalled)` within 4s.
  - `all_streams_failing_is_error`: mock `fail_status=500` → `Err(AllStreamsFailed(_))`.
  - `#[ignore] loopback_exceeds_2_5_gbps`: default cfg, download against mock → `> 2500.0`. Run with `cargo test --release -- --ignored loopback`.
- [ ] **Step 2: Run** `cargo test --test measure` → FAIL.
- [ ] **Step 3: Implement** `measure.rs` and the mock in `tests/support/mod.rs`. Counting: read `response.bytes_stream()` chunks and `fetch_add(chunk.len())` — never buffer the body.
- [ ] **Step 4: Run** `cargo test --test measure` → PASS; `cargo test --release --test measure -- --ignored` → PASS on dev machine (record the number in the commit message).
- [ ] **Step 5: Commit** `git commit -m "feat(measure): parallel throughput engine and latency stats"`

---

### Task 5: Providers (trait, Cloudflare, LibreSpeed, interface binding)

**Files:**
- Create: `src/providers/mod.rs`, `src/providers/cloudflare.rs`, `src/providers/librespeed.rs`, `src/net.rs`
- Modify: `tests/support/mod.rs` (add Cloudflare-shaped routes `/__down`, `/__up` with header `cf-meta-colo: POA`, and LibreSpeed-shaped `/backend/garbage.php?ckSize=N` (N MiB), `/backend/empty.php`, and a `/servers.json` list)
- Test: `tests/providers.rs`

**Interfaces:**
- Consumes: `measure::{measure_throughput, latency_stats, ThroughputConfig, UPLOAD_CHUNK}`.
- Produces:
  - `pub struct Measurement { pub ping: f64, pub jitter: f64, pub download: f64, pub upload: f64, pub server_id: Option<String>, pub server_name: Option<String>, pub server_host: Option<String>, pub result_id: Option<String> }`.
  - `pub enum ProviderError { Network(String), NoServer, Measure(MeasureError) }`.
  - `pub struct RunOptions { pub local_addr: Option<IpAddr>, pub throughput: ThroughputConfig }`.
  - `pub trait SpeedProvider: Send + Sync { fn name(&self) -> &'static str; fn run<'a>(&'a self, opts: &'a RunOptions) -> BoxFuture<'a, Result<Measurement, ProviderError>>; }`
  - `pub struct Cloudflare { base_url: String }` — `Cloudflare::new()` uses `https://speed.cloudflare.com`; `Cloudflare::with_base(url)` for tests.
  - `pub struct LibreSpeed { server: LibreServer }`, `pub struct LibreServer { pub id: u32, pub name: String, pub server: String, pub dl_url: String, pub ul_url: String, pub ping_url: String }` (serde from the public JSON: `server`, `dlURL`, `ulURL`, `pingURL`), `pub async fn fetch_libre_servers(client: &reqwest::Client, list_url: &str) -> Result<Vec<LibreServer>, ProviderError>` (default list `https://librespeed.org/backend-servers/servers.php`), `pub async fn pick_libre_server(...)` choosing lowest ping among the first 10 by a single `ping_url` GET each (2 s timeout).
  - `pub fn custom_libre_server(url: &str) -> LibreServer` — `dl_url = "garbage.php"`, `ul_url = "empty.php"`, `ping_url = "empty.php"` (MySpeed's convention).
  - `pub fn build_provider(cfg: &AppConfig, client_factory) -> Box<dyn SpeedProvider>`.
  - `net.rs`: `pub fn list_interfaces() -> Vec<NetInterface { name: String, addrs: Vec<IpAddr> }>` (non-loopback, via `if-addrs`), `pub fn resolve_local_addr(iface: Option<&str>) -> Option<IpAddr>`, `pub fn http_client(local_addr: Option<IpAddr>) -> reqwest::Client` (rustls, `local_address`, `tcp_nodelay`, connect timeout 5 s, no total timeout — the engine owns timing).

Protocol details:
- Cloudflare latency: 1 warm-up `GET /__down?bytes=0`, then 20 sequential ones on the same client; sample = request round trip in ms. Server name `Cloudflare · <cf-meta-colo>`, host `speed.cloudflare.com`. Download factory: `GET /__down?bytes=25000000`. Upload factory: `POST /__up` with a 25 MB body streamed from `UPLOAD_CHUNK`.
- LibreSpeed latency: 1 warm-up + 20 `GET {server}/{ping_url}?r=<rand>`. Download: `GET {server}/{dl_url}?ckSize=100`. Upload: `POST {server}/{ul_url}` 25 MB body. Server name/host/id from `LibreServer`.
- Whole provider run is wrapped in `tokio::time::timeout(60s)` → `ProviderError::Network("timeout")`.

- [ ] **Step 1: Write failing tests:**
  - `cloudflare_run_against_mock`: `Cloudflare::with_base(mock)` with short throughput cfg → download > 0, upload > 0, ping ≥ 0, `server_name == Some("Cloudflare · POA")`.
  - `librespeed_run_against_mock` with `custom_libre_server(mock + "/backend/")` → positive metrics, server_host is the mock URL.
  - `fetch_libre_servers_parses_public_shape`: mock `/servers.json` returning the exact public JSON shape (escaped slashes included) → 2 servers, `dl_url == "garbage.php"`.
  - `unreachable_server_is_network_error`: `Cloudflare::with_base("http://127.0.0.1:1")` → `Err(ProviderError::Network(_))` in < 10 s.
  - `resolve_local_addr_auto_is_none`: `resolve_local_addr(None) == None`; unknown name → `None`.
- [ ] **Step 2: Run** `cargo test --test providers` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test --test providers` → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(providers): native cloudflare and librespeed"`

---

### Task 6: Runner (lock, retry, persistence, events, expected values)

**Files:**
- Create: `src/runner.rs`
- Test: `tests/runner.rs`

**Interfaces:**
- Consumes: `Db`, `tests_repo::{insert, last_successful}`, `config::{load, set}`, `SpeedProvider`, `RunOptions`, `net::resolve_local_addr`.
- Produces:
  - `pub enum RunEvent { Started { kind: TestKind }, Finished { id: i64, metrics: Metrics }, Failed { id: i64, error: String }, ExpectedUpdated { ping: f64, download: f64, upload: f64 } }` (Clone, Serialize).
  - `#[derive(Clone)] pub struct Runner` with `pub fn new(db: Db, provider_factory: Arc<dyn Fn(&AppConfig) -> Box<dyn SpeedProvider> + Send + Sync>) -> Runner`, `pub fn subscribe(&self) -> broadcast::Receiver<RunEvent>` (capacity 64), `pub fn is_running(&self) -> bool`, `pub async fn run(&self, kind: TestKind) -> Result<i64, RunError>`.
  - `pub enum RunError { Busy, Internal(anyhow::Error) }`.

Behavior: `run` acquires an `AtomicBool` guard (RAII, released on drop — including panic unwinding inside a `tokio::spawn`ed provider task whose `JoinError` becomes a failure); Busy if held. Emits `Started`; calls provider; on error retries once; persists success/failure row; emits `Finished`/`Failed`. After a success with `expected_mode=auto` and ≥ 10 successful tests: expected = best of the last 10 (min ping, max download, max upload, 2 decimals), written via `config::set`, emits `ExpectedUpdated`.

- [ ] **Step 1: Write failing tests** with a `FakeProvider` (scripted `Vec<Result<Measurement, ProviderError>>`, optional delay, optional panic):
  - `successful_run_persists_and_emits_finished`.
  - `runner_records_failure_after_one_retry`: script `[Err, Err]` → one row with `error` set, provider called exactly 2 times, `Failed` emitted.
  - `retry_success_stores_single_row`: `[Err, Ok]` → one ok row.
  - `concurrent_run_returns_busy`: provider delay 500 ms; second `run` → `Err(RunError::Busy)`.
  - `lock_released_when_provider_panics`: panicking provider → failed row stored and `is_running() == false` afterwards; a following `run` succeeds.
  - `expected_values_auto_from_last_10`: 10 ok runs with known values → config expected = (min ping, max dl, max ul); with `expected_mode=manual` → unchanged.
- [ ] **Step 2: Run** `cargo test --test runner` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(runner): single-flight test runner with retry and events"`

---

### Task 7: Statistics queries

**Files:**
- Create: `src/db/stats.rs`
- Test: `tests/db_stats.rs`

**Interfaces:**
- Consumes: `Db`, `tests_repo::insert` (for fixtures).
- Produces: `pub async fn statistics(db: &Db, from: NaiveDate, to: NaiveDate) -> anyhow::Result<Statistics>` where the range is inclusive whole days in the server TZ, and

```rust
pub struct Statistics {               // serde camelCase
  pub total: u64, pub failed: u64, pub availability: f64,            // % of non-failed, 1 decimal
  pub download: MetricSummary, pub upload: MetricSummary,
  pub ping: MetricSummary, pub jitter: MetricSummary,
  pub hourly: Vec<HourBucket>,        // 24 entries, hour 0..23, nulls when empty
  pub heatmap: Vec<HeatCell>,         // weekday×hour with data only
  pub series: Vec<SeriesPoint>,       // ≤ 300 points
}
pub struct MetricSummary { pub avg: Option<f64>, pub min: Option<f64>, pub max: Option<f64>, pub std_dev: Option<f64>, pub consistency: Option<f64> }
pub struct HourBucket { pub hour: u8, pub download: Option<f64>, pub upload: Option<f64>, pub ping: Option<f64>, pub jitter: Option<f64>, pub count: u64 }
pub struct HeatCell { pub weekday: u8, pub hour: u8, pub download: f64, pub count: u64 }
pub struct SeriesPoint { pub t: i64, pub download: Option<f64>, pub upload: Option<f64>, pub ping: Option<f64> }
```

Rules: std_dev is population (`sqrt(avg(x*x) - avg(x)^2)`, computed in SQL); `consistency = 100 - std_dev/avg*100` (1 decimal; 100 when avg is 0 — MySpeed's formula). Series: if rows in range ≤ 300 return them; else bucket width `ceil((to_ms - from_ms) / 300)` and `GROUP BY (created_at - from_ms) / width` with averages, `t` = bucket start. Failed rows count toward `total`/`failed` only.

- [ ] **Step 1: Write failing tests:**
  - `summary_avg_min_max_stddev`: downloads 100, 200, 300 → avg 200, min 100, max 300, std_dev 81.65, consistency 59.2.
  - `failed_rows_excluded_from_metrics_but_counted`: 3 ok + 1 failed → total 4, failed 1, availability 75.0.
  - `hourly_has_24_buckets_in_server_tz`: db tz `America/Sao_Paulo`, row at 2026-03-10T00:30Z → bucket hour 21 has count 1.
  - `heatmap_groups_by_weekday_and_hour`.
  - `series_is_downsampled_to_300_points`: 1000 rows over 10 days → `series.len() <= 300` and the mean of series downloads ≈ overall avg (±1%).
  - `range_is_inclusive_of_whole_days`.
  - `#[ignore] stats_on_100k_rows_under_200ms` (release).
- [ ] **Step 2: Run** `cargo test --test db_stats` → FAIL.
- [ ] **Step 3: Implement** with SQL aggregates; one query per section, all inside a single `db.call`.
- [ ] **Step 4: Run** → PASS (and the ignored perf test in release).
- [ ] **Step 5: Commit** `git commit -m "feat(stats): sql statistics with hourly, heatmap and downsampled series"`

---

### Task 8: Scheduler and retention job

**Files:**
- Create: `src/scheduler.rs`
- Test: `tests/scheduler.rs`

**Interfaces:**
- Consumes: `Runner`, `config::load`, `tests_repo::delete_older_than`.
- Produces:
  - `pub fn next_fire(cron: &str, offset: bool, after: DateTime<Tz>, rng: &mut impl Rng) -> anyhow::Result<DateTime<Tz>>` — next cron time in server TZ; when `offset` is true add a uniform random delay in `0..min(5 min, interval/4)` where interval is the gap to the following cron time.
  - `pub struct Scheduler` with `pub fn spawn(db: Db, runner: Runner, reload: watch::Receiver<()>) -> Scheduler` (a background task that sleeps until `next_fire`, skips firing when `paused`, re-reads config whenever `reload` changes), `pub fn next_run(&self) -> Option<DateTime<Utc>>` (shared `Arc<Mutex<_>>`, `None` while paused), and a daily retention task (03:00 server TZ, plus once at startup) calling `delete_older_than(retention_days)`.

- [ ] **Step 1: Write failing tests:**
  - `next_fire_without_offset_is_exact`: cron `0 * * * *`, after 10:15 → 11:00.
  - `next_fire_offset_is_bounded`: 1000 draws with seeded rng → all in [11:00, 11:05).
  - `next_fire_respects_server_tz`: cron `0 21 * * *`, tz Sao_Paulo, after 2026-03-10T23:00Z (20:00 local) → 2026-03-11T00:00Z.
  - `paused_scheduler_does_not_run` (tokio `start_paused` time; cron `* * * * *`; paused=true; advance 3 min → 0 runs; set paused=false + reload; advance 1 min → 1 run).
  - `retention_runs_at_startup`: old row removed shortly after spawn.
- [ ] **Step 2: Run** `cargo test --test scheduler` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `git commit -m "feat(scheduler): cron scheduling with offset, pause and retention"`

---

### Task 9: HTTP API and boot wiring

**Files:**
- Create: `src/api/mod.rs`, `src/api/speedtests.rs`, `src/api/config.rs`, `src/api/system.rs`, `src/system.rs`
- Modify: `src/main.rs` (open DB, build runner + scheduler, `RUN_TEST_ON_STARTUP`, serve router on `0.0.0.0:<port>` and `[::]`, graceful shutdown on SIGTERM/SIGINT)
- Test: `tests/api.rs`

**Interfaces:**
- Consumes: everything above.
- Produces: `#[derive(Clone)] pub struct AppState { pub db: Db, pub runner: Runner, pub reload: watch::Sender<()>, pub started_at: Instant }`, `pub fn router(state: AppState) -> axum::Router` (Plans 2/3 merge into this). `system.rs`: `pub fn process_stats() -> ProcessStats { cpu_percent: Option<f64>, rss_bytes: Option<u64> }` (Linux `/proc/self/stat` + `/proc/self/statm`, `None` elsewhere).

Routes (JSON, errors as `{ "error": "<message>" }`):

| Method | Path | Result |
|---|---|---|
| GET | `/api/speedtests?before=<id>&limit=<n>` | `SpeedtestRow[]` |
| GET | `/api/speedtests/{id}` | row or 404 |
| DELETE | `/api/speedtests/{id}` | 204 / 404 |
| POST | `/api/speedtests/run` | 202 `{ "status": "started" }` (runs in background, kind `custom`) / 409 `{ "error": "test already running" }` |
| GET | `/api/speedtests/status` | `{ running: bool, paused: bool, nextRun: <unix ms> or null }` |
| POST | `/api/speedtests/pause` / `/continue` | 204 (sets `paused`, pings reload) |
| GET | `/api/speedtests/statistics?from=YYYY-MM-DD&to=YYYY-MM-DD` | `Statistics` / 400 on bad dates or from > to |
| GET | `/api/speedtests/events` | SSE stream of `RunEvent` (Plan 2 uses it for live updates) |
| GET | `/api/config` | `AppConfig` |
| PATCH | `/api/config/{key}` body `{ "value": "<string>" }` | 204 / 400 (`ConfigError`) — pings reload |
| GET | `/api/system/status` | `{ version, uptimeSecs, cpuPercent, rssBytes, tz }` |
| GET | `/api/system/interfaces` | `NetInterface[]` |
| GET | `/api/system/servers/librespeed` | `LibreServer[]` (cached 1 h) |
| GET | `/healthz` | 200 `ok` |

`Scheduler` exposes `next_run()` via a shared `Arc<Mutex<Option<DateTime<Utc>>>>` for `/status`.

- [ ] **Step 1: Write failing tests** (`tower::ServiceExt::oneshot` on `router(state)` with in-memory DB and a FakeProvider factory):
  - `run_then_list_returns_row` (POST run → 202; await `Finished` via `runner.subscribe()`; GET list → 1 row).
  - `run_while_running_is_409`.
  - `patch_config_invalid_is_400_with_message` (`cron=nope`).
  - `statistics_bad_range_is_400` (`from=2026-02-01&to=2026-01-01`).
  - `status_reports_running_and_paused`.
  - `events_stream_emits_started_and_finished`.
  - `healthz_ok`.
- [ ] **Step 2: Run** `cargo test --test api` → FAIL.
- [ ] **Step 3: Implement** handlers and wiring.
- [ ] **Step 4: Run** `cargo test` (full suite) → PASS. Then `cargo run` with `NETRACK_DATA_DIR=./data`, `curl -X POST localhost:5216/api/speedtests/run`, wait ~30 s, `curl localhost:5216/api/speedtests` → one real Cloudflare row with plausible numbers.
- [ ] **Step 5: Commit** `git commit -m "feat(api): json api, sse events and boot wiring"`

---

### Task 10: Docker image and Pi acceptance

**Files:**
- Create: `Dockerfile`, `.dockerignore`, `docker-compose.example.yml`, `README.md` (run instructions only: compose snippet, env vars, ports)

**Interfaces:**
- Consumes: the binary.

Dockerfile: builder stage `rust:1.99-alpine` (musl) with `cargo-zigbuild` or native per-platform builds via `--platform`, targets `aarch64-unknown-linux-musl` and `x86_64-unknown-linux-musl`, `--release` with `lto = "thin"`, `codegen-units = 1`, `strip = true`, `opt-level = 3` in `[profile.release]`. Final stage `FROM scratch`, copy binary + `/etc/ssl/certs/ca-certificates.crt`, `VOLUME /data`, `EXPOSE 5216`, `USER 65534`, `ENTRYPOINT ["/netrack"]`. Compose example: `network_mode: host`, `TZ=America/Sao_Paulo`, volume `./data:/data`, `restart: unless-stopped`.

- [ ] **Step 1: Build** `docker buildx build --platform linux/arm64,linux/amd64 -t netrack:dev .` → succeeds; `docker image ls netrack:dev` → image size < 20 MB.
- [ ] **Step 2: Smoke test** `docker run --rm -p 5216:5216 -v $PWD/data:/data netrack:dev` then `curl localhost:5216/healthz` → `ok`; trigger a run and get a row (TLS works = CA certs present).
- [ ] **Step 3: Pi acceptance (manual, with the user):** on the Orange Pi Zero 3 with `network_mode: host`, run 3 tests; record download/upload, compare with speed.cloudflare.com in a browser on a wired PC (within ±10%), and record peak CPU (`top`) and idle RSS (`/api/system/status`). Target: idle RSS < 30 MB; download not limited by CPU (CPU < 80% at peak). Write the numbers in `README.md` under "Benchmarks".
- [ ] **Step 4: Commit** `git commit -m "build: multi-arch scratch docker image and compose example"`

---

## Self-review notes

- Spec 5.1/5.2 (Cloudflare, LibreSpeed, interface choice, retry, single run, pause, run-on-startup) → Tasks 4–6, 8, 9. 5.3 → Task 8. 5.4 → Task 7 (+ TZ in Task 2). 5.5 → Task 6. 5.13 status → Task 9. Section 7 constraints → Global Constraints + Tasks 4, 10.
- Deferred by design: 5.2 Ookla, 5.6–5.12, 5.13 version check → Plan 3; section 6 + i18n → Plan 2.
