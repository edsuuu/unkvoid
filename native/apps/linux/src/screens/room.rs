//! A sala: o código para mandar a alguém, quem está dentro, e o que está sendo transmitido.
//!
//! O vídeo chega pronto do `watching`: quadros de pixels, já no tamanho do cartão. Aqui eles
//! só viram textura, no relógio da janela — trinta vezes por segundo, o que chegou por
//! último e nada mais.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use core_app::models::Peer;
use gtk::prelude::*;
use gtk::{gdk, glib};

use crate::bridge::Bridge;
use crate::components::{button, clear_box, clear_list, column, item, list, muted, row, scroll, strong, title};
use crate::streaming::{Mine, Tile};
use crate::user_bar::UserBar;
use crate::watching::TILE;

/// De quanto em quanto tempo a janela pega o quadro mais novo. Trinta por segundo é o que o
/// cartão desenha; pedir mais só acharia o mesmo quadro duas vezes.
const REDRAW: Duration = Duration::from_millis(33);

type Pictures = Rc<RefCell<HashMap<String, gtk::Picture>>>;

pub struct RoomScreen {
    root: gtk::Box,
    code: gtk::Button,
    hint: Rc<RefCell<String>>,
    people: gtk::ListBox,
    count: gtk::Label,
    status: gtk::Label,
    share: gtk::Button,
    camera: gtk::Button,
    screens: gtk::Box,
    cameras: gtk::Box,
    pictures: Pictures,
    bar: UserBar,
    room: Rc<RefCell<String>>,
    sharing: Rc<RefCell<bool>>,
}

impl RoomScreen {
    pub fn new(bridge: &Rc<Bridge>) -> Self {
        let room = Rc::new(RefCell::new(String::new()));
        let sharing = Rc::new(RefCell::new(false));
        let code = button("", "chip");
        let count = muted("");
        let hint: Rc<RefCell<String>> = Rc::default();
        let status = crate::components::danger();
        let people = list(false);
        let share = button("Compartilhar tela", "primary");
        let camera = button("Câmera", "ghost");
        let leave = button("Sair da sala", "danger");
        let pictures: Pictures = Rc::default();

        code.set_tooltip_text(Some("Copiar o código para mandar a alguém"));

        let toolbar = row(10);

        toolbar.add_css_class("toolbar");
        toolbar.append(&strong("Sala"));
        toolbar.append(&code);
        toolbar.append(&count);

        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);

        spacer.set_hexpand(true);
        toolbar.append(&spacer);
        toolbar.append(&share);
        toolbar.append(&camera);
        toolbar.append(&leave);

        let screens = column(8);

        screens.set_vexpand(true);

        let cameras = row(8);

        let stage = column(8);

        stage.add_css_class("stage");
        stage.set_vexpand(true);
        stage.append(&screens);
        stage.append(&cameras);

        let inside = column(8);

        inside.append(&strong("Na sala"));
        inside.append(&scroll(&people));
        inside.set_size_request(260, -1);

        let body = row(12);

        body.set_vexpand(true);
        body.append(&stage);
        body.append(&inside);

        let bar = UserBar::new(bridge);
        let root = column(12);

        crate::components::pad(&root, 12);
        root.append(&toolbar);
        root.append(&body);
        root.append(&status);
        root.append(bar.root());

        code.connect_clicked({
            let room = room.clone();

            move |code| code.clipboard().set_text(&room.borrow())
        });

        share.connect_clicked({
            let (bridge, sharing) = (bridge.clone(), sharing.clone());

            move |_| {
                if *sharing.borrow() {
                    bridge.stop_sharing();
                } else {
                    bridge.share_screen();
                }
            }
        });

        camera.connect_clicked({
            let bridge = bridge.clone();

            move |_| bridge.toggle_camera()
        });

        leave.connect_clicked({
            let bridge = bridge.clone();

            move |_| bridge.leave_room()
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
            hint,
            people,
            count,
            status,
            share,
            camera,
            screens,
            cameras,
            pictures,
            bar,
            room,
            sharing,
        };

        screen.set_tiles(&[]);

        screen
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    pub fn set_user(&self, name: &str) {
        self.bar.set_user(name);
    }

    pub fn set_status(&self, message: &str) {
        self.status.set_text(message);
    }

    pub fn set_room(&self, room: &str) {
        self.room.replace(room.to_owned());
        self.code.set_label(room);
        self.hint.replace(format!("Mande o código {room} para quem você quer aqui."));
        self.set_tiles(&[]);
    }

    /// Um cartão por transmissão de vídeo. A tela ocupa o palco; a câmera é cartão pequeno,
    /// sem painel de imagem.
    pub fn set_tiles(&self, tiles: &[Tile]) {
        clear_box(&self.screens);
        clear_box(&self.cameras);
        self.pictures.borrow_mut().clear();

        if tiles.iter().all(|tile| tile.camera) {
            let empty = column(8);

            empty.set_vexpand(true);
            empty.set_valign(gtk::Align::Center);
            empty.append(&title("Ninguém está transmitindo"));
            empty.append(&muted(&self.hint.borrow()));
            self.screens.append(&empty);
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

    pub fn set_mine(&self, mine: Mine) {
        self.sharing.replace(mine.sharing);
        self.share.set_visible(mine.can_share);
        self.share.set_label(if mine.sharing { "Parar de compartilhar" } else { "Compartilhar tela" });
        self.camera.set_visible(mine.can_video);
        self.camera.set_label(if mine.camera { "Fechar câmera" } else { "Câmera" });
        self.bar.set_mine(mine);
    }

    pub fn set_deafened(&self, deafened: bool) {
        self.bar.set_deafened(deafened);
    }

    pub fn set_peers(&self, peers: &[Peer]) {
        clear_list(&self.people);

        let present = peers.iter().filter(|peer| !peer.reconnecting).count();

        self.count.set_text(&format!("{present} na sala"));

        for peer in peers {
            self.people.append(&item(&person(peer)));
        }
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
    let line = row(8);

    crate::components::pad(&line, 6);
    line.append(&strong(&peer.name));

    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);

    spacer.set_hexpand(true);
    line.append(&spacer);

    if peer.producers.iter().any(|producer| producer.source == "mic" && producer.paused) {
        line.append(&badge("mic mudo", "warn"));
    }

    if peer.sharing() {
        line.append(&badge("AO VIVO", "live"));
    }

    if peer.reconnecting {
        line.append(&badge("parado", "warn"));
    }

    if peer.self_peer {
        line.append(&badge("você", "mine"));
    }

    line
}

fn badge(text: &str, style: &str) -> gtk::Label {
    let badge = gtk::Label::new(Some(text));

    badge.add_css_class("badge");
    badge.add_css_class(style);

    badge
}
