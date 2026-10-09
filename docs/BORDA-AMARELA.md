# A borda amarela no Windows 10

Quem transmite num **Windows 10** via uma borda amarela em volta da tela ou da janela capturada,
na transmissão e nos clipes. No Windows 11 ela não aparece. Este arquivo diz por quê e o que foi
feito: levantado em 20/09/2026, **implementado em 01/10/2026** para monitor inteiro (fases 1 e
2 abaixo). Falta a prova numa máquina com Windows 10 de verdade.

## Por que ela aparece

A captura do Windows é o Windows Graphics Capture (`shared/capture/src/windows.rs`, pela crate
`windows-capture`). É o sistema que desenha a borda, como aviso de privacidade, e não o app.

O app já pede para desligá-la: `supported_settings` escolhe `DrawBorderSettings::WithoutBorder`
sempre que `GraphicsCaptureApi::is_border_settings_supported()` diz que pode. A opção por trás
disso (`GraphicsCaptureSession.IsBorderRequired`) só existe do build **20348** em diante —
Windows 11 e Server 2022. O Windows 10 de consumo parou no 19045 (22H2): ali a opção não existe,
e **nenhuma configuração do Windows Graphics Capture tira a borda**. A crate recusa a captura
inteira quando recebe uma chave que o sistema não tem, por isso o app cai no padrão e a borda
fica.

Não é bug nosso e não tem conserto dentro desta API.

## A saída: outro caminho de captura no Windows 10

O **DXGI Desktop Duplication** (`IDXGIOutputDuplication`, o que o OBS usa como "captura de
monitor") existe desde o Windows 8 e não desenha borda nenhuma.

Ele respeita a regra que manda no projeto — `captura → textura na GPU → encoder de hardware`:
o quadro chega como `ID3D11Texture2D` e não desce para a memória do processador.

E não pede dependência nova: a `windows-capture` 2.0.1, que o app já compila, traz o backend
`dxgi_duplication_api::DxgiDuplicationApi`, com `acquire_next_frame`, `texture()`, `device()` e
`frame_info()`.

## O que custa

| Custo | O que quer dizer |
|---|---|
| **Só monitor inteiro** | Desktop Duplication não captura janela. Compartilhar **uma janela** no Windows 10 continua no Windows Graphics Capture, **com a borda** |
| **O cursor não vem na imagem** | A API entrega o ponteiro à parte. Ele é desenhado pelo GDI do próprio Windows (`DrawIconEx`) numa cópia do quadro que continua na GPU (textura `GDI_COMPATIBLE`): sai o mesmo desenho da tela, inclusive o cursor de texto que inverte o fundo, sem decifrar os três formatos de ponteiro. Jogo que esconde o cursor do Windows e desenha o próprio continua igual: o Windows diz que não há cursor, e nada é desenhado |
| **O acesso cai** | Troca de resolução, tela cheia exclusiva, UAC e bloqueio de tela devolvem `DXGI_ERROR_ACCESS_LOST`. A duplicação é reaberta sozinha, de 200 em 200 ms, sem derrubar a transmissão; volta com outro device do Direct3D, e as duas pontes de encoder (`media/src/windows.rs` e `clips/src/encoder.rs`) se refazem quando o device da captura muda |
| **Duas placas de vídeo** | Em notebook híbrido a duplicação pode recusar o monitor da outra placa. Aí a captura volta ao Windows Graphics Capture, **com a borda** — mas transmite |
| **Laço próprio** | O Windows Graphics Capture chama o app a cada quadro; aqui é o app que pede (`acquire_next_frame` com prazo), numa thread dele, e quadro novo só existe quando a tela muda. O teto de fps e o "nada de trabalho por quadro na thread da captura" continuam valendo |
| **Validação** | A máquina do dono é Windows 11 (build 26200): `UNKVOID_DUPLICATION=on` força o caminho e prova a lógica, mas o resultado só vale numa máquina com Windows 10 de verdade |

## O que foi feito

O módulo é `shared/capture/src/windows_duplication.rs`, e vale onde `Duplication::needed()` diz
que a borda não sai (ou com `UNKVOID_DUPLICATION=on`):

1. **Monitor inteiro sem borda**, na transmissão (`capture/src/windows.rs`), nos clipes
   (`clips/src/capture.rs`) e na prévia do seletor de tela — pelo Graphics Capture ela piscava
   no monitor cada vez que o seletor abria. Janela, e todo o Windows 11, continuam como estavam.
   O teto de fps conta a partir do quadro devido, e não do último entregue: num monitor de
   240 Hz a média fica no fps pedido.
2. **O cursor**, pelo GDI, respeitando a opção de mostrar o cursor que a transmissão já tem. Os
   clipes gravam sempre com ele, como no Graphics Capture.

Provado em 01/10/2026 no Windows 11 com o caminho forçado: 61 quadros em 1 s pedindo 60
(`the_primary_monitor_is_duplicated_with_the_cursor`, ignorado por padrão, salva o primeiro
quadro em BMP), a seta desenhada na cópia (`the_cursor_is_painted_on_the_copy_of_the_frame`),
o replay gravando pelo caminho certo (`the_replay_records_through_the_desktop_duplication`) e
uma transmissão de verdade para o SFU de produção, assistida do outro lado a 30 fps sem pacote
perdido.

## O que falta

**Prova em hardware**: uma máquina com Windows 10, com jogo em janela sem borda e em tela cheia
exclusiva, troca de resolução no meio, notebook com duas placas se houver. O resultado entra no
[ESTADO.md](ESTADO.md).

## O que fica de fora

- **Janela no Windows 10 continua com borda.** Os caminhos sem borda para janela (`BitBlt`,
  `PrintWindow`) passam pela CPU, que é exatamente o que o projeto existe para evitar.
- **Trocar a captura do Windows 11.** Lá o Windows Graphics Capture já vem sem borda, captura
  janela e traz o cursor pronto.

## O que mudaria a decisão

A Microsoft levar o `IsBorderRequired` para o Windows 10 (não há sinal disso: o 10 saiu de
suporte em outubro de 2025), ou o Windows 10 deixar de importar entre quem usa o app.
