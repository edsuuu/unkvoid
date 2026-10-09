# Build e teste no Linux

## O `.deb` (a release)

## O app Slint no Linux

O app Slint (`native/apps/windows`, o mesmo crate do Windows) compila, testa e abre num
contêiner próprio, e tem o seu `.deb` em `native/apps/windows/build-deb.sh` (Debian 12, sem GTK):
está tudo em [`native/apps/windows/README.md`](../native/apps/windows/README.md). O que segue é
o `.deb` do app GTK e o laboratório da captura.

O pacote é o `unkvoid`, do app nativo (`native/apps/linux`, GTK4), compilado dentro de um
**Debian 12** (`native/apps/linux/Dockerfile.deb`): o binário fica preso à glibc de quem compila,
e o Debian 12 é a distro mais velha que queremos suportar. Precisa de Docker; num Windows, roda
no WSL.

```bash
native/apps/linux/build-deb.sh          # sai em native/target/deb12/Unkvoid_<versão>_amd64.deb
```

A versão é a do `native/Cargo.toml` (`0.1.0-beta` vira `0.1.0~beta` no Debian, para a final vir
depois da beta). O build roda com `nice -n 19`.

Publicar no APT é na VPS, onde mora a chave GPG do repositório: o passo a passo está em
[AUTO-UPDATE.md](AUTO-UPDATE.md#linux). O `.deb` não leva assinatura própria; quem autentica é a
assinatura do índice.

> O `build-linux.yml` e o `native/apps/desktop/build-vps.sh` ainda geram o `.deb` do **Tauri**.
> Não rode nenhum dos dois: o Tauri voltaria por cima do nativo no próximo `apt upgrade`.

## O laboratório em contêiner

`native/tests/linux` é um Ubuntu 24.04 com os mesmos pacotes que o `.deb` exige, um Xvfb e um
PulseAudio de mentira. Seis casos (captura que passa dos 30 s, som do sistema, microfone,
`webrtcdsp`, encoder, `cargo test`):

```bash
docker build -t unkvoid-linux native/tests/linux
docker run --rm -v "$PWD/native:/unkvoid/native" unkvoid-linux /unkvoid/native/tests/linux/cenarios.sh
```

O que cada caso prova: [`native/tests/linux/README.md`](../native/tests/linux/README.md). A
janela do app sob `xvfb` é o `native/apps/linux/Dockerfile`.

## Compilar e testar sem contêiner

Numa máquina com GTK4 (`libgtk-4-dev`, `libdbus-1-dev`, GStreamer good/bad/ugly, `pactl`,
`cmake`):

```bash
cd native
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
UNKVOID_CAPTURE=x11 cargo test -p unkvoid-linux -- --ignored --nocapture   # teste vivo da captura
```

O teste vivo precisa de alguém escutando na porta de destino: um socket UDP apontado para porta
fechada recebe o ICMP de volta e falha no envio seguinte.

No WSL: uma compilação de Rust por vez (dois `cargo` trocam memória com o disco até o WSL parar
de responder), e `UNKVOID_CAPTURE=x11` para compartilhar a tela no WSLg — compartilhar a tela
inteira sai preto ali; compartilhe uma janela.
