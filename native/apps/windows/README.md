# Unkvoid no Windows

Rust + Slint. O núcleo entra **como crate**, direto — sem ABI C e sem JSON no meio. É o app
publicado no site e o pacote da Microsoft Store.

## Rodar

```bash
cargo run -p unkvoid-windows                                          # contra https://unkvoid.com
UNKVOID_SERVER=http://127.0.0.1:8000 cargo run -p unkvoid-windows     # contra a pilha local
cargo clippy -p unkvoid-windows --all-targets -- -D warnings
cargo test -p unkvoid-windows
```

O endereço do SFU não é variável: vem do `GET /api/config`. O `opusic-sys` compila C e pede o
`cmake` no PATH (o do Visual Studio Build Tools serve; o `build-installer.ps1` o acha sozinho).
Instalador e pacote da Store: [docs/BUILD-WINDOWS.md](../../../docs/BUILD-WINDOWS.md).

Para diagnóstico:

| Variável | O que faz |
|---|---|
| `UNKVOID_ENCODER=cpu` | transmite pelo encoder de software (720p30), sem a placa |
| `UNKVOID_DECODER=cpu` | assiste decodificando na CPU, sem o DXVA |
| `UNKVOID_QUALITY_VS_SPEED=0..100` | o preset do encoder da placa (padrão 50) |
| `UNKVOID_DUPLICATION=on` | captura o monitor pelo Desktop Duplication também no Windows 11 |
| `UNKVOID_LOSS=3` | joga fora 3% do RTP que chega, para provar a recuperação |
| `UNKVOID_ABR=off` | a taxa do vídeo fica fixa, sem o governador |

Teste vivo, sem janela, com alguém transmitindo numa sala:
`UNKVOID_ROOM=<código> cargo test -p unkvoid-windows a_live_screen -- --ignored --nocapture`.

## Onde vai cada coisa

| Arquivo | O quê |
|---|---|
| `src/main.rs` | abre a janela, o log do dia, a instância única e entrega o laço ao Slint |
| `src/bridge.rs` | clique → Tokio → volta para a janela; traduz cada motivo do núcleo em frase. **Sem regra de negócio** |
| `src/watching.rs` | assistir: uma thread por tela, cada quadro guardado comprimido até a hora dele (`Playout`), decodificado na placa e escrito em RGBA direto na imagem; o som vai para o `sound.rs` |
| `src/sound.rs` | a saída e o microfone pelo WASAPI, reabertos sozinhos quando o aparelho some ou o padrão muda |
| `src/devices.rs` | a lista de microfones e saídas |
| `src/stage.rs` | os cartões do palco, do jeito que a janela desenha |
| `src/frame.rs`, `ui/frame.slint` | a moldura da janela, com o botão de atualização ao lado do minimizar |
| `src/logbook.rs` | o log do dia e o envio das linhas com `ERROR` para o site |
| `src/sharing.rs` | testes vivos da captura e do encoder, sem sala |
| `src/clips/` | os Clips (replay instantâneo): bandeja, atalhos, galeria, player, aviso de "replay salvo", e o que muda quando o app roda como pacote da Store (`shell.rs`) |
| `ui/*.slint` | as telas: `entry`, `hub`, `room`, `voice`, `stage`, `share`, `settings`, `clips`, e o vocabulário do desenho em `skin` e `widgets` |
| `build-installer.ps1`, `installer.nsi` | o instalador do site |
| `build-msix.ps1`, `msix/AppxManifest.xml` | o pacote da Microsoft Store |

## O desenho

A referência é o React em `apps/desktop/ui/`: os `.tsx` para comportamento e o `ui/style.css`
para os valores exatos; `ui/skin.slint` é a tradução dele. O `backdrop-filter` não tem par no
Slint (sobra o fundo translúcido sobre o halo violeta) e a fonte Archivo não é distribuída.

## Cuidados

- **A tela não espera rede.** O Slint desenha numa thread só; toda ida ao servidor sai por
  `Bridge::spawn` (Tokio) e volta por `upgrade_in_event_loop`.
- **Nada pesado na thread da janela**: decodificar, converter e soltar o que assiste (`Watch`)
  acontece fora dela.
- **Erro na tela não mostra caminho, URL nem status.** O núcleo devolve o motivo e o
  `bridge.rs` escreve a frase; o erro de validação do Laravel pinta o campo e escreve embaixo
  dele.
- **O Slint não recorta string**: a inicial do avatar e a hora da mensagem são cortadas no Rust.
- **O que o Linux e o macOS também precisariam não mora aqui**: é `shared/core`.
