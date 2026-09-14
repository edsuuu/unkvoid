/**
 * Os bits de permissão, iguais aos do Laravel (SERVIDORES.md).
 *
 * O app só esconde botão: quem decide é o servidor, e todo 403 vira aviso. O canal já
 * chega com o `permissions` que o Laravel calculou, então aqui só mora o que a tela
 * precisa antes de qualquer ida ao servidor: ler um bit e comparar hierarquia.
 */
export class Permissions {
    static ADMINISTRATOR = 1 << 0;
    static MANAGE_SERVER = 1 << 1;
    static MANAGE_ROLES = 1 << 2;
    static MANAGE_CHANNELS = 1 << 3;
    static KICK_MEMBERS = 1 << 4;
    static BAN_MEMBERS = 1 << 5;
    static CREATE_INVITE = 1 << 6;
    static VIEW_AUDIT_LOG = 1 << 7;
    static VIEW_CHANNEL = 1 << 8;
    static SEND_MESSAGES = 1 << 9;
    static MANAGE_MESSAGES = 1 << 10;
    static CONNECT = 1 << 11;
    static SPEAK = 1 << 12;
    static STREAM = 1 << 13;
    static VIDEO = 1 << 14;
    static MUTE_MEMBERS = 1 << 15;
    static DEAFEN_MEMBERS = 1 << 16;
    static MOVE_MEMBERS = 1 << 17;

    static ALL = (1 << 18) - 1;

    /** Rótulos na ordem em que aparecem no editor de cargos. */
    static LABELS = [
        ['ADMINISTRATOR', 'Administrador'],
        ['MANAGE_SERVER', 'Gerenciar servidor'],
        ['MANAGE_ROLES', 'Gerenciar cargos'],
        ['MANAGE_CHANNELS', 'Gerenciar canais'],
        ['KICK_MEMBERS', 'Expulsar membros'],
        ['BAN_MEMBERS', 'Banir membros'],
        ['CREATE_INVITE', 'Criar convite'],
        ['VIEW_AUDIT_LOG', 'Ver auditoria'],
        ['VIEW_CHANNEL', 'Ver canais'],
        ['SEND_MESSAGES', 'Enviar mensagens'],
        ['MANAGE_MESSAGES', 'Gerenciar mensagens'],
        ['CONNECT', 'Conectar na voz'],
        ['SPEAK', 'Falar'],
        ['STREAM', 'Compartilhar tela'],
        ['VIDEO', 'Câmera'],
        ['MUTE_MEMBERS', 'Mutar membros'],
        ['DEAFEN_MEMBERS', 'Ensurdecer membros'],
        ['MOVE_MEMBERS', 'Desconectar da voz'],
    ];

    /** As que o editor "Ocultar canal" oferece por cargo e por membro. */
    static OVERWRITABLE = ['VIEW_CHANNEL', 'SEND_MESSAGES', 'CONNECT', 'SPEAK', 'STREAM', 'VIDEO'];

    static has(bits, flag) {
        return (bits & Permissions.ADMINISTRATOR) !== 0 || (bits & flag) === flag;
    }

    static topPosition(server, member) {
        if (! member || member.is_owner || member.user_id === server.owner_id) {
            return Infinity;
        }

        return Math.max(0, ...server.roles.filter(role => member.role_ids.includes(role.id)).map(role => role.position));
    }

    /** Só se mexe em quem está abaixo: expulsar, banir, dar cargo, mutar, desconectar. */
    static outranks(server, actor, target) {
        return Permissions.topPosition(server, actor) > Permissions.topPosition(server, target);
    }
}
