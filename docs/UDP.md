# UDP: quantas portas, e por que não dá para apertar

O que a rede precisa oferecer para o app não ficar sem porta, e o que acontece
quando ela aperta.

Escrito depois de uma noite perdida com um sintoma que não parecia de rede: a
transmissão subia, todo contador marcava saúde, o socket aceitava cada byte, e
do outro lado não chegava nada. Era porta fechada.

## Duas faixas, para duas coisas diferentes

| Faixa | Protocolo | Quem usa | Quantas portas |
|---|---|---|---|
| 41000-42000 | UDP | o app nativo, por RTP puro: quem transmite e quem assiste | até duas por pessoa: uma de envio e uma de chegada |
| 40000 até 40000 + workers − 1 (hoje 40000-40002) | UDP **e** TCP | o WebRTC do app Tauri de antes | uma por worker |

São faixas separadas de propósito: o WebRtcServer do mediasoup não divide porta com o transporte
plain.

### O app nativo: uma porta por sentido

Cada pessoa recebe **um** transporte plain de envio — tela, som da tela, microfone e câmera
juntos, cada um com o seu SSRC, e o `rtcpMux` junta até o RTCP na mesma porta — e **um** de
chegada, por onde vem tudo o que ela assiste e ouve. Quem está na voz gasta duas portas.

O tamanho da faixa é o teto de transportes simultâneos.

### O WebRTC de antes: uma porta por worker

O WebRtcServer **multiplexa**: um worker atende muitos espectadores na mesma porta,
distinguindo um do outro pelo ICE. A faixa cresce com `SFU_WORKERS` (núcleos menos um; o que
sobra é do nginx, do php-fpm, do MySQL, do MinIO e do e-mail), não com as pessoas. O TCP na
mesma faixa era o caminho reserva de quem está numa rede que bloqueia UDP.

## A conta

```
teto por worker      = SFU_PLAIN_PORTS          (hoje 64)
teto do servidor     = SFU_WORKERS × SFU_PLAIN_PORTS
faixa a abrir        = SFU_PLAIN_PORT  até  SFU_PLAIN_PORT + (workers × portas) - 1
```

Com os valores de hoje, 3 workers e 64 portas, o teto é 192 transportes plain simultâneos e a
faixa mínima é 41000-41191 — dentro da regra 41000-42000 que o firewall já abre.

**Mas o teto que se sente não é esse.** Uma sala inteira vive num worker só — o registro manda
cada sala nova para o worker com menos salas, e ela fica lá. Então o limite prático é
`SFU_PLAIN_PORTS` transportes **por sala**: 64, ou 32 pessoas na voz (cada uma manda e recebe) —
além do teto de banda, que é o que se sente primeiro (ver [INSTALAR-VPS.md](INSTALAR-VPS.md)).

Foi por isso que o valor já foi 1, e duas pessoas nunca conseguiram compartilhar
juntas: o segundo a clicar recebia `no more available ports`.

## Por que a faixa é larga e por que apertar quebra calado

**O mediasoup sorteia.** Ele escolhe uma porta ao acaso dentro da faixa do
worker e confere uma coisa só: se ela está livre nesta máquina. Ele não tem como
saber se o firewall a deixa passar.

Uma porta livre e bloqueada é aceita sem reclamar. O transporte é criado, o
servidor devolve o endereço, o app começa a mandar RTP, o kernel aceita cada
pacote, e nada chega. Do lado de quem transmite todo contador continua subindo:
`sent` cresce, `sendErrors` fica em zero, `sendDropped` fica em zero. Do lado de
quem assiste, tela preta.

Com metade da faixa aberta, o sintoma é pior do que uma falha limpa: funciona
uma vez em cada duas. Preto, para e recomeça, pega, muda a qualidade, morre de
novo. Parece problema de resolução, de codec, de rede da pessoa — e é porta.

Abrir a faixa larga custa uma linha na regra do firewall. O painel da Contabo
aceita intervalo (`41000-42000`) e lista separada por vírgula, então **não é uma
regra por porta**. Deixar 42000 como fim dá folga para subir `SFU_PLAIN_PORTS`
depois sem voltar no painel.

## O que o app avisa hoje

O servidor derruba a transmissão que passa trinta segundos sem receber um pacote
e **avisa quem transmite**, com `producerDead`. O app para de compartilhar e
mostra o erro em vez de ficar eternamente "ao vivo".

Isso não conserta a rede, mas transforma "não funciona e ninguém sabe por quê"
em "a transmissão não chegou ao servidor: nenhum pacote entrou em 30 s".

Se a faixa estiver certa e ainda aparecer essa mensagem, o problema é da rede de
quem transmite, não do servidor.

## Do lado de quem usa o app

Aqui não há porta para abrir, e é de propósito.

Quem transmite **sai** de uma porta efêmera qualquer para a porta do servidor. O
`comedia` do mediasoup aprende o endereço de origem no primeiro pacote que
chega, então nada na máquina de quem transmite precisa ser alcançável de fora.
Roteador doméstico, NAT duplo, CGNAT da operadora: todos funcionam sem
configuração. Se o endereço mudar no meio (o provedor reconectou, o roteador reiniciou), o
`comedia` continua preso ao antigo; o app percebe — 5 s mandando sem RTCP de volta, ou o servidor
recebendo a tela e nada chegando — e abre um transporte novo com chave nova.

O que **precisa** funcionar é a saída UDP. Uma rede que só deixa passar TCP nas
portas 80 e 443 não transmite nem assiste pelo app nativo.

## Conferir, e conferir de fora

De dentro da máquina não dá para saber. O `ss` mostra o processo escutando
mesmo quando o firewall descarta tudo antes.

TCP responde a sondagem:

```bash
nc -z -w 4 unkvoid.com 40000 && echo aberta || echo fechada   # a faixa do WebRTC tem TCP
```

UDP não responde nada, então o teste precisa de captura do outro lado:

```bash
# na VPS, deixe rodando
sudo tcpdump -nn -i any 'udp and (dst portrange 40000-40002 or dst portrange 41000-42000)'

# da sua máquina
for p in 40000 40002 41000 41191 42000; do printf teste | nc -u -w0 unkvoid.com $p; done
```

Toda porta enviada tem de aparecer no tcpdump. A que não aparecer está
bloqueada, e é ela que vai virar uma transmissão preta um dia.

Vale repetir depois de mexer no painel: a regra leva alguns minutos para valer, e
testar cedo demais dá um falso negativo que manda você caçar no lugar errado.

## Se precisar de mais transmissões simultâneas

1. Suba `SFU_PLAIN_PORTS` no `ecosystem.config.cjs`.
2. Confira que `SFU_PLAIN_PORT + (SFU_WORKERS × SFU_PLAIN_PORTS) - 1` continua
   dentro do que o firewall abre.
3. Reinicie o SFU e refaça o teste de UDP acima.

Subir `SFU_WORKERS` também aumenta o teto total, mas não o de uma sala: a sala
continua num worker só.

Ver também [INSTALAR-VPS.md](INSTALAR-VPS.md) para a tabela de portas do
firewall do painel, [SERVIDOR.md](SERVIDOR.md) para por que ele é o primeiro suspeito, e
[REDE.md](REDE.md) para o caminho da imagem.
