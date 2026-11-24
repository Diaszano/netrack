# Development

[← Documentation](../../README.md) · [Português](../pt-BR/development.md)

## Stack

- **Backend**: Rust (edition 2024) with Tokio, Axum, rusqlite (bundled SQLite), reqwest
  (rustls) and croner. Single binary, no runtime dependencies.
- **Frontend**: plain JavaScript ES modules, CSS and hand-drawn SVG charts; no framework, no
  bundler, no npm dependencies. Files in `web/` are embedded in the binary at compile time by
  `build.rs`.

## Layout

| Path | Contents |
|---|---|
| `src/main.rs`, `src/lib.rs` | Startup and wiring |
| `src/api/` | HTTP router, speed test, config and system endpoints |
| `src/auth/` | Password hashing, sessions, lockout, auth middleware |
| `src/providers/` | Cloudflare, LibreSpeed and Ookla providers |
| `src/measure.rs` | Parallel throughput engine and latency statistics |
| `src/runner.rs`, `src/scheduler.rs` | Running tests, cron scheduling, daily maintenance |
| `src/db/` | SQLite access, migrations, statistics, backups |
| `src/integrations/` | Notification dispatcher and the channels |
| `src/nodes/` | Remote nodes and the proxy |
| `src/data_transfer/` | Import and export |
| `src/prometheus.rs`, `src/opengraph.rs`, `src/telemetry.rs` | Metrics, OG image, OpenTelemetry |
| `web/` | Web UI (`app.js`, `views/`, `locales/`, `styles.css`, fonts) |
| `web/tests/` | Frontend unit tests |
| `tests/` | Rust integration tests |

## Running locally

Install the Rust version pinned in `rust-toolchain.toml` (rustup picks it up automatically;
`mise.toml` pins the same version for [mise](https://mise.jdx.dev) users).

```bash
NETRACK_DATA_DIR=./data cargo run
# http://localhost:5216
```

Changes under `web/` need a rebuild (`cargo run` again), since the files are embedded at compile
time. Useful variables while developing: `RUST_LOG=debug`, `RUN_TEST_ON_STARTUP=true`,
`TZ=America/Sao_Paulo`.

To get realistic data without waiting, import a JSON or CSV file through **Settings → Import**
(see [Data](data.md#import) for the format).

## Tests and checks

The same commands CI runs:

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
node --test web/tests/*.test.js     # Node 22+, no npm install needed
```

## Translations

UI strings live in `web/locales/<code>.json`. To add a language, copy `en.json`, translate the
values, set `_meta.name` (shown in the language picker) and rebuild. Missing keys fall back to
Portuguese.

## Continuous integration

Every pull request runs:

- **Rust & Web UI Tests**: the commands above.
- **Multi-Arch Docker Build Check**: builds the image for `amd64` and `arm64`.
- **cargo audit** (also weekly): known vulnerabilities in dependencies.
- **PR checks**: branch name, PR title and commit messages (below).

`main` is protected: CI OK, branch name, PR title and commit message checks must pass, history
is linear and conversations must be resolved.

## Contributing

- **Branch names**: `<type>/<kebab-case-description>`, with type one of `feat`, `fix`,
  `refactor`, `docs`, `test`, `chore`, `hotfix` (e.g. `fix/upload-byte-counting`).
- **Commits and PR titles** follow [Conventional Commits](https://www.conventionalcommits.org/):
  `type(scope): description`, with type one of `feat`, `fix`, `docs`, `style`, `refactor`,
  `perf`, `test`, `build`, `ci`, `chore`, `revert`, and at most 72 characters after the colon.
- Pull requests are squash-merged, so the PR title becomes the commit on `main` and drives the
  changelog.

## Releases

Releases are automated with [release-please](https://github.com/googleapis/release-please):

1. Merging into `main` updates a release PR that bumps the version in `Cargo.toml`/`Cargo.lock`
   and updates `CHANGELOG.md` from the Conventional Commit messages (`feat` → minor, `fix` →
   patch).
2. Merging the release PR creates the Git tag and GitHub release.
3. The release workflow then builds the multi-arch image and pushes it to
   `ghcr.io/diaszano/netrack` with the tags `<version>`, `<major>.<minor>` and `latest`
   (plus `<major>` from 1.0 on).

The release PR is opened by a bot. For CI to run on it, the repository needs a
`RELEASE_PLEASE_TOKEN` secret: a fine-grained personal access token for this repository with
read/write access to **Contents**, **Pull requests** and **Issues**. Without it, the release PR
uses the default `GITHUB_TOKEN`, which doesn't trigger workflows, so the required checks never
run.
