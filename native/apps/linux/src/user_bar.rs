//! A barra de baixo: quem é você, o microfone, o som e a engrenagem.
//!
//! É o `VoicePanel` do React: os três botões são chapados, e a escolha de aparelho mora
//! dentro de "Configurações da conta" — não há setinha nenhuma ao lado do mudo.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use crate::bridge::Bridge;
use crate::components::{
    avatar, body, clear_list, clickable, column, dim, icon_button, item, label_mono, list, muted, row,
    rule, scroll, set_icon, spacer, strong,
};
use crate::devices::{self, Device};
use crate::icons;
use crate::streaming::Mine;

/// O tamanho do desenho dentro dos botões da barra, como no React.
const BAR_ICON: i32 = 15;

pub struct UserBar {
    root: gtk::Box,
    /// O bloco que só existe dentro de um canal de voz, e o nome do canal nele.
    voice: gtk::Box,
    voice_name: gtk::Label,
    /// O sinal, que muda de cor com a latência, e o que o balão dele diz.
    signal: gtk::Box,
    in_voice: std::cell::Cell<bool>,
    face: gtk::Box,
    name: gtk::Label,
    status: gtk::Label,
    menu_name: gtk::Label,
    microphone: gtk::Button,
    camera: gtk::Button,
    sound: gtk::Button,
    sign_out: gtk::Button,
}

impl UserBar {
    pub fn new(bridge: &Rc<Bridge>) -> Self {
        let name = strong("");
        let status = dim("Online");
        let microphone = flat(icon_button("mic", BAR_ICON, icons::RESTING, "Ligar ou calar o microfone"));
        let camera = flat(icon_button("cameraOff", BAR_ICON, icons::RESTING, "Ligar a câmera"));
        let sound = flat(icon_button("headphones", BAR_ICON, icons::RESTING, "Ensurdecer: cala tudo o que chega"));
        let sign_out = gtk::Button::new();
        let menu_name = strong("");

        let root = column(10);
        let line = row(8);
        let face = row(0);
        let who = column(1);
        let voice = column(10);
        let voice_name = crate::components::mono("");

        // O nome encolhe com reticências: ele é o único pedaço elástico da barra, e sem isto
        // um apelido comprido decide a largura da coluna inteira.
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.set_width_chars(4);
        name.set_xalign(0.0);
        status.set_xalign(0.0);
        who.append(&name);
        who.append(&status);

        root.add_css_class("userbar");
        line.append(&face);
        line.append(&who);
        line.append(&spacer());
        line.append(&microphone);
        line.append(&sound);
        line.append(&gear(bridge, &menu_name, &sign_out));

        // O bloco da voz, em cima da linha de quem você é — como no `VoicePanel` do React.
        let connected = row(10);
        let titles = column(1);
        let hang_up = flat(icon_button("phoneOff", BAR_ICON, icons::DANGER, "Desconectar da voz"));
        let doing = row(6);
        let share = wide(icon_button("screen", 16, icons::RESTING, "Compartilhar a tela"));
        let voice_face = strong("Voz conectada");

        voice_face.add_css_class("online");
        voice_face.set_xalign(0.0);
        voice_name.set_xalign(0.0);
        titles.append(&voice_face);
        titles.append(&voice_name);
        let signal = row(0);

        signal.append(&icons::icon("signal", BAR_ICON, icons::DIM));
        signal.set_has_tooltip(true);
        signal.set_tooltip_text(Some("Medindo a ida e volta até o servidor de mídia"));
        connected.append(&signal);
        connected.append(&titles);
        connected.append(&spacer());
        connected.append(&hang_up);

        camera.add_css_class("wide");
        doing.append(&camera);
        doing.append(&share);
        camera.set_hexpand(true);
        share.set_hexpand(true);

        voice.append(&connected);
        voice.append(&doing);
        voice.append(&rule());
        voice.add_css_class("rise");
        voice.set_visible(false);

        root.append(&voice);
        root.append(&line);

        hang_up.connect_clicked({
            let bridge = bridge.clone();

            move |_| bridge.leave_voice()
        });

        share.connect_clicked({
            let (bridge, sharing) = (bridge.clone(), Rc::new(std::cell::Cell::new(false)));

            move |button| {
                // O botão só reflete o que de fato subiu depois que o núcleo responde; aqui
                // ele guarda a intenção, que é o que decide entre ligar e desligar.
                if sharing.get() {
                    bridge.stop_sharing();
                } else {
                    bridge.share_screen();
                }

                sharing.set(!sharing.get());
                highlight(button, "screen", "screen", sharing.get());
            }
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

        sign_out.connect_clicked({
            let bridge = bridge.clone();

            move |_| bridge.sign_out()
        });

        Self {
            root,
            voice,
            voice_name,
            signal,
            in_voice: std::cell::Cell::new(false),
            face,
            name,
            status,
            menu_name,
            microphone,
            camera,
            sound,
            sign_out,
        }
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    pub fn set_user(&self, name: &str) {
        let shown = if name.is_empty() { "Sem conta" } else { name };

        self.name.set_text(shown);
        self.menu_name.set_text(shown);
        self.sign_out.set_visible(!name.is_empty());

        while let Some(child) = self.face.first_child() {
            self.face.remove(&child);
        }

        self.face.append(&avatar(name, 30, true));
    }

    /// A ida e volta até o SFU: verde até 150 ms, lilás acima disso — a mesma régua do
    /// React. O número inteiro fica no balão do mouse, que é onde ele cabe.
    pub fn set_ping(&self, milliseconds: u64) {
        let color = if milliseconds > 150 { icons::LILAC } else { "#34d399" };

        while let Some(child) = self.signal.first_child() {
            self.signal.remove(&child);
        }

        self.signal.append(&icons::icon("signal", BAR_ICON, color));
        self.signal.set_tooltip_text(Some(&format!("{milliseconds} ms até o servidor de mídia")));
    }

    /// Entrou ou saiu da voz. Fora dela o microfone não está mudo, está fora: o React
    /// desenha o microfone inteiro e escreve "Online".
    pub fn set_voice(&self, channel: Option<&str>) {
        self.in_voice.set(channel.is_some());
        self.voice.set_visible(channel.is_some());
        self.voice_name.set_text(channel.unwrap_or(""));

        if channel.is_none() {
            self.status.set_text("Online");
            self.microphone.set_sensitive(false);
            self.sound.set_sensitive(false);
            mark(&self.microphone, "mic", "micOff", true);
        }
    }

    /// O botão só mostra o que o servidor já decidiu: mudo pelo servidor chega como
    /// microfone fechado, e sem `speak` o botão nem fica clicável.
    pub fn set_mine(&self, mine: Mine) {
        let here = self.in_voice.get();

        self.microphone.set_sensitive(here && mine.can_speak);
        self.sound.set_sensitive(here);
        mark(&self.microphone, "mic", "micOff", !here || (mine.mic && !mine.mic_muted));

        self.camera.set_visible(mine.can_video);
        self.camera.set_tooltip_text(Some(if mine.camera { "Desligar a câmera" } else { "Ligar a câmera" }));
        highlight(&self.camera, "camera", "cameraOff", mine.camera);

        self.status.set_text(if !here {
            "Online"
        } else if !mine.can_speak {
            "Mutado pelo servidor"
        } else if mine.mic && !mine.mic_muted {
            "Microfone aberto"
        } else {
            "Mudo"
        });
    }

    pub fn set_deafened(&self, deafened: bool) {
        mark(&self.sound, "headphones", "headphonesOff", !deafened);

        if deafened && self.in_voice.get() {
            self.status.set_text("Surdo");
        }
    }
}

/// Os três controles da barra são chapados: sem contorno, o fundo só aparece ao passar o
/// mouse. É o `btn-icon` dessa barra no React, que é outro botão que o da sala.
fn flat(button: gtk::Button) -> gtk::Button {
    button.add_css_class("bar");

    button
}

/// O botão largo da voz: metade da linha, como o `wide` do React.
fn wide(button: gtk::Button) -> gtk::Button {
    button.add_css_class("wide-icon");

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

/// A engrenagem: abre as configurações da conta numa janela modal, como o Discord — e como
/// o `UserSettingsModal` do React, que é um modal e não um menu.
fn gear(bridge: &Rc<Bridge>, menu_name: &gtk::Label, sign_out: &gtk::Button) -> gtk::Button {
    let button = flat(icon_button("gear", BAR_ICON, icons::RESTING, "Configurações da conta"));

    dress(sign_out, "logout", "Sair da conta", icons::LILAC);

    button.connect_clicked({
        let (bridge, menu_name, sign_out) = (bridge.clone(), menu_name.clone(), sign_out.clone());

        move |button| {
            let parent = button.root().and_downcast::<gtk::Window>();

            settings(&bridge, &menu_name, &sign_out, parent.as_ref()).present();
        }
    });

    button
}

/// A janela das configurações: cabeçalho, os aparelhos que o sistema lista e o rodapé. O que
/// está aqui é o que existe — foto de perfil, teclas e modo do microfone continuam só no
/// React, e botão que não faz nada não entra.
fn settings(
    bridge: &Rc<Bridge>,
    menu_name: &gtk::Label,
    sign_out: &gtk::Button,
    parent: Option<&gtk::Window>,
) -> gtk::Window {
    let window = gtk::Window::new();
    let sheet = column(0);
    let head = row(12);
    let who = column(4);
    let inside = column(8);

    window.set_title(Some("Configurações da conta"));
    window.set_modal(true);
    window.set_default_size(460, 520);
    window.set_transient_for(parent);
    window.add_css_class("settings");

    let name = strong(&menu_name.text());

    name.set_xalign(0.0);
    name.add_css_class("headline");
    who.append(&name);
    who.append(&muted(if bridge.name().is_empty() { "Usando sem login" } else { "Online" }));
    who.set_hexpand(true);
    head.append(&avatar(&menu_name.text(), 40, true));
    head.append(&who);
    head.set_margin_start(24);
    head.set_margin_end(24);
    head.set_margin_top(24);

    let microphones = chooser("Microfone", devices::microphones, devices::current_microphone, {
        let bridge = bridge.clone();

        move |name| bridge.use_microphone(name)
    });

    let speakers = chooser("Saída de áudio", devices::speakers, devices::current_speaker, {
        let bridge = bridge.clone();

        move |name| bridge.use_speaker(name)
    });

    microphones.refresh();
    speakers.refresh();

    inside.append(&microphones.root);
    inside.append(&speakers.root);
    inside.append(&muted("Vale para a voz das pessoas, o áudio das telas e os sons do app."));
    inside.set_margin_start(24);
    inside.set_margin_end(24);
    inside.set_margin_top(20);
    inside.set_margin_bottom(20);
    inside.set_vexpand(true);

    let footer = row(8);
    let done = crate::components::button("Pronto", "primary");

    // O botão de sair é o mesmo da barra: um widget só não cabe em dois pais, então aqui ele
    // vira um irmão que faz a mesma coisa.
    let leave = crate::components::button("Sair da conta", "ghost");

    leave.set_visible(sign_out.is_visible());
    leave.connect_clicked({
        let (bridge, window) = (bridge.clone(), window.clone());

        move |_| {
            window.close();
            bridge.sign_out();
        }
    });

    done.connect_clicked({
        let window = window.clone();

        move |_| window.close()
    });

    footer.append(&leave);
    footer.append(&spacer());
    footer.append(&done);
    footer.set_margin_start(16);
    footer.set_margin_end(16);
    footer.set_margin_top(16);
    footer.set_margin_bottom(16);

    sheet.append(&head);
    sheet.append(&inside);
    sheet.append(&rule());
    sheet.append(&footer);
    window.set_child(Some(&sheet));

    window
}

fn dress(button: &gtk::Button, icon: &str, text: &str, color: &str) {
    let inside = row(10);
    let label = body(text);

    label.set_xalign(0.0);

    if color == icons::LILAC {
        label.add_css_class("lilac");
    }

    inside.append(&icons::icon(icon, BAR_ICON, color));
    inside.append(&label);

    button.set_child(Some(&inside));
    button.add_css_class("menu-item");
    clickable(button);
}

/// Uma lista de aparelhos dentro do menu, com o título em cima.
struct Chooser {
    root: gtk::Box,
    refresh: Rc<dyn Fn()>,
}

impl Chooser {
    fn refresh(&self) {
        (self.refresh)();
    }
}

fn chooser(
    heading: &str,
    available: fn() -> Vec<Device>,
    current: fn() -> Option<String>,
    choose: impl Fn(&str) + 'static,
) -> Chooser {
    let root = column(4);
    let options = list(true);
    let names: Rc<RefCell<Vec<String>>> = Rc::default();
    let holder = scroll(&options);

    holder.set_size_request(-1, 120);
    root.append(&label_mono(heading));
    root.append(&holder);

    options.connect_row_activated({
        let names = names.clone();

        move |_, activated| {
            if let Some(name) = names.borrow().get(activated.index() as usize) {
                choose(name);
            }
        }
    });

    let refresh = {
        let (options, names) = (options.clone(), names.clone());

        move || {
            let found = available();

            clear_list(&options);
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
    };

    Chooser { root, refresh: Rc::new(refresh) }
}
