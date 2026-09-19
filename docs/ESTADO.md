# Estado do projeto — o que falta e o que está em aberto

O que o projeto é, o estado por sistema e como buildar estão no [README](../README.md); o
contrato entre as peças, em [SERVIDORES.md](SERVIDORES.md). Aqui fica só o que ainda não foi
feito: o que foi escrito sem rodar em hardware, o que falta construir e as perguntas que esperam
o dono. O histórico das sessões saiu do repositório e continua no git.

## Escrito, mas nunca rodou em hardware

- **Windows**
  - MFT de hardware que não é o primeiro da lista (placa integrada atrás da dedicada): não há
    máquina híbrida para provar.
  - Janela de jogo minimizada abre o encoder em ~160×28 (`GetWindowRect` de janela minimizada).
  - A mistura de áudio por processo não volta ao `ExcludeSelf` se falhar.
- **macOS**
  - Encoder por software em 720p30 quando o VideoToolbox não usa a placa
    (`UsingHardwareAcceleratedVideoEncoder`).
  - Largura da tela Retina em pontos ou pixels, e a pergunta de encoder de hardware feita antes
    do primeiro quadro.
- **Linux:** os encoders de placa (`nvh264enc`, `vah264enc`, `vaapih264enc`) nunca rodaram em
  hardware; só o degrau do x264 foi provado.
- **Correções feitas pela leitura e pelo SFU, sem captura real** (revisão de 14/09/2026):
  compartilhar a tela pela segunda vez na mesma voz (Windows e macOS), a chave SRTP nova ao
  compartilhar de novo, e o socket de recepção do Linux depois do último producer assistido.
- **A interface em TypeScript** (14/09/2026) passou nos testes, no navegador e na integração,
  mas ainda não abriu no app de verdade nos três sistemas.
- **Linux, correções de 15/09/2026 provadas só no SFU local e num contêiner com WebKitGTK:**
  - a transmissão que caía aos 30 s: o app declarava o áudio da tela mesmo sem áudio saindo, o
    SFU matava esse producer e o app derrubava a tela junto;
  - a janela que congelava: o MJPEG fechava sem o delimitador final (WebKitGTK em laço de CPU),
    comandos lentos rodavam na thread do GTK e o fechar esperava a sessão para sempre.
  Se voltar a congelar, pedir `top -H` nos processos do WebKit, `unkvoid-desktop 2>&1 | tee` e o
  `~/.local/state/unkvoid/unkvoid.log`.

## O que falta fazer

- **A 0.0.29 saiu sem Windows** (16/09/2026). O macOS foi compilado e publicado à mão daqui
  (`darwin-aarch64` + `darwin-aarch64-dmg`, assinados com `9a18c9243ef59b08` e conferidos pelo
  `check-signature.mjs`), e o `.deb` amd64 saiu da VPS pelo fluxo `build-linux`. O Windows
  **não**: enquanto ninguém compilar numa máquina Windows, o `latest.json` da 0.0.29 não tem as
  chaves `windows-*`, e quem está no Windows continua na 0.0.28 — que segue funcionando, porque
  tudo que subiu na API é acréscimo. Quem compilar lá precisa publicar `windows-x86_64-nsis` e
  `windows-x86_64-msi` com o `publish-release.sh`.
- **O código de Windows nunca foi compilado nesta integração** (16/09/2026): a `main` trouxe as
  correções 0.0.25 a 0.0.27 — encoder, captura e áudio de Windows, mais o limitador de quadros
  para monitor de 144 Hz — e elas foram integradas num Mac, onde esse código nem entra na
  compilação. Antes de publicar qualquer instalador, compilar e transmitir numa máquina Windows.
- **O CI não constrói release: a conta do GitHub está bloqueada** (16/09/2026). O `release.yml` é o
  único fluxo em runner do GitHub (`macos-latest` e `windows-latest`), e o repositório é privado,
  então esses minutos são cobrados — o disparo morreu com *"recent account payments have failed or
  your spending limit needs to be increased"*. Os outros três fluxos rodam no runner da própria VPS
  e por isso continuam funcionando. A 0.0.28 do macOS foi compilada e publicada à mão, de um Mac,
  com a chave `9a18c9243ef59b08` e conferida pelo `check-signature.mjs`. Enquanto a cobrança não
  for resolvida, release é trabalho manual em cada sistema.
- **Áudio do Discord vazando no Windows** (relato do dono, 16/09/2026): a exclusão é por nome de
  executável (`Discord.exe` e companhia) e não há queda para o laço clássico, então as duas
  hipóteses são a caixa "Sem o áudio do Discord" desmarcada ou o Discord entrando na chamada
  depois do início da transmissão — a lista de processos é refeita a cada 2 s. Falta a linha
  `broadcast.start` do log da máquina, que diz qual das duas é.
  Conferido em 16/09/2026 na máquina Windows do dono: a única transmissão do log (0.0.28, 5 s)
  saiu com `muteCalls: true` e `scope=ExceptMuted`, e a mistura pegou WebView2 do WhatsApp,
  Steam, Spotify e Chrome — Discord nenhum. Medido também que o Windows corta a árvore no
  intermediário morto, igual ao `valid_parent`: o `Update.exe` que abre o Discord e sai não
  deixa o Explorer trazer o Discord junto. Sobram três caminhos, nenhum reproduzido: Discord
  aberto no navegador ou em cliente de outro nome (Vesktop), que entra com a árvore do Chrome;
  mixer que devolve o som na saída de verdade e não está em `RELAYS` (o Sonar da SteelSeries
  está instalado nessa máquina, e não foi medido com o GG aberto); e o microfone da voz
  pegando o Discord da caixa de som, que a caixa do compartilhamento não alcança.
- ~~**Produção atrás da branch**~~ — resolvido em 16/09/2026: a branch foi para a `main`
  (PR #8), os três fluxos da VPS rodaram (`deploy-web` com as migrations, `deploy-sfu` e
  `build-linux`), e a produção responde `/api/dm`, `/api/friends` e `/api/servers/{id}/audits`.
- **Linux:** Wayland (`pipewiresrc` pelo portal), lista de janelas e VA-API na captura; RTCP de
  volta (NACK/PLI) e fMP4 por MSE para aliviar a CPU do MJPEG.
- **SFU, ingest puro atrás de NAT:** o `comedia` aprende o endereço no primeiro pacote e não
  reaprende; se o roteador trocar a porta no meio, a tela congela calada (o producer já teve
  score e o timer de 30 s não dispara).
- **VPS:** remover os projetos antigos (backup em `/home/ubuntu/backups/2026-09-10`), depois de
  confirmada a lista.
- **Histórico do git:** 77 mensagens de commit com linha de co-autor. A reescrita foi aprovada e
  não foi executada; exige force-push e re-clone.
- **Documentação velha:** `BUILD-MACOS.md` e `AUTO-UPDATE.md` ainda citam `discord.unkvoid.com` e
  a publicação pelo GitHub Releases.
- **Fase 2 do app** — entregue em 16/09/2026, menos: imagem no chat e chat dentro da voz.
  Mensagens diretas, amigos, ícone do servidor, API da auditoria, não lidas e foto de perfil
  estão de pé.
- **Imagem no chat** (17/09/2026): a tabela `files` e o `POST /api/me/avatar` já deixaram o
  caminho de upload pronto. Falta a pivô que liga mensagem e arquivo (mais de uma imagem por
  mensagem) — é migration, e o esquema é decisão do dono.

### Achados da noite de 16/09/2026

- **O Ubuntu 24.04 não distribui o `webrtcdsp`.** É o elemento que o `microphone_pipeline()` usa
  para cancelar eco, tratar ruído e ganho no Linux; sem ele o microfone vai **cru**. Provado nos
  dois lados: `dpkg -L gstreamer1.0-plugins-bad | grep webrtcdsp` não devolve nada em arm64 nem em
  amd64, e `gst-inspect-1.0 webrtcdsp` responde *No such element*. O Caso 4 de
  `native/tests/linux` avisa disso em qualquer máquina.
  Caminho de saída, na ordem: o próprio PulseAudio tem `module-echo-cancel aec_method=webrtc`, que
  **carrega** no contêiner e cria a fonte tratada — mas não consegui provar que ela entrega áudio
  sem um microfone de verdade, e carregar módulo no servidor de som da pessoa sem prova é risco
  maior que o buraco. Precisa de uma máquina Linux com microfone real: carregar o módulo, ler
  `pulsesrc device=unkvoid_mic`, e só então ligar no `microphone_pipeline()` com descarga no
  `stop`. O PipeWire tem o equivalente (`libpipewire-module-echo-cancel`).
- **Microfone nativo no macOS e no Windows continua sendo o do webview**, de propósito: ali quem
  captura é o libwebrtc compilado dentro do WebKit/WebView2, que **já traz** cancelamento de eco,
  supressão de ruído e ganho automático. Trocar por uma captura em Rust sem ligar uma APM de
  verdade pioraria o áudio. `start_voice`/`stop_voice` (o caminho em Rust) segue só no Linux.
- **A detecção de voz não mede nível no caminho nativo do Linux**: o áudio não passa pela janela,
  então o `AnalyserNode` não vê nada. Lá valem *apertar para falar* e *sempre aberto*; a interface
  diz isso em texto. Medir no Linux exige um elemento `level` na pipeline e um evento novo até a
  interface.
- **Permissão de microfone no Windows** deixou de mostrar o balão de navegador por configuração
  (`additionalBrowserArgs` com `--use-fake-ui-for-media-stream`), e não por código: quem decide
  passa a ser a privacidade do Windows. **Não foi compilado no Windows** — conferir no primeiro
  build lá.
- **Criptografia ponta a ponta: não agora** (decisão do dono, 16/09/2026). O desenho proposto,
  para quando for a hora: E2E só em mensagem direta, chave por par (X25519 no cadastro, pública no
  `users`, privada guardada na máquina), corpo cifrado no `direct_messages.body` e o servidor
  guardando opaco. Não vale para canal de servidor (a auditoria e a moderação precisam ler).
  Mídia seria outro projeto: o SFU roteia sem ver a imagem (o cabeçalho RTP fica aberto, como
  no DAVE do Discord), e com os clipes fora do código nada no servidor precisa mais decodificar.

## O laboratório de Linux

`native/tests/linux` (16/09/2026) é um Ubuntu 24.04 em contêiner com os mesmos pacotes que o
`.deb` exige. Seis casos de uso, um comando: `docker run --rm -v "$PWD/native:/unkvoid/native"
unkvoid-linux /unkvoid/native/tests/linux/cenarios.sh`. O Caso 1 é a resposta ao relato "aos 30
segundos a transmissão cai": 45 s de captura, quadro em **todos** os segundos, 15 deles depois do
minuto crítico. O que ele **não** prova: a janela do app (WebKitGTK), o receptor de MJPEG e o
caminho até o SFU.

## Perguntas abertas para o dono

1. **Tempo real de quem foi expulso ou banido:** o Reverb 1.11 não derruba a assinatura de quem
   já estava inscrito. O app oficial sai dos canais no `MemberRemoved`; um cliente modificado
   segue recebendo até reconectar. Fechar isso fazendo os eventos não levarem conteúdo (o app
   busca pela API, que confere a permissão)? Muda o contrato.
2. **Apagar canal ou servidor** apaga em cascata os acessos à voz (`channel_accesses`) e o
   histórico (`channel_audits`): é o que se quer para a auditoria? (Mexer é migration.) E não
   derruba ninguém que está na voz.
3. **Código e `SERVIDORES.md` divergem**, qual lado vale:
   - `server_deaf` já tem escrita;
   - apelido de outra pessoa exige `MANAGE_SERVER`;
   - o dono cria cargo no topo;
   - regenerar convite não emite `ServerUpdated`.
4. **`MANAGE_CHANNELS`** renomeia e apaga canal que a pessoa não enxerga; cargo criado com topo
   ≤ 1 nasce na altura de quem criou; editar um cargo abaixo tira bits que quem edita não tem.
5. **SFU:** a retomada da sessão (30 s) ignora o `can` do token novo.
6. **Clipes removidos** (18/09/2026): o código saiu das três peças, mas a tabela `clips`
   continua (migration é decisão do dono), e o que já estava em `clips/` no MinIO e em
   `/var/tmp` na VPS (o anel) não foi apagado. Dropar a tabela e limpar os dois?
7. **Microfone negado** hoje tira a pessoa da voz. Deixar entrar só ouvindo?
8. **Estado em dobro** no `Hub` e no `Voice` (campos da classe e `Store`): hoje não dessincroniza.
   Mexer agora ou na próxima vez que tocar ali?
