# O app nativo

Um núcleo Rust decide as regras e o app único só desenha e traduz ações. Durante a reescrita para
egui, ele ainda está em `native/apps/unkvoid`; o diretório será renomeado para
`native/apps/unkvoid`.

## As camadas

| Pasta | O que faz |
|---|---|
| `shared/capture/` | captura de tela, som do sistema, microfone e câmera, com implementações por sistema |
| `shared/media/` | encoder de hardware, Opus, RTP/SRTP, recuperação, buffer de chegada e decodificação |
| `shared/core/` | **as regras**: sessão e reconexão, cliente do SFU, voz, tela, câmera, cliente Laravel, estado, atualização e motivos de falha |
| `shared/storage/` | estado em disco e token cifrado pelo chaveiro do sistema |
| `shared/clips/` | replay instantâneo (Clips) |
| `apps/unkvoid/` | caminho temporário do app único; usa o núcleo como crate direto |
| `release/` | assinatura minisign, publicação no site e no APT e validação do `.deb` |

**A regra:** regra de negócio não mora no app. Se a decisão diz respeito à sessão, ao token, ao
SFU ou à permissão, ela sobe para `shared/core`.

## Mídia

O caminho é `captura → textura na GPU → encoder de hardware → RTP/SRTP → SFU → espectadores`.
O app não usa uma webview: a interface e o núcleo são Rust no mesmo processo, e a mídia não deve
descer para a CPU antes de ser comprimida.

- O envio abre um PlainTransport SRTP e pode produzir tela, som da tela, microfone e câmera.
- Quem assiste usa NACK, RTX e pedido de quadro-chave; o `Playout` segura cada fluxo até a hora
  de desenhar e tocar.
- O encoder tenta a placa de vídeo; sem ela, usa o caminho de software configurado pelo núcleo.
- O SFU recebe a autorização já assinada pelo Laravel. O app só pede o token, nunca decide se a
  pessoa pode entrar ou transmitir.

## Build e publicação

O instalador do site e o MSIX usam os ícones em `native/apps/unkvoid/icons/`. A versão de todos
os artefatos é `[workspace.package].version` em `native/Cargo.toml`.

O instalador do site recebe uma assinatura minisign encapsulada em Base64. A pública fica somente
em `native/release/update-public-key.pub`, incluída pelo núcleo e lida pelos scripts de release.
O `.deb` é gerado no Debian 12 e atualizado pelo APT, cuja chave GPG é outra.

Os comandos completos estão em [BUILD-WINDOWS.md](BUILD-WINDOWS.md),
[BUILD-LINUX.md](BUILD-LINUX.md) e [AUTO-UPDATE.md](AUTO-UPDATE.md).

## O que fica no disco

| O quê | Onde |
|---|---|
| estado e preferências (`state.json`) | pasta de configuração do sistema, em `com.unkvoid.desktop` |
| token da conta | cifrado em AES-256-GCM; a chave fica no chaveiro do sistema |
| log do dia | `%LOCALAPPDATA%\com.unkvoid.desktop\` no Windows; no Linux, `$XDG_STATE_HOME` ou `~/.local/state/unkvoid/` |
