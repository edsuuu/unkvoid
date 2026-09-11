# Roadmap — site, login, salas no Laravel e a VPS

Resumo do que foi decidido em 10/09/2026 e do que falta, na ordem em que uma coisa
depende da outra. O [ESTADO.md](ESTADO.md) diz onde o app parou; este arquivo diz
para onde ele vai.

## O desenho, em uma tela

```
app (Tauri) ──login Google──▶ unkvoid.com (Laravel) ──token HMAC──▶ app
app ──join {token}──▶ SFU (Node) ── só confere a assinatura, sem rede no caminho
Laravel ──POST /rooms/:code/kick, cabeçalho HMAC──▶ SFU   (expulsar = gravar ban + derrubar)
CI (runner na VPS) ──POST /api/releases, cabeçalho HMAC──▶ Laravel ──▶ MinIO
site ──URL assinada (vence em 1 h)──▶ MinIO             (instaladores nunca no disco)
apt ──GET──▶ nginx /apt/ ──▶ bucket público do MinIO      (índice assinado com GPG)
```

Um repositório só (`web/`, `sfu/`, `native/`, `infra/`), um runner na VPS, e cada
workflow filtra por pasta: mexer no site não recompila o app.

| Onde | O quê | Vida |
|---|---|---|
| Laravel + MySQL | conta, sala, dono, banimento, versões publicadas | banco, para sempre |
| Token assinado | sala, conta, nome, dono, validade | um minuto |
| SFU | `Room`, `Peer`, mídia | enquanto tem gente dentro |

## Fases

Cada fase é um fluxo independente, revisada e commitada antes da seguinte.

### 1. Site e login — feito, aguardando revisão

- `web/` a partir do template Laravel, sem Fortify. Dois layouts, `<x-guest-layout>`
  (site, escuro, o desenho da landing) e `<x-app-layout>` (painel), duplicados de
  propósito em vez de um `$layout` no construtor.
- Landing implementada a partir do `Unkvoid Landing.dc.html`: fundo em three.js,
  revelações com GSAP, três telas do app em Alpine, sistema detectado no cliente.
  `/` é `Route::view` → `home/index` → `<livewire:home.index>`, conteúdo em
  `livewire/home/index.blade.php`. `/privacidade` e `/termos` no mesmo visual.
- Login do zero, sem Fortify, seguindo o desenho do app: **login é opcional**.
  Site: `/login` e `/cadastro` (Livewire, e-mail e senha), `/oauth2/google` e
  `/oauth2/google/callback` (Socialite), `POST /logout`. API (Sanctum):
  `POST /api/auth/login`, `POST /api/auth/register`, `POST /api/auth/logout`,
  `GET /api/me`. Para o Google dentro do app: `GET /oauth2/app?port=N` abre o
  navegador e o callback devolve o token em `http://127.0.0.1:N/?token=…`.
- Quem entra com o e-mail de `UNKVOID_ADMIN_EMAIL` vira administrador na hora
  (`User::booted`). `/admin` guardado por `role:Administrador`.
- E-mails em HTML de tabela, no visual do site, por Notification: boas-vindas ao
  criar a conta (site, API ou Google), aviso de novo acesso a cada login, e o link
  de redefinição de senha (`/esqueci-a-senha`, `/redefinir-senha/{token}`). Falha
  de e-mail nunca derruba login: `User::notifyQuietly` grava no log e segue.
- Migrations: `users` ganhou `google_id`, `avatar_url` e `password` nula;
  `personal_access_tokens` do Sanctum.
- SFU exige o token assinado no `join`, e ganhou `POST /rooms/:code/kick` com
  cabeçalho assinado. `installId`, dono por instalação e banimento em memória
  saíram. `SFU_SECRET` obrigatório para subir.
- `infra/`: compose (MySQL 3307, MinIO, e-mail), nginx de `unkvoid.com`,
  `deploy-web.sh`, `sfu/install.sh` (espera esvaziar antes de reiniciar),
  `runner-install.sh`. Workflows `deploy-web`, `deploy-sfu`, `build-linux`.
  `apt-publish.sh` publica no bucket `apt` do MinIO em vez do disco.

### 2. Salas, versões e o app

Depende de aprovar o esquema: `rooms`, `room_bans`, `releases`.

- Salas com dono anônimo ou com conta: `rooms` guarda `user_id` **ou** `guest_id`
  (o UUID que o app já gera), e `room_bans` guarda o mesmo par. O token do SFU leva
  `sub` = `user:1` ou `guest:<uuid>`; o SFU não distingue.
- API: `GET|POST /api/rooms`, `DELETE /api/rooms/{code}`, `POST /api/rooms/{code}/token`
  (sem auth: manda `guest_id` e `name`; com token do Sanctum: usa a conta),
  `POST /api/rooms/{code}/bans` (dono; grava e chama o SFU), `POST /api/logs`
  (vai para `storage/logs/client-*.log`, lido no log-viewer),
  `POST /api/releases` (cabeçalho assinado, para o CI).
- Painel em `/admin`: versões (upload para o MinIO) e o log-viewer.
  `GET /downloads/latest.json` e `GET /downloads/{plataforma}` com URL assinada.
- App, a partir do `Unkvoid App.dc.html`: entrada com o painel lateral de login e
  cadastro, menu com Configurações, Logs e Sair, token antes do `join` e a cada
  reconexão, enviar diagnóstico, remover alguém passa pelo Laravel.

### 3. VPS — feito em 11/09/2026

- `/opt/unkvoid`: MySQL na 3307, MinIO e docker-mailserver no ar. Buckets `unkvoid`
  (privado) e `apt` (leitura pública), com os `.deb` de `/var/www/apt` migrados.
- nginx de `unkvoid.com`, `www`, `s3.unkvoid.com` e `mail.unkvoid.com` com um
  certificado só. Site em `/var/www/projects/unkvoid-web` (releases + `current`).
- Runner `unkvoid-vps` registrado como serviço; SFU novo no ar lendo `SFU_SECRET`.
- Caixas `contato@` e `no-reply@` criadas; senhas em `~/auxilos/email-unkvoid.txt`
  na máquina do Edsu. DKIM gerado, falta só o TXT no DNS.
- Backup do que não é Unkvoid em `/home/ubuntu/backups/2026-09-10` (bancos
  `discord`, `discord_dev`, `retro_friends` e as pastas). A limpeza em si ainda não
  foi feita: aguarda confirmação da lista.

### 4. Linux de verdade

- O WebKitGTK de Debian, Ubuntu, Mint e Parrot vem sem WebRTC, provado em Docker.
  Assistir no Linux é pelo navegador (`/assistir/{código}`) até existir um receptor
  nativo: as crates `rtc-*` já estão no repositório, falta decodificar e desenhar.
- Transmitir no Linux: captura pelo PipeWire e encoder por VA-API. É outro bloco.

### 5. Revisão

- Revisão de bugs na base inteira.

## E-mail: como entrar na caixa

O servidor é o docker-mailserver, sem webmail. Qualquer cliente de e-mail serve
(Thunderbird, Gmail no celular com "outra conta", Mail do iPhone):

| | |
|---|---|
| Usuário | o endereço inteiro, `contato@unkvoid.com` |
| Senha | a definida em `docker exec unkvoid-mail setup email add contato@unkvoid.com` |
| Receber | IMAP, `mail.unkvoid.com`, porta 993, SSL/TLS |
| Enviar | SMTP, `mail.unkvoid.com`, porta 587, STARTTLS |

O `no-reply@unkvoid.com` é a conta que o Laravel usa para enviar (`MAIL_USERNAME`
e `MAIL_PASSWORD` no `.env` da VPS). Não precisa ser lida.

## O que o DNS precisa ter

| Registro | Valor | Para quê |
|---|---|---|
| `A unkvoid.com`, `A www` | 144.126.133.10 | já existe |
| `A s3.unkvoid.com` | 144.126.133.10 | URLs assinadas do MinIO |
| `A mail.unkvoid.com` | 144.126.133.10 | o servidor de e-mail |
| `MX unkvoid.com` | `10 mail.unkvoid.com` | receber |
| `TXT unkvoid.com` | `v=spf1 mx -all` | SPF |
| `TXT mail._domainkey` | o que `setup config dkim` imprimir | DKIM |
| `TXT _dmarc` | `v=DMARC1; p=quarantine; rua=mailto:contato@unkvoid.com` | DMARC |
| PTR de 144.126.133.10 | `mail.unkvoid.com` | no painel da Contabo; sem isto o Gmail recusa |

E no firewall do painel: TCP 25, 465, 587, 993.

## Deploy do SFU sem derrubar ninguém

Reiniciar o SFU mata os workers do mediasoup, e com eles toda sala no ar. O app já
se recupera sozinho (reconecta, republica a transmissão), mas são alguns segundos de
tela preta para todo mundo. O `install.sh` espera `rooms == 0` no `/health` por até
30 minutos antes de reiniciar, e só passa por cima do limite se estourar.

Zero downtime de verdade exigiria dois processos em faixas de porta UDP diferentes,
o firewall aberto para as duas, e cuidado com sala repetida nos dois. Fica para
quando houver gente na sala a qualquer hora do dia.
