//! A barra de baixo: quem é você, o microfone, o som e a engrenagem.
//!
//! É o `VoicePanel` do React: os três botões são chapados, e a escolha de aparelho mora
//! dentro de "Configurações da conta" — não há setinha nenhuma ao lado do mudo.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;

use crate::bridge::Bridge;
use crate::components::{
    avatar, body, clear_list, clickable, column, dim, icon_button, item, label_mono, list, muted, popover, row,
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
    menu_name: gtk::Label,
    microphone: gtk::Button,
    camera: gtk::Button,
    sound: gtk::Button,
    sign_out: gtk::Button,
}

impl UserBar {
    pub fn new(bridge: &Rc<Bridge>) -> Self {
        let name = strong("");
        // Sempre "Online": o estado do microfone já está no botão, e escrevê-lo duas vezes
        // só dava chance de as duas linhas discordarem.
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
        line.append(&chooser_arrow("Microfone", devices::microphones, devices::current_microphone, {
            let bridge = bridge.clone();

            move |name| bridge.use_microphone(name)
        }));
        line.append(&sound);
        line.append(&chooser_arrow("Saída de áudio", devices::speakers, devices::current_speaker, {
            let bridge = bridge.clone();

            move |name| bridge.use_speaker(name)
        }));
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

    }

    /// O botão só mostra o que o servidor já decidiu: mudo pelo servidor chega como
    /// microfone fechado, e sem `speak` o botão nem fica clicável.
    pub fn set_mine(&self, mine: Mine) {
        // Mutar e ensurdecer valem fora da voz também: a escolha é de quem usa, e o servidor
        // só entra quando há voz para ele calar.
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

/// A setinha ao lado do botão, como no Mac: ela abre a lista de aparelhos sem depender de
/// estar numa voz para escolher.
fn chooser_arrow(
    heading: &str,
    available: fn() -> Vec<Device>,
    current: fn() -> Option<String>,
    choose: impl Fn(&str) + 'static,
) -> gtk::MenuButton {
    let menu = gtk::MenuButton::new();
    let list = chooser(heading, available, current, choose);

    menu.set_child(Some(&icons::icon("chevronDown", 11, icons::DIM)));
    menu.add_css_class("arrow");
    clickable(&menu);
    list.root.set_size_request(260, -1);

    let popup = popover(&list.root);

    menu.set_popover(Some(&popup));

    // A lista é lida na hora de abrir: aparelho ligado depois que o app abriu tem de
    // aparecer sem reiniciar nada.
    popup.connect_show(move |_| list.refresh());

    menu
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

/// As configurações da conta, no molde do Discord — e o mesmo desenho do macOS: uma janela
/// que toma quase a tela, as seções agrupadas à esquerda, a aberta à direita com o título
/// grande, e o "X / ESC" no canto.
///
/// Só entram as seções que têm o que mostrar. "Teclas" e "Notificações" existem no Mac
/// porque as preferências de voz moram na ABI dele; aqui elas ainda não subiram para o
/// núcleo, e seção que não faz nada não entra.
fn settings(
    bridge: &Rc<Bridge>,
    menu_name: &gtk::Label,
    sign_out: &gtk::Button,
    parent: Option<&gtk::Window>,
) -> gtk::Window {
    let window = gtk::Window::new();
    let body = row(0);
    let rail = column(4);
    let pages = gtk::Stack::new();

    window.set_title(Some("Configurações"));
    window.set_modal(true);
    window.set_default_size(1040, 720);
    window.set_transient_for(parent);
    window.add_css_class("settings");

    // ---- a coluna das seções ----
    let account = section("users", "Minha conta");
    let voice = section("mic", "Voz e vídeo");
    let group_user = label_mono("Configurações de usuário");
    let group_app = label_mono("Configurações do app");

    group_user.set_margin_start(10);
    group_app.set_margin_start(10);
    group_app.set_margin_top(10);
    rail.append(&group_user);
    rail.append(&account);
    rail.append(&group_app);
    rail.append(&voice);
    rail.append(&rule());

    let leave = section("logout", "Sair da conta");

    leave.add_css_class("danger");
    leave.set_visible(sign_out.is_visible());
    rail.append(&leave);
    rail.add_css_class("settings-rail");
    rail.set_size_request(252, -1);

    // ---- a seção aberta ----
    pages.add_named(&account_page(menu_name, bridge), Some("account"));
    pages.add_named(&voice_page(bridge), Some("voice"));
    pages.set_visible_child_name("account");
    pages.set_hexpand(true);
    pages.set_vexpand(true);

    // ---- o "X / ESC" do canto ----
    let corner = column(6);
    let close = gtk::Button::new();

    close.set_child(Some(&icons::icon("close", 14, icons::RESTING)));
    close.add_css_class("escape");
    close.set_tooltip_text(Some("Fechar as configurações"));
    clickable(&close);
    corner.append(&close);
    corner.append(&label_mono("Esc"));
    corner.set_valign(gtk::Align::Start);
    corner.set_margin_top(36);
    corner.set_margin_end(28);
    corner.set_margin_start(8);

    body.append(&rail);
    body.append(&pages);
    body.append(&corner);
    window.set_child(Some(&body));

    account.connect_clicked({
        let (pages, account, voice) = (pages.clone(), account.clone(), voice.clone());

        move |_| {
            pages.set_visible_child_name("account");
            account.add_css_class("on");
            voice.remove_css_class("on");
        }
    });

    voice.connect_clicked({
        let (pages, account, voice) = (pages.clone(), account.clone(), voice.clone());

        move |_| {
            pages.set_visible_child_name("voice");
            voice.add_css_class("on");
            account.remove_css_class("on");
        }
    });

    account.add_css_class("on");

    leave.connect_clicked({
        let (bridge, window) = (bridge.clone(), window.clone());

        move |_| {
            window.close();
            bridge.sign_out();
        }
    });

    close.connect_clicked({
        let window = window.clone();

        move |_| window.close()
    });

    // Esc fecha, como no Mac.
    let keys = gtk::EventControllerKey::new();

    keys.connect_key_pressed({
        let window = window.clone();

        move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                window.close();

                return gtk::glib::Propagation::Stop;
            }

            gtk::glib::Propagation::Proceed
        }
    });

    window.add_controller(keys);

    window
}

/// O desenho à esquerda e a frase à direita, dentro de um botão.
fn dress(button: &gtk::Button, icon: &str, text: &str, color: &str) {
    let inside = row(10);
    let label = body(text);

    label.set_xalign(0.0);
    inside.append(&icons::icon(icon, 14, color));
    inside.append(&label);
    button.set_child(Some(&inside));
    clickable(button);
}

/// Uma linha da coluna da esquerda.
fn section(icon: &str, text: &str) -> gtk::Button {
    let button = gtk::Button::new();

    dress(&button, icon, text, icons::RESTING);
    button.add_css_class("section");

    button
}

/// O miolo da seção: o título grande e o que ela mostra.
fn page(title: &str) -> gtk::Box {
    let inside = column(20);
    let headline = crate::components::headline(title);

    headline.set_xalign(0.0);
    inside.append(&headline);
    inside.set_margin_start(40);
    inside.set_margin_end(40);
    inside.set_margin_top(36);
    inside.set_margin_bottom(36);

    inside
}

fn account_page(menu_name: &gtk::Label, bridge: &Rc<Bridge>) -> gtk::Widget {
    let inside = page("Minha conta");
    let photo = row(12);
    let face = row(0);

    inside.append(&label_mono("Foto de perfil"));
    face.append(&avatar(&menu_name.text(), 56, true));
    photo.append(&face);
    // ponytail: trocar a foto é `POST /api/me/avatar` com um arquivo, e o seletor do GTK
    // ainda não está ligado aqui. Teto: a foto se troca pelo site ou pelo Mac.
    photo.append(&muted("A foto se troca no site ou no app do Mac."));
    inside.append(&photo);

    inside.append(&label_mono("Conta"));
    inside.append(&line("Apelido", &menu_name.text()));
    inside.append(&line(
        "Entrada",
        if bridge.name().is_empty() { "usando sem login" } else { "conta conectada" },
    ));

    crate::components::scroll(&inside).upcast()
}

fn voice_page(bridge: &Rc<Bridge>) -> gtk::Widget {
    let inside = page("Voz e vídeo");

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

    crate::components::scroll(&inside).upcast()
}

/// Um dado da conta: o nome à esquerda, o valor à direita.
fn line(label: &str, value: &str) -> gtk::Box {
    let inside = row(10);
    let left = muted(label);
    let right = body(value);

    left.set_xalign(0.0);
    right.set_xalign(1.0);
    inside.append(&left);
    inside.append(&spacer());
    inside.append(&right);

    inside
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
