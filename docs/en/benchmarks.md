# Benchmarks

[← Documentation](../../README.md) · [Português](../pt-BR/benchmarks.md)

Target: **Orange Pi Zero 3** (4 cores, 4 GB). Measured on 2026-11-24 at commit `aa1c994`,
Cloudflare provider (POA).

## Results

| Item | Run 1 | Run 2 | Run 3 | Acceptance |
|---|---|---|---|---|
| Ping (ms), netrack | 12.71 | 12.62 | 12.06 | ✅ within ±5 ms of Cloudflare |
| Ping (ms), speed.cloudflare.com | 15.6 | 15.4 | — | |
| Download (Mbps), netrack | 217.07 | 218.50 | 218.19 | ✅ within ±10% of Cloudflare |
| Download (Mbps), speed.cloudflare.com | 212 | 212 | — | |
| Upload (Mbps), netrack | 260.32 | 260.81 | 261.52 | ✅ within ±10% (see note) |
| Upload (Mbps), speed.cloudflare.com | 138 | 123 | — | |
| Upload (Mbps), parallel `curl` on the Pi | 240.60 (8 connections) | 234.44 (16 connections) | — | |
| Peak process CPU (`top`) | 40.0% | 44.0% | 58.8% | ✅ < 80% of one core |

| Item | Result | Acceptance |
|---|---|---|
| RSS at rest | 6.6–7.4 MB | ✅ < 30 MB |
| `statistics` over 1 year | 0.38 s (17,520 synthetic tests, one every 30 min) | record the time |

**About upload:** speed.cloudflare.com in a browser and a single-connection `curl` (91–153 Mbps)
don't saturate the link, so they aren't a fair reference for netrack, which uses up to 32
connections. The reference used was the total time of 8 and 16 simultaneous 25 MB uploads to
`https://speed.cloudflare.com/__up` from the Pi itself, which includes the server's
acknowledgement. netrack came out about 8% above it: within the acceptance criterion, and
consistent with a small overestimate from counting bytes when they're handed to the HTTP client
(step 5 below).

The browser ran on a Mac on the same network, not on a wired PC.

## Procedure

1. Build the multi-arch image (requires `docker buildx`):
   `docker buildx build --platform linux/arm64,linux/amd64 -t netrack:dev .`
2. Run netrack on the Pi with `network_mode: host`.
3. Run 3 tests: `curl -XPOST http://<pi>:5216/api/speedtests/run`, wait for it to finish
   (`/api/speedtests/status`) and record ping, download and upload from `GET /api/speedtests`.
4. Compare with https://speed.cloudflare.com in a browser on a wired PC, on the same network and
   at the same time. Acceptance: download and upload within ±10%; ping within ±5 ms.
5. Check upload carefully. netrack counts upload bytes when they're handed to the HTTP client,
   not when the server acknowledges them; an upload consistently above the reference means
   overcounting and must be fixed before accepting.
6. During a test, run `top` on the Pi and record the netrack process's peak CPU. Acceptance:
   < 80% of ONE core (in `top`, 100% = 1 core; the Pi Zero 3 has 4 cores, so the maximum is 400%).
7. At rest, read `rssBytes` from `GET /api/system/status`. Acceptance: < 30 MB.
8. Time `curl -s 'http://<pi>:5216/api/speedtests/statistics?from=YYYY-MM-DD&to=YYYY-MM-DD'`
   with one year of history and record it.
