use base64::Engine;
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::Sha256;

/// Builds the exact signed join-token shape consumed by the SFU.
pub fn join_token(room: &str, subject: &str, name: &str) -> String {
    let claims = json!({
        "room": room,
        "sub": subject,
        "name": name,
        "exp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_secs() + 300,
        "can": ["speak", "stream", "video"]
    });
    let body = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string());
    let secret = std::env::var("SFU_SECRET").unwrap_or_else(|_| "core-test-sfu-secret".into());
    let mut signer = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC key");

    signer.update(body.as_bytes());

    let signature = signer
        .finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    format!("{body}.{signature}")
}
