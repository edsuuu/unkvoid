# Relatório do Stratus — o cliente nativo transmite e assiste, no Linux e no Windows

09/10/2026, sessão de nuvem (contêiner Ubuntu 24.04.5, 4 núcleos, sem placa de vídeo, sem
webcam, sem Windows). Base: `mac-dmg-sem-assinatura` com as quatro integrações
(`feat/discord-linux` #43, `fix/linux-tela` #44, `feat/discord-windows` #45,
`feat/discord-macos` #46) e `fix/sfu-install-espera-health` #48. O roteiro para quem for provar
numa máquina de verdade está em [../VERIFICAR-WINDOWS-LINUX.md](../VERIFICAR-WINDOWS-LINUX.md).

## O que foi testado, e como rodar

| O quê | Como | Resultado |
|---|---|---|
| Build, clippy `-D warnings` e testes do workspace no Linux | `cargo clippy --workspace --exclude unkvoid-desktop --all-targets -- -D warnings` e `cargo test --workspace --exclude unkvoid-desktop` | limpo; todos passam |
| Testes que pedem GStreamer e PulseAudio de verdade | `cargo test -p capture -p media -- --ignored` e `cargo test -p capture --lib linux_audio -- --ignored --test-threads=1` | todos passam |
| Código do Windows | `cargo clippy --target x86_64-pc-windows-gnu -p media -p capture -p core-app -p clips -p storage -p unkvoid-windows --all-targets -- -D warnings` (mingw-w64) | limpo. O `cargo-xwin` (alvo MSVC) não roda aqui: o proxy recusa o `aka.ms`, de onde ele baixa o SDK |
| Revisão do código do Windows (WGC, Desktop Duplication, Media Foundation, WASAPI) | leitura linha a linha, com um segundo par de olhos, cada achado conferido no código | 9 corrigidos, 3 anotados (abaixo) |
| Caminho de mídia sintético | testes do `media`: empacotar/desempacotar H.264, perda, NACK/RTX, PLI, jitter buffer, volta dos números; decodificador do Linux com quadros reais do `x264enc` | passam |
| De ponta a ponta contra o SFU do repo, dois clientes | `native/shared/core/tests/live_room.rs` (6 cenários) e `ponta-a-ponta.sh`, que monta Xvfb, PulseAudio, um vídeo de clarão + bipe e o SFU | os 6 passam, com e sem 3% de perda |
| Queda de rede de 10 s, de quem transmite e de quem assiste | `iptables` cortando só o usuário de um dos dois processos (no script) | a imagem volta sozinha de 1 a 3 s depois da rede, nos dois casos (no script, 11 de 11 segundos com imagem depois da volta) |
| O app Slint no Xvfb | `SLINT_BACKEND=winit-software` e femtovg (Mesa `llvmpipe`), conduzido por `xdotool` | entra pelo código, assiste (29–31 fps em release, ~10% de CPU na janela e ~10% decodificando), abre o seletor, transmite, para e sai; nenhuma trava da janela |

O script inteiro, numa máquina Linux com os pacotes do roteiro:

```bash
sudo native/shared/core/tests/ponta-a-ponta.sh     # SEM_REDE=1 sem root
```

Números do E2E (contêiner, sem placa, encoder x264 em 720p30):

| Medida | Sem perda | 3% de perda na chegada |
|---|---|---|
| primeira imagem de quem entra atrasado | 1,0 s | 1,0 s |
| imagens por segundo / maior parada | 30 / 46 a 79 ms | 27 a 30 / 40 a 141 ms |
| desvio A/V (imagem − som), mediana | 23 a 68 ms (varia com o PulseAudio de quem transmite) | 23 a 59 ms (era 85 ms antes da correção 9) |
| troca de 30 → 15 fps no ar | parada de 102 a 164 ms, relógio do RTP a 20–31 ms do real | — |
| parar e compartilhar de novo | a imagem volta em ~1 s | — |

## Bugs achados e corrigidos

Cada um com um teste que falhou no código de antes (rodado à parte, com a correção fora) e passa
com ela, salvo onde se diz o contrário.

| # | Onde | O que acontecia | Causa | Teste |
|---|---|---|---|---|
| 1 | `media/src/unpack.rs:62` | depois de perder o fim de um NAL fragmentado, o quadro-chave seguinte saía **como** quadro-chave mas com o IDR podre: imagem em lixo até o próximo periódico (macOS e Windows) | o depacotador da `rtc` guarda o FU-A até o pedaço final e não o larga num buraco | `a_lost_fragment_end_does_not_rot_the_next_keyframe` |
| 2 | `media/src/unpack.rs:133` | um STAP-A com o comprimento cortado (cliente modificado; o SFU repassa sem olhar) derrubava a thread de quem assiste: a tela daquela pessoa parava para sempre | a `rtc` lê o comprimento seguinte sem conferir que ele existe (`index out of bounds`) | `a_torn_aggregate_is_a_damaged_frame_and_not_a_panic` |
| 3 | `media/src/plain.rs:506` | com tela e câmera no ar, o pedido de reenvio da tela devolvia o pacote da câmera quando os números das duas se cruzavam (~3% do tempo), e o buraco da tela ficava | o histórico de reenvio era indexado só pelo número de sequência | `a_nack_resends_the_packet_of_the_stream_that_asked` |
| 4 | `media/src/plain.rs:410`, `:572` | build de depuração: transmissão derrubada por estouro depois de ~1 h a 10 Mb/s (14 min em 4K) — o `room` do exemplo e os testes longos | `+=` nos contadores de 32 bits do relatório do remetente, que a RFC 3550 manda dar a volta | `the_sender_report_counters_turn_over_instead_of_overflowing` |
| 5 | `media/src/linux_decoder.rs` (reescrito) | o Linux assistia em 1280x720 fixo com tarja (o detalhe de uma tela 1080p ia fora), e cada quadro devolvia a imagem do **anterior**: a última mudança de uma tela parada (a tecla, o slide) só aparecia no quadro seguinte — 1 s depois, vindo do Windows | `gst-launch` por cano, com tamanho fixo para achar o fim de cada quadro | `the_image_that_comes_back_is_the_frame_that_went_in` (o de antes devolveu `None` para o branco), mais tamanho fora da grade de 16, troca de resolução no meio e quadro pulado sem conversão (`DECODE_ONLY`) |
| 6 | `capture/src/linux.rs:965` | trocar a qualidade, descer um degrau ou refazer a captura travada recomeçava o relógio dos quadros do zero: o RTP de quem assiste andava um quadro só no lugar dos segundos da troca, e a imagem ficava esse tanto atrás do som até a espera do `Playout` descer (~10 s) | `started.elapsed()` por pipeline | `the_frame_clock_keeps_going_across_pipelines` e o E2E `a_quality_change_keeps_the_clock_and_the_picture` |
| 7 | `capture/src/linux_audio.rs:54`, `:103` | **compartilhar a tela com um jogo tocando derrubava o PulseAudio da máquina** (o som do computador inteiro), ao começar (`Assertion 'u->time_event == e'`) e ao parar (`Assertion 'size < (1024*1024*96)'`) — reproduzido só com `pactl` no PulseAudio 16.1 | o relógio de ajuste de taxa do `module-combine-sink` com um stream de outra taxa entrando, e o descarregamento do módulo movendo os streams sozinho | `stopping_the_share_with_a_game_playing_keeps_the_sound_server_alive` (caía na rodada 2; passa 6 rodadas). Correção: `adjust_time=0` (com recuo sem ele) e devolver os streams à saída padrão antes de descarregar |
| 8 | `core/src/watching.rs:103` | ligar o som de uma tela antes de ele chegar não valia (chegava mudo, com o botão mostrando ligado); depois de refazer o caminho (queda, troca de endereço) o som da tela voltava mudo; **ensurdecer e voltar desmutava quem a pessoa tinha mutado** | a escolha ficava só na rota, que é refeita, e voltar a ouvir aplicava a regra | `the_choice_to_hear_or_mute_a_sound_survives_the_route_being_rebuilt` — achado pelo E2E com perda |
| 9 | `core/src/watching.rs:57`, `apps/windows/src/sound.rs:83`, `apps/windows/src/watching.rs` | numa rede com perda a imagem esperava o jitter buffer e o som da tela não: imagem até meio segundo atrás do som (85 ms com 3% de perda) | o som tocava na chegada | o núcleo marca o som da tela com a tela que ele acompanha (`Media::follows`), e o `Speaker` o segura o tanto que o `Playout` segura a imagem. Testes `the_sound_follows_the_wait_of_the_picture_it_goes_with` e o E2E (23 a 59 ms com perda) |
| 10 | `core/src/room.rs:1036`, `:1341`, `:1354` | Linux: a webcam continuava filmando (luz acesa) depois de sair da sala, de ser expulso ou movido, e quando o servidor derrubava a câmera | `leave`, `stop_everything` e `died` não olhavam a câmera do Linux | sem teste automático: o pipeline da câmera não é observável de fora do `Room`; conferido pela leitura |
| 11 | `apps/windows/src/bridge.rs:2504` | o app Slint não ligava a câmera em sistema nenhum (o botão só reclamava), embora o núcleo do Linux a capture | o botão nunca foi ligado ao `open_captured_camera` | E2E `a_camera_is_watched_beside_the_screen` (pelo núcleo); o botão foi conferido no Xvfb |
| 12 | `capture/src/linux.rs:699` | a webcam 4:3 subia esticada em pixel retangular, e o decodificador do Windows (que ignora a proporção do pixel) mostrava o rosto largo | faltava `pixel-aspect-ratio=1/1` nas caps | `the_camera_keeps_square_pixels_at_the_card_size` |
| 13 | `core/examples/room.rs:106` | o medidor `room ... watch` morria (`attempt to subtract with overflow`) quando o caminho de chegada era refeito | a contagem recomeça no receptor novo | achado na queda de rede |

### Windows (compilado, **nunca rodado** — a prova está na seção 3.3 do roteiro)

| # | Onde | O que acontecia | Correção |
|---|---|---|---|
| W1 | `media/src/windows.rs:1064` | a vista de entrada do VideoProcessor ganhava uma referência por quadro e nunca a devolvia: a cada ponte refeita (janela que muda de tamanho, troca de qualidade) a textura de uma tela inteira ficava na placa | devolver a referência depois do blit, como o decodificador já fazia |
| W2 | `media/src/windows.rs:149` | o MFT da placa nunca recebia `Shutdown`: cada troca de qualidade ou refazer podia deixar uma sessão do NVENC aberta, até o encoder cair para o do processador | `MFShutdownObject` no `Drop` |
| W3 | `media/src/windows.rs:866` | `MF_E_TRANSFORM_STREAM_CHANGE` do encoder virava erro: no MFT que troca o tipo da saída no primeiro quadro, a placa nunca codificava | aceitar o tipo novo e seguir |
| W4 | `media/src/windows.rs:375`, `core/src/sharing.rs:590` | tela parada: o quadro acima do teto de fps era largado antes da ponte, e o último da rajada (a rolagem que parou) podia nunca chegar a quem assiste; e o encoder da placa segura um quadro na fila, então a última mudança só saía 1 s depois | o quadro atravessa a ponte mesmo sem ser codificado, e a última imagem se repete 100 ms depois que a captura cala (depois, uma vez por segundo, como antes). Teste `a_still_screen_repeats_the_last_image_soon_and_then_once_a_second` |
| W5 | `media/src/windows.rs:475` | a ponte que caía (prazo do keyed mutex) fazia a repetição da tela parada mandar uma tela preta | guardar a última imagem |
| W6 | `capture/src/windows_duplication.rs:292` | Windows 10, monitor de 60 Hz a 60 fps: o quadro que chegava um tico adiantado era largado — ~40 fps aos trancos (o mesmo bug que o `FramePacer` já tinha corrigido) | ¼ de quadro de folga; testes `a_jittery_60_hz_monitor_keeps_60_fps` e `a_144_hz_monitor_is_held_to_60_fps` (rodam só no Windows) |
| W7 | `capture/src/windows_duplication.rs:189` | o quadro pulado pelo ritmo era devolvido sem cópia, e a duplicação não o entrega de novo (o mesmo do W4, no Windows 10) | copiar sempre para a tela de trabalho e entregar quando a vez chegar |
| W8 | `capture/src/windows.rs:402` | o relógio do Desktop Duplication contava da abertura de cada captura (o bug 6, no Windows 10) e tremia com o desenho do cursor | o carimbo do QPC que a duplicação já calculava |
| W9 | `capture/src/windows_duplication.rs:278` | depois de a duplicação cair e voltar (troca de resolução), o cursor era desenhado no lugar antigo | recalcular a origem do monitor |

Anotados, sem correção (pedem máquina ou decisão):

- **Monitor em retrato no Windows 10**: a duplicação entrega a imagem sem girar (`DXGI_OUTDUPL_DESC.Rotation` não é lido) e o tamanho vem girado: imagem deitada e esticada. Corrigir pede `VideoProcessorSetStreamRotation` e a prova num monitor girado.
- **`SetCurrentLength` no buffer da textura de entrada** do encoder: o Chromium o faz por causa de MFTs da Qualcomm (ARM); não conferido aqui.
- **Mistura do som da tela inteira no Windows** (`windows_audio.rs`): o ritmo é o `Instant`, e o de cada processo é o do aparelho; em minutos uma faixa seca (estalo) ou acumula até ser cortada. Pede correção de deriva por faixa.

## O que mudou na ABI C (para o agente do macOS)

**Nada na assinatura nem no quadro de mídia.** O `unkvoid_next_media` continua com o mesmo
cabeçalho (`frame_of`): o campo novo `Media::follows` é só do Rust. Dois comportamentos mudam pelo
que já passa na ABI:

- `mute_watched` (`{"producerId", "muted"}`) agora vale como **escolha** da pessoa: sobrevive à
  rota refeita (queda, troca de endereço) e ao som que chega depois do clique, e voltar de surdo
  não desmuta quem ela mutou. O Swift não precisa fazer nada; se ele reaplicava o mudo depois de
  uma reconexão para contornar isso, pode parar.
- `room.failed` ganhou `camera`, mas só sai no Linux (o servidor derrubou a câmera capturada pelo
  núcleo). O macOS não recebe.

Para o macOS seguir a espera da imagem com o som da tela (o bug 9), o quadro da ABI teria de
levar o `follows`: fica para quando o agente do macOS quiser.

## O que só a máquina de verdade prova

- Tudo o que é Windows: WGC, Desktop Duplication, Media Foundation (encoder e decodificador na
  placa), WASAPI, e as correções W1–W9. O roteiro está na seção 3.3 do
  [VERIFICAR-WINDOWS-LINUX.md](../VERIFICAR-WINDOWS-LINUX.md).
- Linux: Wayland pelo portal; os encoders de placa (`nvh264enc`, `vah264enc`); o PipeWire no
  lugar do PulseAudio (a correção 7 tem recuo se o `pipewire-pulse` recusar `adjust_time`); uma
  webcam USB; 1080p60 e 4K decodificando numa CPU de verdade.
- A sincronia pelo ouvido e pelo olho, com alto-falante e monitor de verdade (o E2E mede até a
  chegada mais a espera, não a latência do aparelho de som nem a do monitor).
- Perda real de internet, troca de rede no meio e o roteador reiniciando.
- O renderizador por software do Slint desenha os avatares quadrados (o femtovg, redondos):
  cosmético, só onde não há OpenGL.

## Pendências fora do meu escopo

- O `release.yml` e o `build-windows.ps1` ainda compilam o Tauri (ver `CLAUDE.md`).
- O Nimbus ia montar um harness E2E do lado do servidor em `sfu/`; não estava na integração quando
  eu cheguei, e o `live_room.rs` + `ponta-a-ponta.sh` cobrem o lado do cliente.
