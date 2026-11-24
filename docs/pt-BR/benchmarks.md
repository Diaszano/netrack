# Benchmarks

[← Documentação](../../README.pt-BR.md) · [English](../en/benchmarks.md)

Alvo: **Orange Pi Zero 3** (4 núcleos, 4 GB). Medido em 2026-11-24 com o commit `aa1c994`,
provedor Cloudflare (POA).

## Resultados

| Item | Execução 1 | Execução 2 | Execução 3 | Aceite |
|---|---|---|---|---|
| Ping (ms), netrack | 12,71 | 12,62 | 12,06 | ✅ ±5 ms do Cloudflare |
| Ping (ms), speed.cloudflare.com | 15,6 | 15,4 | — | |
| Download (Mbps), netrack | 217,07 | 218,50 | 218,19 | ✅ ±10% do Cloudflare |
| Download (Mbps), speed.cloudflare.com | 212 | 212 | — | |
| Upload (Mbps), netrack | 260,32 | 260,81 | 261,52 | ✅ ±10% (ver nota) |
| Upload (Mbps), speed.cloudflare.com | 138 | 123 | — | |
| Upload (Mbps), `curl` paralelo no Pi | 240,60 (8 conexões) | 234,44 (16 conexões) | — | |
| Pico de CPU do processo (`top`) | 40,0% | 44,0% | 58,8% | ✅ < 80% de um núcleo |

| Item | Resultado | Aceite |
|---|---|---|
| RSS em repouso | 6,6–7,4 MB | ✅ < 30 MB |
| `statistics` com 1 ano | 0,38 s (17.520 testes sintéticos, 1 a cada 30 min) | anotar tempo |

**Sobre o upload:** o speed.cloudflare.com no navegador e o `curl` com uma conexão (91–153 Mbps)
não saturam o link, então não servem de referência para o netrack, que usa até 32 conexões. A
referência usada foi o tempo total de 8 e 16 uploads simultâneos de 25 MB para
`https://speed.cloudflare.com/__up` feitos do próprio Pi, que inclui a confirmação do servidor. O
netrack ficou cerca de 8% acima dela: dentro do aceite, e consistente com uma pequena
superestimação pela contagem na entrega ao cliente HTTP (passo 5 abaixo).

O navegador rodou num Mac na mesma rede, não num PC cabeado.

## Procedimento

1. Gere a imagem multi-arquitetura (requer `docker buildx`):
   `docker buildx build --platform linux/arm64,linux/amd64 -t netrack:dev .`
2. Suba o netrack no Pi com `network_mode: host`.
3. Execute 3 testes: `curl -XPOST http://<pi>:5216/api/speedtests/run`, aguarde terminar
   (`/api/speedtests/status`) e anote ping, download e upload de `GET /api/speedtests`.
4. Compare com https://speed.cloudflare.com em um navegador num PC cabeado, na mesma rede e
   momento. Aceite: download e upload dentro de ±10%; ping dentro de ±5 ms.
5. Upload: confira com atenção. O netrack conta os bytes de upload quando são entregues ao
   cliente HTTP, não quando o servidor confirma o recebimento; um upload consistentemente acima
   da referência indica contagem superestimada e precisa ser corrigido antes de aceitar.
6. Durante um teste, rode `top` no Pi e anote o pico de CPU do processo netrack. Aceite: < 80%
   de UM núcleo (no `top`, 100% = 1 núcleo; o Pi Zero 3 tem 4 núcleos, então o máximo possível é
   400%).
7. Em repouso, leia `rssBytes` em `GET /api/system/status`. Aceite: < 30 MB.
8. Meça o tempo de `curl -s 'http://<pi>:5216/api/speedtests/statistics?from=YYYY-MM-DD&to=YYYY-MM-DD'`
   com histórico de 1 ano e anote-o.
