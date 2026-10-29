//! Inventor-like guidance for command dialogs (`docs/spec/0-ui.md` §2.9,
//! §4.1): selector rows (arrow, coloured dot, count, `x`, the active one
//! underlined), the right-click menu of an open command (OK, Apply,
//! Continue, Back, Cancel, Repeat and the command's options), Enter /
//! Ctrl+Enter, and the hint next to the cursor. The selector logic is GUI
//! free in [`crate::command::flow`]; this module draws it and routes the
//! events. Face Draft, Shell, Combine, Split and Loft use it; other commands adopt it by giving
//! their selectors ([`Flow`]) and options here (see the spec).

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke};

use super::CadCraftApp;
use crate::command::flow::{EnterAction, Flow, MenuItem, PickKind, Selector};

/// Appended to the prompt when every required input is there.
pub(super) const OR_OK: &str = ", or OK (Enter)";

/// What the user did with the selector rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct RowEvents {
    /// A row was clicked: make it active.
    pub activate: Option<&'static str>,
    /// Its `x` was clicked: clear it.
    pub clear: Option<&'static str>,
}

fn color(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// One selector row inside a two-column grid: the name, then the arrow,
/// the coloured dot, the state (`texts`: "3 faces", "XY Plane", or empty
/// for "Select") and an `x` when it holds something. The active row is
/// underlined and highlighted.
pub(super) fn selector_row(
    ui: &mut egui::Ui,
    s: &Selector,
    active: bool,
    text: &str,
    ev: &mut RowEvents,
) {
    super::panels::plabel(ui, s.label);
    ui.horizontal(|ui| {
        // The arrow (Inventor's white arrow button) and the dot.
        let (r, _) = ui.allocate_exact_size(egui::vec2(22.0, 16.0), egui::Sense::hover());
        let p = ui.painter();
        let ink = if active {
            ui.visuals().strong_text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        let a = r.left_center() + egui::vec2(1.0, 0.0);
        p.line_segment([a, a + egui::vec2(9.0, 0.0)], Stroke::new(1.5, ink));
        p.line_segment(
            [a + egui::vec2(9.0, 0.0), a + egui::vec2(5.0, -4.0)],
            Stroke::new(1.5, ink),
        );
        p.line_segment(
            [a + egui::vec2(9.0, 0.0), a + egui::vec2(5.0, 4.0)],
            Stroke::new(1.5, ink),
        );
        p.circle_filled(r.right_center() - egui::vec2(5.0, 0.0), 4.0, color(s.color));
        let shown = if text.is_empty() {
            if s.is_empty() {
                "Select".to_owned()
            } else {
                format!("{}", s.count)
            }
        } else {
            text.to_owned()
        };
        let t = if active {
            RichText::new(shown).strong().underline()
        } else {
            RichText::new(shown)
        };
        let row = ui
            .selectable_label(active, t)
            .on_hover_text(super::i18n::tr(s.prompt));
        if row.clicked() {
            ev.activate = Some(s.key);
        }
        if !s.is_empty() && ui.small_button("x").on_hover_text("Clear").clicked() {
            ev.clear = Some(s.key);
        }
        if s.required && s.is_empty() {
            ui.label(RichText::new("*").color(Color32::from_rgb(210, 60, 50)))
                .on_hover_text("Required");
        }
    });
    ui.end_row();
}

/// The rows of every selector of `flow` (`texts` by key; missing: count).
pub(super) fn selector_rows(ui: &mut egui::Ui, flow: &Flow, texts: &[(&str, String)]) -> RowEvents {
    let mut ev = RowEvents::default();
    for (i, s) in flow.selectors.iter().enumerate() {
        let text = texts
            .iter()
            .find(|(k, _)| *k == s.key)
            .map_or("", |(_, t)| t.as_str());
        selector_row(ui, s, flow.active == Some(i), text, &mut ev);
    }
    ev
}

/// A command option in the right-click menu (a type, a direction, Flip).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct MenuOption {
    /// What the entry says.
    pub label: String,
    /// Whether it is the current choice (shown checked).
    pub checked: bool,
}

/// What was chosen in the right-click menu.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum MenuChoice {
    /// A standard entry.
    Item(MenuItem),
    /// The command's option `k` (index into the options given).
    Option(usize),
}

/// The right-click menu of an open command.
pub(super) fn command_menu(
    ui: &mut egui::Ui,
    flow: &Flow,
    apply: bool,
    repeat: Option<String>,
    options: &[MenuOption],
) -> Option<MenuChoice> {
    let mut out = None;
    for (item, on) in flow.menu(apply, repeat.is_some()) {
        let label = match item {
            MenuItem::Ok => "OK (Ctrl+Enter)".to_owned(),
            MenuItem::Apply => "Apply (+)".to_owned(),
            MenuItem::Continue => "Continue (Enter)".to_owned(),
            MenuItem::Back => "Back".to_owned(),
            MenuItem::Cancel => "Cancel (Esc)".to_owned(),
            MenuItem::Repeat => format!("Repeat {}", repeat.clone().unwrap_or_default()),
        };
        if item == MenuItem::Cancel && !options.is_empty() {
            ui.separator();
            for (k, o) in options.iter().enumerate() {
                if ui.radio(o.checked, o.label.as_str()).clicked() {
                    out = Some(MenuChoice::Option(k));
                    ui.close();
                }
            }
            ui.separator();
        }
        if ui.add_enabled(on, egui::Button::new(label)).clicked() {
            out = Some(MenuChoice::Item(item));
            ui.close();
        }
    }
    out
}

/// The hint next to the cursor: what the active selector expects.
pub(super) fn cursor_hint(painter: &egui::Painter, rect: Rect, pos: Pos2, text: &str) {
    if text.is_empty() || !rect.contains(pos) {
        return;
    }
    let font = egui::FontId::proportional(12.0);
    let galley = painter.layout_no_wrap(text.to_owned(), font, Color32::from_gray(20));
    let mut at = pos + egui::vec2(18.0, 20.0);
    if at.x + galley.size().x + 8.0 > rect.right() {
        at.x = pos.x - galley.size().x - 18.0;
    }
    if at.y + galley.size().y + 6.0 > rect.bottom() {
        at.y = pos.y - galley.size().y - 18.0;
    }
    let bg = Rect::from_min_size(
        at - egui::vec2(4.0, 2.0),
        galley.size() + egui::vec2(8.0, 4.0),
    );
    painter.rect_filled(bg, 3.0, Color32::from_rgba_unmultiplied(255, 252, 220, 235));
    painter.rect_stroke(
        bg,
        3.0,
        Stroke::new(1.0, Color32::from_gray(120)),
        egui::StrokeKind::Inside,
    );
    painter.galley(at, galley, Color32::from_gray(20));
}

/// The command whose dialog uses the selector flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cmd {
    Draft,
    Shell,
    /// Combine and Split (`gui/combine_split.rs`).
    Combine,
    Split,
    /// Loft (`gui/loft.rs`).
    Loft,
    /// The sheet metal commands (`gui/sheet_metal.rs`).
    Sheet,
    /// A part command with its panel only (`gui/part_planned.rs`).
    Planned,
    /// Thicken/Offset (`gui/thicken.rs`).
    Thicken,
    /// Delete Face (`gui/delete_face.rs`).
    DeleteFace,
    /// Move Bodies (`gui/move_bodies.rs`).
    MoveBodies,
}

impl CadCraftApp {
    fn flow_cmd(&self) -> Option<Cmd> {
        if self.draft_draft.is_some() {
            Some(Cmd::Draft)
        } else if self.shell_draft.is_some() {
            Some(Cmd::Shell)
        } else if self.combine_draft.is_some() {
            Some(Cmd::Combine)
        } else if self.split_draft.is_some() {
            Some(Cmd::Split)
        } else if self.loft_draft.is_some() {
            Some(Cmd::Loft)
        } else if self.sm.draft.is_some() {
            Some(Cmd::Sheet)
        } else if self.part_planned.is_some() {
            Some(Cmd::Planned)
        } else if self.thicken_draft.is_some() {
            Some(Cmd::Thicken)
        } else if self.delete_draft.is_some() {
            Some(Cmd::DeleteFace)
        } else if self.move_draft.is_some() {
            Some(Cmd::MoveBodies)
        } else {
            None
        }
    }

    /// The open command's selector flow (Face Draft, Shell).
    pub(super) fn command_flow(&self) -> Option<Flow> {
        match self.flow_cmd()? {
            Cmd::Draft => self.draft_draft.as_ref().map(|d| d.flow()),
            Cmd::Shell => self.shell_draft.as_ref().map(|d| {
                Flow::new(
                    crate::command::flow::shell_selectors(&d.def),
                    Some("remove"),
                )
            }),
            Cmd::Combine | Cmd::Split => self.body_cmd_flow(),
            Cmd::Loft => self.loft_draft.as_ref().map(|d| d.flow()),
            Cmd::Sheet => self.sm_flow(),
            Cmd::Planned => self.part_planned.as_ref().map(|d| d.flow()),
            Cmd::Thicken => self.thicken_draft.as_ref().map(|d| d.flow()),
            Cmd::DeleteFace => self.delete_draft.as_ref().map(|d| d.flow()),
            Cmd::MoveBodies => self.move_draft.as_ref().map(|d| d.flow()),
        }
    }

    /// Whether a command with a selector flow is open (Enter and Space then
    /// do not repeat the last command).
    pub(super) fn flow_command_open(&self) -> bool {
        self.flow_cmd().is_some()
    }

    /// The status-bar prompt of the open command: `<Command>: <prompt>`.
    pub(super) fn command_prompt(&self) -> Option<String> {
        let flow = self.command_flow()?;
        let name = match self.flow_cmd()? {
            Cmd::Draft => "Face Draft",
            Cmd::Shell => "Shell",
            Cmd::Combine => "Combine",
            Cmd::Split => "Split",
            Cmd::Loft => "Loft",
            Cmd::Sheet => return self.sm_prompt(),
            Cmd::Planned => return self.part_planned.as_ref().map(|d| d.prompt()),
            Cmd::Thicken => return self.thicken_prompt(),
            Cmd::DeleteFace => return self.delete_face_prompt(),
            Cmd::MoveBodies => return self.move_bodies_prompt(),
        };
        let p = super::i18n::tr(flow.prompt());
        // A finished command also offers OK (Shell's input is optional).
        let ok = if flow.ready() && self.flow_cmd() != Some(Cmd::Shell) {
            super::i18n::tr(OR_OK)
        } else {
            "".into()
        };
        Some(format!("{name}: {p}{ok}"))
    }

    /// Makes the selector `key` of the open command active.
    pub(super) fn command_activate(&mut self, key: &str) {
        match self.flow_cmd() {
            Some(Cmd::Draft) => {
                if let Some(d) = self.draft_draft.as_mut() {
                    d.set_active(key);
                }
            }
            Some(Cmd::Combine | Cmd::Split) => self.body_cmd_activate(key),
            Some(Cmd::Loft) => {
                if let Some(d) = self.loft_draft.as_mut() {
                    d.set_active(key);
                }
            }
            Some(Cmd::Sheet) => self.sm_activate(key),
            Some(Cmd::Planned) => {
                if let Some(d) = self.part_planned.as_mut() {
                    d.set_active(key);
                }
            }
            Some(Cmd::Thicken) => self.thicken_activate(key),
            Some(Cmd::DeleteFace) => {}
            Some(Cmd::MoveBodies) => self.move_bodies_activate(key),
            Some(Cmd::Shell) | None => {}
        }
    }

    fn command_ok(&mut self, keep_open: bool) {
        match self.flow_cmd() {
            Some(Cmd::Draft) => self.commit_draft(keep_open),
            Some(Cmd::Shell) => self.commit_shell(keep_open),
            Some(Cmd::Combine) => self.commit_combine(keep_open),
            Some(Cmd::Split) => self.commit_split(keep_open),
            Some(Cmd::Loft) => self.commit_loft(keep_open),
            Some(Cmd::Sheet) => self.sm_commit(keep_open),
            Some(Cmd::Thicken) => self.commit_thicken(keep_open),
            Some(Cmd::DeleteFace) => self.commit_delete_face(keep_open),
            Some(Cmd::MoveBodies) => self.commit_move_bodies(keep_open),
            Some(Cmd::Planned) => self.planned_ok(),
            None => {}
        }
    }

    fn command_cancel(&mut self) {
        self.draft_draft = None;
        self.shell_draft = None;
        self.loft_draft = None;
        self.part_planned = None;
        self.cancel_thicken();
        self.cancel_delete_face();
        self.cancel_move_bodies();
        if self.sm.draft.take().is_some() {
            self.end_feature_edit();
        }
        self.close_body_cmds();
        self.end_feature_edit();
    }

    /// Enter (Ctrl+Enter: always OK) while a command is open: Continue to
    /// a required empty selector, else OK. Returns whether a command took it.
    pub(super) fn command_enter(&mut self, ctrl: bool) -> bool {
        let Some(mut flow) = self.command_flow() else {
            return false;
        };
        if ctrl {
            if flow.ready() {
                self.command_ok(false);
            }
            return true;
        }
        match flow.enter() {
            EnterAction::Continue => {
                if let Some(k) = flow.active_key() {
                    self.command_activate(k);
                }
            }
            EnterAction::Ok => self.command_ok(false),
        }
        true
    }

    /// The right-click menu while a command is open; `false` when none is.
    pub(super) fn command_context_menu(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(cmd) = self.flow_cmd() else {
            return false;
        };
        let Some(mut flow) = self.command_flow() else {
            return false;
        };
        let editing = match cmd {
            Cmd::Draft => self.draft_draft.as_ref().is_some_and(|d| d.edit.is_some()),
            Cmd::Shell => self.shell_draft.as_ref().is_some_and(|d| d.edit.is_some()),
            Cmd::Combine => self
                .combine_draft
                .as_ref()
                .is_some_and(|d| d.edit.is_some()),
            Cmd::Split => self.split_draft.as_ref().is_some_and(|d| d.edit.is_some()),
            Cmd::Loft => self.loft_draft.as_ref().is_some_and(|d| d.edit.is_some()),
            Cmd::Sheet => self.sm.draft.as_ref().is_some_and(|d| d.edit.is_some()),
            Cmd::Planned => false,
            Cmd::Thicken => self
                .thicken_draft
                .as_ref()
                .is_some_and(|d| d.edit.is_some()),
            Cmd::DeleteFace => self.delete_draft.as_ref().is_some_and(|d| d.edit.is_some()),
            Cmd::MoveBodies => self.move_draft.as_ref().is_some_and(|d| d.edit.is_some()),
        };
        let repeat = self
            .repeatable()
            .map(|a| super::command_label(a).to_owned());
        let options = match cmd {
            Cmd::Draft => self
                .draft_draft
                .as_ref()
                .map(|d| d.menu_options())
                .unwrap_or_default(),
            Cmd::Shell => self
                .shell_draft
                .as_ref()
                .map(|d| super::shell::menu_options(&d.def))
                .unwrap_or_default(),
            Cmd::Combine | Cmd::Split => self.body_cmd_menu_options(),
            Cmd::Loft => self
                .loft_draft
                .as_ref()
                .map(|d| d.menu_options())
                .unwrap_or_default(),
            Cmd::Sheet => self.sm_menu_options(),
            Cmd::Planned | Cmd::Thicken | Cmd::DeleteFace | Cmd::MoveBodies => Vec::new(),
        };
        // A surface command's selector of edges: chains from the edge (or face) under the
        // cursor (Creo's loop / tangent chain / boundary selection).
        if cmd == Cmd::Planned
            && self.part_planned.as_ref().is_some_and(|d| {
                d.kind.has_logic()
                    && d.flow()
                        .current()
                        .is_some_and(|s| s.accepts.contains(&PickKind::Edge))
            })
        {
            use super::part_planned::Chain;
            let on_face = self.context_face.is_some();
            let entries = [
                (Chain::Loop, "Select Loop", self.context_edge.is_some()),
                (
                    Chain::Tangent,
                    "Select Tangent Chain",
                    self.context_edge.is_some(),
                ),
                (Chain::Boundary, "Select Face Boundary", on_face),
            ];
            let mut any = false;
            for (c, label, on) in entries {
                if !on {
                    continue;
                }
                any = true;
                if ui.button(label).clicked() {
                    let items = self.chain_edges(c);
                    self.part_planned_add(items);
                    ui.close();
                    return true;
                }
            }
            if any {
                ui.separator();
            }
        }
        let Some(choice) = command_menu(ui, &flow, !editing, repeat, &options) else {
            return true;
        };
        match choice {
            MenuChoice::Item(MenuItem::Ok) => self.command_ok(false),
            MenuChoice::Item(MenuItem::Apply) => self.command_ok(true),
            MenuChoice::Item(MenuItem::Continue) => {
                if flow.continue_()
                    && let Some(k) = flow.active_key()
                {
                    self.command_activate(k);
                }
            }
            MenuChoice::Item(MenuItem::Back) => {
                if flow.back()
                    && let Some(k) = flow.active_key()
                {
                    self.command_activate(k);
                }
            }
            MenuChoice::Item(MenuItem::Cancel) => self.command_cancel(),
            MenuChoice::Item(MenuItem::Repeat) => {
                if let Some(a) = self.repeatable() {
                    self.command_cancel();
                    self.apply(a);
                }
            }
            MenuChoice::Option(k) => match cmd {
                Cmd::Draft => {
                    if let Some(d) = self.draft_draft.as_mut() {
                        d.menu_option(k);
                    }
                }
                Cmd::Shell => {
                    if let Some(d) = self.shell_draft.as_mut() {
                        super::shell::menu_option(&mut d.def, k);
                    }
                }
                Cmd::Combine | Cmd::Split => self.body_cmd_menu_option(k),
                Cmd::Loft => {
                    if let Some(d) = self.loft_draft.as_mut() {
                        d.menu_option(k);
                    }
                }
                Cmd::Sheet => self.sm_menu_option(k),
                Cmd::Planned | Cmd::Thicken | Cmd::DeleteFace | Cmd::MoveBodies => {}
            },
        }
        true
    }

    /// The hint next to the cursor while a command with selectors is open.
    pub(super) fn draw_command_hint(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        pointer: Option<Pos2>,
    ) {
        let (Some(p), Some(flow)) = (pointer, self.command_flow()) else {
            return;
        };
        cursor_hint(painter, rect, p, &super::i18n::tr(flow.prompt()));
    }
}

/// The feature name in a command panel's breadcrumb (Inventor: click once
/// to select it, again to rename; Enter or leaving the field keeps it, Esc
/// drops the edit). Minimal stand-in for the shared breadcrumb widget.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct NameCrumb {
    /// The name chosen in the panel (applied on OK).
    pub name: Option<String>,
    /// Selected by a first click.
    selected: bool,
    /// The text being typed.
    editing: Option<String>,
    /// Focus requested once (egui swallows Enter when re-requested).
    focused: bool,
}

/// Draws `Part > <name>` and handles the two-click rename; `shown` is the
/// name it has now.
pub(super) fn name_crumb(ui: &mut egui::Ui, crumb: &mut NameCrumb, shown: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Part >").weak());
        if let Some(text) = crumb.editing.as_mut() {
            let r = ui.add(egui::TextEdit::singleline(text).desired_width(140.0));
            if !crumb.focused {
                super::valuefield::focus_all(ui.ctx(), &r, text);
                crumb.focused = true;
            }
            let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if r.lost_focus() {
                let t = text.trim().to_owned();
                if !esc && !t.is_empty() {
                    crumb.name = Some(t);
                }
                crumb.editing = None;
                crumb.selected = false;
                crumb.focused = false;
            }
        } else {
            let name = crumb.name.clone().unwrap_or_else(|| shown.to_owned());
            let r = ui
                .selectable_label(
                    crumb.selected,
                    RichText::new(name.as_str()).color(Color32::from_rgb(40, 110, 200)),
                )
                .on_hover_text("Click to select, click again to rename");
            if r.clicked() {
                if crumb.selected {
                    crumb.editing = Some(name);
                } else {
                    crumb.selected = true;
                }
            }
        }
    });
}
