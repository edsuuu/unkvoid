# Instalar a VPS nova, do zero

A ordem de execução para levantar uma máquina limpa que hospeda **duas coisas e
mais nada**: o servidor de e-mail e o Unkvoid (site, SFU, chat, MinIO, runner).

Tudo o que existe na máquina de hoje e não está nessa lista **não migra** — a
tabela da seção 13 diz o que fica para trás e por quê.

Cada passo tem o comando copiável e uma linha dizendo por que ele existe. Quem
quer entender o desenho em vez de executar, leia o [SERVIDOR.md](SERVIDOR.md);
quem quer as portas e o motivo de cada faixa, o [UDP.md](UDP.md).

| | |
|---|---|
| Máquina | Contabo **Cloud VPS 8**: 8 vCPU, 24 GB de RAM, 300 GB de SSD, porta de 600 Mbit/s, tráfego ilimitado (uso justo), 3 snapshots, Ubuntu 24.04 |
| Região | **Estados Unidos (Leste)** — leia a seção 1.4 antes de confirmar: é a decisão de qualidade de voz mais importante do projeto |
| O que roda | nginx, php8.4-fpm, pm2 (`sfu` + `reverb`), docker (`unkvoid-mysql`, `unkvoid-minio`, `unkvoid-mail`), runner do GitHub |
| O que **não** roda | TeamSpeak, filebrowser, php8.3, MySQL do sistema, os sites `files`/`ia`/`retro`/`tarkas`, o projeto `discord` |

## A ordem, e por que ela é essa

1. **Contratar e conferir o painel** — o teto do produto é banda e latência, e nenhuma das duas se conserta com comando
2. **Firewall do painel** — é o firewall da Contabo que descarta, não o da máquina; sem ele nada do resto responde
3. **Sistema base, usuário, chave SSH, ufw**
4. **Swap e sysctl** — antes de qualquer coisa que consuma RAM ou UDP
5. **Docker: MySQL, MinIO, e-mail** — o banco e o bucket precisam existir antes do deploy
6. **Node, pnpm, pm2, runner**
7. **nginx e TLS**
8. **Deploy do site e do SFU**, com os dados que vêm da máquina velha
9. **As variáveis que não são versionáveis**
10. **Backup** — antes do corte de DNS, porque depois dele as caixas passam a receber de verdade
11. **A migração do e-mail** — por último de propósito: é o único passo que, errado, perde mensagem
12. **Checagem final, item por item**
13. **O que não vai para a máquina nova**

---

## 1. O que contratar, e o que conferir no painel ANTES de pagar

**Cloud VPS 8**: 8 vCPU, 24 GB de RAM, 300 GB de SSD. Sobra folga: hoje a máquina
usa 38 GB dos 145, e 9,4 GB desses 38 são o cache do Rust, que se refaz.

### 1.1 Os dois campos que decidem o teto do produto

**O que limita o Unkvoid não é CPU nem RAM: é banda de saída.** O SFU replica a
transmissão por espectador — 1080p60 a 10 Mb/s com 58 pessoas assistindo são
580 Mb/s saindo da placa de rede. 8 vCPU não mudam isso em nada.

No formulário do pedido, e depois em **Seus serviços → o VPS → Detalhes do
produto**, os dois campos a conferir são:

| Campo no painel | O que este plano traz | Por que importa |
|---|---|---|
| **Uplink / Port speed** (velocidade da porta) | **600 Mbit/s** | é o teto instantâneo: quantas pessoas assistem ao mesmo tempo |
| **Traffic / Tráfego** (limite mensal) | **ilimitado**, com política de uso justo | é o teto acumulado: quantas **horas** por mês a sala pode ficar cheia |

Confira os dois **no painel, depois de contratar**, e não só no anúncio: é o campo
"Uplink" da página do serviço que manda, e uma porta de 200 Mb/s entregue por
engano devolve o teto de espectadores à máquina velha com 8 vCPU parados.

### 1.2 Teto de espectadores simultâneos, por porta e por qualidade

As taxas são as do [REDE.md](REDE.md): 720p60 = 5 Mb/s, 1080p60 = 10 Mb/s,
1440p60 = 20 Mb/s. A banda útil é a porta **menos ~20 Mb/s**, que são o site, o
download do instalador, o `/apt/` e o e-mail dividindo o mesmo uplink.

| Porta | Banda útil | 720p60 | 1080p60 | 1440p60 |
|---|---|---|---|---|
| 200 Mb/s (a máquina velha) | 180 Mb/s | 36 | 18 | 9 |
| 500 Mb/s | 480 Mb/s | 96 | 48 | 24 |
| **600 Mb/s (este plano)** | **580 Mb/s** | **116** | **58** | **29** |
| 1 Gb/s | 980 Mb/s | 196 | 98 | 49 |

O número é o total da máquina, não por sala: quinze pessoas em duas salas pesam o
mesmo que quinze numa só. A linha de 200 Mb/s dá 18 onde o
[SERVIDOR.md](SERVIDOR.md) registra 17 — é a diferença entre descontar 20 Mb/s
fixos e descontar 15% da porta, e não vale discutir num número que é estimativa de
pico.

**O plano novo triplica o teto de espectadores: de ~17 para 58 em 1080p60.** Isso
vem da porta, não dos oito núcleos.

### 1.3 Tráfego ilimitado, e a política de uso justo

Ilimitado tira a franquia mensal da conta — os planos medidos da Contabo param em
32 TB, e este não tem esse número. A conta que sai de cena:

| Uso | Consumo |
|---|---|
| Porta cheia (580 Mb/s) | 261 GB por hora |
| Oito horas por dia, todo dia | ~61 TB por mês |
| Vinte e quatro horas por dia | ~184 TB por mês |

**Mas escreva isto onde não se esqueça:** um relé de mídia sustentando a porta
cheia é exatamente o perfil que uma política de uso justo observa. 61 TB por mês
são o dobro da franquia de 32 TB dos planos medidos, e 184 TB são seis vezes. Um
cliente que faz isso mês após mês deixa de parecer "uso justo" e passa a parecer
um serviço de streaming hospedado num plano de VPS. O que isso significa na
prática:

- **Ilimitado não é promessa contratual de porta cheia 24/7.** É ausência de
  medidor, com uma cláusula que permite a Contabo conversar.
- **A defesa é o numerador, não o denominador.** Limitar a qualidade por sala é de
  graça: 720p60 gasta metade do que 1080p60 e atende o dobro de gente. É a única
  mudança que reduz o tráfego sem reduzir a plateia.
- **O gatilho de acompanhar é o mesmo do [SERVIDOR.md](SERVIDOR.md)**, com o
  número novo: eth0 acima de **450 Mb/s** de saída sustentados por cinco minutos
  (75% dos 600). Se isso virar rotina, a resposta é uma segunda VPS só para o SFU
  — a banda da Contabo é por máquina, então duas máquinas são o dobro de banda, e
  mudar para onde o app aponta é uma variável (`SFU_PUBLIC_URL`), não uma linha de
  código.

### 1.4 A região: Estados Unidos (Leste), e o que ela custa

**A Contabo não tem região na América do Sul.** As opções relevantes, medidas do
Brasil:

| Região | Ida e volta do Brasil |
|---|---|
| Estados Unidos (Leste) | **116 ms** |
| Europa (Alemanha) | 195 ms |

O SFU é **relé**: nada vai direto de uma pessoa para a outra. Duas pessoas no
Brasil conversando por este servidor mandam o áudio para os Estados Unidos e o
recebem de volta, ou seja, **pagam os 116 ms inteiros** de ida e volta — não
metade. Fica em ~116 ms de rede pura antes de somar o Opus, o buffer de jitter e o
tempo de placa de som, e o limite em que uma conversa começa a atropelar é por
volta de 150 ms.

**Isso é maior do que qualquer ganho de código no orçamento de latência deste
projeto.** Tudo o que está sob nosso controle mede dezenas de milissegundos: o
quadro-chave a cada um segundo, os 400 ms de folga de reprodução (que são de
vídeo, não de voz), o pedido de quadro-chave que troca um segundo congelado por
uma ida e volta. Nenhuma dessas coisas move os 116 ms.

**A escolha da região é a decisão de qualidade de voz mais importante do
projeto**, e é tomada uma vez, no formulário do pedido. Se o público é brasileiro,
os Estados Unidos (Leste) são a melhor opção que a Contabo oferece — a Europa
custaria 79 ms a mais, o que é sair do "dá para conversar" e entrar no
"atropela". Se a latência da voz virar a reclamação principal, a saída não é
otimizar código: é um SFU hospedado em São Paulo, em outro provedor, e o app
aponta para ele em `GET /api/config` sem recompilar nada.

Vale também para o site: cada requisição do Livewire paga os mesmos 116 ms, e o
chat é feito de requisições curtas. É outro motivo para o `http2` do nginx e para
o cache `immutable` do `/build/`.

### 1.5 Os 3 snapshots incluídos, e o que eles não são

Snapshot é imagem da máquina inteira, tirada à mão, guardada no mesmo provedor.
**Não é backup** — some com a conta, e não recupera "a mensagem de ontem". Serve
bem para uma coisa: tirar um antes do corte de DNS da seção 11, para que um erro
custe um restore e não uma reinstalação.

O backup de verdade é a seção 10, e ela existe porque as caixas de e-mail são
pessoais e insubstituíveis.

---

## 2. Firewall do painel

**O firewall da Contabo é do painel, não da máquina.** `ufw status` diz `inactive`,
o `iptables` está limpo, e mesmo assim o tráfego é descartado antes de chegar.
Procurar no servidor não acha nada — foi o que custou uma noite inteira.

Em **Serviços de Rede → Firewall**. O campo de portas aceita intervalo
(`41000-42000`) e lista por vírgula: não é uma regra por porta.

| Protocolo | Portas | Para quê |
|---|---|---|
| TCP | 22 | ssh |
| TCP | 80, 443 | nginx: site, API, `/sfu`, `/app`, `/apt/`, `s3.unkvoid.com` |
| TCP **e** UDP | 40000-40006 | WebRTC de quem assiste — uma porta por worker, e são 7 workers |
| UDP | 41000-42000 | RTP puro de quem transmite pelo app, e de quem assiste no Linux |
| TCP | 25 | receber e-mail |
| TCP | 465, 587 | enviar e-mail autenticado |
| TCP | 993 | IMAP, para ler a caixa |

**40000-40006 e não 40000-40003**: com `SFU_WORKERS=7` o WebRtcServer ocupa uma
porta por worker a partir de `SFU_MEDIA_PORT` (40000), ou seja 40000 a 40006. TCP
na mesma faixa é o caminho reserva de quem está numa rede que bloqueia UDP —
fechado, o ICE tenta, não conecta, e a pessoa olha para uma sala sem imagem.

**41000-42000 e não 41000-41447**: a conta do [UDP.md](UDP.md) é
`SFU_PLAIN_PORT + (SFU_WORKERS × SFU_PLAIN_PORTS) - 1`, que com 7 workers e 64
portas dá `41000 + 448 - 1 = 41447` — 448 portas, dentro da regra 41000-42000 que
já existe. A faixa do firewall é mais larga porque o mediasoup **sorteia** dentro
da faixa do worker e só confere se a porta está livre *nesta máquina*, nunca se o
firewall a deixa passar. Uma porta livre e bloqueada é aceita sem reclamar: o
transporte sobe, `sent` cresce, `sendErrors` fica em zero, e do outro lado é tela
preta. A folga até 42000 deixa subir `SFU_PLAIN_PORTS` depois sem voltar no painel.

Não entram: 9987/udp e 30033/tcp (TeamSpeak, que não migra) e 8443 (site morto).

Conferir **de fora**, porque de dentro o `ss` mostra o processo escutando mesmo
com tudo sendo descartado. A regra do painel leva alguns minutos para valer:
testar cedo dá falso negativo.

```bash
# na VPS nova, deixe rodando
sudo tcpdump -nn -i any 'udp and (dst portrange 40000-40006 or dst portrange 41000-42000)'

# da sua máquina — toda porta abaixo tem de aparecer no tcpdump
for p in 40000 40006 41000 41447 42000; do printf teste | nc -u -w0 NOVO.IP.AQUI $p; done

# TCP responde a sondagem, então dá para testar direto
for p in 22 25 80 443 587 993 40000 40006; do nc -z -w 4 NOVO.IP.AQUI $p && echo "$p aberta" || echo "$p FECHADA"; done
```

---

## 3. Sistema base, usuário, chave SSH e ufw

A Contabo entrega a máquina com `root` e senha. O primeiro login é o único por
senha.

```bash
# Nome da máquina: o e-mail se apresenta com ele no HELO, e o Gmail compara com o PTR.
sudo hostnamectl set-hostname mail.unkvoid.com
echo '127.0.1.1 mail.unkvoid.com mail' | sudo tee -a /etc/hosts

sudo apt update && sudo apt upgrade -y
# Fuso de São Paulo desde já, mesmo com a máquina nos Estados Unidos: o nome de cada
# release do site é um carimbo da hora local, e trocar o fuso depois faz o relógio andar
# para trás e a limpeza apagar justamente a release que está no ar.
sudo timedatectl set-timezone America/Sao_Paulo
```

O usuário de trabalho é `ubuntu` porque é o dono de `/var/www` em todo script do
repositório (`deploy-web.sh`, `install.sh`, `apt-publish.sh`):

```bash
sudo adduser --disabled-password --gecos '' ubuntu
sudo usermod -aG sudo ubuntu
echo 'ubuntu ALL=(ALL) NOPASSWD:ALL' | sudo tee /etc/sudoers.d/ubuntu
```

`NOPASSWD` não é preguiça: o `deploy-web.sh` chama `sudo chgrp`, `sudo chmod` e
`sudo systemctl reload php8.4-fpm` rodando pelo runner, sem terminal para digitar
senha. (O grupo `docker` entra na seção 5, quando ele passar a existir.)

A chave pública, **do seu notebook** (é o que permite fechar a senha depois):

```bash
# no notebook
ssh-copy-id -i ~/.ssh/id_ed25519.pub ubuntu@NOVO.IP.AQUI
```

Fechada a chave, feche a senha:

```bash
sudo sed -i 's/^#\?PasswordAuthentication.*/PasswordAuthentication no/' /etc/ssh/sshd_config
sudo sed -i 's/^#\?PermitRootLogin.*/PermitRootLogin no/' /etc/ssh/sshd_config
sudo rm -f /etc/ssh/sshd_config.d/50-cloud-init.conf   # a Contabo religa a senha por aqui
sudo systemctl restart ssh
```

O alias no `~/.ssh/config` do notebook, que é o que `make build-vps` e
`sfu/deploy.sh` esperam:

```
Host vps
    HostName NOVO.IP.AQUI
    User ubuntu
    IdentityFile ~/.ssh/id_ed25519
```

O `ufw` é a segunda camada — o que descarta de verdade é o painel, mas o `ufw`
protege do dia em que uma regra do painel for aberta demais:

```bash
sudo ufw default deny incoming && sudo ufw default allow outgoing
sudo ufw allow 22/tcp
sudo ufw allow 80,443/tcp
sudo ufw allow 25,465,587,993/tcp
sudo ufw allow 40000:40006/tcp
sudo ufw allow 40000:40006/udp
sudo ufw allow 41000:42000/udp
sudo ufw --force enable
sudo ufw status verbose   # confira o 22 na lista ANTES de sair do ssh
```

---

## 4. Swap e sysctl

**4 GB de swap, mesmo com 24 GB de RAM**, porque esta máquina compila Rust nela
mesma pelo runner. Sem swap, um pico de build faz o OOM killer escolher a vítima
mais gorda — um worker do mediasoup, no meio de uma transmissão. O motivo é a
forma do pico, não o tamanho da RAM: `cargo` com oito jobs paralelos e um `rustc`
gordo no fim não avisa antes de subir.

```bash
sudo fallocate -l 4G /swapfile && sudo chmod 600 /swapfile
sudo mkswap /swapfile && sudo swapon /swapfile
echo '/swapfile none swap sw 0 0' | sudo tee -a /etc/fstab
swapon --show
```

`vm.swappiness=10` vem no sysctl abaixo: é "só use se faltar de verdade", não
"use sempre um pouco".

O sysctl inteiro entra **desde o primeiro dia**, e não depois do primeiro
problema. Cada número dele saiu de medição na máquina velha, e dois deles são
defeito ainda aberto lá: 19 267 `RcvbufErrors` e 30 094 `SndbufErrors`
acumulados.

```bash
sudo cp /var/www/projects/unkvoid/infra/sysctl-unkvoid.conf /etc/sysctl.d/99-unkvoid.conf
sudo sysctl --system
```

Não há `99-unkvoid-udp.conf` nem `99-livekit.conf` para apagar aqui: aqueles dois
eram da máquina velha, e a nova nasce só com este arquivo.

Confira os quatro que mais importam:

```bash
sysctl net.core.wmem_default net.core.rmem_default net.ipv4.ip_local_reserved_ports vm.swappiness
```

`ip_local_reserved_ports = 40000-40006,41000-42000` é o que impede o kernel de
sortear uma porta do SFU como porta de origem efêmera de qualquer conexão de
saída (o curl do deploy, o runner, o webhook). Quando isso acontece, o mediasoup
sorteia, encontra ocupada, e devolve `no more available ports` num horário
aleatório.

---

## 5. Docker, com MySQL, MinIO e e-mail

```bash
curl -fsSL https://get.docker.com | sudo sh
sudo usermod -aG docker ubuntu && newgrp docker
```

O compose e as senhas ficam em `/opt/unkvoid/`, fora de `/var/www` — o deploy
apaga release antiga, e senha de banco não mora em pasta que o deploy varre:

```bash
sudo mkdir -p /opt/unkvoid && sudo chown ubuntu:ubuntu /opt/unkvoid
cp /var/www/projects/unkvoid/infra/docker-compose.yml /opt/unkvoid/
cp /var/www/projects/unkvoid/infra/docker.env.example /opt/unkvoid/.env
mkdir -p /opt/unkvoid/mail
chmod 600 /opt/unkvoid/.env
```

Preencha as quatro senhas do `.env` (`openssl rand -hex 24` em cada), **e se for
migrar o banco e o bucket da máquina velha, use as senhas de lá** — o `.env` do
site guarda a mesma.

```bash
cd /opt/unkvoid && docker compose up -d
docker ps --format '{{.Names}}\t{{.Ports}}'
```

O que cada um é, e por que a porta é essa:

| Contêiner | Escuta | Por que |
|---|---|---|
| `unkvoid-mysql` | `127.0.0.1:3307` | 3307 e não 3306 por herança da máquina velha, onde o MySQL do sistema ocupava a 3306. Aqui não há MySQL do sistema, mas o `.env` do site e os dumps já dizem 3307 — trocar agora só criaria um jeito novo de errar |
| `unkvoid-minio` | `127.0.0.1:9000-9001` | quem fala com o mundo é o nginx em `s3.unkvoid.com`, que precisa repassar o `Host` intacto: a assinatura da URL foi calculada com ele |
| `unkvoid-mail` | `0.0.0.0` nas 25/465/587/993 | é o único que recebe da internet direto, porque SMTP não passa por proxy reverso |

`innodb_buffer_pool_size` **fica em 128M** e não há `command:` para mudar: o banco
`unkvoid` tem 0,6 MB, e 128 MB já o cabem duzentas vezes. Ter 24 GB de RAM não é
motivo para reservar mais — buffer pool maior que o banco é RAM parada. O gatilho
para `512M` é o banco passar de 200 MB — aí sim, no `command:` do
`infra/docker-compose.yml`. `max_connections` de 151 também fica: é quatro vezes o
que 32 filhos do php-fpm mais o Reverb somam.

O MinIO precisa dos dois buckets, e o do APT precisa ser público para leitura —
é o `apt` de cada máquina instalada que lê de lá, sem credencial:

```bash
curl -fsSL https://dl.min.io/client/mc/release/linux-amd64/mc -o /tmp/mc
sudo install -m 755 /tmp/mc /usr/local/bin/mc
mc alias set local http://127.0.0.1:9000 "$MINIO_ROOT_USER" "$MINIO_ROOT_PASSWORD"
mc mb --ignore-existing local/unkvoid local/apt
mc anonymous set download local/apt
```

O bucket `unkvoid` **não** é público: os instaladores saem por URL assinada que
vence, montada pelo Laravel.

---

## 6. Node, pnpm, pm2 e o runner

```bash
sudo apt install -y build-essential curl wget file pkg-config git python3 rsync \
  gnupg dpkg-dev apt-utils nginx certbot python3-certbot-nginx swaks \
  php8.4-fpm php8.4-{cli,mbstring,xml,curl,zip,mysql,bcmath,intl,gd}
```

**Não instale `libwebkit2gtk-4.1-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`,
`libxdo-dev`, `cmake` nem o Rust.** Eram da época em que o `.deb` compilava no
host; hoje o `build-vps.sh` compila dentro de um Debian 12 (`Dockerfile.linux`),
porque um binário fica preso à glibc de quem o gerou. No host o build só precisa
de `node` e `python3`, para as conferências. O `swaks` é para os testes de e-mail
da seção 11.

```bash
curl -fsSL https://deb.nodesource.com/setup_24.x | sudo -E bash -
sudo apt install -y nodejs
sudo npm install -g pnpm pm2
```

Versões que estão em produção hoje, para reproduzir e não descobrir surpresa:

| Ferramenta | Versão |
|---|---|
| Node | 24.19.0 |
| npm | 11.17.0 |
| pnpm | 11.21.0 |
| pm2 | 7.0.4 |
| PHP | 8.4 (só o 8.4 — o 8.3 não migra) |

As pastas, com dono `ubuntu` para publicar sem `sudo`:

```bash
sudo mkdir -p /var/www/projects/unkvoid-web/{releases,shared} /var/www/projects/sfu /var/www/projects/unkvoid
sudo chown -R ubuntu:ubuntu /var/www/projects
```

php-fpm, em `/etc/php/8.4/fpm/pool.d/www.conf`. **Isto é edição no lugar, não
drop-in**: um segundo arquivo em `pool.d/` com `[www]` faz o serviço recusar com
`cannot redeclare pool`.

| Diretiva | Padrão da distro | Nesta máquina |
|---|---|---|
| `pm` | `dynamic` | `dynamic` (fica: idle são 5 filhos, ~400 MB) |
| `pm.max_children` | 5 | **32** |
| `pm.start_servers` | 2 | **5** |
| `pm.min_spare_servers` | 1 | **5** |
| `pm.max_spare_servers` | 3 | **10** |

32 filhos × ~80 MB medidos = ~2,5 GB de pico contra 24 GB. 5 é o padrão da
distro, não uma escolha, e é o teto de **requisições PHP simultâneas** — cada
mensagem do chat pelo Livewire é uma, e cada uma paga os 116 ms de ida e volta da
seção 1.4, o que faz o slot ficar ocupado mais tempo do que ficaria numa máquina
perto de quem usa.

E em `/etc/php/8.4/fpm/php.ini`, senão o envio do instalador de 200 MB pelo `POST /api/releases`
morre **depois** de subir o arquivo inteiro:

```ini
upload_max_filesize = 210M
post_max_size = 215M
max_execution_time = 120
```

```bash
sudo systemctl restart php8.4-fpm
```

O runner do GitHub Actions é quem faz o deploy do site, do SFU e o build do
Linux. Antes de registrá-lo, o repositório precisa estar no disco — o build do
Linux usa `/var/www/projects/unkvoid/native` como cache do cargo, e o
`runner-install.sh` está lá dentro:

```bash
# no notebook, porque o repositório é privado e clonar de lá exigiria uma credencial
# do GitHub guardada numa máquina exposta
rsync -a --exclude target --exclude target-deb12 --exclude node_modules \
  /var/www/projects/unkvoid/ vps:/var/www/projects/unkvoid/

# o token de registro vale uma hora
gh api -X POST repos/edsuuu/unkvoid/actions/runners/registration-token --jq .token

# na VPS, com o token
/var/www/projects/unkvoid/infra/runner-install.sh O_TOKEN_QUE_O_GH_IMPRIMIU
```

No painel do GitHub, apague o runner velho (`unkvoid-vps` da máquina antiga)
**depois** que o novo aparecer online: dois runners com a mesma label fazem o job
cair num deles ao acaso.

---

## 7. nginx e TLS pelo certbot

```bash
sudo cp /etc/nginx/nginx.conf /etc/nginx/nginx.conf.bak
sudo cp /var/www/projects/unkvoid/infra/nginx.conf /etc/nginx/nginx.conf
sudo cp /var/www/projects/unkvoid/infra/nginx-unkvoid.conf /etc/nginx/sites-available/unkvoid
sudo ln -sf /etc/nginx/sites-available/unkvoid /etc/nginx/sites-enabled/unkvoid
sudo rm -f /etc/nginx/sites-enabled/default
sudo nginx -t && sudo systemctl reload nginx
```

O `nginx.conf` do repositório é a config da distro com quatro mudanças, cada uma
marcada com `MUDOU` e o motivo. A que mais importa é `worker_rlimit_nofile 32768`:
sem ela o worker fica com o limite **soft** de 1024 descritores que herda do
systemd, e subir `worker_connections` é mentira — o nginx aceita e falha no
`accept`.

O certificado. **Isto só funciona depois que o DNS apontar para cá**, então há dois
momentos: antes do corte, copie o certificado da máquina velha; depois do corte,
emita o próprio.

```bash
# ANTES do corte de DNS: o certificado da máquina velha, que já é válido para estes nomes
# no notebook
rsync -a --rsync-path='sudo rsync' vps-velha:/etc/letsencrypt/ /tmp/letsencrypt/
rsync -a --rsync-path='sudo rsync' /tmp/letsencrypt/ vps:/tmp/letsencrypt/
# na VPS nova
sudo rsync -a /tmp/letsencrypt/ /etc/letsencrypt/ && rm -rf /tmp/letsencrypt
sudo nginx -t && sudo systemctl reload nginx
```

Copiar em vez de emitir é o que deixa o nginx e o contêiner de e-mail subirem com
TLS válido **antes** de mexer em uma linha de DNS — e é justamente o que permite
provar o e-mail novo com a caixa velha ainda de pé.

```bash
# DEPOIS do corte de DNS, com os A records já no IP novo
sudo certbot --nginx -d unkvoid.com -d www.unkvoid.com -d s3.unkvoid.com -d mail.unkvoid.com
```

Duas coisas para lembrar depois do certbot:

1. Acrescente `http2` na linha que ele escreveu — este nginx é 1.24, onde
   `http2 on;` ainda não existe: `listen 443 ssl http2;`. Vale pela rajada de
   assets do Livewire: uma conexão multiplexada em vez de seis, e com 116 ms de
   ida e volta cada conexão economizada aparece na tela.
2. Não mexa nas linhas marcadas `# managed by Certbot`.

O bloco `default_server` que devolve 404 para qualquer `Host` desconhecido não é
enfeite: sem ele o primeiro `server` da lista atende qualquer nome que aponte para
este IP.

```bash
sudo tee /etc/nginx/sites-available/00-default-deny > /dev/null <<'EOF'
server {
    listen 80 default_server;
    listen [::]:80 default_server;
    server_name _;
    return 404;
}

server {
    listen 443 ssl default_server;
    listen [::]:443 ssl default_server;
    server_name _;

    ssl_certificate /etc/letsencrypt/live/unkvoid.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/unkvoid.com/privkey.pem;

    return 404;
}
EOF
sudo ln -sf /etc/nginx/sites-available/00-default-deny /etc/nginx/sites-enabled/
sudo nginx -t && sudo systemctl reload nginx
```

**A mídia não passa pelo nginx.** Ela vai direto por UDP em 40000-40006 e
41000-41447. O nginx só carrega sinalização (`/sfu`, `/app`) e arquivo.

---

## 8. O deploy do site e do SFU

### 8.1 Os dados que vêm da máquina velha

Três coisas, e só três. Nenhuma delas está no repositório.

```bash
# 1. O banco. 0,6 MB, então é um dump e pronto.
ssh vps-velha 'docker exec unkvoid-mysql mysqldump -uroot -p"$MYSQL_ROOT_PASSWORD" \
  --single-transaction --routines unkvoid' > /tmp/unkvoid.sql
ssh vps 'docker exec -i unkvoid-mysql mysql -uroot -p"$MYSQL_ROOT_PASSWORD" unkvoid' < /tmp/unkvoid.sql

# 2. Os buckets do MinIO: o `apt` (o repositório de pacotes) e o `unkvoid` (os
#    instaladores que o painel publicou). ~281 MB no total.
#    Na velha: mc mirror local/apt /tmp/apt && mc mirror local/unkvoid /tmp/unkvoid
#    rsync para cá, e depois:
mc mirror /tmp/apt local/apt
mc mirror /tmp/unkvoid local/unkvoid

# 3. O chaveiro GPG que assina o índice do APT. Se esta chave se perder, TODA máquina
#    Linux instalada precisa importar uma chave nova à mão — é o item mais caro da lista.
ssh vps-velha 'gpg --export-secret-keys --armor repo@unkvoid.com' > /tmp/apt-secreta.asc
ssh vps 'gpg --import' < /tmp/apt-secreta.asc
rm -f /tmp/apt-secreta.asc
ssh vps 'gpg --list-secret-keys repo@unkvoid.com'
```

A chave do APT **não é** a chave que assina as atualizações do app. Confundir as
duas é o erro mais caro do processo — ver [AUTO-UPDATE.md](AUTO-UPDATE.md). A do
auto-update nunca vem para cá: esta máquina é exposta à internet, e quem a tiver
publica atualização para todo mundo que instalou o app.

### 8.2 O site

A estrutura que o `deploy-web.sh` exige, senão ele para com `[ERRO] falta a
estrutura`:

```bash
mkdir -p /var/www/projects/unkvoid-web/{releases,shared/storage}
# o .env compartilhado: a seção 9 diz quais chaves ele precisa
install -m 600 /dev/null /var/www/projects/unkvoid-web/shared/.env
```

O mais simples é trazer a árvore de `storage/` da máquina velha
(`framework/`, `logs/`, `app/public/`), que já tem as pastas e os arquivos que o
painel enviou:

```bash
rsync -a vps-velha:/var/www/projects/unkvoid-web/shared/storage/ \
  /var/www/projects/unkvoid-web/shared/storage/
```

Depois é o deploy normal, pelo runner — um `workflow_dispatch` em `deploy-web`, ou
à mão na VPS:

```bash
/var/www/projects/unkvoid/infra/deploy-web.sh
```

Ele copia para uma release nova, instala, compila os assets, migra, aponta o
`current` e recarrega o php-fpm. Termina batendo em `/up` pelo próprio nginx, então
um deploy que imprime `site respondeu 200` é um deploy que subiu de verdade.

### 8.3 O SFU e o Reverb

```bash
# no notebook
cd /var/www/projects/unkvoid/sfu && ./deploy.sh vps
```

Antes do primeiro deploy, o `.env` do SFU já precisa existir na VPS (seção 9) — o
SFU **não sobe** sem `SFU_SECRET`, e é melhor fora do ar que aberto.

São **7 workers** e não 8: o oitavo núcleo fica inteiro para o nginx, o php-fpm, o
MySQL, o MinIO e o e-mail. Um worker do mediasoup é um processo de uma thread só
que satura um núcleo e para; a sala é fixada num worker e a voz é presa a um
núcleo, então 7 workers são 7 canais de voz pesados em paralelo. Deixar 8 faria a
oitava sala disputar núcleo com o que responde o site.

O Reverb é um processo separado no mesmo pm2, e sobe uma vez só, à mão:

```bash
cd /var/www/projects/sfu && pm2 startOrRestart ecosystem.config.cjs --only reverb
```

**Um** processo de Reverb, e não um por núcleo: dois só compartilham quem está
escutando o quê através do Redis (`REVERB_SCALING_ENABLED`), que esta máquina não
tem. Sem ele, a mensagem publicada no processo A não chega a ninguém conectado no
B — metade do chat desaparece em silêncio. E `watch: false`, porque o padrão do
pm2 é vigiar o diretório: o Laravel escreve em `storage/logs` a cada erro, e cada
escrita viraria um restart que derruba todo WebSocket aberto.

Para o pm2 voltar sozinho depois de um reboot — sem isto, um reboot leva a mídia e
o chat e nada avisa:

```bash
pm2 startup    # e rode a linha que ele imprimir
pm2 save
```

---

## 9. As variáveis que não são versionáveis

Os nomes, nunca os valores. Os valores saem de `~/auxilos/` na máquina do dono
(`env-google`, `release-secret.env`, `email-unkvoid.txt`) e do `.env` da VPS
velha.

### `/opt/unkvoid/.env` — as senhas do docker

| Nome | De onde vem |
|---|---|
| `MYSQL_ROOT_PASSWORD` | a mesma da máquina velha, senão o dump não restaura |
| `MYSQL_PASSWORD` | a mesma que está no `DB_PASSWORD` do site |
| `MINIO_ROOT_USER` | `unkvoid` |
| `MINIO_ROOT_PASSWORD` | a mesma do `AWS_SECRET_ACCESS_KEY` do site |

### `/var/www/projects/sfu/.env` — o SFU

| Nome | Observação |
|---|---|
| `SFU_SECRET` | idêntico ao do site: um assina o token de entrada, o outro confere. Mínimo de 32 caracteres |
| `SFU_ANNOUNCED_ADDRESS` | **o IP novo.** É o endereço que o servidor anuncia para a mídia; errar isto é o clássico "entra na sala e não vê nada" |
| `SFU_APP_VERSION` | a versão mínima do app que o SFU aceita |

### `/var/www/projects/unkvoid-web/shared/.env` — o site

| Nome | Observação |
|---|---|
| `APP_KEY` | **o mesmo da máquina velha.** Trocar invalida toda sessão e todo dado cifrado no banco |
| `DB_PASSWORD` | o `MYSQL_PASSWORD` do docker |
| `SFU_SECRET` | o mesmo do SFU |
| `RELEASE_SECRET` | com ele o CI publica instalador pela API; está também nos secrets do GitHub |
| `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET` | login com Google |
| `UNKVOID_ADMIN_EMAIL` | quem entra com este e-mail vira administrador na primeira vez |
| `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` | as credenciais do MinIO |
| `MAIL_USERNAME`, `MAIL_PASSWORD` | a caixa `no-reply@unkvoid.com` |
| `REVERB_APP_ID`, `REVERB_APP_KEY`, `REVERB_APP_SECRET` | **ausentes no `.env` da máquina velha.** Sem os três o chat não sobe; gere com `openssl rand -hex 16` |
| `SFU_PUBLIC_URL` | **ausente no `.env` da máquina velha.** É o endereço que o app recebe em `GET /api/config`; na VPS é `wss://unkvoid.com/sfu` |

E os valores de produção que não são segredo mas são por máquina:
`APP_ENV=production`, `APP_DEBUG=false`, `APP_URL=https://unkvoid.com`,
`DB_PORT=3307`, `SESSION_SECURE_COOKIE=true`, `DEBUGBAR_ENABLED=false`,
`AWS_ENDPOINT=http://127.0.0.1:9000`, `REVERB_SERVER_HOST=127.0.0.1`,
`REVERB_HOST=unkvoid.com`, `REVERB_PORT=443`, `REVERB_SCHEME=https`,
`UNKVOID_APT_URL=https://unkvoid.com/apt`.

**Nos secrets do GitHub não muda nada.** O deploy roda no runner que mora na
própria VPS, então não há chave de ssh nem IP em segredo nenhum: só
`TAURI_SIGNING_PRIVATE_KEY`, sua senha, e `RELEASE_SECRET`.

---

## 10. Backup: escolha um dos dois, antes do corte de DNS

O site se refaz deste repositório em uma hora. O banco tem 0,6 MB e os
instaladores voltam de um build. **As caixas de e-mail não voltam de lugar
nenhum**: são pessoais, são conversa de anos, e existem em um lugar só. Snapshot
não resolve (seção 1.5).

### Caminho A — o backup automático pago da Contabo

Add-on no painel, por VPS. Imagem completa da máquina, automática, guardada fora
dela, com restauração por clique.

- **Custa dinheiro por mês.** É o único custo.
- **Não falha em silêncio** — é o argumento que decide: um cron escrito à mão
  falha no dia em que o disco lota ou a chave expira, e ninguém descobre até
  precisar.
- Restaura a máquina inteira, não "a mensagem de ontem": para recuperar uma caixa
  é preciso restaurar e ir buscar dentro.

### Caminho B — dump noturno para fora da máquina

Sem custo, mas é código a manter. Uma unidade `systemd` na VPS que escreve num
diretório, e o notebook **puxando** dali — puxar é melhor do que empurrar, porque
uma VPS invadida não alcança o backup, e porque a falha aparece na máquina de
quem olha:

```bash
# na VPS: /usr/local/bin/unkvoid-dump.sh
sudo tee /usr/local/bin/unkvoid-dump.sh > /dev/null <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

DESTINO=/var/backups/unkvoid
HOJE=$(date +%F)
mkdir -p "$DESTINO"

# shellcheck source=/dev/null
set -a && . /opt/unkvoid/.env && set +a

docker exec unkvoid-mysql mysqldump -uroot -p"$MYSQL_ROOT_PASSWORD" \
    --single-transaction --routines unkvoid | gzip -9 > "$DESTINO/banco-$HOJE.sql.gz"

# As caixas com o Dovecot parado: ele reescreve o índice por baixo do tar, e um índice
# meio escrito é uma caixa que o cliente de e-mail não abre. São 288 KB, então a parada
# dura segundos.
cd /opt/unkvoid && docker compose stop mailserver
tar -C /var/lib/docker/volumes/unkvoid_mail-data/_data -czf "$DESTINO/caixas-$HOJE.tgz" .
tar -C /opt/unkvoid/mail -czf "$DESTINO/mail-config-$HOJE.tgz" .
cd /opt/unkvoid && docker compose start mailserver

# Catorze dias no disco local; o que importa é a cópia que o notebook puxa.
find "$DESTINO" -type f -mtime +14 -delete
EOF
sudo chmod 755 /usr/local/bin/unkvoid-dump.sh

sudo tee /etc/systemd/system/unkvoid-dump.service > /dev/null <<'EOF'
[Unit]
Description=Dump do banco e das caixas de e-mail do Unkvoid

[Service]
Type=oneshot
ExecStart=/usr/local/bin/unkvoid-dump.sh
EOF

sudo tee /etc/systemd/system/unkvoid-dump.timer > /dev/null <<'EOF'
[Unit]
Description=Dump noturno do Unkvoid

[Timer]
OnCalendar=*-*-* 04:00:00
Persistent=true

[Install]
WantedBy=timers.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now unkvoid-dump.timer
sudo systemctl start unkvoid-dump.service   # rode uma vez à mão e confira o resultado
ls -la /var/backups/unkvoid/
```

E no notebook, puxando todo dia (um `cron` local, ou à mão uma vez por semana):

```bash
rsync -a --rsync-path='sudo rsync' vps:/var/backups/unkvoid/ ~/auxilos/backup-unkvoid/
```

O `mail-config-*.tgz` leva o `postfix-accounts.cf` e a chave DKIM: sem ele, um
restore das caixas devolve as mensagens e nenhuma senha.

### O que fazer

**Recomendado: o caminho A, o backup automático pago.** As caixas são
insubstituíveis, e o que protege dado insubstituível é o mecanismo que não
depende de alguém lembrar de conferir. Se a resposta for não pagar, então o
caminho B **deixa de ser opcional** — e nesse caso o passo que não pode faltar é o
`rsync` do notebook, porque backup que mora na mesma máquina que o dado não é
backup.

Em qualquer um dos dois: **teste o restore uma vez**, antes de precisar. Um dump
que ninguém restaurou é uma hipótese.

---

## 11. A migração do e-mail

**Este é o único passo que, na ordem errada, perde mensagem.** Trocar de máquina
troca de IP, e IP novo começa sem reputação nenhuma: o PTR tem de ser pedido no
painel, o DKIM tem de viajar, e o Gmail vai desconfiar nos primeiros dias.

O que o DNS tem hoje, e é o que torna a ordem abaixo possível:

| Registro | Valor hoje |
|---|---|
| `MX unkvoid.com` | `10 mail.unkvoid.com.` |
| `A mail.unkvoid.com` | o IP velho |
| `TXT unkvoid.com` | `v=spf1 mx -all` |
| `TXT mail._domainkey` | `v=DKIM1; k=rsa; p=…` (selector `mail`, RSA 2048) |
| `TXT _dmarc` | `v=DMARC1; p=quarantine; rua=mailto:contato@unkvoid.com` |
| PTR do IP | `mail.unkvoid.com.` |

Duas consequências que decidem tudo:

- **O MX não precisa ser editado.** Ele aponta para um *nome*, não para um IP.
  O corte de verdade é o `A mail.unkvoid.com`.
- **O SPF também não.** `v=spf1 mx -all` autoriza quem estiver no MX, então ele
  segue o `A` automaticamente. A única edição de SPF em todo o processo é
  temporária: acrescentar o IP novo para poder testar antes do corte, e tirar
  depois.

### A ordem

**1. Antes de tudo, baixe o TTL.** Um TTL de uma hora significa uma hora de
remetentes entregando no IP velho depois do corte.

```
TTL de A mail.unkvoid.com, A unkvoid.com, A www, A s3 → 300 s
```
Espere **um TTL antigo inteiro** (uma hora, se era 3600) antes de seguir. É o
passo que ninguém tem paciência de fazer e é o que encurta a janela de risco de
horas para minutos.

**2. Peça o PTR do IP novo, no painel da Contabo.** Em **Seus serviços → o VPS →
Reverse DNS**, aponte para `mail.unkvoid.com`. Sem PTR o Gmail recusa na porta, e
o pedido pode levar horas para valer — é por isso que é o primeiro pedido e não o
último.

```bash
dig +short -x NOVO.IP.AQUI   # tem de responder mail.unkvoid.com.
```

**3. Copie a chave DKIM, não gere outra.** A chave copiada mantém o TXT do DNS
válido, então o DKIM continua fechando no segundo em que o IP muda. Gerar outra
significa um TXT novo, propagação, e uma janela em que a assinatura não fecha.

```bash
# da máquina velha para a nova: a chave privada, a pública e a config do rspamd
ssh vps-velha 'sudo tar -C /opt/unkvoid/mail -cz rspamd' > /tmp/dkim.tgz
ssh vps 'sudo tar -C /opt/unkvoid/mail -xz' < /tmp/dkim.tgz
rm -f /tmp/dkim.tgz
```

Os arquivos são `rspamd/dkim/rsa-2048-mail-unkvoid.com.private.txt` (a chave),
o `.public.dns.txt` (o valor do TXT, para comparar) e
`rspamd/override.d/dkim_signing.conf` (que aponta o selector `mail` para a
chave). Uma cópia do TXT está em `~/auxilos/dkim-unkvoid.txt` na máquina do dono.

**4. Copie as contas e as senhas.** As senhas moram em hash no
`postfix-accounts.cf`, então copiar o arquivo preserva as senhas que já estão
configuradas no celular e no Thunderbird de quem usa a caixa — ninguém precisa
reconfigurar nada. As senhas em claro estão em `~/auxilos/email-unkvoid.txt`, com
as duas contas: `contato@unkvoid.com` e `no-reply@unkvoid.com`.

```bash
ssh vps-velha 'sudo cat /opt/unkvoid/mail/postfix-accounts.cf' \
  | ssh vps 'sudo tee /opt/unkvoid/mail/postfix-accounts.cf > /dev/null'
ssh vps-velha 'sudo cat /opt/unkvoid/mail/dovecot-quotas.cf' \
  | ssh vps 'sudo tee /opt/unkvoid/mail/dovecot-quotas.cf > /dev/null'
```

**5. Copie as caixas.** São 288 KB hoje, num volume do docker. O contêiner tem de
estar **parado** dos dois lados durante a cópia, senão o Dovecot reescreve o
índice por baixo do `tar`:

```bash
ssh vps-velha 'cd /opt/unkvoid && docker compose stop mailserver && \
  sudo tar -C /var/lib/docker/volumes/unkvoid_mail-data/_data -cz .' > /tmp/caixas.tgz
ssh vps-velha 'cd /opt/unkvoid && docker compose start mailserver'

ssh vps 'cd /opt/unkvoid && docker compose stop mailserver'
ssh vps 'sudo tar -C /var/lib/docker/volumes/unkvoid_mail-data/_data -xz' < /tmp/caixas.tgz
ssh vps 'cd /opt/unkvoid && docker compose start mailserver'
```

O `mail-state` e o `mail-logs` **não** vêm: são cache do rspamd e log, que o
contêiner refaz. Copiá-los traz estado de fail2ban e bayes de outra máquina.

**6. Prove o envio, com o IP novo, antes de mexer no DNS.** Para o teste passar, o
IP novo precisa estar no SPF — acrescente e **não tire nada**, porque o velho
continua autorizado pelo `mx`:

```
TXT unkvoid.com → v=spf1 mx ip4:NOVO.IP.AQUI -all
```

Espere os 300 s do TTL e mande de verdade, da máquina nova:

```bash
docker exec unkvoid-mail swaks --server 127.0.0.1 --port 587 --tls \
  --auth-user no-reply@unkvoid.com --auth-password '<a senha>' \
  --from no-reply@unkvoid.com --to seu-endereco@gmail.com \
  --header 'Subject: teste do IP novo'
```

No Gmail, abra **Mostrar original**. Os três precisam dizer `PASS`: `SPF`, `DKIM`
e `DMARC`. Se o DKIM falhar, a chave não veio ou o `dkim_signing.conf` aponta para
um caminho que não existe — `docker logs unkvoid-mail | grep -i dkim` diz qual.

Mande também para `https://www.mail-tester.com`: ele dá nota e lista o que falta
(PTR, DKIM, DMARC, listas de bloqueio) numa tela só.

**7. Prove o recebimento, com o IP novo, antes de mexer no DNS.** Este teste não
precisa de DNS nenhum: entregue direto no IP, como um remetente faria se o MX já
apontasse para cá.

```bash
# de fora, de qualquer máquina
swaks --server NOVO.IP.AQUI --port 25 \
  --from teste@gmail.com --to contato@unkvoid.com \
  --header 'Subject: entrega direta no IP novo'
```

E confirme que a mensagem chegou **na caixa**, não só no log:

```bash
ssh vps 'sudo ls -la /var/lib/docker/volumes/unkvoid_mail-data/_data/unkvoid.com/contato/new/'
```

Se este passo falhar, o problema é porta 25 no firewall do painel, ou o
`postfix-accounts.cf` que não veio. Nada disso se descobre depois do corte.

**8. Tire um snapshot** (seção 1.5). É o último momento em que dá para voltar
atrás sem custo.

**9. Só agora o corte: o `A mail.unkvoid.com` para o IP novo.** Junto com ele os
outros três (`unkvoid.com`, `www`, `s3`), porque o site e o MinIO moram na mesma
máquina e um corte só é menos janela do que dois.

```bash
dig +short mail.unkvoid.com   # tem de responder o IP novo
dig +short unkvoid.com
```

**10. Limpe o SPF.** Tirado o `ip4:` temporário, `v=spf1 mx -all` volta a valer
sozinho, agora apontando para o IP novo pelo `mx`:

```
TXT unkvoid.com → v=spf1 mx -all
```

**11. Emita o certificado próprio** (seção 7) e confira que o contêiner de e-mail
o pegou — ele lê `/etc/letsencrypt` do host em modo leitura:

```bash
docker restart unkvoid-mail
openssl s_client -connect mail.unkvoid.com:993 -servername mail.unkvoid.com 2>/dev/null \
  | openssl x509 -noout -dates -subject
```

**12. A máquina velha continua ligada por 48 h a 72 h.** Remetente com DNS em
cache entrega no IP velho, e a porta 25 de lá tem de continuar aceitando, senão a
mensagem volta com erro. Antes de desligar de vez, drene e recopie:

```bash
ssh vps-velha 'docker exec unkvoid-mail postqueue -p'     # tem de dizer "Mail queue is empty"
ssh vps-velha 'docker exec unkvoid-mail postqueue -f'     # e força o que estiver preso
```
Depois repita o passo 5 (a cópia das caixas) para trazer o que chegou na velha
durante a janela. É a única parte do processo que se faz duas vezes, e é ela que
garante que nenhuma mensagem fica para trás.

**13. Desligue a máquina velha.** Só depois de: MX resolvendo para o IP novo,
fila vazia, caixas recopiadas, e um envio e um recebimento de teste passando
pelos nomes de verdade (não pelo IP).

### Se a reputação demorar

IP novo é IP desconhecido, e desconhecido não é o mesmo que ruim. O que fazer, na
ordem do que resolve mais:

1. **Confira o PTR primeiro, sempre.** `dig +short -x NOVO.IP` tem de responder
   `mail.unkvoid.com.`, e `dig +short mail.unkvoid.com` tem de voltar ao mesmo IP.
   Os dois lados fechando é o que o Gmail chama de FCrDNS, e é a checagem que ele
   aplica antes de qualquer outra.
2. **Veja se o IP já chega sujo.** Contabo recicla IP, e o anterior pode ter
   queimado. `https://mxtoolbox.com/blacklists.aspx` e o Spamhaus dizem em um
   minuto. Se estiver listado, **peça outro IP à Contabo** em vez de tentar
   limpar — é um pedido de suporte contra semanas de espera. Vale conferir isto
   **no primeiro dia da máquina**, antes de investir nela.
3. **Cadastre o domínio no Google Postmaster Tools.** É de graça e é a única
   janela para o que o Gmail pensa do IP.
4. **Aqueça devagar.** Poucas mensagens por dia na primeira semana, e para
   destinos que respondem. A caixa faz 0,1 mensagem por segundo em pico, então
   "aquecer" aqui é literalmente o uso normal.
5. **O remetente automático é o que mais sofre.** `no-reply@unkvoid.com` manda
   confirmação de conta e recuperação de senha, e cair na pasta de spam parece bug
   do site. Nos primeiros dias, teste com uma conta Gmail de verdade a cada
   cadastro e marque como "não é spam" quando cair errado.
6. **`p=quarantine` no DMARC ajuda agora.** Quarentena manda o que falhar para o
   spam em vez de descartar, o que dá para ver e corrigir. Subir para `p=reject`
   só depois de uma semana de `PASS` nos três.
7. **O que NÃO fazer:** mudar o DKIM de novo, mexer no SPF "para garantir", ou
   trocar o `From`. Cada mudança reinicia a contagem de confiança do zero.

---

## 12. A checagem final, item por item

Cada linha tem o comando e o que ele tem de responder. Uma que falha explica a
seguinte, então rode na ordem.

| # | O quê | Comando | Tem de responder |
|---|---|---|---|
| 1 | A máquina é a que se contratou | `nproc; free -g \| awk '/Mem/{print $2}'; df -h / \| tail -1` | `8`, `~23` e `300G` |
| 2 | A porta é de 600 Mb/s | painel: **Seus serviços → Detalhes do produto → Uplink** | `600 Mbit/s` |
| 3 | Swap existe | `swapon --show` | `/swapfile  file  4G` |
| 4 | O sysctl entrou | `sysctl net.core.wmem_default net.ipv4.ip_local_reserved_ports` | `4194304` e `40000-40006,41000-42000` |
| 5 | Os contêineres estão de pé | `docker ps --format '{{.Names}}'` | `unkvoid-mysql`, `unkvoid-minio`, `unkvoid-mail` |
| 6 | O banco responde e tem dados | `docker exec unkvoid-mysql mysql -uunkvoid -p"$MYSQL_PASSWORD" -e 'SELECT COUNT(*) FROM unkvoid.users'` | um número, não erro |
| 7 | Os buckets existem, e o `apt` é público | `mc ls local/; curl -sI http://127.0.0.1:9000/apt/InRelease \| head -1` | `apt/`, `unkvoid/` e `200 OK` |
| 8 | O site responde | `curl -s -o /dev/null -w '%{http_code}\n' https://unkvoid.com/up` | `200` |
| 9 | O SFU responde | `curl -s https://unkvoid.com/health` | `{"ok":true,...,"workers":[0,0,0,0,0,0,0]}` — **sete** zeros |
| 10 | O SFU abriu as faixas certas | `pm2 logs sfu --lines 40 --nostream \| grep 'media workers'` | `7 media workers on ports 40000-40006 · plain RTP on 41000-41447` |
| 11 | O ambiente do SFU é o do repositório, e não um deploy velho | `pm2 env 0 \| grep -E 'SFU_(WORKERS\|PLAIN_PORTS\|CONNECTIONS_PER_MINUTE\|ANNOUNCED)'` | `7`, `64`, `120`, e o **IP novo** |
| 12 | O chat está no ar | `pm2 list` e `curl -sI https://unkvoid.com/app/test` | `reverb` em `online`, e não 404 |
| 13 | O app recebe a configuração certa | `curl -s https://unkvoid.com/api/config` | `SFU_PUBLIC_URL` em `wss://unkvoid.com/sfu` e os campos do Reverb |
| 14 | O manifesto de atualização existe | `curl -s https://unkvoid.com/downloads/latest.json` | JSON com as plataformas; 404 aqui significa que ninguém se atualiza |
| 15 | O APT fecha a assinatura, de uma máquina limpa | `gpg --verify <(curl -s https://unkvoid.com/apt/InRelease)` | `Good signature from "Unkvoid APT..."` |
| 16 | As portas TCP passam o firewall do painel | `for p in 22 25 80 443 587 993 40000 40006; do nc -z -w4 unkvoid.com $p && echo "$p ok"; done` | as oito com `ok` |
| 17 | As portas UDP também | o tcpdump da seção 2, com `40000 40006 41000 41447 42000` | as cinco aparecem |
| 18 | O PTR fecha nos dois sentidos | `dig +short -x NOVO.IP; dig +short mail.unkvoid.com` | `mail.unkvoid.com.` e o mesmo IP |
| 19 | O DNS aponta para cá | `dig +short unkvoid.com s3.unkvoid.com mail.unkvoid.com; dig +short unkvoid.com MX` | o IP novo três vezes, e `10 mail.unkvoid.com.` |
| 20 | O e-mail sai e fecha os três | o `swaks` do passo 6 da seção 11, para um Gmail | `SPF`, `DKIM` e `DMARC` em `PASS` |
| 21 | O e-mail entra | o `swaks` do passo 7, e o `ls` da pasta `new/` | a mensagem na caixa |
| 22 | O IMAP tem o certificado do nome certo | `openssl s_client -connect mail.unkvoid.com:993 -servername mail.unkvoid.com` | cadeia válida para `mail.unkvoid.com` |
| 23 | O backup existe e foi restaurado uma vez | `systemctl list-timers unkvoid-dump` (caminho B) ou o painel (caminho A) | o timer com próxima execução, ou o backup ativo no painel |
| 24 | O runner está online e é só um | painel do GitHub, ou `sudo ./svc.sh status` em `~/actions-runner-unkvoid` | `active (running)`, e nenhum runner velho na lista |
| 25 | O pm2 volta depois do reboot | `sudo reboot`, e depois `pm2 list` | `sfu` e `reverb` em `online` sem ninguém subir à mão |
| 26 | Não sobrou erro de buffer | `grep ^Udp: /proc/net/snmp` | `SndbufErrors` e `RcvbufErrors` em zero — na máquina velha eram 30 094 e 19 267 |
| 27 | A latência é a esperada, e não pior | `ping -c 20 unkvoid.com` do Brasil | ~116 ms de média; acima de 150 ms, a região não é a que se pediu |
| 28 | Uma transmissão de verdade, de ponta a ponta | abrir o app, criar sala, compartilhar a tela, assistir de outra máquina | imagem aparecendo, e `A=$(awk '/eth0/{print $10}' /proc/net/dev); sleep 10; B=$(awk '/eth0/{print $10}' /proc/net/dev); echo $(( (B-A)*8/10/1000000 ))` dando a taxa esperada em Mb/s |

Os itens 27 e 28 são os únicos que provam o produto. Os 26 anteriores provam que
ele *pode* funcionar.

---

## 13. O que NÃO vai para a máquina nova

A máquina velha tem ~1,1 GB de RAM e 78 GB de disco em coisas que não servem. A
nova nasce sem elas — nada aqui é para migrar e depois desligar, é para não
instalar.

| O quê | Como roda hoje | Por que não vai |
|---|---|---|
| MySQL do sistema (`mysql.service`, 3306) | 395 MB de RAM | Só tem `discord`, `discord_dev` e `retro_friends`. O Unkvoid usa o MySQL do docker na 3307. Faça o dump antes de desligar a velha, por segurança |
| ~~Reverb do projeto `discord` (pm2, 8081)~~ | 61 MB | **Já saiu em 16/09/2026**: o `reverb` do pm2 agora é o do Unkvoid, na 8080 |
| ~~`filebrowser.service` (8080)~~ | 25 MB | **Já saiu em 16/09/2026**: desligado para liberar a 8080 para o Reverb do Unkvoid |
| `php8.3-fpm` | 50 MB | Nenhum site aponta para o socket dele. A nova instala só o 8.4 |
| TeamSpeak 6 (docker, 9987/udp, 30033/tcp) | 28 MB e 2,7% de CPU | Não é o e-mail nem o Unkvoid, e as duas portas saem do firewall |
| amavis dentro do `unkvoid-mail` | 176 MB | O rspamd já filtra; o amavis é a segunda passada, e a caixa faz 0,1 mensagem por segundo. `ENABLE_AMAVIS: 0` já está no `docker-compose.yml` |
| Sites do nginx: `files`, `ia.unkvoid.com`, `retro`, `tarkas` | — | Os quatro devolvem 404 ou apontam para um root que não existe |
| Certificados de `discord`, `ia`, `retro`, `tarkas`.unkvoid.com | — | São domínios que não vão existir aqui. A nova emite quatro nomes: `unkvoid.com`, `www`, `s3`, `mail` |
| Runners `actions.runner.edsuuu-{retro-friends,tarkas}` | — | Em `failed` desde sempre. A nova registra um runner, o `unkvoid-vps` |
| `/var/www/projects/discord`, `/var/www/discord-spike` | 78 MB de disco | Projeto morto |
| `/var/www/apt` e `/var/www/downloads` no disco | — | O repositório APT e os instaladores moram no MinIO; o nginx serve o bucket. Só o **conteúdo dos buckets** migra (seção 8.1) |
| `/etc/sysctl.d/99-unkvoid-udp.conf` e `99-livekit.conf` | — | O primeiro virou o `infra/sysctl-unkvoid.conf`; o segundo sobrou de um teste de LiveKit e é quem prendia `rmem_max` em 5 MB |
| O cache do Rust (`native/target`, `target-deb12`) | 9,4 GB de disco | É cache: se refaz. Custa um build de ~40 minutos, uma vez |
| `mail-state` e `mail-logs` do e-mail | — | Cache do rspamd, estado do fail2ban e log. O contêiner refaz, e trazer é herdar decisão tomada em outra máquina |
| A chave que assina as atualizações do app | — | Nunca esteve nesta máquina e não vem: quem a tiver publica atualização para todo mundo que instalou o app. Ver [AUTO-UPDATE.md](AUTO-UPDATE.md) |

O que **precisa** vir da velha está na seção 8.1 (banco, buckets, chave do APT),
na seção 9 (os segredos) e na seção 11 (as caixas, as contas e a chave DKIM). Fora
essas cinco coisas, a máquina nova se levanta inteira a partir deste repositório.
