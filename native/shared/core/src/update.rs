//! A atualização dos apps nativos. O manifesto do site diz qual é a versão mais nova e onde
//! está o instalador; a assinatura minisign prova que ele saiu das nossas mãos. Quem abre o
//! instalador é cada sistema — decidir se há versão e confiar no arquivo mora aqui.
//!
//! No Linux nada disto roda: lá quem atualiza é o APT, junto com o resto da máquina.

use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use minisign_verify::{PublicKey, Signature};
use serde_json::Value;

use crate::api::Api;
use crate::app::App;

/// A versão que o servidor anunciou pelo tempo real, guardada até estar instalada. É ela que
/// dispensa perguntar ao site de tempos em tempos: o app só pergunta quando é avisado.
const ANNOUNCED_KEY: &str = "unkvoid:update-announced";

/// A chave do manifesto que serve a esta plataforma. No Windows é a do `.exe`: o instalador
/// nativo é um NSIS, e é por essa mesma chave que o app do Tauri chega até ele.
pub const PLATFORM: &str = if cfg!(target_os = "macos") {
    "darwin-aarch64"
} else if cfg!(target_os = "windows") {
    "windows-x86_64-nsis"
} else {
    "linux-x86_64-deb"
};

/// A pública do atualizador, a mesma de `plugins.updater.pubkey` no `tauri.conf.json` (par
/// `9a18c9243ef59b08`): é por ela que quem ainda tem o app do Tauri aceita o nativo. Trocar
/// uma sem a outra deixa alguém sem atualização — ver `docs/AUTO-UPDATE.md`.
const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDg5QkY1M0UyNEM5MTg5QQpSV1NhR01ra1B2V2JDSzhaSUJRR3Ezak1nYlNBWEh0NWZkWXRBTlJqdUQyaVNoVXh0b2RBeTVwOAo=";

/// Uma versão mais nova que esta, publicada para esta plataforma.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub signature: String,
}

/// `0.1.0` vem depois de `0.1.0-beta`, e `1.10.0` depois de `1.9.3`: a ordem do semver. O
/// que não é versão nunca é mais novo — melhor não atualizar que atualizar para lixo.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (semver::Version::parse(candidate), semver::Version::parse(current)) {
        (Ok(candidate), Ok(current)) => candidate > current,
        _ => false,
    }
}

/// A versão que um `ReleasePublished` do canal `releases` anuncia, quando ela é desta
/// plataforma e mais nova que esta. O aviso vai a todo app aberto, de todos os sistemas.
pub fn newer_in(announcement: &Value) -> Option<String> {
    let version = announcement["version"].as_str()?;

    (announcement["platform"] == PLATFORM && is_newer(version, env!("CARGO_PKG_VERSION"))).then(|| version.to_owned())
}

/// Anota a versão anunciada: fica na configuração até a versão nova estar rodando.
pub fn announce(app: &App, version: &str) {
    app.set_preference(ANNOUNCED_KEY, Value::String(version.to_owned()));
}

/// A versão anunciada que ainda falta instalar. Já instalada — esta versão é ela ou mais
/// nova —, a anotação sai do disco.
pub fn announced(app: &App) -> Option<String> {
    let version = app.preference(ANNOUNCED_KEY)?.as_str()?.to_owned();

    if is_newer(&version, env!("CARGO_PKG_VERSION")) {
        return Some(version);
    }

    app.set_preference(ANNOUNCED_KEY, Value::Null);

    None
}

/// Os bytes são os que a nossa chave assinou? A assinatura do manifesto é o arquivo do
/// minisign inteiro em base64, do jeito que o Tauri escreve.
pub fn signed(installer: &[u8], signature: &str) -> bool {
    let text = |encoded: &str| {
        STANDARD
            .decode(encoded)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    };
    let (Some(key), Some(signature)) = (text(PUBLIC_KEY), text(signature)) else {
        return false;
    };
    let (Ok(key), Ok(signature)) = (PublicKey::decode(&key), Signature::decode(&signature)) else {
        return false;
    };

    key.verify(installer, &signature, true).is_ok()
}

/// Baixa o instalador, confere a assinatura e só então o põe no disco, na pasta temporária
/// e com o nome que ele tem no site. `progress` recebe o baixado e o total, quando o
/// servidor o diz. `None` em qualquer falha: o app abre na versão que tem.
pub async fn fetch(
    api: &Api,
    release: &Release,
    progress: impl FnMut(u64, Option<u64>),
) -> Option<PathBuf> {
    let Some(installer) = api.download(&release.url, progress).await else {
        tracing::warn!(version = release.version, "atualização: o download falhou");

        return None;
    };

    if !signed(&installer, &release.signature) {
        tracing::warn!(version = release.version, "atualização: a assinatura não confere, instalador descartado");

        return None;
    }

    let url = reqwest::Url::parse(&release.url).ok()?;
    let path = std::env::temp_dir().join(url.path_segments()?.next_back()?);

    match std::fs::write(&path, installer) {
        Ok(()) => Some(path),
        Err(failure) => {
            tracing::warn!(%failure, "atualização: o instalador não foi para o disco");

            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_announced_version_stays_noted_until_it_is_the_one_running() {
        let dir = tempfile::tempdir().expect("temp dir");
        let app = App::new(storage::Storage::open_at(dir.path()).expect("open"));

        announce(&app, "99.0.0");

        assert_eq!(announced(&app), Some("99.0.0".to_owned()));
        assert_eq!(announced(&app), Some("99.0.0".to_owned()), "ler não apaga");

        announce(&app, env!("CARGO_PKG_VERSION"));

        assert_eq!(announced(&app), None, "a versão anunciada é a que está rodando");
        assert_eq!(app.preference(ANNOUNCED_KEY), None, "e a anotação sai do disco");
    }

    #[test]
    fn only_a_newer_version_of_this_platform_is_worth_announcing() {
        let newer = serde_json::json!({ "version": "99.0.0", "platform": PLATFORM });

        assert_eq!(newer_in(&newer), Some("99.0.0".to_owned()));
        assert_eq!(newer_in(&serde_json::json!({ "version": "99.0.0", "platform": "outra-plataforma" })), None);
        assert_eq!(newer_in(&serde_json::json!({ "version": "0.0.1", "platform": PLATFORM })), None);
        assert_eq!(newer_in(&serde_json::json!({ "platform": PLATFORM })), None);
    }

    /// `printf unkvoid | tauri signer sign` com a privada do par `9a18c9243ef59b08`.
    const SAMPLE_SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVTYUdNa2tQdldiQ0hGSW5yRjYxbXFhTG1iV2M1YUVyY0xjTzBQSjBUNmpaQVVjMEhMUldScTQyelhxcmhmaS9waTExdFdFcXc4VWRLVGY2UzhFRlJvOTY1TlVJb1p5MVF3PQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkwNDM1OTM0CWZpbGU6YW1vc3RyYQpscGFIaFNVdVlIbERYcUZodDJaVDlGcENqd0IzK2VGL3A5UVZzdGJnYUoxOEZqbHh2MjdvSmJ0TTFXWWdPd1d4MXM0VGFkQ09CZXcwd2FQNElmSjBDdz09Cg==";

    #[test]
    fn versions_compare_by_number_and_not_by_letter() {
        assert!(is_newer("1.10.0", "1.9.3"));
        assert!(is_newer("0.0.41", "0.0.40"));
        assert!(!is_newer("0.0.40", "0.0.40"));
        assert!(!is_newer("0.9.9", "1.0.0"));
    }

    #[test]
    fn a_beta_comes_after_the_old_versions_and_before_its_release() {
        assert!(is_newer("0.1.0-beta", "0.0.40"));
        assert!(is_newer("0.1.0", "0.1.0-beta"));
        assert!(!is_newer("0.1.0-beta", "0.1.0"));
        assert!(!is_newer("lixo", "0.0.1"));
    }

    #[test]
    fn what_our_key_signed_passes_and_nothing_else_does() {
        assert!(signed(b"unkvoid", SAMPLE_SIGNATURE));

        assert!(!signed(b"unkvoiD", SAMPLE_SIGNATURE), "bytes trocados passaram");
        assert!(!signed(b"unkvoid", "não é base64"));
        assert!(!signed(b"unkvoid", ""));
    }
}
