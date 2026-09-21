//! A barra de baixo: quem é você, o microfone e o som — cada um com a setinha do lado.
//!
//! A setinha abre a lista de aparelhos do sistema, no molde dos apps de chamada: escolher ali troca
//! o microfone que sobe e a saída por onde o som da sala toca, sem sair da tela.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use crate::bridge::Bridge;
use crate::components::{item, list, muted, row, scroll, strong};
use crate::devices::{self, Device};
use crate::streaming::Mine;

pub struct UserBar {
    root: gtk::Box,
    name: gtk::Label,
    microphone: gtk::Button,
    sound: gtk::Button,
}

impl UserBar {
    pub fn new(bridge: &Rc<Bridge>) -> Self {
        let name = strong("");
        let microphone = icon("🎙", "Ligar ou calar o microfone");
        let sound = icon("🎧", "Ensurdecer: cala tudo o que chega");

        let root = row(6);

        root.add_css_class("userbar");
        root.append(&name);

        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);

        spacer.set_hexpand(true);
        root.append(&spacer);
        root.append(&microphone);
        root.append(&chooser(devices::microphones, devices::current_microphone, {
            let bridge = bridge.clone();

            move |name| bridge.use_microphone(name)
        }));
        root.append(&sound);
        root.append(&chooser(devices::speakers, devices::current_speaker, {
            let bridge = bridge.clone();

            move |name| bridge.use_speaker(name)
        }));

        microphone.connect_clicked({
            let bridge = bridge.clone();

            move |_| bridge.toggle_mic()
        });

        sound.connect_clicked({
            let bridge = bridge.clone();

            move |sound| mark(sound, !bridge.toggle_deafen())
        });

        Self { root, name, microphone, sound }
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    pub fn set_user(&self, name: &str) {
        self.name.set_text(if name.is_empty() { "Sem conta" } else { name });
    }

    /// O botão só mostra o que o servidor já decidiu: mudo pelo servidor chega como
    /// microfone fechado, e sem `speak` o botão nem fica clicável.
    pub fn set_mine(&self, mine: Mine) {
        self.microphone.set_sensitive(mine.can_speak);
        mark(&self.microphone, mine.mic && !mine.mic_muted);
    }

    pub fn set_deafened(&self, deafened: bool) {
        mark(&self.sound, !deafened);
    }
}

fn icon(glyph: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::with_label(glyph);

    button.add_css_class("icon");
    button.set_tooltip_text(Some(tooltip));

    button
}

/// Ligado ou desligado, na cor. Duas classes de cor na mesma string não se resolvem pela
/// ordem escrita, e sim pela do CSS: por isso é uma classe só, posta e tirada.
fn mark(button: &gtk::Button, on: bool) {
    if on {
        button.remove_css_class("off");
    } else {
        button.add_css_class("off");
    }
}

/// A setinha ao lado do botão. A lista é lida na hora de abrir: aparelho ligado depois que
/// o app abriu tem de aparecer sem reiniciar nada.
fn chooser(
    available: fn() -> Vec<Device>,
    current: fn() -> Option<String>,
    choose: impl Fn(&str) + 'static,
) -> gtk::MenuButton {
    let menu = gtk::MenuButton::builder().label("▾").build();
    let popover = gtk::Popover::new();
    let options = list(true);
    let names: Rc<RefCell<Vec<String>>> = Rc::default();
    let holder = scroll(&options);

    menu.add_css_class("arrow");
    holder.set_size_request(280, 180);
    popover.set_child(Some(&holder));
    menu.set_popover(Some(&popover));

    popover.connect_show({
        let (options, names) = (options.clone(), names.clone());

        move |_| {
            let found = available();

            crate::components::clear_list(&options);
            names.replace(found.iter().map(|device| device.name.clone()).collect());

            if found.is_empty() {
                options.append(&item(&muted("Nenhum aparelho encontrado.")));

                return;
            }

            let chosen = current();

            for device in &found {
                let marked = if Some(&device.name) == chosen.as_ref() {
                    format!("• {}", device.label)
                } else {
                    device.label.clone()
                };

                options.append(&item(&strong(&marked)));
            }
        }
    });

    options.connect_row_activated({
        let (names, popover) = (names.clone(), popover.clone());

        move |_, activated| {
            if let Some(name) = names.borrow().get(activated.index() as usize) {
                choose(name);
            }

            popover.popdown();
        }
    });

    menu
}
