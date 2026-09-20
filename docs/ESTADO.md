# Estado do projeto — o que falta e o que está em aberto

O que o projeto é e como instalar estão no [README](../README.md); o contrato entre as peças, no
[CONTRATO.md](CONTRATO.md); o porquê das decisões, no [DECISOES.md](DECISOES.md). Aqui fica só o
que ainda não foi feito: o que foi escrito sem rodar em hardware, o que falta construir e as
perguntas que esperam o dono. O histórico das sessões saiu do repositório e continua no git.

Última revisão: 20/09/2026 (versão 0.0.40).

## Escrito, mas nunca rodou em hardware

### Windows

- MFT de hardware que não é o primeiro da lista (placa integrada atrás da dedicada): não há
  máquina híbrida para provar.
- **Taxa que acompanha a perda** (0.0.38). Provado numa RTX 4060 Ti: o MFT da NVIDIA aceita
  trocar a taxa com o encoder no ar, nos dois sentidos, e leva de 6 a 8 s para chegar à taxa
  nova (daí a carência de 8 janelas do governador). **Não provado:** o governador de ponta a
  ponta com perda de verdade, e QuickSync, VCE e o MFT de software, que podem convergir em outro
  tempo. `broadcast_stats` mostra `targetBitrate` e `lossPermille`; `UNKVOID_ABR=off` desliga.
- **Falar-apertando por consulta de tecla** (0.0.38): compilado e testado com leitor de tecla
  injetado; ninguém apertou uma tecla de verdade com um jogo na frente. Com o jogo rodando como
  administrador o Windows esconde o estado das teclas de um app sem elevação.
- **Janela minimizada** (0.0.38): provado numa RTX 4060 Ti com uma janela comum — o encoder abre
  no tamanho para onde a janela volta (a área útil do monitor, se estava maximizada), e o
  primeiro quadro depois de restaurar chega nesse tamanho. **Não provado** com jogo em tela cheia
  exclusiva, em especial rodando em resolução menor que a da área de trabalho. Janela que fica
  minimizada mais de 30 s ainda cai pelo relógio de producer sem pacote do SFU.
- **Queda da mistura de áudio por processo para o `ExcludeSelf`** (0.0.38): a regra tem teste
  (cai na hora se não abre laço nenhum, ou depois de 5 varreduras cega), mas a queda em si nunca
  foi vista — não há como forçar a falha sem gancho de teste em produção. Teto conhecido: um
  processo que recusa o laço enquanto outro está aberto fica mudo na transmissão, sem queda.
  `OnlyProcess` (só o som da janela escolhida) continua saindo mudo se a janela recusar o laço.
- "Saída de áudio" (`setSinkId`) e o botão lateral do mouse na captura da tecla: nunca vistos no
  WebView2 de verdade. Arrastar imagem para o chat depende do `dragDropEnabled: false` do
  `tauri.conf.json`, que também nunca rodou instalado.

### macOS

- Encoder por software em 720p30 quando o VideoToolbox não usa a placa
  (`UsingHardwareAcceleratedVideoEncoder`).
- Largura da tela Retina em pontos ou pixels, e a pergunta de encoder de hardware feita antes
  do primeiro quadro.
- `set_bitrate` do VideoToolbox (`kVTCompressionPropertyKey_AverageBitRate`), escrito em
  19/09/2026 pela leitura do binding. Sem teto de rajada (`DataRateLimits` pede um `CFArray`).
- **Compila, e ninguém abriu.** Em 19/09/2026 o `release.yml` compilou e empacotou o `.app` e o
  `.dmg` num `macos-latest` do GitHub, sem publicar — é a primeira vez que o código do Mac
  compila desde a 0.0.29. Rodar, ninguém rodou.
- **Nenhuma versão do macOS está no manifesto de atualização** (conferido em 19/09/2026: o
  `latest.json` só tem as três chaves `windows-*`). Quem está no Mac não recebe nada.

### Linux

- Os encoders de placa (`nvh264enc`, `vah264enc`, `vaapih264enc`) nunca rodaram em hardware; só
  o degrau do x264 foi provado.
- **Captura no Wayland pelo portal** (0.0.38): tudo a partir do `create_session` nunca rodou —
  o seletor abrindo, o cancelar, o tamanho e o nó do PipeWire, o fd herdado pelo `gst-launch`
  como entrada padrão (`pipewiresrc fd=0`), o `keepalive-time` com tela parada, o fechamento da
  sessão e a troca de qualidade reaproveitando a sessão. Provado só o caminho de falha: sem
  portal, a negociação falha limpa em ~380 ms, com mensagem. O roteiro para provar está abaixo.
- **Nível do microfone (`voice:level`) e o silêncio mutado** (0.0.38): a conta e o portão têm
  teste; de ponta a ponta nunca rodou (não há microfone nem `gst-launch` na máquina de quem
  escreveu). É o que conserta o microfone que morria 30 s depois de entrar mutado.
- Correções de 15/09/2026 provadas só no SFU local e num contêiner com WebKitGTK: a transmissão
  que caía aos 30 s e a janela que congelava (MJPEG sem o delimitador final). Se voltar a
  congelar, pedir `top -H` nos processos do WebKit, `unkvoid-desktop 2>&1 | tee` e o
  `~/.local/state/unkvoid/unkvoid.log`.

<details>
<summary>Roteiro para provar o Wayland (GNOME ou KDE)</summary>

1. Instale `gstreamer1.0-pipewire`, `gstreamer1.0-tools`, `xdg-desktop-portal` e o backend do
   ambiente. `gst-inspect-1.0 pipewiresrc` responde e `echo $XDG_SESSION_TYPE` dá `wayland`.
2. `cd native && cargo run -p unkvoid-desktop -- --check-capture` com a tela parada: o seletor
   abre **uma** vez; escolhido um monitor, saem ~90 quadros e 3 keyframes ou mais em 3 s. Com 1
   ou 2 quadros, o `keepalive-time` não está funcionando.
3. Repita e cancele: `screen picker closed without choosing a source`, saída 1.
4. No app, dentro de uma voz, compartilhe. Com o seletor aberto, mute e desmute o microfone:
   responde na hora (o cadeado da sessão não está preso). Escolha uma janela em vez de monitor.
   Quem entra com a tela parada vê imagem em até 1 s.
5. Troque a qualidade no meio (720 → 1080): o seletor não abre de novo e a imagem muda.
6. Pare de compartilhar: o indicador do sistema some. Compartilhe de novo e dê `kill -9` no
   app: o indicador também some.
7. Com a transmissão no ar, `ls -l /proc/$(pgrep -f 'gst-launch.*DEFAULT_MONITOR')/fd`: o filho
   do áudio **não** pode ter o socket do PipeWire.
8. `UNKVOID_CAPTURE=x11` numa sessão Wayland e `UNKVOID_CAPTURE=portal` numa X11 do GNOME.
9. Voz, no modo detecção de voz: falar abre e calar fecha o "falando", sem o aviso "parou de
   medir"; entrar mutado, esperar 40 s e desmutar não dá 404 nem `producerDead`.

</details>

### As três peças

- **Interface nova da 0.0.38** (imagem no chat, chat da voz, volume por pessoa, saída de áudio,
  aviso "internet apertada"): passa nos testes unitários e na integração contra Laravel, Reverb,
  SFU e MinIO locais, mas **ninguém clicou** nela ainda, em sistema nenhum.
- A interface em TypeScript (14/09/2026) ainda não abriu no app de verdade nos três sistemas.
- Correções feitas pela leitura e pelo SFU, sem captura real (14/09/2026): compartilhar a tela
  pela segunda vez na mesma voz, a chave SRTP nova ao compartilhar de novo, e o socket de
  recepção do Linux depois do último producer assistido.
- **`can` novo na retomada** (0.0.38): 32 cenários no `check.mjs`, mas nenhum app de verdade
  retomou uma sessão com permissão reduzida.

## O que falta fazer

- **macOS sem release.** O bloqueio era de cobrança, e caiu quando o repositório ficou público:
  o `release.yml` voltou a rodar nos runners do GitHub (19/09/2026) e o macOS compila lá. Falta
  alguém abrir o app num Mac antes de publicar: `gh workflow run release.yml -f platform=macos
  -f publish=false` deixa o `.dmg` como artefato do run para testar.
- **Assinatura de código do Windows (SignPath Foundation).** Inscrição enviada em 19/09/2026, em
  análise. O que eles conferem já existe: a página `/code-signing-policy`, a seção no README, a
  licença MIT e a release saindo do `release.yml`. Quando aprovarem, falta o passo de assinatura
  no fluxo (a ordem está em [AUTO-UPDATE.md](AUTO-UPDATE.md#de-onde-sai-a-release)) e
  autenticação em dois fatores na conta do GitHub. Até lá o Windows avisa que o editor é
  desconhecido.
- **Borda amarela no Windows 10.** O Windows Graphics Capture só desliga a borda do Windows 11
  em diante; no 10 a saída é capturar o monitor pelo DXGI Desktop Duplication (sem borda, quadro
  na GPU, backend que a `windows-capture` já traz). Nada implementado: o porquê, os custos (só
  monitor, cursor desenhado à mão, acesso que cai) e as fases estão em
  [BORDA-AMARELA.md](BORDA-AMARELA.md).
- **Áudio do Discord vazando no Windows.** A exclusão é por nome de executável. Em 0.0.38
  entraram os clientes alternativos mais comuns (Vesktop, ArmCord/Legcord, WebCord). Continua sem
  conserto: o Discord aberto **no navegador** (entra pela árvore do Chrome, e nome não distingue),
  mixer que devolve o som na saída de verdade e não está em `RELAYS` (o Sonar da SteelSeries
  nunca foi medido com o GG aberto), e o microfone da voz pegando o Discord da caixa de som.
- **Linux, captura:** `cudaconvert`/`vapostproc` para tirar a conversão de cor da CPU; o app
  não percebe o "parar de compartilhar" do indicador do sistema (falta ouvir o fechamento da
  sessão do portal); o botão de compartilhar deveria travar enquanto o seletor está aberto; o
  seletor abre sem janela-pai e pode aparecer atrás do app; em monitor com escala o portal
  informa o tamanho lógico, e a qualidade fica limitada a ele.
- **Linux, assistir:** RTCP de volta no receptor (NACK e PLI). O mediasoup já aceita: o
  `consumePlain` usa as capacidades inteiras do router, então o reenvio vem por **RTX** (outro
  SSRC e outro payload), que o receptor não conhece. Falta escolher entre expor o RTX no
  `consumePlain` ou tirá-lo das capacidades do consumer plain (aí o reenvio vem no mesmo SSRC), e
  o `rtpjitterbuffer` precisa de folga de pelo menos uma ida e volta para o pacote reenviado
  ainda servir — latência a mais para quem assiste no Linux. E fMP4 por MSE no lugar do MJPEG.
- **Linux, voz:** volume e mudo por pessoa (a voz dos outros toca pelo Rust, fora do alcance do
  `audio.volume`); DTX do Opus desligado em `crates/media/src/audio.rs` embora o `plain.rs`
  anuncie `usedtx` — o silêncio mutado custa ~21 kb/s no fio em vez de ~1.
- **SFU, ingest puro atrás de NAT:** o `comedia` aprende o endereço no primeiro pacote e não
  reaprende; se o roteador trocar a porta no meio, a tela congela calada. **Um relógio no SFU não
  resolve:** tela parada não manda RTP, então "ficou N segundos sem pacote" mataria transmissão
  legítima. O caminho é o app notar que o caminho de volta morreu (nenhum SRTCP chegando enquanto
  ele manda) e republicar como já faz quando o SFU reinicia — e antes disso medir de quanto em
  quanto tempo o mediasoup manda relatório com a tela parada, para não republicar à toa.
- **Imagem no chat, o que ficou de fora:** mensagem direta (não há pivô; é migration); reservar a
  caixa da imagem pela proporção antes de baixar, para o chat não pular (exige `width`/`height`
  em `files`; é migration); e o lixo no bucket — ver as perguntas abertas.
- **Segurança:** token do app no cofre do sistema em vez do `localStorage`, PKCE no login com
  Google, e ponta a ponta nas mensagens diretas (decisão do dono: não agora). Os três estão em
  [SEGURANCA.md](SEGURANCA.md).
- **VPS:** remover os projetos antigos (backup em `/home/ubuntu/backups/2026-09-10`), depois de
  confirmada a lista.
- **O Ubuntu 24.04 não distribui o `webrtcdsp`**, que o `microphone_pipeline()` usa para cancelar
  eco, tratar ruído e ganho: sem ele o microfone do Linux vai cru (o Caso 4 de
  `native/tests/linux` avisa). O caminho de saída é o `module-echo-cancel aec_method=webrtc` do
  PulseAudio (ou o `libpipewire-module-echo-cancel`), mas carregar módulo no servidor de som da
  pessoa sem prova é risco maior que o buraco: precisa de uma máquina Linux com microfone real.
- **Microfone nativo no macOS e no Windows continua sendo o do webview**, de propósito: o
  libwebrtc de dentro do WebKit/WebView2 já traz cancelamento de eco, supressão de ruído e ganho.
- **Permissão de microfone no Windows** sem o balão de navegador (`--use-fake-ui-for-media-stream`):
  compilado e publicado desde a 0.0.30; falta alguém confirmar com os olhos.

## O laboratório de Linux

`native/tests/linux` é um Ubuntu 24.04 em contêiner com os mesmos pacotes que o `.deb` exige.
Seis casos de uso, um comando: `docker run --rm -v "$PWD/native:/unkvoid/native" unkvoid-linux
/unkvoid/native/tests/linux/cenarios.sh`. O Caso 1 é a resposta ao relato "aos 30 segundos a
transmissão cai": 45 s de captura, quadro em **todos** os segundos. O que ele **não** prova: a
janela do app (WebKitGTK), o receptor de MJPEG, o caminho até o SFU, e nada de Wayland (o
contêiner é Xvfb).

## Perguntas abertas para o dono

Cada uma com a recomendação de quem escreveu; nenhuma foi decidida.

1. **Tempo real de quem foi expulso ou banido:** o Reverb 1.11 não derruba a assinatura de quem
   já estava inscrito; um cliente modificado segue recebendo até reconectar. Fechar fazendo os
   eventos não levarem conteúdo (o app busca pela API, que confere a permissão)? Muda o contrato.
   *Recomendação: sim, mas só quando houver outro motivo para mexer nos eventos — o app oficial
   já sai dos canais, e o custo é um `GET` a mais por mensagem.*
2. **Apagar canal ou servidor** apaga em cascata `channel_accesses` e `channel_audits`: é o que
   se quer para a auditoria? E não derruba ninguém que está na voz. *Recomendação: manter a
   auditoria (tirar a cascata é migration) e chamar o `kick` do SFU ao apagar canal de voz.*
3. **Código e contrato divergem**, qual lado vale: `server_deaf` já tem escrita; apelido de
   outra pessoa exige `MANAGE_SERVER`; o dono cria cargo no topo; regenerar convite não emite
   `ServerUpdated`. *Recomendação: nos três primeiros vale o código (o contrato ficou para trás);
   no convite vale o contrato — quem tem a tela aberta fica com o código velho.*
4. **`MANAGE_CHANNELS`** renomeia e apaga canal que a pessoa não enxerga; cargo criado com topo
   ≤ 1 nasce na altura de quem criou; editar um cargo abaixo tira bits que quem edita não tem.
   *Recomendação: exigir `VIEW_CHANNEL` junto com `MANAGE_CHANNELS`, como o Discord; os outros
   dois são defeito e merecem teste negativo.*
5. **Clipes removidos** (18/09/2026): a tabela `clips` continua, e o que já estava em `clips/` no
   MinIO e em `/var/tmp` na VPS não foi apagado. Dropar a tabela e limpar os dois?
   *Recomendação: sim; é migration e é apagar dado, então só com o seu aval.*
6. **Microfone negado** hoje tira a pessoa da voz. Deixar entrar só ouvindo? *Recomendação:
   sim — é o que o Discord faz, e quem só quer assistir uma tela não precisa de microfone.*
7. **Estado em dobro** no `Hub` e no `Voice` (campos da classe e `Store`): hoje não
   dessincroniza. *Recomendação: na próxima vez que tocar ali, não agora.*
8. **Lixo no bucket** (novo): apagar canal, servidor ou conta derruba as mensagens por chave
   estrangeira, mas as imagens ficam no MinIO (e, para canal e servidor, as linhas de `files`
   ficam órfãs). Não é vazamento — o bucket é privado e sem linha não há URL —, é lixo
   acumulando; a foto de perfil já se comporta assim quando a conta é apagada. Limpar no
   `Channel::remove` e no `Server::delete`, ou um comando de varredura? *Recomendação: o comando
   de varredura, agendado, que cobre os três casos de uma vez.*
9. **`producerDead` de microfone e câmera revogados** (novo): hoje fecham calados para o dono,
   por causa do app antigo. Quando a 0.0.37 sair de circulação, passar a avisar com
   `reason: 'revoked'` como já acontece com a tela?
