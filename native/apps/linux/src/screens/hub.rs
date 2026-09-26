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

use crate::bridge::{Bridge, Peer};
use crate::components::{
    avatar, badge, body, button, clear_box, clear_list, column, dim, field, item, label_mono, list, muted, row,
    scroll, spacer, strong, title,
};
use crate::icons;
use crate::streaming::Mine;
use crate::user_bar::UserBar;

/// A trilha dos servidores, a coluna da esquerda da Home e a dos canais e membros.
const RAIL_WIDTH: i32 = 182;
/// As duas colunas de baixo têm a mesma largura porque a barra de baixo é a mesma nas duas:
/// com larguras diferentes ela se apertava na Home.
const COLUMN_WIDTH: i32 = 300;
const CHANNELS_WIDTH: i32 = 300;
const MEMBERS_WIDTH: i32 = 240;

pub struct HubScreen {
    root: gtk::Box,
    /// Home ou servidor aberto: as duas nunca aparecem juntas.
    main: gtk::Stack,
    /// Conversas, amigos ou a conversa aberta — os três miolos da Home.
    home: gtk::Stack,
    rail: gtk::Box,
    server_ids: Rc<RefCell<Vec<i64>>>,
    /// Canais de texto e de voz em duas seções, como no React. O núcleo continua com uma
    /// lista só, e cada linha guarda a posição dela nela.
    text_channels: gtk::ListBox,
    voice_channels: gtk::Box,
    channel_ids: Rc<RefCell<Vec<Channel>>>,
    /// O canal de texto aberto: é ele que editar e apagar releem.
    open_channel: Rc<RefCell<Option<String>>>,
    /// O canal de voz em que se está, e as caixas que precisam saber disso.
    voice_open: Rc<RefCell<Option<String>>>,
    voice_people: Rc<RefCell<Vec<Peer>>>,
    members: gtk::ListBox,
    messages: gtk::Box,
    messages_scroll: gtk::ScrolledWindow,
    composer: gtk::Entry,
    server_name: gtk::Label,
    invite: gtk::Label,
    status: gtk::Label,
    greeting: gtk::Label,
    /// A Home: criar servidor, conversas, amigos.
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
    server_bar: UserBar,
    bridge: Rc<Bridge>,
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

        home.add_named(&servers_home(bridge, &greeting, &new_server, &recent), Some("servers"));
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
        let text_channels = list(true);
        let voice_channels = column(6);
        let voice_open: Rc<RefCell<Option<String>>> = Rc::default();
        let voice_people: Rc<RefCell<Vec<Peer>>> = Rc::default();
        let members = list(false);
        let messages = column(6);
        let messages_scroll = scroll(&messages);
        let composer = field("Escreva uma mensagem");
        let server_name = title("");
        let channel_name = strong("");
        let invite = crate::components::mono("");

        // A coluna dos canais é a do React: o nome do servidor num cartão, os canais
        // noutro e a barra de baixo fechando.
        let channels_column = column(10);
        let server_card = crate::components::panel_box(0);
        let channels_card = crate::components::panel_box(14);
        let text_head = row(8);
        let voice_head = row(8);
        let new_text = icons::small_plus("Criar canal de texto");
        let new_voice = icons::small_plus("Criar canal de voz");
        let text_section = column(8);
        let voice_section = column(8);
        let new_channel = field("Nome do canal");
        let naming: Rc<RefCell<Option<ChannelKind>>> = Rc::default();

        channels_column.set_size_request(CHANNELS_WIDTH, -1);
        server_card.append(&server_name);

        text_head.append(&label_mono("Canais de texto"));
        text_head.append(&spacer());
        text_head.append(&new_text);
        text_section.append(&text_head);
        text_section.append(&text_channels);

        voice_head.append(&label_mono("Canais de voz"));
        voice_head.append(&spacer());
        voice_head.append(&new_voice);
        voice_section.append(&voice_head);
        voice_section.append(&voice_channels);

        new_channel.set_visible(false);

        let channels_inside = column(14);

        channels_inside.append(&text_section);
        channels_inside.append(&voice_section);
        channels_inside.append(&new_channel);
        channels_card.set_vexpand(true);
        channels_card.append(&scroll(&channels_inside));

        channels_column.append(&server_card);
        channels_column.append(&channels_card);

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

        people.set_size_request(MEMBERS_WIDTH, -1);
        people.append(&label_mono("Membros"));
        people.append(&scroll(&members));

        // A barra de baixo fecha as duas colunas, como no React: o `VoicePanel` é o mesmo
        // na Home e dentro do servidor. Um widget só não cabe em dois pais, então a do
        // servidor é outra instância da mesma coisa.
        let server_bar = UserBar::new(bridge);

        channels_column.append(server_bar.root());

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

        text_channels.connect_row_activated({
            let (bridge, ids, open) = (bridge.clone(), channel_ids.clone(), open_channel.clone());
            let opened_name = channel_name.clone();

            move |_, activated| {
                let Some(channel) = ids
                    .borrow()
                    .iter()
                    .filter(|channel| channel.kind == ChannelKind::Text)
                    .nth(activated.index() as usize)
                    .cloned()
                else {
                    return;
                };

                open.replace(Some(channel.id.clone()));
                opened_name.set_text(&format!("# {}", channel.name));
                bridge.open_channel(&channel.id);
            }
        });

        new_text.connect_clicked({
            let (field, naming) = (new_channel.clone(), naming.clone());

            move |_| open_naming(&field, &naming, ChannelKind::Text)
        });

        new_voice.connect_clicked({
            let (field, naming) = (new_channel.clone(), naming.clone());

            move |_| open_naming(&field, &naming, ChannelKind::Voice)
        });

        new_channel.connect_activate({
            let (bridge, naming) = (bridge.clone(), naming.clone());

            move |entry| {
                let written = entry.text();
                let Some(kind) = *naming.borrow() else {
                    return;
                };

                if written.trim().is_empty() {
                    return;
                }

                bridge.create_channel(written.as_str(), kind);
                entry.set_text("");
                entry.set_visible(false);
                naming.replace(None);
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
            text_channels,
            voice_channels,
            channel_ids,
            open_channel,
            voice_open,
            voice_people,
            members,
            messages,
            messages_scroll,
            composer,
            server_name,
            invite,
            status,
            greeting,
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
            server_bar,
            bridge: bridge.clone(),
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
        self.server_bar.set_user(&name);
    }

    pub fn set_status(&self, message: &str) {
        self.status.set_text(message);
    }

    /// A voz mora aqui, não na sala por código: é esta barra que mostra o microfone aberto,
    /// o mudo do servidor e a permissão de falar.
    pub fn set_mine(&self, mine: Mine) {
        self.bar.set_mine(mine);
        self.server_bar.set_mine(mine);
    }

    pub fn set_deafened(&self, deafened: bool) {
        self.bar.set_deafened(deafened);
        self.server_bar.set_deafened(deafened);
    }

    /// A ida e volta até o SFU. O sinal muda de cor com ela, e o balão do mouse diz o número.
    pub fn set_ping(&self, milliseconds: u64) {
        self.bar.set_ping(milliseconds);
        self.server_bar.set_ping(milliseconds);
    }

    pub fn set_servers(&self, servers: &[ServerSummary], bridge: &Rc<Bridge>) {
        self.server_ids.replace(servers.iter().map(|server| server.id).collect());

        clear_box(&self.rail);

        for server in servers {
            self.rail.append(&server_button(server, bridge));
        }
    }

    pub fn set_tree(&self, tree: &ServerTree) {
        self.server_name.set_text(&tree.name);
        // Só quem pode convidar recebe o código; para os outros ele nem vem na árvore.
        self.invite.set_text(tree.invite_code.as_deref().unwrap_or(""));
        self.main.set_visible_child_name("server");

        clear_list(&self.text_channels);
        clear_box(&self.voice_channels);

        let ordered = tree.ordered_channels();

        for channel in ordered.iter().filter(|channel| channel.kind == ChannelKind::Text) {
            self.text_channels.append(&item(&channel_row(channel)));
        }

        self.channel_ids.replace(ordered);
        self.paint_voice();

        clear_list(&self.members);

        for member in &tree.members {
            self.members.append(&item(&member_row(&member.name)));
        }
    }

    /// Entrou ou saiu da voz. `Some` traz o canal e o nome dele.
    pub fn set_voice(&self, voice: Option<(&str, &str)>) {
        self.voice_open.replace(voice.map(|(channel, _)| channel.to_owned()));

        if voice.is_none() {
            self.voice_people.replace(Vec::new());
        }

        self.bar.set_voice(voice.map(|(_, name)| name));
        self.server_bar.set_voice(voice.map(|(_, name)| name));
        self.paint_voice();
    }

    pub fn set_voice_peers(&self, peers: &[Peer]) {
        self.voice_people.replace(peers.to_vec());
        self.paint_voice();
    }

    /// Redesenha a seção de voz: o canal aberto se marca e quem está dentro aparece logo
    /// abaixo do nome — o `VoiceChannelItem` do React.
    fn paint_voice(&self) {
        clear_box(&self.voice_channels);

        let open = self.voice_open.borrow().clone();
        let people = self.voice_people.borrow().clone();
        let channels = self.channel_ids.borrow().clone();

        for channel in channels.iter().filter(|channel| channel.kind == ChannelKind::Voice) {
            let here = open.as_deref() == Some(channel.id.as_str());

            self.voice_channels.append(&voice_row(
                &self.bridge,
                channel,
                here,
                if here { &people } else { &[] },
            ));
        }
    }

    pub fn set_messages(&self, messages: &[Message]) {
        clear_box(&self.messages);

        let mine = self.user.borrow().as_ref().map(|person| person.id);
        let channel = self.open_channel.borrow().clone().unwrap_or_default();

        for message in messages {
            self.messages.append(&message_row(
                &self.bridge,
                &channel,
                message,
                Some(message.user.id) == mine,
            ));
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
            self.talking_messages.append(&said_row(
                &message.sender.name,
                &message.body,
                message.created_at.get(11..16).unwrap_or_default(),
            ));
        }

        self.main.set_visible_child_name("home");
        self.home.set_visible_child_name("direct");
        scroll_to_end(&self.talking_scroll);
    }

    pub fn refresh_recent(&self, bridge: &Rc<Bridge>) {
        crate::components::clear_flow(&self.recent);

        // Só as três últimas: é o que o dono quer ver na Home, e o núcleo guarda mais.
        for code in bridge.recent_rooms().into_iter().take(3) {
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
    recent: &gtk::FlowBox,
) -> gtk::Box {
    // Os dois cartões quebram a linha em vez de esticar a janela, como o `flex-wrap` do
    // React: numa janela estreita eles empilham, e a janela nunca manda na largura. O
    // terceiro, "Últimas salas", saiu como no Mac: as salas em que se está já moram na
    // trilha da esquerda, e repeti-las ao lado só empurrava o resto.
    let cards = gtk::FlowBox::new();

    cards.set_selection_mode(gtk::SelectionMode::None);
    cards.set_row_spacing(10);
    cards.set_column_spacing(10);
    cards.set_max_children_per_line(2);
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

    cards.insert(&create, -1);
    cards.insert(&code, -1);

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

/// Um canal de voz. Clicar entra na voz **sem trocar de tela**, e quem está dentro aparece
/// logo abaixo do nome — é o `VoiceChannelItem` do React, que é o jeito do Discord.
fn voice_row(bridge: &Rc<Bridge>, channel: &Channel, here: bool, people: &[Peer]) -> gtk::Box {
    let card = column(8);
    let head = row(8);
    let name = body(&channel.name);

    card.add_css_class("voice-channel");
    card.add_css_class("rise");

    if here {
        card.add_css_class("on");
    }

    head.append(&icons::icon("speaker", 14, if here { icons::LILAC } else { icons::DIM }));
    head.append(&name);
    head.append(&spacer());
    card.append(&head);

    if !people.is_empty() {
        let inside = column(6);

        inside.set_margin_start(20);

        for person in people {
            let line = row(8);

            line.append(&avatar(&person.name, 22, person.self_peer));
            line.append(&body(&person.name));

            if person.sharing() {
                line.append(&spacer());
                line.append(&crate::components::mono("transmitindo"));
            }

            inside.append(&line);
        }

        card.append(&inside);
    }

    let click = gtk::GestureClick::new();

    click.connect_released({
        let (bridge, channel) = (bridge.clone(), channel.clone());

        move |_, _, _, _| bridge.join_voice(&channel.id, &channel.name)
    });

    card.add_controller(click);
    crate::components::clickable(&card);

    card
}

/// Abre o campo de nome do canal na seção clicada. O React abre um modal; aqui o campo
/// nasce embaixo das duas listas, e o "+" diz de qual delas ele é.
fn open_naming(field: &gtk::Entry, naming: &Rc<RefCell<Option<ChannelKind>>>, kind: ChannelKind) {
    let same = *naming.borrow() == Some(kind);

    field.set_text("");
    field.set_visible(!same);
    naming.replace(if same { None } else { Some(kind) });

    if !same {
        field.grab_focus();
    }
}

fn member_row(name: &str) -> gtk::Box {
    let line = row(10);

    line.add_css_class("person");
    line.append(&avatar(name, 24, false));
    line.append(&body(name));

    line
}

/// Uma mensagem. O "⋯" tem lugar próprio no fim da linha — o texto quebra antes dele — e só
/// aparece na sua, que é a única que dá para editar e apagar.
fn message_row(bridge: &Rc<Bridge>, channel: &str, message: &Message, mine: bool) -> gtk::Box {
    let line = said_row(
        &message.user.name,
        &message.body,
        message.created_at.get(11..16).unwrap_or_default(),
    );

    if mine {
        line.append(&message_menu(bridge, channel, message));
    }

    line
}

/// A linha de uma fala: o avatar, o nome com a hora ao lado, e o texto embaixo.
fn said_row(author: &str, written: &str, when: &str) -> gtk::Box {
    let line = row(10);
    let texts = column(2);
    let head = row(6);
    let said = body(written);

    said.set_wrap(true);
    said.set_xalign(0.0);
    head.append(&strong(author));
    head.append(&dim(when));
    texts.append(&head);
    texts.append(&said);
    texts.set_hexpand(true);
    line.add_css_class("fade-in");
    line.add_css_class("person");
    line.append(&avatar(author, 30, false));
    line.append(&texts);

    line
}

/// O menu da própria mensagem: editar e apagar. Responder ainda não existe — o `send_message`
/// do núcleo não leva `reply_to`, e inventar um caminho por fora duplicaria a regra.
fn message_menu(bridge: &Rc<Bridge>, channel: &str, message: &Message) -> gtk::MenuButton {
    let menu = gtk::MenuButton::new();
    let sheet = column(2);
    let edit = crate::components::button("Editar", "ghost");
    let erase = crate::components::button("Apagar", "ghost");

    menu.set_child(Some(&icons::icon("dots", 13, icons::DIM)));
    menu.add_css_class("arrow");
    menu.add_css_class("more");
    crate::components::clickable(&menu);
    erase.add_css_class("danger");
    sheet.append(&edit);
    sheet.append(&erase);
    sheet.set_size_request(168, -1);
    menu.set_popover(Some(&crate::components::popover(&sheet)));

    edit.connect_clicked({
        let (bridge, message, channel) = (bridge.clone(), message.clone(), channel.to_owned());

        move |edit| {
            let Some(popover) = edit.ancestor(gtk::Popover::static_type()).and_downcast::<gtk::Popover>() else {
                return;
            };

            popover.popdown();
            editing(&bridge, &channel, &message);
        }
    });

    erase.connect_clicked({
        let (bridge, id, channel) = (bridge.clone(), message.id, channel.to_owned());

        move |erase| {
            if let Some(popover) = erase.ancestor(gtk::Popover::static_type()).and_downcast::<gtk::Popover>() {
                popover.popdown();
            }

            bridge.delete_message(&channel, id);
        }
    });

    menu
}

/// Editar abre a mensagem num campo. Enter grava.
///
/// ponytail: no React o campo nasce no lugar da mensagem; aqui ele é um diálogo, porque
/// trocar a linha por um campo pede reconstruir a lista inteira a cada tecla. Teto: o
/// caminho é o mesmo, a diferença é onde o campo aparece.
fn editing(bridge: &Rc<Bridge>, channel: &str, message: &Message) {
    let dialog = gtk::Window::new();
    let sheet = column(10);
    let written = field("Escreva a mensagem");

    written.set_text(&message.body);
    dialog.set_title(Some("Editar a mensagem"));
    dialog.set_modal(true);
    dialog.set_default_size(420, -1);
    dialog.add_css_class("settings");
    crate::components::pad(&sheet, 16);
    sheet.append(&written);
    dialog.set_child(Some(&sheet));

    written.connect_activate({
        let (bridge, id, dialog) = (bridge.clone(), message.id, dialog.clone());
        let channel = channel.to_owned();

        move |written| {
            let typed = written.text();

            if !typed.trim().is_empty() {
                bridge.edit_message(&channel, id, typed.as_str());
            }

            dialog.close();
        }
    });

    dialog.present();
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
