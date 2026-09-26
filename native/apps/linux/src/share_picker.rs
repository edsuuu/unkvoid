//! O seletor de tela do React (`ShareModal.tsx`): as telas e os aplicativos com a prévia, o
//! áudio, a qualidade e o fps, Cancelar e Transmitir. No ar, é o "Mudar a transmissão" — e
//! é daqui que ela para.
//!
//! No Wayland quem escolhe a tela é o próprio sistema (o portal): aqui ficam só as opções, e
//! o seletor do sistema abre ao transmitir.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};
use serde_json::{Value, json};

use crate::bridge::Bridge;
use crate::components::{button, clickable, column, label_mono, muted, row, spacer, title};

/// Quantos aplicativos o seletor lista, e quantos ganham prévia — os números do React.
const MAX_WINDOWS: usize = 12;
const MAX_WINDOW_PREVIEWS: usize = 4;
const CARD_WIDTH: i32 = 160;

#[derive(Clone)]
struct Source {
    value: String,
    label: String,
    detail: String,
}

/// O que a thread de listar e de tirar prévias devolve para a janela.
enum Found {
    Listed { portal: bool, displays: Vec<Source>, windows: Vec<Source> },
    Preview { value: String, jpeg: Vec<u8> },
}

/// O que a janela sabe entre um clique e outro.
#[derive(Default)]
struct Picking {
    tab: String,
    chosen: String,
    portal: bool,
    displays: Vec<Source>,
    windows: Vec<Source>,
    /// As prévias que já chegaram, para a troca de aba não pedir de novo.
    previews: Vec<(String, gdk::Texture)>,
    /// Os cartões na tela agora, para marcar o escolhido e pôr a prévia no lugar.
    cards: Vec<(String, gtk::Button, gtk::Picture)>,
}

pub fn open(bridge: &Rc<Bridge>, parent: Option<&gtk::Window>) {
    let window = gtk::Window::new();
    let sharing = bridge.is_sharing();
    let body = column(0);
    let (quality, fps) = bridge.share_quality();
    let picking = Rc::new(RefCell::new(Picking { tab: "display".into(), ..Picking::default() }));

    window.set_title(Some(if sharing { "Mudar a transmissão" } else { "Compartilhar tela" }));
    window.set_modal(true);
    window.set_transient_for(parent);
    window.set_default_size(560, -1);
    window.set_resizable(false);
    window.add_css_class("settings");
    crate::components::pad(&body, 24);

    body.append(&title(if sharing { "Mudar a transmissão" } else { "Compartilhar tela" }));

    let subtitle = muted("Escolha o que a sala vai ver.");

    subtitle.set_margin_top(4);
    body.append(&subtitle);

    // ---- as abas ----
    let tabs = row(6);
    let displays_tab = button("Telas", "tab-soft");
    let windows_tab = button("Aplicativos", "tab-soft");

    displays_tab.add_css_class("on");
    tabs.set_margin_top(18);
    tabs.append(&displays_tab);
    tabs.append(&windows_tab);
    body.append(&tabs);

    // ---- os cartões ----
    let grid = gtk::FlowBox::builder()
        .min_children_per_line(3)
        .max_children_per_line(3)
        .column_spacing(12)
        .row_spacing(12)
        .homogeneous(true)
        .selection_mode(gtk::SelectionMode::None)
        .valign(gtk::Align::Start)
        .build();
    let cards_scroll = gtk::ScrolledWindow::builder()
        .child(&grid)
        .min_content_height(340)
        .max_content_height(340)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    let portal_note = muted("O sistema vai perguntar qual tela ou janela compartilhar.");

    cards_scroll.set_margin_top(16);
    portal_note.set_margin_top(16);
    portal_note.set_visible(false);
    body.append(&cards_scroll);
    body.append(&portal_note);

    for _ in 0..2 {
        let skeleton = gtk::Box::new(gtk::Orientation::Vertical, 0);

        skeleton.add_css_class("skeleton");
        skeleton.set_size_request(CARD_WIDTH, 150);
        grid.insert(&skeleton, -1);
    }

    // ---- o áudio ----
    let audio = gtk::CheckButton::with_label("Transmitir o áudio");
    let mute_calls = gtk::CheckButton::with_label("Sem o áudio do Discord");
    let sound = row(20);
    let linux_note = muted("No Linux vai o som do sistema inteiro: não dá para deixar o Discord de fora.");

    audio.set_active(true);
    mute_calls.set_active(true);
    sound.set_margin_top(20);
    sound.append(&audio);
    sound.append(&mute_calls);
    body.append(&sound);
    linux_note.set_margin_top(8);
    body.append(&linux_note);

    audio.connect_toggled({
        let (mute_calls, linux_note) = (mute_calls.clone(), linux_note.clone());

        move |audio| {
            mute_calls.set_sensitive(audio.is_active());
            linux_note.set_visible(audio.is_active() && mute_calls.is_active());
        }
    });
    mute_calls.connect_toggled({
        let (audio, linux_note) = (audio.clone(), linux_note.clone());

        move |mute_calls| linux_note.set_visible(audio.is_active() && mute_calls.is_active())
    });

    // ---- a qualidade, o fps e os botões ----
    let qualities = core_app::app::QUALITIES;
    let rates = core_app::app::FRAME_RATES;
    let quality_labels: Vec<String> = qualities.iter().map(|value| quality_label(value)).collect();
    let quality_list = gtk::DropDown::from_strings(&quality_labels.iter().map(String::as_str).collect::<Vec<_>>());
    let fps_list = gtk::DropDown::from_strings(&rates);

    quality_list.set_selected(qualities.iter().position(|value| *value == quality).unwrap_or(1) as u32);
    fps_list.set_selected(rates.iter().position(|value| *value == fps).unwrap_or(1) as u32);

    let footer = row(12);
    let cancel = button("Cancelar", "quiet");
    let transmit = button("Transmitir", "primary");

    footer.set_margin_top(22);
    footer.append(&labelled_choice("Qualidade", &quality_list));
    footer.append(&labelled_choice("FPS", &fps_list));
    footer.append(&spacer());

    // No ar, o seletor é o "Mudar a transmissão" — e é daqui que ela para, como o "Parar de
    // transmitir" do menu do React.
    if sharing {
        let stop = button("Parar de transmitir", "danger");

        stop.set_valign(gtk::Align::End);
        stop.connect_clicked({
            let (bridge, window) = (bridge.clone(), window.clone());

            move |_| {
                bridge.stop_sharing();
                window.close();
            }
        });
        footer.append(&stop);
    }

    cancel.set_valign(gtk::Align::End);
    transmit.set_valign(gtk::Align::End);
    transmit.set_sensitive(false);
    footer.append(&cancel);
    footer.append(&transmit);
    body.append(&footer);

    window.set_child(Some(&body));

    cancel.connect_clicked({
        let window = window.clone();

        move |_| window.close()
    });

    let escape = gtk::EventControllerKey::new();

    escape.connect_key_pressed({
        let window = window.clone();

        move |_, key, _, _| {
            if key == gdk::Key::Escape {
                window.close();

                return glib::Propagation::Stop;
            }

            glib::Propagation::Proceed
        }
    });
    window.add_controller(escape);

    transmit.connect_clicked({
        let (bridge, window, picking) = (bridge.clone(), window.clone(), picking.clone());
        let (audio, mute_calls) = (audio.clone(), mute_calls.clone());
        let (quality_list, fps_list) = (quality_list.clone(), fps_list.clone());

        move |_| {
            let quality = qualities[quality_list.selected() as usize % qualities.len()];
            let fps = rates[fps_list.selected() as usize % rates.len()];
            let picking = picking.borrow();

            bridge.remember_share_quality(quality, fps);
            bridge.share(json!({
                "source": picking.chosen,
                "quality": quality,
                "fps": fps.parse::<u64>().unwrap_or(60),
                "audio": audio.is_active(),
                "muteCalls": mute_calls.is_active(),
                "portal": picking.portal,
            }));
            window.close();
        }
    });

    for (tab, other, name) in [(&displays_tab, &windows_tab, "display"), (&windows_tab, &displays_tab, "window")] {
        tab.connect_clicked({
            let (grid, picking, transmit, other) = (grid.clone(), picking.clone(), transmit.clone(), other.clone());

            move |tab| {
                tab.add_css_class("on");
                other.remove_css_class("on");
                picking.borrow_mut().tab = name.to_owned();
                show_tab(&grid, &picking, &transmit);
            }
        });
    }

    // A lista e as prévias levam segundos (no X11 é rodar `xrandr` e tirar um quadro de
    // cada), e ficam numa thread: a janela abre na hora, com o esqueleto no lugar.
    let (found, mut arriving) = tokio::sync::mpsc::unbounded_channel::<Found>();

    std::thread::spawn(move || list_sources(&found));

    glib::spawn_future_local({
        let (grid, picking, transmit, tabs, cards_scroll, portal_note) =
            (grid.clone(), picking.clone(), transmit.clone(), tabs.clone(), cards_scroll.clone(), portal_note.clone());

        async move {
            while let Some(news) = arriving.recv().await {
                match news {
                    Found::Listed { portal, displays, windows } => {
                        {
                            let mut picking = picking.borrow_mut();

                            picking.portal = portal;
                            picking.displays = displays;
                            picking.windows = windows;
                        }

                        tabs.set_visible(!portal);
                        cards_scroll.set_visible(!portal);
                        portal_note.set_visible(portal);
                        show_tab(&grid, &picking, &transmit);

                        if portal {
                            transmit.set_sensitive(true);
                        }
                    }
                    Found::Preview { value, jpeg } => {
                        let Ok(texture) = gdk::Texture::from_bytes(&glib::Bytes::from(&jpeg)) else {
                            continue;
                        };
                        let mut picking = picking.borrow_mut();

                        if let Some((_, _, picture)) = picking.cards.iter().find(|(card, _, _)| *card == value) {
                            picture.set_paintable(Some(&texture));
                        }

                        picking.previews.push((value, texture));
                    }
                }
            }
        }
    });

    window.present();
}

/// Lista as telas e os aplicativos, e tira a prévia das telas e dos primeiros aplicativos.
fn list_sources(found: &tokio::sync::mpsc::UnboundedSender<Found>) {
    let listed = core_app::sharing::displays().unwrap_or_else(|failure| {
        tracing::warn!(%failure, "seletor: não deu para listar as telas");

        Value::Null
    });
    let portal = listed["portal"].as_bool().unwrap_or(false);
    let displays = sources_of(&listed["displays"], |display| Source {
        value: format!("display:{}", display["id"]),
        label: format!("Tela {}", display["id"]),
        detail: format!("{}×{}", display["width"], display["height"]),
    });
    let windows: Vec<Source> = sources_of(&listed["windows"], |shown| Source {
        value: format!("window:{}", shown["id"]),
        label: shown["title"].as_str().unwrap_or_default().to_owned(),
        detail: shown["application"].as_str().unwrap_or_default().to_owned(),
    })
    .into_iter()
    .filter(|shown| !shown.label.trim().is_empty())
    .take(MAX_WINDOWS)
    .collect();
    let wanted: Vec<String> = displays
        .iter()
        .map(|display| display.value.clone())
        .chain(windows.iter().take(MAX_WINDOW_PREVIEWS).map(|shown| shown.value.clone()))
        .collect();

    if found.send(Found::Listed { portal, displays, windows }).is_err() || portal {
        return;
    }

    for value in wanted {
        let source = core_app::sharing::capture_config(&json!({ "source": value })).source;
        let jpeg = capture::PlatformCapturer::preview(source).unwrap_or_default();

        if !jpeg.is_empty() && found.send(Found::Preview { value, jpeg }).is_err() {
            return;
        }
    }
}

fn sources_of(listed: &Value, source: impl Fn(&Value) -> Source) -> Vec<Source> {
    listed.as_array().map(|items| items.iter().map(source).collect()).unwrap_or_default()
}

/// Desenha os cartões da aba aberta e escolhe o primeiro, como o React.
fn show_tab(grid: &gtk::FlowBox, picking: &Rc<RefCell<Picking>>, transmit: &gtk::Button) {
    while let Some(child) = grid.first_child() {
        grid.remove(&child);
    }

    let mut state = picking.borrow_mut();
    let items = if state.tab == "window" { state.windows.clone() } else { state.displays.clone() };

    state.cards.clear();
    state.chosen = items.first().map(|item| item.value.clone()).unwrap_or_default();
    transmit.set_sensitive(!state.chosen.is_empty());

    if items.is_empty() {
        grid.insert(
            &muted(if state.tab == "window" {
                "Nenhuma janela aberta para compartilhar."
            } else {
                "Nenhuma tela X11 encontrada. Em sessão Wayland a captura ainda não funciona."
            }),
            -1,
        );

        return;
    }

    for item in &items {
        let (card, picture) = source_card(item);

        if let Some((_, texture)) = state.previews.iter().find(|(value, _)| *value == item.value) {
            picture.set_paintable(Some(texture));
        }

        if item.value == state.chosen {
            card.add_css_class("on");
        }

        card.connect_clicked({
            let (picking, value) = (picking.clone(), item.value.clone());

            move |_| {
                let mut state = picking.borrow_mut();

                state.chosen.clone_from(&value);

                for (other, button, _) in &state.cards {
                    if *other == value {
                        button.add_css_class("on");
                    } else {
                        button.remove_css_class("on");
                    }
                }
            }
        });

        grid.insert(&card, -1);
        state.cards.push((item.value.clone(), card, picture));
    }
}

/// Uma tela ou um aplicativo: a prévia em 16:10, o nome e o detalhe em mono.
fn source_card(item: &Source) -> (gtk::Button, gtk::Picture) {
    let card = gtk::Button::new();
    let inside = column(0);
    let picture = gtk::Picture::new();
    let frame = gtk::Overlay::new();
    let nothing = crate::components::mono("sem prévia");
    let text = column(2);
    let name = gtk::Label::builder().label(&item.label).xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).build();
    let detail = crate::components::mono(&item.detail);

    picture.set_keep_aspect_ratio(true);
    picture.set_can_shrink(true);
    picture.set_size_request(CARD_WIDTH, CARD_WIDTH * 10 / 16);
    frame.add_css_class("source-preview");
    frame.set_child(Some(&nothing));
    frame.add_overlay(&picture);
    name.add_css_class("strong");
    detail.set_xalign(0.0);
    detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
    text.append(&name);
    text.append(&detail);
    crate::components::pad(&text, 10);
    inside.append(&frame);
    inside.append(&text);
    card.set_child(Some(&inside));
    card.add_css_class("source-card");
    clickable(&card);

    (card, picture)
}

/// O rótulo mono em cima do `<select>`, como o React.
fn labelled_choice(label: &str, choice: &gtk::DropDown) -> gtk::Box {
    let stack = column(6);

    stack.set_valign(gtk::Align::End);
    stack.append(&label_mono(label));
    stack.append(choice);

    stack
}

/// `2160` é "4K (2160p)"; o resto ganha o "p" — a regra do React.
fn quality_label(value: &str) -> String {
    if value == "2160" { "4K (2160p)".to_owned() } else { format!("{value}p") }
}
