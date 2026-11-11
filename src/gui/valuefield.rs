//! Numeric fields that take expressions (Inventor's edit boxes,
//! `docs/spec/0-ui.md` §2.7): `20*2`, `width + 5 mm`, `depth = 12`, with
//! the `>` menu (List Parameters, Recently Used, …).

use eframe::egui::{self, Color32, RichText};

use crate::kernel::Real;
use crate::part::expr::Unit;
use crate::part::params::{ParamRow, eval_field_in, format_value};
use crate::part::units::UnitDef;

/// What a field shows and checks against.
pub struct FieldSpec<'a> {
    /// Stable id (per dialog and field).
    pub id: egui::Id,
    /// Current value (internal units).
    pub value: Real,
    /// Unit of the field.
    pub unit: Unit,
    /// The field's parameter name, if it has one yet.
    pub name: Option<&'a str>,
    /// Its equation, if any.
    pub expr: Option<&'a str>,
    /// All parameters of the part (to evaluate and to list).
    pub table: &'a [ParamRow],
    /// A quantity other than a length or an angle (N, N·m, MPa): the value
    /// is in its unit's internal size and shown with its label.
    pub quantity: Option<Quantity>,
}

/// A field's unit by its label (`N`, `N·m`, `MPa`, `kW`): what the parser
/// checks the expression against, and what the field shows after the number.
#[derive(Clone, Copy, Debug)]
pub struct Quantity {
    /// The unit.
    pub def: UnitDef,
    /// Its label.
    pub label: &'static str,
}

impl Quantity {
    /// The unit named `label` (`parse_spec`); unknown labels are unitless
    /// with the label shown (`rpm`).
    pub fn of(label: &'static str) -> Quantity {
        Quantity {
            def: crate::part::units::parse_spec(label).unwrap_or_else(|_| UnitDef::from(Unit::Ul)),
            label,
        }
    }

    fn format(&self, v: Real) -> String {
        let d = self.def.to_display(v);
        let s = format!("{d:.6}");
        let s = s.trim_end_matches('0').trim_end_matches('.').to_owned();
        if self.label.is_empty() {
            s
        } else {
            format!("{s} {}", self.label)
        }
    }
}

impl FieldSpec<'_> {
    fn shown(&self, v: Real) -> String {
        match &self.quantity {
            Some(q) => q.format(v),
            None => format_value(v, self.unit),
        }
    }

    fn eval(&self, text: &str) -> Result<(Option<String>, Real, Option<String>), String> {
        match &self.quantity {
            Some(q) => eval_field_in(self.table, text, q.def, self.name),
            None => eval_field_in(self.table, text, self.unit, self.name),
        }
    }
}

/// The drag payload of a parameter's pick whip (Parameters table `@`).
#[derive(Clone, Debug)]
pub struct ParamDrag(pub String);

fn whip_id() -> egui::Id {
    egui::Id::new("param_whip_from")
}

/// A pick whip drag began at screen point `from`.
pub fn whip_start(ctx: &egui::Context, from: egui::Pos2) {
    ctx.data_mut(|d| d.insert_temp(whip_id(), from));
}

/// The whip line from the `@` to the pointer while a parameter is dragged.
pub fn draw_whip(ctx: &egui::Context) {
    if !egui::DragAndDrop::has_payload_of_type::<ParamDrag>(ctx) {
        return;
    }
    let (Some(from), Some(to)) = (
        ctx.data(|d| d.get_temp::<egui::Pos2>(whip_id())),
        ctx.pointer_latest_pos(),
    ) else {
        return;
    };
    let name = egui::DragAndDrop::payload::<ParamDrag>(ctx).map(|p| p.0.clone());
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, whip_id()));
    let col = Color32::from_rgb(230, 120, 20);
    p.line_segment([from, to], egui::Stroke::new(2.0, col));
    p.circle_filled(to, 4.0, col);
    if let Some(n) = name {
        p.text(
            to + egui::vec2(8.0, -8.0),
            egui::Align2::LEFT_BOTTOM,
            n,
            egui::FontId::proportional(13.0),
            col,
        );
    }
}

fn active_field_id() -> egui::Id {
    egui::Id::new("value_field_active")
}

/// The field that had the keyboard focus within the last 2 s (a click in
/// the view takes the focus away before it is seen as a click).
pub fn recent_field(ctx: &egui::Context, now: f64) -> Option<egui::Id> {
    let (id, t) = ctx.data(|d| d.get_temp::<(egui::Id, f64)>(active_field_id()))?;
    (now - t < 2.0).then_some(id)
}

/// Recently used values (shared by all fields).
fn recent_id() -> egui::Id {
    egui::Id::new("value_field_recent")
}

/// The recently used values, newest first (the `>` menu of every field).
pub fn recent(ctx: &egui::Context) -> Vec<String> {
    ctx.data(|d| d.get_temp(recent_id())).unwrap_or_default()
}

/// Notes a value as used (it heads the Recently Used list).
pub fn remember(ctx: &egui::Context, text: &str) {
    ctx.data_mut(|d| {
        let list = d.get_temp_mut_or_default::<Vec<String>>(recent_id());
        list.retain(|t| t != text);
        list.insert(0, text.to_owned());
        list.truncate(8);
    });
}

/// Draws the field (width ≈ 130 px incl. the `>` button); returns the
/// committed text (Enter / focus loss with a valid change, or a menu
/// choice). The caller applies it.
pub fn value_field(ui: &mut egui::Ui, spec: &FieldSpec) -> Option<String> {
    let shown = match spec.expr {
        Some(e) => e.to_owned(),
        None => spec.shown(spec.value),
    };
    let buf_id = spec.id.with("buf");
    let te_id = spec.id.with("te");
    // A value measured for this field (`>` → Measure) comes in as typed.
    let mut commit: Option<String> =
        ui.data_mut(|d| d.remove_temp(super::measure_pick::result_id(spec.id)));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let editing: Option<String> = ui.data(|d| d.get_temp(buf_id));
        let mut text = editing.clone().unwrap_or_else(|| shown.clone());
        let check = spec.eval(&text);
        let bad = editing.is_some() && check.is_err();
        let mut te = egui::TextEdit::singleline(&mut text)
            .id(te_id)
            .desired_width(100.0);
        if bad {
            te = te.text_color(super::theme::tokens().error);
        }
        let mut r = ui.add(te);
        // The field last typed in (a click on a dimension in the view then
        // puts its name here, `recent_field`).
        if r.has_focus() {
            let t = ui.input(|i| i.time);
            ui.data_mut(|d| d.insert_temp(active_field_id(), (spec.id, t)));
            // `if(` being typed: which argument, and the condition's state.
            super::if_builder::draw_hint(ui, r.rect, &text, spec.table);
        }
        // Pick whip: a parameter dropped here becomes the field's value.
        if r.dnd_hover_payload::<ParamDrag>().is_some() {
            ui.painter().rect_stroke(
                r.rect.expand(2.0),
                2.0,
                egui::Stroke::new(2.0, Color32::from_rgb(230, 120, 20)),
                egui::StrokeKind::Outside,
            );
        }
        if let Some(p) = r.dnd_release_payload::<ParamDrag>()
            && Some(p.0.as_str()) != spec.name
        {
            commit = Some(p.0.clone());
        }
        if r.gained_focus() {
            // Edit the equation (or the plain value), all selected.
            let start = shown.clone();
            ui.data_mut(|d| d.insert_temp(buf_id, start.clone()));
            if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), te_id) {
                let all = egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(start.chars().count()),
                );
                st.cursor.set_char_range(Some(all));
                st.store(ui.ctx(), te_id);
            }
        } else if r.changed() {
            ui.data_mut(|d| d.insert_temp(buf_id, text.clone()));
        }
        let hover = match (&check, editing.is_some()) {
            (Err(e), true) => format!("Invalid: {e}"),
            _ => {
                let v = spec.shown(spec.value);
                match spec.name {
                    Some(n) if !n.is_empty() => format!("{n} = {v}"),
                    _ => v,
                }
            }
        };
        r = r.on_hover_text(format!(
            "{hover}\nAccepts expressions: 20*2, width + 5 mm, name = 12"
        ));
        if r.lost_focus() {
            let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if !esc && editing.as_deref().is_some_and(|t| t != shown) && check.is_ok() {
                commit = editing.clone();
            }
            ui.data_mut(|d| d.remove::<String>(buf_id));
        }
        ui.menu_button("›", |ui| {
            ui.menu_button("List Parameters", |ui| {
                let mut any = false;
                for p in spec
                    .table
                    .iter()
                    .filter(|p| Some(p.name.as_str()) != spec.name)
                {
                    any = true;
                    let label = format!("{}   {}", p.name, format_value(p.value, p.unit));
                    if ui.button(label).clicked() {
                        commit = Some(p.name.clone());
                        ui.close();
                    }
                }
                if !any {
                    ui.label(RichText::new("No parameters yet").weak());
                }
            });
            let recent: Vec<String> = ui.data(|d| d.get_temp(recent_id())).unwrap_or_default();
            ui.menu_button("Recently Used", |ui| {
                if recent.is_empty() {
                    ui.label(RichText::new("Nothing yet").weak());
                }
                for t in recent {
                    if ui.button(&t).clicked() {
                        commit = Some(t);
                        ui.close();
                    }
                }
            });
            if ui
                .button("If...")
                .on_hover_text(
                    "A value that depends on a condition: if(condition, value, other value), built from lists",
                )
                .clicked()
            {
                super::if_builder::open(ui.ctx(), spec.id, &shown);
                ui.close();
            }
            if ui
                .button("Measure")
                .on_hover_text(
                    "Pick two points, a line, a circle, an edge or a face in the view and use the value",
                )
                .clicked()
            {
                let req = (spec.id, spec.unit);
                ui.data_mut(|d| d.insert_temp(super::measure_pick::request_id(), req));
                ui.close();
            }
            if ui
                .button("Show Dimensions")
                .on_hover_text("Click a feature on the model, then one of its dimensions: its name goes here")
                .clicked()
            {
                ui.data_mut(|d| d.insert_temp(super::show_dims::request_id(), spec.id));
                ui.close();
            }
        })
        .response
        .on_hover_text("Parameters, recent values");
    });
    // The If window of this field (`>` → If...).
    let title = spec.name.filter(|n| !n.is_empty()).unwrap_or("value");
    let eval = |t: &str| spec.eval(t).map(|(_, v, _)| spec.shown(v));
    if let Some(t) =
        super::if_builder::window(ui.ctx(), spec.id, title, spec.table, spec.name, &eval)
    {
        commit = Some(t);
    }
    if let Some(t) = &commit {
        remember(ui.ctx(), t);
    }
    commit
}

/// The part's parameters for [`ExprValue`] fields, set once per frame.
pub fn set_frame_table(ctx: &egui::Context, table: Vec<ParamRow>) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("frame_param_table"), table));
}

pub(super) fn frame_table(ctx: &egui::Context) -> Vec<ParamRow> {
    ctx.data(|d| d.get_temp(egui::Id::new("frame_param_table")))
        .unwrap_or_default()
}

/// A number an [`ExprValue`] edits.
pub trait ExprNumber: Copy {
    /// As a real.
    fn to_real(self) -> Real;
    /// From a real (counts round, at least 0).
    fn from_real(v: Real) -> Self;
}

impl ExprNumber for Real {
    fn to_real(self) -> Real {
        self
    }
    fn from_real(v: Real) -> Self {
        v
    }
}

impl ExprNumber for usize {
    fn to_real(self) -> Real {
        self as Real
    }
    fn from_real(v: Real) -> Self {
        v.round().max(0.0) as usize
    }
}

/// A drop-in for `egui::DragValue` on plain values (tool options, pattern
/// counts, …) that takes expressions and parameter names with the `>` menu
/// (`docs/spec/0-ui.md` §2.7): the result is evaluated when entered. The
/// unit follows the suffix (` mm` length, `°` an angle *in degrees*,
/// none unitless).
pub struct ExprValue<'a, T: ExprNumber> {
    value: &'a mut T,
    min: Real,
    max: Real,
    suffix: String,
    equation: Option<&'a mut Option<String>>,
    cursor: Option<&'a str>,
    quantity: Option<Quantity>,
}

impl<'a, T: ExprNumber> ExprValue<'a, T> {
    /// A field for `value`.
    pub fn new(value: &'a mut T) -> Self {
        Self {
            value,
            min: Real::NEG_INFINITY,
            max: Real::INFINITY,
            suffix: String::new(),
            equation: None,
            cursor: None,
            quantity: None,
        }
    }

    /// Also offered as `label` next to the cursor while its command is open
    /// (`cursor_value`).
    pub fn cursor(mut self, label: &'a str) -> Self {
        self.cursor = Some(label);
        self
    }

    /// Keeps the entered text when it names parameters (else clears it),
    /// for callers that store the equation with the result.
    pub fn equation(mut self, eq: &'a mut Option<String>) -> Self {
        self.equation = Some(eq);
        self
    }

    /// The allowed range (values outside are clamped).
    pub fn range(mut self, r: std::ops::RangeInclusive<T>) -> Self {
        self.min = r.start().to_real();
        self.max = r.end().to_real();
        self
    }

    /// Kept for `DragValue` compatibility (no dragging here).
    pub fn speed(self, _speed: Real) -> Self {
        self
    }

    /// The unit by its suffix: ` mm`, `°`.
    pub fn suffix(mut self, s: impl Into<String>) -> Self {
        self.suffix = s.into();
        self
    }

    /// A quantity by its unit's label (`N`, `N·m`, `MPa`, `kW`, `kg/m³`):
    /// the value is kept in that unit, shown with it, and typed values in
    /// other units of the same kind (`2 kN`, `150 N/mm^2`) are converted.
    pub fn unit(mut self, label: &'static str) -> Self {
        self.quantity = Some(Quantity::of(label));
        self
    }
}

impl<T: ExprNumber> egui::Widget for ExprValue<'_, T> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let deg = self.suffix.contains('°');
        let unit = if deg {
            Unit::Deg
        } else if self.suffix.contains("mm") {
            Unit::Mm
        } else {
            Unit::Ul
        };
        let shown = self.value.to_real();
        let quantity = self.quantity;
        // Angles of these fields are kept in degrees; the parser works in
        // radians. A quantity is kept in its own unit.
        let internal = match &quantity {
            Some(q) => q.def.from_display(shown),
            None if deg => shown.to_radians(),
            None => shown,
        };
        let table = frame_table(ui.ctx());
        let id = ui.next_auto_id();
        let spec = FieldSpec {
            id,
            value: internal,
            unit,
            name: None,
            expr: None,
            table: &table,
            quantity,
        };
        if let Some(label) = self.cursor {
            super::cursor_value::register(
                ui.ctx(),
                super::cursor_value::PanelField {
                    id,
                    label: label.to_owned(),
                    value: internal,
                    unit,
                    name: None,
                    expr: None,
                },
            );
        }
        let mut inner = ui.scope(|ui| value_field(ui, &spec));
        if let Some(text) = inner.inner
            && let Ok((_, v, _)) = eval_field_in(&table, &text, unit, None)
        {
            let v = match &quantity {
                Some(q) => q.def.to_display(v),
                None if deg => v.to_degrees(),
                None => v,
            };
            *self.value = T::from_real(v.clamp(self.min, self.max));
            if let Some(eq) = self.equation {
                let named = !crate::part::expr::names_in(&text).is_empty();
                *eq = named.then(|| text.trim().to_owned());
            }
            inner.response.mark_changed();
        }
        inner.response
    }
}

/// Focuses text field `r` with its whole text selected, so typing replaces
/// it (every value and name box opens this way; call it once, when the box
/// opens — re-requesting focus every frame swallows Enter).
pub(super) fn focus_all(ctx: &egui::Context, r: &egui::Response, text: &str) {
    r.request_focus();
    let mut st = egui::TextEdit::load_state(ctx, r.id).unwrap_or_default();
    st.cursor.set_char_range(Some(egui::text::CCursorRange::two(
        egui::text::CCursor::new(0),
        egui::text::CCursor::new(text.chars().count()),
    )));
    st.store(ctx, r.id);
}
