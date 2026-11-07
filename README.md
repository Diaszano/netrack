# netrack

Monitor de velocidade de internet auto-hospedado, escrito em Rust. Roda testes
de velocidade (Cloudflare e LibreSpeed) em intervalos agendados, guarda o
histórico em SQLite e expõe tudo por uma API JSON. Imagem Docker estática
(`scratch`, ~11 MB) para `linux/amd64` e `linux/arm64`.

## Como rodar

```bash
docker build -t netrack:dev .
mkdir -p data && sudo chown 65534:65534 data
docker compose -f docker-compose.example.yml up -d
curl http://localhost:5216/healthz   # ok
```

Use `docker-compose.example.yml` como base. Ele usa `network_mode: host` para
que o teste meça a interface real do host.

O container roda como UID 65534 (`nobody`). O diretório montado em `/data`
precisa ser gravável por esse UID (`chown 65534:65534 ./data`). Com volume
nomeado do Docker, as permissões já vêm corretas da imagem.

## Variáveis de ambiente

| Variável | Padrão | Descrição |
|---|---|---|
| `NETRACK_PORT` | `5216` | Porta HTTP (escuta em `0.0.0.0` e `[::]`) |
| `NETRACK_DATA_DIR` | `/data` | Onde fica o banco SQLite |
| `TZ` | `UTC` | Fuso horário usado no agendamento (ex.: `America/Sao_Paulo`); vazio = `UTC` |
| `RUN_TEST_ON_STARTUP` | `false` | Executa um teste ao iniciar |
| `RUST_LOG` | `info` | Nível de log (`info`, `debug`, ...) |

## Portas

- `5216/tcp`: API HTTP (sem `network_mode: host`, publique com `-p 5216:5216`).

## API

- `GET /healthz`
- `GET /api/speedtests`
- `POST /api/speedtests/run`
- `GET /api/speedtests/status`
- `POST /api/speedtests/pause`
- `POST /api/speedtests/continue`
- `GET /api/speedtests/statistics`
- `GET /api/speedtests/events` (SSE)
- `GET /api/speedtests/{id}`, `DELETE /api/speedtests/{id}`
- `GET /api/config`, `PATCH /api/config/{key}`
- `GET /api/system/status`
- `GET /api/system/interfaces`
- `GET /api/system/servers/librespeed`

## Benchmarks

Alvo: Orange Pi Zero 3. Resultados **pendentes** (requer o hardware).

Procedimento de aceite:

1. Gere a imagem multi-arquitetura (requer `docker buildx`):
   `docker buildx build --platform linux/arm64,linux/amd64 -t netrack:dev .`
2. Suba o netrack no Pi com `network_mode: host` (compose acima).
3. Execute 3 testes: `curl -XPOST http://<pi>:5216/api/speedtests/run`, aguarde
   terminar (`/api/speedtests/status`) e anote ping, download e upload de
   `GET /api/speedtests`.
4. Compare com https://speed.cloudflare.com em um navegador num PC cabeado, na
   mesma rede e momento. Aceite: download e upload dentro de ±10%; ping dentro
   de ±5 ms.
5. Upload: confira com atenção. O netrack conta os bytes de upload quando são
   entregues ao cliente HTTP, não quando o servidor confirma o recebimento; um
   upload consistentemente acima do Cloudflare indica contagem superestimada e
   precisa ser corrigido antes de aceitar.
6. Durante um teste, rode `top` no Pi e anote o pico de CPU do processo
   `netrack`. Aceite: < 80% de UM núcleo (no `top`, 100% = 1 núcleo; o Pi Zero 3
   tem 4 núcleos, então o máximo possível é 400%).
7. Em repouso, leia `rssBytes` em `GET /api/system/status`. Aceite: < 30 MB.
8. Meça `time curl -s 'http://<pi>:5216/api/speedtests/statistics?from=AAAA-MM-DD&to=AAAA-MM-DD'`
   com histórico de 1 ano e anote o tempo.

| Item | Execução 1 | Execução 2 | Execução 3 | Aceite |
|---|---|---|---|---|
| Ping (ms), netrack | pendente | pendente | pendente | ±5 ms do Cloudflare |
| Ping (ms), speed.cloudflare.com | pendente | pendente | pendente | |
| Download (Mbps), netrack | pendente | pendente | pendente | ±10% do Cloudflare |
| Download (Mbps), speed.cloudflare.com | pendente | pendente | pendente | |
| Upload (Mbps), netrack | pendente | pendente | pendente | ±10% do Cloudflare |
| Upload (Mbps), speed.cloudflare.com | pendente | pendente | pendente | |
| Pico de CPU do processo (`top`) | pendente | pendente | pendente | < 80% de um núcleo |

| Item | Resultado | Aceite |
|---|---|---|
| RSS em repouso | pendente | < 30 MB |
| `statistics` com 1 ano | pendente | anotar tempo |
