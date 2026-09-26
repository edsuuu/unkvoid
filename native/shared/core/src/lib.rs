//! A lógica do app, compartilhada pelas três interfaces nativas.
//!
//! O que está aqui decide; as pastas de sistema (`apps/macos`, `apps/windows`,
//! `apps/linux`) só desenham e mandam eventos. Regra que aparecer lá terá de ser escrita
//! três vezes — e é assim que um app vira três apps com os mesmos defeitos em lugares
//! diferentes.

pub mod api;
pub mod app;
pub mod chimes;
pub mod client;
pub mod failure;
pub mod ffi;
pub mod google;
pub mod keymap;
pub mod members;
pub mod models;
pub mod permissions;
pub mod protocol;
pub mod realtime;
pub mod reconnect;
pub mod room;
pub mod room_code;
pub mod routes;
pub mod session;
pub mod sharing;
pub mod speaking;
pub mod update;
pub mod watching;

pub use api::{Api, HttpError};
pub use app::{App, AppState, EntryRefusal};
pub use client::SfuClient;
pub use failure::Failure;
pub use models::Screen;
pub use protocol::{Event, ServerError};
pub use session::{Identity, Roster, Session};
