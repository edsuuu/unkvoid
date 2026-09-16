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
- **Produção atrás da branch:** o `unkvoid.com` roda o site do `main`. O app desta branch exige
  o `state` na volta do login do Google (sem ele, 404) e usa `/api/config` e `/api/servers`, que
  ainda não existem lá. Sobem o site e o SFU; o app não sobe.
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
- **Fase 2 do app** (precisa de tabela nova): mensagens diretas, amigos, foto e ícone, imagem no
  chat, API da auditoria, não lidas, chat dentro da voz e indicador de fala.

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
   - clipe em canal de texto oculto dá 403, e não 422;
   - regenerar convite não emite `ServerUpdated`.
4. **`MANAGE_CHANNELS`** renomeia e apaga canal que a pessoa não enxerga; cargo criado com topo
   ≤ 1 nasce na altura de quem criou; editar um cargo abaixo tira bits que quem edita não tem.
5. **SFU:** a retomada da sessão (30 s) ignora o `can` do token novo; tela em VP8 não é gravada
   (o contrato não diz que é só H.264).
6. **Clipes:**
   - `schedule:run` no cron da VPS, para os clipes vencidos serem apagados sem depender de um
     clipe novo?
   - limite de pedidos no `POST` de clipe, para proteger o `ffmpeg` do SFU?
   - apagar do MinIO os clipes de uma conta apagada?
   - carregar o hls.js só ao abrir o player?
7. **Microfone negado** hoje tira a pessoa da voz. Deixar entrar só ouvindo?
8. **Estado em dobro** no `Hub` e no `Voice` (campos da classe e `Store`): hoje não dessincroniza.
   Mexer agora ou na próxima vez que tocar ali?
