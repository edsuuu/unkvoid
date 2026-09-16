# O laboratório de Linux

O Linux é onde a tela é compartilhada de verdade, e é o único dos três sistemas que ninguém
aqui tem à mão. Este contêiner é o Linux de mentira que responde às perguntas que importam:
a captura aguenta passar dos 30 segundos, o som do sistema entra junto, o microfone abre, e
o tratamento de áudio existe na máquina.

```bash
# constrói a imagem (uma vez; ~3 min na primeira)
docker build -t unkvoid-linux native/tests/linux

# roda os casos de uso
docker run --rm -v "$PWD/native:/unkvoid/native" unkvoid-linux native/tests/linux/cenarios.sh
```

O `-v` monta o código de fora: o que roda lá dentro é o mesmo commit que está aqui, sem
copiar nada para dentro da imagem.

`SEGUNDOS=90 docker run ...` alonga o Caso 1, que é o do relato "aos 30 segundos a
transmissão cai".

## O que cada caso prova

| Caso | Pergunta | Como falha |
|---|---|---|
| 1 | a captura entrega quadro em todo segundo, inclusive depois dos 30 s | algum segundo com 0 fps |
| 2 | o áudio do sistema entra junto com a tela | nenhum bloco de áudio |
| 3 | o microfone abre e entrega som | menos de 10 KB em 100 buffers |
| 4 | a máquina tem `webrtcdsp` (eco, ruído, ganho) | o plugin não está instalado |
| 5 | algum encoder de H.264 abre de verdade | a captura não sobe |
| 6 | os testes do Rust passam neste Linux | `cargo test --workspace` |

O que este contêiner **não** prova: a janela do app (WebKitGTK), o receptor nativo de
MJPEG e o caminho até o SFU — para esses, veja `cargo run -p media --example plain`, que
precisa de um SFU no ar.
