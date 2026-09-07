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

### App desktop (v0.4.0)
Abre, verifica atualização, exige servidor, pede login (e-mail/senha **ou Google**),
lista servidores e canais com o design do web, **captura a tela nativamente** e
**transmite por P2P**.

**Topologia:** quem envia usa Rust (captura nativa + encoder por hardware, sem barra
do navegador); quem recebe usa o WebRTC do próprio webview e um `<video>`. A
limitação do WKWebView era só o `getDisplayMedia` — receber vídeo ele faz bem, e
assim não é preciso decodificar nem desenhar em Rust.

**Áudio do sistema entra na transmissão** em Opus (48 kHz estéreo, blocos de 20 ms),
como segunda trilha da mesma conexão.

> **O som do próprio app fica de fora.** Quem filtra é o macOS, por processo
> (`with_excludes_current_process_audio`), não um `if` no nosso código. Sem isso,
> compartilhar áudio devolveria a voz de quem está na chamada e criaria
> realimentação. A crate documenta essa opção literalmente como *"Prevent feedback"*.
>
> Filtrar por processo é mais confiável que tentar adivinhar a origem do som: o
> sistema sabe exatamente o que saiu de qual aplicativo.

**Um encoder, N conexões:** o quadro é comprimido uma vez e enviado a cada
espectador. Codificar por espectador derreteria a máquina de quem transmite — o
custo do P2P é banda de upload, não CPU. Limite de 3 espectadores (`LIMITE_P2P`);
acima disso o SFU compensa.

A sinalização vai pelo WebSocket do SFU (ação `signal`): mesma sala, mesma
autenticação, nenhum canal novo.

---

## ⚠️ Só existe build de macOS

**Nenhum `.msi` ou `.deb` foi gerado ou publicado até hoje.** Todas as releases
(v0.1.0 a v0.5.0) contêm apenas artefatos de macOS, e só para Apple Silicon.

O workflow do CI (`.github/workflows/desktop.yml`) está configurado para os três
sistemas e nunca rodou — as releases foram feitas à mão, desta máquina.

### Por que não dá para gerar daqui

Instalador **não pode ser cross-compilado**: `.msi` exige Windows, `.deb` exige
Linux. É limitação da ferramenta, não escolha.

E mesmo o `cargo check` cruzado para Windows para no `ring` (dependência de
criptografia do WebRTC), que precisa de um toolchain C do Windows. Num runner nativo
compila normalmente.

### O que foi verificado por plataforma

| Peça | macOS | Windows | Linux |
|---|---|---|---|
| `capture` | roda | type-check cruzado | type-check cruzado |
| `media` (encoder + WebRTC) | roda e medido | **não verificado** | **não verificado** |
| App Tauri | roda | **não verificado** | **não verificado** |
| Instalador | `.dmg` publicado | **nunca gerado** | **nunca gerado** |

### O que o app faria hoje fora do macOS

Compila e abre, mas **não transmite**: o encoder por hardware só existe no macOS
(VideoToolbox). Fora dele, `PlatformEncoder::new` devolve `EncoderError::Unsupported`
e a interface mostra o erro.

O stub existe com a **mesma forma** do encoder real de propósito — sem isso o app nem
compilaria fora do mac, e aí nem o `.msi` sairia. Antes desta correção o
`broadcast.rs` usava um campo (`surface`) que só existia no macOS: **o build de
Windows estava quebrado, não só não testado.**

### Para destravar

1. Rodar o workflow (`workflow_dispatch` ou uma tag `v*`) e ver o que quebra de
   verdade num runner nativo.
2. Encoder no Windows: Media Foundation, espelhando `crates/media/src/macos.rs`.
3. Captura no Linux: consumir o nó do PipeWire que o portal XDG devolve.

---

## O que NÃO existe

| Peça | Situação |
|---|---|
| **Microfone** | só o áudio do sistema entra; falta a voz de quem transmite (`cpal`) |
| Trocar qualidade sem parar | o app web faz; aqui exige reiniciar a transmissão |
| Fallback para o SFU | acima de 3 espectadores recusa, mas não cai para o SFU sozinho |
| Encoder no Windows | falta Media Foundation — ver a seção sobre build acima |
| Chat no desktop | lista mensagens em texto cru, sem enviar |
| Captura no Linux | recusa com erro claro; falta consumir o nó do PipeWire |
| Áudio de sistema no Windows | precisa de WASAPI loopback, separado do Graphics Capture |
| Chat no desktop | lista mensagens em texto cru, sem enviar |
| SFU em Rust (str0m) | não começou |

**A captura nunca rodou de verdade.** No macOS a permissão de gravação de tela foi
negada nesta máquina; Windows e Linux só passaram por cross-compile. O encoder e a
negociação WebRTC **foram testados** e os números estão abaixo — o que não foi testado
é a ponta a ponta com tela real e duas pessoas.

---

## Próximo passo sugerido

O caminho de mídia do app, nesta ordem — cada etapa é verificável sozinha:

1. **Microfone**: capturar com `cpal` e misturar com o áudio de sistema antes do
   Opus, ou publicar como terceira trilha.
2. **Cair para o SFU acima de 3**: hoje `broadcast` recusa. A rota do SFU já existe
   e funciona no app web — falta o app escolher entre as duas.
3. **Encoder no Windows** (Media Foundation) e captura no Linux (PipeWire).
4. **Chat no desktop**: a API já envia e lê; falta a interface.

### Perfis de qualidade

Escolhidos na interface e válidos ponta a ponta — a mesma opção define a resolução da
captura **e** o bitrate do encoder:

| Perfil | Resolução | Bitrate | Custo medido |
|---|---|---|---|
| 720p | 1280×720 | 4 Mbps | — |
| 1080p | 1920×1080 | 7 Mbps | 7,86 ms/quadro (47%) |
| 1440p | 2560×1440 | 12 Mbps | 12,25 ms/quadro (73%) |

Áudio: Opus a 96 kbps, 48 kHz estéreo, independente do perfil de vídeo.

### Verificações que existem

```bash
cargo run -p media --example encoder   # encoder por hardware, ms/quadro
cargo run -p media --example peer      # oferta com H.264 + ICE
cargo run -p media --example p2p       # negociação completa entre dois lados
cargo run -p capture --example spike   # captura (exige permissão de tela)
cargo test --workspace                 # inclui o empacotamento de blocos Opus
```

### Números já medidos do encoder

| Resolução | ms/quadro | Orçamento a 60 fps | Uso |
|---|---|---|---|
| 1080p | 7,86 ms | 16,67 ms | 47% |
| 1440p | 12,25 ms | 16,67 ms | 73% |

Medido com superfície estática — tela real dá mais trabalho. 1440p60 é o limite.

Por que P2P primeiro: a VPS está nos EUA e os usuários no Brasil — **139 ms de RTT
medidos**. Dois brasileiros direto ficam em ~20 ms.

---

## Armadilhas já pagas

**Build**
- O Opus vem da crate `opus`, que compila libopus do zero via **cmake**. Os runners do
  GitHub já têm cmake; localmente foi preciso `brew install cmake`.

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
