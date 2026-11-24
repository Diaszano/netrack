# Installation

[← Documentation](../../README.md) · [Português](../pt-BR/installation.md)

netrack ships as a multi-arch container image (`linux/amd64`, `linux/arm64`) published to the
GitHub Container Registry on every release:

```
ghcr.io/diaszano/netrack:latest     # newest release
ghcr.io/diaszano/netrack:0.2.0      # exact version
ghcr.io/diaszano/netrack:0.2        # latest patch of a minor
```

The image is built `FROM scratch`: it contains only the static binary, CA certificates and an
empty `/data` directory. It runs as UID `65534` (`nobody`), listens on port `5216` and stores
everything under `/data`.

## Docker Compose (recommended)

```yaml
services:
  netrack:
    image: ghcr.io/diaszano/netrack:latest
    container_name: netrack
    network_mode: host
    environment:
      - TZ=America/Sao_Paulo
    volumes:
      - ./data:/data
    restart: unless-stopped
```

```bash
mkdir -p data && sudo chown 65534:65534 data
docker compose up -d
curl http://localhost:5216/healthz   # ok
```

Then open `http://<host>:5216`.

### Why `network_mode: host`?

A speed test measures whatever network path the process uses. With Docker's default bridge
network, traffic goes through NAT and a virtual interface. That usually costs little, but it hides
the host's interfaces, so you can't pick one in **Servers → Network interface**. With host
networking, netrack sees and binds to the real interfaces.

If you must use a bridge network, publish the port instead:

```yaml
    ports:
      - "5216:5216"
```

### Data directory permissions

The container runs as `65534:65534`, so a bind-mounted directory must be writable by that UID
(`chown 65534:65534 ./data`). A named Docker volume already has the right owner:

```yaml
    volumes:
      - netrack-data:/data
volumes:
  netrack-data:
```

`/data` holds:

| Path | Contents |
|---|---|
| `netrack.db` (+ `-wal`, `-shm`) | SQLite database: history, settings, integrations, nodes, sessions |
| `backups/netrack-YYYY-MM-DD.db` | Daily backups (see [Data](data.md)) |
| `bin/speedtest` | Ookla CLI, only after you accept its license |

## Building the image yourself

```bash
docker build -t netrack:dev .
# or for both architectures (requires buildx)
docker buildx build --platform linux/arm64,linux/amd64 -t netrack:dev .
```

The Dockerfile cross-compiles on the build host with `cargo zigbuild` into static musl binaries,
so building the arm64 image on an x86 machine (or the opposite) is fast and needs no emulation.
`docker-compose.example.yml` in the repository uses this locally built `netrack:dev` tag.

## Running the binary without Docker

Requires the Rust toolchain pinned in `rust-toolchain.toml`.

```bash
cargo build --release
NETRACK_DATA_DIR=./data ./target/release/netrack
```

The web UI is embedded in the binary at compile time; there is nothing else to deploy. Set
`TZ` if the machine's time zone isn't what you want for scheduling and statistics.

## Behind a reverse proxy

netrack serves plain HTTP. To expose it over HTTPS, put it behind any reverse proxy. Example with
Caddy:

```
netrack.example.com {
    reverse_proxy 127.0.0.1:5216
}
```

Nginx needs buffering disabled for the live-update stream (`/api/speedtests/events` uses
Server-Sent Events):

```nginx
location / {
    proxy_pass http://127.0.0.1:5216;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_buffering off;
    proxy_read_timeout 1h;
}
```

When you expose netrack this way, also set:

- `NETRACK_SECURE_COOKIE=true`, so the session cookie is only sent over HTTPS.
- `NETRACK_TRUST_PROXY=true`, so the login lockout counts failures per real client IP (taken
  from the last `X-Forwarded-For` value) instead of the proxy's IP. Only enable it when netrack
  is reachable **only** through the proxy, otherwise clients can spoof the header.

Turn on a password too. See [Security](security.md).

## Upgrading

```bash
docker compose pull && docker compose up -d
```

Database migrations run automatically on startup. A backup is taken at startup and every day at
03:00, so a copy from before the upgrade is in `/data/backups/`. When a new release exists, the
web UI shows a dismissible banner (checked against GitHub releases at most once an hour).

## Boards without a real-time clock

Single-board computers without an RTC can boot with a clock in the past. netrack waits until the
system clock is later than the newest stored test before scheduling anything, checking every
30 seconds, so tests are never recorded with bogus timestamps.
