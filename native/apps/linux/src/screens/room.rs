//! A sala: o código para mandar a alguém, quem está dentro, e o que está sendo transmitido.
//!
//! O vídeo chega pronto do `watching`: quadros de pixels, já no tamanho do cartão. Aqui eles
//! só viram textura, no relógio da janela — trinta vezes por segundo, o que chegou por
//! último e nada mais.
//!
//! O desenho é o do `apps/desktop/ui/components/room`: uma barra só, em pílula, com o código
//! à esquerda e os botões redondos à direita; quem está na sala mora na setinha, não numa
//! coluna ao lado.
//!
//! **A sala por código não tem voz.** Nada de microfone, som, câmera ou barra de baixo: ela
//! é só a tela compartilhada, como no React (`RoomToolbar` só desenha isso no modo `voice`).
//! Botão de mudar mic aqui prometeria um canal de áudio que esta sala não tem.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use core_app::models::Peer;
use gtk::prelude::*;
use gtk::{gdk, glib};

use crate::bridge::Bridge;
use crate::components::{
    avatar, badge, body, clear_box, clear_list, column, dim, headline, icon_button, item, label_mono, list, mono,
    muted, pill, row, scroll, set_icon, spacer,
};
use crate::icons;
use crate::streaming::{Mine, Tile};
use crate::watching::TILE;

/// De quanto em quanto tempo a janela pega o quadro mais novo. Trinta por segundo é o que o
/// cartão desenha; pedir mais só acharia o mesmo quadro duas vezes.
const REDRAW: Duration = Duration::from_millis(33);

/// O relógio do "tempo na sala" anda de segundo em segundo, que é o que ele mostra.
const TICK: Duration = Duration::from_secs(1);

/// O tamanho do desenho dentro dos botões redondos da barra, como no React.
const TOOL_ICON: i32 = 17;

/// A largura da lista de quem está na sala, o `w-80` do React.
const PEOPLE_WIDTH: i32 = 320;

/// Até onde a lista cresce antes de passar a rolar.
const PEOPLE_MAX_HEIGHT: i32 = 320;

type Pictures = Rc<RefCell<HashMap<String, gtk::Picture>>>;

pub struct RoomScreen {
    root: gtk::Box,
    code: gtk::Label,
    people: gtk::ListBox,
    faces: gtk::Box,
    count: gtk::Label,
    status: gtk::Label,
    ping: gtk::Label,
    share: gtk::Button,
    stop: gtk::Button,
    screens: gtk::Box,
    cameras: gtk::Box,
    pictures: Pictures,
    room: Rc<RefCell<String>>,
    sharing: Rc<RefCell<bool>>,
    tiles: Rc<RefCell<Vec<Tile>>>,
    since: Rc<RefCell<Instant>>,
}

impl RoomScreen {
    pub fn new(bridge: &Rc<Bridge>) -> Self {
        let room = Rc::new(RefCell::new(String::new()));
        let sharing = Rc::new(RefCell::new(false));
        let tiles: Rc<RefCell<Vec<Tile>>> = Rc::default();
        let since = Rc::new(RefCell::new(Instant::now()));
        let status = crate::components::danger();
        let people = list(false);
        let faces = row(5);
        let count = mono("1");
        let pictures: Pictures = Rc::default();

        // O código é um botão porque copiar é o que se faz com ele.
        let code = gtk::Label::new(Some(""));
        let copy = gtk::Button::new();
        let code_inside = row(6);

        code_inside.append(&code);
        code_inside.append(&icons::icon("copy", 12, icons::LILAC));
        copy.set_child(Some(&code_inside));
        copy.add_css_class("chip");
        copy.set_valign(gtk::Align::Center);
        copy.set_tooltip_text(Some("Copiar o código para mandar a alguém"));

        let share = icon_button("screen", TOOL_ICON, icons::RESTING, "Compartilhar tela");
        let leave = icon_button("phoneOff", TOOL_ICON, icons::STRONG, "Sair da sala");

        leave.add_css_class("hangup");

        let toolbar = row(8);

        toolbar.add_css_class("toolbar");
        toolbar.append(&label_mono("Sala"));
        toolbar.append(&copy);
        toolbar.append(&who(bridge, &faces, &count, &people));

        let elapsed = mono("0:00:00");
        let clock = pill(8);

        clock.set_tooltip_text(Some("Tempo na sala"));
        clock.append(&crate::components::online_dot());
        clock.append(&elapsed);
        toolbar.append(&clock);

        // O ping é o ida e volta que o núcleo mede no batimento de 5 s. Até o primeiro
        // voltar não há número, e o traço é o que o React mostra nesse intervalo.
        let ping = dim("-- ms");

        ping.add_css_class("mono");
        ping.set_tooltip_text(Some("Ida e volta até o servidor de mídia"));
        toolbar.append(&ping);

        // Transmitindo, o React põe um "Parar" de texto ao lado do ícone: o botão da barra
        // vira o menu da transmissão, e parar precisa de um caminho que não dependa dele.
        let stop = gtk::Button::new();
        let stop_inside = row(8);

        stop_inside.append(&icons::icon("stop", 18, icons::DANGER));
        stop_inside.append(&gtk::Label::new(Some("Parar")));
        stop.set_child(Some(&stop_inside));
        stop.add_css_class("danger");
        stop.add_css_class("stop");
        stop.set_tooltip_text(Some("Parar de transmitir"));
        stop.set_valign(gtk::Align::Center);
        stop.set_visible(false);
        crate::components::clickable(&stop);

        toolbar.append(&spacer());
        toolbar.append(&stop);
        toolbar.append(&share);
        toolbar.append(&leave);

        let screens = column(10);

        screens.set_vexpand(true);

        let cameras = row(10);

        let stage = column(10);

        stage.set_vexpand(true);
        stage.append(&screens);
        stage.append(&cameras);

        let root = column(12);

        crate::components::pad(&root, 12);
        root.append(&toolbar);
        root.append(&stage);
        root.append(&status);

        copy.connect_clicked({
            let room = room.clone();

            move |copy| copy.clipboard().set_text(&room.borrow())
        });

        // Abre o seletor de tela; no ar, o "Mudar a transmissão". Parar é o botão ao lado.
        share.connect_clicked({
            let bridge = bridge.clone();

            move |button| {
                let parent = button.root().and_downcast::<gtk::Window>();

                crate::share_picker::open(&bridge, parent.as_ref());
            }
        });

        stop.connect_clicked({
            let bridge = bridge.clone();

            move |_| bridge.stop_sharing()
        });

        leave.connect_clicked({
            let bridge = bridge.clone();

            move |_| bridge.leave_room()
        });

        let _clock = glib::timeout_add_local(TICK, {
            let (since, elapsed) = (since.clone(), elapsed.clone());

            move || {
                let seconds = since.borrow().elapsed().as_secs();

                elapsed.set_text(&format!("{}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60));

                glib::ControlFlow::Continue
            }
        });

        let _redraw = glib::timeout_add_local(REDRAW, {
            let (bridge, pictures) = (bridge.clone(), pictures.clone());

            move || {
                for (producer_id, pixels) in bridge.fresh_frames() {
                    if let Some(picture) = pictures.borrow().get(&producer_id) {
                        picture.set_paintable(Some(&texture(pixels)));
                    }
                }

                glib::ControlFlow::Continue
            }
        });

        let screen = Self {
            root,
            code,
            people,
            faces,
            count,
            status,
            ping,
            share,
            stop,
            screens,
            cameras,
            pictures,
            room,
            sharing,
            tiles,
            since,
        };

        screen.set_tiles(&[]);

        screen
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    pub fn set_status(&self, message: &str) {
        self.status.set_text(message);
    }

    pub fn set_ping(&self, milliseconds: u64) {
        self.ping.set_text(&format!("{milliseconds} ms"));
    }

    pub fn set_room(&self, room: &str) {
        self.room.replace(room.to_owned());
        self.code.set_text(room);
        self.since.replace(Instant::now());
        // Sala nova, medição nova: o número da sala anterior não vale para esta.
        self.ping.set_text("-- ms");
        self.set_tiles(&[]);
    }

    /// Um cartão por transmissão de vídeo. A tela ocupa o palco; a câmera é cartão pequeno,
    /// sem painel de imagem.
    pub fn set_tiles(&self, tiles: &[Tile]) {
        self.tiles.replace(tiles.to_vec());
        self.draw_tiles(tiles);
    }

    fn draw_tiles(&self, tiles: &[Tile]) {
        clear_box(&self.screens);
        clear_box(&self.cameras);
        self.pictures.borrow_mut().clear();

        if tiles.iter().all(|tile| tile.camera) {
            self.screens.append(&self.nobody());
        }

        for tile in tiles {
            let picture = gtk::Picture::new();
            let card = column(4);

            card.add_css_class("tile");
            card.append(&picture);
            card.append(&muted(&tile.label));

            if tile.camera {
                card.set_size_request(200, -1);
                picture.set_size_request(192, 108);
                self.cameras.append(&card);
            } else {
                picture.set_vexpand(true);
                card.set_vexpand(true);
                self.screens.append(&card);
            }

            self.pictures.borrow_mut().insert(tile.producer_id.clone(), picture);
        }
    }

    /// O cartão do meio quando não há nada para assistir: o desenho, a frase, e o botão que
    /// começa a transmissão — nessa ordem, como no React.
    fn nobody(&self) -> gtk::Box {
        let empty = column(0);

        empty.set_vexpand(true);
        empty.set_valign(gtk::Align::Center);
        empty.set_halign(gtk::Align::Center);
        empty.add_css_class("stage-card");
        empty.set_size_request(520, -1);

        let tile = gtk::Box::new(gtk::Orientation::Horizontal, 0);

        tile.add_css_class("icon-tile");
        tile.set_size_request(64, 64);
        tile.set_halign(gtk::Align::Center);
        tile.set_margin_bottom(20);
        tile.append(&{
            let drawing = icons::icon("screen", 26, icons::LILAC);

            drawing.set_hexpand(true);
            drawing
        });
        empty.append(&tile);

        let mine = *self.sharing.borrow();
        let heading = crate::components::centered(&headline(if mine {
            "Você está transmitindo."
        } else {
            "Ninguém está compartilhando ainda."
        }));

        empty.append(&heading);

        if mine {
            let line =
                crate::components::centered(&muted("A sua tela não aparece aqui para não gastar um decoder à toa."));

            line.set_margin_top(10);
            empty.append(&line);
        } else {
            // O código vem num chip no meio da frase, como no React: é o que a pessoa vai
            // copiar, e ele precisa saltar do texto em volta.
            let line = row(6);

            line.set_halign(gtk::Align::Center);
            line.set_margin_top(10);
            line.append(&muted("Mande o código"));
            line.append(&crate::components::code_chip(&self.room.borrow()));
            line.append(&muted("para quem você quer aqui."));
            empty.append(&line);
        }

        if !mine {
            let start = crate::components::button("Iniciar compartilhamento", "primary");

            start.set_halign(gtk::Align::Center);
            start.set_margin_top(20);
            start.connect_clicked({
                let share = self.share.clone();

                move |_| share.emit_clicked()
            });
            empty.append(&start);
        }

        empty
    }

    pub fn set_mine(&self, mine: Mine) {
        // O cartão do meio muda de frase quando a transmissão começa ("Você está
        // transmitindo"), e quem sabe disso é o `Mine` — não a lista de cartões.
        let changed = self.sharing.replace(mine.sharing) != mine.sharing;

        if changed {
            let tiles = self.tiles.borrow().clone();

            self.draw_tiles(&tiles);
        }

        self.share.set_visible(mine.can_share);
        self.share.set_tooltip_text(Some(if mine.sharing {
            "Parar de compartilhar"
        } else {
            "Compartilhar tela"
        }));
        mark(&self.share, mine.sharing);
        set_icon(&self.share, "screen", TOOL_ICON, if mine.sharing { icons::STRONG } else { icons::RESTING });

        // O "Parar" só existe enquanto há o que parar, como no React.
        self.stop.set_visible(mine.sharing);
    }

    pub fn set_peers(&self, peers: &[Peer]) {
        clear_list(&self.people);
        clear_box(&self.faces);

        let present = peers.iter().filter(|peer| !peer.reconnecting).count();

        self.count.set_text(&present.max(1).to_string());

        for peer in peers.iter().take(3) {
            self.faces.append(&avatar(&peer.name, 26, peer.self_peer));
        }

        if peers.is_empty() {
            self.people.append(&item(&muted("Nenhuma pessoa conectada.")));
        }

        for peer in peers {
            self.people.append(&item(&person(peer)));
        }
    }

}

/// A pílula de quem está na sala: os rostos, a conta, e a setinha que abre a lista.
fn who(bridge: &Rc<Bridge>, faces: &gtk::Box, count: &gtk::Label, people: &gtk::ListBox) -> gtk::MenuButton {
    let menu = gtk::MenuButton::new();
    let inside = row(10);

    inside.append(faces);
    inside.append(count);
    inside.append(&icons::icon("chevronDown", 12, icons::RESTING));
    menu.set_child(Some(&inside));
    menu.add_css_class("pill");
    menu.set_valign(gtk::Align::Center);
    menu.set_tooltip_text(Some("Quem está na sala"));
    crate::components::clickable(&menu);

    let panel = column(6);
    let holder = scroll(people);
    let head = row(0);
    let heading = label_mono("Na sala");

    // A mesma largura do `w-80` do React, descontada a folga de 8 do popover. A altura segue
    // o conteúdo e só rola depois do teto: o React não deixa um vazio preto embaixo da lista.
    panel.set_size_request(PEOPLE_WIDTH - 16, -1);
    holder.set_propagate_natural_height(true);
    holder.set_max_content_height(PEOPLE_MAX_HEIGHT);

    heading.set_hexpand(true);
    head.set_margin_start(8);
    head.set_margin_end(8);
    head.set_margin_top(6);
    head.set_margin_bottom(6);
    head.append(&heading);

    // "Atualizar" existe porque o evento de transmissão nova pode se perder: é a saída manual
    // para voltar a ver quem já estava no ar.
    let again = gtk::Button::new();
    let again_inside = row(5);

    again_inside.append(&icons::icon("refresh", 12, icons::DIM));
    again_inside.append(&dim("Atualizar"));
    again.set_child(Some(&again_inside));
    again.add_css_class("link");
    again.set_tooltip_text(Some("Procurar de novo quem está transmitindo"));
    crate::components::clickable(&again);
    again.connect_clicked({
        let bridge = bridge.clone();

        move |_| bridge.refresh_watch()
    });
    head.append(&again);

    panel.append(&head);
    panel.append(&holder);
    menu.set_popover(Some(&crate::components::popover(&panel)));

    menu
}

/// Ligado ou desligado, na cor. Duas classes de cor na mesma string não se resolvem pela
/// ordem escrita, e sim pela do CSS: por isso é uma classe só, posta e tirada.
fn mark(button: &gtk::Button, on: bool) {
    if on {
        button.add_css_class("on");
    } else {
        button.remove_css_class("on");
    }
}

/// Os pixels que chegaram viram textura sem cópia: o GTK fica com o mesmo buffer.
fn texture(pixels: Vec<u8>) -> gdk::MemoryTexture {
    let (width, height) = TILE;

    gdk::MemoryTexture::new(
        width as i32,
        height as i32,
        gdk::MemoryFormat::R8g8b8,
        &glib::Bytes::from_owned(pixels),
        width as usize * 3,
    )
}

fn person(peer: &Peer) -> gtk::Box {
    let line = row(10);

    line.add_css_class("person");

    // Quem é você ganha o fundo verde do React, o mesmo tom da bolinha de "conectado".
    if peer.self_peer {
        line.add_css_class("mine");
    }

    line.append(&avatar(&peer.name, 24, peer.self_peer));
    line.append(&body(&peer.name));
    line.append(&spacer());

    if peer.producers.iter().any(|producer| producer.source == "mic" && producer.paused) {
        let quiet = icons::icon("micOff", 13, icons::DANGER);

        quiet.set_tooltip_text(Some("Microfone mutado"));
        line.append(&quiet);
    }

    if peer.sharing() {
        line.append(&badge("AO VIVO", "live"));
    }

    if peer.reconnecting {
        line.append(&dim("parado"));
    }

    if peer.self_peer {
        line.append(&dim("você"));
    }

    line
}

