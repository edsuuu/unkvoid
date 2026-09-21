//! O Unkvoid no Linux: GTK4 por cima do `shared/core`, sem ponte nenhuma no meio.
//!
//! A janela é uma pilha com as cinco telas; quem diz qual está valendo é o núcleo. Este
//! arquivo só escuta a fila do `Bridge` e manda cada novidade para a tela que a desenha.

mod bridge;
mod components;
mod devices;
mod icons;
mod screens;
mod sending;
mod streaming;
mod user_bar;
mod watching;

use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, CssProvider, Stack, gdk, glib};

use bridge::{Bridge, Update};
use core_app::Screen;
use screens::{EntryScreen, HubScreen, OfflineScreen, RoomScreen, UpdatingScreen};

const APP_ID: &str = "com.unkvoid.desktop";

fn main() -> glib::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let application = Application::builder().application_id(APP_ID).build();

    application.connect_startup(|_| load_theme());
    application.connect_activate(open);

    application.run()
}

fn load_theme() {
    let Some(display) = gdk::Display::default() else {
        tracing::warn!("sem display: o tema não foi carregado");

        return;
    };

    let provider = CssProvider::new();

    // `load_from_data` e não `load_from_string`: o segundo é do GTK 4.12, e a distribuição
    // que faz este app existir (o WebKitGTK do Debian sem WebRTC) costuma vir com menos.
    provider.load_from_data(include_str!("style.css"));
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn open(application: &Application) {
    let (bridge, mut updates) = match Bridge::new() {
        Ok(ready) => ready,
        Err(failure) => {
            // Sem estado em disco e sem runtime não há app: melhor dizer o porquê do que
            // abrir uma janela que não faz nada.
            tracing::error!(%failure, "o núcleo não subiu");

            return;
        }
    };

    let entry = EntryScreen::new(&bridge);
    let hub = HubScreen::new(&bridge);
    let room = RoomScreen::new(&bridge);
    let offline = OfflineScreen::new();
    let updating = UpdatingScreen::new();

    let stack = Stack::new();

    stack.add_named(entry.root(), Some("entry"));
    stack.add_named(hub.root(), Some("hub"));
    stack.add_named(room.root(), Some("room"));
    stack.add_named(offline.root(), Some("offline"));
    stack.add_named(updating.root(), Some("updating"));

    let window = ApplicationWindow::builder()
        .application(application)
        .title("Unkvoid")
        .default_width(1180)
        .default_height(760)
        .child(&stack)
        .build();

    show(&stack, Screen::Updating);
    window.present();
    bridge.start();

    glib::spawn_future_local(async move {
        while let Some(update) = updates.recv().await {
            match update {
                Update::Ready(user) => {
                    entry.set_user(user.as_ref());
                    entry.refresh_recent(&bridge);
                    hub.set_user(user.as_ref());

                    let landing = bridge.home();

                    show(&stack, landing);

                    if landing == Screen::Hub {
                        bridge.load_servers();
                        bridge.load_conversations();
                        hub.refresh_recent(&bridge);
                    } else {
                        entry.focus();
                    }
                }
                Update::Offline(status) => {
                    offline.set_status(&status);
                    updating.set_status(&status);
                    show(&stack, Screen::Offline);
                }
                Update::Complaint(message) => {
                    entry.set_error(&message);
                    hub.set_status(&message);
                    room.set_status(&message);
                }
                Update::LoginComplaint(message) => entry.set_login_error(&message),
                Update::Servers(servers) => hub.set_servers(&servers, &bridge),
                Update::Tree(tree) => hub.set_tree(&tree),
                Update::Messages(messages) => {
                    hub.set_messages(&messages);
                    hub.focus();
                }
                Update::Friends(friends) => hub.set_friends(&friends, &bridge),
                Update::Conversations(conversations) => hub.set_conversations(&conversations, &bridge),
                Update::Direct { person, messages } => {
                    hub.set_direct(&person, &messages);
                    hub.focus_direct();
                }
                // Canal de voz: a tela continua sendo o hub, e quem está dentro aparece
                // embaixo do nome do canal — como no React.
                Update::Joined { room: code, voice: Some(name), peers } => {
                    hub.set_status("");
                    hub.set_voice(Some((&code, &name)));
                    hub.set_voice_peers(&peers);
                }
                Update::Joined { room: code, voice: None, peers } => {
                    entry.set_error("");
                    room.set_room(&code);
                    room.set_peers(&peers);
                    show(&stack, Screen::Room);
                }
                Update::VoiceLeft => hub.set_voice(None),
                Update::Peers(peers) => {
                    room.set_peers(&peers);
                    hub.set_voice_peers(&peers);
                }
                Update::Ping(milliseconds) => {
                    room.set_ping(milliseconds);
                    hub.set_ping(milliseconds);
                }
                Update::Tiles(tiles) => room.set_tiles(&tiles),
                Update::Mine(mine) => {
                    // A sala por código só tem tela; o microfone e o som são do hub, que é
                    // onde a voz existe. Cada tela pega o pedaço que desenha.
                    room.set_mine(mine);
                    hub.set_mine(mine);
                    hub.set_deafened(bridge.is_deafened());
                }
                Update::Show(screen) => {
                    if screen == Screen::Hub {
                        bridge.load_servers();
                    }

                    show(&stack, screen);
                }
            }
        }
    });
}

/// O nome de cada tela na pilha. A tradução mora num lugar só para uma tela nova não poder
/// ser esquecida aqui.
fn show(stack: &Stack, screen: Screen) {
    stack.set_visible_child_name(match screen {
        Screen::Entry => "entry",
        Screen::Hub => "hub",
        Screen::Room => "room",
        Screen::Offline => "offline",
        Screen::Updating => "updating",
    });
}

#[cfg(test)]
mod tests {
    use core_app::Screen;

    #[test]
    fn leaving_a_room_lands_where_the_core_says() {
        // Se o núcleo mudar de ideia sobre onde se cai ao sair, este teste cai junto — e é
        // ele que prova que a ligação com o `core` não é decorativa.
        assert_eq!(Screen::home(true), Screen::Hub);
        assert_eq!(Screen::home(false), Screen::Entry);
    }

    #[test]
    fn the_core_validates_the_code_and_this_app_does_not() {
        assert!(core_app::room_code::is_valid(&core_app::room_code::generate()));
        assert!(!core_app::room_code::is_valid("-nao-"));
    }
}
