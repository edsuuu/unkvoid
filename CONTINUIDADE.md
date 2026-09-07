# Discord 2.0 — estado do projeto

Documento para quem pegar o trabalho daqui. Diz o que existe, o que **não** existe,
e onde estão as armadilhas que já custaram tempo.

**Web em produção:** https://discord.unkvoid.com · **Repo:** github.com/edsuuu/discord2.0
**VPS:** 144.126.133.10 (Contabo, St. Louis/EUA)

---

## As três partes

```
web/      Laravel 13 + Livewire 4   auth, servidores, canais, chat, API do desktop
sfu/      Node + TypeScript          mídia (mediasoup) e sinalização (WebSocket)
native/   Rust + Tauri               app desktop com captura nativa
```

O `web/` e o `sfu/` estão **em produção e funcionando**. O `native/` está no começo.

---

## O que funciona hoje

### Web (produção)
Login por e-mail/senha e Google, servidores com convite, canais de texto e voz,
chat, voz com compartilhamento de tela pelo SFU, presença por WebSocket, moderação
em três níveis, reconexão que sobrevive a queda de rede e a deploy do SFU.

### SFU (produção, 4 workers)
API em TypeScript no padrão do MoneyClips: rota → Request → controller → Service →
Resource. Cobre retomada de sessão, presença, moderação e **relay de sinalização P2P**
(`signal`), que existe mas ainda não tem cliente usando.

```bash
cd sfu && pnpm run check   # asserções sobre o contrato inteiro
```

### API para o desktop
`web/routes/api.php`, autenticada por Sanctum com token. Login, servidores, canais,
mensagens, e os mesmos emissores de token do SFU que o web usa.
Testes em `web/tests/Feature/Api/DesktopApiTest.php`.

### App desktop (v0.2.0)
Abre, verifica atualização, exige servidor, pede login, lista servidores e canais com
o design do web, e **captura a tela nativamente** — sem barra do Chrome.

---

## O que NÃO existe

| Peça | Situação |
|---|---|
| **WebRTC no app desktop** | **nada.** A captura entrega quadros que morrem ali |
| Encoder de vídeo | nenhum. Falta VideoToolbox (mac) / Media Foundation (win) |
| P2P ponta a ponta | servidor pronto, cliente não começou |
| Captura no Linux | recusa com erro claro; falta consumir o nó do PipeWire |
| Áudio de sistema no Windows | precisa de WASAPI loopback, separado do Graphics Capture |
| Chat no desktop | lista mensagens em texto cru, sem enviar |
| SFU em Rust (str0m) | não começou |

**Nenhuma captura foi testada de verdade.** No macOS a permissão foi negada nesta
máquina; Windows e Linux só passaram por cross-compile (type-check, não execução).

---

## Próximo passo sugerido

O caminho de mídia do app, nesta ordem — cada etapa é verificável sozinha:

1. **Encoder**: quadros BGRA da `capture` → H.264. `videotoolbox` no macOS.
   Verificação: gravar 10s em arquivo e abrir.
2. **PeerConnection** com a crate `webrtc` (0.21-rc), usando o relay `signal` do SFU
   para trocar SDP e ICE. Verificação: dois apps na mesma sala, `connectionState`
   virando `connected`.
3. **Ligar encoder ao track** e assistir do outro lado.
4. **Regra dos 3**: acima de 3 espectadores, cair para o SFU. O upload de quem
   compartilha multiplica no P2P (4 pessoas em 1080p ≈ 28 Mbps de subida).

Por que P2P primeiro: a VPS está nos EUA e os usuários no Brasil — **139 ms de RTT
medidos**. Dois brasileiros direto ficam em ~20 ms.

---

## Armadilhas já pagas

**Rust / macOS**
- A crate `screencapturekit` compila Swift e o linker procura o runtime no caminho do
  Xcode completo. Com só os Command Line Tools o caminho é outro — resolvido em
  `native/.cargo/config.toml`, junto do alvo mínimo 13.0.
- Sem `NSScreenCaptureUsageDescription` no Info.plist o macOS nem mostra o pedido de
  permissão. Está em `native/apps/desktop/src-tauri/Info.plist`.
- O app não é assinado nem notarizado: o Gatekeeper bloqueia. `xattr -dr
  com.apple.quarantine` resolve. Assinar exige conta Apple paga.

**SFU**
- `pm2 restart <nome> --update-env` relê o ambiente do **shell**, não o
  `ecosystem.config.cjs`. Mudança no ecosystem exige `pm2 delete` + `pm2 start`.
- `pnpm install` não baixa o worker do mediasoup e **sai com erro** por isso. O deploy
  roda o postinstall na mão e valida o binário.
- O mediasoup é **ICE Lite**: só responde, nunca inicia. Atrás de firewall stateful a
  porta de mídia precisa aceitar entrada não solicitada. São 4 portas (40000-40003),
  uma por worker — `WebRtcServer` não é compartilhável entre processos.

**Laravel**
- Usuários são **UUID**. A tabela do Sanctum precisou de `uuidMorphs`, não `morphs`.
- `#[Override]` em `rules()` de FormRequest quebra: o pai não declara o método.
- `wire:ignore` no palco de voz é obrigatório — sem ele o Livewire recria o trecho a
  cada render e leva os `<video>` junto.

**Rede**
- O firewall da Contabo tem allowlist por porta. Abertas: 22, 80, 443, 8443,
  30033/tcp, 9987/udp e 40000-40003 (tcp+udp).

---

## Deploy

```bash
./infra/deploy-web.sh    # Laravel
./sfu/deploy.sh          # SFU
```

`.env` e `storage` vivem em `shared/` e sobrevivem ao deploy. Nenhum segredo está no
repositório.

App desktop: tag `v*` dispara o CI, que gera `.msi`, `.deb` e `.dmg` em runners
nativos e anexa na Release. Instalador não pode ser cross-compilado.

**A chave privada do updater não está no repositório.** Ela precisa ir para os
secrets do GitHub como `TAURI_SIGNING_PRIVATE_KEY` para o CI assinar as atualizações.
Se ela for perdida, nenhuma versão futura consegue atualizar as já instaladas.

---

## Decisões que valem entender antes de mudar

- **Sem Redis.** Cache, fila e sessão em banco.
- **Sem Reverb.** Chat por `wire:poll`; presença por WebSocket do próprio SFU.
- **Sem navegação de página no workspace.** Trocar de canal é estado Livewire — é o
  que impede a chamada de cair. A URL é reescrita com `history.replaceState`.
- **Tema escuro fixo.** O alternador do template causava texto escuro sobre fundo
  escuro quando o SO estava em claro.
- **`maintain-framerate` + `contentHint: motion`** e piso de 30 fps na captura. Com
  `maintain-resolution` o FPS oscilava entre 5 e 60.
