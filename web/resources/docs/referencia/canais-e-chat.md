# Canais e chat

Cada canal tem um tipo: `text`, `voice` ou `category`. Todos pertencem a um servidor e usam identificadores ULID em minúsculas.

## Tipos de canal

- **Texto:** guarda mensagens e imagens do servidor.
- **Voz:** concentra a presença ao vivo, a mídia e também tem chat.
- **Categoria:** apenas agrupa canais de texto e voz; não aceita mensagens, voz ou limite de pessoas.

Apagar uma categoria devolve seus canais para a raiz do servidor. O servidor precisa manter pelo menos um canal de texto.

## Mensagens

Quem possui `VIEW_CHANNEL` pode ler o histórico; quem também tem `SEND_MESSAGES` pode enviar mensagens. Uma mensagem de texto aceita corpo de 1 a 2.000 caracteres, resposta a outra mensagem do mesmo canal e até três imagens JPEG, PNG, WebP ou GIF de até 2 MB cada.

As imagens ficam em bucket privado e recebem URL pré-assinada temporária. Editar uma mensagem muda apenas o texto; apagar remove também os arquivos associados. A pessoa autora pode editar ou apagar a própria mensagem, enquanto `MANAGE_MESSAGES` permite moderar mensagens de outras pessoas.

## Leitura e tempo real

Abrir o histórico sem paginação marca como lidas as mensagens recebidas até então. As não lidas contam somente mensagens de outras pessoas, em canais que a pessoa consegue ver. O Laravel grava a mensagem e o SFU a entrega em tempo real apenas às contas autorizadas para aquele canal.
