# Unkvoid

Compartilhar a tela com quem você mandar o código, **sem perder fps no jogo** — e, para quem
tem conta, servidores com canais de texto e voz, câmera e chat, no molde do Discord.

Você abre o app, escreve seu nome e clica em **Criar uma sala**. Sai um código de 12
caracteres. Manda o código para quem quiser; quem cola o código entra e vê a tela de quem
estiver transmitindo. O código **é** a sala: não existe em banco nenhum e some quando o
último sai.

## Por que um app, e não o navegador

No navegador o encoder de vídeo é da CPU. Transmitindo 1080p60 enquanto se joga, a CPU
disputa com o jogo e a transmissão cai para 1 fps ou trava — que é o problema que este
projeto existe para resolver. Aqui o caminho é outro:

```
captura → textura na GPU → encoder da placa de vídeo → 1 quadro → SFU → N espectadores
```

O quadro nunca passa pela CPU antes de ser codificado, é codificado **uma vez**, e sobe
**uma vez** para o servidor, que replica. O upload de quem transmite não cresce com a
plateia. O encoder roda em tempo real e sem B-frames, que comprimem melhor mas exigem
reordenar quadros — latência que uma chamada não paga.

De quebra: sem barra do Chrome por cima, e o áudio do sistema entra junto.

## As três peças

| Pasta | O quê | Onde roda |
|---|---|---|
| `native/` | o app: captura, encoder, interface (Rust + Tauri + React em TypeScript) | na máquina de quem usa |
| `sfu/` | o relé de mídia (Node 22 + mediasoup) | na VPS |
| `web/` | site, contas, servidores, canais, chat, auditoria (Laravel 13 + Livewire 4 + Flux) | na VPS |

**Sala por código (sem conta):** o app fala só com o SFU, sem banco e sem login. **Servidores
(com conta):** o Laravel decide quem pode o quê e assina um token de 60 s; o SFU só confere a
assinatura; o app só esconde botão. Os dois modos são produto: mexer num não degrada o outro.

Como as peças conversam, os fluxos e o que roda onde: [docs/ARQUITETURA.md](docs/ARQUITETURA.md).
Tudo o que atravessa a rede: [docs/CONTRATO.md](docs/CONTRATO.md).

## Estado por sistema

| | Captura de tela | Áudio do sistema | Encoder | Assistir |
|---|---|---|---|---|
| Windows | Graphics Capture | WASAPI loopback, por processo | Media Foundation (NVENC/QuickSync/VCE); sem placa, software em 720p30 | WebRTC da webview |
| macOS | ScreenCaptureKit | sim | VideoToolbox | WebRTC da webview |
| Linux | GStreamer: `ximagesrc` (X11) ou `pipewiresrc` pelo portal (Wayland) | monitor do PulseAudio/PipeWire | `nvh264enc`/`vah264enc`/`vaapih264enc`; sem placa, `x264enc` | receptor nativo: RTP puro → GStreamer → MJPEG |

No Linux, Debian, Ubuntu, Mint e Parrot compilam o WebKitGTK **sem WebRTC**, e nenhum pacote
muda isso. Por isso lá o app transmite e assiste por um caminho nativo em Rust.
`unkvoid-desktop --check` diz o que o motor da janela desta máquina sabe fazer, e
`unkvoid-desktop --check-capture` prova a captura e o encoder em três segundos, sem abrir
janela. O que está provado em hardware e o que só compila: [docs/ESTADO.md](docs/ESTADO.md).

## Instalar

- **Windows e macOS:** o instalador está em <https://unkvoid.com>. O app se atualiza sozinho.
- **Linux (Debian, Ubuntu e derivados):** pelo repositório APT, e a versão nova chega com o
  `apt upgrade`:

```bash
curl -fsSL https://unkvoid.com/apt/unkvoid.gpg | sudo tee /usr/share/keyrings/unkvoid.gpg > /dev/null
echo "deb [signed-by=/usr/share/keyrings/unkvoid.gpg] https://unkvoid.com/apt ./" | sudo tee /etc/apt/sources.list.d/unkvoid.list
sudo apt update && sudo apt install unkvoid
```

## Desenvolver

Rodar as três peças local, o que verificar antes de um PR e as regras de código estão no
[CONTRIBUTING.md](CONTRIBUTING.md). O caminho curto:

```bash
cd web && composer setup && composer dev                 # Laravel em :8000
cd web && php artisan reverb:start                       # Reverb em :8080
cd sfu && pnpm install && pnpm run build && SFU_SECRET=<o do web/.env> SFU_LARAVEL_URL=http://127.0.0.1:8000 node dist/server.js
cd native/apps/desktop && npm ci && VITE_SERVER=http://127.0.0.1:8000 npm run dev:app
```

Gerar instalador **não cross-compila**: cada um só sai no seu próprio sistema.
[Windows](docs/BUILD-WINDOWS.md) · [macOS](docs/BUILD-MACOS.md) · [Linux](docs/BUILD-LINUX.md) ·
[assinar e publicar uma versão](docs/AUTO-UPDATE.md).

## Documentação

O índice está em [docs/README.md](docs/README.md). Os que mais se abre:

| Arquivo | O que tem |
|---|---|
| [docs/ARQUITETURA.md](docs/ARQUITETURA.md) | o mapa: cada peça, como conversam, os fluxos, onde roda |
| [docs/CONTRATO.md](docs/CONTRATO.md) | o contrato entre as três peças: API, token, SFU, Reverb, comandos do Tauri |
| [docs/ESTADO.md](docs/ESTADO.md) | o que falta, o que nunca rodou em hardware e as perguntas abertas |
| [docs/DECISOES.md](docs/DECISOES.md) | o que foi decidido e por quê |
| [docs/SEGURANCA.md](docs/SEGURANCA.md) | o que é cifrado, o que está protegido e o que não está |

## Contribuir

Leia o [CONTRIBUTING.md](CONTRIBUTING.md) e o [Código de Conduta](CODE_OF_CONDUCT.md). Falha de
segurança não vai em issue pública: **contato@unkvoid.com**.

## Licença

[MIT](LICENSE).
