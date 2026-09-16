# Build e teste no Linux

O Linux é onde a tela é compartilhada de verdade, e é o sistema que ninguém da equipe tem
na mesa. Por isso o build e os testes moram num contêiner: a máquina é sempre a mesma, e a
imagem é o próprio Ubuntu que o `.deb` exige.

## O contêiner

```bash
docker build -t unkvoid-linux native/tests/linux
```

A imagem traz o GStreamer (os mesmos plugins do `depends` do `.deb`), o WebKitGTK e o GTK de
desenvolvimento, o Rust, o Node 22, o `cmake` (o Opus compila em C) e um Xvfb com PulseAudio
para haver tela e som de mentira.

## Os casos de uso

```bash
docker run --rm -v "$PWD/native:/unkvoid/native" unkvoid-linux \
    /unkvoid/native/tests/linux/cenarios.sh
```

Seis casos, descritos em [`native/tests/linux/README.md`](../native/tests/linux/README.md). O
primeiro é o que responde ao relato "aos 30 segundos a transmissão cai".

## Quem gera a release de verdade

**A release do Linux não sai daqui.** Ela sai da VPS: `push` na `main` que toque `native/`
dispara o fluxo `build-linux`, que roda `native/apps/desktop/build-vps.sh` dentro de um
Debian 12 e publica no repositório APT. Duas razões que não dá para reproduzir num Mac: o
binário fica preso à glibc de quem compila (Debian 12 é a distro mais velha que queremos
suportar), e a chave GPG que assina o repositório mora só naquela máquina. No Linux quem
atualiza é o `apt`, não o atualizador embutido do Tauri.

O que está abaixo é o build **local**, para provar que o código compila no Linux antes de
mandar para a `main` — não para distribuir.

## Gerar o `.deb` local

```bash
docker volume create unkvoid-linux-target

docker run --rm \
    -v "$PWD/native:/unkvoid/native" \
    -v unkvoid-linux-target:/target \
    -v unkvoid-linux-node:/unkvoid/native/apps/desktop/node_modules \
    -e CARGO_TARGET_DIR=/target \
    -w /unkvoid/native/apps/desktop \
    unkvoid-linux bash -lc 'npm ci && npx tauri build --bundles deb'
```

Três detalhes que não são decoração:

- **`CARGO_TARGET_DIR=/target`**: sem isso o cargo escreve os objetos de Linux no mesmo
  `native/target` que o Mac usa, e os dois passam a se recompilar em looping.
- **O volume em `node_modules`**: o `esbuild` e o `rollup` instalam binário por plataforma. O
  `node_modules` do Mac não roda no contêiner, e o volume anônimo esconde o de fora.
- **Não passe `VITE_SERVER`**: sem a variável o app aponta para `https://unkvoid.com`, que é o
  que a release precisa. Já houve `.dmg` publicado apontando para `127.0.0.1`.

O arquivo sai em `/target/release/bundle/deb/Unkvoid_<versao>_<arch>.deb`, dentro do volume.
Para tirar de lá:

```bash
docker run --rm -v unkvoid-linux-target:/target -v "$PWD:/fora" unkvoid-linux \
    bash -lc 'cp /target/release/bundle/deb/*.deb /fora/'
```

## Arquitetura

O contêiner compila para a arquitetura do host. Num Mac com Apple Silicon sai um `.deb`
**arm64**; para o `amd64` que a maioria usa, acrescente `--platform linux/amd64` ao `docker
run` (o Docker Desktop traduz, e a compilação fica bem mais lenta).

## Assinatura

O `.deb` local **não** é assinado, e não precisa: quem autentica o pacote no Linux é a
assinatura GPG do repositório APT, feita na VPS. A chave do atualizador embutido (a mesma do
macOS e do Windows) não tem uso aqui — veja [AUTO-UPDATE.md](AUTO-UPDATE.md).
