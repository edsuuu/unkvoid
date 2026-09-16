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

Teste nesta ordem:

1. O app passa da tela “Sem conexão” e mostra a tela de entrada.
2. Crie uma sala e copie o código.
3. Abra o mesmo instalador em outro Mac ou Windows e entre com o código.
4. Compartilhe a tela e confirme vídeo e áudio.

## Atualização automática

A versão do desktop fica em `native/apps/desktop/src-tauri/tauri.conf.json`.
Ela deve ser igual à versão publicada no SFU em `SFU_APP_VERSION`. O endpoint
`https://discord.unkvoid.com/health` retorna essa versão em `appVersion`.

Quando o app detectar que `appVersion` é diferente da própria versão, ele consulta
o updater do Tauri e instala a release publicada em:

```text
https://github.com/edsuuu/unkvoid/releases/latest/download/latest.json
```

Depois de mudar a versão, faça o seguinte:

1. Atualize `version` no `tauri.conf.json`.
2. Gere o instalador assinado com `TAURI_SIGNING_PRIVATE_KEY` definido.
3. Publique a release com `node release.mjs`.
4. Atualize `SFU_APP_VERSION` no `sfu/ecosystem.config.cjs`.
5. Rode `./deploy.sh vps` dentro de `sfu`.

Sem assinatura (`.sig`) o instalador funciona, mas o updater automático não consegue
validar e instalar a atualização.
