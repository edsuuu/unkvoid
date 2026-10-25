# Voz, tela e câmera

Entre em um canal de voz do servidor para participar da conversa em tempo real. O Laravel confere a sua conta, as permissões e o limite do canal antes de emitir um token de voz que vale 60 segundos.

## O que o canal controla

- `CONNECT` permite entrar no canal de voz.
- `SPEAK` permite produzir o microfone.
- `STREAM` permite produzir a tela e o áudio da tela.
- `VIDEO` é a permissão de câmera no protocolo de voz.

O token leva apenas as capacidades liberadas. O SFU confere esse token e recusa produzir uma fonte que a pessoa não pode enviar; a interface do aplicativo é apenas uma conveniência, não a autoridade.

## Tela e voz

A transmissão de tela é enviada uma vez ao SFU e ele a replica para as pessoas que estão assistindo. O chat continua disponível no canal de voz, com as mesmas regras de visibilidade e envio de mensagens de um canal de texto.

## Câmera

O protocolo de voz aceita as fontes `mic`, `screen`, `screenAudio` e `camera`, e a câmera exige `VIDEO`. Os clientes macOS e Linux podem produzir a câmera quando a pessoa tem essa permissão. No Windows, a câmera ainda está desativada; não há um passo alternativo que a habilite.

Para entender o caminho técnico da mídia, leia [Como o Unkvoid conecta](/documentacao/referencia/como-conecta).
