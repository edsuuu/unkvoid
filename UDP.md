# UDP: quantas portas, e por que não dá para apertar

O que a rede precisa oferecer para o app não ficar sem porta, e o que acontece
quando ela aperta.

Escrito depois de uma noite perdida com um sintoma que não parecia de rede: a
transmissão subia, todo contador marcava saúde, o socket aceitava cada byte, e
do outro lado não chegava nada. Era porta fechada.

## Duas faixas, para duas coisas diferentes

| Faixa | Protocolo | Quem usa | Quantas portas |
|---|---|---|---|
| 40000-40006 | UDP **e** TCP | Quem **assiste**, por WebRTC | uma por worker |
| 41000-42000 | UDP | Quem **transmite** ou **assiste pelo caminho nativo** (Linux), por RTP puro | uma por pessoa que envia, mais uma por pessoa que recebe no Linux |

São faixas separadas de propósito. O WebRtcServer do mediasoup não pode dividir
porta com o transporte plain, e não existe processo que sirva os dois.

### Quem assiste: uma porta por worker, e só

O WebRtcServer **multiplexa**. Um worker atende dez, cinquenta ou duzentos
espectadores na mesma porta, distinguindo um do outro pelo ICE. Sete workers,
sete portas: 40000 a 40006.

Essa faixa não cresce com o número de pessoas. Ela cresce com o número de
workers, que é `SFU_WORKERS` e é um por núcleo menos um — o núcleo que sobra é do
nginx, do php-fpm, do MySQL, do MinIO e do e-mail.

TCP na mesma faixa é o caminho reserva. Quem estiver numa rede que bloqueia UDP
— empresa, escola, alguns hotéis — só assiste por ele. Deixar o TCP fechado não
dá erro visível: o ICE tenta, não conecta, e a pessoa fica olhando para uma sala
sem imagem.

### Quem transmite: uma porta por sentido

Cada pessoa que compartilha a tela recebe **um** transporte plain de envio, e nele
cabem vídeo e áudio juntos — o `rtcpMux` junta até o RTCP na mesma porta; quem
participa da voz pelo Linux, que também recebe por RTP puro, gasta **duas** portas
(envia e recebe), e a faixa é de 64 por worker.

O tamanho da faixa é o teto de transmissões simultâneas.

## A conta

```
teto por worker      = SFU_PLAIN_PORTS          (hoje 64)
teto do servidor     = SFU_WORKERS × SFU_PLAIN_PORTS
faixa a abrir        = SFU_PLAIN_PORT  até  SFU_PLAIN_PORT + (workers × portas) - 1
```

Com os valores de hoje, 7 workers e 64 portas, o teto é 448 transports plain
simultâneos (cada pessoa na voz pelo Linux usa dois: envio e recepção) e a faixa mínima é 41000-41447 — dentro da regra 41000-42000 que o firewall já abre.

**Mas o teto que se sente não é esse.** Uma sala inteira vive num worker só — o
registro manda cada sala nova para o worker com menos salas, e ela fica lá. Então
o limite prático é `SFU_PLAIN_PORTS` transmissões **por sala**: hoje, 64 pessoas
compartilhando a tela ao mesmo tempo na mesma sala — muito além do teto de banda,
que é o que se sente primeiro (ver [infra/INSTALAR-VPS.md](infra/INSTALAR-VPS.md)).

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
configuração.

O que **precisa** funcionar é a saída UDP. Uma rede que só deixa passar TCP nas
portas 80 e 443 não transmite de jeito nenhum, e assiste só pelo caminho reserva
em TCP.

## Conferir, e conferir de fora

De dentro da máquina não dá para saber. O `ss` mostra o processo escutando
mesmo quando o firewall descarta tudo antes.

TCP responde a sondagem:

```bash
nc -z -w 4 unkvoid.com 40000 && echo aberta || echo fechada
```

UDP não responde nada, então o teste precisa de captura do outro lado:

```bash
# na VPS, deixe rodando
sudo tcpdump -nn -i any 'udp and (dst portrange 40000-40006 or dst portrange 41000-42000)'

# da sua máquina
for p in 40000 40006 41000 41447 42000; do printf teste | nc -u -w0 unkvoid.com $p; done
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

Ver também [infra/INSTALAR-VPS.md](infra/INSTALAR-VPS.md) para a tabela de portas do
firewall do painel, [SERVIDOR.md](SERVIDOR.md) para por que ele é o primeiro suspeito, e
[REDE.md](REDE.md) para o caminho da imagem.
