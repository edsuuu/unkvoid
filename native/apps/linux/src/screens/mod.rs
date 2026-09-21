//! Uma tela por arquivo. São as cinco que o núcleo escolhe, e nenhuma a mais.

pub mod entry;
pub mod hub;
pub mod offline;
pub mod room;
pub mod updating;

pub use entry::EntryScreen;
pub use hub::HubScreen;
pub use offline::OfflineScreen;
pub use room::RoomScreen;
pub use updating::UpdatingScreen;
