//! O servidor não respondeu. A tela diz o que está sendo tentado, e o núcleo é quem espera.

use gtk::prelude::*;

use crate::components::{column, muted, title};

pub struct OfflineScreen {
    root: gtk::Box,
    status: gtk::Label,
}

impl Default for OfflineScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl OfflineScreen {
    pub fn new() -> Self {
        let status = muted("");
        let spinner = gtk::Spinner::new();

        spinner.start();

        let root = column(10);

        root.set_valign(gtk::Align::Center);
        root.set_halign(gtk::Align::Center);
        root.set_vexpand(true);
        root.append(&spinner);
        root.append(&title("Servidor sem resposta"));
        root.append(&status);

        Self { root, status }
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    pub fn set_status(&self, message: &str) {
        self.status.set_text(message);
    }
}
