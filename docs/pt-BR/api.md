# API HTTP

[← Documentação](../../README.pt-BR.md) · [English](../en/api.md)

Tudo o que a interface web faz passa por esta API JSON, então qualquer coisa que você consegue
clicar também dá para automatizar com scripts. URL base: `http://<host>:5216`.

- Os corpos de requisição e resposta são JSON (`content-type: application/json`).
- Erros retornam um status fora de 2xx com `{"error": "message"}`.
- Timestamps são **milissegundos** Unix; velocidades em Mbps, ping e jitter em ms, durações em
  segundos.
- No [modo de demonstração](security.md#modo-de-demonstração-preview), toda requisição que não é
  GET retorna `403`.

## Autenticação

Quais rotas exigem login depende do modo de proteção; veja
[Segurança](security.md#modos-de-proteção). Faça login uma vez e reutilize o token:

```bash
TOKEN=$(curl -s -X POST http://localhost:5216/api/auth/login \
  -H 'content-type: application/json' -d '{"password":"secret"}' | jq -r .token)

curl -H "Authorization: Bearer $TOKEN" http://localhost:5216/api/integrations
```

O login também define o cookie `netrack_session`, que os navegadores usam automaticamente.

| Método e caminho | Descrição |
|---|---|
| `GET /api/auth/status` | `{mode, authenticated}`: o modo de proteção e se quem chama está logado. |
| `POST /api/auth/login` | Corpo `{"password"}`. Retorna `{token, expiresAt}` e define o cookie. `401` para senha errada, `429` durante o bloqueio. |
| `POST /api/auth/logout` | Encerra a sessão atual. |
| `POST /api/auth/password` | Corpo `{password?, mode?, current_password?}`. Define ou remove a senha e o modo. |

## Saúde

| Método e caminho | Descrição |
|---|---|
| `GET /healthz` | `ok`. Sempre aberto; use-o nos health checks do contêiner. |

## Testes de velocidade

| Método e caminho | Descrição |
|---|---|
| `GET /api/speedtests?limit=30&before=<id>` | Testes, do mais recente para o mais antigo. `limit` 1–100 (padrão 30). `before` retorna testes com id menor que ele (paginação). |
| `GET /api/speedtests/{id}` | Um teste; `404` se não existir. |
| `DELETE /api/speedtests/{id}` | Exclui um teste (`204`). |
| `POST /api/speedtests/run` | Inicia um teste manual: `202 {"status":"started"}`, ou `409` se já houver um rodando. |
| `GET /api/speedtests/status` | `{running, paused, nextRun}`; `nextRun` é um timestamp, ou `null` quando pausado. |
| `POST /api/speedtests/pause` | Pausa o agendamento. |
| `POST /api/speedtests/continue` | Retoma o agendamento. |
| `GET /api/speedtests/statistics?from=YYYY-MM-DD&to=YYYY-MM-DD` | Estatísticas de um intervalo de datas locais (ambas obrigatórias, inclusivas). |
| `GET /api/speedtests/events` | Eventos ao vivo (Server-Sent Events). |

Um objeto de teste:

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

`kind` é `auto` (agendado ou na inicialização) ou `custom` (manual). Um teste que falhou tem
`error` preenchido e métricas nulas. `resultId` é o id do resultado no Ookla, quando houver.

### Executando um teste e aguardando o resultado

```bash
curl -s -X POST http://localhost:5216/api/speedtests/run
while curl -s http://localhost:5216/api/speedtests/status | grep -q '"running":true'; do sleep 2; done
curl -s 'http://localhost:5216/api/speedtests?limit=1'
```

### Estatísticas

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

- Médias e afins usam só testes bem-sucedidos; `availability` é a porcentagem de testes que
  tiveram sucesso (100 quando não há nenhum).
- `consistency` é `100 − stdDev / avg × 100`; `stdDev` é o desvio padrão populacional.
- `hourly` sempre tem 24 entradas; `heatmap` só tem células com dados (`weekday` 0 = segunda-feira).
- `series` tem no máximo 300 pontos; intervalos maiores são agregados pela média em faixas de
  tempo iguais, e `t` é o início de cada faixa.

### Eventos ao vivo

`GET /api/speedtests/events` é um fluxo de Server-Sent Events. O `data` de cada mensagem é um JSON
com um `type`:

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

## Configuração

| Método e caminho | Descrição |
|---|---|
| `GET /api/config` | Todos os ajustes em tempo de execução (chaves em camelCase). |
| `PATCH /api/config/{key}` | Corpo `{"value":"..."}` com a chave em snake_case. `204`, ou `400` para chave desconhecida ou valor inválido. |

Veja [Configuração](configuration.md#ajustes-em-tempo-de-execução) para todas as chaves.

## Sistema

| Método e caminho | Descrição |
|---|---|
| `GET /api/system/status` | `{version, latest_version, update_available, preview_mode, preview_message, uptimeSecs, cpuPercent, rssBytes, tz}`. CPU e memória são `null` fora do Linux. |
| `GET /api/system/interfaces` | Interfaces de rede e seus endereços. |
| `GET /api/system/servers/librespeed` | Lista pública de servidores LibreSpeed. |
| `GET /api/system/servers/ookla` | Servidores Ookla (`speedtest -L`), quando a CLI está instalada. |
| `GET /api/system/ookla/status` | `{installed, consented, arch}`. |
| `POST /api/system/ookla/consent` | Registra a aceitação dos termos de licença do Ookla. |
| `POST /api/system/ookla/install` | Baixa a CLI (requer consentimento; só `x86_64`/`aarch64`). |

## Integrações

| Método e caminho | Descrição |
|---|---|
| `GET /api/integrations` | Todas as integrações, com segredos mascarados. |
| `POST /api/integrations` | Cria uma (`201`). |
| `PUT /api/integrations/{id}` | Substitui uma; `***` mantém um segredo armazenado. |
| `DELETE /api/integrations/{id}` | Exclui uma. |
| `POST /api/integrations/{id}/test` | Envia uma notificação de teste: `200 {success, message}` ou `502`. |

Corpo e campos dos canais: [Integrações](integrations.md).

## Dados

| Método e caminho | Descrição |
|---|---|
| `GET /api/data/stats` | `{rowCount, dbSizeBytes, backups: [{name, sizeBytes}]}`. |
| `GET /api/data/export?format=json\|csv&from=&to=` | Baixa o histórico (datas opcionais, locais, inclusivas). |
| `POST /api/data/import` | Corpo: o conteúdo do arquivo (JSON/CSV do netrack ou JSON do MySpeed). Retorna `{imported, skipped}`. |
| `GET /api/data/backups/{name}` | Baixa um backup diário, ex.: `netrack-2026-11-24.db`. |
| `POST /api/data/reset` | Exclui todos os testes e restaura os ajustes padrão (a senha é mantida). |

Detalhes: [Dados](data.md).

## Nós

| Método e caminho | Descrição |
|---|---|
| `GET /api/nodes` | Nós remotos (`{id, name, url, hasPassword, createdAt}`). |
| `POST /api/nodes` | Corpo `{name, url, password?}`. |
| `GET /api/nodes/{id}` | Um nó. |
| `PUT` / `PATCH /api/nodes/{id}` | Atualiza; `"password": null` ou `""` apaga a senha, omiti-la a mantém. |
| `DELETE /api/nodes/{id}` | Remove. |
| `GET /api/nodes/{id}/status` | `{online, version, latestTest, error}`, consultado ao vivo. |
| `ANY /api/nodes/{id}/proxy/{path}` | Encaminha para `<node>/api/{path}`. |

Detalhes: [Múltiplos nós](nodes.md).

## Métricas e imagens

| Método e caminho | Descrição |
|---|---|
| `GET /api/prometheus/metrics` | Formato de texto do Prometheus. Veja [Observabilidade](observability.md#prometheus). |
| `GET /api/opengraph/image` | Card SVG de 1200×630 com o resultado mais recente. |
