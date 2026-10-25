# Privacidade e segurança

O Unkvoid cifra o tráfego, mas não oferece criptografia de ponta a ponta. Isso é importante: a mídia não é gravada, porém a infraestrutura do serviço é parte confiável do caminho.

## O que viaja e o que fica guardado

| Dado | Em trânsito | Armazenamento |
| --- | --- | --- |
| Site, API e login | TLS | Senhas protegidas por bcrypt; tokens do Sanctum ficam somente como hash no banco. |
| Chat e mensagens diretas | TLS até o Laravel e WSS no SFU | Texto puro no MySQL. |
| Fotos, ícones e imagens de chat | TLS | Bucket privado; URLs são temporárias. |
| Voz, tela e câmera nos clientes que a suportam | SRTP | Não são gravadas. |
| Token da conta no aplicativo nativo | — | Cifrado no disco com chave do chaveiro do sistema. |

O SFU repassa mídia sem decodificá-la no funcionamento normal, mas possui as chaves de cada transmissão. Quem controla a infraestrutura poderia acessar a mídia; por isso não é uma solução ponta a ponta.

## Permissões e acesso

O Laravel calcula as permissões e o SFU só aceita um token assinado e ainda válido. Um canal oculto por `VIEW_CHANNEL` não aparece, não expõe mensagens e não concede acesso à voz. A entrada em um servidor ocorre por convite e banimentos impedem uma nova entrada pelo mesmo caminho.

## Diagnóstico e retenção

O endpoint público de erros recebe relatório técnico limitado por IP. Os aplicativos enviam trechos de log que contêm erro, com versão e plataforma; a mídia transmitida não faz parte desse relatório. A política de privacidade também explica os dados da conta, as solicitações de exclusão e o contato em caso de dúvida.

Leia a [Política de privacidade](/privacidade) e os [Termos de uso](/termos) para os compromissos voltados a quem usa o serviço.
