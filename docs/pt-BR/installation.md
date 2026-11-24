# Instalação

[← Documentação](../../README.pt-BR.md) · [English](../en/installation.md)

O netrack é distribuído como uma imagem de contêiner multiarquitetura (`linux/amd64`, `linux/arm64`)
publicada no GitHub Container Registry a cada release:

```
ghcr.io/diaszano/netrack:latest     # release mais recente
ghcr.io/diaszano/netrack:0.2.0      # versão exata
ghcr.io/diaszano/netrack:0.2        # último patch de uma versão minor
```

A imagem é construída `FROM scratch`: contém apenas o binário estático, os certificados de CA e um
diretório `/data` vazio. Ela roda como UID `65534` (`nobody`), escuta na porta `5216` e guarda
tudo em `/data`.

## Docker Compose (recomendado)

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

Depois, abra `http://<host>:5216`.

### Por que `network_mode: host`?

Um teste de velocidade mede o caminho de rede que o processo usa. Com a rede bridge padrão do
Docker, o tráfego passa por NAT e por uma interface virtual. Isso costuma custar pouco, mas esconde
as interfaces do host, e você não consegue escolher uma em **Servidores → Interface de rede**. Com
a rede do host, o netrack enxerga as interfaces reais e se vincula a elas.

Se você precisar usar uma rede bridge, publique a porta:

```yaml
    ports:
      - "5216:5216"
```

### Permissões do diretório de dados

O contêiner roda como `65534:65534`, então um diretório montado via bind mount precisa ter
permissão de escrita para esse UID (`chown 65534:65534 ./data`). Um volume nomeado do Docker já
tem o dono correto:

```yaml
    volumes:
      - netrack-data:/data
volumes:
  netrack-data:
```

`/data` contém:

| Caminho | Conteúdo |
|---|---|
| `netrack.db` (+ `-wal`, `-shm`) | Banco SQLite: histórico, ajustes, integrações, nós, sessões |
| `backups/netrack-YYYY-MM-DD.db` | Backups diários (veja [Dados](data.md)) |
| `bin/speedtest` | Ookla CLI, só depois que você aceita a licença |

## Gerando a imagem você mesmo

```bash
docker build -t netrack:dev .
# ou para as duas arquiteturas (requer buildx)
docker buildx build --platform linux/arm64,linux/amd64 -t netrack:dev .
```

O Dockerfile faz compilação cruzada no host de build com `cargo zigbuild`, gerando binários musl
estáticos, então gerar a imagem arm64 em uma máquina x86 (ou o contrário) é rápido e não precisa
de emulação. O `docker-compose.example.yml` do repositório usa essa tag `netrack:dev` gerada
localmente.

## Rodando o binário sem Docker

Requer a toolchain Rust fixada em `rust-toolchain.toml`.

```bash
cargo build --release
NETRACK_DATA_DIR=./data ./target/release/netrack
```

A interface web é embutida no binário em tempo de compilação; não há mais nada para implantar.
Defina `TZ` se o fuso horário da máquina não for o que você quer para o agendamento e as
estatísticas.

## Atrás de um proxy reverso

O netrack serve HTTP simples. Para expô-lo via HTTPS, coloque-o atrás de qualquer proxy reverso.
Exemplo com Caddy:

```
netrack.example.com {
    reverse_proxy 127.0.0.1:5216
}
```

O Nginx precisa de buffering desativado para o fluxo de atualizações ao vivo
(`/api/speedtests/events` usa Server-Sent Events):

```nginx
location / {
    proxy_pass http://127.0.0.1:5216;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_buffering off;
    proxy_read_timeout 1h;
}
```

Ao expor o netrack dessa forma, defina também:

- `NETRACK_SECURE_COOKIE=true`, para que o cookie de sessão só seja enviado via HTTPS.
- `NETRACK_TRUST_PROXY=true`, para que o bloqueio de login conte as falhas pelo IP real do cliente
  (obtido do último valor de `X-Forwarded-For`) em vez do IP do proxy. Só ative quando o netrack
  estiver acessível **apenas** pelo proxy; caso contrário, os clientes podem falsificar o
  cabeçalho.

Ative também uma senha. Veja [Segurança](security.md).

## Atualizando

```bash
docker compose pull && docker compose up -d
```

As migrações do banco rodam automaticamente na inicialização. Um backup é feito ao iniciar e todo
dia às 03:00, então há uma cópia de antes da atualização em `/data/backups/`. Quando existe uma
nova release, a interface web mostra um aviso que pode ser dispensado (verificado nas releases do
GitHub no máximo uma vez por hora).

## Placas sem relógio de tempo real

Computadores de placa única sem RTC podem iniciar com o relógio no passado. O netrack espera até
que o relógio do sistema esteja depois do teste mais recente armazenado antes de agendar qualquer
coisa, verificando a cada 30 segundos, para que os testes nunca sejam gravados com horários
errados.
