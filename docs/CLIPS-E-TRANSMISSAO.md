# Clips com a tela no ar, e jogos em tela cheia

Dois assuntos do Windows que se encostam: o replay dos Clips gravando enquanto a tela é
transmitida, e a captura de jogo em tela cheia (o caso do Minecraft). O primeiro é desenho
decidido pelo dono em 13/10/2026 e ainda não feito; o segundo é só mapa, sem conserto escolhido.

## Clips enquanto a tela está no ar

### Hoje

Transmitindo e com o replay ligado, a máquina captura e codifica a tela **duas vezes**:

| | Captura | Encoder | Para onde vai |
|---|---|---|---|
| Transmissão | Graphics Capture (ou Desktop Duplication no Windows 10) | Media Foundation, GOP de 1 s, taxa que acompanha a perda | RTP para o SFU (`core/sharing.rs`) |
| Replay | Desktop Duplication do monitor escolhido (`clips/capture.rs`) | Media Foundation, GOP de 2 s, taxa fixa (`clips/encoder.rs`) | `RecordSink` do buffer em disco (`clips/replay.rs`) |

### O que o dono pediu

Transmitindo, o replay usa **o mesmo processo** da transmissão. Parou de transmitir, o replay
volta à captura dele.

### Como fica

1. Ao começar a transmitir, o `Recorder` solta a captura e o encoder dele e passa a receber os
   quadros **já codificados** da transmissão, por um segundo destino no ponto em que o
   `EncodedVideo` sai do encoder em `core/sharing.rs`. Nada é codificado de novo.
2. O som do replay continua o dele (`clips/audio.rs`): é barato, e o som da transmissão é outro
   (exclui o próprio app e os mixers de chamada).
3. Ao parar de transmitir, `Recorder::restart` religa a captura e o encoder do replay, como já
   faz quando a captura cai sozinha.

Os formatos já batem: o buffer guarda H.264 Annex-B com SPS/PPS na frente de cada IDR, que é o
que a transmissão produz, e o GOP de 1 s da transmissão cabe nos segmentos de 10 s do buffer,
que só fecham em quadro-chave.

### O que muda para quem usa (decidir antes de fazer)

- **A qualidade do clipe acompanha a transmissão.** Em upload fraco a transmissão baixa a taxa e
  até desce para 720p; o clipe desses minutos sai igual. Recomendação: aceitar e dizer isso na
  tela dos Clips enquanto a transmissão está no ar.
- **Transmitindo uma janela, o replay grava a janela**, e não o monitor escolhido nos Clips.
  Recomendação: aceitar, porque é o jogo que está sendo transmitido.
- **Clipe que atravessa a troca.** O MP4 do `clips/clip.rs` tem uma descrição de vídeo só (um
  SPS, uma largura e uma altura); antes e depois da troca a resolução e o encoder podem ser
  outros. Recomendação: o clipe começa no primeiro quadro-chave depois da última troca, e a tela
  avisa quando cortou.

### Onde mexer

- `core/sharing.rs`: o segundo destino do vídeo codificado, ligado e desligado sem parar a
  transmissão, com um quadro-chave pedido na hora em que liga.
- `clips/recorder.rs`: o modo "alimentado", sem `ScreenCapture`, e a volta ao modo normal.
- `clips/replay.rs`: marcar a troca no buffer, para o `prepare_save` saber onde cortar.
- O app novo liga os dois quando a transmissão começa e quando termina.

No Linux os Clips ainda não existem; quando existirem, o segundo destino é um `tee` no pipeline
do GStreamer.

## Jogos em tela cheia (Minecraft): o que se sabe

Só mapa. Nada aqui foi reproduzido de propósito.

### O que já aconteceu

- **10/10/2026, Clips no Windows 11:** a sessão do Graphics Capture aberta no login passou 8
  horas entregando só preto com o cursor, sem fechar, enquanto uma sessão nova no mesmo monitor
  via o Minecraft. Desde então o replay grava pelo Desktop Duplication também no Windows 11
  (commit `d4d6649`, [BORDA-AMARELA.md](BORDA-AMARELA.md)).
- O Desktop Duplication grava o Minecraft 26.3 **em janela** a 60 fps numa RTX 4060 Ti.
  **Ninguém salvou um clipe com o Minecraft em tela cheia**, nem com o app aberto pelo login.
- Transmissão: a janela minimizada é repetida a cada 1 s para o SFU não derrubá-la; **não
  provado com jogo em tela cheia exclusiva numa resolução menor que a da área de trabalho**
  ([ESTADO.md](ESTADO.md)).
- O painel do Alt+Z não abre sobre jogo em tela cheia exclusiva: salva direto e toca o bip
  (`apps/unkvoid/src/clips/mod.rs`, até o app novo).

### Hipóteses

| Hipótese | Sintoma esperado | Onde olhar |
|---|---|---|
| A tela cheia do Minecraft Java é exclusiva e troca o modo do monitor quando a resolução do jogo é outra | O Desktop Duplication devolve `DXGI_ERROR_ACCESS_LOST` a cada troca e reabre de 200 em 200 ms; o tamanho do quadro muda e o encoder é refeito | Log da captura: quantas vezes o acesso caiu e o encoder foi refeito |
| Em tela cheia exclusiva sem composição, a duplicação não recebe quadro nenhum | Clipe ou transmissão preta ou parada | Quadros por segundo no log, e o primeiro quadro salvo em BMP pelo teste ignorado `the_primary_monitor_is_duplicated_with_the_cursor` |
| A janela do jogo transmitida pelo Graphics Capture fica preta em tela cheia exclusiva | Quem assiste vê preto; com o monitor inteiro aparece | Transmitir a janela e o monitor, um de cada vez |
| Sessão do Graphics Capture velha (aberta muito antes do jogo) | Preto com cursor, sem erro | Como em 10/10: comparar com uma sessão aberta depois do jogo |

### Matriz de teste (quando o dono liberar, sem jogo aberto por outro motivo)

Minecraft Java 26.x e um jogo DirectX 11 de tela cheia exclusiva, cada um em: **janela**,
**tela cheia sem bordas** (mesma resolução da área de trabalho) e **tela cheia exclusiva numa
resolução menor**. Para cada um, em Windows 11 e Windows 10:

1. transmitir o monitor; 2. transmitir a janela do jogo; 3. salvar um clipe de 30 s;
4. alternar a tela cheia no meio (F11 no Minecraft).

Anotar: fps que chega do outro lado, se ficou preto, quantas vezes o acesso caiu, se o encoder
foi refeito e se o clipe toca do começo ao fim.

### Saídas possíveis, por sintoma (nenhuma escolhida)

- Acesso caindo sem parar em tela cheia exclusiva: cair para o Graphics Capture da janela do
  jogo enquanto ela estiver em tela cheia.
- Encoder refeito a cada troca de modo: escalar na placa para um tamanho de saída fixo (o
  `VideoProcessor` do Direct3D 11) e manter o encoder aberto.
- Preto no Graphics Capture: Desktop Duplication, como os Clips já fazem.
