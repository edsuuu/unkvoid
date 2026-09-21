//! As rotas da API do Laravel, pelo nome. É o mesmo mapa para as três interfaces: quem
//! desenha pede `"kickMember"` com `{server, user}` e não sabe (nem precisa saber) o caminho.
//!
//! O contrato é `docs/CONTRATO.md`; mudou uma rota lá, muda uma linha aqui.

use reqwest::Method;
use serde_json::Value;

/// `(nome, método, caminho)`. O que está entre chaves vem de `params`.
const ROUTES: &[(&str, &str, &str)] = &[
    ("updateMe", "PATCH", "/api/me"),
    ("uploadAvatar", "POST", "/api/me/avatar"),
    ("deleteAvatar", "DELETE", "/api/me/avatar"),
    ("signOut", "POST", "/api/auth/logout"),
    ("createServer", "POST", "/api/servers"),
    ("updateServer", "PATCH", "/api/servers/{server}"),
    ("deleteServer", "DELETE", "/api/servers/{server}"),
    ("leaveServer", "POST", "/api/servers/{server}/leave"),
    ("uploadServerIcon", "POST", "/api/servers/{server}/icon"),
    ("deleteServerIcon", "DELETE", "/api/servers/{server}/icon"),
    ("renewInvite", "POST", "/api/servers/{server}/invite"),
    ("acceptInvite", "POST", "/api/invites/{code}"),
    ("audits", "GET", "/api/servers/{server}/audits"),
    ("bans", "GET", "/api/servers/{server}/bans"),
    ("ban", "POST", "/api/servers/{server}/bans/{user}"),
    ("unban", "DELETE", "/api/servers/{server}/bans/{user}"),
    (
        "updateMember",
        "PATCH",
        "/api/servers/{server}/members/{user}",
    ),
    (
        "kickMember",
        "DELETE",
        "/api/servers/{server}/members/{user}",
    ),
    ("createRole", "POST", "/api/servers/{server}/roles"),
    ("updateRole", "PATCH", "/api/roles/{role}"),
    ("deleteRole", "DELETE", "/api/roles/{role}"),
    ("createChannel", "POST", "/api/servers/{server}/channels"),
    ("updateChannel", "PATCH", "/api/channels/{channel}"),
    ("deleteChannel", "DELETE", "/api/channels/{channel}"),
    (
        "putOverwrite",
        "PUT",
        "/api/channels/{channel}/overwrites/{type}/{id}",
    ),
    (
        "deleteOverwrite",
        "DELETE",
        "/api/channels/{channel}/overwrites/{type}/{id}",
    ),
    (
        "disconnectFromVoice",
        "DELETE",
        "/api/channels/{channel}/voice/members/{user}",
    ),
    ("messages", "GET", "/api/channels/{channel}/messages"),
    ("sendMessage", "POST", "/api/channels/{channel}/messages"),
    ("friends", "GET", "/api/friends"),
    ("addFriend", "POST", "/api/friends"),
    ("answerFriend", "PATCH", "/api/friends/{friendship}"),
    ("removeFriend", "DELETE", "/api/friends/{friendship}"),
    ("conversations", "GET", "/api/dm"),
    ("directMessages", "GET", "/api/dm/{user}"),
    ("sendDirect", "POST", "/api/dm/{user}"),
    ("readDirect", "POST", "/api/dm/{user}/read"),
    ("editDirect", "PATCH", "/api/dm/{message}"),
    ("deleteDirect", "DELETE", "/api/dm/{message}"),
    ("reportError", "POST", "/api/errors"),
];

/// O método e o caminho já preenchido de uma rota. `None` para nome desconhecido ou
/// parâmetro que faltou: caminho com `{server}` sobrando iria ao servidor como lixo.
pub fn resolve(name: &str, params: &Value) -> Option<(Method, String)> {
    let (_, method, template) = ROUTES.iter().find(|(known, _, _)| *known == name)?;
    let mut path = String::new();

    for piece in template.split('/').skip(1) {
        path.push('/');

        match piece
            .strip_prefix('{')
            .and_then(|inner| inner.strip_suffix('}'))
        {
            Some(key) => path.push_str(&segment(&params[key])?),
            None => path.push_str(piece),
        }
    }

    // A consulta (`?before=…`) acompanha o caminho: é o único jeito de paginar um GET.
    if let Some(query) = params["query"].as_str().filter(|query| !query.is_empty()) {
        path.push('?');
        path.push_str(query);
    }

    Some((method.parse().ok()?, path))
}

/// Um pedaço de caminho: número ou texto sem nada que saia do próprio pedaço.
fn segment(value: &Value) -> Option<String> {
    let text = match value {
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        _ => return None,
    };

    let safe = !text.is_empty()
        && text
            .chars()
            .all(|letter| letter.is_ascii_alphanumeric() || matches!(letter, '-' | '_'));

    safe.then_some(text)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_route_is_filled_from_its_parameters() {
        let (method, path) =
            resolve("kickMember", &json!({ "server": 7, "user": 42 })).expect("route");

        assert_eq!(method, Method::DELETE);
        assert_eq!(path, "/api/servers/7/members/42");
    }

    #[test]
    fn a_missing_or_hostile_parameter_never_reaches_the_server() {
        assert!(
            resolve("kickMember", &json!({ "server": 7 })).is_none(),
            "faltou o user"
        );
        assert!(resolve("deleteChannel", &json!({ "channel": "../me" })).is_none());
        assert!(resolve("deleteChannel", &json!({ "channel": "a/b" })).is_none());
        assert!(resolve("somethingElse", &json!({})).is_none());
    }

    #[test]
    fn a_query_rides_along_with_the_path() {
        let (_, path) = resolve(
            "messages",
            &json!({ "channel": "01abc", "query": "before=90" }),
        )
        .expect("route");

        assert_eq!(path, "/api/channels/01abc/messages?before=90");
    }

    #[test]
    fn every_route_parses_and_every_name_is_unique() {
        let mut names: Vec<&str> = ROUTES.iter().map(|(name, _, _)| *name).collect();

        names.sort_unstable();
        names.dedup();

        assert_eq!(names.len(), ROUTES.len(), "nome repetido");

        for (name, method, path) in ROUTES {
            assert!(method.parse::<Method>().is_ok(), "{name}: método {method}");
            assert!(path.starts_with("/api/"), "{name}: {path}");
        }
    }
}
