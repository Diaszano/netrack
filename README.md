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
| `TZ` | `UTC` | Fuso horário usado no agendamento (ex.: `America/Sao_Paulo`) |
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

1. Suba o netrack no Pi com `network_mode: host` (compose acima).
2. Execute 3 testes: `curl -XPOST http://<pi>:5216/api/speedtests/run`, aguarde
   terminar (`/api/speedtests/status`) e anote download/upload de
   `GET /api/speedtests`.
3. Compare com https://speed.cloudflare.com em um navegador num PC cabeado, na
   mesma rede e momento. Aceite: diferença dentro de ±10%.
4. Durante um teste, rode `top` no Pi e anote o pico de CPU. Aceite: < 80%.
5. Em repouso, leia `rssBytes` em `GET /api/system/status`. Aceite: < 30 MB.
6. Meça `time curl -s 'http://<pi>:5216/api/speedtests/statistics?from=AAAA-MM-DD&to=AAAA-MM-DD'`
   com histórico de 1 ano e anote o tempo.

| Item | Execução 1 | Execução 2 | Execução 3 | Aceite |
|---|---|---|---|---|
| Download (Mbps), netrack | pendente | pendente | pendente | ±10% do Cloudflare |
| Download (Mbps), speed.cloudflare.com | pendente | pendente | pendente | |
| Upload (Mbps), netrack | pendente | pendente | pendente | ±10% do Cloudflare |
| Upload (Mbps), speed.cloudflare.com | pendente | pendente | pendente | |
| Pico de CPU (`top`) | pendente | pendente | pendente | < 80% |

| Item | Resultado | Aceite |
|---|---|---|
| RSS em repouso | pendente | < 30 MB |
| `statistics` com 1 ano | pendente | anotar tempo |
