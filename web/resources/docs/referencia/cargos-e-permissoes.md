# Cargos e permissões

As permissões do Unkvoid são bits calculados pelo Laravel. O cargo `@everyone` sempre existe; nele, o padrão permite ver canal, enviar mensagens, conectar, falar, transmitir tela, usar vídeo e criar convite.

## Grupos de permissões

| Área | Exemplos |
| --- | --- |
| Administração | `ADMINISTRATOR`, `MANAGE_SERVER`, `MANAGE_ROLES`, `MANAGE_CHANNELS` |
| Membros | `KICK_MEMBERS`, `BAN_MEMBERS`, `MUTE_MEMBERS`, `DEAFEN_MEMBERS`, `MOVE_MEMBERS` |
| Canais e chat | `VIEW_CHANNEL`, `SEND_MESSAGES`, `MANAGE_MESSAGES` |
| Voz e mídia | `CONNECT`, `SPEAK`, `STREAM`, `VIDEO` |
| Convites e auditoria | `CREATE_INVITE`, `VIEW_AUDIT_LOG` |

`ADMINISTRATOR` e a dona do servidor têm todas as permissões. Fora esses casos, o cálculo começa no `@everyone`, soma os cargos da pessoa e aplica as sobrescritas do canal.

## Sobrescritas de canal

Uma sobrescrita pode permitir ou negar bits para `@everyone`, um cargo ou uma pessoa. A ordem é: `@everyone`, todos os cargos da pessoa e, por fim, a sobrescrita individual. Assim é possível, por exemplo, esconder um canal do servidor inteiro e liberá-lo apenas para um cargo.

## Hierarquia

O cargo de posição mais alta define até onde uma pessoa pode administrar. Ela só pode modificar cargos e membros abaixo de si, e não pode atribuir uma permissão que ela própria não possui. Essas regras também valem para expulsar, banir, mutar, desconectar e mover membros.

As rotas de cargos e sobrescritas estão resumidas na [API para integrações](/documentacao/referencia/api-para-integracoes).
