//! Entrar e criar conta: o cartão no meio da janela, com o Google e o e-mail (ou apelido) e
//! senha. Do lado, onde o Discord põe o QR code, entrar com o nome e uma senha: a conta nasce
//! na hora, sem e-mail. Aqui também mora o miolo da sala por código, que o hub abre num modal.

use egui::{CornerRadius, Frame, Key, Ui};

use super::{theme, widgets};
use crate::app::{Action, Unkvoid};

pub fn show(app: &mut Unkvoid, ui: &mut Ui) {
    // Os dois lados e o vão entre eles. A janela nunca fica mais estreita que o cartão inteiro
    // (`MIN_SIZE`), então não há versão empilhada.
    const LOGIN: f32 = 420.0;
    const ROOM: f32 = 280.0;
    const GAP: f32 = 64.0;
    const PADDING: i8 = 32;

    egui::CentralPanel::no_frame()
        .frame(Frame::new().fill(theme::RAIL))
        .show(ui, |ui| {
            // Sem o `auto_shrink` a área encolhe até a largura do cartão e fica colada à esquerda.
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let width = LOGIN + GAP + ROOM + 2.0 * ui.spacing().item_spacing.x + 2.0 * f32::from(PADDING);

                ui.add_space((ui.available_height() / 2.0 - 270.0).max(24.0));
                ui.horizontal(|ui| {
                    ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));

                    Frame::new()
                        .fill(theme::MAIN)
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(egui::Margin::same(PADDING))
                        .shadow(egui::epaint::Shadow {
                            offset: [0, 12],
                            blur: 40,
                            spread: 0,
                            color: egui::Color32::from_black_alpha(120),
                        })
                        .show(ui, |ui| {
                            ui.horizontal_top(|ui| {
                                let login = ui
                                    .vertical(|ui| {
                                        ui.set_width(LOGIN);
                                        card(app, ui);
                                    })
                                    .response
                                    .rect;

                                ui.add_space(GAP);

                                let room = ui
                                    .vertical(|ui| {
                                        ui.set_width(ROOM);
                                        start(app, ui);
                                    })
                                    .response
                                    .rect;

                                // A linha do meio na altura do cartão: o `separator` do egui
                                // desceria até o pé da janela.
                                ui.painter().vline(
                                    (login.right() + room.left()) / 2.0,
                                    egui::Rangef::new(login.top(), login.bottom().max(room.bottom())),
                                    egui::Stroke::new(1.0, theme::DIVIDER),
                                );
                            });
                        });
                });
                ui.add_space(24.0);
            });
        });
}

/// Entrar com o nome e uma senha: a conta nasce agora, com o apelido tirado do nome, e entra
/// de novo por ele e a senha. O e-mail fica para as configurações, se a pessoa quiser.
fn start(app: &mut Unkvoid, ui: &mut Ui) {
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new("Entrar com o nome").font(theme::bold(24.0)));
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("Sua conta nasce agora, sem e-mail. Depois você entra com o apelido e a senha.")
                .font(theme::regular(13.0))
                .color(theme::MUTED),
        );
    });
    ui.add_space(20.0);

    widgets::field_label(ui, "Seu nome", None);
    ui.add_space(6.0);

    let name = widgets::text_field(ui, &mut app.login.guest_name, "", false);

    ui.add_space(14.0);
    widgets::field_label(ui, "Senha", None);
    ui.add_space(6.0);

    let password = widgets::text_field(ui, &mut app.login.start_password, "pelo menos 8 caracteres", true);

    if name.changed() || password.changed() {
        app.login.start_error = None;
    }

    // Enter no nome vai para a senha; Enter na senha envia.
    if name.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
        password.request_focus();
    }

    let submitted = password.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));

    if let Some(error) = &app.login.start_error {
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(error)
                .font(theme::regular(13.0))
                .color(theme::DANGER),
        );
    }

    ui.add_space(20.0);

    let ready = !app.login.starting
        && !app.login.guest_name.trim().is_empty()
        && !app.login.start_password.is_empty();
    let label = if app.login.starting { "Um instante…" } else { "Começar" };

    if widgets::primary_button(ui, label, ready, ui.available_width()) || (submitted && ready) {
        app.actions.push(Action::Start);
    }
}

/// A sala por código: o nome, o código (vazio cria uma sala nova) e as últimas salas. O
/// trilho do hub abre num modal.
pub(super) fn code_room(app: &mut Unkvoid, ui: &mut Ui) {
    // O início não tira ninguém da sala: ela continua no ar, e o caminho de volta fica no topo.
    // A voz de um servidor não é sala por código, e não aparece aqui.
    if let Some(seat) = app.call.seat.clone().filter(|seat| seat.guest.is_some()) {
        Frame::new()
            .fill(theme::ELEVATED)
            .stroke(egui::Stroke::new(1.0, theme::DIVIDER))
            .corner_radius(theme::RADIUS)
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    egui::RichText::new("Conectado à sala")
                        .font(theme::bold(12.0))
                        .color(theme::SUCCESS),
                );
                ui.label(
                    egui::RichText::new(&seat.channel)
                        .font(theme::bold(16.0))
                        .color(theme::BRIGHT),
                );
                ui.add_space(10.0);

                if widgets::primary_button(ui, "Voltar para a sala", true, ui.available_width()) {
                    app.actions.push(Action::BackToRoom);
                }

                ui.add_space(6.0);

                if widgets::danger_button(ui, "Desconectar", true) {
                    app.actions.push(Action::LeaveVoice);
                }
            });
        ui.add_space(24.0);
    }

    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new("Sala por código").font(theme::bold(20.0)));
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("Crie uma sala e mande o código, ou entre com o código de alguém.")
                .font(theme::regular(13.0))
                .color(theme::MUTED),
        );
    });
    ui.add_space(20.0);

    widgets::field_label(ui, "Seu nome", None);
    ui.add_space(6.0);

    let name = widgets::text_field(ui, &mut app.login.guest_name, "", false);

    if name.changed() {
        app.login.room_error = None;
    }

    ui.add_space(14.0);
    widgets::field_label(ui, "Código da sala", None);
    ui.add_space(6.0);

    let code = widgets::text_field(ui, &mut app.login.room_code, "vazio cria uma sala nova", false);

    if code.changed() {
        app.login.room_error = None;
    }

    if name.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
        code.request_focus();
    }

    let submitted = code.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));

    if let Some(error) = &app.login.room_error {
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(error)
                .font(theme::regular(13.0))
                .color(theme::DANGER),
        );
    }

    ui.add_space(20.0);

    let ready = !app.login.guest_name.trim().is_empty();
    let label = if app.login.room_code.trim().is_empty() {
        "Criar sala"
    } else {
        "Entrar na sala"
    };

    if widgets::primary_button(ui, label, ready, ui.available_width()) || (submitted && ready) {
        app.actions.push(Action::EnterRoom(None));
    }

    if app.login.recent_rooms.is_empty() {
        return;
    }

    ui.add_space(18.0);
    ui.label(
        egui::RichText::new("ÚLTIMAS SALAS")
            .font(theme::bold(12.0))
            .color(theme::DIM),
    );
    ui.add_space(6.0);

    // Os códigos lado a lado, quebrando a linha quando não cabem.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);

        for recent in app.login.recent_rooms.clone() {
            let chip = egui::Button::new(egui::RichText::new(&recent).font(theme::bold(13.0)))
                .fill(theme::ELEVATED)
                .stroke(egui::Stroke::new(1.0, theme::DIVIDER))
                .corner_radius(theme::RADIUS)
                .min_size(egui::vec2(0.0, 32.0));

            if ui.add(chip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                app.actions.push(Action::EnterRoom(Some(recent)));
            }
        }
    });
}

fn card(app: &mut Unkvoid, ui: &mut Ui) {
    let register = app.login.register;

    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new(if register {
                "Criar uma conta"
            } else {
                "Boas-vindas de volta!"
            })
            .font(theme::bold(24.0)),
        );
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(if register {
                "Uma conta para os seus servidores, canais e conversas."
            } else {
                "Que bom te ver de novo!"
            })
            .color(theme::MUTED),
        );
    });
    ui.add_space(20.0);

    // Como o cartão antigo: no cadastro o botão já diz que cria a conta.
    let google = if register { "Criar conta com o Google" } else { "Entrar com o Google" };

    if widgets::secondary_button_wide(ui, google) {
        app.actions.push(Action::GoogleSignIn);
    }

    ui.add_space(14.0);
    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new("ou com e-mail")
                .font(theme::regular(12.0))
                .color(theme::DIM),
        )
    });
    ui.add_space(10.0);

    // Entrar aceita o apelido no lugar do e-mail; criar conta, não.
    widgets::field_label(ui, if register { "E-mail" } else { "E-mail ou apelido" }, app.login.email_error.as_deref());
    ui.add_space(6.0);

    let email = widgets::text_field(ui, &mut app.login.email, "", false);

    if email.changed() {
        app.login.email_error = None;
    }

    ui.add_space(14.0);
    widgets::field_label(ui, "Senha", app.login.password_error.as_deref());
    ui.add_space(6.0);

    // O mínimo do servidor (RegisterRequest) dito antes de errar, e não depois.
    let hint = if register { "pelo menos 8 caracteres" } else { "" };
    let password = widgets::text_field(ui, &mut app.login.password, hint, true);

    if password.changed() {
        app.login.password_error = None;
    }

    // Enter no e-mail vai para a senha; Enter na senha envia.
    if email.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
        password.request_focus();
    }

    let submitted = password.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));

    if let Some(error) = &app.login.error {
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(error)
                .font(theme::regular(13.0))
                .color(theme::DANGER),
        );
    }

    ui.add_space(20.0);

    let ready =
        !app.login.busy && !app.login.email.trim().is_empty() && !app.login.password.is_empty();
    let label = match (app.login.busy, register) {
        (true, _) => "Um instante…",
        (false, true) => "Criar conta",
        (false, false) => "Entrar",
    };

    if widgets::primary_button(ui, label, ready, ui.available_width()) || (submitted && ready) {
        app.actions.push(Action::SignIn);
    }

    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(if register {
                "Já tem uma conta?"
            } else {
                "Precisando de uma conta?"
            })
            .font(theme::regular(13.0))
            .color(theme::DIM),
        );

        let switch = ui.add(
            egui::Label::new(
                egui::RichText::new(if register { "Entrar" } else { "Registre-se" })
                    .font(theme::regular(13.0))
                    .color(theme::ACCENT_TEXT),
            )
            .sense(egui::Sense::click()),
        );

        if switch
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            app.login.register = !register;
            app.login.error = None;
            app.login.email_error = None;
            app.login.password_error = None;
        }
    });
}
