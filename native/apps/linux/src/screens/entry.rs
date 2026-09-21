//! A tela sem conta: nome, criar uma sala, entrar por código — e o login, ao lado.
//!
//! É o caminho que não passa por banco nenhum, e ele não pode piorar por causa do outro.
//!
//! O desenho é o do `apps/desktop/ui/components/entry`: dois cartões de 420 lado a lado,
//! tudo centralizado dentro deles, e o segundo some quando já há conta.

use std::cell::RefCell;
use std::rc::Rc;

use core_app::models::User;
use gtk::prelude::*;

use crate::bridge::Bridge;
use crate::components::{
    avatar, button, card, centered, column, danger, divider, field, google_button, label_mono, muted, row, strong,
    title,
};

/// A folga em volta do bloco, o `p-6` do React.
const AROUND: i32 = 24;

/// O quanto o bloco desce do topo, o `pt-[10vh]` do React numa janela de 800. O GTK não tem
/// `vh`: é a altura de projeto, e a tela rola se a janela for menor.
const FROM_TOP: i32 = 80;

pub struct EntryScreen {
    root: gtk::Box,
    name: gtk::Entry,
    name_field: gtk::Box,
    code: gtk::Entry,
    error: gtk::Label,
    recent: gtk::FlowBox,
    signed: gtk::Box,
    face: gtk::Box,
    greeting: gtk::Label,
    to_hub: gtk::Button,
    sign_in: gtk::Box,
    email: gtk::Entry,
    password: gtk::Entry,
    create: gtk::Button,
    submit: gtk::Button,
    toggle: gtk::Button,
    switch_hint: gtk::Label,
    heading: gtk::Label,
    hint: gtk::Label,
    login_error: gtk::Label,
    registering: Rc<RefCell<bool>>,
}

impl EntryScreen {
    pub fn new(bridge: &Rc<Bridge>) -> Self {
        let name = field("Como aparecer para os outros");
        let code = field("Código da sala");
        let error = danger();
        let login_error = danger();
        let recent = crate::components::chip_wrap();
        let greeting = strong("");
        let email = field("voce@email.com");
        let password = field("••••••••");
        let create = button("Criar uma sala sem login", "primary");
        let submit = button("Entrar", "primary");
        let toggle = button("Criar conta", "link");
        let switch_hint = crate::components::dim("Não tem conta?");
        let heading = centered(&title("Entrar"));
        let hint = centered(&muted(""));
        let to_hub = button("Voltar aos servidores", "ghost");
        let registering = Rc::new(RefCell::new(false));

        // O campo de senha esconde o que se digita, como o `type="password"` do React. O
        // `PasswordEntry` do GTK traria o olhinho de espiar, que o app em React não tem.
        password.set_visibility(false);

        name.set_max_length(40);
        name.set_text(&bridge.name());
        code.set_max_length(32);

        let room = card();

        room.append(&centered(&title("Criar uma sala")));

        let subtitle = centered(&muted("Compartilhe sua tela com quem você quiser."));

        subtitle.set_margin_top(6);
        subtitle.set_margin_bottom(24);
        room.append(&subtitle);

        // Com conta, o rosto e o nome ficam no lugar do campo: a identidade já está decidida.
        let face = column(10);

        face.set_halign(gtk::Align::Center);
        face.set_margin_bottom(20);
        face.append(&greeting);

        let name_field = column(8);

        name_field.set_margin_bottom(14);
        name_field.append(&label_mono("Seu nome"));
        name_field.append(&name);

        room.append(&face);
        room.append(&name_field);
        room.append(&create);

        let or_room = divider("ou");

        or_room.set_margin_top(16);
        or_room.set_margin_bottom(16);
        room.append(&or_room);

        let joining = row(8);
        let join = button("Entrar", "ghost");

        joining.append(&code);
        joining.append(&join);
        room.append(&joining);

        error.set_margin_top(12);
        room.append(&error);
        recent.set_margin_top(4);
        room.append(&recent);

        let signed = column(0);

        signed.set_valign(gtk::Align::End);
        signed.set_vexpand(true);
        to_hub.set_margin_top(16);
        signed.append(&to_hub);
        room.append(&signed);

        let sign_in = card();

        sign_in.append(&heading);
        hint.set_margin_top(6);
        hint.set_margin_bottom(20);
        sign_in.append(&hint);

        let google = google_button("Entrar com Google");

        sign_in.append(&google);

        let or_login = divider("ou");

        or_login.set_margin_top(16);
        or_login.set_margin_bottom(16);
        sign_in.append(&or_login);

        let email_field = column(8);

        email_field.append(&label_mono("E-mail"));
        email_field.append(&email);
        sign_in.append(&email_field);

        let password_field = column(8);

        password_field.set_margin_top(14);
        password_field.append(&label_mono("Senha"));
        password_field.append(&password);
        sign_in.append(&password_field);

        submit.set_margin_top(16);
        sign_in.append(&submit);
        login_error.set_margin_top(12);
        sign_in.append(&login_error);

        let switching = row(5);

        switching.set_halign(gtk::Align::Center);
        switching.set_margin_top(16);
        switching.append(&switch_hint);
        switching.append(&toggle);
        sign_in.append(&switching);

        let both = row(20);

        both.set_halign(gtk::Align::Center);
        both.set_valign(gtk::Align::Start);
        both.set_margin_top(FROM_TOP - AROUND);
        both.append(&room);
        both.append(&sign_in);

        let root = column(0);

        crate::components::pad(&root, AROUND);
        root.append(&both);

        create.connect_clicked({
            let (bridge, name, code) = (bridge.clone(), name.clone(), code.clone());

            move |_| bridge.create_room(name.text().as_str(), code.text().as_str())
        });

        let enter = {
            let (bridge, name, code) = (bridge.clone(), name.clone(), code.clone());

            move || bridge.join_room(name.text().as_str(), code.text().as_str())
        };

        join.connect_clicked({
            let enter = enter.clone();

            move |_| enter()
        });

        code.connect_activate({
            let enter = enter.clone();

            move |_| enter()
        });

        name.connect_activate({
            let (bridge, name, code) = (bridge.clone(), name.clone(), code.clone());

            move |_| bridge.create_room(name.text().as_str(), code.text().as_str())
        });

        // O fluxo do Google atravessa o navegador e volta por `unkvoid://`, e esse caminho
        // ainda não existe fora do Tauri. Dizer isso é melhor que um botão que não responde.
        google.connect_clicked({
            let login_error = login_error.clone();

            move |_| login_error.set_text("Entrar com Google ainda não funciona no app do Linux. Use e-mail e senha.")
        });

        submit.connect_clicked({
            let (bridge, email, password, registering) =
                (bridge.clone(), email.clone(), password.clone(), registering.clone());

            move |_| {
                bridge.sign_in(email.text().as_str(), password.text().as_str(), *registering.borrow())
            }
        });

        password.connect_activate({
            let (bridge, email, password, registering) =
                (bridge.clone(), email.clone(), password.clone(), registering.clone());

            move |_| {
                bridge.sign_in(email.text().as_str(), password.text().as_str(), *registering.borrow())
            }
        });

        toggle.connect_clicked({
            let (registering, heading, hint, submit, switch_hint, password) = (
                registering.clone(),
                heading.clone(),
                hint.clone(),
                submit.clone(),
                switch_hint.clone(),
                password.clone(),
            );

            move |toggle| {
                let now = !*registering.borrow();

                registering.replace(now);
                wording(now, &heading, &hint, &submit, toggle, &switch_hint, &password);
            }
        });

        to_hub.connect_clicked({
            let bridge = bridge.clone();

            move |_| bridge.show_home()
        });

        let screen = Self {
            root,
            name,
            name_field,
            code,
            error,
            recent,
            signed,
            face,
            greeting,
            to_hub,
            sign_in,
            email,
            password,
            create,
            submit,
            toggle,
            switch_hint,
            heading,
            hint,
            login_error,
            registering,
        };

        screen.set_user(None);
        screen.refresh_recent(bridge);

        screen
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// O erro do cartão da sala: nome vazio, código que não existe, servidor fora do ar.
    pub fn set_error(&self, message: &str) {
        self.error.set_text(message);
    }

    /// O erro do cartão de login, que é outro cartão e outra frase.
    pub fn set_login_error(&self, message: &str) {
        self.login_error.set_text(message);
    }

    /// Com conta, o nome vem da conta e o campo some; sem conta, o campo é a identidade.
    pub fn set_user(&self, user: Option<&User>) {
        let signed_in = user.is_some();

        // Entrou (ou saiu): o erro da tentativa anterior não vale mais.
        self.login_error.set_text("");
        self.face.set_visible(signed_in);
        self.signed.set_visible(signed_in);
        self.to_hub.set_visible(signed_in);
        self.sign_in.set_visible(!signed_in);
        self.name_field.set_visible(!signed_in);
        self.password.set_text("");
        self.create.set_label(if signed_in { "Criar uma sala" } else { "Criar uma sala sem login" });

        crate::components::clear_box(&self.face);

        if let Some(user) = user {
            self.face.append(&avatar(&user.name, 54, true));
            self.greeting.set_text(&user.name);
            self.face.append(&self.greeting);
            self.name.set_text(&user.name);
        }

        self.email.set_sensitive(!signed_in);
        self.submit.set_sensitive(!signed_in);
        self.toggle.set_sensitive(!signed_in);
        self.registering.replace(false);
        wording(
            false,
            &self.heading,
            &self.hint,
            &self.submit,
            &self.toggle,
            &self.switch_hint,
            &self.password,
        );
    }

    /// As salas anteriores viram botão: ninguém decora um código de 12 caracteres.
    pub fn refresh_recent(&self, bridge: &Rc<Bridge>) {
        crate::components::clear_flow(&self.recent);

        for code in bridge.recent_rooms().into_iter().take(3) {
            let again = button(&code, "chip");

            again.connect_clicked({
                let (bridge, name, code) = (bridge.clone(), self.name.clone(), code.clone());

                move |_| bridge.join_room(name.text().as_str(), &code)
            });

            self.recent.insert(&again, -1);
        }
    }

    pub fn focus(&self) {
        // `EntryExt::is_visible` é outra coisa (o campo esconde o que se digita), e o nome
        // igual faz a chamada curta virar erro de compilação.
        if WidgetExt::is_visible(&self.name) {
            self.name.grab_focus();
        } else {
            self.code.grab_focus();
        }
    }
}

/// Os mesmos textos do cartão, virados para criar conta ou para entrar.
fn wording(
    registering: bool,
    heading: &gtk::Label,
    hint: &gtk::Label,
    submit: &gtk::Button,
    toggle: &gtk::Button,
    switch_hint: &gtk::Label,
    password: &gtk::Entry,
) {
    heading.set_text(if registering { "Criar conta" } else { "Entrar" });
    hint.set_text(if registering {
        "Para ter servidores, voz e chat."
    } else {
        "Sem conta dá para compartilhar a tela. Servidores, voz e chat pedem login."
    });
    submit.set_label(if registering { "Criar conta" } else { "Entrar" });
    toggle.set_label(if registering { "Entrar" } else { "Criar conta" });
    switch_hint.set_text(if registering { "Já tem conta?" } else { "Não tem conta?" });
    password.set_placeholder_text(Some(if registering { "8 ou mais" } else { "••••••••" }));
}
