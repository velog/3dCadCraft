//! Numeric fields that take expressions (Inventor's edit boxes,
//! `docs/spec/0-ui.md` §2.7): `20*2`, `width + 5 mm`, `depth = 12`, with
//! the `>` menu (List Parameters, Recently Used, …).

use eframe::egui::{self, Color32, RichText};

use crate::kernel::Real;
use crate::part::expr::Unit;
use crate::part::params::{ParamRow, eval_field_in, format_value};

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
}

/// Recently used values (shared by all fields).
fn recent_id() -> egui::Id {
    egui::Id::new("value_field_recent")
}

fn remember(ctx: &egui::Context, text: &str) {
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
        None => format_value(spec.value, spec.unit),
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
        let check = eval_field_in(spec.table, &text, spec.unit, spec.name);
        let bad = editing.is_some() && check.is_err();
        let mut te = egui::TextEdit::singleline(&mut text)
            .id(te_id)
            .desired_width(100.0);
        if bad {
            te = te.text_color(Color32::from_rgb(230, 70, 60));
        }
        let mut r = ui.add(te);
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
                let v = format_value(spec.value, spec.unit);
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
        ui.menu_button(">", |ui| {
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
                .button("Measure")
                .on_hover_text(
                    "Pick two points, a line, a circle, an edge or a face in the view and use                      the value",
                )
                .clicked()
            {
                let req = (spec.id, spec.unit);
                ui.data_mut(|d| d.insert_temp(super::measure_pick::request_id(), req));
                ui.close();
            }
            ui.add_enabled(false, egui::Button::new("Show Dimensions"))
                .on_disabled_hover_text("Pick a dimension in the graphics window (planned)");
        })
        .response
        .on_hover_text("Parameters, recent values");
    });
    if let Some(t) = &commit {
        remember(ui.ctx(), t);
    }
    commit
}

/// The part's parameters for [`ExprValue`] fields, set once per frame.
pub fn set_frame_table(ctx: &egui::Context, table: Vec<ParamRow>) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("frame_param_table"), table));
}

fn frame_table(ctx: &egui::Context) -> Vec<ParamRow> {
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
        }
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
        // Angles of these fields are kept in degrees; the parser works in
        // radians.
        let internal = if deg { shown.to_radians() } else { shown };
        let table = frame_table(ui.ctx());
        let id = ui.next_auto_id();
        let spec = FieldSpec {
            id,
            value: internal,
            unit,
            name: None,
            expr: None,
            table: &table,
        };
        let mut inner = ui.scope(|ui| value_field(ui, &spec));
        if let Some(text) = inner.inner
            && let Ok((_, v, _)) = eval_field_in(&table, &text, unit, None)
        {
            let v = if deg { v.to_degrees() } else { v };
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
