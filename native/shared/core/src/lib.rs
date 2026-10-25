//! A lógica do app, compartilhada pelas interfaces nativas que vierem.
//!
//! O que está aqui decide; a pasta do app só desenha e manda eventos. Regra que aparecer nela
//! terá de ser reescrita por cada interface futura — e é assim que um app acumula os mesmos
//! defeitos em lugares diferentes.

pub mod activity;
pub mod api;
pub mod app;
pub mod chimes;
pub mod e2ee_group;
pub mod e2ee_call;
pub mod client;
pub mod failure;
pub mod google;
pub mod logbook;
pub mod members;
pub mod models;
pub mod permissions;
pub mod protocol;
pub mod realtime;
pub mod reconnect;
pub mod resume;
pub mod room;
pub mod room_code;
pub mod routes;
pub mod session;
pub mod sharing;
pub mod speaking;
pub mod update;
pub mod voice;
pub mod watching;

pub use api::{Api, HttpError};
pub use app::{App, AppState, EntryRefusal};
pub use client::SfuClient;
pub use failure::Failure;
pub use models::Screen;
pub use protocol::{Event, ServerError};
pub use session::{Identity, Roster, Session};
pub use voice::VoiceFilterConfig;
