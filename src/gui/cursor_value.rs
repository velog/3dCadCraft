//! The value next to the cursor (`docs/spec/0-ui.md` §4.2, owner's rule 21):
//! an open command's value (Distance A, Radius…) follows the mouse, typing a
//! number opens its editor there, without a click and without moving to the
//! panel. The editor is the panel's own field: every `field_row` registers
//! here each frame ([`register`]) and a commit comes back through the same
//! door as a measured value (`measure_pick::result_id`), so equations,
//! parameters, the `>` menu and Undo work as in the panel and no command
//! needs code of its own.

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke};

use super::CadCraftApp;
use super::valuefield::{self, FieldSpec};
use crate::part::expr::Unit;

/// One value of the open panel.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct PanelField {
    /// The panel field's id.
    pub id: egui::Id,
    /// Its label ("Distance A").
    pub label: String,
    /// Its value (internal units).
    pub value: f64,
    /// Its unit.
    pub unit: Unit,
    /// Its parameter's name.
    pub name: Option<String>,
    /// Its equation.
    pub expr: Option<String>,
}

/// The editor's state.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct CursorValue {
    /// The open editor's field (index into the panel's values).
    pub step: Option<usize>,
    /// The field the label next to the cursor shows.
    pub current: usize,
    /// Where the editor stands (it stops following, so it can be clicked).
    pub at: Option<Pos2>,
    /// Typed before the editor opened (the first digit).
    pub seed: Option<String>,
    /// Focus still to be given.
    pub focus: bool,
    /// Tab (+1) / Shift+Tab (-1) pressed, taken before egui moves the focus.
    pub tab: i32,
    /// Esc pressed in the editor: it closes and forgets what was typed.
    pub esc: bool,
    /// The pointer is over the view (typing goes to the value).
    pub in_view: bool,
}

fn list_id() -> egui::Id {
    egui::Id::new("panel_fields")
}

/// Forgets last frame's values (start of `properties`).
pub(super) fn begin(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove_temp::<Vec<PanelField>>(list_id()));
}

/// Adds a value of the panel being drawn.
pub(super) fn register(ctx: &egui::Context, f: PanelField) {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Vec<PanelField>>(list_id())
            .push(f);
    });
}

/// The values of the open panel, in its order.
pub(super) fn fields(ctx: &egui::Context) -> Vec<PanelField> {
    ctx.data(|d| d.get_temp(list_id())).unwrap_or_default()
}

/// A character that starts a value without a click: never a letter, those
/// are the command aliases.
pub(super) fn starts_value(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit() || c == '.' || c == '-')
}

/// The next field index after `k` of `n`, or `None` past either end.
pub(super) fn stepped(k: usize, n: usize, by: i32) -> Option<usize> {
    let to = k as i64 + i64::from(by);
    (to >= 0 && to < n as i64).then_some(to as usize)
}

impl CadCraftApp {
    /// Whether the value may show or take typing now: a part's command is
    /// open and it has values. A sketch has Dynamic Input instead.
    pub(super) fn cursor_value_on(&self, ctx: &egui::Context) -> bool {
        self.cursor_value_enabled
            && self.editing.is_none()
            && self.open_state().command_open()
            && !fields(ctx).is_empty()
    }

    /// Opens the editor on field `k` at the cursor.
    pub(super) fn cursor_value_open(&mut self, ctx: &egui::Context, k: usize, rect: Option<Rect>) {
        let n = fields(ctx).len();
        if n == 0 {
            return;
        }
        let k = k.min(n - 1);
        self.cursor_value_commit(ctx);
        self.cv.step = Some(k);
        self.cv.current = k;
        self.cv.focus = true;
        let p = ctx
            .pointer_latest_pos()
            .or(self.cv.at)
            .unwrap_or(ctx.content_rect().center());
        let at = p + egui::vec2(18.0, 20.0);
        self.cv.at = Some(match rect {
            Some(r) => egui::pos2(at.x.min(r.right() - 330.0), at.y.min(r.bottom() - 40.0)),
            None => at,
        });
    }

    /// Closes the editor (what was typed stays in its field's commit).
    pub(super) fn cursor_value_close(&mut self) {
        self.cv.step = None;
        self.cv.seed = None;
        self.cv.focus = false;
    }

    /// Gives the panel's field what the open editor holds.
    fn cursor_value_commit(&self, ctx: &egui::Context) {
        let Some(k) = self.cv.step else { return };
        let Some(f) = fields(ctx).get(k).cloned() else {
            return;
        };
        let buf = editor_id(f.id).with("buf");
        if let Some(text) = ctx.data_mut(|d| d.remove_temp::<String>(buf)) {
            forward(ctx, f.id, text);
        }
    }

    /// Draws the label next to the cursor, or the open editor. Called once
    /// a frame by the views, after the command's own hints.
    pub(super) fn draw_cursor_value(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        rect: Rect,
        pointer: Option<Pos2>,
    ) {
        self.cv.in_view = pointer.is_some_and(|p| rect.contains(p));
        if !self.cursor_value_on(ctx) {
            self.cursor_value_close();
            self.cv.current = 0;
            self.cv.tab = 0;
            return;
        }
        let list = fields(ctx);
        // The first digit typed over the view opens the editor with it.
        if self.cv.seed.is_some() && self.cv.step.is_none() {
            let seed = self.cv.seed.take();
            self.cursor_value_open(ctx, self.cv.current, Some(rect));
            self.cv.seed = seed;
        }
        if std::mem::take(&mut self.cv.esc) {
            if let Some(f) = self.cv.step.and_then(|k| list.get(k)) {
                let id = editor_id(f.id);
                ctx.data_mut(|d| d.remove_temp::<String>(id.with("buf")));
                ctx.memory_mut(|m| m.surrender_focus(id.with("te")));
            }
            self.cursor_value_close();
            return;
        }
        let tab = std::mem::take(&mut self.cv.tab);
        if tab != 0
            && let Some(k) = self.cv.step
            && let Some(to) = stepped(k, list.len(), tab)
        {
            self.cursor_value_open(ctx, to, Some(rect));
        }
        match self.cv.step {
            Some(k) => {
                let Some(f) = list.get(k) else {
                    self.cursor_value_close();
                    return;
                };
                self.cursor_value_editor(ctx, f, rect);
            }
            None => {
                let ready = self.step_flow().is_none_or(|flow| flow.ready());
                let busy = ctx.input(|i| i.pointer.any_down());
                if let (Some(p), true, false) = (pointer, ready, busy)
                    && let Some(f) = list.get(self.cv.current.min(list.len() - 1))
                {
                    value_label(painter, rect, p, f);
                }
            }
        }
    }

    fn cursor_value_editor(&mut self, ctx: &egui::Context, f: &PanelField, rect: Rect) {
        let id = editor_id(f.id);
        let te = id.with("te");
        // Before the field draws: a first digit replaces the value.
        if let Some(seed) = self.cv.seed.take() {
            ctx.data_mut(|d| d.insert_temp(id.with("buf"), seed));
        }
        let at = self.cv.at.unwrap_or(rect.center());
        let table = valuefield::frame_table(ctx);
        let flip = self.flip_available();
        let mut flip_clicked = false;
        let mut commit = None;
        let shown = egui::Area::new(egui::Id::new("cursor_value"))
            .fixed_pos(at)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&f.label).small());
                        let spec = FieldSpec {
                            id,
                            value: f.value,
                            unit: f.unit,
                            name: f.name.as_deref(),
                            expr: f.expr.as_deref(),
                            table: &table,
                        };
                        commit = valuefield::value_field(ui, &spec);
                        if flip
                            && ui
                                .small_button("Flip")
                                .on_hover_text("Flip the direction")
                                .clicked()
                        {
                            flip_clicked = true;
                        }
                    });
                });
            });
        if std::mem::take(&mut self.cv.focus) {
            let typed: Option<String> = ctx.data(|d| d.get_temp(id.with("buf")));
            let n = typed
                .clone()
                .or_else(|| f.expr.clone())
                .unwrap_or_else(|| crate::part::params::format_value(f.value, f.unit))
                .chars()
                .count();
            let mut st = egui::TextEdit::load_state(ctx, te).unwrap_or_default();
            // A digit typed goes on; a field reached by Next is replaced.
            let from = if typed.is_some() { n } else { 0 };
            st.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(from),
                egui::text::CCursor::new(n),
            )));
            st.store(ctx, te);
            ctx.memory_mut(|m| m.request_focus(te));
        }
        if let Some(text) = commit {
            forward(ctx, f.id, text);
        }
        if flip_clicked {
            self.flip_direction();
        }
        // Enter ends the editing; a click elsewhere too, unless it is in the
        // field's own `>` menu.
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        let outside = ctx.input(|i| i.pointer.any_pressed())
            && ctx
                .pointer_latest_pos()
                .is_some_and(|p| !shown.response.rect.contains(p))
            && !egui::Popup::is_any_open(ctx);
        if enter || outside {
            self.cursor_value_close();
        }
    }
}

/// The editor's own field id (a second widget beside the panel's).
fn editor_id(panel: egui::Id) -> egui::Id {
    panel.with("cursor")
}

/// A value for panel field `id`: it takes it as if typed there.
fn forward(ctx: &egui::Context, id: egui::Id, text: String) {
    ctx.data_mut(|d| d.insert_temp(super::measure_pick::result_id(id), text));
}

/// "Distance A: 10 mm" below the cursor's prompt.
fn value_label(painter: &egui::Painter, rect: Rect, pos: Pos2, f: &PanelField) {
    if !rect.contains(pos) {
        return;
    }
    let value = f
        .expr
        .clone()
        .unwrap_or_else(|| crate::part::params::format_value(f.value, f.unit));
    let text = format!("{}: {value}", f.label);
    let galley = painter.layout_no_wrap(
        text,
        egui::FontId::proportional(12.0),
        Color32::from_gray(240),
    );
    let mut at = pos + egui::vec2(18.0, 42.0);
    if at.x + galley.size().x + 8.0 > rect.right() {
        at.x = pos.x - galley.size().x - 18.0;
    }
    if at.y + galley.size().y + 6.0 > rect.bottom() {
        at.y = pos.y - galley.size().y - 40.0;
    }
    let bg = Rect::from_min_size(
        at - egui::vec2(4.0, 2.0),
        galley.size() + egui::vec2(8.0, 4.0),
    );
    painter.rect_filled(bg, 3.0, Color32::from_rgba_unmultiplied(40, 60, 90, 235));
    painter.rect_stroke(
        bg,
        3.0,
        Stroke::new(1.0, Color32::from_gray(120)),
        egui::StrokeKind::Inside,
    );
    painter.galley(at, galley, Color32::from_gray(240));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Letters stay the aliases' (E, F, H…); a value starts with a digit,
    /// a point or a sign.
    #[test]
    fn only_numbers_start_a_value() {
        for t in ["0", "7", ".5", "-3"] {
            assert!(starts_value(t), "{t}");
        }
        for t in ["e", "F", " ", "", "ф"] {
            assert!(!starts_value(t), "{t}");
        }
    }

    /// It runs in every frame of an open command (rule 20: a frame is 16 ms
    /// and this must not show in it). Generous for loaded debug builds.
    #[test]
    fn the_panel_list_costs_nothing_in_a_frame() {
        let ctx = egui::Context::default();
        let t0 = std::time::Instant::now();
        const FRAMES: u32 = 1000;
        for _ in 0..FRAMES {
            begin(&ctx);
            for k in 0..30 {
                register(
                    &ctx,
                    PanelField {
                        id: egui::Id::new(k),
                        label: "Distance A".into(),
                        value: 10.0,
                        unit: Unit::Mm,
                        name: Some("d0".into()),
                        expr: Some("width / 2".into()),
                    },
                );
            }
            assert_eq!(fields(&ctx).len(), 30);
        }
        let ms = t0.elapsed().as_secs_f64() * 1000.0 / f64::from(FRAMES);
        assert!(ms < 1.0, "{ms} ms a frame");
    }

    #[test]
    fn tab_stops_at_both_ends() {
        assert_eq!(stepped(0, 3, 1), Some(1));
        assert_eq!(stepped(2, 3, 1), None);
        assert_eq!(stepped(0, 3, -1), None);
    }
}
