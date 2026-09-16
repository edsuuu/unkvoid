# Segurança

O que está protegido, o que não está, e por quê. Escrito para ser escolha e não
acidente: quase tudo aqui é segurança de portador, e isso é adequado ao produto
desde que esteja dito em voz alta.

## O que já é forte

**A imagem viaja cifrada.** SRTP com AES-128 e autenticação HMAC-SHA1, chave
sorteada por transmissão e nunca reutilizada. Quem interceptar o tráfego UDP vê
ruído. A chave viaja pelo WebSocket, dentro do TLS.

**O caminho de volta também.** O pedido de quadro-chave que o servidor manda vem
em SRTCP, e o app o descarta quando a autenticação falha. É justamente isso que
impede um estranho de nos fazer gastar quadro-chave a cada pacote forjado.

**A chave de retomada é por sessão.** Quem cai e volta prova ser a mesma pessoa
com um segredo de 16 bytes que sai apenas na resposta do `join`, nunca no
broadcast. Se o `peerId` bastasse, qualquer um derrubaria qualquer um.

## O elo fraco: o código da sala

O código é digitado à mão. O servidor aceita de 3 a 32 caracteres de `a-z0-9`
com hífens, e **cria a sala se ela não existir**. Na prática as salas se chamam
"teste", "sala1", "amigos".

Esse código é a única credencial que existe. Quem adivinhar um nome comum entra,
e a criptografia não impede nada — a pessoa entrou pela porta da frente com a
chave legítima.

É o buraco real do sistema, maior que qualquer coisa que criptografia adicional
resolva.

## O que foi construído contra isso

**Aviso de quem entra.** Um cartão no canto, que some sozinho, para entrada,
saída e início de transmissão. Não impede a entrada; transforma um problema
invisível em visível, que é o primeiro passo para alguém reagir. Também vai para
o log, porque quem chega depois precisa saber quem esteve na sala.

**Tranca.** A sala trancada recusa quem ainda não está dentro. Resolve o caso
comum: as pessoas certas já entraram, e a partir dali ninguém mais entra, saiba
o código ou não. Liberada para qualquer pessoa de dentro, porque quem já entrou
é confiado por definição e trancar não age sobre quem está lá. Reconexão passa
pela tranca de propósito — expulsar por oscilação de rede seria pior do que
tranca nenhuma.

**Dono e expulsão.** A primeira instalação a entrar numa sala vazia fica dona.
Só ela expulsa. Expulsar bane a instalação **enquanto a sala existir**: sem essa
metade não seria expulsão, porque a pessoa continua sabendo o código e volta no
segundo seguinte. O banimento morre junto com a sala, que é o tempo de vida
certo — uma lista que durasse além dela seria um cadastro de pessoas.

Remover quem já parou de transmitir e ficou órfão continua liberado para
qualquer um: é faxina, e não tira ninguém de lugar nenhum.

### Por que a identidade é a instalação

O `peerId` é sorteado a cada conexão. Um dono amarrado a ele perderia a sala na
primeira oscilação de rede. A chave de retomada sobrevive à queda mas morre ao
fechar o app.

O app gera um UUID na primeira execução e guarda no `localStorage`. Sobrevive a
reconectar e a fechar o app, então quem criou a sala continua dono ao voltar.

**Não é prova de identidade.** Quem editar o próprio app manda o UUID que
quiser. Vale exatamente o que o código da sala vale. Esconder o botão de
expulsar de quem não é dono é desenho de interface, não autorização — o servidor
recusa a ação de qualquer jeito.

## Ofuscação, e por que ela não entra

A ideia avaliada foi embutir uma chave simétrica por build e ofuscar o binário
com LLVM, para o servidor só aceitar clientes legítimos.

**Não entrega o que parece entregar.** Uma chave distribuída para todo mundo é
conhecida por qualquer pessoa que baixe o app e o abra num desmontador.
Ofuscação transforma minutos de trabalho em algumas horas, e é isso. O custo é
permanente: quando o app quebrar na máquina de alguém, o relatório de erro vem
ilegível.

Vale como camada contra curiosos, nunca como defesa. E o esforço rende muito
mais no elo fraco acima do que aqui.

## O que falta, em ordem de valor

1. **Códigos com entropia por padrão.** Oferecer um código gerado, deixando o
   nome próprio como escolha. Não para quem insiste, mas remove a classe
   "sala1" inteira.
2. **Identidade de verdade.** Conta e convite. É o único caminho que sai da
   segurança de portador, e é outro tamanho de projeto.
3. **Limite de tentativas de entrada.** Hoje nada impede varrer nomes prováveis.
