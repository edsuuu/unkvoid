# App desktop e SFU em Rust

O motivo de existir: no navegador o compartilhamento sempre carrega a barra do
Chrome, e **no macOS o WKWebView nem oferece `getDisplayMedia`** — então embrulhar a
interface web num Tauri não compartilharia tela. Aqui a captura é nativa.

## Estado

| Peça | Situação |
|---|---|
| Captura macOS (ScreenCaptureKit + áudio de sistema) | escrita, **bloqueada por permissão** |
| Captura Windows (Graphics Capture) | escrita, type-checa por cross-compile, **não testada** |
| Captura Linux (portal XDG) | recusa com erro claro; falta consumir o nó do PipeWire |
| App Tauri + comandos | compila e roda |
| Instaladores .msi/.deb/.dmg | via CI, em runner nativo |
| WebRTC P2P / SFU em str0m | ainda não começou |

**Nada aqui foi testado com captura real.** No macOS a permissão de gravação de tela
foi negada pelo sistema; Windows e Linux não têm como ser testados desta máquina.

## Permissão no macOS

A captura pede autorização e o sistema atribui a permissão ao app que **lançou** o
processo (o terminal, no caso do spike). Libere em:

**Ajustes do Sistema → Privacidade e Segurança → Gravação de Tela**

Depois rode o spike, que mede fps, resolução e se o áudio de sistema chega:

```bash
cd native
cargo run -p capture --example spike -- 1080 10
```

## Estrutura

```
native/
  crates/capture/       captura por plataforma, uma API só
  apps/desktop/         Tauri: comandos + interface
  .cargo/config.toml    caminho do runtime Swift e alvo mínimo do macOS
```

## Instaladores

Saem do CI (`.github/workflows/desktop.yml`), um por runner nativo — não dá para
gerar os três de uma máquina só. Ficam como artefato de cada execução.

Localmente, só para a sua plataforma:

```bash
cargo install tauri-cli --version "^2" --locked
cd native/apps/desktop && cargo tauri build
```

## Armadilha já resolvida

A crate `screencapturekit` compila Swift e o linker procura o runtime no caminho do
Xcode completo. Com só os Command Line Tools instalados o caminho é outro — está
resolvido no `.cargo/config.toml`, junto com o alvo mínimo 13.0 que o
ScreenCaptureKit exige.
