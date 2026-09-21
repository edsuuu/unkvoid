//! Os pedaços que se repetem entre as telas.
//!
//! Só forma: nenhum deles sabe o que é uma sala, um canal ou um membro.

use gtk::prelude::*;

pub fn column(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}

pub fn row(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}

pub fn title(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));

    label.add_css_class("title");
    label.set_xalign(0.0);

    label
}

pub fn strong(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));

    label.add_css_class("strong");
    label.set_xalign(0.0);

    label
}

pub fn muted(text: &str) -> gtk::Label {
    let label = gtk::Label::builder().label(text).wrap(true).xalign(0.0).build();

    label.add_css_class("muted");

    label
}

pub fn danger() -> gtk::Label {
    let label = gtk::Label::builder().label("").wrap(true).xalign(0.5).build();

    label.add_css_class("danger");

    label
}

pub fn card() -> gtk::Box {
    let card = column(10);

    card.add_css_class("card");
    pad(&card, 18);

    card
}

pub fn field(placeholder: &str) -> gtk::Entry {
    gtk::Entry::builder().placeholder_text(placeholder).hexpand(true).build()
}

pub fn button(label: &str, style: &str) -> gtk::Button {
    let button = gtk::Button::with_label(label);

    button.add_css_class(style);

    button
}

/// `Single` porque a linha precisa responder ao clique; `None` para lista que só mostra.
pub fn list(clickable: bool) -> gtk::ListBox {
    let list = gtk::ListBox::new();

    list.set_selection_mode(if clickable {
        gtk::SelectionMode::Single
    } else {
        gtk::SelectionMode::None
    });
    list.add_css_class("list");

    list
}

pub fn scroll(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(child)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build()
}

pub fn pad(widget: &impl IsA<gtk::Widget>, margin: i32) {
    let widget = widget.as_ref();

    widget.set_margin_top(margin);
    widget.set_margin_bottom(margin);
    widget.set_margin_start(margin);
    widget.set_margin_end(margin);
}

/// Esvaziar antes de redesenhar. O `remove_all` do GTK só existe do 4.12 para cá, e a
/// distribuição que este app precisa alcançar (o WebKitGTK do Debian é o motivo de ele
/// existir) costuma vir com menos que isso.
pub fn clear_list(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

pub fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

/// Uma linha de lista já com o conteúdo dentro, para o índice da linha valer como índice
/// da coisa que ela mostra.
pub fn item(child: &impl IsA<gtk::Widget>) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();

    row.set_child(Some(child));

    row
}
