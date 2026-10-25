# Criptografia de ponta a ponta da mídia

O desenho de mídia e a integração com o app ainda são proposta. A fase 2 — estado MLS em memória
no `core` — está implementada e verificada abaixo; ela ainda não abre socket, não altera RTP e
não torna uma sala E2EE por si só.

## Escopo da primeira fase

A primeira fase cifra somente a mídia de voz, câmera e tela. O chat fica fora: ele precisa de
histórico para quem entra depois, busca e ferramentas de moderação. O Discord também mantém o
texto fora da cifra ponta a ponta.

Hoje, cada transmissão usa SRTP entre o app e o SFU. O PlainTransport recebe uma chave SRTP por
transmissão, e o mediasoup termina essa proteção para poder encaminhar os pacotes. Isso protege o
trecho de rede, mas o SFU consegue ler a mídia. E2EE acrescenta outra camada: a própria carga
codificada é cifrada no app e só os outros apps da chamada têm a chave. O SRTP continua por fora
como proteção do transporte; o SFU não recebe nem deriva as chaves E2EE.

## Caminho de áudio e vídeo

O ponto de cifra fica no app, depois do encoder e antes do empacotamento RTP (`PlainSender`,
`native/shared/media/src/plain.rs:398-445` para vídeo e `594-622` para Opus). O encoder continua
comprimindo uma vez; o quadro cifrado também é enviado uma vez e o SFU o replica. Não se cifra a
textura nem se decodifica/recomprime no SFU.

- Opus: cifra o payload inteiro de cada pacote.
- H.264: mantém em claro apenas os cabeçalhos de NAL necessários ao transporte e à recuperação.
  Isso inclui o cabeçalho de cada NAL, o cabeçalho e os comprimentos das unidades agregadas em
  STAP-A e os cabeçalhos de fragmentação FU-A. O restante do payload é cifrado. A implementação
  deve validar a estrutura e autenticar os bytes
  em claro como dados associados; não pode confiar em um tipo de NAL adulterado. Hoje o remetente
  identifica o início de NAL e keyframe em `plain.rs:70-103`, e o pedido de quadro-chave depende
  do tipo de NAL. O parser do SFU também precisa continuar vendo o mínimo necessário para o
  mediasoup identificar keyframes.

Usaria ChaCha20-Poly1305 para a carga de mídia: é AEAD padronizado, tem tag de autenticação e
desempenho consistente sem depender de AES acelerado por hardware. AES-GCM também é rápido em
máquinas com AES-NI/ARM crypto e é amplamente implantado, mas seu desempenho varia mais por
hardware. O RFC do GCM exige disciplina estrita de nonce único por chave; ChaCha20-Poly1305 também
proíbe reutilizar nonce com a mesma chave. A decisão final deve ser confirmada por benchmark nos
PCs mínimos do projeto, não por estimativa.

Cada quadro/pacote cifra uma única vez e recebe uma tag de 16 bytes. O nonce de 96 bits será
derivado de um prefixo exclusivo da origem e de um contador monotônico de 64 bits. Cada origem
(microfone, câmera, tela e áudio da tela) terá chave, contador e prefixo separados; reiniciar ou
trocar a chave cria uma nova época e zera o contador. O contador, a época, a origem e os campos
RTP que precisam ser lidos pelo SFU entram nos dados associados autenticados. Reordenação e perda
não podem fazer o receptor aceitar nonce repetido; contadores regressivos/duplicados são
descartados. Nunca reutilizar chave/nonce, inclusive depois de retomar uma sessão.

## Chaves e participantes

Não recomendo lançar um protocolo caseiro de chaves por pares. A ideia inicial — par X25519 por
sessão, segredo derivado com HKDF e chave de envio de cada participante cifrada individualmente
para cada outro — é viável como protótipo, mas exige implementar à mão associação de identidade,
ordenação de mensagens, concorrência, entrada/saída, troca de época, recuperação de estado e
proteção pós-comprometimento. O custo de distribuição e atualização cresce com o número de
participantes. X25519 e HKDF são primitivas, não resolvem esses problemas de protocolo.

Recomendação: adotar MLS (RFC 9420) para a associação autenticada do grupo e suas épocas, usando
uma implementação auditada que possa ser mantida no Rust do app. O MLS administra as chaves de
acordo X25519 dos membros dentro do grupo e atualiza o segredo quando membros mudam; o app deriva,
pelo exporter MLS e rótulos distintos, uma
chave de envio por participante, origem e época. Isso preserva o formato de mídia por sender key,
sem distribuir manualmente cada chave a todos. A entrada e a saída criam época nova: quem chega
recebe o estado atual, não o segredo anterior; quem sai não obtém o segredo novo. A RFC descreve
MLS como um protocolo de grupo com forward secrecy e post-compromise security; ainda assim, é
preciso excluir participantes desconectados e avançar épocas corretamente.

O DAVE do Discord também usa MLS para o grupo e deriva uma chave de mídia por remetente. É uma
referência de desenho, não uma promessa de compatibilidade de protocolo.

MLS pressupõe autenticação das credenciais dos participantes. X25519 sozinho não autentica a
chave pública. A primeira versão pode usar o usuário autenticado pelo Laravel como nome de
credencial, mas a confiança final ainda depende de como a chave pública é vinculada a essa conta.
Um servidor malicioso que possa substituir as chaves durante a distribuição consegue se passar
por participante enquanto ninguém confere as chaves fora do servidor.

### Código de privacidade

O app mostra um código curto calculado de forma determinística sobre a transcrição autenticada da
chamada: identificador da sala, época atual e lista ordenada de identidade e chave pública de cada
membro. Todos comparam o mesmo código por outro canal, como uma conversa presencial ou uma
mensagem independente. Uma mudança de membro/época muda o código e exige nova comparação.

Sem essa comparação, o servidor pode apresentar uma chave diferente a cada participante, ou
substituir uma chave durante uma reconexão. O tráfego continua cifrado, mas a chamada pode não ser
ponta a ponta entre as pessoas esperadas. O código não substitui a autenticação MLS nem protege
quem compara o valor no mesmo servidor comprometido.

## Sinalização opaca

A fase 4a implementa no SFU as ações `e2eeJoin` (pedido de entrada e KeyPackage),
`e2eeCommit` e `e2eeWelcome`, detalhadas em [CONTRATO.md](CONTRATO.md). Cada envelope é
versionado, carrega `group`, `epoch` quando aplicável e `payload` MLS base64url. O SFU vincula os
eventos ao `peerId`/`userId` autenticado, limita a 64 KiB por bloco e a 20 blocos/256 KiB por peer
a cada 10 s, e nunca desserializa o payload.

O SFU ordena atomicamente e exclusivamente o primeiro Commit da época guardada para o grupo:
responde aceito/recusado ao autor, avança a época só no aceite e só então aceita o Welcome desse
autor para a mesma época. Assim o núcleo pode confirmar ou descartar seu Commit pendente sem
bifurcar o grupo. A época no envelope é controle do relé, não prova: todos os clientes conferem a
época e a transcrição de verdade no MLS antes de publicar mídia.

Para uma Room ativa o SFU guarda apenas a época e o digest do último Commit aceito — o bastante
para serializar e responder a um reenvio idempotente. Não guarda KeyPackage, Commit nem Welcome.
Isto é intencional: alguém que entra tarde não deve receber histórico de grupo; manda KeyPackage
novo e recebe Welcome novo após um Commit aceito. Quando a Room esvazia, esse metadado some junto.
Mensagens E2EE não passam para webhook, Laravel, logs ou `presence`.

## Mudanças necessárias quando sair do desenho

| Peça | Mudança futura |
|---|---|
| Núcleo | manter estado MLS por chamada, identidade/credenciais, épocas e código de privacidade; bloquear publicação se o estado não estiver pronto; apagar segredos ao sair. |
| `media` | transformar payload Opus/H.264 depois do encoder e antes de `PlainSender`; decifrar após receber e antes dos decoders; validar cabeçalhos, tags, contadores e limites sem alterar os cabeçalhos RTP que o SFU usa. |
| SFU | retransmitir mensagens E2EE opacas para membros autorizados; não persistir payload de mídia nem chave; continuar encaminhando os pacotes SRTP sem saber as chaves internas. |
| Laravel | anunciar suporte/versão e política E2EE da sala, fornecer a identidade autenticada e autorizar a sinalização; nunca guardar chaves privadas ou segredos de grupo. |
| Contrato | definir mensagens, limites, épocas, políticas de compatibilidade e recusa de downgrade; atualizar `docs/CONTRATO.md` antes de implementar. |

Na 4a o contrato usa `e2eeJoin` (KeyPackage), `e2eeCommit` e `e2eeWelcome`. Propostas MLS e
mensagens de aplicação continuam fora do escopo até o app precisar delas; quando entrarem, usam a
serialização TLS da implementação MLS escolhida, nunca uma serialização paralela inventada.

## Compatibilidade, gravação e clipes

Não aceitar downgrade silencioso. Quando a política E2EE estiver ativa, clientes antigos ou sem
suporte não publicam nem consomem mídia na sala; a interface explica que é preciso atualizar.
Uma sala sem E2EE só pode existir como modo explicitamente separado, informado a todos antes de
entrar. Não misturar áudio claro e cifrado na mesma chamada protegida.

Clipes locais podem continuar se forem capturados e gravados no próprio cliente antes da cifra de
saída ou depois da decifra de entrada, com indicação clara e consentimento. SFU, servidor ou
gravação remota não conseguem produzir gravação compreensível sem receber chaves; não criar
exceção de chave para gravação. O chat e seu histórico continuam no servidor e fora do escopo.

## Custo e critério de desempenho

ChaCha20-Poly1305 percorre os bytes codificados uma vez e acrescenta uma tag de 16 bytes por
quadro/pacote, além de cabeçalho de contador. A câmera/tela continua sendo comprimida uma vez; não
se copia textura da GPU para CPU para cifrar, porque a cifra atua apenas no quadro já comprimido.
O custo deve ser medido como tempo p50/p95/p99 por tamanho de quadro e por origem, em 720p/1080p
30/60, áudio simultâneo e CPU de jogo ocupada. Meta: a cifra não atrasar o envio nem retirar fps
do jogo; falha de autenticação descarta o pacote, sem tentar decodificar texto parcial. O limite
de MTU e fragmentação RTP continua sendo aplicado depois da cifra.

## Fases propostas e testes

1. **Decisão e ameaça:** escolher biblioteca MLS, suite, identidade e política de sala. Revisão
   externa do desenho; testes de vetores das primitivas e geração de chaves, sem mídia.
2. **Grupo e sinalização:** implementar KeyPackage, add/remove, commits, reconexão e épocas.
   Testar dois e muitos participantes, mensagens duplicadas/atrasadas/reordenadas, conflito de
   commit, participante removido, reinício e código de privacidade igual entre clientes.
3. **Payload de mídia:** cifra/decifra determinística de Opus e H.264, incluindo STAP-A, FU-A,
   keyframe, fragmentação e RTP fora de ordem. Testar alteração de cabeçalho, tag inválida,
   nonce repetido, contador regressivo e épocas erradas; o decoder nunca recebe payload parcial.
4. **Integração SFU/Laravel:** encaminhar bytes opacos, autorizar membros e recusar cliente sem
   versão/sala incompatível. Testar que chaves e plaintext não aparecem em logs, webhook, API,
   dump do SFU ou pacotes depois de remover SRTP no teste local.
5. **Verificação e interface:** exibir e comparar código de privacidade; testar mudança de membro,
   reconexão e troca de época. Explicar claramente falha de verificação e nunca indicar “seguro”
   antes de convergir.
6. **Desempenho e lançamento:** medir custo e latência nos Windows/Linux suportados, áudio com
   microfone, câmera e tela simultâneos, jogo aberto e perda de rede. Liberar por versão com
   compatibilidade estrita e rollback sem downgrade oculto.

## Fase 2 implementada — grupo MLS, sem integração de rede

`native/shared/core/src/e2ee_group.rs` usa `openmls 0.9.1`,
`openmls_rust_crypto 0.6.0` e `openmls_basic_credential 0.6.0` com a única suite aceita
`MLS_128_DHKEMX25519_CHACHA20POLY1305_SHA256_Ed25519`. A identidade é Ed25519 nova por
sessão/dispositivo; o Basic Credential continua sendo a conta autenticada. O estado e a chave de
assinatura são somente de memória nesta fase e não atravessam a API, o SFU ou o Laravel.

O envelope serializável `E2eeSignal` é `{ version: 1, group_id: bytes, epoch: u64, kind,
sender_account_id, payload: bytes }`, onde `kind` é `key_package`, `commit` ou `welcome`.
`payload` é a serialização TLS do MLS e fica opaca para a entrega. `epoch` no envelope **não é
autenticado**: é só metadado do relé para ordenar/repetir; a época, a árvore e a transcrição que
valem são as verificadas pelo MLS ao processar Commit ou Welcome.

Um Commit local agora fica pendente: `add_member` ou `remove_member` apenas o cria;
`confirm_pending` faz `merge_pending_commit` depois de o relé aceitar o único Commit da época;
`discard_pending` faz `clear_pending_commit` quando ele é recusado. Um Welcome de entrada fica
guardado no criador e só sai de `confirm_pending` após esse aceite. A regra cooperativa para
reduzir disputas é que o membro de menor leaf index ativo é o comitador (`designated_committer`);
se ele sair, o próximo menor assume. A regra não substitui a ordenação do SFU: cliente antigo ou
defeituoso ainda pode tentar outro Commit e deve receber recusa.

`members` expõe cada folha como `{ account_id, member_id, leaf_index }`. `member_id` é a chave
pública de assinatura da sessão MLS, não a conta. O contexto do exporter de mídia inclui
`(group_id, epoch, member_id, origin)`, então duas máquinas da mesma conta recebem chaves de
remetente diferentes e seus contadores iniciados em zero nunca reutilizam um nonce
ChaCha20-Poly1305. Remover uma conta remove todas as folhas/dispositivos dela no mesmo Commit.

O código de privacidade usa os 16 primeiros bytes de SHA-256, módulo `10^30`, exibido em seis
grupos de cinco dígitos (`00000 00000 00000 00000 00000 00000`). Ele cobre grupo, época e cada
par credencial/chave de assinatura, ordenado. São aproximadamente 100 bits de comparação, não os
~40 bits da versão de 12 dígitos.

Os testes de `core-app` cobrem convergência de 2 e 5 membros, Commit duplicado/atrasado/futuro,
remoção e entrada por época, alteração do código de privacidade, Commit inválido com motivo
estável, dois Commits concorrentes (aceita um, descarta o outro e só então libera Welcome) e duas
sessões da mesma conta com chaves de mídia distintas. A integração da fase 4b usa essa confirmação
para mídia em `Policy::Require`.

## Fase 4a implementada — relé MLS no SFU, sem app

O SFU implementa somente o caminho de sinalização em memória: repassa KeyPackage com o pedido de
entrada, ordena o primeiro Commit da época e entrega Welcome para o peer alvo só após a resposta
de aceite. A implementação e os eventos estão em `sfu/`; o contrato completo está em
[CONTRATO.md](CONTRATO.md). Não muda a política de salas nem ativa E2EE no app.

Para não guardar estado sensível, por grupo ativo ele conserva somente `currentEpoch`, o hash do
último Commit aceito e seu peer autor. Essa é a menor retenção que permite ordenar e confirmar um
reenvio perdido; KeyPackages, Commits e Welcomes não ficam no SFU. A pessoa que chega depois
manda KeyPackage novo e recebe Welcome novo após um Commit aceito, o que é mais seguro e útil do
que entregar um histórico MLS opaco. O estado desaparece quando a Room fica vazia.

## Fase 4b implementada — chamada cifrada no core

`core-app::Room` cria um `E2eeCall` por sessão de voz: o primeiro peer cria o grupo, os demais
enviam KeyPackage; o membro MLS ativo de menor leaf index comita adições e saídas. Commits só
entram no estado local após a resposta aceita do SFU, e Welcome só sai depois disso. Se a rede
recusar o Commit, o cliente descarta o estado pendente; commits recebidos passam pelo parser MLS,
e época futura pede novo KeyPackage/Welcome. Uma retomada que não preservou o peer reinicia a
identidade/grupo local e volta a negociar.

O caminho RTP cifra Opus após o encoder e H.264 após o encoder da placa, antes do packetizer; o
receptor remonta primeiro o payload e só então decifra antes do decoder. Cada chave é derivada por
época, MemberId de sessão e origem. A troca conserva a chave de recepção anterior por 3 s, apenas
para pacotes já em trânsito; o sender passa imediatamente à chave da nova época. `Room` recusa
abrir produtor enquanto a cifra não estiver estabelecida; payload claro é recusado por `Require`
antes de alcançar o decoder.

`producePlain` carrega o MemberId público e o SFU o ecoa no roster e em `consumePlain`; o cliente
confere a chave contra a árvore MLS antes de derivar a chave de recepção. `room.encryption` expõe
estado (`establishing`, `encrypted`, `failed`), motivo estável, código de privacidade e membros
verificáveis, sem material secreto.

O teste `live_room::two_then_three_clients_exchange_only_encrypted_opus` roda contra SFU local,
negocia dois e depois três clientes, troca áudio Opus real pelo RTP cifrado e confirma a remoção
MLS de quem sai. A suíte MLS já prova que membro removido não deriva a chave da época seguinte; a
suíte de mídia prova rejeição de payload claro. O contador `e2ee_frame_cost_snapshot` mede a
duração da cifra no ponto real entre encoder e RTP (separado de encode/packetização), e o teste
live imprime a média por quadro.

## Referências

- Estado atual do produto: [ARQUITETURA.md](ARQUITETURA.md), [CONTRATO.md](CONTRATO.md) e
  `native/shared/media/src/plain.rs`.
- [RFC 9420 — Messaging Layer Security](https://www.rfc-editor.org/rfc/rfc9420.html)
- [RFC 7748 — X25519](https://www.rfc-editor.org/rfc/rfc7748.html)
- [RFC 5869 — HKDF](https://www.rfc-editor.org/rfc/rfc5869.html)
- [RFC 8439 — ChaCha20-Poly1305](https://www.rfc-editor.org/rfc/rfc8439.html)
- [NIST SP 800-38D — AES-GCM](https://csrc.nist.gov/pubs/sp/800/38/d/final)
- [Discord — DAVE e MLS](https://discord.com/blog/meet-dave-e2ee-for-audio-video)

## Decisões da fase 1

### AEAD da mídia

`native/Cargo.lock` já contém as primitivas RustCrypto de que o desenho precisa:
`chacha20poly1305 0.10.1` e `x25519-dalek 2.0.1` chegam por `rtc-dtls`, `aes-gcm 0.10.3`
por `rtc-shared`, e `hkdf 0.12.4` já está na árvore. A dependência transitiva não é uma
API do `media`, então a fase 3 declara `chacha20poly1305 = "0.10.1"` diretamente no
crate que a usa; o lock continua reutilizando a mesma versão já compilada.

A cifra da carga é **ChaCha20-Poly1305 do RustCrypto**. É a mesma implementação auditada
que o `rtc` já traz, usa nonce de 96 bits e tag de 16 bytes da API AEAD, e evita escrever
primitiva ou Poly1305/ChaCha à mão. AES-GCM permanece disponível para outros usos, mas não
é a escolha da mídia: a carga roda também nas máquinas sem AES-NI/crypto extension e o
desempenho de ChaCha é mais previsível nelas. A disciplina de nonce continua obrigatória
para as duas cifras; por isso o contador por origem/época nunca é reutilizado.

### MLS

A recomendação para a fase 2 é **OpenMLS 0.9.x com `openmls_rust_crypto`**, pinado junto
com os crates irmãos da mesma release, no `shared/core` (não no caminho de mídia). O
provedor é RustCrypto puro: acrescenta OpenMLS, TLS codec, HPKE, Ed25519, P-256/P-384,
armazenamento e suas dependências, portanto é uma adição considerável mas adequada para o
estado de grupo, e não requer CMake nem AWS-LC. CMake continua sendo requisito separado do
`opusic-sys` já existente.

OpenMLS declara CI testada para `x86_64-pc-windows-msvc` e `x86_64-unknown-linux-gnu`; a
release 0.9 elevou o MSRV para Rust 1.91. O toolchain local é 1.98.1, mas antes de o adicionar
é preciso fixar e verificar Rust >= 1.91 nos builds Windows e Linux de release. O projeto é
mantido por Phoenix R&D e CE Labs, implementa RFC 9420 e teve releases de manutenção recentes;
isso o torna preferível a protocolo de distribuição próprio. A fase 2 deve ainda revisar as
advisories e pin de dependências da release escolhida.

A suite inicial recomendada é
`MLS_128_DHKEMX25519_CHACHA20POLY1305_SHA256_Ed25519`: X25519/Ed25519 evitam uma segunda
família de curvas, ChaCha coincide com a AEAD da mídia e a suite é suportada pelo provedor
RustCrypto. Ela não é a suite MTI AES-128-GCM do RFC; se a chamada precisar interoperar com
outro cliente MLS, a negociação deve incluir a MTI explicitamente e ser testada, nunca trocar
ou fazer downgrade em silêncio. Nenhum crate OpenMLS entra nesta fase: ela só deixa a decisão,
o requisito de toolchain e o custo de build registrados.
