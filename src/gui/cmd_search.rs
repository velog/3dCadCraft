//! Command search (`docs/spec/0.15-context-menus-shortcuts.md` §5.3, G7): a
//! "Search commands" field in the Quick Access bar; Alt+S focuses it. The
//! results drop down below it (icon · name · key · Tab › Panel); ↑/↓ move,
//! Enter or a click starts the command like its ribbon button, Esc clears
//! and then gives the focus back to the view. Matching is
//! `commands::search` (aliases, names in English / Inventor / Bulgarian,
//! tooltips, `*` / `?`, recent commands first).

use eframe::egui::{self, Color32, Key, RichText};

use super::CadCraftApp;
use super::commands::{self, Env};
use super::icons::{self, Palette};

/// Rows shown at most.
const ROWS: usize = 12;

/// State of the search field.
#[derive(Default)]
pub(super) struct CmdSearch {
    /// Text typed.
    pub text: String,
    /// Alt+S asked for the focus (taken once).
    pub focus: bool,
    /// Highlighted result row.
    pub row: usize,
}

impl CadCraftApp {
    /// The environment commands are searched in.
    fn search_env(&self) -> Env {
        if self.is_drawing() {
            Env::Drawing
        } else if self.is_assembly() {
            Env::Assembly
        } else if self.editing.is_some() {
            Env::Sketch
        } else {
            Env::Part
        }
    }

    /// Alt+S: focus the search field (spec §5.3; works in a sketch and in
    /// an open command, which is not aborted until another one starts).
    pub(super) fn search_shortcut(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, Key::S)) {
            self.cmd_search.focus = true;
        }
    }

    /// The field and its result list (Quick Access bar).
    pub(super) fn command_search(&mut self, ui: &mut egui::Ui) {
        let id = egui::Id::new("cmd_search_field");
        let focused = ui.memory(|m| m.has_focus(id));
        // Keys of the list are taken before the field sees them.
        let (up, down, enter, esc) = if focused {
            ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, Key::ArrowDown)
                        || i.consume_key(egui::Modifiers::NONE, Key::Tab),
                    i.consume_key(egui::Modifiers::NONE, Key::Enter),
                    i.consume_key(egui::Modifiers::NONE, Key::Escape),
                )
            })
        } else {
            (false, false, false, false)
        };
        let r = ui.add(
            egui::TextEdit::singleline(&mut self.cmd_search.text)
                .id(id)
                .desired_width(170.0)
                .hint_text("Search commands (Alt+S)"),
        );
        if std::mem::take(&mut self.cmd_search.focus) {
            r.request_focus();
            // Select the text typed before.
            if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), id) {
                let n = self.cmd_search.text.chars().count();
                st.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(n),
                )));
                st.store(ui.ctx(), id);
            }
        }
        if r.changed() {
            self.cmd_search.row = 0;
        }
        if !focused && !r.has_focus() {
            return;
        }
        if esc {
            if self.cmd_search.text.is_empty() {
                ui.memory_mut(|m| m.surrender_focus(id));
            } else {
                self.cmd_search.text.clear();
            }
        }
        let env = self.search_env();
        let hits = commands::search(&self.cmd_search.text, env, &self.recent_commands);
        let hits: Vec<_> = hits.into_iter().take(ROWS).collect();
        if hits.is_empty() {
            if !self.cmd_search.text.trim().is_empty() {
                egui::Area::new(egui::Id::new("cmd_search_list"))
                    .fixed_pos(r.rect.left_bottom() + egui::vec2(0.0, 2.0))
                    .order(egui::Order::Foreground)
                    .show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.label(RichText::new("No command found").weak());
                        });
                    });
            }
            return;
        }
        let n = hits.len();
        let row = &mut self.cmd_search.row;
        if down {
            *row = (*row + 1) % n;
        }
        if up {
            *row = (*row + n - 1) % n;
        }
        *row = (*row).min(n - 1);
        let mut run = None;
        if enter {
            // The highlighted row, or the first one that can run here.
            let h = &hits[*row];
            run = if h.ready && h.valid {
                Some(h.cmd)
            } else {
                hits.iter().find(|h| h.ready && h.valid).map(|h| h.cmd)
            };
            if run.is_none() {
                self.file_status = Some(Err(format!(
                    "{}: {}",
                    h.cmd.name,
                    h.cmd.unavailable(env).unwrap_or_default()
                )));
            }
        }
        let bg = super::i18n::bulgarian();
        let current = *row;
        egui::Area::new(egui::Id::new("cmd_search_list"))
            .fixed_pos(r.rect.left_bottom() + egui::vec2(0.0, 2.0))
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(380.0);
                    for (k, h) in hits.iter().enumerate() {
                        let can = h.ready && h.valid;
                        let name = if bg { h.cmd.bg } else { h.cmd.name };
                        let resp = ui
                            .horizontal(|ui| {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(18.0, 18.0),
                                    egui::Sense::hover(),
                                );
                                let pal = if can {
                                    Palette::ENABLED
                                } else {
                                    Palette::DISABLED
                                };
                                icons::paint(ui.painter(), rect, h.cmd.icon, &pal);
                                let mut t = RichText::new(name);
                                if k == current {
                                    t = t.strong();
                                }
                                if !can {
                                    t = t.color(Color32::GRAY);
                                }
                                let b = ui.add(
                                    egui::Button::new(t)
                                        .frame(k == current)
                                        .min_size(egui::vec2(200.0, 0.0)),
                                );
                                if let Some(key) = h.cmd.shown_key() {
                                    ui.label(RichText::new(key).monospace().weak());
                                }
                                ui.label(RichText::new(h.cmd.path()).small().weak());
                                b
                            })
                            .inner;
                        let tip = match h.cmd.unavailable(env) {
                            Some(why) => format!("{}\n{why}", h.cmd.tip),
                            None => h.cmd.tip.to_owned(),
                        };
                        let resp = resp.on_hover_text(tip);
                        if resp.clicked() && can {
                            run = Some(h.cmd);
                        }
                    }
                });
            });
        if let Some(c) = run {
            self.cmd_search.text.clear();
            ui.memory_mut(|m| m.surrender_focus(id));
            self.start_command(c);
        }
    }
}
