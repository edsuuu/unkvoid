//! As configurações do servidor, no molde das do Discord: Visão geral, Cargos, Membros,
//! Convites, Banimentos e Registro de auditoria, e o "Excluir servidor" do dono no fim do menu.
//! O que cada pessoa pode mexer quem diz é a árvore (`can`, `role_rows`, `member_actions`); o
//! Laravel confere tudo de novo.

use egui::{Align, Align2, Color32, CornerRadius, Frame, Layout, Rect, RichText, Sense, Ui, pos2, vec2};
use egui_phosphor::regular as icons;
use serde_json::{Value, json};

use core_app::members::display_name;
use core_app::models::{Role, ServerTree};
use core_app::permissions::{
    ADMINISTRATOR, BAN_MEMBERS, CONNECT, CREATE_INVITE, DEAFEN_MEMBERS, KICK_MEMBERS,
    MANAGE_CHANNELS, MANAGE_EXPRESSIONS, MANAGE_MESSAGES, MANAGE_ROLES, MANAGE_SERVER,
    MOVE_MEMBERS, MUTE_MEMBERS, SEND_MESSAGES, SPEAK, STREAM, VIDEO, VIEW_AUDIT_LOG, VIEW_CHANNEL,
};

use super::{settings, theme, voice_settings, widgets};
use crate::app::{Action, Confirm, Modal, RoleDraft, RoleTab, ServerPage, ServerSettings, Unkvoid};
use crate::backend::Refresh;

/// Uma permissão como a tela a mostra: o bit, o nome e o que ela deixa fazer.
pub(super) type Permission = (i64, &'static str, &'static str);

/// As permissões nos grupos e na ordem do Discord. A página de permissões do canal usa as mesmas.
pub(super) const PERMISSIONS: [(&str, &[Permission]); 5] = [
    (
        "Permissões gerais do servidor",
        &[
            (
                VIEW_CHANNEL,
                "Ver canais",
                "Deixa ver os canais, menos os que forem fechados para o cargo.",
            ),
            (
                MANAGE_CHANNELS,
                "Gerenciar canais",
                "Deixa criar, editar e excluir canais e categorias.",
            ),
            (
                MANAGE_ROLES,
                "Gerenciar cargos",
                "Deixa criar cargos abaixo do seu e editar ou dar os que estão abaixo dele.",
            ),
            (
                VIEW_AUDIT_LOG,
                "Ver registro de auditoria",
                "Deixa ver quem mudou o quê no servidor.",
            ),
            (
                MANAGE_SERVER,
                "Gerenciar servidor",
                "Deixa mudar o nome e o ícone do servidor e o apelido dos outros.",
            ),
            (
                MANAGE_EXPRESSIONS,
                "Gerenciar expressões",
                "Deixa enviar e apagar os sons do soundboard do servidor.",
            ),
        ],
    ),
    (
        "Permissões de membros",
        &[
            (
                CREATE_INVITE,
                "Criar convite",
                "Deixa ver e gerar o convite do servidor.",
            ),
            (
                KICK_MEMBERS,
                "Expulsar membros",
                "Deixa tirar alguém do servidor. A pessoa volta com outro convite.",
            ),
            (
                BAN_MEMBERS,
                "Banir membros",
                "Deixa banir alguém do servidor de vez, e revogar o banimento.",
            ),
        ],
    ),
    (
        "Permissões de canais de texto",
        &[
            (
                SEND_MESSAGES,
                "Enviar mensagens",
                "Deixa mandar mensagens nos canais de texto e no chat da voz.",
            ),
            (
                MANAGE_MESSAGES,
                "Gerenciar mensagens",
                "Deixa apagar as mensagens dos outros.",
            ),
        ],
    ),
    (
        "Permissões de canais de voz",
        &[
            (
                CONNECT,
                "Conectar",
                "Deixa entrar nos canais de voz e ouvir quem está lá.",
            ),
            (SPEAK, "Falar", "Deixa falar nos canais de voz."),
            (VIDEO, "Câmera", "Deixa ligar a câmera nos canais de voz."),
            (
                STREAM,
                "Compartilhar tela",
                "Deixa transmitir a tela nos canais de voz.",
            ),
            (
                MUTE_MEMBERS,
                "Silenciar membros",
                "Deixa silenciar alguém na voz para todo mundo.",
            ),
            (
                DEAFEN_MEMBERS,
                "Ensurdecer membros",
                "Deixa ensurdecer alguém na voz para todo mundo.",
            ),
            (
                MOVE_MEMBERS,
                "Mover membros",
                "Deixa mover e desconectar pessoas dos canais de voz.",
            ),
        ],
    ),
    (
        "Permissões avançadas",
        &[(
            ADMINISTRATOR,
            "Administrador",
            "Dá todas as permissões e passa por cima das de cada canal. Cuidado: é a mais perigosa.",
        )],
    ),
];

/// As cores que o editor de cargo oferece, além da cor padrão do texto.
const ROLE_COLORS: [&str; 20] = [
    "#1abc9c", "#2ecc71", "#3498db", "#9b59b6", "#e91e63", "#f1c40f", "#e67e22", "#e74c3c",
    "#95a5a6", "#607d8b", "#11806a", "#1f8b4c", "#206694", "#71368a", "#ad1457", "#c27c0e",
    "#a84300", "#992d22", "#979c9f", "#546e7a",
];

/// Quem pode abrir as configurações: o Discord só mostra o item a quem administra alguma coisa.
pub fn can_open(tree: &ServerTree) -> bool {
    [
        MANAGE_SERVER,
        MANAGE_ROLES,
        KICK_MEMBERS,
        BAN_MEMBERS,
        VIEW_AUDIT_LOG,
        CREATE_INVITE,
        MANAGE_EXPRESSIONS,
    ]
    .iter()
    .any(|flag| tree.can(*flag))
}

fn server_nav_header(ui: &mut Ui, label: &str) {
    ui.add_space(6.0);
    ui.label(
        RichText::new(label)
            .font(theme::bold(12.0))
            .color(theme::MUTED),
    );
    ui.add_space(2.0);
}

fn server_nav_item(ui: &mut Ui, label: &str, selected: bool, danger: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width().max(0.0), 34.0),
        Sense::click(),
    );
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(5),
            if selected { theme::SELECTED } else { theme::HOVER },
        );
    }
    ui.painter().text(
        rect.left_center() + vec2(10.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        theme::regular(14.0),
        if danger { theme::DANGER } else { theme::TEXT },
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn compact_button(ui: &mut Ui, label: &str, enabled: bool, danger: bool) -> bool {
    let fill = if danger { theme::DANGER } else { theme::ACCENT };
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(label).font(theme::regular(14.0)))
            .fill(fill)
            .corner_radius(CornerRadius::same(6))
            .min_size(vec2(0.0, 36.0)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .clicked()
}

fn compact_button_with_width(
    ui: &mut Ui,
    label: &str,
    enabled: bool,
    danger: bool,
    width: f32,
) -> bool {
    let fill = if danger { theme::DANGER } else { theme::ACCENT };
    ui.add_enabled(
        enabled,
        egui::Button::new((egui::Atom::grow(), RichText::new(label).font(theme::regular(14.0)), egui::Atom::grow()))
            .fill(fill)
            .corner_radius(CornerRadius::same(6))
            .min_size(vec2(width.max(0.0), 36.0)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .clicked()
}

fn compact_menu_item(ui: &mut Ui, label: &str, danger: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        vec2(widgets::menu_width(ui), 34.0),
        Sense::click(),
    );
    if response.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(4), theme::HOVER);
    }
    ui.painter().text(
        rect.left_center() + vec2(10.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        theme::regular(14.0),
        if danger { theme::DANGER } else { theme::TEXT },
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn compact_icon_menu_item(ui: &mut Ui, icon: &str, label: &str) -> bool {
    // Na largura dos menus: com a largura toda o popup ia a 600 px.
    let (rect, response) = ui.allocate_exact_size(
        vec2(widgets::menu_width(ui), 34.0),
        Sense::click(),
    );
    if response.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(4), theme::HOVER);
    }
    crate::ui::icons::text(ui.painter(), rect.left_center() + vec2(10.0, 0.0), Align2::LEFT_CENTER, icon, theme::icon(16.0), theme::MUTED);
    ui.painter().text(
        rect.left_center() + vec2(34.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        theme::regular(14.0),
        theme::TEXT,
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn hand_icon_button(ui: &mut Ui, icon: &str, tooltip: &str, tint: Color32) -> egui::Response {
    widgets::icon_button(ui, icon, tooltip, tint).on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn show(app: &mut Unkvoid, ui: &mut Ui, mut state: ServerSettings) {
    let Some(tree) = app.tree.clone().filter(|tree| tree.id == state.server) else {
        app.actions.push(Action::CloseModal);

        return;
    };
    if let Some(after) = state.new_role_after
        && let Some(role) = tree.roles.iter().find(|role| role.id > after)
    {
        state.page = ServerPage::Roles;
        state.role = Some(draft_of(role, RoleTab::Display));
        state.new_role_after = None;
    }
    let shown = state.clone();

    settings::window(
        app,
        ui,
        "server-settings",
        None,
        |app, ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            navigation(app, ui, &tree, &shown)
        },
        |app, ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            match state.page {
            ServerPage::Overview => overview(app, ui, &tree, &mut state),
            ServerPage::Soundboard => soundboard(app, ui, &tree),
            ServerPage::Roles => roles(app, ui, &tree, &mut state),
            ServerPage::Members => members(app, ui, &tree, &mut state),
            ServerPage::Invites => invites(app, ui, &tree),
            ServerPage::Bans => bans(app, ui, &tree),
            ServerPage::Audit => audit(app, ui),
            }
        },
    );

    // O que foi digitado volta para o estado antes das ações do quadro, que podem trocar o modal.
    if matches!(app.modal, Some(Modal::ServerSettings(_))) {
        app.modal = Some(Modal::ServerSettings(state));
    }
}

fn navigation(app: &mut Unkvoid, ui: &mut Ui, tree: &ServerTree, state: &ServerSettings) {
    if state.page == ServerPage::Roles && state.role.is_some() {
        if link(ui, "← VOLTAR", theme::MUTED) {
            let mut next = state.clone();
            next.role = None;
            app.actions.push(Action::Show(Modal::ServerSettings(next)));
        }
        ui.add_space(12.0);
        // O título na margem dos outros e o "+" na ponta, na mesma linha: com o cabeçalho
        // comum dentro do `horizontal`, os espaços dele viravam recuo e o "+" ficava abaixo.
        ui.allocate_ui_with_layout(vec2(ui.available_width(), 28.0), Layout::left_to_right(Align::Center), |ui| {
            ui.label(RichText::new("CARGOS").font(theme::bold(12.0)).color(theme::MUTED));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if hand_icon_button(ui, icons::PLUS, "Criar cargo", theme::MUTED).clicked() {
                    create_role(app, tree, state);
                }
            });
        });
        // Na ordem da lista da página (a de `role_rows`): pela árvore crua o cargo novo vinha
        // embaixo aqui e em cima lá.
        for row in tree.role_rows() {
            let Some(role) = tree.roles.iter().find(|role| role.id == row.id && !role.is_everyone) else {
                continue;
            };
            let selected = state.role.as_ref().is_some_and(|draft| draft.id == role.id);
            if server_nav_item(ui, &role.name, selected, false) && !selected {
                let mut next = state.clone();
                next.role = Some(draft_of(role, RoleTab::Display));
                app.actions.push(Action::Show(Modal::ServerSettings(next)));
            }
        }
        return;
    }
    let go = |app: &mut Unkvoid, ui: &mut Ui, page: ServerPage, _icon: &str, label: &str| {
        if server_nav_item(ui, label, state.page == page, false) && state.page != page {
            app.actions
                .push(Action::Show(Modal::ServerSettings(ServerSettings {
                    page,
                    role: None,
                    search: String::new(),
                    ..state.clone()
                })));
        }
    };

    server_nav_header(ui, &tree.name.to_uppercase());
    go(app, ui, ServerPage::Overview, icons::INFO, "Perfil do servidor");

    if tree.can(MANAGE_EXPRESSIONS) {
        ui.add_space(8.0);
        server_nav_header(ui, "EXPRESSÕES");
        go(app, ui, ServerPage::Soundboard, icons::SPEAKER_HIGH, "Painel de efeitos sonoros");
    }

    ui.add_space(8.0);
    server_nav_header(ui, "PESSOAS");
    go(app, ui, ServerPage::Members, icons::USERS, "Membros");

    if tree.can(MANAGE_ROLES) {
        go(app, ui, ServerPage::Roles, icons::SHIELD_CHEVRON, "Cargos");
    }

    if tree.can(MANAGE_SERVER) {
        go(app, ui, ServerPage::Invites, icons::LINK, "Convites");
    }

    if tree.can(VIEW_AUDIT_LOG) || tree.can(BAN_MEMBERS) {
        ui.add_space(8.0);
        server_nav_header(ui, "MODERAÇÃO");

        if tree.can(VIEW_AUDIT_LOG) {
            go(
                app,
                ui,
                ServerPage::Audit,
                icons::LIST_MAGNIFYING_GLASS,
                "Registro de auditoria",
            );
        }

        if tree.can(BAN_MEMBERS) {
            go(app, ui, ServerPage::Bans, icons::PROHIBIT, "Banimentos");
        }
    }

    if tree.owner_id == tree.me.user_id {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), 34.0), Sense::click());
        if response.hovered() { ui.painter().rect_filled(rect, theme::RADIUS, theme::HOVER); }
        ui.painter().text(rect.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, "Excluir servidor", theme::regular(14.0), theme::DANGER);
        crate::ui::icons::text(ui.painter(), rect.right_center() - vec2(12.0, 0.0), Align2::RIGHT_CENTER, icons::TRASH, theme::icon(18.0), theme::DANGER);
        if response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            app.actions.push(Action::Ask(Confirm::DeleteServer {
                server: tree.id,
                name: tree.name.clone(),
                typed: String::new(),
            }));
        }
    }
}

fn overview(app: &mut Unkvoid, ui: &mut Ui, tree: &ServerTree, state: &mut ServerSettings) {
    let manage = tree.can(MANAGE_SERVER);
    let server = json!({ "server": tree.id });
    let upload = Action::UploadImage {
        route: "uploadServerIcon",
        params: server.clone(),
        field: "icon",
        refresh: Refresh::Server(tree.id),
    };

    settings::heading(ui, "Perfil do servidor");
    ui.add_space(24.0);
    let available_width = ui.available_width().max(0.0);
    // A prévia só entra com folga: na janela estreita ela empurrava o formulário, e o modal
    // inteiro passava das bordas da janela.
    let preview_width = if available_width >= 640.0 { 300.0_f32.min(available_width * 0.38) } else { 0.0 };
    let form_width = if preview_width > 0.0 { available_width - preview_width - 24.0 } else { available_width }.clamp(0.0, 560.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.allocate_ui_with_layout(
            vec2(form_width, 320.0),
            Layout::top_down(Align::Min),
            |ui| {
                ui.label(RichText::new("Ícone").font(theme::bold(16.0)));
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Recomendamos uma imagem quadrada de pelo menos 512×512: jpeg, png ou webp, até 2 MB.")
                        .font(theme::regular(14.0))
                        .color(theme::DESCRIPTION),
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let (rect, response) = ui.allocate_exact_size(
                        vec2(64.0, 64.0),
                        if manage { Sense::click() } else { Sense::hover() },
                    );
                    widgets::paint_avatar(ui, rect, &tree.name, tree.icon_url.as_deref());
                    if manage && response.hovered() {
                        ui.painter().circle_filled(
                            rect.center(),
                            32.0,
                            Color32::from_black_alpha(150),
                        );
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            "MUDAR",
                            theme::bold(10.0),
                            Color32::WHITE,
                        );
                    }
                    if response
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        app.actions.push(upload.clone());
                    }
                    if manage && compact_button(ui, "Altere o ícone do servidor", true, false) {
                        app.actions.push(upload.clone());
                    }
                    if manage && tree.icon_url.is_some() {
                        let response = ui.add(
                            egui::Button::new(RichText::new("Remover o Ícone").font(theme::regular(14.0)).color(theme::DANGER))
                                .fill(theme::BUTTON)
                                .corner_radius(CornerRadius::same(6))
                                .min_size(vec2(0.0, 36.0)),
                        ).on_hover_cursor(egui::CursorIcon::PointingHand);
                        if response.clicked() {
                            app.actions.push(Action::request(
                                "deleteServerIcon",
                                server.clone(),
                                Value::Null,
                                Refresh::Server(tree.id),
                            ));
                        }
                    }
                });

                ui.add_space(32.0);
                ui.separator();
                ui.add_space(32.0);
                ui.label(RichText::new("Nome do servidor").font(theme::bold(16.0)));
                ui.add_space(8.0);
                if manage {
                    widgets::text_field(ui, &mut state.name, "Nome do servidor", false);
                } else {
                    ui.label(RichText::new(&tree.name).font(theme::regular(15.0)));
                }
            },
        );
        if preview_width > 0.0 {
            ui.add_space(24.0);
            ui.allocate_ui_with_layout(
                vec2(preview_width, 320.0),
                Layout::top_down(Align::Min),
                |ui| server_preview(ui, tree),
            );
        }
    });

    let name = state.name.trim().to_owned();

    if name != tree.name && !name.is_empty() {
        ui.add_space(24.0);

        match settings::unsaved(ui) {
            Some(true) => app.actions.push(Action::request(
                "updateServer",
                server,
                json!({ "name": name }),
                Refresh::Server(tree.id),
            )),
            Some(false) => state.name.clone_from(&tree.name),
            None => {}
        }
    }
}

fn server_preview(ui: &mut Ui, tree: &ServerTree) {
    Frame::new()
        .fill(theme::ELEVATED)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::same(0))
        .show(ui, |ui| {
            let width = ui.available_width().max(0.0);
            let (banner, _) = ui.allocate_exact_size(vec2(width, 84.0), Sense::hover());
            ui.painter().rect_filled(
                banner,
                CornerRadius {
                    nw: 8,
                    ne: 8,
                    sw: 0,
                    se: 0,
                },
                widgets::color_for(&tree.name),
            );
            let avatar = Rect::from_min_size(
                pos2(banner.left() + 16.0, banner.bottom() - 32.0),
                vec2(64.0, 64.0),
            );
            // O aro da cor do cartão: a faixa e o ícone sem foto saem da mesma cor do nome.
            ui.painter()
                .circle_filled(avatar.center(), avatar.width() / 2.0 + 5.0, theme::ELEVATED);
            widgets::paint_avatar(ui, avatar, &tree.name, tree.icon_url.as_deref());
            ui.add_space(40.0);
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(&tree.name).font(theme::bold(16.0)));
                    ui.add_space(4.0);
                    let member_label = if tree.members.len() == 1 { "membro" } else { "membros" };
                    ui.label(
                        RichText::new(format!("{} {member_label}", tree.members.len()))
                            .font(theme::regular(13.0))
                            .color(theme::DESCRIPTION),
                    );
                });
            });
            ui.add_space(16.0);
        });
}

fn roles(app: &mut Unkvoid, ui: &mut Ui, tree: &ServerTree, state: &mut ServerSettings) {
    if state.role.is_some() {
        return role_editor(app, ui, tree, state);
    }

    settings::heading(ui, "Cargos");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let button_width = 112.0_f32.min(ui.available_width().max(0.0));
        let search_width = (ui.available_width() - button_width - 8.0).max(0.0);
        ui.allocate_ui_with_layout(
            vec2(search_width, 40.0),
            Layout::top_down(Align::Min),
            |ui| {
                widgets::text_field(ui, &mut state.search, "Buscar cargos", false);
            },
        );
        if compact_button_with_width(ui, "Criar cargo", true, false, button_width) {
            create_role(app, tree, state);
        }
    });
    ui.add_space(12.0);
    ui.label(
        RichText::new("Os membros usam a cor do cargo mais alto que eles possuem nesta lista.")
            .color(theme::MUTED),
    );
    ui.add_space(16.0);

    if let Some(everyone) = tree.roles.iter().find(|role| role.is_everyone) {
        let response = Frame::new()
            .fill(theme::ELEVATED)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(16, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    crate::ui::icons::label(ui, icons::USERS_THREE, theme::icon(18.0), theme::MUTED);
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Permissões padrão").font(theme::bold(15.0)));
                        ui.label(
                            RichText::new("@everyone · valem para todos os membros do servidor")
                                .font(theme::regular(13.0))
                                .color(theme::MUTED),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        crate::ui::icons::label(ui, icons::CARET_RIGHT, theme::icon(18.0), theme::MUTED);
                    });
                });
            })
            .response
            .interact(Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);

        if response.clicked() {
            state.role = Some(draft_of(everyone, RoleTab::Permissions));
        }
    }

    ui.add_space(20.0);

    let rows = tree.role_rows();
    let listed: Vec<_> = rows
        .iter()
        .filter(|row| {
            tree.roles
                .iter()
                .any(|role| role.id == row.id && !role.is_everyone)
        })
        .filter(|row| state.search.is_empty() || tree.roles.iter().any(|role| role.id == row.id && role.name.to_lowercase().contains(&state.search.to_lowercase())))
        .collect();

    let (header, _) = ui.allocate_exact_size(
        vec2(ui.available_width().max(0.0), 28.0),
        Sense::hover(),
    );
    ui.painter().text(
        header.left_center(),
        Align2::LEFT_CENTER,
        format!("CARGOS — {}", listed.len()),
        theme::bold(12.0),
        theme::MUTED,
    );
    ui.painter().text(
        pos2(header.left() + header.width() * 0.55, header.center().y),
        Align2::LEFT_CENTER,
        "MEMBROS",
        theme::bold(11.0),
        theme::MUTED,
    );
    ui.separator();

    for row in listed {
        let Some(role) = tree.roles.iter().find(|role| role.id == row.id) else {
            continue;
        };
        let count = tree
            .members
            .iter()
            .filter(|member| member.role_ids.contains(&role.id))
            .count();
        let (rect, response) = ui
            .allocate_exact_size(vec2(ui.available_width().max(0.0), 42.0), Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

        if response.hovered() {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(6), theme::HOVER);
        }

        let color = role
            .color
            .as_deref()
            .and_then(theme::color_of)
            .unwrap_or(theme::MUTED);

        ui.painter()
            .circle_filled(pos2(rect.left() + 18.0, rect.center().y), 7.0, color);
        ui.painter().text(
            pos2(rect.left() + 36.0, rect.center().y),
            Align2::LEFT_CENTER,
            &role.name,
            theme::regular(14.0),
            theme::TEXT,
        );
        ui.painter().text(
            pos2(rect.left() + rect.width() * 0.55, rect.center().y),
            Align2::LEFT_CENTER,
            format!("{}  {count}", icons::USER),
            theme::regular(14.0),
            theme::MUTED,
        );

        let mut buttons = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(vec2(8.0, 7.0)))
                .layout(Layout::right_to_left(Align::Center)),
        );

        if hand_icon_button(&mut buttons, icons::PENCIL_SIMPLE, "Editar cargo", theme::MUTED).clicked() {
            state.role = Some(draft_of(role, RoleTab::Display));
        }

        if row.editable
            && hand_icon_button(&mut buttons, icons::TRASH, "Excluir cargo", theme::MUTED)
                .clicked()
        {
            app.actions.push(Action::Ask(Confirm::DeleteRole {
                server: tree.id,
                role: role.id,
                name: role.name.clone(),
            }));
        }

        buttons.menu_button(icons::DOTS_THREE, |ui| {
            ui.set_min_width(180.0);
            if compact_menu_item(ui, "Editar", false) {
                state.role = Some(draft_of(role, RoleTab::Display));
                ui.close();
            }
            if row.editable && compact_menu_item(ui, "Excluir cargo", true) {
                app.actions.push(Action::Ask(Confirm::DeleteRole { server: tree.id, role: role.id, name: role.name.clone() }));
                ui.close();
            }
            if let Some(position) = row.up && compact_menu_item(ui, "Mover para cima", false) {
                app.actions.push(move_role(tree.id, role.id, position));
                ui.close();
            }
            if let Some(position) = row.down && compact_menu_item(ui, "Mover para baixo", false) {
                app.actions.push(move_role(tree.id, role.id, position));
                ui.close();
            }
        });

        if let Some(position) = row.down
            && hand_icon_button(&mut buttons, icons::ARROW_DOWN, "Descer", theme::MUTED)
                .clicked()
        {
            app.actions.push(move_role(tree.id, role.id, position));
        }

        if let Some(position) = row.up
            && hand_icon_button(&mut buttons, icons::ARROW_UP, "Subir", theme::MUTED).clicked()
        {
            app.actions.push(move_role(tree.id, role.id, position));
        }

        if response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            state.role = Some(draft_of(role, RoleTab::Display));
        }
    }
}

fn move_role(server: i64, role: i64, position: i64) -> Action {
    Action::request(
        "updateRole",
        json!({ "role": role }),
        json!({ "position": position }),
        Refresh::Tree(server),
    )
}

/// Cria o cargo e marca o maior id de agora: o cargo que vier acima dele abre no editor.
fn create_role(app: &mut Unkvoid, tree: &ServerTree, state: &ServerSettings) {
    app.actions.push(Action::request(
        "createRole",
        json!({ "server": tree.id }),
        json!({ "name": "novo cargo", "permissions": 0 }),
        Refresh::Tree(tree.id),
    ));
    let after = tree.roles.iter().map(|role| role.id).max().unwrap_or(0);
    app.actions.push(Action::Show(Modal::ServerSettings(ServerSettings {
        new_role_after: Some(after),
        ..state.clone()
    })));
}

fn draft_of(role: &Role, tab: RoleTab) -> RoleDraft {
    RoleDraft {
        id: role.id,
        tab,
        name: role.name.clone(),
        color: role.color.clone(),
        permissions: role.permissions,
        hoist: role.hoist,
        mentionable: role.mentionable,
    }
}

fn role_editor(app: &mut Unkvoid, ui: &mut Ui, tree: &ServerTree, state: &mut ServerSettings) {
    let Some(role) = state
        .role
        .as_ref()
        .and_then(|draft| tree.roles.iter().find(|role| role.id == draft.id))
        .cloned()
    else {
        state.role = None;

        return;
    };
    let row = tree.role_rows().into_iter().find(|row| row.id == role.id);
    let editable = row.as_ref().is_some_and(|row| row.editable) && tree.can(MANAGE_ROLES);
    let Some(draft) = state.role.as_mut() else {
        return;
    };

    // O "← VOLTAR" fica só no menu da esquerda, como no Discord: aqui ele saía repetido.
    ui.horizontal(|ui| {
        settings::heading(
            ui,
            &format!(
            "Editar cargo — {}",
            if role.is_everyone {
                "@everyone"
            } else {
                &role.name
            }
            ),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.menu_button(icons::DOTS_THREE, |ui| {
                if row.as_ref().is_some_and(|row| row.editable) && compact_menu_item(ui, "Excluir cargo", true) {
                    app.actions.push(Action::Ask(Confirm::DeleteRole { server: tree.id, role: role.id, name: role.name.clone() }));
                    ui.close();
                }
                if let Some(position) = row.as_ref().and_then(|row| row.up) && compact_menu_item(ui, "Mover para cima", false) {
                    app.actions.push(move_role(tree.id, role.id, position));
                    ui.close();
                }
                if let Some(position) = row.as_ref().and_then(|row| row.down) && compact_menu_item(ui, "Mover para baixo", false) {
                    app.actions.push(move_role(tree.id, role.id, position));
                    ui.close();
                }
            });
        });
    });

    if role.is_everyone {
        draft.tab = RoleTab::Permissions;
    } else {
        let count = tree
            .members
            .iter()
            .filter(|member| member.role_ids.contains(&role.id))
            .count();

        ui.horizontal(|ui| {
            for (tab, label) in [
                (RoleTab::Display, "Exibição".to_owned()),
                (RoleTab::Permissions, "Permissões".to_owned()),
                (RoleTab::Members, format!("Gerenciar membros ({count})")),
            ] {
                if tab_button(ui, &label, draft.tab == tab) {
                    draft.tab = tab;
                }
            }
        });
        ui.separator();
    }

    ui.add_space(16.0);

    match draft.tab {
        RoleTab::Display => {
            widgets::field_label(ui, "Nome do cargo", None);
            ui.add_space(6.0);
            ui.add_enabled_ui(editable, |ui| {
                widgets::text_field(ui, &mut draft.name, "novo cargo", false)
            });
            ui.add_space(20.0);
            widgets::field_label(ui, "Cor do cargo", None);
            ui.label(
                RichText::new("Os membros aparecem com a cor do cargo mais alto que têm.")
                    .font(theme::regular(13.0))
                    .color(theme::MUTED),
            );
            ui.add_space(8.0);
            ui.add_enabled_ui(editable, |ui| color_palette(ui, &mut draft.color));
            ui.add_space(16.0);
            ui.add_enabled_ui(editable, |ui| {
                if voice_settings::switch_row(ui, "Exibir membros do cargo separadamente dos membros conectados", "", draft.hoist) { draft.hoist = !draft.hoist; }
                ui.add_space(12.0);
                if voice_settings::switch_row(ui, "Permitir que qualquer um @mencione este cargo", "", draft.mentionable) { draft.mentionable = !draft.mentionable; }
            });
        }
        RoleTab::Permissions => {
            if role.is_everyone {
                ui.label(
                    RichText::new(
                        "O que todo membro do servidor pode fazer, antes de qualquer cargo.",
                    )
                    .color(theme::MUTED),
                );
                ui.add_space(12.0);
            }

            for (group, permissions) in PERMISSIONS {
                ui.label(
                    RichText::new(group.to_uppercase())
                        .font(theme::bold(12.0))
                        .color(theme::MUTED),
                );
                ui.add_space(8.0);

                for (flag, name, about) in permissions {
                    let on = draft.permissions & flag != 0;

                    ui.add_enabled_ui(editable && tree.can(*flag), |ui| {
                        if voice_settings::switch_row(ui, name, about, on) {
                            draft.permissions ^= flag;
                        }
                    });
                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);
                }

                ui.add_space(14.0);
            }
        }
        RoleTab::Members => role_members(app, ui, tree, &role),
    }

    let changed = draft.name.trim() != role.name
        || draft.color != role.color
        || draft.permissions != role.permissions;
    let changed = changed || draft.hoist != role.hoist || draft.mentionable != role.mentionable;

    if editable && changed && !draft.name.trim().is_empty() {
        ui.add_space(20.0);

        match settings::unsaved(ui) {
            Some(true) => {
                let body = if role.is_everyone {
                    json!({ "permissions": draft.permissions })
                } else {
                    json!({ "name": draft.name.trim(), "color": draft.color, "permissions": draft.permissions, "hoist": draft.hoist, "mentionable": draft.mentionable })
                };

                app.actions.push(Action::request(
                    "updateRole",
                    json!({ "role": role.id }),
                    body,
                    Refresh::Tree(tree.id),
                ));
            }
            Some(false) => *draft = draft_of(&role, draft.tab),
            None => {}
        }
    }
}

/// O "padrão" (sem cor), a cor livre e as vinte cores em duas linhas de dez, como no Discord. A
/// escolhida leva o visto.
fn color_palette(ui: &mut Ui, chosen: &mut Option<String>) {
    let custom_chosen = chosen.as_deref().is_some_and(|hex| !ROLE_COLORS.contains(&hex));
    let mut custom = chosen.as_deref().and_then(theme::color_of).unwrap_or(Color32::from_rgb(88, 101, 242));

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);

        let (rect, response) = ui.allocate_exact_size(vec2(64.0, 52.0), Sense::click());

        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), theme::MUTED);

        if chosen.is_none() {
            crate::ui::icons::text(ui.painter(), rect.center(), Align2::CENTER_CENTER, icons::CHECK, theme::icon(22.0), Color32::WHITE);
        }

        if response.on_hover_text("Padrão").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            *chosen = None;
        }

        // A cor livre no mesmo tamanho do padrão, ao lado dele: solta acima da paleta ela
        // parecia uma faixa perdida.
        let picker = ui
            .scope(|ui| {
                ui.spacing_mut().interact_size = vec2(64.0, 52.0);
                // Opaca: cor de cargo não tem transparência, e o seletor some com o alfa e a mistura.
                egui::widgets::color_picker::color_edit_button_srgba(ui, &mut custom, egui::widgets::color_picker::Alpha::Opaque)
            })
            .inner;
        let icon = if custom_chosen { icons::CHECK } else { icons::EYEDROPPER };

        crate::ui::icons::text(ui.painter(), picker.rect.center(), Align2::CENTER_CENTER, icon, theme::icon(20.0), Color32::WHITE);

        if picker.on_hover_text("Cor personalizada").on_hover_cursor(egui::CursorIcon::PointingHand).changed() {
            let [red, green, blue, _] = custom.to_srgba_unmultiplied();
            *chosen = Some(format!("#{red:02x}{green:02x}{blue:02x}"));
        }

        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 4.0);

            for line in ROLE_COLORS.chunks(10) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;

                    for hex in line {
                        let (rect, response) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
                        let selected = chosen.as_deref() == Some(*hex);

                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(4),
                            theme::color_of(hex).unwrap_or(theme::MUTED),
                        );

                        if selected {
                            crate::ui::icons::text(ui.painter(), rect.center(), Align2::CENTER_CENTER, icons::CHECK, theme::icon(16.0), Color32::WHITE);
                        }

                        if response.on_hover_text(*hex).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            *chosen = Some((*hex).to_owned());
                        }
                    }
                });
            }
        });
    });
}

/// Quem tem o cargo, com o X para tirar, e o menu para dar o cargo a mais alguém.
fn role_members(app: &mut Unkvoid, ui: &mut Ui, tree: &ServerTree, role: &Role) {
    let assignable = tree
        .role_rows()
        .iter()
        .any(|row| row.id == role.id && row.assignable);
    let (with, without): (Vec<_>, Vec<_>) = tree
        .members
        .iter()
        .partition(|member| member.role_ids.contains(&role.id));

    if assignable {
        let candidates: Vec<_> = without
            .iter()
            .filter(|member| tree.member_actions(member).roles || member.user_id == tree.me.user_id)
            .collect();

        ui.add_enabled_ui(!candidates.is_empty(), |ui| {
            ui.menu_button(
                RichText::new(format!("{}  Adicionar membros", icons::PLUS))
                    .font(theme::bold(14.0)),
                |ui| {
                    ui.set_min_width(220.0);

                    for member in candidates {
                        if compact_icon_menu_item(ui, icons::USER_PLUS, display_name(member)) {
                            let mut role_ids = member.role_ids.clone();

                            role_ids.push(role.id);
                            app.actions
                                .push(set_roles(tree.id, member.user_id, &role_ids));
                            ui.close();
                        }
                    }
                },
            );
        });
        ui.add_space(12.0);
    }

    if with.is_empty() {
        ui.label(RichText::new("Ninguém tem este cargo ainda.").color(theme::DIM));
    }

    for member in with {
        // Na altura do avatar e com vão: a janela das configurações zera o espaço entre itens.
        ui.allocate_ui_with_layout(vec2(ui.available_width(), 36.0), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            widgets::avatar(ui, display_name(member), member.avatar_url.as_deref(), 32.0);
            ui.add_space(4.0);
            ui.label(RichText::new(display_name(member)).font(theme::bold(15.0)));
            ui.label(
                RichText::new(&member.name)
                    .font(theme::regular(13.0))
                    .color(theme::DIM),
            );

            let removable = assignable
                && (tree.member_actions(member).roles || member.user_id == tree.me.user_id);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if removable
                    && hand_icon_button(ui, icons::X, "Tirar o cargo", theme::MUTED).clicked()
                {
                    let role_ids: Vec<i64> = member
                        .role_ids
                        .iter()
                        .copied()
                        .filter(|id| *id != role.id)
                        .collect();

                    app.actions
                        .push(set_roles(tree.id, member.user_id, &role_ids));
                }
            });
        });
        ui.add_space(4.0);
    }
}

/// Os cargos de alguém, sempre a lista inteira: é o que o `PATCH` do membro espera.
pub(super) fn set_roles(server: i64, user: i64, role_ids: &[i64]) -> Action {
    Action::request(
        "updateMember",
        json!({ "server": server, "user": user }),
        json!({ "role_ids": role_ids }),
        Refresh::Tree(server),
    )
}

fn members(app: &mut Unkvoid, ui: &mut Ui, tree: &ServerTree, state: &mut ServerSettings) {
    settings::heading(ui, "Membros do servidor");

    let search = state.search.trim().to_lowercase();
    let mut shown: Vec<_> = tree
        .members
        .iter()
        .filter(|member| {
            search.is_empty()
                || display_name(member).to_lowercase().contains(&search)
                || member.name.to_lowercase().contains(&search)
        })
        .collect();

    shown.sort_by_key(|member| display_name(member).to_lowercase());

    // A linha já nasce da altura do campo: no `horizontal` o texto ficava acima do meio dele.
    ui.allocate_ui_with_layout(vec2(ui.available_width(), 36.0), Layout::left_to_right(Align::Center), |ui| {
        let member_label = if tree.members.len() == 1 { "membro" } else { "membros" };
        ui.label(RichText::new(format!("{} {member_label}", tree.members.len())).font(theme::bold(15.0)));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.set_max_width(260.0);
            widgets::text_field(ui, &mut state.search, "Buscar membros", false);
        });
    });
    ui.add_space(12.0);

    for member in shown {
        Frame::new()
            .fill(theme::SIDEBAR)
            .corner_radius(CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(12, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                // Na altura do avatar, com o nome no meio dele e um vão: a janela das configurações
                // zera o espaço entre itens, e o nome colava no avatar, rente ao topo.
                ui.allocate_ui_with_layout(vec2(ui.available_width(), 40.0), Layout::left_to_right(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    widgets::avatar(ui, display_name(member), member.avatar_url.as_deref(), 40.0);
                    // Um `vertical` começa sempre no alto da linha; com a altura do texto (o nome, e
                    // as pílulas quando há) medida no quadro anterior, a linha o centra.
                    let text_id = ui.id().with(("member-text", member.user_id));
                    let text_height = ui.data(|data| data.get_temp::<f32>(text_id)).unwrap_or(22.0);
                    let text = ui.allocate_ui_with_layout(vec2((ui.available_width() - 48.0).max(0.0), text_height), Layout::top_down(Align::Min), |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.label(RichText::new(display_name(member)).font(theme::bold(16.0)));

                            if member.is_owner || member.user_id == tree.owner_id {
                                crate::ui::icons::label(ui, icons::CROWN_SIMPLE, theme::icon(14.0), theme::WARNING)
                                .on_hover_text("Dono do servidor");
                            }

                            ui.label(
                                RichText::new(&member.name)
                                    .font(theme::regular(14.0))
                                    .color(theme::DESCRIPTION),
                            );
                        });
                        role_chips(app, ui, tree, member);
                    });
                    ui.data_mut(|data| data.insert_temp(text_id, text.response.rect.height()));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let more = hand_icon_button(
                            ui,
                            icons::DOTS_THREE_VERTICAL,
                            "Mais",
                            theme::MUTED,
                        );

                        egui::Popup::menu(&more)
                            .frame(widgets::menu_frame())
                            .show(|ui| super::members::member_menu(app, ui, tree, member));
                    });
                });
            });
        ui.separator();
    }
}

/// Os cargos de alguém em pílulas, como no Discord: a bolinha da cor, o nome, o X para tirar e o
/// "+" para dar outro.
pub(super) fn role_chips(
    app: &mut Unkvoid,
    ui: &mut Ui,
    tree: &ServerTree,
    member: &core_app::models::Member,
) {
    let manage = tree.member_actions(member).roles
        || (member.user_id == tree.me.user_id && tree.can(MANAGE_ROLES));
    let rows = tree.role_rows();
    let assignable = |role: &Role| rows.iter().any(|row| row.id == role.id && row.assignable);
    let mut owned: Vec<&Role> = tree
        .roles
        .iter()
        .filter(|role| !role.is_everyone && member.role_ids.contains(&role.id))
        .collect();

    owned.sort_by_key(|role| std::cmp::Reverse(role.position));

    let missing: Vec<&Role> = tree
        .roles
        .iter()
        .filter(|role| !role.is_everyone && assignable(role) && !member.role_ids.contains(&role.id))
        .collect();

    // Sem pílula nem "+", nada: a linha vazia ainda ocupava altura e tirava o nome do meio.
    if owned.is_empty() && !(manage && !missing.is_empty()) {
        return;
    }

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);

        for role in &owned {
            Frame::new()
                .fill(theme::ELEVATED)
                .corner_radius(CornerRadius::same(4))
                .inner_margin(egui::Margin::symmetric(6, 2))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;

                        let color = role
                            .color
                            .as_deref()
                            .and_then(theme::color_of)
                            .unwrap_or(theme::MUTED);
                        let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());

                        ui.painter().circle_filled(dot.center(), 5.0, color);
                        ui.label(RichText::new(&role.name).font(theme::regular(12.0)));

                        if manage && assignable(role) {
                            let remove = ui.add(
                                egui::Label::new(
                                    RichText::new(icons::X)
                                        .font(theme::icon(11.0))
                                        .color(theme::DIM),
                                )
                                .sense(Sense::click()),
                            );

                            if remove.on_hover_text("Tirar o cargo").clicked() {
                                let role_ids: Vec<i64> = member
                                    .role_ids
                                    .iter()
                                    .copied()
                                    .filter(|id| *id != role.id)
                                    .collect();

                                app.actions
                                    .push(set_roles(tree.id, member.user_id, &role_ids));
                            }
                        }
                    });
                });
        }

        if manage && !missing.is_empty() {
            let add = ui.add(
                egui::Button::new(crate::ui::icons::atom(icons::PLUS, theme::icon(12.0), theme::TEXT))
                    .fill(theme::ELEVATED)
                    .corner_radius(CornerRadius::same(4)),
            );

            egui::Popup::menu(&add)
                .frame(widgets::menu_frame())
                .show(|ui| {
                    ui.set_min_width(200.0);

                    for role in missing {
                        if compact_icon_menu_item(ui, icons::SHIELD_CHEVRON, &role.name) {
                            let mut role_ids = member.role_ids.clone();

                            role_ids.push(role.id);
                            app.actions
                                .push(set_roles(tree.id, member.user_id, &role_ids));
                            ui.close();
                        }
                    }
                });
        }
    });
}

fn invites(app: &mut Unkvoid, ui: &mut Ui, tree: &ServerTree) {
    ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
    settings::heading(ui, "Convites");
    ui.add_space(6.0);
    ui.label(
        RichText::new("Crie um link para que outras pessoas possam entrar no servidor.")
            .font(theme::regular(14.0))
            .color(theme::DESCRIPTION),
    );
    ui.add_space(22.0);
    ui.allocate_ui_with_layout(vec2(ui.available_width(), 36.0), Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(RichText::new("Links de convite ativos").font(theme::regular(13.0)).color(theme::DESCRIPTION));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if compact_button(ui, "Criar link de convite", tree.can(CREATE_INVITE), false) {
                app.actions.push(Action::CreateServerInvite { server: tree.id, max_age: 604800, max_uses: 0 });
            }
            if compact_button(ui, if tree.invites_paused { "Retomar convites" } else { "Pausar convites" }, tree.can(MANAGE_SERVER), true) {
                app.actions.push(Action::SetInvitesPaused { server: tree.id, paused: !tree.invites_paused });
            }
        });
    });
    ui.add_space(24.0);
    let table_width = ui.available_width().max(0.0);
    let (header, _) = ui.allocate_exact_size(vec2(table_width, 28.0), Sense::hover());
    for (label, fraction) in [
        ("CRIADO POR", 0.0),
        ("CÓDIGO DO CONVITE", 0.32),
        ("USOS", 0.65),
        ("EXPIRA EM", 0.76),
    ] {
        ui.painter().text(
            pos2(header.left() + header.width() * fraction, header.center().y),
            Align2::LEFT_CENTER,
            label,
            theme::bold(11.0),
            theme::MUTED,
        );
    }
    ui.separator();
    let invites = app.server_invites.clone();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for invite in invites {
            let (row, response) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), 58.0), Sense::hover());
            if response.hovered() { ui.painter().rect_filled(row, CornerRadius::same(5), theme::HOVER); }
            let creator_name = invite.creator.as_ref().map_or("Conta removida", |person| person.name.as_str());
            let creator = Rect::from_center_size(pos2(row.left() + 18.0, row.center().y), vec2(30.0, 30.0));
            widgets::paint_avatar(ui, creator, creator_name, invite.creator.as_ref().and_then(|person| person.avatar_url.as_deref()));
            let channel = invite.channel.as_ref().map_or(String::new(), |channel| format!("#{}", channel.name));
            ui.painter().text(pos2(creator.right() + 8.0, row.center().y - 7.0), Align2::LEFT_CENTER, creator_name, theme::regular(13.0), theme::TEXT);
            ui.painter().text(pos2(creator.right() + 8.0, row.center().y + 10.0), Align2::LEFT_CENTER, channel, theme::regular(12.0), theme::MUTED);
            ui.painter().text(pos2(row.left() + row.width() * 0.32, row.center().y), Align2::LEFT_CENTER, &invite.code, egui::FontId::monospace(13.0), theme::TEXT);
            ui.painter().text(pos2(row.left() + row.width() * 0.65, row.center().y), Align2::LEFT_CENTER, invite.uses.to_string(), theme::regular(13.0), theme::TEXT);
            ui.painter().text(pos2(row.left() + row.width() * 0.76, row.center().y), Align2::LEFT_CENTER, invite.expires_at.as_deref().map_or_else(|| "∞".to_owned(), countdown), egui::FontId::monospace(12.0), theme::TEXT);
            if response.hovered() && tree.can(MANAGE_SERVER) {
                let remove = Rect::from_center_size(pos2(row.right() - 20.0, row.center().y), vec2(30.0, 30.0));
                if ui.put(remove, egui::Button::new(crate::ui::icons::atom(icons::X, theme::icon(16.0), theme::TEXT)).fill(theme::BUTTON).corner_radius(theme::RADIUS)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    app.actions.push(Action::DeleteServerInvite { server: tree.id, invite: invite.id });
                }
            }
        }
    });
}

fn countdown(expires_at: &str) -> String {
    let Some(expiry) = chrono::DateTime::parse_from_rfc3339(expires_at).ok() else { return "∞".to_owned() };
    let seconds = (expiry.timestamp() - chrono::Utc::now().timestamp()).max(0);
    let days = seconds / 86_400;
    let hours = seconds % 86_400 / 3_600;
    let minutes = seconds % 3_600 / 60;
    let seconds = seconds % 60;
    format!("{days:02}:{hours:02}:{minutes:02}:{seconds:02}")
}

fn soundboard(app: &mut Unkvoid, ui: &mut Ui, tree: &ServerTree) {
    settings::heading(ui, "Painel de efeitos sonoros");
    ui.add_space(8.0);
    ui.label(RichText::new("Sons disponíveis para tocar durante a voz.").font(theme::regular(14.0)).color(theme::DESCRIPTION));
    ui.add_space(16.0);
    if tree.can(MANAGE_EXPRESSIONS) && compact_button(ui, "Adicionar som", true, false) {
        app.actions.push(Action::PickServerSound(tree.id));
    }
    ui.add_space(16.0);
    for sound in app.server_sounds.clone() {
        Frame::new()
            .fill(theme::SIDEBAR)
            .corner_radius(CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width().max(0.0));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.label(RichText::new(sound.emoji.as_deref().unwrap_or("🎵")).font(theme::regular(18.0)));
                    ui.label(RichText::new(&sound.name).font(theme::regular(14.0)));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if hand_icon_button(ui, icons::PLAY, "Reproduzir", theme::MUTED).clicked() {
                            app.actions.push(Action::PlayServerSound(sound.clone()));
                        }
                        if tree.can(MANAGE_EXPRESSIONS) && compact_button(ui, "Excluir", true, true) {
                            app.actions.push(Action::DeleteServerSound(tree.id, sound.id));
                        }
                    });
                });
            });
        ui.add_space(8.0);
    }
}

fn bans(app: &mut Unkvoid, ui: &mut Ui, tree: &ServerTree) {
    settings::heading(ui, "Banimentos");
    ui.label(RichText::new("Quem está banido não volta ao servidor, nem com convite, até o banimento ser revogado.").color(theme::MUTED));
    ui.add_space(16.0);

    if tree.bans.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(30.0);
            crate::ui::icons::label(ui, icons::PROHIBIT, theme::icon(24.0), theme::DIM);
            ui.label(RichText::new("Ninguém foi banido deste servidor.").color(theme::MUTED));
        });

        return;
    }

    ui.label(
        RichText::new(format!("BANIDOS — {}", tree.bans.len()))
            .font(theme::bold(12.0))
            .color(theme::MUTED),
    );
    ui.add_space(8.0);

    for ban in &tree.bans {
        Frame::new()
            .fill(theme::SIDEBAR)
            .corner_radius(CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.allocate_ui_with_layout(vec2(ui.available_width(), 40.0), Layout::left_to_right(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    widgets::avatar(ui, &ban.name, None, 36.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&ban.name).font(theme::bold(15.0)));
                        ui.label(
                            RichText::new(
                                ban.reason
                                    .as_deref()
                                    .filter(|reason| !reason.is_empty())
                                    .unwrap_or("Sem motivo informado."),
                            )
                            .font(theme::regular(13.0))
                            .color(theme::MUTED),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if compact_button(ui, "Revogar banimento", true, false) {
                            app.actions.push(Action::request(
                                "unban",
                                json!({ "server": tree.id, "user": ban.user_id }),
                                Value::Null,
                                Refresh::Tree(tree.id),
                            ));
                        }
                    });
                });
            });
        ui.add_space(6.0);
    }
}

fn audit(app: &mut Unkvoid, ui: &mut Ui) {
    settings::heading(ui, "Registro de auditoria");

    if app.audits.is_empty() {
        ui.label(RichText::new("Nada registrado ainda.").color(theme::DIM));

        return;
    }

    for entry in &app.audits {
        let (icon, color) = match entry.event.as_str() {
            "created" => (icons::PLUS_CIRCLE, theme::SUCCESS),
            "deleted" => (icons::TRASH, theme::DANGER),
            "sync" => (icons::USERS, theme::ACCENT_TEXT),
            _ => (icons::PENCIL_SIMPLE, theme::WARNING),
        };
        let actor = entry
            .actor
            .as_ref()
            .map_or("Conta apagada", |actor| actor.name.as_str());

        Frame::new()
            .fill(theme::SIDEBAR)
            .corner_radius(CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                // Na altura das duas linhas do texto: no `horizontal` o ícone e o avatar ficavam
                // presos no alto, e o texto colado no avatar.
                ui.allocate_ui_with_layout(vec2(ui.available_width(), 40.0), Layout::left_to_right(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    crate::ui::icons::label(ui, icon, theme::icon(20.0), color);
                    widgets::avatar(
                        ui,
                        actor,
                        entry
                            .actor
                            .as_ref()
                            .and_then(|actor| actor.avatar_url.as_deref()),
                        32.0,
                    );
                    ui.vertical(|ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            ui.label(RichText::new(actor).font(theme::bold(14.0)));
                            ui.label(RichText::new(&entry.summary).font(theme::regular(14.0)));
                        });
                        ui.label(
                            RichText::new(format!(
                                "{} às {}",
                                crate::words::day(&entry.at),
                                crate::words::clock(&entry.at)
                            ))
                            .font(theme::regular(12.0))
                            .color(theme::DIM),
                        );
                    });
                });
            });
        ui.add_space(6.0);
    }
}

/// As abas do editor de cargo: o texto com o sublinhado roxo na escolhida.
fn tab_button(ui: &mut Ui, label: &str, selected: bool) -> bool {
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), theme::bold(15.0), theme::TEXT);
    let (rect, response) = ui.allocate_exact_size(galley.size() + vec2(16.0, 14.0), Sense::click());
    let color = if selected || response.hovered() {
        theme::TEXT
    } else {
        theme::MUTED
    };

    ui.painter().text(
        rect.center() - vec2(0.0, 2.0),
        Align2::CENTER_CENTER,
        label,
        theme::bold(15.0),
        color,
    );

    if selected {
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 1.0,
            egui::Stroke::new(2.0, theme::ACCENT),
        );
    }

    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// Um texto que se clica, como o "Remover" e o "Voltar" do Discord.
pub(super) fn link(ui: &mut Ui, text: &str, color: Color32) -> bool {
    ui.add(
        egui::Label::new(RichText::new(text).font(theme::bold(13.0)).color(color))
            .sense(Sense::click()),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_permission_bit_appears_once() {
        let flags: Vec<i64> = PERMISSIONS
            .iter()
            .flat_map(|(_, permissions)| permissions.iter().map(|(flag, _, _)| *flag))
            .collect();
        let joined = flags.iter().fold(0, |bits, flag| bits | flag);

        assert_eq!(
            flags.len(),
            19,
            "uma permissão sumiu da tela ou apareceu duas vezes"
        );
        assert_eq!(joined, (1 << 19) - 1);
    }
}
