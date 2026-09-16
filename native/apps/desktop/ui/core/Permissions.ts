import type { Member, ServerTree } from './Models.ts';

export type PermissionName =
    | 'ADMINISTRATOR'
    | 'MANAGE_SERVER'
    | 'MANAGE_ROLES'
    | 'MANAGE_CHANNELS'
    | 'KICK_MEMBERS'
    | 'BAN_MEMBERS'
    | 'CREATE_INVITE'
    | 'VIEW_AUDIT_LOG'
    | 'VIEW_CHANNEL'
    | 'SEND_MESSAGES'
    | 'MANAGE_MESSAGES'
    | 'CONNECT'
    | 'SPEAK'
    | 'STREAM'
    | 'VIDEO'
    | 'MUTE_MEMBERS'
    | 'DEAFEN_MEMBERS'
    | 'MOVE_MEMBERS';

export class Permissions {
    static readonly ADMINISTRATOR = 1 << 0;
    static readonly MANAGE_SERVER = 1 << 1;
    static readonly MANAGE_ROLES = 1 << 2;
    static readonly MANAGE_CHANNELS = 1 << 3;
    static readonly KICK_MEMBERS = 1 << 4;
    static readonly BAN_MEMBERS = 1 << 5;
    static readonly CREATE_INVITE = 1 << 6;
    static readonly VIEW_AUDIT_LOG = 1 << 7;
    static readonly VIEW_CHANNEL = 1 << 8;
    static readonly SEND_MESSAGES = 1 << 9;
    static readonly MANAGE_MESSAGES = 1 << 10;
    static readonly CONNECT = 1 << 11;
    static readonly SPEAK = 1 << 12;
    static readonly STREAM = 1 << 13;
    static readonly VIDEO = 1 << 14;
    static readonly MUTE_MEMBERS = 1 << 15;
    static readonly DEAFEN_MEMBERS = 1 << 16;
    static readonly MOVE_MEMBERS = 1 << 17;

    static readonly ALL = (1 << 18) - 1;

    static readonly LABELS: [PermissionName, string][] = [
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

    static readonly OVERWRITABLE: PermissionName[] = ['VIEW_CHANNEL', 'SEND_MESSAGES', 'CONNECT', 'SPEAK', 'STREAM', 'VIDEO'];

    static has(bits: number, flag: number): boolean {
        return (bits & Permissions.ADMINISTRATOR) !== 0 || (bits & flag) === flag;
    }

    static topPosition(server: ServerTree, member: Member | null): number {
        if (! member || member.is_owner || member.user_id === server.owner_id) {
            return Infinity;
        }

        return Math.max(0, ...server.roles.filter(role => member.role_ids.includes(role.id)).map(role => role.position));
    }

    static outranks(server: ServerTree, actor: Member | null, target: Member | null): boolean {
        return Permissions.topPosition(server, actor) > Permissions.topPosition(server, target);
    }
}
