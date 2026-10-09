# Unkvoid

Compartilhar a tela com quem você mandar o código, **sem perder fps no jogo** — e, para quem
tem conta, servidores com canais de texto e voz, câmera e chat, no molde do Discord.

Você abre o app, escreve seu nome e clica em **Criar uma sala**. Sai um código de 12
caracteres; quem cola o código entra e vê a tela de quem estiver transmitindo. O código **é** a
sala: não existe em banco nenhum e some quando o último sai.

## Por que um app, e não o navegador

No navegador o encoder de vídeo roda na CPU: transmitindo 1080p60 enquanto se joga, a CPU
disputa com o jogo e a transmissão cai para 1 fps. Aqui o caminho é outro:

```
captura → textura na GPU → encoder da placa de vídeo → 1 quadro → SFU → N espectadores
```

O quadro não passa pela CPU antes de ser codificado, é codificado **uma vez** e sobe **uma
vez** para o servidor, que replica. O upload de quem transmite não cresce com a plateia.

## As três peças

| Pasta | O quê | Onde roda |
|---|---|---|
| `native/` | o app: núcleo em Rust (captura, encoder, mídia, regras) e uma interface nativa por sistema — Slint no Windows, GTK4 no Linux, SwiftUI no macOS | na máquina de quem usa |
| `sfu/` | o relé de mídia (Node 22 + mediasoup) | na VPS |
| `web/` | site, contas, servidores, canais, chat, auditoria (Laravel 13 + Livewire 4 + Flux) | na VPS |

O app Tauri + React (`native/apps/desktop`) é o de antes dos nativos: continua no repositório
como referência de comportamento, mas não é mais publicado.

**Sala por código (sem conta):** o app fala só com o SFU. **Servidores (com conta):** o Laravel
decide quem pode o quê e assina um token de 60 s; o SFU só confere a assinatura; o app só esconde
botão. Os dois modos são produto: mexer num não degrada o outro.

Como as peças conversam: [docs/ARQUITETURA.md](docs/ARQUITETURA.md). Tudo o que atravessa a
rede: [docs/CONTRATO.md](docs/CONTRATO.md).

## Por sistema

| | Captura | Encoder | Assistir |
|---|---|---|---|
| Windows | Graphics Capture (Desktop Duplication no monitor do Windows 10, sem a borda amarela) | Media Foundation na placa (NVENC, QuickSync, AMF); sem placa, CPU em 720p30 | Media Foundation na placa (DXVA), com reserva na CPU |
| Linux | GStreamer: `ximagesrc` (X11) ou `pipewiresrc` pelo portal (Wayland) | `nvh264enc`, `vah264enc`, `vaapih264enc`; sem placa, `x264enc` em 720p30 | GStreamer, um processo por transmissão |
| macOS | ScreenCaptureKit | VideoToolbox | VideoToolbox |

Nos três a mídia é RTP puro cifrado (SRTP) direto com o SFU, com reenvio de pacote perdido,
pedido de quadro-chave, buffer de chegada e o caminho refeito sozinho quando a rede troca de
endereço. O que já rodou em hardware e o que falta: [docs/ESTADO.md](docs/ESTADO.md).

## Instalar

- **Windows:** o instalador está em <https://unkvoid.com>, e o app se atualiza sozinho.
- **Linux (Debian, Ubuntu e derivados):** pelo repositório APT; a versão nova chega com o
  `apt upgrade`.

```bash
curl -fsSL https://unkvoid.com/apt/unkvoid.gpg | sudo tee /usr/share/keyrings/unkvoid.gpg > /dev/null
echo "deb [signed-by=/usr/share/keyrings/unkvoid.gpg] https://unkvoid.com/apt ./" | sudo tee /etc/apt/sources.list.d/unkvoid.list
sudo apt update && sudo apt install unkvoid
```

- **macOS:** ainda sem versão publicada.

## Desenvolver

Rodar as três peças, o que verificar antes de um PR e as regras de código:
[CONTRIBUTING.md](CONTRIBUTING.md). O caminho curto:

```bash
cd web && composer setup && composer dev                 # Laravel em :8000
cd sfu && pnpm install && pnpm run build && SFU_SECRET=<o do web/.env> SFU_LARAVEL_URL=http://127.0.0.1:8000 node dist/server.js
cd native && UNKVOID_SERVER=http://127.0.0.1:8000 cargo run -p unkvoid-windows   # ou unkvoid-linux
cd native/apps/macos && ./run.sh                                                  # no Mac
```

Instalador não se cross-compila: cada um sai no seu sistema.
[Windows](docs/BUILD-WINDOWS.md) · [Linux](docs/BUILD-LINUX.md) · [macOS](docs/BUILD-MACOS.md) ·
[assinar e publicar](docs/AUTO-UPDATE.md).

## Documentação

O índice é o [docs/README.md](docs/README.md): um arquivo por pergunta.

## Contribuir

Leia o [CONTRIBUTING.md](CONTRIBUTING.md) e o [Código de Conduta](CODE_OF_CONDUCT.md). Falha de
segurança não vai em issue pública: **contato@unkvoid.com**.

## Licença

[MIT](LICENSE).
