//! A lista de membros de um servidor como a do Discord: quem está online agrupado pelo cargo mais
//! alto que pede grupo próprio (`hoist`), do cargo mais alto para o mais baixo; quem não tem
//! nenhum assim fica em "Online"; e os offline num grupo só no fim. Dentro de cada grupo, por nome.

use std::collections::HashSet;

use crate::models::{Member, Role, ServerTree};

pub const OFFLINE_KEY: &str = "offline";
pub const EVERYONE_KEY: &str = "everyone";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberGroup {
    pub key: String,
    pub label: String,
    /// A cor do cargo, em `#rrggbb`. `None` é a cor comum do texto.
    pub color: Option<String>,
    pub members: Vec<Member>,
}

pub fn display_name(member: &Member) -> &str {
    member.nickname.as_deref().unwrap_or(&member.name)
}

/// O cargo mais alto da pessoa, fora o `@everyone`, que todo mundo tem.
pub fn top_role<'tree>(tree: &'tree ServerTree, member: &Member) -> Option<&'tree Role> {
    tree.roles
        .iter()
        .filter(|role| !role.is_everyone && member.role_ids.contains(&role.id))
        .max_by_key(|role| role.position)
}

/// O cargo mais alto da pessoa entre os que pedem grupo próprio (`hoist`): é por ele que o
/// Discord separa a lista. Sem nenhum, a pessoa fica em "Online".
fn top_hoisted_role<'tree>(tree: &'tree ServerTree, member: &Member) -> Option<&'tree Role> {
    tree.roles
        .iter()
        .filter(|role| !role.is_everyone && role.hoist && member.role_ids.contains(&role.id))
        .max_by_key(|role| role.position)
}

pub fn group(tree: &ServerTree, online: &HashSet<i64>) -> Vec<MemberGroup> {
    let mut groups: Vec<MemberGroup> = Vec::new();
    let mut offline = MemberGroup {
        key: OFFLINE_KEY.to_owned(),
        label: "Offline".to_owned(),
        color: None,
        members: Vec::new(),
    };

    for member in &tree.members {
        if !online.contains(&member.user_id) {
            offline.members.push(member.clone());

            continue;
        }

        let role = top_hoisted_role(tree, member);
        let key = role.map_or_else(|| EVERYONE_KEY.to_owned(), |role| role.id.to_string());

        match groups.iter_mut().find(|group| group.key == key) {
            Some(group) => group.members.push(member.clone()),
            None => groups.push(MemberGroup {
                key,
                // Quem não tem cargo fica em "Online", como no Discord: o nome do `@everyone`
                // ali parecia um cargo.
                label: role.map_or_else(|| "Online".to_owned(), |role| role.name.clone()),
                color: role.and_then(|role| role.color.clone()),
                members: vec![member.clone()],
            }),
        }
    }

    let position = |group: &MemberGroup| {
        tree.roles
            .iter()
            .find(|role| role.id.to_string() == group.key)
            .map_or(-1, |role| role.position)
    };

    groups.sort_by_key(|group| std::cmp::Reverse(position(group)));

    if !offline.members.is_empty() {
        groups.push(offline);
    }

    for group in &mut groups {
        group
            .members
            .sort_by_key(|member| display_name(member).to_lowercase());
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Membership;

    fn role(id: i64, name: &str, position: i64, everyone: bool) -> Role {
        Role {
            id,
            name: name.into(),
            color: (!everyone).then(|| format!("#00000{id}")),
            position,
            permissions: 0,
            is_everyone: everyone,
            hoist: !everyone,
            mentionable: false,
        }
    }

    fn member(user_id: i64, name: &str, roles: &[i64]) -> Member {
        Member {
            user_id,
            name: name.into(),
            avatar_url: None,
            nickname: None,
            status: Default::default(),
            activity: None,
            role_ids: roles.to_vec(),
            server_mute: false,
            server_deaf: false,
            is_owner: false,
            profile: Default::default(),
            member_since: None,
            joined_at: None,
        }
    }

    fn tree() -> ServerTree {
        ServerTree {
            id: 1,
            name: "servidor".into(),
            owner_id: 1,
            invite_code: None,
            invites_paused: false,
            icon_url: None,
            me: Membership { user_id: 1, permissions: 0, top_position: 0 },
            roles: vec![role(1, "todos", 0, true), role(2, "Moderação", 5, false), role(3, "Amigos", 2, false)],
            channels: Vec::new(),
            members: vec![
                member(10, "zeca", &[3]),
                member(11, "ana", &[2, 3]),
                member(12, "bia", &[]),
                member(13, "caio", &[2]),
                member(14, "duda", &[]),
            ],
            voice: Default::default(),
            bans: Vec::new(),
        }
    }

    #[test]
    fn online_people_group_under_their_highest_role_and_offline_goes_last() {
        let online = HashSet::from([10, 11, 12, 13]);
        let groups = group(&tree(), &online);
        let names: Vec<(&str, Vec<&str>)> = groups
            .iter()
            .map(|group| (group.label.as_str(), group.members.iter().map(display_name).collect()))
            .collect();

        assert_eq!(
            names,
            [
                ("Moderação", vec!["ana", "caio"]),
                ("Amigos", vec!["zeca"]),
                ("Online", vec!["bia"]),
                ("Offline", vec!["duda"]),
            ]
        );
        assert_eq!(groups[0].color.as_deref(), Some("#000002"));
        assert_eq!(groups[2].color, None);
    }

    #[test]
    fn nobody_offline_means_no_offline_group() {
        let online = HashSet::from([10, 11, 12, 13, 14]);

        assert!(group(&tree(), &online).iter().all(|group| group.key != OFFLINE_KEY));
    }

    /// O cargo sem "mostrar separado" não abre grupo: quem só tem ele fica em "Online", e quem
    /// tem outro que mostra vai para o desse.
    #[test]
    fn a_role_that_is_not_hoisted_does_not_get_its_own_group() {
        let mut tree = tree();

        tree.roles.iter_mut().filter(|role| role.name == "Moderação").for_each(|role| role.hoist = false);

        let names: Vec<(String, Vec<String>)> = group(&tree, &HashSet::from([10, 11, 13]))
            .into_iter()
            .map(|group| (group.label, group.members.iter().map(|member| display_name(member).to_owned()).collect()))
            .collect();

        assert_eq!(
            names,
            [
                ("Amigos".to_owned(), vec!["ana".to_owned(), "zeca".to_owned()]),
                ("Online".to_owned(), vec!["caio".to_owned()]),
                ("Offline".to_owned(), vec!["bia".to_owned(), "duda".to_owned()]),
            ]
        );
    }
}
