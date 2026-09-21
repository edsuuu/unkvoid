//! A tela de quem tem conta: servidores, canais, chat e membros.
//!
//! Quem autoriza é o Laravel. Aqui só se esconde botão — e quando o servidor responde 403,
//! a frase dele é que aparece.

use std::cell::RefCell;
use std::rc::Rc;

use core_app::models::{Channel, ChannelKind, Message, ServerSummary, ServerTree, User};
use gtk::prelude::*;

use crate::bridge::Bridge;
use crate::components::{
    body, button, clear_box, clear_list, column, field, item, label_mono, list, muted, row, scroll, strong, title,
};
use crate::streaming::Mine;
use crate::user_bar::UserBar;

/// A coluna da esquerda e as dos canais e membros. A primeira é mais larga porque a barra de
/// quem está logado mora nela, e os botões de voz têm tamanho fixo.
const RAIL_WIDTH: i32 = 240;
const COLUMN_WIDTH: i32 = 220;

/// A linha de um canal: o desenho à esquerda e o nome ao lado, como no React. Emoji ficaria
/// à mercê da fonte do sistema — e no contêiner mínimo ela não existe, então vira quadrado.
fn channel_row(channel: &Channel) -> gtk::Box {
    let line = row(8);
    let mark = if channel.kind == ChannelKind::Voice { "speaker" } else { "hash" };

    line.append(&crate::icons::icon(mark, 15, crate::icons::DIM));
    line.append(&body(&channel.name));
    crate::components::pad(&line, 4);

    line
}

/// Uma coluna da tela: painel de vidro com a folga de dentro do React.
fn panel(width: i32) -> gtk::Box {
    let panel = column(8);

    panel.add_css_class("panel");
    panel.set_size_request(width, -1);

    panel
}

pub struct HubScreen {
    root: gtk::Box,
    servers: gtk::ListBox,
    server_ids: Rc<RefCell<Vec<i64>>>,
    channels: gtk::ListBox,
    channel_ids: Rc<RefCell<Vec<Channel>>>,
    members: gtk::ListBox,
    messages: gtk::Box,
    messages_scroll: gtk::ScrolledWindow,
    composer: gtk::Entry,
    server_name: gtk::Label,
    status: gtk::Label,
    greeting: gtk::Label,
    bar: UserBar,
}

impl HubScreen {
    pub fn new(bridge: &Rc<Bridge>) -> Self {
        let servers = list(true);
        let server_ids = Rc::new(RefCell::new(Vec::new()));
        let channels = list(true);
        let channel_ids: Rc<RefCell<Vec<Channel>>> = Rc::new(RefCell::new(Vec::new()));
        let members = list(false);
        let messages = column(6);
        let messages_scroll = scroll(&messages);
        let composer = field("Escreva uma mensagem");
        let server_name = title("Servidores");
        let channel_name = strong("");
        let status = muted("");
        let greeting = muted("");
        let open_channel: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

        // Cada coluna é um painel de vidro, como no React: fundo, contorno e canto. Sem isto
        // as listas ficam soltas no fundo da janela e a tela perde a divisão.
        let to_code = button("Sala por código", "ghost");
        let rail = panel(RAIL_WIDTH);

        rail.append(&label_mono("Servidores"));
        rail.append(&scroll(&servers));
        rail.append(&to_code);

        let sidebar = panel(COLUMN_WIDTH);

        sidebar.append(&server_name);
        sidebar.append(&scroll(&channels));

        let chat = panel(-1);

        chat.set_hexpand(true);
        chat.append(&channel_name);
        chat.append(&messages_scroll);
        chat.append(&composer);

        let people = panel(COLUMN_WIDTH);

        people.append(&label_mono("Membros"));
        people.append(&scroll(&members));

        // A coluna da esquerda termina na barra de quem está logado, como no React — e não
        // numa faixa que atravessa a janela inteira.
        let user = UserBar::new(bridge);
        let left = column(10);

        // A largura é desta coluna, não do que estiver dentro: sem o travamento, a barra de
        // voz manda na medida e empurra o chat para o canto.
        left.set_size_request(RAIL_WIDTH, -1);
        left.set_hexpand(false);
        left.append(&rail);
        left.append(user.root());

        let body = row(10);

        body.set_vexpand(true);
        body.append(&left);
        body.append(&sidebar);
        body.append(&chat);
        body.append(&people);

        let root = column(10);

        crate::components::pad(&root, 12);
        greeting.set_hexpand(true);
        status.set_halign(gtk::Align::End);
        root.append(&body);
        root.append(&status);

        servers.connect_row_activated({
            let (bridge, ids) = (bridge.clone(), server_ids.clone());

            move |_, activated| {
                if let Some(id) = ids.borrow().get(activated.index() as usize) {
                    bridge.open_server(*id);
                }
            }
        });

        channels.connect_row_activated({
            let (bridge, ids, open) = (bridge.clone(), channel_ids.clone(), open_channel.clone());
            let opened_name = channel_name.clone();

            move |_, activated| {
                let Some(channel) = ids.borrow().get(activated.index() as usize).cloned() else {
                    return;
                };

                match channel.kind {
                    ChannelKind::Text => {
                        open.replace(Some(channel.id.clone()));
                        opened_name.set_text(&format!("# {}", channel.name));
                        bridge.open_channel(&channel.id);
                    }
                    // Compartilhar tela só existe dentro de um canal de voz, e entrar nele
                    // é a mesma sala do código — com o token de 60 s no lugar do nome.
                    ChannelKind::Voice => bridge.join_voice(&channel.id),
                }
            }
        });

        composer.connect_activate({
            let (bridge, open) = (bridge.clone(), open_channel.clone());

            move |composer| {
                let body = composer.text();
                let Some(channel) = open.borrow().clone() else {
                    return;
                };

                if body.trim().is_empty() {
                    return;
                }

                bridge.send_message(&channel, body.as_str());
                composer.set_text("");
            }
        });

        to_code.connect_clicked({
            let bridge = bridge.clone();

            move |_| bridge.show(core_app::Screen::Entry)
        });

        Self {
            root,
            servers,
            server_ids,
            channels,
            channel_ids,
            members,
            messages,
            messages_scroll,
            composer,
            server_name,
            status,
            greeting,
            bar: user,
        }
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    pub fn set_user(&self, user: Option<&User>) {
        let name = user.map(|user| user.name.clone()).unwrap_or_default();

        self.greeting.set_text(&name);
        self.bar.set_user(&name);
    }

    pub fn set_status(&self, message: &str) {
        self.status.set_text(message);
    }

    /// A voz mora aqui, não na sala por código: é esta barra que mostra o microfone aberto,
    /// o mudo do servidor e a permissão de falar.
    pub fn set_mine(&self, mine: Mine) {
        self.bar.set_mine(mine);
    }

    pub fn set_deafened(&self, deafened: bool) {
        self.bar.set_deafened(deafened);
    }

    pub fn set_servers(&self, servers: &[ServerSummary]) {
        clear_list(&self.servers);
        self.server_ids.replace(servers.iter().map(|server| server.id).collect());

        for server in servers {
            self.servers.append(&item(&strong(&server.name)));
        }

        if servers.is_empty() {
            self.servers.append(&item(&muted("Nenhum servidor ainda.")));
            self.server_ids.replace(Vec::new());
        }
    }

    pub fn set_tree(&self, tree: &ServerTree) {
        self.server_name.set_text(&tree.name);

        clear_list(&self.channels);

        let ordered = tree.ordered_channels();

        for channel in &ordered {
            self.channels.append(&item(&channel_row(channel)));
        }

        self.channel_ids.replace(ordered);

        clear_list(&self.members);

        for member in &tree.members {
            let name = member.nickname.clone().unwrap_or_else(|| member.name.clone());
            let line = row(6);

            crate::components::pad(&line, 4);
            line.append(&strong(&name));

            if member.is_owner {
                line.append(&muted("dono"));
            }

            if member.server_mute {
                line.append(&muted("mutado"));
            }

            self.members.append(&item(&line));
        }
    }

    pub fn set_messages(&self, messages: &[Message]) {
        clear_box(&self.messages);

        for message in messages {
            let line = column(2);

            crate::components::pad(&line, 4);
            line.append(&strong(&message.user.name));
            line.append(&muted(&message.body));
            self.messages.append(&line);
        }

        let adjustment = self.messages_scroll.vadjustment();

        gtk::glib::idle_add_local_once(move || adjustment.set_value(adjustment.upper()));
    }

    pub fn focus(&self) {
        self.composer.grab_focus();
    }
}
