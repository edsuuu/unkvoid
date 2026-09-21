# A borda amarela no Windows 10

Quem transmite num **Windows 10** vê uma borda amarela em volta da tela ou da janela capturada.
No Windows 11 ela não aparece. Este arquivo diz por quê, o que dá para fazer e o que custa —
levantado em 20/09/2026, **nada disto está implementado**.

## Por que ela aparece

A captura do Windows é o Windows Graphics Capture (`crates/capture/src/windows.rs`, pela crate
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
| **O cursor não vem na imagem** | A API entrega o ponteiro à parte (posição no `frame_info`, forma no `GetFramePointerShape`). Sem desenhá-lo na GPU por cima do quadro, quem assiste um jogo com cursor de hardware não vê o mouse. São três formatos de ponteiro (monocromático, colorido, colorido com máscara), e o colorido tem transparência: é um desenho, não uma cópia |
| **O acesso cai** | Troca de resolução, tela cheia exclusiva, UAC e bloqueio de tela devolvem `DXGI_ERROR_ACCESS_LOST`. O app precisa recriar a duplicação sozinho (`recreate`), sem derrubar a transmissão |
| **Duas placas de vídeo** | Em notebook híbrido o device do Direct3D tem de estar na placa dona da saída; a ponte entre devices que o encoder já monta (`media/src/windows.rs`) tem de continuar valendo |
| **Laço próprio** | O Windows Graphics Capture chama o app a cada quadro; aqui é o app que pede (`acquire_next_frame` com prazo), numa thread dele, e quadro novo só existe quando a tela muda. O teto de fps e o "nada de trabalho por quadro na thread da captura" continuam valendo |
| **Validação** | A máquina do dono é Windows 11 (build 26200): dá para forçar o caminho por variável de ambiente e provar a lógica, mas o resultado só vale numa máquina com Windows 10 de verdade |

## O plano, em fases

Cada fase para para revisão antes da seguinte.

1. **Monitor inteiro sem borda.** Quando o sistema não desliga a borda
   (`is_border_settings_supported()` falso) e a origem é um monitor, a captura vai pelo Desktop
   Duplication; janela, e todo o Windows 11, continuam como estão. Recriação automática no
   `ACCESS_LOST`. Uma variável de ambiente (`UNKVOID_CAPTURE=duplication|wgc`) força um caminho
   ou o outro — é o botão de calibração, e é como o caminho novo se testa no Windows 11.
2. **O cursor desenhado na GPU**, respeitando a opção de mostrar ou não o cursor que a captura
   já tem. Até esta fase sair, a fase 1 transmite **sem cursor** no Windows 10.
3. **Prova em hardware**: uma máquina com Windows 10, com jogo em janela sem borda e em tela
   cheia exclusiva, troca de resolução no meio, notebook com duas placas se houver. O resultado
   entra no [ESTADO.md](ESTADO.md).

## O que fica de fora

- **Janela no Windows 10 continua com borda.** Os caminhos sem borda para janela (`BitBlt`,
  `PrintWindow`) passam pela CPU, que é exatamente o que o projeto existe para evitar.
- **Trocar a captura do Windows 11.** Lá o Windows Graphics Capture já vem sem borda, captura
  janela e traz o cursor pronto.

## O que mudaria a decisão

A Microsoft levar o `IsBorderRequired` para o Windows 10 (não há sinal disso: o 10 saiu de
suporte em outubro de 2025), ou o Windows 10 deixar de importar entre quem usa o app.
