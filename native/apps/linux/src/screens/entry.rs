//! A tela sem conta: nome, criar uma sala, entrar por código — e o login, ao lado.
//!
//! É o caminho que não passa por banco nenhum, e ele não pode piorar por causa do outro.

use std::cell::RefCell;
use std::rc::Rc;

use core_app::models::User;
use gtk::prelude::*;

use crate::bridge::Bridge;
use crate::components::{button, card, column, danger, field, muted, row, strong, title};

pub struct EntryScreen {
    root: gtk::Box,
    name: gtk::Entry,
    code: gtk::Entry,
    error: gtk::Label,
    recent: gtk::Box,
    account: gtk::Box,
    greeting: gtk::Label,
    sign_in: gtk::Box,
    email: gtk::Entry,
    password: gtk::PasswordEntry,
    submit: gtk::Button,
    toggle: gtk::Button,
    heading: gtk::Label,
    hint: gtk::Label,
    registering: Rc<RefCell<bool>>,
}

impl EntryScreen {
    pub fn new(bridge: &Rc<Bridge>) -> Self {
        let name = field("Como aparecer para os outros");
        let code = field("Código da sala");
        let error = danger();
        let recent = row(6);
        let greeting = strong("");
        let email = field("voce@exemplo.com");
        let password = gtk::PasswordEntry::builder().show_peek_icon(true).hexpand(true).build();
        let submit = button("Entrar", "primary");
        let toggle = button("Criar conta", "link");
        let heading = title("Entrar");
        let hint = muted("Sem conta dá para compartilhar a tela. Servidores, voz e chat pedem login.");
        let registering = Rc::new(RefCell::new(false));

        name.set_max_length(40);
        name.set_text(&bridge.name());
        code.set_max_length(32);

        let room = card();

        room.append(&title("Criar uma sala"));
        room.append(&muted("Compartilhe sua tela com quem você quiser. Sem conta, sem cadastro."));
        room.append(&muted("Seu nome"));
        room.append(&name);

        let create = button("Criar uma sala sem login", "primary");

        room.append(&create);
        room.append(&muted("ou"));

        let joining = row(8);
        let join = button("Entrar", "ghost");

        joining.append(&code);
        joining.append(&join);
        room.append(&joining);
        room.append(&error);
        room.append(&recent);

        let account = card();

        account.append(&greeting);

        let leave_account = button("Sair da conta", "ghost");
        let to_hub = button("Ver meus servidores", "primary");

        account.append(&to_hub);
        account.append(&leave_account);

        let sign_in = card();

        sign_in.append(&heading);
        sign_in.append(&hint);
        sign_in.append(&muted("E-mail"));
        sign_in.append(&email);
        sign_in.append(&muted("Senha"));
        sign_in.append(&password);
        sign_in.append(&submit);
        sign_in.append(&toggle);

        let side = column(12);

        side.append(&account);
        side.append(&sign_in);

        let both = row(18);

        both.set_halign(gtk::Align::Center);
        both.set_valign(gtk::Align::Center);
        both.set_vexpand(true);
        both.append(&room);
        both.append(&side);

        let root = column(0);

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

        code.connect_activate(move |_| enter());

        submit.connect_clicked({
            let (bridge, email, password, registering) =
                (bridge.clone(), email.clone(), password.clone(), registering.clone());

            move |_| {
                bridge.sign_in(email.text().as_str(), password.text().as_str(), *registering.borrow())
            }
        });

        toggle.connect_clicked({
            let (registering, heading, hint, submit) =
                (registering.clone(), heading.clone(), hint.clone(), submit.clone());

            move |toggle| {
                let now = !*registering.borrow();

                registering.replace(now);
                wording(now, &heading, &hint, &submit, toggle);
            }
        });

        leave_account.connect_clicked({
            let bridge = bridge.clone();

            move |_| bridge.sign_out()
        });

        to_hub.connect_clicked({
            let bridge = bridge.clone();

            move |_| bridge.show_home()
        });

        let screen = Self {
            root,
            name,
            code,
            error,
            recent,
            account,
            greeting,
            sign_in,
            email,
            password,
            submit,
            toggle,
            heading,
            hint,
            registering,
        };

        screen.set_user(None);
        screen.refresh_recent(bridge);

        screen
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    pub fn set_error(&self, message: &str) {
        self.error.set_text(message);
    }

    /// Com conta, o nome vem da conta e o campo some; sem conta, o campo é a identidade.
    pub fn set_user(&self, user: Option<&User>) {
        let signed_in = user.is_some();

        self.account.set_visible(signed_in);
        self.sign_in.set_visible(!signed_in);
        self.name.set_visible(!signed_in);
        self.password.set_text("");

        if let Some(user) = user {
            self.greeting.set_text(&format!("Conectado como {}", user.name));
            self.name.set_text(&user.name);
        }

        self.email.set_sensitive(!signed_in);
        self.submit.set_sensitive(!signed_in);
        self.toggle.set_sensitive(!signed_in);
        self.registering.replace(false);
        wording(false, &self.heading, &self.hint, &self.submit, &self.toggle);
    }

    /// As salas anteriores viram botão: ninguém decora um código de 12 caracteres.
    pub fn refresh_recent(&self, bridge: &Rc<Bridge>) {
        crate::components::clear_box(&self.recent);

        for code in bridge.recent_rooms().into_iter().take(4) {
            let again = button(&code, "chip");

            again.connect_clicked({
                let (bridge, name, code) = (bridge.clone(), self.name.clone(), code.clone());

                move |_| bridge.join_room(name.text().as_str(), &code)
            });

            self.recent.append(&again);
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

/// Os mesmos quatro textos do cartão, virados para criar conta ou para entrar.
fn wording(registering: bool, heading: &gtk::Label, hint: &gtk::Label, submit: &gtk::Button, toggle: &gtk::Button) {
    heading.set_text(if registering { "Criar conta" } else { "Entrar" });
    hint.set_text(if registering {
        "Para ter servidores, voz e chat."
    } else {
        "Sem conta dá para compartilhar a tela. Servidores, voz e chat pedem login."
    });
    submit.set_label(if registering { "Criar conta" } else { "Entrar" });
    toggle.set_label(if registering { "Entrar" } else { "Criar conta" });
}
