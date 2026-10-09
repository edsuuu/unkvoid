# O app nativo

Um núcleo em Rust que decide tudo, e uma interface por sistema que só desenha. Cada pasta de
sistema tem o seu `README.md` com o detalhe (`native/apps/windows`, `linux`, `macos`).

## As camadas

| Pasta | O que faz |
|---|---|
| `shared/capture/` | captura de tela, do som do sistema, do microfone e da câmera, um arquivo por sistema |
| `shared/media/` | encoder de hardware, Opus, envio RTP/SRTP (`plain.rs`, com pacer e governador de taxa), recepção com reenvio e pedido de quadro-chave (`receiver.rs`, `recovery.rs`), remontagem do quadro e do som (`unpack.rs`), buffer de chegada (`playout.rs`) e os decodificadores do Windows (`windows_decoder.rs`) e do Linux (`linux_decoder.rs`) |
| `shared/core/` | **as regras**: sessão e cliente do SFU, a sala viva (`room.rs`: publicar, assistir, microfone, câmera, os vigias da transmissão e do caminho de chegada), cliente da API do Laravel, código de sala, estado do app, motivos de erro, atualização, log do dia e a ABI C do macOS |
| `shared/storage/` | o estado em disco, com o token cifrado |
| `shared/clips/` | o replay instantâneo (Clips) do Windows |
| `apps/windows/` | Rust + Slint, usa o núcleo como crate |
| `apps/linux/` | Rust + GTK4, aposentado: o app Slint de `apps/windows/` é o do Linux também |
| `apps/macos/` | Swift + SwiftUI, fala com o núcleo pela ABI C |
| `apps/desktop/` | o app Tauri + React de antes: referência de comportamento, não é mais publicado |

**A regra:** regra de negócio não mora em pasta de sistema. O teste é "o Windows vai precisar
disto igual?" — se sim, sobe para o `shared/core`. Senão a mesma regra é escrita três vezes e
diverge no primeiro ajuste.

## O que é de cada sistema

| | Windows | Linux | macOS |
|---|---|---|---|
| Tela | Graphics Capture; Desktop Duplication no monitor do Windows 10 | `ximagesrc` (X11) ou `pipewiresrc` pelo portal (Wayland) | ScreenCaptureKit |
| Som do sistema | WASAPI por processo: só o jogo, sem o app de chamada | monitor do PulseAudio/PipeWire, sem o app de chamada | ScreenCaptureKit |
| Encoder | Media Foundation na placa; sem placa, CPU em 720p30 | `nvh264enc`/`vah264enc`/`vaapih264enc`; sem placa, `x264enc` em 720p30 | VideoToolbox |
| Microfone | WASAPI, pela interface | `pulsesrc`, pela captura | `AVAudioEngine` (com o cancelamento de eco do sistema) |
| Câmera | ainda não (`capture::captures_cameras` diz que não, e a tela apaga o botão) | `v4l2src`, a escolhida em Configurações → Voz e vídeo (o botão do app Slint a liga; no build de depuração, `UNKVOID_CAMERA_SOURCE` troca a webcam por qualquer origem do GStreamer, para testar sem uma). Volta sozinha depois de uma queda | AVFoundation, `IOSurface` sem cópia; o núcleo a reabre depois de uma queda |
| Assistir | Media Foundation na placa (DXVA), com reserva na CPU | `avdec_h264` dentro do processo (`gstreamer-rs`), no tamanho que veio | VideoToolbox (`AVSampleBufferDisplayLayer`) |
| Som de quem assiste | WASAPI; o som da tela segue a espera da imagem dela no `Playout`, fora de 40 ms de folga e com emendas sem estalo | `pacat`, depois do mesmo mixer, com a mesma espera | `AVAudioEngine` (o som da tela ainda não segue a espera da imagem: o `follows` não atravessa a ABI) |
| Atualização | pelo site, assinada (ver [AUTO-UPDATE.md](AUTO-UPDATE.md)), ou pela Microsoft Store | pelo APT | ainda sem versão publicada |

## A ponte do Swift

Só o macOS passa pela ABI C (`shared/core/src/ffi.rs`); Windows e Linux são Rust. As funções
estão em [ARQUITETURA.md](ARQUITETURA.md#a-ponte-para-o-swift), e as ações e avisos que passam
por elas no [CONTRATO.md](CONTRATO.md). Duas regras que não perdoam: `unkvoid_call` e
`unkvoid_app` **bloqueiam** (nunca na thread que desenha), e toda string devolvida volta em
`unkvoid_string_free`, uma vez só.

## O que fica no disco

| O quê | Onde |
|---|---|
| estado e preferências (`state.json`) | a pasta de configuração do sistema, em `com.unkvoid.desktop` |
| token da conta | cifrado em AES-256-GCM no mesmo arquivo; a chave fica no chaveiro do sistema (Keychain, Credential Manager, Secret Service) |
| log do dia | Windows: `%LOCALAPPDATA%\com.unkvoid.desktop\unkvoid-AAAA-MM-DD.log`; Linux: `~/.local/state/unkvoid/unkvoid-AAAA-MM-DD.log` (`$XDG_STATE_HOME`), a partir do `info`. Sete dias, e as linhas com `ERROR` vão para `POST /api/errors` a cada 30 s |
