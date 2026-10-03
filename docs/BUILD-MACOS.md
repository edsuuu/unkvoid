# Build e teste no macOS

O app do macOS é o nativo (`native/apps/macos`, SwiftUI sobre o núcleo em Rust pela ABI C). Só
compila num Mac. **Ainda não há versão publicada.**

## Preparar o Mac

- Xcode Command Line Tools: `xcode-select --install`
- Rust: <https://rustup.rs>

## Rodar e testar

```bash
cd native/apps/macos
./run.sh            # compila o núcleo e abre o app
./run.sh test       # os testes do núcleo e do app, em série, contra a pilha local
./run.sh app        # monta o build/Unkvoid.app e abre
```

Os detalhes (variáveis, contas de teste, por que os testes rodam em série) estão no
[`native/apps/macos/README.md`](../native/apps/macos/README.md).

## Gerar o `.app`

```bash
cd native/apps/macos
./bundle.sh                                         # build/Unkvoid.app, assinado ad-hoc
./bundle.sh --sign "Developer ID Application: …"    # com certificado de editor
```

Na primeira execução, permita **Gravação de Tela**, **Microfone** e **Câmera** em `Ajustes do
Sistema > Privacidade e Segurança`. A permissão vai para o app que *lançou* o processo: pelo
`./run.sh`, é o terminal que aparece na lista; pelo `.app`, é o Unkvoid.

Teste nesta ordem: o app passa da tela "Sem conexão"; crie uma sala; entre com o código de outro
computador; compartilhe a tela e confirme vídeo e áudio.

## Publicar

Falta o caminho: empacotar o `.dmg` e registrar a plataforma `darwin-aarch64` com o
`publish-release.sh` ([AUTO-UPDATE.md](AUTO-UPDATE.md)). Sem certificado da Apple, o macOS pede
para liberar o app na primeira abertura.
