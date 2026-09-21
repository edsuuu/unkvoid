//! Os pedaços que se repetem entre as telas.
//!
//! Só forma: nenhum deles sabe o que é uma sala, um canal ou um membro. As medidas saem do
//! app em React (`apps/desktop/ui`), classe por classe, para as duas telas serem a mesma.

use gtk::prelude::*;

use crate::icons;

/// A largura do cartão de entrada, a mesma do `max-w-[420px]` do React.
pub const CARD: i32 = 420;

/// Quantos caracteres um texto corrido pede antes de quebrar. Sem este teto o GTK pede a
/// largura da frase inteira, e uma frase comprida estica o cartão para fora do desenho.
const WRAP_AT: i32 = 34;

pub fn column(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}

pub fn row(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}

pub fn title(text: &str) -> gtk::Label {
    labelled(text, "title")
}

/// O título maior, o do vazio da sala.
pub fn headline(text: &str) -> gtk::Label {
    labelled(text, "headline")
}

pub fn strong(text: &str) -> gtk::Label {
    labelled(text, "strong")
}

/// O texto de uma linha de lista: mais claro que o apagado, menor que o forte.
pub fn body(text: &str) -> gtk::Label {
    labelled(text, "body")
}

pub fn muted(text: &str) -> gtk::Label {
    let label = gtk::Label::builder().label(text).wrap(true).xalign(0.0).max_width_chars(WRAP_AT).build();

    label.add_css_class("muted");

    label
}

/// O texto miúdo que acompanha um número: o ping, a contagem, o "você".
pub fn dim(text: &str) -> gtk::Label {
    labelled(text, "dim")
}

/// O rótulo acima do campo, em maiúsculas. O CSS faz a caixa alta: escrever o texto já
/// gritado deixaria o leitor de tela soletrando letra por letra.
pub fn label_mono(text: &str) -> gtk::Label {
    labelled(text, "label-mono")
}

/// Número ou código, na monoespaçada do desenho.
pub fn mono(text: &str) -> gtk::Label {
    labelled(text, "mono")
}

/// O código da sala no meio de uma frase: violeta claro sobre fundo violeta, para saltar do
/// texto em volta. É o `code-chip` do React.
pub fn code_chip(text: &str) -> gtk::Label {
    let label = labelled(text, "code-chip");

    label.set_valign(gtk::Align::Center);

    label
}

pub fn danger() -> gtk::Label {
    let label = gtk::Label::builder().label("").wrap(true).xalign(0.5).max_width_chars(WRAP_AT).build();

    label.add_css_class("danger");

    label
}

fn labelled(text: &str, style: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));

    label.add_css_class(style);
    label.set_xalign(0.0);

    label
}

/// Centraliza o rótulo. O cartão de entrada centraliza tudo; a lista, nada.
pub fn centered(label: &gtk::Label) -> gtk::Label {
    label.set_xalign(0.5);
    label.set_justify(gtk::Justification::Center);

    label.clone()
}

/// O cartão de 420 do React. A largura é pedida e travada: `set_size_request` sozinho é um
/// mínimo, e um texto comprido dentro esticaria o cartão para além do desenho.
pub fn card() -> gtk::Box {
    let card = column(0);

    card.add_css_class("card");
    card.set_size_request(CARD, -1);
    card.set_hexpand(false);
    card.set_halign(gtk::Align::Start);

    card
}

pub fn field(placeholder: &str) -> gtk::Entry {
    gtk::Entry::builder().placeholder_text(placeholder).hexpand(true).build()
}

pub fn button(label: &str, style: &str) -> gtk::Button {
    let button = gtk::Button::with_label(label);

    button.add_css_class(style);
    clickable(&button);

    button
}

/// A mãozinha em cima do que responde ao clique. No GTK o cursor não é CSS como no React
/// (`cursor: pointer`): ele é uma propriedade do widget, e sem isto tudo fica com a seta.
pub fn clickable(widget: &impl IsA<gtk::Widget>) {
    widget.as_ref().set_cursor_from_name(Some("pointer"));
}

/// O botão quadrado que só tem desenho dentro. A cor entra aqui porque o GTK não deixa a
/// imagem herdar a cor do botão como o `currentColor` do React faz.
pub fn icon_button(name: &str, size: i32, color: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::new();

    button.set_child(Some(&icons::icon(name, size, color)));
    button.add_css_class("icon");
    button.set_tooltip_text(Some(tooltip));
    button.set_valign(gtk::Align::Center);
    clickable(&button);

    button
}

/// Troca o desenho de dentro sem trocar o botão: é o que liga e desliga o microfone, a
/// câmera e o som sem a janela piscar.
pub fn set_icon(button: &gtk::Button, name: &str, size: i32, color: &str) {
    button.set_child(Some(&icons::icon(name, size, color)));
}

/// A linha fina de 1 px. Duas delas, com o "ou" no meio, fazem o divisor do React.
pub fn rule() -> gtk::Box {
    let rule = gtk::Box::new(gtk::Orientation::Horizontal, 0);

    rule.add_css_class("rule");
    rule.set_hexpand(true);
    rule.set_valign(gtk::Align::Center);

    rule
}

pub fn divider(text: &str) -> gtk::Box {
    let divider = row(12);

    divider.append(&rule());
    divider.append(&label_mono(text));
    divider.append(&rule());

    divider
}

/// O tamanho da letra dentro do círculo, a mesma conta do React: um terço do diâmetro, e
/// nunca menos que 9 — abaixo disso as iniciais somem.
fn initials_size(diameter: i32) -> i32 {
    ((diameter as f32 * 0.33).round() as i32).max(9)
}

/// As iniciais do nome num círculo. `mine` é o que ganha o violeta: ele marca o que é seu.
pub fn avatar(name: &str, size: i32, mine: bool) -> gtk::Label {
    let words: Vec<&str> = name.split_whitespace().collect();
    let initials = match words.as_slice() {
        [] => "?".to_owned(),
        [single] => single.chars().take(2).collect::<String>(),
        [first, second, ..] => {
            format!("{}{}", first.chars().next().unwrap_or('?'), second.chars().next().unwrap_or('?'))
        }
    };

    let label = gtk::Label::new(Some(&initials.to_uppercase()));

    label.add_css_class("avatar");

    if !mine {
        label.add_css_class("flat");
    }

    // O tamanho da letra acompanha o círculo, e isso o CSS não faz: uma classe só teria um
    // valor fixo, e o mesmo avatar aparece em 24, 26 e 54.
    let attributes = gtk::pango::AttrList::new();

    attributes.insert(gtk::pango::AttrSize::new_size_absolute(initials_size(size) * gtk::pango::SCALE));
    label.set_attributes(Some(&attributes));

    label.set_size_request(size, size);
    label.set_valign(gtk::Align::Center);
    label.set_halign(gtk::Align::Center);

    label
}

/// A pílula de contorno redondo: o tempo na sala, a contagem, o ping.
pub fn pill(spacing: i32) -> gtk::Box {
    let pill = row(spacing);

    pill.add_css_class("pill");
    pill.set_valign(gtk::Align::Center);

    pill
}

/// A bolinha verde de quem está conectado.
pub fn online_dot() -> gtk::Box {
    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);

    dot.add_css_class("dot-online");
    dot.set_valign(gtk::Align::Center);
    dot.set_size_request(7, 7);

    dot
}

pub fn badge(text: &str, style: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));

    label.add_css_class("badge");
    label.add_css_class(style);
    label.set_valign(gtk::Align::Center);

    label
}

/// O botão branco do Google, com a marca à esquerda do texto.
pub fn google_button(label: &str) -> gtk::Button {
    let button = gtk::Button::new();
    let inside = row(10);

    inside.set_halign(gtk::Align::Center);
    inside.append(&icons::google_mark(18));
    inside.append(&gtk::Label::new(Some(label)));
    button.set_child(Some(&inside));
    button.add_css_class("google");
    clickable(&button);

    button
}

/// O popover do React não tem setinha e encosta na borda de baixo do botão: `p-2` por dentro,
/// canto de 14 e a sombra funda. O do GTK vem com bico e com a folga do tema.
pub fn popover(child: &impl IsA<gtk::Widget>) -> gtk::Popover {
    let popover = gtk::Popover::new();

    popover.set_child(Some(child));
    popover.set_has_arrow(false);
    popover.set_position(gtk::PositionType::Bottom);
    popover.set_halign(gtk::Align::Start);

    popover
}

/// Uma coluna que estica para empurrar o que vem depois para a outra ponta.
pub fn spacer() -> gtk::Box {
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);

    spacer.set_hexpand(true);

    spacer
}

/// Uma fila de chips que quebra a linha em vez de esticar o cartão. Numa `Box` horizontal o
/// quarto código empurraria o cartão para além dos 420 do desenho.
pub fn chip_wrap() -> gtk::FlowBox {
    let wrap = gtk::FlowBox::new();

    wrap.set_selection_mode(gtk::SelectionMode::None);
    wrap.set_halign(gtk::Align::Center);
    wrap.set_row_spacing(6);
    wrap.set_column_spacing(6);
    wrap.set_min_children_per_line(1);
    wrap.set_max_children_per_line(3);

    wrap
}

pub fn clear_flow(wrap: &gtk::FlowBox) {
    while let Some(child) = wrap.first_child() {
        wrap.remove(&child);
    }
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
