//! O que não pode ficar legível no disco: hoje, o token da conta.
//!
//! O arquivo guarda o texto cifrado; a chave que o abre mora no chaveiro do sistema
//! (Keychain, Credential Manager, Secret Service). Separar os dois é o que faz o arquivo
//! sozinho não valer nada — quem copiar o `state.json` de uma máquina não entra em conta
//! nenhuma.

use std::fs;
use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rand::RngCore;

const SERVICE: &str = "com.unkvoid.desktop";

const ENTRY: &str = "state-key";

/// 96 bits é o tamanho que o AES-GCM espera; outro valor obriga a uma derivação interna e
/// perde a garantia de que dois nonces diferentes nunca colidem.
const NONCE_BYTES: usize = 12;

pub struct Cipher {
    key: Key<Aes256Gcm>,
}

impl Cipher {
    /// Pega a chave do chaveiro, criando uma na primeira vez.
    ///
    /// **Em desenvolvimento o chaveiro não é usado.** O macOS libera um item por assinatura
    /// do binário, e cada `cargo build` produz uma assinatura nova: o sistema trata como
    /// outro programa e pede a senha toda vez. A chave vai para um arquivo só do dono, ao
    /// lado do estado — o que se protege aqui é o token de uma máquina de desenvolvimento,
    /// não o de quem usa o app.
    ///
    /// `UNKVOID_KEYCHAIN=1` força o chaveiro mesmo em debug, para conferir o caminho real.
    pub fn open_for(dir: &Path) -> Result<Self> {
        let forced = std::env::var("UNKVOID_KEYCHAIN").is_ok_and(|value| value == "1");

        if cfg!(debug_assertions) && !forced {
            return Self::from_file(dir);
        }

        Self::open()
    }

    /// A chave num arquivo que só o dono lê (0600). Some junto com o estado, e um token
    /// perdido custa um login.
    fn from_file(dir: &Path) -> Result<Self> {
        let path = dir.join("dev-key");

        if let Ok(key) = fs::read_to_string(&path).map_err(drop).and_then(|stored| Self::decode(stored.trim()).map_err(drop)) {
            return Ok(Self { key: *Key::<Aes256Gcm>::from_slice(&key) });
        }

        let fresh = Self::generate();

        fs::write(&path, STANDARD.encode(fresh))
            .with_context(|| format!("não deu para gravar {}", path.display()))?;

        #[cfg(unix)]
        fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .with_context(|| format!("não deu para fechar {}", path.display()))?;

        Ok(Self { key: *Key::<Aes256Gcm>::from_slice(&fresh) })
    }

    pub fn open() -> Result<Self> {
        let entry = keyring::Entry::new(SERVICE, ENTRY)
            .context("o chaveiro do sistema não pôde ser aberto")?;

        let key = match entry.get_password() {
            Ok(stored) => Self::decode(&stored)?,
            Err(_) => {
                let fresh = Self::generate();

                entry
                    .set_password(&STANDARD.encode(fresh))
                    .context("não deu para guardar a chave no chaveiro")?;

                fresh
            }
        };

        Ok(Self { key: *Key::<Aes256Gcm>::from_slice(&key) })
    }

    /// Uma chave solta, sem tocar no chaveiro: é o que deixa o teste rodar em máquina
    /// sem sessão gráfica, onde o Secret Service não existe.
    #[cfg(test)]
    pub fn test_only() -> Self {
        Self { key: *Key::<Aes256Gcm>::from_slice(&Self::generate()) }
    }

    fn generate() -> [u8; 32] {
        let mut key = [0_u8; 32];

        OsRng.fill_bytes(&mut key);

        key
    }

    fn decode(stored: &str) -> Result<[u8; 32]> {
        let raw = STANDARD.decode(stored).context("the keychain value is not base64")?;

        raw.try_into().map_err(|_| anyhow!("the keychain key is not 32 bytes"))
    }

    /// Um nonce novo a cada escrita. Repetir nonce com a mesma chave quebra o GCM por
    /// inteiro — não é economia que se faça aqui.
    pub fn encrypt(&self, plain: &str) -> Result<String> {
        let mut nonce = [0_u8; NONCE_BYTES];

        OsRng.fill_bytes(&mut nonce);

        let sealed = Aes256Gcm::new(&self.key)
            .encrypt(Nonce::from_slice(&nonce), plain.as_bytes())
            .map_err(|_| anyhow!("não deu para cifrar"))?;

        let mut joined = nonce.to_vec();

        joined.extend_from_slice(&sealed);

        Ok(STANDARD.encode(joined))
    }

    pub fn decrypt(&self, sealed: &str) -> Result<String> {
        let raw = STANDARD.decode(sealed).context("o valor cifrado não é base64")?;

        if raw.len() <= NONCE_BYTES {
            return Err(anyhow!("o valor cifrado está truncado"));
        }

        let (nonce, body) = raw.split_at(NONCE_BYTES);

        let plain = Aes256Gcm::new(&self.key)
            .decrypt(Nonce::from_slice(nonce), body)
            .map_err(|_| anyhow!("o valor cifrado não abre com esta chave"))?;

        String::from_utf8(plain).context("o valor decifrado não é texto")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher() -> Cipher {
        Cipher::test_only()
    }

    #[test]
    fn what_is_sealed_comes_back_the_same() {
        let cipher = cipher();
        let sealed = cipher.encrypt("a-secret-token").expect("seal");

        assert_ne!(sealed, "a-secret-token");
        assert_eq!(cipher.decrypt(&sealed).expect("unseal"), "a-secret-token");
    }

    #[test]
    fn sealing_the_same_text_twice_never_repeats_the_nonce() {
        let cipher = cipher();

        assert_ne!(
            cipher.encrypt("same text").expect("seal"),
            cipher.encrypt("same text").expect("seal"),
            "repeated nonce: GCM loses its guarantee"
        );
    }

    #[test]
    fn another_key_does_not_open_it() {
        let sealed = cipher().encrypt("a-secret-token").expect("seal");

        assert!(cipher().decrypt(&sealed).is_err());
    }

    #[test]
    fn tampered_text_does_not_open() {
        let cipher = cipher();
        let sealed = cipher.encrypt("a-secret-token").expect("seal");
        let mut raw = STANDARD.decode(&sealed).expect("decode");
        let last = raw.len() - 1;

        raw[last] ^= 0xff;

        assert!(cipher.decrypt(&STANDARD.encode(raw)).is_err());
    }

    #[test]
    fn the_development_key_is_reused_and_kept_to_the_owner() {
        let dir = tempfile::tempdir().expect("temp dir");
        let sealed = Cipher::open_for(dir.path()).expect("open").encrypt("1|token").expect("seal");

        // Reabrir tem de achar a mesma chave: senão cada build deslogava a pessoa.
        assert_eq!(
            Cipher::open_for(dir.path()).expect("reopen").decrypt(&sealed).expect("unseal"),
            "1|token",
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mode = fs::metadata(dir.path().join("dev-key")).expect("stat").permissions().mode();

            assert_eq!(mode & 0o077, 0, "a chave está legível para outros: {mode:o}");
        }
    }

    #[test]
    fn the_development_key_still_seals_what_goes_to_disk() {
        let dir = tempfile::tempdir().expect("temp dir");
        let sealed = Cipher::open_for(dir.path()).expect("open").encrypt("1|token").expect("seal");

        // Sem chaveiro o token continua cifrado — o que muda é onde a chave mora.
        assert!(!sealed.contains("token"), "o token ficou legível: {sealed}");
    }

    #[test]
    fn a_truncated_value_does_not_open() {
        assert!(cipher().decrypt(&STANDARD.encode([0_u8; 4])).is_err());
    }
}
