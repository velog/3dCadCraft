//! The user's keys (`docs/spec/0.15-context-menus-shortcuts.md` §4.3, G5):
//! aliases and key combinations that replace the registry's defaults, kept in
//! `settings.json` → `options.keymap` by command id (ids never change).
//! Tools → Customize → Keyboard lists every command with its keys (as
//! Inventor's Customize dialog); a right click on a ribbon button assigns
//! one directly (as Blender's *Assign Shortcut*).

use std::collections::BTreeMap;

use eframe::egui::{self, Key, Modifiers, RichText};
use serde::{Deserialize, Serialize};

use super::CadCraftApp;
use super::commands::{self, Command, Env};

/// Only the differences from the registry's defaults.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Keymap {
    /// Aliases replacing a command's defaults (an empty list removes them).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub aliases: BTreeMap<String, Vec<String>>,
    /// A key combination per command (`Ctrl+Shift+E`, `F4`, `Alt+]`); an
    /// empty text removes the default one.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keys: BTreeMap<String, String>,
}

/// Keys the application itself uses: assigning one of them warns (the
/// user's key wins).
pub const BUILT_IN: &[&str] = &[
    "Ctrl+S",
    "Ctrl+Shift+S",
    "Ctrl+Z",
    "Ctrl+Y",
    "Ctrl+Shift+Z",
    "Ctrl+O",
    "Ctrl+N",
    "Ctrl+A",
    "Ctrl+C",
    "Ctrl+V",
    "Ctrl+0",
    "Ctrl+Tab",
    "Alt+S",
    "Alt+V",
    "F1",
    "F6",
    "F7",
    "F8",
    "F9",
    "F11",
];

impl Keymap {
    /// The aliases `c` answers to.
    pub fn aliases_of(&self, c: &Command) -> Vec<String> {
        match self.aliases.get(c.id) {
            Some(v) => v.clone(),
            None => c.aliases.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    /// The key combination of `c` (the user's, else the registry's).
    pub fn key_of(&self, c: &Command) -> Option<String> {
        match self.keys.get(c.id) {
            Some(k) if k.is_empty() => None,
            Some(k) => Some(k.clone()),
            None => c.key.map(str::to_owned),
        }
    }

    /// [`Command::shown_key`] with the user's keys: the shortcut, else the
    /// first alias.
    pub fn shown_key(&self, c: &Command) -> Option<String> {
        if !self.changed(c.id) {
            return c.shown_key().map(str::to_owned);
        }
        (c.bound || c.ready())
            .then(|| {
                self.key_of(c)
                    .or_else(|| self.aliases_of(c).into_iter().next())
            })
            .flatten()
    }

    /// Whether the user changed anything of `c`.
    pub fn changed(&self, id: &str) -> bool {
        self.aliases.contains_key(id) || self.keys.contains_key(id)
    }

    /// Back to the registry's defaults for `id`.
    pub fn reset(&mut self, id: &str) {
        self.aliases.remove(id);
        self.keys.remove(id);
    }

    /// Sets `c`'s aliases from a comma / space separated text; the default
    /// text drops the override.
    pub fn set_aliases(&mut self, c: &Command, text: &str) {
        let list: Vec<String> = text
            .split([',', ' '])
            .map(|a| a.trim().to_ascii_uppercase())
            .filter(|a| !a.is_empty())
            .collect();
        let default: Vec<String> = c.aliases.iter().map(|a| a.to_ascii_uppercase()).collect();
        if list == default {
            self.aliases.remove(c.id);
        } else {
            self.aliases.insert(c.id.to_owned(), list);
        }
    }

    /// Sets `c`'s key combination (`None` removes it).
    pub fn set_key(&mut self, c: &Command, key: Option<String>) {
        let key = key.unwrap_or_default();
        if Some(key.as_str()) == c.key || (key.is_empty() && c.key.is_none()) {
            self.keys.remove(c.id);
        } else {
            self.keys.insert(c.id.to_owned(), key);
        }
    }

    /// Another runnable command of `env` already answering to `alias`.
    pub fn alias_owner(&self, alias: &str, env: Env, except: &str) -> Option<&'static Command> {
        let a = alias.to_ascii_uppercase();
        runnable(env).find(|c| {
            c.id != except
                && self
                    .aliases_of(c)
                    .iter()
                    .any(|x| x.eq_ignore_ascii_case(&a))
        })
    }

    /// Another command of `env` already using the key combination `key`.
    pub fn key_owner(&self, key: &str, env: Env, except: &str) -> Option<&'static Command> {
        runnable(env).find(|c| c.id != except && self.key_of(c).as_deref() == Some(key))
    }

    /// The command of `env` the user bound to `key` (user bindings only:
    /// the registry's own keys are served by `shortcuts`).
    pub fn command_for_key(&self, key: &str, env: Env) -> Option<&'static Command> {
        self.keys
            .iter()
            .filter(|(_, k)| k.as_str() == key)
            .find_map(|(id, _)| commands::by_id(id).filter(|c| c.valid_in(env) && c.ready()))
    }
}

fn runnable(env: Env) -> impl Iterator<Item = &'static Command> {
    commands::COMMANDS
        .iter()
        .filter(move |c| c.ready() && !c.held && c.valid_in(env))
}

/// The text of a key combination, or `None` when it is an alias's letter or
/// digit (no Ctrl / Alt, not a function key) or a modifier alone.
pub fn chord_text(m: Modifiers, key: Key) -> Option<String> {
    if is_modifier(key) {
        return None;
    }
    let name = key.name();
    let plain = name.len() == 1 && name.chars().all(|c| c.is_ascii_alphanumeric());
    if plain && !m.ctrl && !m.command && !m.alt {
        return None;
    }
    if matches!(key, Key::Escape | Key::Backspace | Key::Delete) && m.is_none() {
        return None;
    }
    let mut s = String::new();
    if m.ctrl || m.command {
        s.push_str("Ctrl+");
    }
    if m.alt {
        s.push_str("Alt+");
    }
    if m.shift {
        s.push_str("Shift+");
    }
    s.push_str(name);
    Some(s)
}

/// A modifier key pressed on its own (some platforms send it as a key).
fn is_modifier(key: Key) -> bool {
    let n = key.name();
    [
        "Control", "Ctrl", "Shift", "Alt", "Meta", "Super", "Command",
    ]
    .iter()
    .any(|m| n.contains(m))
}

/// Which commands the Keyboard table shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Show {
    #[default]
    All,
    Assigned,
    Unassigned,
}

/// A request from a ribbon button's right-click menu (egui temp data).
#[derive(Clone, Debug)]
pub enum Request {
    /// Open the table on this command and wait for its key.
    AssignKey(&'static str),
    /// Open the table on this command's alias field.
    EditAlias(&'static str),
    RemoveKey(&'static str),
    RemoveAliases(&'static str),
    Reset(&'static str),
    /// Open the table on this command.
    Show(&'static str),
}

/// The egui temp-data id of the pending [`Request`].
pub fn request_id() -> egui::Id {
    egui::Id::new("keymap_request")
}

/// The egui temp-data id of the tooltip keys (command id → text).
pub fn tips_id() -> egui::Id {
    egui::Id::new("keymap_tips")
}

/// The right-click menu of a ribbon button (Blender's button menu).
pub fn button_menu(r: &egui::Response, id: &'static str) {
    if id.is_empty() {
        return;
    }
    r.context_menu(|ui| {
        let mut req = None;
        if ui.button("Assign Shortcut...").clicked() {
            req = Some(Request::AssignKey(id));
        }
        if ui.button("Change Alias...").clicked() {
            req = Some(Request::EditAlias(id));
        }
        ui.separator();
        if ui.button("Remove Shortcut").clicked() {
            req = Some(Request::RemoveKey(id));
        }
        if ui.button("Remove Alias").clicked() {
            req = Some(Request::RemoveAliases(id));
        }
        if ui.button("Reset to Default").clicked() {
            req = Some(Request::Reset(id));
        }
        ui.separator();
        if ui.button("Customize Keyboard...").clicked() {
            req = Some(Request::Show(id));
        }
        if let Some(q) = req {
            ui.ctx().data_mut(|d| d.insert_temp(request_id(), Some(q)));
            ui.close();
        }
    });
}

/// The keys shown in a button's tooltip: the user's when changed.
pub fn tip_keys(ctx: &egui::Context, id: &str) -> Option<Option<String>> {
    ctx.data(|d| {
        d.get_temp::<std::sync::Arc<BTreeMap<String, String>>>(tips_id())
            .and_then(|m| m.get(id).cloned())
    })
    .map(|t| (!t.is_empty()).then_some(t))
}

/// Tools → Customize → Keyboard.
#[derive(Default)]
pub struct CustomizeUi {
    pub open: bool,
    /// Environment filter (`None` = all).
    env: Option<Env>,
    show: Show,
    search: String,
    /// The row waiting for a key press.
    capture: Option<&'static str>,
    /// The row whose aliases are being typed, and the text.
    alias_edit: Option<(&'static str, String, bool)>,
    /// The row to scroll to once.
    scroll_to: Option<&'static str>,
    /// The last warning or result.
    message: Option<String>,
    /// Import text (pasted JSON).
    import: Option<String>,
}

impl CustomizeUi {
    /// Keys go to the table while it waits for a key (the app's shortcuts
    /// must not run them).
    pub fn capturing(&self) -> bool {
        self.open && self.capture.is_some()
    }
}

fn env_label(e: Env) -> &'static str {
    match e {
        Env::Part => "Part",
        Env::Sketch => "Sketch",
        Env::Assembly => "Assembly",
        Env::Drawing => "Drawing",
    }
}

/// The first environment a command runs in (for conflicts and the table).
fn home_env(c: &Command) -> Env {
    [Env::Part, Env::Sketch, Env::Assembly, Env::Drawing]
        .into_iter()
        .find(|e| c.envs.has(*e))
        .unwrap_or(Env::Part)
}

impl CadCraftApp {
    /// Takes a ribbon right-click request and keeps the tooltip keys
    /// current; called once a frame.
    pub(super) fn keymap_frame(&mut self, ctx: &egui::Context) {
        let req = ctx.data_mut(|d| d.get_temp::<Option<Request>>(request_id()).flatten());
        if let Some(q) = req {
            ctx.data_mut(|d| d.insert_temp::<Option<Request>>(request_id(), None));
            self.keymap_request(q);
        }
        if self.keymap_tips_dirty {
            self.keymap_tips_dirty = false;
            let mut m = BTreeMap::new();
            for id in self.keymap.aliases.keys().chain(self.keymap.keys.keys()) {
                if let Some(c) = commands::by_id(id) {
                    m.insert(id.clone(), self.keymap.key_of(c).unwrap_or_default());
                }
            }
            ctx.data_mut(|d| d.insert_temp(tips_id(), std::sync::Arc::new(m)));
        }
    }

    fn keymap_request(&mut self, q: Request) {
        let ui = &mut self.customize;
        match q {
            Request::AssignKey(id) => {
                ui.open = true;
                ui.capture = Some(id);
                ui.scroll_to = Some(id);
                ui.message = Some("Press the keys for the command (Esc cancels)".into());
            }
            Request::EditAlias(id) => {
                let text = commands::by_id(id)
                    .map(|c| self.keymap.aliases_of(c).join(", "))
                    .unwrap_or_default();
                ui.open = true;
                ui.alias_edit = Some((id, text, true));
                ui.scroll_to = Some(id);
            }
            Request::RemoveKey(id) => {
                if let Some(c) = commands::by_id(id) {
                    self.keymap.set_key(c, None);
                    self.keymap_changed();
                }
            }
            Request::RemoveAliases(id) => {
                if let Some(c) = commands::by_id(id) {
                    self.keymap.set_aliases(c, "");
                    self.keymap_changed();
                }
            }
            Request::Reset(id) => {
                self.keymap.reset(id);
                self.keymap_changed();
            }
            Request::Show(id) => {
                ui.open = true;
                ui.scroll_to = Some(id);
            }
        }
    }

    /// Tooltips follow; `settings.json` is saved by the options snapshot.
    fn keymap_changed(&mut self) {
        self.keymap_tips_dirty = true;
    }

    /// A key combination the user bound starts its command (before the
    /// built-in keys). Returns whether it took the key.
    pub(super) fn user_key_shortcuts(&mut self, ctx: &egui::Context) -> bool {
        if self.keymap.keys.is_empty() {
            return false;
        }
        let env = self.alias_env();
        let hit = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } => chord_text(*modifiers, *key).and_then(|t| {
                    self.keymap
                        .command_for_key(&t, env)
                        .map(|c| (c, *key, *modifiers))
                }),
                _ => None,
            })
        });
        let Some((c, key, m)) = hit else {
            return false;
        };
        ctx.input_mut(|i| i.consume_key(m, key));
        self.start_command(c);
        true
    }

    /// The Customize window (Keyboard tab).
    pub(super) fn customize_window(&mut self, ctx: &egui::Context) {
        if !self.customize.open {
            return;
        }
        let mut open = true;
        // A key for the waiting row: taken here, before anything else.
        if let Some(id) = self.customize.capture {
            let got = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } if !is_modifier(*key) => Some((*key, *modifiers)),
                    _ => None,
                })
            });
            if let Some((key, m)) = got {
                ctx.input_mut(|i| i.events.clear());
                self.customize.capture = None;
                if key == Key::Escape && m.is_none() {
                    self.customize.message = None;
                } else if let Some(c) = commands::by_id(id) {
                    if matches!(key, Key::Backspace | Key::Delete) && m.is_none() {
                        self.keymap.set_key(c, None);
                        self.customize.message = Some(format!("{}: shortcut removed", c.name));
                        self.keymap_changed();
                    } else if let Some(text) = chord_text(m, key) {
                        let env = home_env(c);
                        let mut note = format!("{}: {text}", c.name);
                        if let Some(o) = self.keymap.key_owner(&text, env, c.id) {
                            note.push_str(&format!(
                                ". Warning: {} had it (now removed there)",
                                o.name
                            ));
                            self.keymap.set_key(o, None);
                        }
                        if BUILT_IN.contains(&text.as_str()) {
                            note.push_str(". Warning: it replaces a built-in key");
                        }
                        self.keymap.set_key(c, Some(text));
                        self.customize.message = Some(note);
                        self.keymap_changed();
                    } else {
                        self.customize.message = Some(
                            "A letter or digit alone is an alias: use Change Alias, or add Ctrl / Alt"
                                .into(),
                        );
                    }
                }
            }
        }
        let win = egui::Window::new("Customize")
            .open(&mut open)
            .default_size([720.0, 520.0])
            .resizable(true)
            .collapsible(false);
        self.layout
            .place("customize", ctx.content_rect(), true, win)
            .show(ctx, |ui| self.customize_body(ui));
        if !open {
            self.customize.open = false;
            self.customize.capture = None;
            self.customize.alias_edit = None;
        }
    }

    fn customize_body(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let _ = ui.selectable_label(true, "Keyboard");
            ui.add_enabled(false, egui::Button::new("Ribbon"))
                .on_disabled_hover_text("Planned — spec 0.15 §G0");
            ui.add_enabled(false, egui::Button::new("Marking Menu"))
                .on_disabled_hover_text("Planned — spec 0.15 §G4");
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Category");
            egui::ComboBox::from_id_salt("km_env")
                .selected_text(self.customize.env.map_or("All", env_label))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.customize.env, None, "All");
                    for e in [Env::Part, Env::Sketch, Env::Assembly, Env::Drawing] {
                        ui.selectable_value(&mut self.customize.env, Some(e), env_label(e));
                    }
                });
            ui.label("Show");
            ui.radio_value(&mut self.customize.show, Show::All, "All");
            ui.radio_value(&mut self.customize.show, Show::Assigned, "Assigned");
            ui.radio_value(&mut self.customize.show, Show::Unassigned, "Unassigned");
            ui.label("Search");
            ui.add(egui::TextEdit::singleline(&mut self.customize.search).desired_width(140.0));
        });
        let msg = self.customize.message.clone();
        ui.label(
            RichText::new(msg.unwrap_or_else(|| {
                "Click a shortcut to press new keys; type aliases separated by commas".into()
            }))
            .weak(),
        );
        let bottom = egui::Panel::bottom(egui::Id::new("km_bottom"));
        bottom.show(ui, |ui| self.customize_buttons(ui));
        let q = self.customize.search.to_lowercase();
        let rows: Vec<&'static Command> = commands::COMMANDS
            .iter()
            .filter(|c| c.ready() && !c.held)
            .filter(|c| self.customize.env.is_none_or(|e| c.valid_in(e)))
            .filter(|c| {
                let assigned =
                    !self.keymap.aliases_of(c).is_empty() || self.keymap.key_of(c).is_some();
                match self.customize.show {
                    Show::All => true,
                    Show::Assigned => assigned,
                    Show::Unassigned => !assigned,
                }
            })
            .filter(|c| {
                q.is_empty()
                    || c.name.to_lowercase().contains(&q)
                    || c.bg.to_lowercase().contains(&q)
                    || self
                        .keymap
                        .aliases_of(c)
                        .iter()
                        .any(|a| a.to_lowercase().starts_with(&q))
                    || self
                        .keymap
                        .key_of(c)
                        .is_some_and(|k| k.to_lowercase().contains(&q))
            })
            .collect();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("km_rows")
                    .num_columns(5)
                    .striped(true)
                    .min_col_width(60.0)
                    .show(ui, |ui| {
                        ui.label(RichText::new("Command").strong());
                        ui.label(RichText::new("Environment").strong());
                        ui.label(RichText::new("Alias").strong());
                        ui.label(RichText::new("Shortcut").strong());
                        ui.label("");
                        ui.end_row();
                        for c in rows {
                            self.customize_row(ui, c);
                        }
                    });
            });
    }

    fn customize_row(&mut self, ui: &mut egui::Ui, c: &'static Command) {
        let changed = self.keymap.changed(c.id);
        let name = if changed {
            RichText::new(format!("{} *", c.name)).strong()
        } else {
            RichText::new(c.name)
        };
        let r = ui.label(name).on_hover_text(c.bg);
        if self.customize.scroll_to == Some(c.id) {
            r.scroll_to_me(Some(egui::Align::Center));
            self.customize.scroll_to = None;
        }
        ui.label(env_label(home_env(c)));
        // Aliases: typed, taken on Enter or when the field loses focus.
        let editing = self
            .customize
            .alias_edit
            .as_ref()
            .is_some_and(|(id, ..)| *id == c.id);
        if editing {
            let mut done = false;
            if let Some((_, text, focus)) = self.customize.alias_edit.as_mut() {
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(90.0));
                if *focus {
                    r.request_focus();
                    *focus = false;
                }
                done = r.lost_focus();
            }
            if done && let Some((_, text, _)) = self.customize.alias_edit.take() {
                self.take_aliases(c, &text);
            }
        } else {
            let text = self.keymap.aliases_of(c).join(", ");
            let shown = if text.is_empty() {
                "—".to_owned()
            } else {
                text.clone()
            };
            if ui
                .add(egui::Button::new(shown).frame(false))
                .on_hover_text("Click to type aliases (letters, separated by commas)")
                .clicked()
            {
                self.customize.alias_edit = Some((c.id, text, true));
            }
        }
        // Shortcut: a click waits for the keys.
        let waiting = self.customize.capture == Some(c.id);
        let key = if waiting {
            "Press keys...".to_owned()
        } else {
            self.keymap.key_of(c).unwrap_or_else(|| "—".into())
        };
        if ui
            .selectable_label(waiting, key)
            .on_hover_text("Click, then press the key combination (Backspace removes, Esc cancels)")
            .clicked()
        {
            self.customize.capture = Some(c.id);
            self.customize.message = Some(format!("{}: press the keys", c.name));
        }
        if ui
            .add_enabled(changed, egui::Button::new("Reset").small())
            .on_hover_text("Back to the default keys")
            .clicked()
        {
            self.keymap.reset(c.id);
            self.keymap_changed();
        }
        ui.end_row();
    }

    /// Takes typed aliases for `c`, refusing ones another command of its
    /// environment has.
    fn take_aliases(&mut self, c: &'static Command, text: &str) {
        let env = home_env(c);
        let taken: Vec<String> = text
            .split([',', ' '])
            .map(|a| a.trim().to_ascii_uppercase())
            .filter(|a| !a.is_empty())
            .filter_map(|a| {
                self.keymap
                    .alias_owner(&a, env, c.id)
                    .map(|o| format!("{a} ({})", o.name))
            })
            .collect();
        if !taken.is_empty() {
            self.customize.message =
                Some(format!("{}: already used: {}", c.name, taken.join(", ")));
            return;
        }
        if text
            .split([',', ' '])
            .any(|a| !a.trim().chars().all(|ch| ch.is_ascii_alphanumeric()))
        {
            self.customize.message = Some(format!("{}: aliases are letters and digits", c.name));
            return;
        }
        self.keymap.set_aliases(c, text);
        self.customize.message = Some(format!(
            "{}: aliases {}",
            c.name,
            self.keymap.aliases_of(c).join(", ")
        ));
        self.keymap_changed();
    }

    fn customize_buttons(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        if let Some(text) = self.customize.import.as_mut() {
            ui.label("Paste the keys (JSON) and press Import:");
            ui.add(
                egui::TextEdit::multiline(text)
                    .desired_rows(4)
                    .desired_width(f32::INFINITY),
            );
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.keymap != Keymap::default(),
                    egui::Button::new("Reset All"),
                )
                .on_hover_text("Every command back to its default keys")
                .clicked()
            {
                self.keymap = Keymap::default();
                self.customize.message = Some("All keys are the defaults again".into());
                self.keymap_changed();
            }
            if ui
                .button("Copy to Clipboard")
                .on_hover_text("Your changed keys as JSON (to keep or move to another computer)")
                .clicked()
            {
                let text = serde_json::to_string_pretty(&self.keymap).unwrap_or_default();
                ui.ctx().copy_text(text);
                self.customize.message = Some("Your keys are on the clipboard".into());
            }
            match self.customize.import.take() {
                None => {
                    if ui.button("Import...").clicked() {
                        self.customize.import = Some(String::new());
                    }
                }
                Some(text) => {
                    if ui.button("Import").clicked() {
                        match serde_json::from_str::<Keymap>(&text) {
                            Ok(k) => {
                                self.keymap = k;
                                self.customize.message = Some("Keys imported".into());
                                self.keymap_changed();
                            }
                            Err(e) => {
                                self.customize.message = Some(format!("Import: {e}"));
                                self.customize.import = Some(text);
                            }
                        }
                    } else if ui.button("Cancel").clicked() {
                    } else {
                        self.customize.import = Some(text);
                    }
                }
            }
            if ui.button("Close").clicked() {
                self.customize.open = false;
                self.customize.capture = None;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(id: &str) -> &'static Command {
        match commands::by_id(id) {
            Some(c) => c,
            None => panic!("no command {id}"),
        }
    }

    #[test]
    fn only_differences_are_stored() {
        let mut k = Keymap::default();
        let e = cmd("part.extrude");
        k.set_aliases(e, "e");
        assert!(k.aliases.is_empty(), "the default text drops the override");
        k.set_aliases(e, "EX, E");
        assert_eq!(k.aliases_of(e), vec!["EX", "E"]);
        k.set_key(e, Some("Ctrl+Shift+E".into()));
        assert_eq!(k.key_of(e).as_deref(), Some("Ctrl+Shift+E"));
        k.reset(e.id);
        assert_eq!(k, Keymap::default());
        // An empty list removes the defaults.
        k.set_aliases(e, "");
        assert!(k.aliases_of(e).is_empty());
    }

    #[test]
    fn chords_and_aliases_are_told_apart() {
        assert_eq!(chord_text(Modifiers::NONE, Key::E), None);
        assert_eq!(chord_text(Modifiers::SHIFT, Key::E), None);
        assert_eq!(
            chord_text(Modifiers::CTRL | Modifiers::SHIFT, Key::E).as_deref(),
            Some("Ctrl+Shift+E")
        );
        assert_eq!(chord_text(Modifiers::NONE, Key::F4).as_deref(), Some("F4"));
        assert_eq!(
            chord_text(Modifiers::ALT, Key::Num1).as_deref(),
            Some("Alt+1")
        );
    }

    #[test]
    fn user_keys_find_their_command_and_conflicts() {
        let mut k = Keymap::default();
        let f = cmd("part.fillet");
        k.set_key(f, Some("Ctrl+Shift+F".into()));
        assert_eq!(
            k.command_for_key("Ctrl+Shift+F", Env::Part).map(|c| c.id),
            Some(f.id)
        );
        assert!(k.command_for_key("Ctrl+Shift+F", Env::Drawing).is_none());
        assert_eq!(
            k.key_owner("Ctrl+Shift+F", Env::Part, "x").map(|c| c.id),
            Some(f.id)
        );
        // Extrude's E is taken in a part.
        assert_eq!(
            k.alias_owner("e", Env::Part, f.id).map(|c| c.id),
            Some("part.extrude")
        );
        // Serde keeps only what changed.
        let json = serde_json::to_string(&k).unwrap_or_default();
        assert_eq!(json, r#"{"keys":{"part.fillet":"Ctrl+Shift+F"}}"#);
    }
    /// The alias field and Alt+S find a command by the user's alias and by
    /// its whole name alike.
    #[test]
    fn search_takes_user_aliases_and_names() {
        let mut k = Keymap::default();
        let shell = cmd("part.shell");
        k.set_aliases(shell, "Q");
        let first = |q: &str, k: &Keymap| {
            commands::search_in(q, Env::Part, &[], k)
                .first()
                .map(|h| h.cmd.id)
        };
        assert_eq!(first("q", &k), Some("part.shell"));
        assert_eq!(first("shell", &k), Some("part.shell"));
        assert_eq!(first("sh", &Keymap::default()), Some("part.shell"));
        // Without the default alias the old one no longer finds it first.
        assert_ne!(first("sh", &k), Some("part.shell"));
        assert_eq!(first("fillet", &k), Some("part.fillet"));
    }
}
