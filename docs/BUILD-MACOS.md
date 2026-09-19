# Build e teste no macOS

O build do macOS precisa ser feito em um Mac. O projeto não faz cross-compilação
confiável para gerar o `.app` ou o `.dmg` a partir do Windows/Linux.

## Preparar o Mac

Instale:

- Xcode Command Line Tools: `xcode-select --install`
- Rust: https://rustup.rs
- Node.js 22 ou superior

Confirme:

```bash
xcode-select -p
rustc --version
node --version
```

## Gerar o build

Na raiz do repositório:

```bash
cd native/apps/desktop
npm ci
npm run check
npx tauri build --bundles app,dmg
```

Os arquivos serão gerados em:

```text
native/target/release/bundle/macos/Unkvoid.app.tar.gz
native/target/release/bundle/dmg/Unkvoid_<versao>_aarch64.dmg
```

Em Mac Intel, o sufixo do instalador será `x64` em vez de `aarch64`.

## Testar

Abra o aplicativo pelo Finder ou pelo terminal:

```bash
open native/target/release/bundle/macos/Unkvoid.app
```

Na primeira execução, permita **Gravação de Tela** em:

`Ajustes do Sistema > Privacidade e Segurança > Gravação de Tela`

A permissão vai para o app que *lançou* o processo: rodando pelo terminal, é o terminal que
aparece na lista, e não o Unkvoid.

Teste nesta ordem:

1. O app passa da tela “Sem conexão” e mostra a tela de entrada.
2. Crie uma sala e copie o código.
3. Abra o mesmo instalador em outro Mac ou Windows e entre com o código.
4. Compartilhe a tela e confirme vídeo e áudio.

## Assinar e publicar

O `.dmg` é para quem instala pela primeira vez; quem já tem o app se atualiza pelo
`.app.tar.gz` assinado. A chave, o comando de publicação e o que conferir quando alguém
não atualiza estão em [AUTO-UPDATE.md](AUTO-UPDATE.md#macos). Sem o `.sig` o instalador
funciona, mas não atualiza ninguém.
