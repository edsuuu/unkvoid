//! A abertura, e depois a atualização.
//!
//! No Linux a versão vem do gerenciador de pacotes, não do app; enquanto for assim, esta
//! tela é só o "entrando…" dos primeiros segundos.

use gtk::prelude::*;

use crate::components::{column, muted, title};

pub struct UpdatingScreen {
    root: gtk::Box,
    status: gtk::Label,
    progress: gtk::ProgressBar,
}

impl Default for UpdatingScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdatingScreen {
    pub fn new() -> Self {
        let status = muted("Procurando o servidor…");
        let progress = gtk::ProgressBar::new();

        progress.set_size_request(280, -1);
        progress.pulse();

        let root = column(10);

        root.set_valign(gtk::Align::Center);
        root.set_halign(gtk::Align::Center);
        root.set_vexpand(true);
        root.append(&title("Unkvoid"));
        root.append(&status);
        root.append(&progress);

        Self { root, status, progress }
    }

    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    pub fn set_status(&self, message: &str) {
        self.status.set_text(message);
        self.progress.pulse();
    }
}
