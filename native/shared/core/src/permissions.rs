//! O que esta pessoa pode fazer num servidor, para a interface saber que botão mostrar.
//!
//! Quem **autoriza** é o Laravel, em toda chamada. Isto aqui só responde "vale a pena
//! desenhar o botão?", e mora no núcleo porque a conta é a mesma nas três interfaces: a
//! ordem dos cargos, quem está acima de quem, que cargo dá para entregar. Os bits são os de
//! `web/app/Enums/PermissionEnum.php`.

use serde::Serialize;

use crate::models::{Member, Role, ServerTree};

pub const ADMINISTRATOR: i64 = 1 << 0;
pub const MANAGE_SERVER: i64 = 1 << 1;
pub const MANAGE_ROLES: i64 = 1 << 2;
pub const MANAGE_CHANNELS: i64 = 1 << 3;
pub const KICK_MEMBERS: i64 = 1 << 4;
pub const BAN_MEMBERS: i64 = 1 << 5;
pub const CREATE_INVITE: i64 = 1 << 6;
pub const VIEW_AUDIT_LOG: i64 = 1 << 7;
pub const VIEW_CHANNEL: i64 = 1 << 8;
pub const SEND_MESSAGES: i64 = 1 << 9;
pub const MANAGE_MESSAGES: i64 = 1 << 10;
pub const CONNECT: i64 = 1 << 11;
pub const SPEAK: i64 = 1 << 12;
pub const STREAM: i64 = 1 << 13;
pub const VIDEO: i64 = 1 << 14;
pub const MUTE_MEMBERS: i64 = 1 << 15;
pub const DEAFEN_MEMBERS: i64 = 1 << 16;
pub const MOVE_MEMBERS: i64 = 1 << 17;

/// O nome que a interface lê e o bit que ele vale.
const NAMES: &[(&str, i64)] = &[
    ("administrator", ADMINISTRATOR),
    ("manageServer", MANAGE_SERVER),
    ("manageRoles", MANAGE_ROLES),
    ("manageChannels", MANAGE_CHANNELS),
    ("kickMembers", KICK_MEMBERS),
    ("banMembers", BAN_MEMBERS),
    ("createInvite", CREATE_INVITE),
    ("viewAuditLog", VIEW_AUDIT_LOG),
    ("manageMessages", MANAGE_MESSAGES),
    ("muteMembers", MUTE_MEMBERS),
    ("deafenMembers", DEAFEN_MEMBERS),
    ("moveMembers", MOVE_MEMBERS),
];

/// Administrador tem tudo, como no cálculo do servidor.
pub fn has(bits: i64, flag: i64) -> bool {
    bits & ADMINISTRATOR != 0 || bits & flag == flag
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MemberActions {
    pub nickname: bool,
    pub mute: bool,
    pub deafen: bool,
    pub disconnect: bool,
    pub kick: bool,
    pub ban: bool,
    pub roles: bool,
}

/// Um cargo na lista de configuração: se dá para mexer nele, e para que posição ele iria
/// subindo ou descendo (`None` quando o vizinho não deixa).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RoleRow {
    pub id: i64,
    pub editable: bool,
    pub assignable: bool,
    pub up: Option<i64>,
    pub down: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Abilities {
    pub can: Vec<&'static str>,
    pub owner: bool,
    /// Por `user_id`, em texto: chave de objeto JSON não é número.
    pub members: std::collections::BTreeMap<String, MemberActions>,
    pub roles: Vec<RoleRow>,
}

impl ServerTree {
    pub fn can(&self, flag: i64) -> bool {
        has(self.me.permissions, flag)
    }

    /// O cargo mais alto de um membro. O dono está acima de todo mundo.
    fn top_position(&self, member: &Member) -> i64 {
        if member.is_owner || member.user_id == self.owner_id {
            return i64::MAX;
        }

        self.roles
            .iter()
            .filter(|role| member.role_ids.contains(&role.id))
            .map(|role| role.position)
            .max()
            .unwrap_or(0)
            .max(0)
    }

    pub fn member_actions(&self, member: &Member) -> MemberActions {
        let this_is_me = member.user_id == self.me.user_id;
        let mine = self
            .members
            .iter()
            .find(|candidate| candidate.user_id == self.me.user_id);
        let below = !this_is_me
            && mine.is_some_and(|mine| self.top_position(mine) > self.top_position(member));
        let in_voice = self
            .voice
            .values()
            .flatten()
            .any(|person| person.user_id == member.user_id);

        MemberActions {
            nickname: this_is_me || (below && self.can(MANAGE_SERVER)),
            mute: below && self.can(MUTE_MEMBERS),
            deafen: below && self.can(DEAFEN_MEMBERS),
            disconnect: below && in_voice && self.can(MOVE_MEMBERS),
            kick: below && self.can(KICK_MEMBERS),
            ban: below && self.can(BAN_MEMBERS),
            roles: below && self.can(MANAGE_ROLES),
        }
    }

    /// Os cargos do mais alto para o mais baixo, com o que dá para fazer em cada um.
    pub fn role_rows(&self) -> Vec<RoleRow> {
        let mut roles: Vec<&Role> = self.roles.iter().collect();
        let my_top = self.me.top_position;

        roles.sort_by_key(|role| std::cmp::Reverse(role.position));

        roles
            .iter()
            .enumerate()
            .map(|(index, role)| {
                let above = index.checked_sub(1).and_then(|index| roles.get(index));
                let under = roles.get(index + 1);
                let editable = role.is_everyone || role.position < my_top;
                let movable = editable && !role.is_everyone;

                RoleRow {
                    id: role.id,
                    editable,
                    assignable: !role.is_everyone && role.position < my_top,
                    up: above
                        .filter(|above| movable && !above.is_everyone && above.position < my_top)
                        .map(|above| above.position),
                    down: under
                        .filter(|under| movable && !under.is_everyone)
                        .map(|under| under.position),
                }
            })
            .collect()
    }

    pub fn abilities(&self) -> Abilities {
        Abilities {
            can: NAMES
                .iter()
                .filter(|(_, flag)| self.can(*flag))
                .map(|(name, _)| *name)
                .collect(),
            owner: self.owner_id == self.me.user_id,
            members: self
                .members
                .iter()
                .map(|member| (member.user_id.to_string(), self.member_actions(member)))
                .collect(),
            roles: self.role_rows(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A Ada é moderadora (cargo 5), a Grace é membro comum (cargo 2), o Linus é o dono.
    fn tree(my_permissions: i64) -> ServerTree {
        serde_json::from_value(json!({
            "id": 1, "name": "Estúdio", "owner_id": 3, "invite_code": null, "icon_url": null,
            "me": { "user_id": 1, "permissions": my_permissions, "top_position": 5 },
            "roles": [
                { "id": 10, "name": "@everyone", "color": null, "position": 0, "permissions": 0, "is_everyone": true },
                { "id": 11, "name": "Membro", "color": null, "position": 2, "permissions": 0 },
                { "id": 12, "name": "Moderação", "color": null, "position": 5, "permissions": 0 },
                { "id": 13, "name": "Direção", "color": null, "position": 9, "permissions": 0 },
            ],
            "channels": [],
            "members": [
                { "user_id": 1, "name": "Ada", "avatar_url": null, "nickname": null, "role_ids": [12] },
                { "user_id": 2, "name": "Grace", "avatar_url": null, "nickname": null, "role_ids": [11] },
                { "user_id": 3, "name": "Linus", "avatar_url": null, "nickname": null, "role_ids": [], "is_owner": true },
                { "user_id": 4, "name": "Barbara", "avatar_url": null, "nickname": null, "role_ids": [12] },
            ],
            "voice": { "01abc": [{ "user_id": 2, "name": "Grace" }] },
        }))
        .expect("tree")
    }

    fn member(tree: &ServerTree, id: i64) -> &Member {
        tree.members
            .iter()
            .find(|member| member.user_id == id)
            .expect("member")
    }

    #[test]
    fn an_administrator_has_every_permission() {
        assert!(has(ADMINISTRATOR, BAN_MEMBERS));
        assert!(!has(KICK_MEMBERS, BAN_MEMBERS));
    }

    #[test]
    fn a_moderator_acts_only_on_who_is_below_her() {
        let tree = tree(KICK_MEMBERS | BAN_MEMBERS | MOVE_MEMBERS);

        let grace = tree.member_actions(member(&tree, 2));

        assert!(grace.kick && grace.ban);
        assert!(grace.disconnect, "a Grace está na voz");
        assert!(!grace.mute && !grace.roles, "sem o bit, sem o botão");

        assert_eq!(
            tree.member_actions(member(&tree, 3)),
            MemberActions::default(),
            "ninguém age sobre o dono"
        );
        assert_eq!(
            tree.member_actions(member(&tree, 4)),
            MemberActions::default(),
            "nem sobre quem tem o mesmo cargo"
        );
    }

    #[test]
    fn everyone_can_change_their_own_nickname_and_nothing_else_on_themselves() {
        let tree = tree(ADMINISTRATOR);

        assert_eq!(
            tree.member_actions(member(&tree, 1)),
            MemberActions {
                nickname: true,
                ..MemberActions::default()
            }
        );
    }

    #[test]
    fn roles_move_only_below_my_own_and_never_past_everyone() {
        let tree = tree(MANAGE_ROLES);
        let rows = tree.role_rows();
        let row = |id: i64| rows.iter().find(|row| row.id == id).expect("row");

        assert_eq!(
            rows.iter().map(|row| row.id).collect::<Vec<_>>(),
            [13, 12, 11, 10],
            "do mais alto para o mais baixo"
        );
        assert!(
            !row(13).editable && !row(12).editable,
            "o meu cargo e os de cima não são meus para mexer"
        );
        assert!(row(11).editable && row(11).assignable);
        assert_eq!(
            (row(11).up, row(11).down),
            (None, None),
            "acima está o meu cargo, abaixo o @everyone"
        );
        assert!(
            row(10).editable && !row(10).assignable,
            "as permissões do @everyone se editam, mas ele não se entrega"
        );
    }

    #[test]
    fn the_abilities_name_what_the_bits_allow() {
        let abilities = tree(MANAGE_SERVER | VIEW_AUDIT_LOG).abilities();

        assert_eq!(abilities.can, ["manageServer", "viewAuditLog"]);
        assert!(!abilities.owner);
        assert!(abilities.members.contains_key("2"));
    }
}
