//! A barra de baixo: quem é você, o microfone e o som — cada um com a setinha do lado.
//!
//! A setinha abre a lista de aparelhos do sistema, no molde dos apps de chamada: escolher ali troca
//! o microfone que sobe e a saída por onde o som da sala toca, sem sair da tela.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use crate::bridge::Bridge;
use crate::components::{body, icon_button, item, list, muted, row, scroll, set_icon, spacer, strong};
use crate::devices::{self, Device};
use crate::icons;
use crate::streaming::Mine;

/// O tamanho do desenho dentro dos botões da barra, como no React.
const BAR_ICON: i32 = 15;

pub struct UserBar {
    root: gtk::Box,
    name: gtk::Label,
    microphone: gtk::Button,
    camera: gtk::Button,
    sound: gtk::Button,
    sign_out: gtk::Button,
}

impl UserBar {
    pub fn new(bridge: &Rc<Bridge>) -> Self {
        let name = strong("");
        let microphone = small(icon_button("mic", BAR_ICON, icons::RESTING, "Ligar ou calar o microfone"));
        let camera = small(icon_button("cameraOff", BAR_ICON, icons::RESTING, "Ligar a câmera"));
        let sound = small(icon_button("headphones", BAR_ICON, icons::RESTING, "Ensurdecer: cala tudo o que chega"));

        let root = row(6);

        // O nome encolhe com reticências: ele é o único pedaço elástico da barra, e sem isto
        // um apelido comprido decide a largura da coluna inteira.
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.set_width_chars(4);
        root.add_css_class("userbar");
        root.append(&name);
        root.append(&spacer());
        root.append(&camera);
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

        // No React, "Sair da conta" mora na área de quem está logado — aqui é esta barra.
        // Sem conta ele não aparece: não há de onde sair.
        let sign_out = small(icon_button("logout", BAR_ICON, icons::DIM, "Sair da conta"));

        root.append(&sign_out);

        sign_out.connect_clicked({
            let bridge = bridge.clone();

            move |_| bridge.sign_out()
        });

        microphone.connect_clicked({
            let bridge = bridge.clone();

            move |_| bridge.toggle_mic()
        });

        camera.connect_clicked({
            let bridge = bridge.clone();

            move |_| bridge.toggle_camera()
        });

        sound.connect_clicked({
            let bridge = bridge.clone();

            move |sound| mark(sound, "headphones", "headphonesOff", !bridge.toggle_deafen())
        });

        Self { root, name, microphone, camera, sound, sign_out }
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    pub fn set_user(&self, name: &str) {
        self.name.set_text(if name.is_empty() { "Sem conta" } else { name });
        self.sign_out.set_visible(!name.is_empty());
    }

    /// O botão só mostra o que o servidor já decidiu: mudo pelo servidor chega como
    /// microfone fechado, e sem `speak` o botão nem fica clicável.
    pub fn set_mine(&self, mine: Mine) {
        self.microphone.set_sensitive(mine.can_speak);
        mark(&self.microphone, "mic", "micOff", mine.mic && !mine.mic_muted);

        self.camera.set_visible(mine.can_video);
        self.camera.set_tooltip_text(Some(if mine.camera { "Desligar a câmera" } else { "Ligar a câmera" }));
        highlight(&self.camera, "camera", "cameraOff", mine.camera);
    }

    pub fn set_deafened(&self, deafened: bool) {
        mark(&self.sound, "headphones", "headphonesOff", !deafened);
    }
}

/// O quadrado menor, de 30: são seis controles numa coluna de 240, e o de 34 não cabe.
fn small(button: gtk::Button) -> gtk::Button {
    button.add_css_class("small");

    button
}

/// O microfone e o som: fechado é aviso, e fica vermelho — o `btn-icon-off` do React.
/// Duas classes de cor na mesma string não se resolvem pela ordem escrita, e sim pela do
/// CSS: por isso é uma classe só, posta e tirada.
fn mark(button: &gtk::Button, on_name: &str, off_name: &str, on: bool) {
    if on {
        button.remove_css_class("off");
    } else {
        button.add_css_class("off");
    }

    set_icon(
        button,
        if on { on_name } else { off_name },
        BAR_ICON,
        if on { icons::RESTING } else { icons::DANGER },
    );
}

/// A câmera segue a convenção contrária: desligada é o estado comum, e ligada é que se
/// destaca no violeta — o `btn-icon-on` do React. Câmera fechada não é aviso nenhum.
fn highlight(button: &gtk::Button, on_name: &str, off_name: &str, on: bool) {
    if on {
        button.add_css_class("on");
    } else {
        button.remove_css_class("on");
    }

    set_icon(
        button,
        if on { on_name } else { off_name },
        BAR_ICON,
        if on { icons::STRONG } else { icons::RESTING },
    );
}

/// A setinha ao lado do botão. A lista é lida na hora de abrir: aparelho ligado depois que
/// o app abriu tem de aparecer sem reiniciar nada.
fn chooser(
    available: fn() -> Vec<Device>,
    current: fn() -> Option<String>,
    choose: impl Fn(&str) + 'static,
) -> gtk::MenuButton {
    let menu = gtk::MenuButton::new();

    menu.set_child(Some(&icons::icon("chevronDown", 12, icons::DIM)));
    crate::components::clickable(&menu);

    let options = list(true);
    let names: Rc<RefCell<Vec<String>>> = Rc::default();
    let holder = scroll(&options);

    menu.add_css_class("arrow");
    holder.set_size_request(280, 180);

    let popover = crate::components::popover(&holder);

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

                options.append(&item(&body(&marked)));
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
