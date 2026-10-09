# O caminho da imagem

Como um quadro sai da placa de vídeo de quem transmite e chega na tela de quem assiste, e o que
foi ajustado em cada trecho. Cada número aqui saiu de medição na máquina real, não de
recomendação de manual.

## O trajeto

```
captura (Windows Graphics Capture / ScreenCaptureKit / GStreamer)
  → textura na GPU
  → encoder de hardware (Media Foundation / VideoToolbox / GStreamer)
  → empacotador RTP, MTU de 1200 bytes → SRTP (AES-128 com HMAC-SHA1)
  → pacer: o vídeo sai espalhado a 2,5× a taxa
  → socket UDP → internet → mediasoup, transporte plain
  → replicado, ainda cifrado em SRTP, para o transporte plain de cada espectador
  → PlainReceiver: reordena, pede de novo o que faltou (NACK/RTX), remonta o quadro
  → buffer de chegada (Playout): cada quadro no horário do relógio do RTP
  → decodificador de cada sistema → tela
```

O quadro é codificado **uma vez** e sobe **uma vez**. É isso que faz transmitir enquanto se joga
não custar fps, e que faz o upload não crescer com a plateia.

## Os ajustes, e o que cada um resolveu

### Buffer de saída do socket, em quem transmite

**Sintoma:** 14% dos pacotes largados de forma contínua em 1080p60; a imagem de quem assistia
travava a cada movimento na tela.

**Causa:** o padrão do Windows para envio de datagrama é 8 KB, menos que um quadro. Medindo o
upload durante a transmissão deu 78 Mb/s livres para 5 Mb/s em uso: não era o uplink.

**Correção:** `SO_SNDBUF` de 4 MB, em `shared/media/src/plain.rs`. É teto e não reserva.

### Buffer de recepção, no servidor e em quem assiste

**No servidor:** com o lado de quem transmite corrigido, o socket do transporte plain ainda
acumulou 5738 descartes. O padrão do Ubuntu é 208 KB e o mediasoup não chama `setsockopt`.
**Correção:** `net.core.rmem_default = 4194304`, em `/etc/sysctl.d/99-unkvoid-udp.conf` na VPS —
fora do repositório, e por isso registrado aqui. Para conferir:

```
grep "^Udp:" /proc/net/snmp   # a quinta coluna é RcvbufErrors
sudo ss -uanm | grep -A1 ':41'  # `d0` no fim da linha é zero descartes
```

**Em quem assiste** (0.1.0-beta.2): o padrão do Windows é 64 KB, menor que um quadro-chave, e a
perda acontecia depois da recuperação, então nem NACK nem PLI saíam. **Correção:** 16 MB
(`RECEIVE_BUFFER` em `shared/media/src/receiver.rs`).

### O pacer (0.1.12)

**Sintoma:** o servidor mediu 16% de perda em 30/09; cada pacote perdido era um quadro que quem
assiste não montava. **Causa:** um quadro-chave de 1080p (centenas de pacotes) saía de uma vez,
na velocidade da placa de rede, e a fila curta do roteador de casa jogava fora o que não cabia.
**Correção:** o vídeo sai a 2,5× a taxa, como o WebRTC do navegador (`shared/media/src/pacer.rs`);
os reenvios entram na frente do vídeo novo, no mesmo ritmo. Medido com
`examples/room <wss> <sala> watch 40`: de 49 + 2 pacotes perdidos em 30 s para 0 em 40 s.

### Controle de taxa do encoder do Windows

**Sintoma:** 11 Mb/s medidos com 7 Mb/s configurados. **Causa:** o tipo de mídia só declarava a
taxa média, que é dica, e a amostra ia carimbada com o fps nominal (53 quadros reais andando como
60). **Correção:** modo de taxa explícito, baixa latência e tempo real pelo `ICodecAPI`, e a hora
real da captura na amostra.

### A tabela de taxa

| Qualidade | Taxa |
|---|---|
| 720p60 | 5 Mb/s |
| 1080p60 | 10 Mb/s |
| 1440p60 | 20 Mb/s |
| 2160p60 | 40 Mb/s |

Subiu depois que o controle passou a ser respeitado: com o excesso removido, 1080p60 caiu para
6,5 Mb/s reais e a imagem piorou visivelmente. Jogo a 60 quadros é o pior caso do H.264.

### A taxa que acompanha a perda

O governador (`media::BitrateGovernor`) baixa o alvo do encoder quando os NACK de uma janela
passam de 5% e sobe devagar quando a perda some, até o piso de 35% da tabela; perda que continua
no piso desce a resolução (720p, depois 720p30). Por que perda e não REMB:
[DECISOES.md](DECISOES.md).

### Recuperação de perda

**De quem transmite até o servidor:** o servidor pede o pacote de volta (NACK por SRTCP) e o app
reenvia do histórico (1024 pacotes, a janela anti-repetição do SRTP); o pedido de quadro-chave
(PLI) também é atendido, com espaço de 2 s entre dois, que cresce até 4 s se os pedidos se
repetem.

**Do servidor até quem assiste:** o `PlainReceiver` segura quem chegou adiantado, pede de novo o
que faltou com prazos pela ida e volta medida (`shared/media/src/recovery.rs`), e recebe o reenvio
pelo RTX que o `consumePlain` anuncia. Buraco que passa do prazo vira PLI. Do lado do app, o
quadro que ficou de fora da fila ou que o decodificador recusou também pede quadro-chave, de novo
a cada 1 s enquanto a imagem não volta.

### O buffer de chegada (0.1.14)

Numa rede com perda, cada pacote pedido de novo segura o fluxo uma ida e volta, e os quadros de
trás chegam em bolo — medido em 02/10: ~7 reenvios por segundo, bolos de 44 a 70 quadros e só ~27
imagens por segundo na tela, aos solavancos. O `media::Playout` mostra cada quadro no horário do
relógio do RTP de quem transmite, com a espera do maior atraso recente (até 0,5 s, descendo
devagar).

### Quem assiste não fica para trás (0.1.13)

Uma thread única decodificava e convertia cada quadro (~15 ms); com duas telas 1080p60 a fila
enchia e a imagem ficava 4,1 s atrás. Hoje é uma thread por tela, quadro que vai ser substituído
passa pelo decodificador sem virar imagem, e no Windows o decodificador é o da placa (DXVA):
0,2–0,4 s de atraso.

### Intervalo de quadro-chave

Quatro segundos no Windows: o periódico é só rede de segurança, porque quem perde pacote pede um
na hora; com 2 s, somado aos pedidos, saía um por segundo e a transmissão de quem tem upload fraco
travava. Um segundo no Linux: o `gst-launch` da captura não atende pedido de fora, então o
periódico é a única recuperação.

### O caminho que muda de endereço (0.1.16)

O `comedia` prende o transporte ao primeiro endereço de quem manda; se o roteador troca de
endereço no meio, tudo o que vem do novo é descartado. O app percebe (5 s mandando sem RTCP de
volta; ou o servidor recebendo a tela, pelo `producerReceiving`, e nada chegando aqui) e abre um
transporte novo com chave nova. A manutenção do caminho de chegada sai a cada 5 s.

## Como medir

`cargo run -p core-app --example room -- <wss> <sala> watch 40` imprime, por segundo, fps,
quadros-chave, pacotes, recuperados, **perdidos** (o número que importa: buraco largado é
quadro-chave pedido) e o atraso de chegada e de decodificação. Rodar o binário velho e o novo na
mesma sala é o A/B.

## O que ainda não existe

- **Estimativa de banda por atraso** (o GCC do WebRTC): o governador só reage depois que há perda,
  e não à fila crescendo antes dela.
- **Simulcast:** um espectador com download ruim pede quadro-chave para todos, e todos recebem a
  mesma taxa.
- **Histórico de reenvio maior em 4K:** 1024 pacotes cobrem ~0,24 s a 40 Mb/s; precisa de um fluxo
  RTX próprio de quem transmite, que muda o SFU.
