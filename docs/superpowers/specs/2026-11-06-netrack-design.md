# netrack — design

Data: 2026-11-06
Status: rascunho para aprovação

## 1. Intenção

O dono quer acompanhar a velocidade da internet da casa dele ao longo do tempo
(download, upload, ping, médias) olhando pelo navegador. Hoje a referência é o
[MySpeed](https://github.com/gnmyt/myspeed), mas ele acha o MySpeed **lento** e
**feio**. O netrack é uma reescrita em Rust com **paridade de funcionalidades**
com o MySpeed, mais rápida, mais leve e com uma interface bem mais caprichada.

Ambiente-alvo inicial: Orange Pi Zero 3 (ARM64, Allwinner H618, porta gigabit),
ligado ao roteador por cabo, rodando em Docker. Mas nada pode ficar amarrado a
esse hardware: o mesmo programa tem que rodar igual em x86_64 e em links mais
rápidos.

## 2. Objetivos

1. **Paridade com o MySpeed** (lista completa na seção 5).
2. **O programa nunca é o gargalo.** O teste mede a internet, não a CPU do
   servidor. O único limite aceitável é o físico (ex.: porta de 1 Gbps do Pi).
3. **Leve.** Um único binário, com a interface web embutida. Memória em repouso
   na casa de poucas dezenas de MB, no máximo. Imagem Docker pequena.
4. **Rápido para abrir.** Telas abrem "na hora", mesmo com anos de histórico
   (dezenas de milhares de testes).
5. **Bonito.** A interface segue a direção visual "C · Painel denso" (seção 6).

## 3. Não-objetivos

- Ser compatível com a API HTTP do MySpeed, ou ser substituto direto dele
  (fora a importação de dados, ver 5.9).
- Implementar o protocolo da Ookla em Rust (ver 4.3).
- App nativo de celular. O acesso é só pelo navegador.

## 4. Anti-objetivos (o que tornaria isso um fracasso, mesmo funcionando)

- Ficar tão pesado ou lento quanto o MySpeed: tela de estatísticas que demora
  com histórico grande, uso alto de memória, dependência de processos externos
  para os provedores principais.
- Medições subestimadas por limitação do próprio programa (por exemplo, uma
  única conexão TCP ou CPU saturada no Pi).
- Interface genérica e sem cuidado.

## 5. Funcionalidades (paridade com o MySpeed)

Levantado a partir do código do MySpeed v1.0.9.

### 5.1 Testes de velocidade
- Mede download (Mbps), upload (Mbps), ping (ms) e jitter (ms), além da duração
  do teste.
- Guarda servidor usado (id, nome, host), tipo (`auto` agendado / `custom`
  manual), id do resultado no provedor (quando existir) e mensagem de erro em
  caso de falha.
- Teste manual pelo botão "Testar agora".
- Só um teste por vez. Pedir um teste enquanto outro roda retorna erro claro.
- Em caso de falha, tenta de novo uma vez (como o MySpeed). Se falhar de novo,
  registra o teste como falho com a mensagem de erro.
- Pausar e retomar os testes automáticos.
- Opção de rodar um teste ao iniciar (`RUN_TEST_ON_STARTUP`).
- Durante um teste, a interface mostra que ele está em andamento.

### 5.2 Provedores
- **Cloudflare** (speed.cloudflare.com): implementado nativamente em Rust.
- **LibreSpeed**: implementado nativamente em Rust. Usa a lista pública de
  servidores ou um servidor próprio por URL.
- **Ookla**: opcional, usando o CLI oficial da Ookla (`speedtest`). *(Decisão
  do Claude)* Não vem na imagem: é baixado em tempo de execução para o volume
  de dados, na arquitetura certa, só depois que o usuário escolhe Ookla e
  aceita a licença na interface. Assim não redistribuímos o binário e a imagem
  continua pequena. Por isso a Ookla nunca é o provedor padrão.
- Escolha do provedor e do servidor nas configurações. Para Ookla e
  LibreSpeed, há lista de servidores para escolher e a opção "automático".
- Escolha da interface de rede de onde o teste sai, igual ao MySpeed
  (detecção das interfaces com acesso à internet). *(Decisão do Claude)* O
  `docker-compose` de exemplo usa `network_mode: host`, que é o recomendado:
  evita a camada de rede do Docker e deixa o programa ver as interfaces reais.
  Na rede bridge padrão o programa funciona, mas a lista só mostra a interface
  do contêiner.

### 5.3 Agendamento e retenção
- Agendamento por expressão cron (padrão: a cada hora, `0 * * * *`).
- Opção de deslocamento aleatório do horário (`scheduleOffset` no MySpeed),
  para não bater exatamente na virada da hora.
- Retenção configurável em dias (padrão 365, máximo 10000). Testes mais antigos
  são apagados automaticamente.

### 5.4 Estatísticas
- Período selecionável (de/até).
- Médias, mínimos e máximos de download, upload, ping e jitter.
- Médias por hora do dia.
- Mapa de calor por dia da semana × hora.
- Índice de consistência (100 − desvio-padrão/média × 100), como o MySpeed.
- Contagem de testes e falhas, e disponibilidade.
- *(Decisão do Claude)* Fuso horário: um só, o do servidor (variável `TZ`,
  com os dados de fuso embutidos no binário). Ele vale para as médias por
  hora, o mapa de calor e o cron. O navegador mostra os horários nesse fuso.
- Gráficos com no máximo ~300 pontos por série. Se houver mais dados, o
  servidor agrega por intervalo.
- **Todo cálculo e filtro de período acontece no banco**, com índice na data.
  Nunca carregar o histórico inteiro na memória.

### 5.5 Valores esperados ("plano")
- O usuário informa os valores esperados de download, upload e ping (o
  MySpeed chama de "recommendations" e calcula automaticamente a partir dos
  últimos 10 testes). Mantém o cálculo automático e permite valores manuais,
  por exemplo a velocidade contratada.
- A interface mostra o resultado como porcentagem do plano (medidores da
  tela principal).

### 5.6 Notificações / integrações
Mesmos eventos do MySpeed: teste iniciado, teste concluído, teste falhou,
"minuto passou" (heartbeat), configuração alterada, valores esperados
atualizados. Integrações:
- Discord, Telegram, Pushover, Gotify, ntfy, webhook genérico, InfluxDB,
  Healthchecks (ping de saúde com `/start` e `/fail`).
- Cada integração pode ter várias instâncias configuradas, cada uma com seus
  campos e escolha de quais eventos disparam envio, como no MySpeed.

### 5.7 Prometheus
- Endpoint `/api/prometheus/metrics` com as métricas do último teste (ping,
  jitter, download, upload, duração, info do servidor), com os rótulos de
  servidor.
- Protegido por Basic Auth quando há senha definida.
- Nomes de métrica: `netrack_*` (não `myspeed_*`).

### 5.8 Senha e acesso
- Sem senha por padrão.
- Com senha definida (guardada com hash), dois níveis, como no MySpeed:
  - **leitura liberada:** qualquer um vê, só quem tem a senha altera ou dispara
    testes;
  - **tudo protegido:** sem a senha, nada aparece.

### 5.9 Dados: importar, exportar, limpar
- Exportar histórico em JSON e CSV.
- Importar histórico em JSON, **incluindo o formato exportado pelo MySpeed**,
  para quem migrar.
- Exportar e importar configurações.
- Apagar todo o histórico e restaurar as configurações padrão.
- Ver o tamanho do banco.

### 5.10 Nós (várias instâncias)
- Uma instância do netrack pode cadastrar outras instâncias do netrack (URL +
  senha opcional) e mostrar todas num painel.
- Renomear, trocar senha e remover nós.
- *(Decisão do Claude)* Funciona como no MySpeed: o navegador fala só com a
  instância local, que repassa as chamadas para os nós. A senha de cada nó é
  guardada de forma que possa ser lida de volta (não como hash), porque o
  servidor precisa enviá-la. Isso evita CORS e problemas de HTTP/HTTPS
  misturados. O painel de nós mostra um card-resumo por nó (status e último
  resultado). Clicando, abre o painel completo daquele nó dentro da mesma
  interface. A API entre instâncias tem versão, e uma instância avisa quando um
  nó é de versão incompatível.

### 5.11 Prévia para redes sociais
- Imagem Open Graph (`/api/opengraph/image`) com o último resultado, para o
  link ficar bonito quando compartilhado. Respeita o nível de senha.

### 5.12 Modo prévia (demo)
- `PREVIEW_MODE=true`: gera resultados falsos, bloqueia alterações de senha,
  nós e dados, e mostra uma mensagem configurável (`PREVIEW_MESSAGE`).

### 5.13 Sistema
- Versão atual e aviso de versão nova disponível. *(Decisão do Claude)* A
  verificação consulta os releases do GitHub do projeto
  (`github.com/Diaszano/netrack`, a confirmar quando o repositório remoto
  existir). Sem internet, falha em silêncio.
- Status do próprio servidor (online, CPU, memória do processo), mostrado no
  canto da barra lateral.

## 6. Interface

Direção visual escolhida: **"C · Painel denso"**, desenhada no arquivo Penpot
do projeto (Page 1, board "C · Painel denso"). As boards A e B foram as
alternativas descartadas.

- Tema escuro azul-marinho (`#0E1020` fundo, `#13162A` barra lateral,
  `#171A31` cartões, `#262A47` bordas). Texto `#E8E9F5`, secundário `#8A8DB0`.
- Cores das métricas: download roxo `#8B7CFF`, upload verde-água `#3FD6C2`,
  ping rosa `#FF8FB1`.
- Tipografia: Space Grotesk (títulos e números grandes), Inter (texto),
  JetBrains Mono (números de tabela e eixos). Fontes embutidas no binário,
  sem CDN, porque o servidor pode estar sem acesso a serviços externos.
- Barra lateral: Visão geral, Estatísticas, Histórico, Servidores, Nós,
  Integrações, Ajustes. No rodapé, o card de status do servidor.
- Visão geral: três cards com medidor circular (valor atual e % do plano; ao
  lado, média/mín/máx), gráfico de linha de velocidade com área, mapa de calor
  dia × hora e tabela de testes recentes com selo ok/falha.
- **Idiomas:** internacionalização desde o início, com todos os textos em
  arquivos de tradução por chave e datas e números no formato do idioma.
  Começa com **pt-BR** e **en**. Adicionar um idioma deve ser só criar um
  arquivo novo. O idioma segue o navegador, e há um seletor nos Ajustes.
- Responsiva: no celular, a barra lateral vira menu e os cards empilham.
- Atualiza sozinha quando um teste termina, sem recarregar a página.

As telas que não foram desenhadas (Estatísticas, Histórico, Servidores, Nós,
Integrações, Ajustes, tela de senha) seguem a mesma linguagem visual.

## 7. Restrições técnicas (decididas)

- **Linguagem:** Rust.
- **Distribuição:** um binário único com a interface web embutida. Imagem
  Docker multi-arquitetura (linux/arm64 e linux/amd64), com volume para os
  dados. Imagem final mínima (distroless ou scratch + certificados).
- **Banco:** SQLite em arquivo no volume de dados.
- **Provedores:** Cloudflare e LibreSpeed nativos em Rust. Ookla via CLI
  oficial, opcional (o protocolo é fechado e os termos proíbem clientes não
  oficiais).
- **Medição sem gargalo:** download e upload com várias conexões em paralelo e
  I/O assíncrono, sem copiar ou guardar os bytes recebidos. Precisa saturar
  1 Gbps num Orange Pi Zero 3 usando bem menos que toda a CPU. *(Decisão do
  Claude)* O motor de medição não tem teto fixo: o número de conexões cresce
  conforme a vazão. Ele deve chegar a pelo menos 2,5 Gbps em x86 sem recursos
  especiais do kernel (nada de io_uring). O critério de aceite é saturar
  1 Gbps no Pi.
- **Configuração:** pela interface (guardada no banco) e por variáveis de
  ambiente para coisas de implantação (porta, caminho dos dados, modo prévia,
  teste ao iniciar).

## 8. Deixado para quem for construir

- Escolha de crates (servidor HTTP, cliente HTTP, acesso ao SQLite, cron) e do
  framework da interface web, desde que respeitem leveza e tempo de abertura.
- Esquema do banco, desde que cubra os campos da seção 5.1 e tenha índice por
  data.
- Formato da API HTTP interna entre interface e servidor.
- Algoritmo exato de medição (número de conexões, duração, descarte do
  aquecimento), desde que os resultados fiquem coerentes com o site oficial de
  cada provedor.
- Desenho das telas não desenhadas, seguindo a seção 6.
