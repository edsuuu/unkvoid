//! A tela de quem tem conta: servidores, canais, chat, amigos e mensagens diretas.
//!
//! Quem autoriza é o Laravel. Aqui só se esconde botão — e quando o servidor responde 403,
//! a frase dele é que aparece.
//!
//! O desenho é o do `apps/desktop/ui/components/hub`: a trilha dos servidores à esquerda,
//! e ao lado dela ou a **Home** (conversas, amigos, criar sala) ou o **servidor aberto**
//! (canais, chat, membros). Quem troca é o mesmo clique que troca no React.

use std::cell::RefCell;
use std::rc::Rc;

use core_app::models::{
    Channel, ChannelKind, Conversation, DirectMessage, Friendship, FriendshipStatus, Message, Person, ServerSummary,
    ServerTree, User,
};
use gtk::prelude::*;

use crate::bridge::Bridge;
use crate::components::{
    avatar, badge, body, button, clear_box, clear_list, column, dim, field, item, label_mono, list, muted, row,
    scroll, spacer, strong, title,
};
use crate::icons;
use crate::streaming::Mine;
use crate::user_bar::UserBar;

/// A trilha dos servidores, a coluna da esquerda da Home e a dos canais e membros.
const RAIL_WIDTH: i32 = 182;
const COLUMN_WIDTH: i32 = 260;
const CHANNELS_WIDTH: i32 = 240;

pub struct HubScreen {
    root: gtk::Box,
    /// Home ou servidor aberto: as duas nunca aparecem juntas.
    main: gtk::Stack,
    /// Conversas, amigos ou a conversa aberta — os três miolos da Home.
    home: gtk::Stack,
    rail: gtk::Box,
    server_ids: Rc<RefCell<Vec<i64>>>,
    channels: gtk::ListBox,
    channel_ids: Rc<RefCell<Vec<Channel>>>,
    members: gtk::ListBox,
    messages: gtk::Box,
    messages_scroll: gtk::ScrolledWindow,
    composer: gtk::Entry,
    server_name: gtk::Label,
    invite: gtk::Label,
    status: gtk::Label,
    greeting: gtk::Label,
    /// A Home: criar servidor, conversas, amigos.
    servers_list: gtk::Box,
    conversations: gtk::Box,
    friends_list: gtk::Box,
    pending_badge: gtk::Label,
    recent: gtk::FlowBox,
    /// A conversa aberta e o que ela já disse.
    talking: Rc<RefCell<Option<Person>>>,
    talking_name: gtk::Label,
    talking_messages: gtk::Box,
    talking_scroll: gtk::ScrolledWindow,
    talking_composer: gtk::Entry,
    user: Rc<RefCell<Option<User>>>,
    bar: UserBar,
}

impl HubScreen {
    pub fn new(bridge: &Rc<Bridge>) -> Self {
        let server_ids: Rc<RefCell<Vec<i64>>> = Rc::default();
        let channel_ids: Rc<RefCell<Vec<Channel>>> = Rc::default();
        let talking: Rc<RefCell<Option<Person>>> = Rc::default();
        let user: Rc<RefCell<Option<User>>> = Rc::default();
        let open_channel: Rc<RefCell<Option<String>>> = Rc::default();

        let status = muted("");
        let greeting = title("");
        let rail = column(8);
        let main = gtk::Stack::new();
        let home = gtk::Stack::new();

        // ---- a trilha dos servidores, sempre à esquerda ----
        rail.add_css_class("panel");
        rail.set_size_request(RAIL_WIDTH, -1);

        let to_home = wide_row("home", "Home");

        rail.append(&to_home);
        rail.append(&rule());

        let servers_rail = column(8);

        rail.append(&scroll(&servers_rail));

        let new_server_button = wide_row("plus", "Criar servidor");

        rail.append(&new_server_button);

        // ---- a Home: coluna de conversas + o miolo ----
        let conversations = column(6);
        let friends_list = column(6);
        let servers_list = column(6);
        let pending_badge = badge("", "live");
        let new_server = field("Nome da sala");
        let friend_email = field("e-mail de quem você quer adicionar");
        let recent = crate::components::chip_wrap();

        let left = column(10);

        left.set_size_request(COLUMN_WIDTH, -1);
        left.set_hexpand(false);

        let tabs = crate::components::panel_box(8);
        let to_servers = wide_row("home", "Salas");
        let to_friends = wide_row("users", "Amigos");

        pending_badge.set_visible(false);
        to_friends.first_child().and_downcast::<gtk::Box>().inspect(|inside| inside.append(&pending_badge));
        tabs.append(&to_servers);
        tabs.append(&to_friends);

        let direct_panel = crate::components::panel_box(6);

        direct_panel.set_vexpand(true);
        direct_panel.append(&label_mono("Mensagens diretas"));
        direct_panel.append(&scroll(&conversations));

        let bar = UserBar::new(bridge);

        left.append(&tabs);
        left.append(&direct_panel);
        left.append(bar.root());

        home.add_named(&servers_home(bridge, &greeting, &new_server, &servers_list, &recent), Some("servers"));
        home.add_named(&friends_home(&friend_email, &friends_list), Some("friends"));

        let talking_name = strong("");
        let talking_messages = column(6);
        let talking_scroll = scroll(&talking_messages);
        let talking_composer = field("Escreva uma mensagem");

        home.add_named(
            &direct_home(&talking_name, &talking_scroll, &talking_composer),
            Some("direct"),
        );
        home.set_visible_child_name("servers");

        let home_side = row(10);

        home_side.append(&left);
        home_side.append(&home);
        home.set_hexpand(true);

        // ---- o servidor aberto: canais, chat, membros ----
        let channels = list(true);
        let members = list(false);
        let messages = column(6);
        let messages_scroll = scroll(&messages);
        let composer = field("Escreva uma mensagem");
        let server_name = title("");
        let channel_name = strong("");
        let invite = crate::components::mono("");

        let channels_column = crate::components::panel_box(8);

        channels_column.set_size_request(CHANNELS_WIDTH, -1);
        channels_column.append(&server_name);
        channels_column.append(&scroll(&channels));

        let invite_line = row(8);

        invite_line.add_css_class("panel");
        invite_line.append(&muted("Convite da sala"));
        invite.add_css_class("code-chip");
        invite_line.append(&invite);

        // A faixa do convite mora acima do chat, dentro do servidor — como no `ServerView`.
        let chat_side = column(10);

        chat_side.set_hexpand(true);
        chat_side.append(&invite_line);

        let chat = crate::components::panel_box(8);

        chat.set_hexpand(true);
        chat.set_vexpand(true);
        chat.append(&channel_name);
        chat.append(&messages_scroll);
        chat.append(&composer);
        chat_side.append(&chat);

        let people = crate::components::panel_box(8);

        people.set_size_request(CHANNELS_WIDTH, -1);
        people.append(&label_mono("Membros"));
        people.append(&scroll(&members));

        let server_side = row(10);

        server_side.append(&channels_column);
        server_side.append(&chat_side);
        server_side.append(&people);

        main.add_named(&home_side, Some("home"));
        main.add_named(&server_side, Some("server"));
        main.set_visible_child_name("home");
        main.set_hexpand(true);

        let body_row = row(10);

        body_row.set_vexpand(true);
        body_row.append(&rail);
        body_row.append(&main);

        let root = column(10);

        crate::components::pad(&root, 12);
        status.set_halign(gtk::Align::End);
        root.append(&body_row);
        root.append(&status);

        // ---- o que cada clique faz ----
        to_home.connect_clicked({
            let (main, home) = (main.clone(), home.clone());
            let bridge = bridge.clone();

            move |_| {
                main.set_visible_child_name("home");
                home.set_visible_child_name("servers");
                bridge.load_servers();
                bridge.load_conversations();
            }
        });

        to_servers.connect_clicked({
            let (main, home) = (main.clone(), home.clone());
            let (chosen, other) = (to_servers.clone(), to_friends.clone());

            move |_| {
                main.set_visible_child_name("home");
                home.set_visible_child_name("servers");
                choose(&chosen, &other);
            }
        });

        to_friends.connect_clicked({
            let (main, home) = (main.clone(), home.clone());
            let (chosen, other) = (to_friends.clone(), to_servers.clone());
            let bridge = bridge.clone();

            move |_| {
                main.set_visible_child_name("home");
                home.set_visible_child_name("friends");
                choose(&chosen, &other);
                bridge.load_friends();
            }
        });

        choose(&to_servers, &to_friends);

        new_server_button.connect_clicked({
            let (main, home) = (main.clone(), home.clone());

            move |_| {
                main.set_visible_child_name("home");
                home.set_visible_child_name("servers");
            }
        });


        friend_email.connect_activate({
            let bridge = bridge.clone();

            move |entry| {
                let email = entry.text();

                if email.trim().is_empty() {
                    return;
                }

                bridge.add_friend(email.as_str());
                entry.set_text("");
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
                let written = composer.text();
                let Some(channel) = open.borrow().clone() else {
                    return;
                };

                if written.trim().is_empty() {
                    return;
                }

                bridge.send_message(&channel, written.as_str());
                composer.set_text("");
            }
        });

        talking_composer.connect_activate({
            let (bridge, talking) = (bridge.clone(), talking.clone());

            move |composer| {
                let written = composer.text();
                let Some(person) = talking.borrow().clone() else {
                    return;
                };

                if written.trim().is_empty() {
                    return;
                }

                bridge.send_direct(person, written.as_str());
                composer.set_text("");
            }
        });

        Self {
            root,
            main,
            home,
            rail: servers_rail,
            server_ids,
            channels,
            channel_ids,
            members,
            messages,
            messages_scroll,
            composer,
            server_name,
            invite,
            status,
            greeting,
            servers_list,
            conversations,
            friends_list,
            pending_badge,
            recent,
            talking,
            talking_name,
            talking_messages,
            talking_scroll,
            talking_composer,
            user,
            bar,
        }
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    pub fn set_user(&self, person: Option<&User>) {
        let name = person.map(|person| person.name.clone()).unwrap_or_default();

        self.greeting.set_text(&format!("Oi, {name}."));
        self.user.replace(person.cloned());
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

    pub fn set_servers(&self, servers: &[ServerSummary], bridge: &Rc<Bridge>) {
        self.server_ids.replace(servers.iter().map(|server| server.id).collect());

        clear_box(&self.rail);
        clear_box(&self.servers_list);

        if servers.is_empty() {
            self.servers_list.append(&muted("Nenhuma ainda. Crie uma ao lado ou entre com um convite."));
        }

        let mine = self.user.borrow().as_ref().map(|person| person.id);

        for server in servers {
            self.rail.append(&server_button(server, bridge));
            self.servers_list.append(&server_line(server, mine, bridge));
        }
    }

    pub fn set_tree(&self, tree: &ServerTree) {
        self.server_name.set_text(&tree.name);
        // Só quem pode convidar recebe o código; para os outros ele nem vem na árvore.
        self.invite.set_text(tree.invite_code.as_deref().unwrap_or(""));
        self.main.set_visible_child_name("server");

        clear_list(&self.channels);

        let ordered = tree.ordered_channels();

        for channel in &ordered {
            self.channels.append(&item(&channel_row(channel)));
        }

        self.channel_ids.replace(ordered);

        clear_list(&self.members);

        for member in &tree.members {
            self.members.append(&item(&member_row(&member.name)));
        }
    }

    pub fn set_messages(&self, messages: &[Message]) {
        clear_box(&self.messages);

        for message in messages {
            self.messages.append(&message_row(&message.user.name, &message.body));
        }

        scroll_to_end(&self.messages_scroll);
    }

    pub fn set_friends(&self, friends: &[Friendship], bridge: &Rc<Bridge>) {
        clear_box(&self.friends_list);

        let mine = self.user.borrow().as_ref().map(|person| person.id);
        let waiting = friends
            .iter()
            .filter(|friend| friend.status == FriendshipStatus::Pending && Some(friend.addressee.id) == mine)
            .count();

        self.pending_badge.set_text(&waiting.to_string());
        self.pending_badge.set_visible(waiting > 0);

        if friends.is_empty() {
            self.friends_list.append(&muted("Nenhum amigo ainda."));
        }

        for friend in friends {
            self.friends_list.append(&friend_row(friend, mine, bridge));
        }
    }

    pub fn set_conversations(&self, conversations: &[Conversation], bridge: &Rc<Bridge>) {
        clear_box(&self.conversations);

        if conversations.is_empty() {
            self.conversations.append(&muted("Nenhuma conversa ainda."));
        }

        for conversation in conversations {
            self.conversations.append(&conversation_row(conversation, bridge));
        }
    }

    /// A conversa aberta. O nome no topo e as falas embaixo, como no `DirectPanel`.
    pub fn set_direct(&self, person: &Person, messages: &[DirectMessage]) {
        self.talking.replace(Some(person.clone()));
        self.talking_name.set_text(&person.name);

        clear_box(&self.talking_messages);

        for message in messages {
            self.talking_messages.append(&message_row(&message.sender.name, &message.body));
        }

        self.main.set_visible_child_name("home");
        self.home.set_visible_child_name("direct");
        scroll_to_end(&self.talking_scroll);
    }

    pub fn refresh_recent(&self, bridge: &Rc<Bridge>) {
        crate::components::clear_flow(&self.recent);

        for code in bridge.recent_rooms().into_iter().take(6) {
            let again = button(&code, "chip");

            again.connect_clicked({
                let (bridge, code) = (bridge.clone(), code.clone());

                move |_| bridge.join_room("", &code)
            });

            self.recent.insert(&again, -1);
        }
    }

    pub fn focus(&self) {
        self.composer.grab_focus();
    }

    pub fn focus_direct(&self) {
        self.talking_composer.grab_focus();
    }
}

/// O cartão da Home: criar sala, sala por código e as últimas salas.
///
/// O React abre um modal no "Tenho um convite"; aqui o mesmo botão troca o campo de cima
/// entre o nome da sala nova e o código do convite. Os textos são os dele.
fn servers_home(
    bridge: &Rc<Bridge>,
    greeting: &gtk::Label,
    name: &gtk::Entry,
    servers: &gtk::Box,
    recent: &gtk::FlowBox,
) -> gtk::Box {
    // Os três cartões quebram a linha em vez de esticar a janela, como o `flex-wrap` do
    // React: numa janela estreita eles empilham, e a janela nunca manda na largura.
    let cards = gtk::FlowBox::new();

    cards.set_selection_mode(gtk::SelectionMode::None);
    cards.set_row_spacing(10);
    cards.set_column_spacing(10);
    cards.set_max_children_per_line(3);
    cards.set_homogeneous(false);

    let create = crate::components::panel_box(10);
    let make = button("Criar sala", "primary");
    let by_invite = button("Tenho um convite", "ghost");
    let joining = Rc::new(std::cell::Cell::new(false));

    create.set_size_request(320, -1);
    create.append(&label_mono("Home"));
    create.append(greeting);
    create.append(&muted(
        "Uma sala nova já vem com um canal de texto e um de voz. Depois é só mandar o convite.",
    ));
    create.append(name);
    create.append(&make);
    create.append(&by_invite);

    let send = {
        let (bridge, name, joining) = (bridge.clone(), name.clone(), joining.clone());

        move || {
            let written = name.text();

            if written.trim().is_empty() {
                return;
            }

            if joining.get() {
                bridge.join_invite(written.as_str());
            } else {
                bridge.create_server(written.as_str());
            }

            name.set_text("");
        }
    };

    make.connect_clicked({
        let send = send.clone();

        move |_| send()
    });

    name.connect_activate({
        let send = send.clone();

        move |_| send()
    });

    by_invite.connect_clicked({
        let (name, joining, make) = (name.clone(), joining.clone(), make.clone());

        move |by_invite| {
            let now = !joining.get();

            joining.set(now);
            name.set_placeholder_text(Some(if now { "Código do convite" } else { "Nome da sala" }));
            name.set_text("");
            make.set_label(if now { "Entrar com o convite" } else { "Criar sala" });
            by_invite.set_label(if now { "Quero criar uma sala" } else { "Tenho um convite" });
        }
    });

    let code = crate::components::panel_box(10);
    let by_code = button("Criar ou entrar com código", "primary");

    code.set_size_request(320, -1);
    code.append(&label_mono("Só compartilhar a tela"));
    code.append(&muted("Uma sala por código, sem servidor: quem tiver o código assiste."));
    code.append(&by_code);
    code.append(&label_mono("Últimas salas acessadas"));
    code.append(recent);

    by_code.connect_clicked({
        let bridge = bridge.clone();

        move |_| bridge.show(core_app::Screen::Entry)
    });

    let list = crate::components::panel_box(8);

    list.set_size_request(320, -1);
    list.append(&label_mono("Últimas salas"));
    list.append(&scroll(servers));

    cards.insert(&create, -1);
    cards.insert(&code, -1);
    cards.insert(&list, -1);

    let holder = column(0);

    holder.set_hexpand(true);
    holder.append(&scroll(&cards));

    holder
}

fn friends_home(email: &gtk::Entry, friends: &gtk::Box) -> gtk::Box {
    let panel = crate::components::panel_box(10);

    panel.set_hexpand(true);
    panel.append(&label_mono("Amigos"));
    panel.append(&muted("Adicione pela conta: o e-mail é o que o servidor conhece."));
    panel.append(email);
    panel.append(&scroll(friends));

    panel
}

fn direct_home(name: &gtk::Label, messages: &gtk::ScrolledWindow, composer: &gtk::Entry) -> gtk::Box {
    let panel = crate::components::panel_box(10);

    panel.set_hexpand(true);
    panel.append(name);
    panel.append(messages);
    panel.append(composer);

    panel
}

/// Uma linha larga da trilha: o desenho num quadrado e o nome ao lado.
fn wide_row(icon: &str, label: &str) -> gtk::Button {
    let line = gtk::Button::new();
    let inside = row(10);

    inside.append(&icons::icon(icon, 16, icons::RESTING));
    inside.append(&body(label));
    inside.append(&spacer());
    line.set_child(Some(&inside));
    line.add_css_class("wide");
    crate::components::clickable(&line);

    line
}

fn server_button(server: &ServerSummary, bridge: &Rc<Bridge>) -> gtk::Button {
    let line = gtk::Button::new();
    let inside = row(10);

    inside.append(&avatar(&server.name, 34, false));
    inside.append(&body(&server.name));
    inside.append(&spacer());
    line.set_child(Some(&inside));
    line.add_css_class("wide");
    line.set_tooltip_text(Some(&server.name));
    crate::components::clickable(&line);

    line.connect_clicked({
        let (bridge, id) = (bridge.clone(), server.id);

        move |_| bridge.open_server(id)
    });

    line
}

fn server_line(server: &ServerSummary, mine: Option<i64>, bridge: &Rc<Bridge>) -> gtk::Button {
    let line = gtk::Button::new();
    let inside = row(10);
    let texts = column(2);

    texts.append(&strong(&server.name));
    texts.append(&label_mono(if Some(server.owner_id) == mine { "dono" } else { "membro" }));
    inside.append(&avatar(&server.name, 32, false));
    inside.append(&texts);
    inside.append(&spacer());
    line.set_child(Some(&inside));
    line.add_css_class("wide");
    crate::components::clickable(&line);

    line.connect_clicked({
        let (bridge, id) = (bridge.clone(), server.id);

        move |_| bridge.open_server(id)
    });

    line
}

fn conversation_row(conversation: &Conversation, bridge: &Rc<Bridge>) -> gtk::Button {
    let line = gtk::Button::new();
    let inside = row(10);
    let texts = column(2);
    let last = if conversation.last.mine {
        format!("você: {}", conversation.last.body)
    } else {
        conversation.last.body.clone()
    };

    texts.append(&body(&conversation.user.name));
    texts.append(&dim(&last));
    inside.append(&avatar(&conversation.user.name, 28, false));
    inside.append(&texts);
    inside.append(&spacer());

    if conversation.unread > 0 {
        inside.append(&badge(&conversation.unread.to_string(), "live"));
    }

    line.set_child(Some(&inside));
    line.add_css_class("wide");
    crate::components::clickable(&line);

    line.connect_clicked({
        let (bridge, person) = (bridge.clone(), conversation.user.clone());

        move |_| bridge.open_conversation(person.clone())
    });

    line
}

/// Uma amizade. O pedido que chegou ganha "Aceitar" e "Recusar"; o que saiu, só a espera.
fn friend_row(friend: &Friendship, mine: Option<i64>, bridge: &Rc<Bridge>) -> gtk::Box {
    let line = row(10);
    let other = if Some(friend.requester.id) == mine { &friend.addressee } else { &friend.requester };

    line.add_css_class("person");
    line.append(&avatar(&other.name, 28, false));
    line.append(&body(&other.name));
    line.append(&spacer());

    match friend.status {
        FriendshipStatus::Accepted => {
            let talk = button("Conversar", "ghost");

            talk.connect_clicked({
                let (bridge, person) = (bridge.clone(), other.clone());

                move |_| bridge.open_conversation(person.clone())
            });
            line.append(&talk);
        }
        FriendshipStatus::Pending if Some(friend.addressee.id) == mine => {
            let accept = button("Aceitar", "primary");
            let refuse = button("Recusar", "danger");

            accept.connect_clicked({
                let (bridge, id) = (bridge.clone(), friend.id);

                move |_| bridge.answer_friend(id, true)
            });
            refuse.connect_clicked({
                let (bridge, id) = (bridge.clone(), friend.id);

                move |_| bridge.answer_friend(id, false)
            });
            line.append(&accept);
            line.append(&refuse);
        }
        FriendshipStatus::Pending => line.append(&dim("aguardando")),
        FriendshipStatus::Blocked => line.append(&dim("bloqueado")),
    }

    line
}

/// A linha de um canal: o desenho à esquerda e o nome ao lado, como no React. Emoji ficaria
/// à mercê da fonte do sistema — e no contêiner mínimo ela não existe, então vira quadrado.
fn channel_row(channel: &Channel) -> gtk::Box {
    let line = row(8);
    let mark = if channel.kind == ChannelKind::Voice { "speaker" } else { "hash" };

    line.append(&icons::icon(mark, 15, icons::DIM));
    line.append(&body(&channel.name));
    crate::components::pad(&line, 4);

    line
}

fn member_row(name: &str) -> gtk::Box {
    let line = row(10);

    line.add_css_class("person");
    line.append(&avatar(name, 24, false));
    line.append(&body(name));

    line
}

fn message_row(author: &str, written: &str) -> gtk::Box {
    let line = row(10);
    let texts = column(2);

    texts.append(&strong(author));
    texts.append(&body(written));
    line.add_css_class("person");
    line.append(&avatar(author, 28, false));
    line.append(&texts);

    line
}

/// A aba escolhida fica marcada e a outra apaga — o `row-item-on` do React.
fn choose(chosen: &gtk::Button, other: &gtk::Button) {
    chosen.add_css_class("on");
    other.remove_css_class("on");
}

fn rule() -> gtk::Box {
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 0);

    line.add_css_class("rule");
    line.set_hexpand(true);

    line
}

/// O chat abre no fim: mensagem nova aparece sem que ninguém role.
fn scroll_to_end(view: &gtk::ScrolledWindow) {
    let adjustment = view.vadjustment();

    glib_idle(move || adjustment.set_value(adjustment.upper()));
}

fn glib_idle(work: impl FnOnce() + 'static) {
    let cell = std::cell::Cell::new(Some(work));

    gtk::glib::idle_add_local(move || {
        if let Some(work) = cell.take() {
            work();
        }

        gtk::glib::ControlFlow::Break
    });
}
