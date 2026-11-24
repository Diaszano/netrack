# Configuração

[← Documentação](../../README.pt-BR.md) · [English](../en/configuration.md)

O netrack tem dois tipos de configuração:

- **Variáveis de ambiente**, lidas uma vez na inicialização: porta, diretório de dados, fuso
  horário, comportamento de proxy e de cookie, OpenTelemetry.
- **Ajustes em tempo de execução**, guardados no banco e alterados pela interface web (páginas
  **Ajustes** e **Servidores**) ou pela API (`PATCH /api/config/{key}`). Entram em vigor
  imediatamente, sem reiniciar.

## Variáveis de ambiente

| Variável | Padrão | Descrição |
|---|---|---|
| `NETRACK_PORT` | `5216` | Porta HTTP. Escuta em `0.0.0.0` e `[::]`. |
| `NETRACK_DATA_DIR` | `/data` | Diretório do banco SQLite, dos backups diários e do binário opcional do Ookla. |
| `TZ` | `UTC` | Fuso horário IANA (ex.: `America/Sao_Paulo`) usado no agendamento cron, na manutenção das 03:00, nas estatísticas (hora do dia, dia da semana, intervalos de datas) e nos nomes dos arquivos de backup. Vazio significa `UTC`. |
| `RUN_TEST_ON_STARTUP` | `false` | Executa um teste assim que o servidor inicia. |
| `NETRACK_TRUST_PROXY` | `false` | Usa o último valor de `X-Forwarded-For` como IP do cliente no bloqueio de login. Ative só atrás de um proxy reverso confiável. |
| `NETRACK_SECURE_COOKIE` | `false` | Adiciona `Secure` ao cookie de sessão. Ative só quando os usuários acessam o netrack via HTTPS; caso contrário, os navegadores descartam o cookie e o login para de funcionar. |
| `PREVIEW_MODE` | `false` | Modo de demonstração: toda alteração pela API é recusada com `403` e a interface mostra um aviso. Veja [Segurança](security.md#modo-de-demonstração-preview). |
| `PREVIEW_MESSAGE` | *(padrão em português)* | Texto do aviso de demonstração e do erro `403`. |
| `RUST_LOG` | `info` | Nível/filtro de log (`warn`, `info`, `debug`, `netrack=debug,tower_http=info`, ...). |
| `OTEL_*` | *(não definida)* | Exportação OpenTelemetry. Veja [Observabilidade](observability.md#opentelemetry). |

Variáveis booleanas aceitam apenas `true`/`1` e `false`/`0`; qualquer outro valor (ex.: `yes`)
interrompe o netrack na inicialização com um erro, para que erros de digitação não passem
despercebidos.

> Alterar `TZ` é seguro: na próxima inicialização, o netrack recalcula a hora local e o dia da
> semana armazenados de cada teste, para que as estatísticas e o mapa de calor sigam o novo fuso.

## Ajustes em tempo de execução

Todos os valores são strings na API (`{"value": "..."}`), e `GET /api/config` os retorna em
camelCase (`expectedDownload`, ...). O título de cada seção indica onde o ajuste é editado na
interface.

### Agendamento (Ajustes → Agendamento)

| Chave | Padrão | Valores válidos | Significado |
|---|---|---|---|
| `cron` | `0 * * * *` | Expressão cron com 5 campos (6 ou 7 com segundos/anos), ou apelidos como `@hourly`, `@daily` | Quando os testes automáticos rodam, em `TZ`. |
| `schedule_offset` | `true` | `true`/`false` | Adiciona um atraso aleatório a cada execução, entre 0 e o menor valor entre 5 minutos e um quarto do intervalo. Evita que muitas instalações acessem os servidores de teste no mesmo segundo. |
| `paused` | `false` | `true`/`false` | Interrompe os testes automáticos. Testes manuais continuam funcionando. Equivale à API de pausar/continuar. |
| `retention_days` | `365` | 1–10000 | Testes mais antigos que isso são excluídos pela rotina diária das 03:00. |
| `backup_keep` | `7` | 0–30 | Número de backups diários mantidos. `0` desativa novos backups (os arquivos existentes não são tocados). |

Exemplos de cron:

| Expressão | Executa |
|---|---|
| `0 * * * *` | A cada hora |
| `*/30 * * * *` | A cada 30 minutos |
| `0 */4 * * *` | A cada 4 horas |
| `0 8-23 * * *` | De hora em hora, das 08:00 às 23:00 |
| `@daily` | Uma vez por dia, à meia-noite |

Se uma execução agendada chegar enquanto outro teste ainda está rodando, ela é pulada. Uma
expressão inválida é informada nos logs e tentada novamente a cada minuto.

### Plano contratado (Ajustes → Plano contratado)

Esses valores alimentam os medidores ("% do plano") e os alertas de lentidão.

| Chave | Padrão | Valores válidos | Significado |
|---|---|---|---|
| `expected_mode` | `auto` | `auto`, `manual` | De onde vêm os valores esperados. |
| `expected_download` | `100` | número ≥ 0 (Mbps) | Velocidade de download contratada. `0` desativa a verificação de lentidão no download. |
| `expected_upload` | `50` | número ≥ 0 (Mbps) | Velocidade de upload contratada. `0` desativa a verificação de lentidão no upload. |
| `expected_ping` | `25` | número ≥ 0 (ms) | Latência de referência, mostrada no medidor de ping. |
| `degraded_threshold_pct` | `70` | 1–100 | Um teste é *lento* quando o download **ou** o upload fica abaixo dessa porcentagem do valor esperado. O ping não é considerado. |

- **`manual`**: você digita as velocidades do seu plano; o netrack nunca as altera.
- **`auto`**: após cada teste bem-sucedido, a partir de 10 testes bem-sucedidos, o netrack define
  o download e o upload esperados como o **maior** valor e o ping como o **menor** valor vistos
  nos últimos 10 testes bem-sucedidos. Os campos ficam somente leitura na interface. Use quando
  você não sabe qual é o seu plano, mas lembre que o "% do plano" passa a ser relativo aos
  melhores resultados recentes, e não ao que você paga.

### Provedor (página Servidores)

| Chave | Padrão | Valores válidos | Significado |
|---|---|---|---|
| `provider` | `cloudflare` | `cloudflare`, `librespeed`, `ookla` | Qual motor executa os testes. |
| `libre_server_id` | `auto` | `auto` ou um id de servidor | Servidor LibreSpeed da lista pública. `auto` faz ping em todos os servidores da lista (timeout de 2 s) e escolhe o que responde mais rápido. |
| `libre_url` | `none` | `none` ou uma URL `http(s)://` | Sua própria instância do LibreSpeed. Tem prioridade sobre `libre_server_id`. O netrack usa os endpoints `garbage.php` e `empty.php` dela. |
| `ookla_server_id` | `auto` | `auto` ou um id de servidor | Servidor Ookla (a lista vem de `speedtest -L`). `auto` deixa a CLI escolher. |
| `ookla_license_agreed` | `false` | `true`/`false` | Definido pelo botão de consentimento; necessário antes de instalar a CLI. |
| `interface` | `auto` | `auto` ou um nome de interface (ex.: `eth0`) | Vincula o tráfego dos testes ao endereço IPv4 dessa interface (ou ao primeiro endereço dela, se não houver IPv4). Uma interface que não existe mais volta silenciosamente ao padrão do sistema operacional; a página Servidores sinaliza isso. |

Como os provedores medem:

- **Cloudflare** e **LibreSpeed** usam o motor próprio do netrack: 20 amostras de latência
  (ping = mediana, jitter = diferença média entre amostras consecutivas), depois 10 s de download
  e 10 s de upload. Cada fase começa com 4 conexões paralelas e adiciona mais 4 (até 32) enquanto
  a vazão continua crescendo durante um aquecimento de 2 s; só o tráfego depois do aquecimento
  conta.
- **Ookla** executa a CLI oficial (`speedtest --format=json-pretty`) com timeout de 60 s. A CLI
  não vem junto: na página Servidores, **Aceitar termos e instalar Ookla CLI** registra seu
  consentimento com o EULA, a política de privacidade e os termos de uso da Ookla e, em seguida,
  baixa a versão 1.2.0 para `x86_64` ou `aarch64` em `/data/bin/speedtest`.

Uma tentativa que falha é repetida uma vez antes de o teste ser registrado como falho.

### Idioma (Ajustes → Idioma)

O idioma da interface é uma preferência do navegador (guardada em `localStorage`), não um ajuste
do servidor. **Automático** escolhe o primeiro idioma do navegador que corresponde a um locale
incluído (`en`, `pt-BR`) e, se nenhum corresponder, usa `pt-BR`.

Adicionar um idioma significa adicionar `web/locales/<code>.json` com uma entrada `_meta.name` e
recompilar: os arquivos de locale são embutidos em tempo de compilação.

### Segurança (Ajustes → Segurança e Acesso)

`password_mode` e a própria senha não podem ser alterados por `PATCH /api/config`; use o
formulário de segurança ou `POST /api/auth/password`. Veja [Segurança](security.md).

## Alterando ajustes pela API

```bash
curl -X PATCH http://localhost:5216/api/config/cron \
  -H 'content-type: application/json' \
  -d '{"value":"*/30 * * * *"}'
```

Retorna `204` em caso de sucesso ou `400` com `{"error": "..."}` para uma chave desconhecida ou um
valor inválido. Toda alteração dispara o evento de integração `config.updated`. Com uma senha
definida, envie o cookie de sessão ou `Authorization: Bearer <token>` (veja
[API HTTP](api.md#autenticação)).
