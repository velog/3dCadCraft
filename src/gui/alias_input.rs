//! Command alias input next to the cursor (`docs/spec/0.15-context-menus-shortcuts.md`
//! §5.1, G6; Application Options → *Show command alias input dialog*): with
//! the option on, a letter typed over the view opens a small field 16 px
//! right of the cursor holding it; the list below it shows the matching
//! commands (`commands::search`: exact alias first, recent commands, then
//! prefixes and fuzzy), also those of other environments, greyed with the
//! reason. What the letters can only mean starts by itself: one command has
//! an alias starting with them (E, SMF, RCT); two letters or more that are a
//! whole alias with longer ones beside it (RC next to RCT) start after a short
//! wait, unless the user is choosing (arrows, or the mouse over the list). A
//! single letter with longer ones (R) only shows the list. Enter, Space or a
//! click on a row starts the highlighted command; ↑/↓/Tab move; Esc or a click
//! elsewhere closes it. With the option off the letters gather in the
//! status bar, as before.

use eframe::egui::{self, Color32, Key, RichText};

use super::CadCraftApp;
use super::commands::{self, Command};
use super::icons::{self, Palette};

/// Rows of the list at most.
const ROWS: usize = 40;

/// Height of the list before it scrolls (points).
const LIST_HEIGHT: f32 = 340.0;

/// State of the alias field.
#[derive(Default)]
pub(super) struct AliasInput {
    /// Open, at this screen point (the cursor when the first letter came).
    pub at: Option<egui::Pos2>,
    /// Text typed.
    pub text: String,
    /// Highlighted row.
    pub row: usize,
    /// The field still has to take the focus (once).
    pub focus: bool,
    /// When the text last changed (egui time, s).
    pub last: f64,
    /// The user is choosing a row with the arrows: nothing starts by itself.
    pub choosing: bool,
}

/// The typed alias without the field (spec 0.15, alias input rule 2),
/// shown in the status bar.
#[derive(Default)]
pub(super) struct AliasBuffer {
    /// Letters typed (upper case).
    pub text: String,
    /// When the last letter came (egui time, s).
    pub last: f64,
}

/// Wait for a further letter when the buffer is an alias and longer ones
/// start with it (s).
const ALIAS_WAIT: f64 = 0.6;
/// The buffer is cleared after this long without a key (s).
const ALIAS_CLEAR: f64 = 2.0;

impl CadCraftApp {
    /// The command environment for aliases.
    pub(super) fn alias_env(&self) -> commands::Env {
        if self.is_drawing() {
            commands::Env::Drawing
        } else if self.is_assembly() {
            commands::Env::Assembly
        } else if self.editing.is_some() {
            commands::Env::Sketch
        } else {
            commands::Env::Part
        }
    }

    /// Multi-letter aliases with the field off (rule 2): letters gather in a
    /// buffer shown in the status bar; a buffer that is an alias with no
    /// longer one starts at once, one with longer ones after a short wait
    /// (or Enter / Space). A single letter that no longer alias starts with
    /// is left to the single-letter shortcuts. Returns whether it took this
    /// frame's keys.
    pub(super) fn alias_buffer_keys(&mut self, ctx: &egui::Context) -> bool {
        if self.alias_input {
            return false;
        }
        let env = self.alias_env();
        let now = ctx.input(|i| i.time);
        if !self.alias_buf.text.is_empty() {
            let idle = now - self.alias_buf.last;
            let m = commands::alias_match_in(&self.alias_buf.text, env, &self.keymap);
            if idle > ALIAS_WAIT
                && let Some(c) = m.exact
            {
                self.alias_buf = AliasBuffer::default();
                self.start_command(c);
                return true;
            }
            if idle > ALIAS_CLEAR {
                self.alias_buf = AliasBuffer::default();
            }
        }
        let typed = super::keys::letter_alias(ctx.input(|i| i.modifiers))
            .then(|| {
                ctx.input(|i| {
                    i.events.iter().find_map(|e| match e {
                        egui::Event::Text(t)
                            if !t.is_empty() && t.chars().all(|c| c.is_ascii_alphabetic()) =>
                        {
                            Some(t.to_ascii_uppercase())
                        }
                        _ => None,
                    })
                })
            })
            .flatten();
        if let Some(t) = typed {
            let cand = format!("{}{t}", self.alias_buf.text);
            let m = commands::alias_match_in(&cand, env, &self.keymap);
            if self.alias_buf.text.is_empty() && m.longer == 0 {
                // A single letter the user gave a command (Customize) starts
                // it; one they took away from its default command does
                // nothing; the others are the built-in single-letter keys.
                let user = m.exact.filter(|c| self.keymap.aliases.contains_key(c.id));
                let removed = m.exact.is_none()
                    && commands::alias_match(&cand, env)
                        .exact
                        .is_some_and(|c| self.keymap.aliases.contains_key(c.id));
                if user.is_none() && !removed {
                    return false; // a single-letter shortcut, as before
                }
                ctx.input_mut(|i| {
                    i.events.retain(|e| {
                        !matches!(e, egui::Event::Text(_))
                            && !matches!(e, egui::Event::Key { key, pressed: true, .. }
                                if key.name().len() == 1 && key.name().chars().all(|c| c.is_ascii_alphabetic()))
                    })
                });
                if let Some(c) = user {
                    self.start_command(c);
                }
                return true;
            }
            // The letters belong to the buffer, not to the single-letter keys.
            ctx.input_mut(|i| {
                i.events.retain(|e| {
                    !matches!(e, egui::Event::Text(_))
                        && !matches!(e, egui::Event::Key { key, pressed: true, .. }
                            if key.name().len() == 1 && key.name().chars().all(|c| c.is_ascii_alphabetic()))
                })
            });
            self.alias_buf.text = cand;
            self.alias_buf.last = now;
            if let Some(c) = m.exact
                && m.longer == 0
            {
                self.alias_buf = AliasBuffer::default();
                self.start_command(c);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(ALIAS_WAIT + 0.05));
            }
            return true;
        }
        if self.alias_buf.text.is_empty() {
            return false;
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(0.2));
        let (enter, esc, back) = ctx.input_mut(|i| {
            let space = i.consume_key(egui::Modifiers::NONE, Key::Space);
            (
                i.consume_key(egui::Modifiers::NONE, Key::Enter) || space,
                i.consume_key(egui::Modifiers::NONE, Key::Escape),
                i.consume_key(egui::Modifiers::NONE, Key::Backspace),
            )
        });
        if esc {
            self.alias_buf = AliasBuffer::default();
            return true;
        }
        if back {
            self.alias_buf.text.pop();
            self.alias_buf.last = now;
            return true;
        }
        if enter {
            let m = commands::alias_match_in(&self.alias_buf.text, env, &self.keymap);
            self.alias_buf = AliasBuffer::default();
            if let Some(c) = m.exact.or(m.first_longer) {
                self.start_command(c);
            }
            return true;
        }
        false
    }

    /// The buffer in the status bar: `Command: SM_  (Flange)`.
    pub(super) fn alias_buffer_label(&self, ui: &mut egui::Ui) {
        if self.alias_buf.text.is_empty() {
            return;
        }
        let m = commands::alias_match_in(&self.alias_buf.text, self.alias_env(), &self.keymap);
        let bg = super::i18n::bulgarian();
        let name = |c: &Command| if bg { c.bg } else { c.name };
        match m.exact.or(m.first_longer) {
            Some(c) => ui.label(
                RichText::new(format!("Command: {}_  ({})", self.alias_buf.text, name(c))).strong(),
            ),
            None => ui.colored_label(
                Color32::from_rgb(220, 70, 60),
                format!("{} — no such command here", self.alias_buf.text),
            ),
        };
        ui.separator();
    }

    /// Opens the field when a letter is typed (the option on, no text field
    /// focused — `shortcuts` checks that before). Returns whether it took
    /// the key.
    pub(super) fn alias_start(&mut self, ctx: &egui::Context) -> bool {
        if !self.alias_input || self.alias.at.is_some() {
            return false;
        }
        let first = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Text(t) if t.chars().all(|c| c.is_alphabetic()) => Some(t.clone()),
                _ => None,
            })
        });
        let Some(t) = first else { return false };
        let env = self.alias_env();
        let only = commands::alias_candidates(&t, env, &self.keymap);
        // A letter nothing answers to is left to the built-in keys.
        if only.is_empty()
            && commands::search_in(&t, env, &self.recent_commands, &self.keymap).is_empty()
        {
            return false;
        }
        let at = ctx.input(|i| i.pointer.hover_pos()).unwrap_or_default();
        // The letter belongs to the field, not to the single-letter keys.
        ctx.input_mut(|i| {
            i.events.retain(|e| {
                !matches!(e, egui::Event::Text(_))
                    && !matches!(e, egui::Event::Key { key, pressed: true, .. }
                        if key.name().len() == 1 && key.name().chars().all(|c| c.is_ascii_alphabetic()))
            })
        });
        // It can only mean one command (E, H, L): at once, no list.
        if let [c] = only.as_slice() {
            self.start_command(c);
            return true;
        }
        self.alias = AliasInput {
            at: Some(at),
            text: t,
            row: 0,
            focus: true,
            last: ctx.input(|i| i.time),
            choosing: false,
        };
        true
    }

    /// Starts command `c` like a click on its button (the ribbon tab of
    /// its first place comes forward).
    pub(super) fn start_command(&mut self, c: &'static Command) {
        if let Some(commands::Place::Ribbon(tab, _)) = c.places.first()
            && let Some(t) = super::ribbon::Tab::by_label(tab)
        {
            self.tab = t;
        }
        if let Some(a) = c.action {
            self.apply(a);
        }
    }

    /// Draws the open field and its list; handles its keys.
    pub(super) fn alias_window(&mut self, ctx: &egui::Context) {
        let Some(at) = self.alias.at else { return };
        let id = egui::Id::new("alias_field");
        let (up, down, enter, esc) = ctx.input_mut(|i| {
            let space = i.consume_key(egui::Modifiers::NONE, Key::Space);
            if space {
                i.events
                    .retain(|e| !matches!(e, egui::Event::Text(t) if t == " "));
            }
            (
                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp)
                    || i.consume_key(egui::Modifiers::SHIFT, Key::Tab),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown)
                    || i.consume_key(egui::Modifiers::NONE, Key::Tab),
                i.consume_key(egui::Modifiers::NONE, Key::Enter) || space,
                i.consume_key(egui::Modifiers::NONE, Key::Escape),
            )
        });
        let env = self.alias_env();
        let hits: Vec<_> =
            commands::search_in(&self.alias.text, env, &self.recent_commands, &self.keymap)
                .into_iter()
                .take(ROWS)
                .collect();
        let n = hits.len().max(1);
        if down {
            self.alias.row = (self.alias.row + 1) % n;
        }
        if up {
            self.alias.row = (self.alias.row + n - 1) % n;
        }
        self.alias.choosing |= up || down;
        self.alias.row = self.alias.row.min(n - 1);
        let mut run: Option<&'static Command> = None;
        let mut close = esc;
        let mut edited = false;
        let bg = super::i18n::bulgarian();
        let current = self.alias.row;
        let area = egui::Area::new(egui::Id::new("alias_area"))
            .fixed_pos(at + egui::vec2(16.0, -26.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                // A label that can be selected would take a click meant for its row.
                ui.style_mut().interaction.selectable_labels = false;
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.alias.text)
                            .id(id)
                            .desired_width(150.0)
                            .hint_text("command"),
                    );
                    if std::mem::take(&mut self.alias.focus) {
                        r.request_focus();
                        if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), id) {
                            let end = egui::text::CCursor::new(self.alias.text.chars().count());
                            st.cursor
                                .set_char_range(Some(egui::text::CCursorRange::one(end)));
                            st.store(ui.ctx(), id);
                        }
                    } else if r.lost_focus() && !enter {
                        // A click on a row takes the focus before it counts as a click: the
                        // field keeps the typing; a click outside closes it (below).
                        r.request_focus();
                    }
                    if r.changed() {
                        self.alias.row = 0;
                        self.alias.last = ui.input(|i| i.time);
                        self.alias.choosing = false;
                        edited = true;
                    }
                    if hits.is_empty() {
                        ui.colored_label(
                            Color32::from_rgb(220, 70, 60),
                            format!("{} — no such command here", self.alias.text),
                        );
                    }
                    egui::ScrollArea::vertical()
                        .max_height(LIST_HEIGHT)
                        .show(ui, |ui| {
                            for (k, h) in hits.iter().enumerate() {
                                let can = h.ready && h.valid;
                                let row = ui.horizontal(|ui| {
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(16.0, 16.0),
                                        egui::Sense::hover(),
                                    );
                                    let pal = if can {
                                        Palette::enabled()
                                    } else {
                                        Palette::disabled()
                                    };
                                    icons::paint(ui.painter(), rect, h.cmd.icon, &pal);
                                    let mut t =
                                        RichText::new(if bg { h.cmd.bg } else { h.cmd.name });
                                    if k == current {
                                        t = t.strong();
                                    }
                                    if !can {
                                        t = t.color(super::theme::tokens().text_weak);
                                    }
                                    let b = ui.add(egui::Button::new(t).frame(k == current));
                                    if let Some(key) = self.keymap.shown_key(h.cmd) {
                                        ui.label(RichText::new(key).monospace().weak());
                                    }
                                    // Why it cannot run here, else where it lives.
                                    let why = h.cmd.unavailable(env);
                                    ui.label(
                                        RichText::new(why.clone().unwrap_or_else(|| h.cmd.path()))
                                            .small()
                                            .weak(),
                                    );
                                    if k == current {
                                        b.scroll_to_me(None);
                                    }
                                    let tip = why.unwrap_or_else(|| h.cmd.tip.to_owned());
                                    if b.on_hover_text(tip).clicked() && can {
                                        run = Some(h.cmd);
                                    }
                                });
                                // The whole row is the target, not only the name.
                                if row.response.interact(egui::Sense::click()).clicked() && can {
                                    run = Some(h.cmd);
                                }
                            }
                        });
                });
            });
        // A click outside the field closes it.
        if ctx.input(|i| i.pointer.any_pressed())
            && let Some(p) = ctx.input(|i| i.pointer.interact_pos())
            && !area.response.rect.contains(p)
        {
            close = true;
        }
        if enter && run.is_none() {
            match hits.get(self.alias.row) {
                Some(h) if h.ready && h.valid => run = Some(h.cmd),
                Some(h) => {
                    self.file_status = Some(Err(format!(
                        "{}: {}",
                        h.cmd.name,
                        h.cmd.unavailable(env).unwrap_or_default()
                    )));
                }
                None => {}
            }
        }
        // What the letters can only mean starts by itself.
        if run.is_none() {
            let cands = commands::alias_candidates(&self.alias.text, env, &self.keymap);
            let m = commands::alias_match_in(&self.alias.text, env, &self.keymap);
            let idle = ctx.input(|i| i.time) - self.alias.last;
            if edited && let [c] = cands.as_slice() {
                run = Some(*c);
            } else if self.alias.text.chars().count() >= 2
                && m.exact.is_some()
                // Choosing with the arrows or the mouse: the list waits.
                && !self.alias.choosing
                && !ctx.input(|i| {
                    i.pointer
                        .hover_pos()
                        .is_some_and(|p| area.response.rect.contains(p))
                })
            {
                if idle > ALIAS_WAIT {
                    run = m.exact;
                } else {
                    ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                        ALIAS_WAIT + 0.05,
                    ));
                }
            }
        }
        if let Some(c) = run {
            self.alias = AliasInput::default();
            self.start_command(c);
        } else if close || self.alias.text.is_empty() {
            self.alias = AliasInput::default();
        }
    }
}
