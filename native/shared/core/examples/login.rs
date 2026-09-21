//! Entra numa conta e guarda o token cifrado na pasta de estado, como o app faria. Serve
//! para abrir uma interface já logada numa pasta separada (`UNKVOID_STATE_DIR`).
//!
//! ```bash
//! UNKVOID_STATE_DIR=/tmp/ada cargo run -p core-app --example login -- http://127.0.0.1:8000 ada@teste.local <senha>
//! ```

use core_app::{Api, App};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut arguments = std::env::args().skip(1);
    let (Some(server), Some(email), Some(password)) =
        (arguments.next(), arguments.next(), arguments.next())
    else {
        anyhow::bail!("uso: login <servidor> <e-mail> <senha>");
    };

    let api = Api::new(&server)?;
    let entered = api
        .login(&email, &password, "exemplo")
        .await
        .map_err(|failure| anyhow::anyhow!("{failure:?}"))?;

    App::new(storage::Storage::open()?).set_token(Some(&entered.token));

    println!(
        "token guardado em {}",
        std::env::var("UNKVOID_STATE_DIR").unwrap_or_else(|_| "a pasta do sistema".into())
    );

    Ok(())
}
