# Unkvoid no Windows

Rust + Slint. O núcleo entra **como crate**, direto — sem ABI C, sem JSON no meio, sem
`DllImport` e sem liberar ponteiro à mão. É o mesmo desenho do `apps/linux`.

## Rodar

```bash
cargo run -p unkvoid-windows
UNKVOID_SERVER=http://127.0.0.1:8000 cargo run -p unkvoid-windows   # contra a pilha local
```

Sem `UNKVOID_SERVER`, `https://unkvoid.com`. O endereço do SFU **não** é variável: ele vem do
`GET /api/config`, como no app de hoje.

```bash
cargo clippy -p unkvoid-windows --all-targets -- -D warnings
cargo test -p unkvoid-windows
```

O Slint compila e roda no macOS e no Linux também: dá para abrir a janela e conferir o
desenho fora do Windows. O que só uma máquina Windows confirma é a lista de aparelhos de
áudio (WASAPI) e a aparência final naquele sistema.

## Onde vai cada coisa

| Arquivo | O quê |
|---|---|
| `src/main.rs` | abre a janela, liga os cliques ao núcleo, entrega o laço ao Slint |
| `src/bridge.rs` | clique → Tokio → volta para a janela. Traduz `Failure`/`EntryRefusal` em frase. **Sem regra de negócio** |
| `src/devices.rs` | a lista de microfones e de saídas de áudio, pelo WASAPI |
| `ui/skin.slint` | a paleta, as medidas e os traçados dos ícones |
| `ui/widgets.slint` | o vocabulário do desenho: vidro, campo, botão, avatar, linha de lista |
| `ui/state.slint` | o `Ui`: o que a tela mostra e o nome de cada clique |
| `ui/entry.slint`, `hub.slint`, `room.slint`, `shell.slint` | as cinco telas |
| `ui/userbar.slint` | a barra de baixo: nome, microfone, áudio e a setinha de cada um |
| `ui/app.slint` | a janela: a pilha das cinco telas |

## O desenho

A referência é `apps/desktop/ui/` — os `.tsx` para comportamento e `ui/style.css` para os
valores exatos. `ui/skin.slint` é a tradução dela: `#06050a` no fundo, `.glass` com raio 20 e
borda `rgba(255,255,255,0.09)`, `.field` com raio 12 e padding 11/13, `.btn-primary` com o
degradê de 180°, os cartões da entrada com 420 de largura e 32 de respiro.

Duas coisas do CSS não têm par no Slint e estão aproximadas:

- **`backdrop-filter`.** Não existe desfoque por elemento. O que sobra é o fundo translúcido
  sobre o halo violeta — que é de onde vem quase toda a aparência do vidro.
- **A fonte.** A cadeia é `Archivo, Helvetica, Arial`; o projeto não distribui a Archivo,
  então na prática aparece a fonte de sistema mais próxima.

Os ícones são `Path` com os mesmos traçados de `ui/components/common/Icon.tsx`. Emoji ficaria
à mercê da fonte do sistema e destoaria do desenho, que é todo de linha.

## O que ainda não está ligado

| Falta | Sem isso, a tela |
|---|---|
| captura e mídia (`shared/capture` + `shared/media`) | "Compartilhar tela", "Câmera", microfone e ensurdecer são botão e mais nada; o palco da `Room` fica vazio |
| login com Google | o botão diz que não está ligado e pede e-mail e senha |
| tempo real (Reverb) | o chat só atualiza quando alguém manda uma mensagem daqui |
| criar servidor, canal, cargo; convite, expulsar, banir, auditoria | os modais de `ui/components/hub/modals/` não têm par aqui |

O caminho para ligar a mídia já existe em `apps/linux` (`sending.rs`, `streaming.rs`,
`watching.rs`, e o `bridge.rs` de lá): nada disso é GTK, e o que muda é o encoder.

## O que é do Windows, e não do núcleo

- Bandeja, notificação, atalho global
- A lista de aparelhos de áudio (WASAPI, em `src/devices.rs`)
- A borda amarela da captura (ver `docs/BORDA-AMARELA.md`)

## O que nunca vai aqui

O que o macOS e o Linux também precisariam. Isso é `shared/core`.

## Cuidados

**A tela não espera rede.** O Slint desenha numa thread só; toda ida ao servidor sai por
`Bridge::spawn` (Tokio) e volta por `upgrade_in_event_loop`. Chamar o núcleo direto do clique
congelaria a janela.

**Erro na tela não mostra caminho, URL nem status.** O núcleo devolve o motivo (`Failure`,
`EntryRefusal`) e `bridge.rs` o vira em frase. O erro de validação do Laravel chega com o
campo: ele pinta o campo e escreve logo abaixo dele, nunca numa linha solta no rodapé.

**O Slint não recorta string.** Não há `substring`: a inicial do avatar e a hora da mensagem
são cortadas no Rust, onde se sabe onde um caractere começa e termina.
