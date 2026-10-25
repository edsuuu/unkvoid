# Build e teste no Linux

O app único está provisoriamente em `native/apps/unkvoid`; ele também gera o `.deb` e será
renomeado para `native/apps/unkvoid`. O build usa Debian 12 para o binário continuar compatível
com distribuições mais antigas.

## O `.deb`

Precisa de Docker. No Windows, rode no WSL:

```bash
native/apps/unkvoid/build-deb.sh
```

O arquivo sai em `native/target/deb12/Unkvoid_<versão>_amd64.deb`. A versão é a de
`[workspace.package]` em `native/Cargo.toml`; uma pré-release como `0.1.0-beta` vira
`0.1.0~beta` para o Debian ordenar a final depois dela.

Antes de publicar, o validador instala o arquivo em uma Debian 12 limpa:

```bash
native/release/validate-deb.sh native/target/deb12/Unkvoid_<versão>_amd64.deb
```

O passo de APT está em [AUTO-UPDATE.md](AUTO-UPDATE.md#linux). O workflow `build-linux.yml`
continua pausado e chama `native/release/build-vps.sh`, sem instalar nem compilar o aplicativo
antigo.

## Rodar a janela no Linux

O `Dockerfile` do app compila, testa e abre a janela sob Xvfb:

```bash
cd native
docker build -f apps/unkvoid/Dockerfile -t unkvoid-app .
docker run --rm --cpus=2 unkvoid-app
```

Para compilar sem contêiner, instale as bibliotecas de desenvolvimento listadas em
`native/apps/unkvoid/Dockerfile.deb` e rode `cargo test -p unkvoid`.
