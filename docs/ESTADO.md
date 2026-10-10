# Estado do projeto — o que falta e o que está em aberto

Só o que ainda não foi feito: o que foi escrito sem rodar em hardware, o que falta construir e as
perguntas que esperam o dono. O que já existe está no [README](../README.md) e na
[ARQUITETURA.md](ARQUITETURA.md); o histórico está no git.

Última revisão: 03/10/2026 (versão 0.1.16).

## Escrito, mas nunca rodou em hardware

### Windows

- **Decodificar na placa (DXVA)**: provado numa RTX 4060 Ti (3,6 ms por quadro 1080p, contra 10,6
  ms na CPU, a mesma imagem nos dois caminhos). Não provado em Intel integrado nem em AMD; a
  reserva para a CPU (`UNKVOID_DECODER=cpu` força) é automática e vale para o resto do processo
  depois de uma falha.
- **Desktop Duplication no Windows 10** (a captura do monitor sem a borda amarela): falta a prova
  numa máquina com Windows 10 de verdade — ver [BORDA-AMARELA.md](BORDA-AMARELA.md).
- **Clips pelo Desktop Duplication no Windows 11**: a duplicação grava o Minecraft 26.3 em janela
  numa RTX 4060 Ti (60 fps, imagem certa). Ninguém salvou ainda um clipe com o Minecraft em tela
  cheia, nem com o app aberto pelo login do Windows, que é onde o Graphics Capture ficou preto.
- **Encoder que pendura ou que falha seguido**: a espera do MFT com prazo de 1 s, o
  `WAIT_TIMEOUT` do keyed mutex tratado como erro e a queda para o encoder de CPU depois de duas
  travadas foram escritos pela leitura; ninguém reproduziu um reset de driver (o caso suspeito é
  um jogo pesado abrindo, como o H1 do relato de 02/10).
- **Degrau de resolução em upload fraco** (perda que continua com a taxa no piso desce para 720p
  e 720p30): tem teste; nunca rodou com perda de verdade. O governador da taxa foi provado numa
  RTX 4060 Ti (o MFT da NVIDIA troca a taxa no ar, em 6 a 8 s); QuickSync, AMF e o MFT de software
  não.
- **Caminho morto refeito** (0.1.16: 5 s mandando sem RTCP do servidor, ou o servidor recebendo e
  nada chegando aqui): tem teste da regra; ninguém trocou de endereço no meio de uma transmissão de
  verdade (reconectar o PPPoE, reiniciar o roteador).
- **Som que reabre** quando o fone cai ou o padrão do Windows muda: compilado e testado; ninguém
  desplugou um headset com o app aberto.
- **Clips**: o motor e o painel do Alt+Z rodaram por dias como UnkvoidClips, o app separado, numa
  RTX 4060 Ti (0,1% de CPU, 0,4% do motor 3D da GPU, 6 MB/s de disco em Ultra). Dentro do
  Unkvoid, a aba, as Configurações, a bandeja, o player e o instalador removendo o UnkvoidClips só
  foram compilados e fotografados pela `vitrine`.
- **Pacote da Microsoft Store**: enviado (0.1.15.0) e em certificação desde 03/10/2026. Ninguém
  instalou pela Store: a `StartupTask`, o app sem elevação e a atualização pela Store não foram
  vistos.
- MFT de hardware que não é o primeiro da lista (placa integrada atrás da dedicada): não há
  máquina híbrida para provar.
- **Falar-apertando por consulta de tecla**: testado com leitor de tecla injetado; ninguém apertou
  com um jogo na frente. Com o jogo rodando como administrador, o Windows esconde as teclas de um
  app sem elevação.
- **Janela minimizada**: a imagem repetida a cada 1 s (0.1.16) impede o SFU de derrubá-la em
  30 s; não provado com jogo em tela cheia exclusiva em resolução menor que a da área de trabalho.
- **Queda da mistura de áudio por processo para o `ExcludeSelf`**: a regra tem teste, a queda
  nunca foi vista. Um processo que recusa o laço fica mudo na transmissão, sem queda; o
  `OnlyProcess` (só o som da janela escolhida) sai mudo se a janela recusar.

### Linux

- O jitter buffer, o redesenho por quadro do monitor e a pausa com a janela escondida (0.1.16)
  foram provados ponta a ponta no WSLg (Linux → VPS → Linux, 30 fps, 0 perdidos); não numa área de
  trabalho Linux de verdade, com placa de vídeo.
- Os encoders de placa (`nvh264enc`, `vah264enc`, `vaapih264enc`) nunca rodaram em hardware; só o
  x264 foi provado.
- **PLI → keyframe e taxa no ar** (09/10/2026): o vídeo da captura saiu do `gst-launch` e roda
  dentro do processo (`gstreamer-rs`). Provado no contêiner só com o `x264enc`: o keyframe pedido
  chega num GOP de 10 s, e a taxa cai tocando. Nos encoders de placa, a troca de taxa só vale se
  o `bitrate` deles for mutável tocando (o código confere a flag; se não for, o governador desiste
  e a taxa fica a de abertura, como antes). Na NVIDIA, conferir no log do `broadcast` a linha "a
  perda mudou a taxa do vídeo".
- **Captura no Wayland pelo portal**: tudo a partir do `create_session` nunca rodou (o seletor, o
  cancelar, o nó do PipeWire, o fd do portal entregue ao `pipewiresrc` dentro do processo, o
  `keepalive-time` com tela parada, o fechamento da sessão). Provado só o caminho de falha. O
  roteiro está abaixo.

- **O app Slint no Linux** (09/10/2026, `apps/windows`, o mesmo crate do Windows): compila,
  passa nos testes e abre sob `xvfb` no contêiner (`apps/windows/Dockerfile`), com a Archivo
  embutida. O que só um Linux de verdade prova:
  - a janela num Wayland real (o contêiner é X11 pelo Xvfb) e o OpenGL de uma placa (lá é o
    `llvmpipe` do Mesa);
  - o som: o `pacat` tocando na saída escolhida e o `pulsesrc` lendo o microfone escolhido
    (no contêiner não há PulseAudio de pé, e `pactl` responde vazio);
  - o portal do Wayland abrindo o seletor antes da captura — o `capture::prepare` que o
    `bridge.rs` precisa chamar (patch em `_relatorios/`, para o Sable aplicar);
  - o `.deb` do `build-deb.sh` instalando e abrindo num Debian 12 e num Ubuntu 24.04 de verdade;
  - assistir: o `media::H264Decoder` do Linux (`linux_decoder.rs`, com o Tux) ainda não existe —
    até lá o Linux entra na voz, fala e ouve, mas não vê a tela de ninguém.

<details>
<summary>Roteiro para provar o Wayland (GNOME ou KDE)</summary>

1. Instale `gstreamer1.0-pipewire`, `gstreamer1.0-tools`, `xdg-desktop-portal` e o backend do
   ambiente. `gst-inspect-1.0 pipewiresrc` responde e `echo $XDG_SESSION_TYPE` dá `wayland`.
2. `cd native && cargo test -p unkvoid-linux -- --ignored --nocapture` com a tela parada: o
   seletor abre **uma** vez; escolhido um monitor, saem quadros e keyframes em todos os segundos.
3. Repita e cancele: a falha é limpa, com mensagem.
4. No app, dentro de uma voz, compartilhe. Com o seletor aberto, mute e desmute o microfone:
   responde na hora. Escolha uma janela em vez de monitor. Quem entra com a tela parada vê
   imagem em até 1 s.
5. Troque a qualidade no meio (720 → 1080): o seletor não abre de novo e a imagem muda.
6. Pare de compartilhar: o indicador do sistema some. Compartilhe de novo e dê `kill -9` no app:
   o indicador também some.
7. Com a transmissão no ar, `ls -l /proc/$(pgrep -f 'gst-launch.*monitor')/fd`: o filho do áudio
   **não** pode ter o socket do PipeWire.
8. Com um app de chamada tocando e um jogo com som: `pactl list sinks short` mostra
   `unkvoid_share` enquanto a transmissão está no ar; quem assiste ouve o jogo e não a chamada.
   Ao parar, o sink some.
9. `UNKVOID_CAPTURE=x11` numa sessão Wayland e `UNKVOID_CAPTURE=portal` numa X11 do GNOME. E
   com `PIPEWIRE_NODE=<outro nó>` no ambiente, a tela que sobe continua sendo a escolhida no
   seletor: o pipeline agora roda dentro do processo, e não dá mais para tirar a variável só dele.

</details>

### macOS

- **O app nativo nunca foi publicado.** Ele roda e passa nos testes num Mac (21/09/2026), mas não
  há `.dmg` nem release (os PRs #30 e #31, toques e `.dmg` sem assinatura, estão abertos).
- Encoder por software em 720p30 quando o VideoToolbox não usa a placa
  (`UsingHardwareAcceleratedVideoEncoder`), a largura da tela Retina em pontos ou pixels, e o
  `set_bitrate` do VideoToolbox (sem teto de rajada).

### As três peças

- **`can` novo na retomada**: nenhum app de verdade retomou uma sessão com permissão reduzida.
- **Reconexão sem desistir** (0.1.16): a rede fora do ar por minutos não tira mais a pessoa da
  sala; provado só com o SFU reiniciando.

## O que falta fazer

- **macOS:** o `.dmg`, a publicação de `darwin-aarch64` e a atualização sozinha.
- **Assinatura de código:** nenhum instalador leva assinatura de editor, e o Windows avisa que o
  editor é desconhecido. Decidido não pagar certificado por ora (28/09/2026); a SignPath reprovou o
  projeto. A saída grátis no Windows é a Microsoft Store, que assina o pacote. Quando houver
  certificado, o passo entra entre o build e a publicação ([AUTO-UPDATE.md](AUTO-UPDATE.md)).
- **Microfone sem tratamento no Windows e no Linux:** sem cancelamento de eco, supressão de ruído
  nem ganho automático (o macOS usa o do sistema). No Windows o `nnnoiseless` já é dependência dos
  Clips; no Linux o `webrtcdsp` roda com `echo-cancel=false` (o eco exige a sonda no mesmo processo
  que toca o som), e o Ubuntu 24.04 nem distribui o `webrtcdsp`.
- **Windows:** a câmera; o Discord aberto **no navegador** e mixers fora de `RELAYS` continuam
  vazando som na transmissão (a exclusão é por nome de executável); o `logbook.rs` do app ainda é
  próprio, enquanto o Linux já usa o `core_app::logbook` (unificar).
- **Linux, captura:** a conversão de cor na CPU (`videoconvert`; o caminho é `cudaconvert` ou
  `vapostproc`); o app não percebe o "parar de compartilhar" do indicador do sistema; o seletor do
  portal pode aparecer atrás do app; em monitor com escala o portal informa o tamanho lógico.
- **Linux, assistir:** o decodificador é o de software (`avdec_h264`) e o cartão é 720p fixo.
- **Linux, voz:** detecção de voz e falar apertando; volume e mudo por pessoa; o DTX do Opus está
  desligado em `shared/media/src/audio.rs` embora o `plain.rs` anuncie `usedtx` — o silêncio mutado
  custa ~21 kb/s no fio em vez de ~1.
- **Histórico de reenvio em 4K:** 1024 pacotes cobrem só ~0,24 s a 40 Mb/s; aumentar exige um fluxo
  RTX próprio de quem transmite, que muda o SFU.
- **Imagem no chat, o que ficou de fora:** mensagem direta com imagem (não há pivô; é migration);
  reservar a caixa da imagem pela proporção (exige `width`/`height` em `files`; é migration); e o
  lixo no bucket (pergunta aberta 7).
- **Segurança:** PKCE no login com Google e ponta a ponta nas mensagens diretas (decisão do dono:
  não agora). Ver [SEGURANCA.md](SEGURANCA.md).
- **VPS:** remover os projetos antigos (backup em `/home/ubuntu/backups/2026-09-10`), depois de
  confirmada a lista.

## Perguntas abertas para o dono

Cada uma com a recomendação de quem escreveu; nenhuma foi decidida.

1. **Tempo real de quem foi expulso ou banido:** o SFU não derruba a inscrição de quem já estava
   inscrito; um cliente modificado segue recebendo até reconectar. Fechar fazendo os eventos não
   levarem conteúdo (o app busca pela API, que confere a permissão)? Muda o contrato.
   *Recomendação: sim, mas só quando houver outro motivo para mexer nos eventos.*
2. **Apagar canal ou servidor** apaga em cascata `channel_accesses` e `channel_audits`: é o que se
   quer para a auditoria? E não derruba ninguém que está na voz. *Recomendação: manter a auditoria
   (tirar a cascata é migration) e chamar o `kick` do SFU ao apagar canal de voz.*
3. **Código e contrato divergem**, qual lado vale: `server_deaf` já tem escrita; apelido de outra
   pessoa exige `MANAGE_SERVER`; o dono cria cargo no topo; regenerar convite não emite
   `ServerUpdated`. *Recomendação: nos três primeiros vale o código; no convite vale o contrato.*
4. **`MANAGE_CHANNELS`** renomeia e apaga canal que a pessoa não enxerga; cargo criado com topo ≤ 1
   nasce na altura de quem criou; editar um cargo abaixo tira bits que quem edita não tem.
   *Recomendação: exigir `VIEW_CHANNEL` junto com `MANAGE_CHANNELS`, como o Discord; os outros dois
   são defeito e merecem teste negativo.*
5. **Clipes removidos do servidor** (18/09/2026): a tabela `clips` continua, e o que estava em
   `clips/` no MinIO e em `/var/tmp` na VPS não foi apagado. *Recomendação: dropar e limpar; é
   migration e é apagar dado, então só com o seu aval.*
6. **Microfone negado** hoje tira a pessoa da voz. Deixar entrar só ouvindo? *Recomendação: sim —
   é o que o Discord faz.*
7. **Lixo no bucket:** apagar canal, servidor ou conta derruba as mensagens, mas as imagens ficam no
   MinIO (e, para canal e servidor, as linhas de `files` ficam órfãs). *Recomendação: um comando de
   varredura agendado, que cobre os três casos.*
8. **`producerDead` de microfone e câmera revogados**: hoje fecham calados para o dono, por causa do
   app antigo. Passar a avisar com `reason: 'revoked'`, como a tela?
9. **A mesma frase escrita três vezes:** cada interface traduz os motivos do núcleo nas mesmas
   frases em português (`unreachable` → "Não deu para falar com o servidor"). Foi desenho (o núcleo
   dá o motivo, a interface escreve), e o `match` exaustivo faz motivo novo virar erro de compilação
   nas interfaces em Rust; mas se o produto for só em português, a frase caberia no núcleo.
