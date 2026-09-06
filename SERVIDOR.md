# Discord 2.0 — o que está rodando na VPS

**URL:** https://discord.unkvoid.com · **VPS:** 144.126.133.10 (Contabo, St. Louis/EUA)
**Repositório:** https://github.com/edsuuu/discord2.0 (privado)

---

## Arquitetura

```
                        Navegador (Chrome/Edge)
                          │              │
              HTTPS/WSS   │              │  WebRTC (UDP 40000)
                          ▼              │
                   ┌──────────────┐      │
                   │    nginx     │      │
                   │  :80 :443    │      │
                   └──┬────────┬──┘      │
             /        │        │  /sfu   │
                      ▼        ▼         │
              ┌───────────┐  ┌─────────────────┐
              │  Laravel  │  │   API de mídia  │
              │ php8.4-fpm│  │  Node + pm2     │
              │  Livewire │  │  (mediasoup)    │
              └─────┬─────┘  └─────────────────┘
                    │            ▲
                    │  JWT HS256 │  (identidade + sala + papel)
                    └────────────┘
                    │
                    ▼
                 MySQL 8
```

A mídia **não** passa pelo nginx nem pelo PHP: vai direto do navegador para a porta
UDP 40000. O Laravel só diz **quem** pode entrar **onde** e com **qual papel**.

---

## Serviços

| Serviço | Onde | Como sobe |
|---|---|---|
| Laravel 13 + Livewire 4 | `/var/www/projects/discord/current` | php8.4-fpm + nginx |
| API de mídia (SFU) | `/var/www/projects/sfu` | `pm2` (`sfu`), habilitado no boot |
| MySQL 8 | local | systemd |

```bash
pm2 list                 # estado da API de mídia
pm2 logs sfu             # logs
curl 127.0.0.1:3000/health
```

## Portas

| Porta | Protocolo | Uso | Liberada no firewall |
|---|---|---|---|
| 80 / 443 | TCP | nginx | sim |
| 3000 | TCP | API de mídia (só 127.0.0.1) | não precisa |
| **40000** | **UDP** | **mídia WebRTC** | **sim — sem isso nada conecta** |
| 40000 | TCP | fallback de mídia | sim |

> **Por que a porta UDP precisa estar liberada:** o mediasoup é **ICE Lite** — ele só
> responde a checagens ICE, nunca inicia. Num firewall stateful isso significa que a
> porta tem que aceitar entrada não solicitada. Foi medido: com um listener na porta,
> pacotes de fora não chegavam até a regra ser criada no painel da Contabo.

Ajuste de kernel aplicado (`/etc/sysctl.d/99-livekit.conf`): buffers de UDP de 212 KB
para 5 MB. O default gera perda de pacote sob carga.

---

## Deploy

```bash
./infra/deploy-web.sh    # Laravel: build, rsync, composer, migrate, cache
./sfu/deploy.sh          # API de mídia: rsync, deps, pm2
```

`.env` e `storage` moram em `shared/` e sobrevivem ao deploy. Nenhum segredo está no
repositório — o `SFU_SECRET` fica em `/var/www/projects/sfu/.env` (600).

**Duas armadilhas que já custaram tempo:**

1. `pnpm install` não baixa o worker do mediasoup (o pnpm 11 ignora
   `onlyBuiltDependencies` e ainda sai com erro). O deploy roda o postinstall na mão.
2. `pm2 restart <nome> --update-env` relê o ambiente do **shell**, não o
   `ecosystem.config.cjs`. Use `pm2 startOrRestart ecosystem.config.cjs --update-env`.

---

## A API de mídia

Escrita por nós, na estrutura do MoneyClips: rota → Request (validação na fronteira,
com acessores) → controller magro → Service → **retorno sempre via Resource**.

```
src/
  Enums/          Action, Role
  Exceptions/     ApiException + 422/401/403/404
  Http/
    routes.js       mapa ação → request + handler ('guest' só no join)
    Kernel.js       despacho e tradução de exceção para status
    Server.js       WebSocket + /health
    Requests/       validação e acessores por ação
    Controllers/    Join, Transport, Producer, Consumer, Moderation
    Resources/      shape de toda resposta
  Services/       RoomRegistry, Room, Peer, TokenVerifier
```

O mediasoup entra só como motor de transporte (ICE, DTLS, SRTP, RTP, estimativa de
banda) — o mesmo papel que o Pion faz dentro do LiveKit.

```bash
cd sfu && pnpm run check   # 14 asserções sobre o contrato da API
```

O check já pegou um bug real: o `SFU_SECRET` não estava chegando na VPS.

---

## Autenticação e permissão

- E-mail/senha (Fortify) e **Google OAuth** em `/oauth2/google`
- Toda pessoa é um **UUID** — `users.id` é uuid, não sequencial
- Primeiro acesso sem nickname cai em `/bem-vindo` (middleware `nickname`)
- **Só entra em servidor por link de convite** (`/convite/{code}`) e **só logado**
- Papéis por servidor: `owner`, `admin`, `member`

O dono/admin pode **encerrar a transmissão** de alguém e **expulsar da chamada**. Isso
é verificado nos dois lados: no Laravel (quem pode disparar) e no SFU (o papel vem
assinado dentro do token, o cliente não escolhe).

**Token de voz:** `POST /api/voz/{channel}/token` verifica canal de voz → membro do
servidor → emite JWT HS256 (`sub`, `room`, `role`, 6h). O SFU valida assinatura,
expiração e sala. TTL longo de propósito: o token é reusado na reconexão.

---

## Banco

`users` · `servers` · `server_members` · `channels` · `messages` — tudo com UUID.
Criar servidor gera, em transação, o dono + `#geral` (texto) + `Geral` (voz).

---

## Decisões que valem lembrar

- **Sem Redis.** Cache, fila e sessão em banco. Um serviço a menos.
- **Sem Reverb no MVP.** Chat atualiza por `wire:poll`. Teto: N usuários × 1 req/2s.
- **Sem navegação de página no workspace.** Trocar de canal é estado Livewire, não
  `wire:navigate` — é o que impede a chamada de voz de cair ao abrir um chat. A URL
  (`/canais/{servidor}/{canal}`) é reescrita com `history.replaceState`, não navegando.
- **`wire:ignore` no palco de voz é obrigatório.** Sem ele o Livewire recria o trecho a
  cada render e leva junto os `<video>` da chamada. Pelo mesmo motivo, a visibilidade e
  o rótulo de status vivem no Alpine, não em classes que o JS aplica no DOM.
- **Tema escuro fixo.** O alternador do template foi removido: com o SO em claro, o
  `.dark` não era aplicado e o texto sumia sobre os fundos escuros do Discord.
- **LiveKit removido.** Serviu de referência para provar o caminho de mídia; agora que
  o SFU é nosso, o container, a config e a rota `/rtc` saíram.

---

## Pendências conhecidas

- Áudio da tela só existe em Chrome/Edge (macOS exige Chrome ≥ 141 e macOS ≥ 14.2)
- Servidor nos EUA: ~139 ms de RTT do Brasil. Tolerável para tela, pesado para voz.
  Uma VPS em São Paulo levaria a ~20 ms
- Egress: 5 espectadores em 1440p ≈ 18 GB/hora
- Perfis de qualidade (codec, simulcast × SVC) ainda não foram medidos sob carga real
