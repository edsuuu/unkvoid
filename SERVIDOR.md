# O servidor, do zero

Como levantar de novo o que hoje roda em `discord.unkvoid.com`, na ordem em que
as peças dependem umas das outras.

Escrito olhando a máquina que existe, não de memória. O que estiver aqui foi
verificado; o que não deu para verificar está marcado.

Hoje é um Ubuntu 24.04 na Contabo. A mesma máquina é servidor de mídia, servidor
de download e repositório APT.

## A ordem

1. Firewall — sem ele, nada do resto responde
2. Pacotes base, Node, pm2
3. nginx e certificado
4. O SFU
5. O repositório APT
6. A pasta de downloads e o manifesto de atualização

## 1. Firewall

**É o primeiro passo porque é o que mais custou.** O firewall da Contabo é do
painel, não da máquina: `ufw status` diz `inactive` e o `iptables` está limpo, e
mesmo assim o tráfego é descartado antes de chegar. Procurar no servidor não
acha nada.

No painel, em Serviços de Rede → Firewall. O campo de portas aceita intervalo
(`41000-42000`) e lista separada por vírgula — não precisa de uma regra por
porta.

| Protocolo | Portas | Para quê |
|---|---|---|
| TCP | 22 | ssh |
| TCP | 80, 443 | nginx |
| TCP e UDP | 40000-40003 | WebRTC de quem assiste, uma porta por worker do mediasoup |
| UDP | 41000-42000 | RTP puro de quem transmite pelo app |
| TCP | 30033 | TeamSpeak |
| UDP | 9987 | TeamSpeak |

A faixa de RTP é larga de propósito. O mediasoup **sorteia** uma porta dentro da
faixa do worker e só confere se ela está livre na máquina, nunca se é alcançável
de fora. Uma porta sorteada fora do que o firewall abre vira uma transmissão em
que todo contador marca saúde, o socket aceita cada byte, e nada chega do outro
lado. Foi exatamente esse o sintoma que custou uma noite: preto, reinicia e
pega, muda a qualidade e morre de novo.

Confira de fora, não de dentro. TCP dá para testar com `nc -z`; UDP só com uma
captura do outro lado:

```bash
# na VPS
sudo tcpdump -nn -i any 'udp and dst portrange 41000-42000'

# na sua máquina
printf 'teste' | nc -u -w0 SEU.IP.AQUI 41500
```

Se não aparecer no tcpdump, é o firewall do painel, não o código.

## 2. Pacotes base, Node, pm2

```bash
sudo apt update && sudo apt install -y \
  build-essential curl wget file pkg-config cmake git nginx \
  libwebkit2gtk-4.1-dev libssl-dev libayatana-appindicator3-dev \
  librsvg2-dev libxdo-dev
```

Os cinco últimos são para compilar o app Linux nesta máquina. O `cmake` é do
`opusic-sys`, que compila o libopus do zero — sem ele o build morre depois de
quarenta minutos, não no começo.

Node e pm2, nas versões que estão rodando hoje:

```bash
curl -fsSL https://deb.nodesource.com/setup_24.x | sudo -E bash -
sudo apt install -y nodejs
sudo npm install -g pnpm pm2
```

| Ferramenta | Versão hoje |
|---|---|
| Node | 24.19.0 |
| npm | 11.17.0 |
| pnpm | 11.21.0 |
| pm2 | 7.0.4 |

Rust, só se esta máquina for compilar o app Linux:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
```

## 3. nginx e certificado

As pastas, com dono `ubuntu` para publicar sem `sudo`:

```bash
sudo mkdir -p /var/www/apt /var/www/downloads/unkvoid /var/www/projects
sudo chown -R ubuntu:ubuntu /var/www/apt /var/www/downloads /var/www/projects
```

O site, em `/etc/nginx/sites-available/discord`:

```nginx
server {
    server_name discord.unkvoid.com;

    # A sinalização do SFU. `upgrade` porque é WebSocket, e `proxy_buffering off`
    # porque bufferizar sinalização é atrasar o começo de cada transmissão.
    location /sfu {
        proxy_pass http://127.0.0.1:3000;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_read_timeout 600s;
        proxy_send_timeout 600s;
        proxy_buffering off;
    }

    location /health {
        proxy_pass http://127.0.0.1:3000;
        proxy_set_header Host $host;
        add_header Access-Control-Allow-Origin "*" always;
    }

    location /apt/ {
        alias /var/www/apt/;
        autoindex on;
    }

    location = /downloads { return 301 /downloads/; }

    # `no-store` é o que faz o manifesto de atualização valer no minuto em que
    # sobe. Com cache, o app continua vendo a versão de ontem.
    location /downloads/ {
        alias /var/www/downloads/unkvoid/;
        autoindex on;
        add_header Cache-Control "no-store" always;
    }

    location / { return 404; }
}
```

**A mídia não passa por aqui.** Ela vai direto por UDP nas portas 40000-40003 e
41000-42000. O nginx só carrega sinalização e arquivo.

```bash
sudo ln -s /etc/nginx/sites-available/discord /etc/nginx/sites-enabled/
sudo nginx -t && sudo systemctl reload nginx
sudo certbot --nginx -d discord.unkvoid.com
```

O certbot reescreve o bloco acrescentando o `listen 443 ssl` e o redirecionamento
do 80. Não mexa nas linhas que ele marca com `# managed by Certbot`.

## 4. O SFU

Do seu Mac, com o alias `vps` no `~/.ssh/config`:

```bash
cd sfu && ./deploy.sh vps
```

Ele compila o TypeScript aqui, manda por rsync, instala as dependências de
produção e sobe pelo pm2. Termina batendo no `/health`, então um deploy que
imprime `{"ok":true,...}` é um deploy que subiu de verdade.

O `.env` fica **só na VPS**, em `/var/www/projects/sfu/.env`, com o
`SFU_SECRET`. O `ecosystem.config.cjs` define o resto: 4 workers, mídia em
40000, RTP puro a partir de 41000 com 8 portas por worker.

Para o pm2 voltar sozinho depois de um reboot:

```bash
pm2 startup   # e rode a linha que ele imprimir
pm2 save
```

## 5. O repositório APT

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
curl -fsSL https://discord.unkvoid.com/apt/unkvoid.gpg \
  | sudo tee /usr/share/keyrings/unkvoid.gpg > /dev/null
echo "deb [signed-by=/usr/share/keyrings/unkvoid.gpg] https://discord.unkvoid.com/apt ./" \
  | sudo tee /etc/apt/sources.list.d/unkvoid.list
sudo apt update && sudo apt install unkvoid
```

Depois disso, `sudo apt upgrade` junto com o resto da máquina. **No Linux o
atualizador embutido do app está desligado de propósito**: pedir senha de root
com `pkexec` no meio da abertura faria o que o `apt` já faz.

Para conferir que a assinatura fecha, de uma máquina limpa:

```bash
gpg --verify <(curl -s https://discord.unkvoid.com/apt/InRelease)
```

## 6. Downloads e manifesto de atualização

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

## O que mais roda nesta máquina

Não faz parte do Unkvoid, mas uma VPS nova que substitua esta precisa saber que
existe:

| O quê | Como roda | Portas |
|---|---|---|
| Laravel Reverb | pm2, `php`, em `/var/www/projects/discord/current` | atrás do nginx |
| TeamSpeak 6 | docker, `teamspeaksystems/teamspeak6-server` | 9987/udp, 30033/tcp |
| n9router | docker | 127.0.0.1:20128 |
| Outros sites | nginx: `files`, `ia.unkvoid.com`, `retro` | 443 |

**O Laravel não está documentado aqui.** Só dá para ver de fora que o Reverb sobe
pelo pm2 a partir daquele caminho e que há um `php` escutando em 127.0.0.1:8081.
Como subir o projeto — migrations, `.env`, php-fpm, filas — precisa vir de quem o
conhece, e inventar os passos seria pior do que não tê-los.

## Quando alguma coisa não responde

Na ordem, porque cada uma explica a seguinte:

1. `curl -s https://discord.unkvoid.com/health` — se não responder, é nginx ou
   pm2, e nada de mídia vai funcionar.
2. `pm2 list` e `pm2 logs sfu --lines 50`.
3. `sudo ss -lntup | grep -E '3000|4000[0-3]'` — o processo está escutando?
4. O tcpdump da seção 1 — o pacote chega na máquina? Se não, é o painel da
   Contabo.
5. `curl -s https://discord.unkvoid.com/downloads/latest.json` — 404 aqui
   significa que ninguém se atualiza, mesmo com tudo o resto de pé.
