# Segurança

O que está protegido, o que não está, e por quê. Escrito para ser escolha e não acidente.

## O que é cifrado, e até onde

| | Em trânsito | Guardado | Ponta a ponta |
|---|---|---|---|
| Site, API, login | TLS (nginx, 443) | senha com bcrypt (12 rodadas); token do Sanctum só como hash no banco | — |
| Chat e mensagens diretas | TLS até o Laravel; WSS no Reverb | **texto puro** no MySQL | **não** |
| Imagens (foto, ícone, chat) | TLS | bucket privado no MinIO; toda URL é assinada e vence em 2 h | **não** |
| Sinalização do SFU | WSS | nada é guardado | — |
| Tela, câmera e voz pelo app (RTP puro) | SRTP: AES-128 com HMAC-SHA1, chave sorteada por transmissão | nada é guardado | **não** |
| Assistir, e mic/câmera no Windows e macOS (WebRTC) | DTLS-SRTP | nada é guardado | **não** |
| Instaladores | TLS + assinatura minisign conferida pelo app; APT assinado com GPG | bucket privado, URL de 1 h | — |

Em uma frase: **tudo viaja cifrado, nada é cifrado de ponta a ponta.** Quem intercepta a rede
vê ruído. Quem controla a VPS vê tudo: o SFU tem as chaves SRTP de cada transmissão (ele
repassa sem decodificar, mas poderia abrir) e o banco guarda as mensagens em texto.

### Por que não há ponta a ponta

Decisão do dono em 16/09/2026: **não agora**. Em canal de servidor nem poderia: moderação e
auditoria precisam ler a mensagem. O desenho guardado para quando for a hora:

- só em mensagem direta; chave por par (X25519 gerada no cadastro, pública em `users`, privada
  guardada na máquina); corpo cifrado em `direct_messages.body`, e o servidor guarda opaco;
- mídia é outro projeto: o SFU rotearia sem ver a imagem, com o cabeçalho RTP aberto e o
  conteúdo cifrado por quadro (é o que o DAVE do Discord faz). Com os clipes fora do código,
  nada no servidor precisa mais decodificar vídeo.

As duas coisas mexem no esquema do banco e no contrato.

## O que já é forte

**A mídia viaja cifrada.** SRTP com chave sorteada por transmissão e nunca reutilizada. A
chave viaja pelo WebSocket, dentro do TLS.

**O caminho de volta também.** O pedido de quadro-chave e o pedido de reenvio que o servidor
manda vêm em SRTCP, e o app os descarta quando a autenticação falha. É isso que impede um
estranho de nos fazer gastar quadro-chave a cada pacote forjado.

**Quem decide permissão é um lugar só.** O Laravel calcula quem pode o quê e assina um token
de 60 s com o que a pessoa pode produzir (`speak`, `stream`, `video`). O SFU só confere a
assinatura e recusa o resto; o app só esconde botão. Na retomada de uma sessão que caiu vale o
token novo, não o da entrada.

**Laravel e SFU só conversam assinado.** HMAC com o `SFU_SECRET` sobre hora, método, caminho e
corpo, com janela de 300 s. Sem o segredo (ou com menos de 32 caracteres) o SFU não sobe: é
melhor fora do ar do que aberto.

**Uma conta, uma sessão no SFU.** Entrar de novo derruba a sessão anterior daquela conta. O
visitante só é substituído pela chave de retomada, que é um segredo de 16 bytes que sai apenas
na resposta do `join`, nunca no broadcast: se o `peerId` bastasse, qualquer um derrubaria
qualquer um.

**Canal que a pessoa não enxerga não existe para ela.** Sem `VIEW_CHANNEL` o canal não aparece
na árvore, não devolve mensagem, o Reverb recusa a assinatura e não sai token de voz.

**Atualização assinada.** O app confere a assinatura minisign com a chave pública que carrega
por dentro; a privada não fica na VPS. O Linux confia na GPG do repositório APT.

**O repositório é público e o runner mora na VPS.** Os fluxos de deploy e o build do Linux
rodam num runner do GitHub Actions instalado na própria VPS. O que impede um fork de executar
código lá é que **nenhum fluxo dispara em `pull_request`**: só em push na `main`, em tag e em
disparo manual, e ainda conferem `github.actor`. Acrescentar um gatilho de PR a qualquer fluxo
que use `runs-on: self-hosted` entrega a VPS a quem abrir um PR.

**Tentativa tem teto.** Login, cadastro, convite, mensagem, amizade e envio de foto têm
`throttle` por rota; o SFU tem teto de conexões novas por IP.

## A sala por código: o elo fraco, de propósito

O código é a única credencial. Quem tem o código entra pela porta da frente com a chave
legítima, e criptografia nenhuma impede isso.

- O app **sorteia** 12 caracteres de `a-z0-9` ao criar a sala: adivinhar não é viável, e o teto
  de conexões novas por IP (`SFU_CONNECTIONS_PER_MINUTE`, padrão 20) impede varrer.
- O servidor ainda aceita código digitado à mão (3 a 32 caracteres). Quem escolhe "sala1"
  escolheu uma sala que qualquer um acha.
- Código de 26 caracteres é recusado sem token: é o formato do id de canal de voz, e sem isso
  alguém entraria num canal de servidor sem passar pelo Laravel.
- Quem entra fica registrado em `guest_accesses` (nome, sala, IP, entrada e saída). Não há tela
  que mostre isso por enquanto.
- O aviso de quem entra, sai e começa a transmitir aparece para a sala inteira: não impede a
  entrada, mas transforma um problema invisível em visível.
- **Não há dono, tranca nem expulsão na sala por código.** O `installId` é escolhido pelo
  próprio app e a sala inteira o recebe: valer como identidade deixaria qualquer um derrubar
  qualquer um. Só se remove da lista quem já caiu e ficou órfão. Quem precisa expulsar, banir
  ou esconder canal usa um servidor, onde existe conta.

## O que não está protegido

1. **O token do app mora em texto puro.** O token do Sanctum fica no `localStorage` da webview
   (`unkvoid:token`). Qualquer programa rodando como o usuário lê o arquivo. O caminho de saída
   é guardar no cofre do sistema (DPAPI no Windows, Keychain no macOS, libsecret no Linux) por
   um comando do Rust.
2. **O retorno do login com Google pode ser interceptado na própria máquina.** O token volta
   por `unkvoid://login?token=&state=`; o `state` sorteado impede uma página qualquer de logar
   a pessoa em conta alheia, mas outro programa que registre o mesmo esquema recebe o endereço.
   O caminho de saída é PKCE, com o segredo nascendo dentro do app.
3. **Quem foi expulso ainda ouve até reconectar, se o cliente for modificado.** O Reverb 1.11
   não derruba a assinatura de quem já estava inscrito; o app oficial sai dos canais no
   `MemberRemoved`. Fechar isso é fazer os eventos não levarem conteúdo — muda o contrato, e é
   pergunta aberta no [ESTADO.md](ESTADO.md).
4. **Quem controla a VPS lê tudo** — ver "Por que não há ponta a ponta".

## Ofuscação, e por que ela não entra

A ideia avaliada foi embutir uma chave simétrica por build e ofuscar o binário com LLVM, para o
servidor só aceitar clientes legítimos.

**Não entrega o que parece entregar.** Uma chave distribuída para todo mundo é conhecida por
qualquer pessoa que baixe o app e o abra num desmontador. Ofuscação transforma minutos de
trabalho em algumas horas, e é isso. O custo é permanente: quando o app quebrar na máquina de
alguém, o relatório de erro vem ilegível.

Vale como camada contra curiosos, nunca como defesa.

## Achou uma falha?

Não abra issue pública: escreva para **contato@unkvoid.com**.
