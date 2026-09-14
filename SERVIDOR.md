# O servidor

**Para levantar uma máquina do zero, em ordem de execução, o roteiro é
[infra/INSTALAR-VPS.md](infra/INSTALAR-VPS.md).** Aquele arquivo tem o comando de cada
passo; este tem o porquê — as medições da máquina que existe, o que cada número
resolveu, e o que desligar.

Hoje é um Ubuntu 24.04 na Contabo com 4 vCPU e 8 GB de RAM, acumulando servidor de
mídia, site, e-mail, repositório APT e runner, mais meia dúzia de projetos mortos. A
máquina que substitui essa é um **Cloud VPS 8** — 8 vCPU, 24 GB, 300 GB de SSD, porta de
600 Mb/s — e hospeda **duas coisas**: o servidor de e-mail e este projeto.

Escrito olhando a máquina que existe, não de memória. O que estiver aqui foi verificado;
o que não deu para verificar está marcado.

## Onde está cada assunto

| Assunto | Onde |
|---|---|
| Instalar do zero, na ordem, com comando copiável | [infra/INSTALAR-VPS.md](infra/INSTALAR-VPS.md) |
| Teto de espectadores por banda, e a região | [infra/INSTALAR-VPS.md](infra/INSTALAR-VPS.md), seções 1.2 a 1.4 |
| Migrar o e-mail sem derrubar a caixa | [infra/INSTALAR-VPS.md](infra/INSTALAR-VPS.md), seção 11 |
| Quais portas abrir, e por que a faixa é larga | [UDP.md](UDP.md) e o roteiro, seção 2 |
| O caminho da imagem e os buffers de socket | [REDE.md](REDE.md) |
| Publicar uma versão no repositório APT | aqui embaixo, seção 2 |
| O que está medido nesta máquina e o que desligar | aqui embaixo, seções 4 e 5 |

## 1. Firewall: por que ele é sempre o primeiro suspeito

**O firewall da Contabo é do painel, não da máquina.** `ufw status` diz `inactive`, o
`iptables` está limpo, e mesmo assim o tráfego é descartado antes de chegar. Procurar no
servidor não acha nada, e foi isso que custou uma noite inteira.

A tabela de portas mora no roteiro (seção 2), porque ela muda com `SFU_WORKERS`. O que
não muda é o sintoma: **o mediasoup sorteia** uma porta dentro da faixa do worker e só
confere se ela está livre nesta máquina, nunca se é alcançável de fora. Uma porta
sorteada fora do que o firewall abre vira uma transmissão em que todo contador marca
saúde, o socket aceita cada byte, e nada chega do outro lado. Preto, reinicia e pega,
muda a qualidade e morre de novo.

Confira de fora, não de dentro. TCP dá para testar com `nc -z`; UDP só com uma captura
do outro lado:

```bash
# na VPS
sudo tcpdump -nn -i any 'udp and dst portrange 41000-42000'

# na sua máquina
printf 'teste' | nc -u -w0 SEU.IP.AQUI 41500
```

Se não aparecer no tcpdump, é o firewall do painel, não o código.

## 2. O repositório APT

É por aqui que o Linux instala e atualiza. O formato é o "plano": um diretório
só, sem a árvore `dists/pool`. O APT aceita, e do lado de quem instala isso é uma
linha de `sources.list` em vez de quatro.

### A chave, uma vez só

**Esta é a chave do repositório, e não é a mesma que assina as atualizações do
app.** Confundir as duas é o erro mais caro do processo — ver o
[AUTO-UPDATE.md](AUTO-UPDATE.md).

```bash
sudo apt install -y gnupg dpkg-dev apt-utils

gpg --batch --gen-key <<'EOF'
%no-protection
Key-Type: RSA
Key-Length: 4096
Name-Real: Unkvoid APT (assinatura do repositorio de pacotes)
Name-Email: repo@unkvoid.com
Expire-Date: 0
%commit
EOF

# A pública vai para o repositório, é ela que cada máquina importa uma vez.
gpg --export repo@unkvoid.com > /var/www/apt/unkvoid.gpg
```

Sem senha (`%no-protection`) porque o `apt-publish.sh` assina sem terminal, a
cada build. Quem tiver essa chave publica pacote como se fosse você, então ela
não sai desta máquina. Se sumir, gere outra e cada máquina instalada precisa
importar a nova.

Guarde uma cópia fora da VPS:

```bash
gpg --export-secret-keys --armor repo@unkvoid.com > unkvoid-apt-secreta.asc
```

### Publicar uma versão

Do Mac:

```bash
make build-vps
```

Ele sincroniza o código por rsync — o repositório é privado, e clonar de lá
exigiria uma credencial do GitHub guardada numa máquina exposta —, compila o
`.deb` e chama o `apt-publish.sh`, que refaz o `Packages`, o `Release` e assina
o `InRelease`.

O build divide a máquina com o SFU, então roda com um núcleo de folga e em
prioridade baixa. Alguns minutos a mais, e nenhuma transmissão engasgando no
meio.

### Do lado de quem instala

```bash
curl -fsSL https://unkvoid.com/apt/unkvoid.gpg \
  | sudo tee /usr/share/keyrings/unkvoid.gpg > /dev/null
echo "deb [signed-by=/usr/share/keyrings/unkvoid.gpg] https://unkvoid.com/apt ./" \
  | sudo tee /etc/apt/sources.list.d/unkvoid.list
sudo apt update && sudo apt install unkvoid
```

Depois disso, `sudo apt upgrade` junto com o resto da máquina. **No Linux o
atualizador embutido do app está desligado de propósito**: pedir senha de root
com `pkexec` no meio da abertura faria o que o `apt` já faz.

Para conferir que a assinatura fecha, de uma máquina limpa:

```bash
gpg --verify <(curl -s https://unkvoid.com/apt/InRelease)
```

## 3. Downloads e manifesto de atualização

Nada disso mora no disco do servidor. Os instaladores do macOS e do Windows vão
para o MinIO pela API assinada de `publish-release.sh`, e o `latest.json` que o
app lê é montado pelo Laravel a cada pedido, em `/downloads/latest.json`, a
partir da tabela `releases`. Cada sistema compila numa máquina diferente e
registra só a sua linha, então publicar o Windows não apaga o macOS que subiu
ontem.

**A chave que assina atualizações nunca vem para cá.** O Linux não a usa, e esta
máquina é exposta à internet: quem a tiver publica atualização para todo mundo
que instalou o app. O passo a passo dela está no
[AUTO-UPDATE.md](AUTO-UPDATE.md).

## 4. A config que aguenta o tráfego

Medido nesta máquina em 12/09/2026: 4 vCPU (AMD EPYC), 7941 MB de RAM com 5715
livres, 145 GB de disco com 38 usados, load 0,11 e 0,06% de steal. **O que estoura
primeiro é a banda de saída, não a CPU.** O SFU replica a transmissão por
espectador: 1080p60 a 10 Mb/s vira 180 Mb/s com 17 pessoas assistindo, e aí acaba
o uplink. A CPU dos workers, nesse ponto, está empurrando ~19 mil pacotes por
segundo divididos por quatro núcleos — uma ordem de grandeza longe de saturar.

**Antes de qualquer ajuste, o SFU que está no ar é de um deploy velho.** O `pm2 env`
dele mostra `SFU_PLAIN_PORTS=8` e nenhum `SFU_CONNECTIONS_PER_MINUTE`, então hoje o
teto real é 8 transmissões por sala (ou 1 transmissão e 3 espectadores de Linux, que
gastam duas portas cada) e 20 entradas por minuto por IP — o padrão do `config.ts`.
O `ecosystem.config.cjs` do repositório já diz 64 e 120; um `./deploy.sh vps` vale
mais do que qualquer linha das tabelas abaixo.

Cada arquivo versionado e onde ele entra:

| Arquivo do repositório | Vai para | O que muda |
|---|---|---|
| `infra/nginx.conf` | `/etc/nginx/nginx.conf` | `worker_rlimit_nofile` (novo), `worker_connections` 768 → 4096, `gzip_types`, `keepalive_timeout` |
| `infra/nginx-unkvoid.conf` | `/etc/nginx/sites-available/unkvoid` | o Reverb em `/app`, cache do `/build/`, `limit_rate` no download |
| `infra/sysctl-unkvoid.conf` | `/etc/sysctl.d/99-unkvoid.conf` | buffer UDP de **envio**, backlog, e as portas do SFU fora do sorteio do kernel |
| `sfu/ecosystem.config.cjs` | `/var/www/projects/sfu/` | o Reverb no pm2, com `watch` desligado |

Ao aplicar o sysctl, apague os dois arquivos que ele substitui:
`99-unkvoid-udp.conf` (virou este) e `99-livekit.conf` (sobrou de um teste de
LiveKit, e é quem prende `rmem_max` em 5 MB — um passo acima do `rmem_default`).

### O que não dá para versionar

**php-fpm não aceita drop-in**: um segundo arquivo em `pool.d/` com `[www]` faz o
serviço recusar com `cannot redeclare pool`. É edição no lugar, em
`/etc/php/8.4/fpm/pool.d/www.conf`:

| Diretiva | Nesta máquina (8 GB) | Na máquina nova (24 GB) |
|---|---|---|
| `pm.max_children` | 5, o padrão da distro | 32 |
| `pm.start_servers` | 2 | 5 |
| `pm.min_spare_servers` | 1 | 5 |
| `pm.max_spare_servers` | 3 | 10 |

5 é o padrão da distro, não uma escolha, e é o teto de **requisições PHP
simultâneas** — cada mensagem do chat pelo Livewire é uma. O processo mede 55-80 MB
de RSS, então 32 filhos são ~2,5 GB de pico contra 24 GB; `dynamic` fica porque
idle são 5 filhos, ~400 MB. Os valores da coluna da direita estão no roteiro, seção
6, que é onde eles se aplicam.

Em `/etc/php/8.4/fpm/php.ini`, para o painel conseguir publicar o instalador que
o nginx já aceita em 200 MB (`upload_max_filesize = 2M` e `post_max_size = 8M`
hoje matam o envio **depois** de subir o arquivo inteiro):

```ini
upload_max_filesize = 210M
post_max_size = 215M
max_execution_time = 120
```

**Swap não existe nesta máquina**, e ela compila Rust nela mesma pelo runner. Sem
swap, um pico de build faz o OOM killer escolher a vítima mais gorda — um worker
do mediasoup, no meio de uma transmissão. Vale igual com 24 GB: o que decide é a
forma do pico, não o tamanho da RAM.

```bash
sudo fallocate -l 4G /swapfile && sudo chmod 600 /swapfile
sudo mkswap /swapfile && sudo swapon /swapfile
echo '/swapfile none swap sw 0 0' | sudo tee -a /etc/fstab
```

**MySQL e MinIO não mudam.** O banco `unkvoid` tem 0,6 MB: o `innodb_buffer_pool_size`
de 128 MB já cabe o banco duzentas vezes, e `max_connections` de 151 é cinco vezes
o que 32 filhos do php-fpm mais o Reverb somam. O gatilho para mexer é o banco
passar de 200 MB — aí `innodb_buffer_pool_size=512M` no `command:` do
`infra/docker-compose.yml`.

### O que desligar

Medido com `ps`, `docker stats` e `ss`: tudo abaixo está ligado e não serve mais.

| O quê | Ganho | Por que sai |
|---|---|---|
| `filebrowser.service` | 25 MB e **a porta 8080** | Gerenciador de arquivos de uma pasta de Minecraft; o site que o expunha (`tarkas.unkvoid.com:8443`) só devolve 404. É ele que ocupa a porta que o Reverb precisa. |
| `pm2 delete reverb` | 61 MB | É o Reverb do projeto `discord`, na 8081, servindo um Laravel que já não existe. |
| `systemctl disable --now php8.3-fpm` | 50 MB | Nenhum site aponta para `php8.3-fpm.sock`; o `retro` usa o socket do 8.4. |
| `systemctl disable --now mysql` (o do sistema, na 3306) | 395 MB | Só tem `discord`, `discord_dev` e `retro_friends`. O Unkvoid usa o MySQL do docker, na 3307. Faça o dump antes. |
| `ENABLE_AMAVIS=0` no `mailserver` | 176 MB | O rspamd já filtra; o amavis é a segunda passada, e a caixa faz 0,1 mensagem por segundo. |
| `rm /etc/nginx/sites-enabled/{files,ia.unkvoid.com,retro}` | — | `files` só devolve 404, `ia.unkvoid.com` aponta para a 20128 onde não há ninguém, e o root do `retro` (`/var/www/projects/retro`) não existe. |
| `systemctl disable actions.runner.edsuuu-{retro-friends,tarkas}` | — | As duas unidades estão em `failed` desde sempre. |
| `docker stop teamspeak-server` | 28 MB e 2,7% de CPU | Só se ninguém mais usa o TeamSpeak — é decisão do dono, não da infra. |
| `/var/www/projects/discord` | 78 MB de disco | Projeto morto. |

São ~1,1 GB de RAM sem tocar em nada do Unkvoid, e a porta 8080 liberada para o
Reverb. O `target/` do Rust em `/var/www/projects/unkvoid/native` (9,4 GB) **fica**:
é o cache que faz o build do `.deb` não levar quarenta minutos.

### Quando esta máquina não bastar

O gatilho é medível, e é um só: **eth0 passando de 75% da porta de saída sustentados
por cinco minutos** — 150 Mb/s nesta máquina, que tem porta de 200; 450 Mb/s na nova,
que tem 600. Para medir sem instalar nada:

```bash
# duas leituras de 10 s, em Mb/s de saída
A=$(awk '/eth0/{print $10}' /proc/net/dev); sleep 10
B=$(awk '/eth0/{print $10}' /proc/net/dev); echo $(( (B-A)*8/10/1000000 ))
```

Os outros dois, que dizem *qual* peça acabou: qualquer worker do mediasoup acima
de 70% de um núcleo (`top -p "$(pgrep -d, mediasoup-worker)"`), e
`RcvbufErrors`/`SndbufErrors` crescendo em `grep ^Udp: /proc/net/snmp` depois do
sysctl novo.

Na ordem do mais barato:

1. **Limitar a qualidade por sala** — de graça. 720p60 são 5 Mb/s em vez de 10: a
   mesma banda atende o dobro de gente (34 em vez de 17 nesta máquina, 116 em vez de
   58 na nova). É o dobro de plateia sem pagar nada, e é a única mudança que age no
   numerador. É também a defesa contra a política de uso justo do tráfego ilimitado
   — ver o roteiro, seção 1.3.
2. **Segunda VPS só para o SFU** — a banda da Contabo é por máquina, então duas
   máquinas são o dobro de banda, enquanto um plano maior dá vCPU e RAM que não
   são o gargalo. Custa uma variável: o app pergunta o endereço do SFU em
   `GET /api/config`, então mudar para onde ele aponta não toca em uma linha de
   código.
3. **Subir o plano** — só quando o gatilho for RAM (`MemAvailable` abaixo de 800 MB)
   ou CPU dos workers, e não banda. Plano maior dá vCPU e RAM; porta maior dá
   plateia, e é outro campo do painel (roteiro, seção 1.1).

## 5. O que mais roda nesta máquina

Não faz parte do Unkvoid, mas uma VPS nova que substitua esta precisa saber que
existe:

| O quê | Como roda | Portas | Ainda serve? |
|---|---|---|---|
| MySQL do sistema | `mysql.service` (8.0) | 127.0.0.1:3306 | Não — só `discord`, `discord_dev`, `retro_friends` |
| Reverb do `discord` | pm2, `php`, em `/var/www/projects/discord/current` | 127.0.0.1:8081 | Não — o Laravel daquele caminho já não existe |
| filebrowser | `filebrowser.service` | 127.0.0.1:8080 | Não — e ocupa a porta do Reverb novo |
| php8.3-fpm | `php8.3-fpm.service` | socket | Não — nenhum site usa o socket dele |
| TeamSpeak 6 | docker, `teamspeaksystems/teamspeak6-server` | 9987/udp, 30033/tcp | Decisão do dono |
| Runner do GitHub | `actions.runner.edsuuu-unkvoid.unkvoid-vps` | — | Sim, é o CI |
| Outros sites | nginx: `files`, `ia.unkvoid.com`, `retro` | 443, 8443 | Não — os três devolvem 404 ou apontam para um root que não existe |

O ganho de desligar cada um está na tabela da seção 4. O Unkvoid em si é o nginx,
o php8.4-fpm, o `sfu` e o `reverb` no pm2, e os contêineres `unkvoid-mysql`,
`unkvoid-minio` e `unkvoid-mail`.

## 6. Quando alguma coisa não responde

Na ordem, porque cada uma explica a seguinte:

1. `curl -s https://unkvoid.com/health` — se não responder, é nginx ou
   pm2, e nada de mídia vai funcionar.
2. `pm2 list` e `pm2 logs sfu --lines 50`.
3. `sudo ss -lntup | grep -E '3000|4000[0-6]'` — o processo está escutando?
4. O tcpdump da seção 1 — o pacote chega na máquina? Se não, é o painel da
   Contabo.
5. `curl -s https://unkvoid.com/downloads/latest.json` — 404 aqui
   significa que ninguém se atualiza, mesmo com tudo o resto de pé.
