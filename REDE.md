# O caminho da imagem

Como um quadro sai da placa de vídeo de quem transmite e chega na tela de quem
assiste, e o que foi ajustado em cada trecho. Cada número aqui saiu de medição
na máquina real, não de recomendação de manual.

## O trajeto

```
captura (Windows Graphics Capture)
  → textura no Direct3D 11
  → ponte entre dois devices, com keyed mutex
  → encoder de hardware (NVENC / QuickSync / VCE via Media Foundation)
  → empacotador RTP, MTU de 1200 bytes
  → SRTP, AES-128 com HMAC-SHA1
  → socket UDP → internet → mediasoup, transporte plain
  → replicado por WebRTC para cada espectador
```

O quadro é codificado **uma vez** e sobe **uma vez**. É isso que faz transmitir
enquanto se joga não custar fps, e que faz o upload não crescer com a plateia.

## Os ajustes, e o que cada um resolveu

### Buffer de saída do socket, em quem transmite

**Sintoma:** 14% dos pacotes largados de forma contínua, umas noventa por
segundo, em 1080p60. A imagem de quem assistia travava a cada movimento na tela.

**Causa:** o padrão do Windows para envio de datagrama é 8 KB. Um quadro em
1080p60 sai em cerca de dez mensagens de 1200 bytes, ou seja, mais do que cabe.
Todo quadro estourava o buffer, e o que não coubesse voltava como recusa.

O comentário do código afirmava que buffer cheio significa uplink saturado.
Medindo o upload durante a própria transmissão deu 78 Mb/s para 5 Mb/s em uso.
Não era o uplink.

**Correção:** `SO_SNDBUF` de 4 MB, em `crates/media/src/plain.rs`. É teto e não
reserva. Vale para os três sistemas, porque o padrão do macOS é igualmente
pequeno.

### Buffer de recepção do socket, no servidor

**Sintoma:** com o lado de quem transmite corrigido, o espectador ainda via
perda. O socket do transporte plain acumulou 5738 pacotes descartados.

**Causa:** o mesmo defeito espelhado. O padrão do Ubuntu é 208 KB e o mediasoup
não chama `setsockopt` para o socket dele, então herda esse valor.

**Correção:** `net.core.rmem_default = 4194304`, em
`/etc/sysctl.d/99-unkvoid-udp.conf` na VPS. **Isto mora fora do repositório** —
é configuração de máquina, e está registrado aqui porque senão só existe na
memória de quem aplicou.

| | |
|---|---|
| Pior caso com 4 transmissões em 1440p | 16 MB |
| Teto que o kernel impõe ao UDP | 1,43 GB |

Não precisa reiniciar o SFU: o mediasoup cria um socket novo a cada transmissão.

Para conferir se voltou a acontecer, na VPS:

```
grep "^Udp:" /proc/net/snmp   # a quinta coluna é RcvbufErrors
sudo ss -uanm | grep -A1 ':41'  # `d0` no fim da linha é zero descartes
```

### Controle de taxa do encoder do Windows

**Sintoma:** 11 Mb/s medidos com 7 Mb/s configurados.

**Causa, primeira metade:** o tipo de mídia só declarava a taxa média, que é
dica. Sem declarar o **modo**, o MFT da placa escolhe o dele e gasta o que
quiser.

**Causa, segunda metade:** a amostra ia carimbada com um contador de quadros
sobre o fps nominal. Com a captura entregando 53 quadros por segundo e o relógio
andando como se fossem 60, o encoder espalhava um segundo de bits por 0,88
segundo de vídeo. O caminho do macOS já tinha apanhado disso, com dois terços em
vez de um oitavo, e o comentário do `encode` de lá conta a história.

**Correção:** taxa constante, baixa latência e tempo real pelo `ICodecAPI`, mais
a hora real da captura na amostra. Os ajustes não são fatais: encoder que não
implementa a propriedade recusa, e o que foi recusado vai para o log. Na placa
de teste, "baixa latência" e "tempo real" foram recusados; taxa constante e
intervalo de quadro-chave foram aceitos.

### A tabela de taxa

Subiu depois que o controle passou a ser respeitado de verdade: com o excesso
removido, 1080p60 caiu para 6,5 Mb/s reais e a imagem piorou visivelmente. O
excesso estava tapando um teto baixo demais.

| Qualidade | Antes | Agora |
|---|---|---|
| 720p60 | 4 Mb/s | 5 Mb/s |
| 1080p60 | 7 Mb/s | 10 Mb/s |
| 1440p60 | 12 Mb/s | 16 Mb/s |

Jogo a 60 quadros é o pior caso do H.264: a cena inteira muda a cada quadro.

### Recuperação de perda

Perder um pacote congela quem assiste até o próximo quadro-chave. Duas pernas,
dois remédios diferentes.

**Do servidor até quem assiste** a retransmissão já existe, porque o mediasoup a
liga por padrão. O que faltava era tempo: com 150 ms de ida e volta, o pacote
reenviado chegava depois da hora de exibir e era descartado. O player agora pede
400 ms de folga de reprodução, em `ui/SfuClient.js`. Só vídeo — o áudio já se
protege com o FEC do Opus, e atrasar som é o que se nota primeiro.

**De quem transmite até o servidor** não existe retransmissão. O servidor manda
um pedido de quadro-chave assim que vê o buraco, e o app simplesmente não lia o
socket. Agora lê, uma vez por quadro, sem bloquear. O pedido vem cifrado com a
chave de saída do servidor, que sempre veio na resposta do `producePlain` e era
descartada pelo app.

Isso troca até um segundo congelado por uma ida e volta. O contador
`keyframesAsked` no diagnóstico mede quantas vezes o servidor viu um buraco — é
a medida de perda que existe nessa perna.

### Intervalo de quadro-chave

Um segundo, nos dois sistemas. É o teto da travada de quem perdeu um pacote e
não tem quem retransmita. O macOS já usava esse valor; o Windows não usava
nenhum.

## O que ainda não existe

- **Pacing.** Um quadro inteiro é enviado em rajada, no ritmo da placa de rede.
  É a suspeita seguinte se voltar a haver perda com os buffers grandes dos dois
  lados.
- **Retransmissão de quem transmite para o servidor.** Só o quadro-chave sob
  demanda, que é mais barato e resolve o caso comum.
- **Adaptação de taxa.** A qualidade é escolhida à mão e nada a reduz quando a
  rede piora.
