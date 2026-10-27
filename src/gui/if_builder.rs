//! Decisions without typing their syntax (`docs/spec/0.5-parameters.md`,
//! "Decisions in equations" and "Suppress if"): a condition builder — rows
//! of *parameter · comparison · value* joined by `and` / `or` — and the
//! **If** window that puts `if(condition, value, other)` into a field, plus
//! the Excel-like hint under a field while `if(` is being typed. The text
//! stays the source of truth: the builder reads a typed condition back
//! when it can, and falls back to the text otherwise.

use eframe::egui::{self, Color32, RichText};

use crate::part::expr::{self, Unit};
use crate::part::params::ParamRow;
use crate::part::units::Logic;

/// The comparisons of a row; the last two need no value.
pub(super) const OPS: [&str; 8] = ["==", "!=", "<", "<=", ">", ">=", "is true", "is false"];
const IS_TRUE: usize = 6;
const IS_FALSE: usize = 7;

/// A parameter the builder offers: its name, kind and unit label.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Choice {
    pub name: String,
    pub logic: Logic,
    pub unit: String,
    /// What the list shows after the name: the value now and where the
    /// parameter comes from (`92 mm · Base`).
    pub about: String,
}

/// The parameters of `table` the builder offers (all but `own`).
pub(super) fn choices(table: &[ParamRow], own: Option<&str>) -> Vec<Choice> {
    table
        .iter()
        .filter(|r| Some(r.name.as_str()) != own)
        .map(|r| {
            let value = match r.as_value() {
                expr::Value::Bool(b) => b.to_string(),
                expr::Value::Text(t) => format!("\"{t}\""),
                expr::Value::Num(v, _) => {
                    crate::part::params::format_in(v, r.def(), &r.unit_label())
                }
            };
            let from = if r.consumed_by.is_empty() {
                r.comment.clone()
            } else {
                r.consumed_by.clone()
            };
            Choice {
                name: r.name.clone(),
                logic: r.def().logic,
                unit: r.unit_label(),
                about: if from.is_empty() {
                    value
                } else {
                    format!("{value} · {from}")
                },
            }
        })
        .collect()
}

/// One row: `name op value`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Cond {
    pub name: String,
    pub op: usize,
    pub value: String,
}

impl Cond {
    fn text(&self, names: &[Choice]) -> Option<String> {
        if self.name.trim().is_empty() {
            return None;
        }
        let n = self.name.trim();
        let logic = names.iter().find(|c| c.name == n).map(|c| c.logic);
        Some(match self.op {
            IS_TRUE => n.to_owned(),
            IS_FALSE => format!("not {n}"),
            op => {
                let v = self.value.trim();
                // A text parameter compared with a word that is no
                // parameter: quote it (`Material == "Al"`).
                let v = if logic == Some(Logic::Text)
                    && !v.is_empty()
                    && !v.starts_with('"')
                    && !names.iter().any(|c| c.name == v)
                {
                    format!("\"{v}\"")
                } else {
                    v.to_owned()
                };
                format!("{n} {} {v}", OPS[op])
            }
        })
    }
}

/// A condition as rows, or as typed text when it does not fit rows.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct CondBuilder {
    pub rows: Vec<Cond>,
    /// Between row k and k + 1: `or` (true) or `and` (false).
    pub ors: Vec<bool>,
    /// Edit the text itself.
    pub text_mode: bool,
    pub text: String,
}

/// Splits `s` at the top level (outside brackets and quotes) where `at`
/// says a separator starts; returns the pieces and the separators.
fn split_top<'a>(s: &'a str, at: &dyn Fn(&str) -> Option<usize>) -> (Vec<&'a str>, Vec<&'a str>) {
    let mut parts = Vec::new();
    let mut seps = Vec::new();
    let (mut depth, mut quote, mut start, mut i) = (0i32, false, 0usize, 0usize);
    let b = s.as_bytes();
    while i < b.len() {
        let c = b[i] as char;
        if c == '"' {
            quote = !quote;
        } else if !quote && c == '(' {
            depth += 1;
        } else if !quote && c == ')' {
            depth -= 1;
        } else if !quote
            && depth == 0
            && s.is_char_boundary(i)
            && let Some(n) = at(&s[i..])
        {
            parts.push(&s[start..i]);
            seps.push(&s[i..i + n]);
            i += n;
            start = i;
            continue;
        }
        i += 1;
    }
    parts.push(&s[start..]);
    (parts, seps)
}

impl CondBuilder {
    /// The builder for a typed condition: rows when every piece is
    /// `name op value`, `name` or `not name` joined by and / or; the text
    /// otherwise.
    pub(super) fn from_text(text: &str) -> Self {
        let t = text.trim();
        let mut b = Self {
            text: t.to_owned(),
            ..Self::default()
        };
        if t.is_empty() {
            b.rows.push(Cond::default());
            return b;
        }
        let word = |rest: &str| {
            for w in [" and ", " or "] {
                if rest.starts_with(w) {
                    return Some(w.len());
                }
            }
            None
        };
        let (parts, seps) = split_top(t, &word);
        let mut rows = Vec::new();
        for p in &parts {
            let p = p.trim();
            if let Some(n) = p.strip_prefix("not ").map(str::trim)
                && expr::valid_name(n)
            {
                rows.push(Cond {
                    name: n.to_owned(),
                    op: IS_FALSE,
                    value: String::new(),
                });
                continue;
            }
            if expr::valid_name(p) {
                rows.push(Cond {
                    name: p.to_owned(),
                    op: IS_TRUE,
                    value: String::new(),
                });
                continue;
            }
            let op_at = |rest: &str| {
                ["==", "!=", "<=", ">=", "<", ">"]
                    .iter()
                    .find(|o| rest.starts_with(**o))
                    .map(|o| o.len())
            };
            let (sides, ops) = split_top(p, &op_at);
            match (sides.as_slice(), ops.as_slice()) {
                ([l, r], [o]) if expr::valid_name(l.trim()) && !r.trim().is_empty() => {
                    rows.push(Cond {
                        name: l.trim().to_owned(),
                        op: OPS.iter().position(|x| x == o).unwrap_or(0),
                        value: r.trim().to_owned(),
                    });
                }
                _ => {
                    b.text_mode = true;
                    return b;
                }
            }
        }
        b.ors = seps.iter().map(|s| s.trim() == "or").collect();
        b.rows = rows;
        b
    }

    /// The condition's text.
    pub(super) fn text(&self, names: &[Choice]) -> String {
        if self.text_mode {
            return self.text.trim().to_owned();
        }
        let mut out = String::new();
        for (k, r) in self.rows.iter().enumerate() {
            let Some(t) = r.text(names) else {
                continue;
            };
            if !out.is_empty() {
                let or = k > 0 && self.ors.get(k - 1).copied().unwrap_or(false);
                out.push_str(if or { " or " } else { " and " });
            }
            out.push_str(&t);
        }
        out
    }

    /// Draws the rows (or the text); `salt` keeps ids apart.
    pub(super) fn ui(&mut self, ui: &mut egui::Ui, names: &[Choice], salt: egui::Id) {
        let mut text_mode = self.text_mode;
        if ui
            .checkbox(&mut text_mode, "Type the condition")
            .on_hover_text("and, or, not, < > <= >= == !=, if(), true / false and text parameters")
            .changed()
        {
            if text_mode {
                self.text = self.text(names);
            } else {
                *self = Self::from_text(&self.text);
                if self.text_mode {
                    // It does not fit rows: stays text.
                    return;
                }
            }
            self.text_mode = text_mode;
        }
        if self.text_mode {
            ui.add(
                egui::TextEdit::singleline(&mut self.text)
                    .desired_width(320.0)
                    .hint_text("not Heavy    Width < 100 mm    Material == \"Al\""),
            );
            return;
        }
        if self.rows.is_empty() {
            self.rows.push(Cond::default());
        }
        self.ors.resize(self.rows.len().saturating_sub(1), false);
        let mut remove = None;
        egui::Grid::new(salt.with("rows"))
            .num_columns(4)
            .spacing([6.0, 4.0])
            .show(ui, |ui| {
                for k in 0..self.rows.len() {
                    if k > 0 {
                        let mut or = self.ors[k - 1];
                        egui::ComboBox::from_id_salt(salt.with(("join", k)))
                            .width(50.0)
                            .selected_text(if or { "or" } else { "and" })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut or, false, "and");
                                ui.selectable_value(&mut or, true, "or");
                            });
                        self.ors[k - 1] = or;
                    } else {
                        ui.label("If");
                    }
                    let row = &mut self.rows[k];
                    let logic = names.iter().find(|c| c.name == row.name).map(|c| c.logic);
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt(salt.with(("name", k)))
                            .width(110.0)
                            .height(320.0)
                            .selected_text(if row.name.is_empty() {
                                "parameter".to_owned()
                            } else {
                                row.name.clone()
                            })
                            .show_ui(ui, |ui| {
                                for c in names {
                                    let label = format!("{}   {}", c.name, c.about);
                                    if ui.selectable_label(row.name == c.name, label).clicked() {
                                        row.name = c.name.clone();
                                        // A sensible comparison for the kind.
                                        row.op = match c.logic {
                                            Logic::Bool => IS_TRUE,
                                            Logic::Text => 0,
                                            Logic::Num if row.op >= IS_TRUE => 2,
                                            Logic::Num => row.op,
                                        };
                                    }
                                }
                            });
                        let ops: &[usize] = match logic {
                            Some(Logic::Bool) => &[IS_TRUE, IS_FALSE, 0, 1],
                            Some(Logic::Text) => &[0, 1],
                            _ => &[0, 1, 2, 3, 4, 5],
                        };
                        egui::ComboBox::from_id_salt(salt.with(("op", k)))
                            .width(64.0)
                            .selected_text(OPS[row.op.min(OPS.len() - 1)])
                            .show_ui(ui, |ui| {
                                for &o in ops {
                                    ui.selectable_value(&mut row.op, o, OPS[o]);
                                }
                            });
                    });
                    if row.op < IS_TRUE {
                        let hint = match logic {
                            Some(Logic::Text) => "Al".to_owned(),
                            Some(Logic::Bool) => "true".to_owned(),
                            Some(Logic::Num) => names
                                .iter()
                                .find(|c| c.name == row.name)
                                .map_or_else(|| "100".to_owned(), |c| format!("100 {}", c.unit)),
                            None => "value".to_owned(),
                        };
                        ui.add(
                            egui::TextEdit::singleline(&mut row.value)
                                .desired_width(110.0)
                                .hint_text(hint),
                        )
                        .on_hover_text("A value with its unit, another parameter or an expression");
                    } else {
                        ui.label("");
                    }
                    if self.rows.len() > 1
                        && ui
                            .small_button("x")
                            .on_hover_text("Remove this row")
                            .clicked()
                    {
                        remove = Some(k);
                    }
                    ui.end_row();
                }
            });
        if let Some(k) = remove {
            self.rows.remove(k);
            if !self.ors.is_empty() {
                self.ors.remove(k.min(self.ors.len() - 1));
            }
        }
        ui.horizontal(|ui| {
            if ui.small_button("+ and").clicked() {
                self.rows.push(Cond::default());
                self.ors.push(false);
            }
            if ui.small_button("+ or").clicked() {
                self.rows.push(Cond::default());
                self.ors.push(true);
            }
        });
    }
}

/// The If window's state: a condition and both values.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct IfBuilder {
    pub cond: CondBuilder,
    pub then: String,
    pub other: String,
}

/// The three arguments of a text that is one `if(a, b, c)` call.
fn if_args(text: &str) -> Option<(String, String, String)> {
    let t = text.trim();
    let inner = t.strip_prefix("if(")?.strip_suffix(')')?;
    // The closing bracket must be the call's own.
    let (parts, _) = split_top(inner, &|rest| rest.starts_with(',').then_some(1));
    let mut depth = 0i32;
    for c in inner.chars() {
        depth += i32::from(c == '(') - i32::from(c == ')');
        if depth < 0 {
            return None;
        }
    }
    match parts.as_slice() {
        [a, b, c] => Some((a.trim().into(), b.trim().into(), c.trim().into())),
        _ => None,
    }
}

impl IfBuilder {
    /// The builder for a field's text: its `if()` read back, or the text
    /// as both values.
    pub(super) fn from_expr(text: &str) -> Self {
        match if_args(text) {
            Some((c, a, b)) => Self {
                cond: CondBuilder::from_text(&c),
                then: a,
                other: b,
            },
            None => Self {
                cond: CondBuilder::from_text(""),
                then: text.trim().to_owned(),
                other: text.trim().to_owned(),
            },
        }
    }

    /// The equation.
    pub(super) fn expr(&self, names: &[Choice]) -> String {
        format!(
            "if({}, {}, {})",
            self.cond.text(names),
            self.then.trim(),
            self.other.trim()
        )
    }
}

/// What an evaluation shows: the value, or why it fails.
pub(super) type Eval<'a> = &'a dyn Fn(&str) -> Result<String, String>;

/// A condition's current state with the parameters of `table`.
pub(super) fn condition_now(table: &[ParamRow], text: &str) -> Result<bool, String> {
    match expr::eval_value(text, Unit::Bool, &|n| {
        table.iter().find(|r| r.name == n).map(ParamRow::as_value)
    })? {
        (expr::Value::Bool(b), _) => Ok(b),
        _ => Err("the condition must be true or false".into()),
    }
}

/// Whether the builder still waits for its first complete row.
pub(super) fn unfinished(b: &CondBuilder) -> bool {
    !b.text_mode
        && b.rows
            .iter()
            .all(|r| r.name.trim().is_empty() || (r.op < IS_TRUE && r.value.trim().is_empty()))
}

/// The line that says whether a condition holds now.
pub(super) fn state_label(
    ui: &mut egui::Ui,
    state: &Result<bool, String>,
    if_true: &str,
    if_false: &str,
) {
    match state {
        Ok(true) => ui.label(
            RichText::new(format!("Now: true — {if_true}")).color(Color32::from_rgb(60, 170, 90)),
        ),
        Ok(false) => ui.label(RichText::new(format!("Now: false — {if_false}")).weak()),
        Err(e) => ui.label(
            RichText::new(format!("Not a condition: {e}")).color(Color32::from_rgb(230, 70, 60)),
        ),
    };
}

fn state_id(id: egui::Id) -> egui::Id {
    id.with("if_builder")
}

/// Whether the If window of field `id` is open.
pub(super) fn is_open(ctx: &egui::Context, id: egui::Id) -> bool {
    ctx.data(|d| d.get_temp::<IfBuilder>(state_id(id)).is_some())
}

/// Opens the If window for the field `id` with its current text.
pub(super) fn open(ctx: &egui::Context, id: egui::Id, current: &str) {
    ctx.data_mut(|d| d.insert_temp(state_id(id), IfBuilder::from_expr(current)));
}

/// Draws the If window of field `id` while it is open; returns the
/// equation when Insert was pressed. `value` evaluates the equation as the
/// field would (its value or the error).
pub(super) fn window(
    ctx: &egui::Context,
    id: egui::Id,
    title: &str,
    table: &[ParamRow],
    own: Option<&str>,
    value: Eval<'_>,
) -> Option<String> {
    let mut b: IfBuilder = ctx.data(|d| d.get_temp(state_id(id)))?;
    let names = choices(table, own);
    let mut done = None;
    let mut close = false;
    let at = ctx.content_rect().center() - egui::vec2(180.0, 140.0);
    egui::Window::new(format!("If: {title}"))
        .id(id.with("if_window"))
        .default_pos(at)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(RichText::new("Condition").strong());
            b.cond.ui(ui, &names, id.with("if_cond"));
            let cond = b.cond.text(&names);
            if unfinished(&b.cond) {
                ui.label(RichText::new("Choose a parameter, a comparison and a value").weak());
            } else {
                state_label(
                    ui,
                    &condition_now(table, &cond),
                    "the first value is used",
                    "the second value is used",
                );
            }
            ui.separator();
            egui::Grid::new(id.with("if_values"))
                .num_columns(2)
                .show(ui, |ui| {
                    ui.label("Then");
                    ui.add(
                        egui::TextEdit::singleline(&mut b.then)
                            .desired_width(200.0)
                            .hint_text("value if true"),
                    );
                    ui.end_row();
                    ui.label("Else");
                    ui.add(
                        egui::TextEdit::singleline(&mut b.other)
                            .desired_width(200.0)
                            .hint_text("value if false"),
                    );
                    ui.end_row();
                });
            let text = b.expr(&names);
            ui.label(RichText::new(&text).monospace());
            let result = value(&text);
            match &result {
                Ok(v) => ui.label(format!("= {v}")),
                Err(_) if unfinished(&b.cond) => ui.label(""),
                Err(e) => ui.label(RichText::new(e).color(Color32::from_rgb(230, 70, 60))),
            };
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(result.is_ok(), egui::Button::new("Insert"))
                    .on_disabled_hover_text("The equation is not valid yet")
                    .clicked()
                {
                    done = Some(text.clone());
                }
                close |= ui.button("Cancel").clicked();
            });
        });
    if done.is_some() || close || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        ctx.data_mut(|d| d.remove::<IfBuilder>(state_id(id)));
    } else {
        ctx.data_mut(|d| d.insert_temp(state_id(id), b));
    }
    done
}

/// Which argument of the innermost open `if(` the end of `text` is in
/// (0 condition, 1 value if true, 2 value if false), and the condition's
/// text once it is complete.
pub(super) fn if_hint(text: &str) -> Option<(usize, Option<String>)> {
    // (is an if, argument, start of the argument) per open bracket.
    let mut stack: Vec<(bool, usize, usize)> = Vec::new();
    let mut quote = false;
    let mut cond: Vec<Option<String>> = Vec::new();
    for (i, c) in text.char_indices() {
        match c {
            '"' => quote = !quote,
            '(' if !quote => {
                let is_if = text[..i].trim_end().ends_with("if")
                    && text[..i].trim_end().strip_suffix("if").is_some_and(|b| {
                        b.chars()
                            .last()
                            .is_none_or(|p| !p.is_alphanumeric() && p != '_')
                    });
                stack.push((is_if, 0, i + 1));
                cond.push(None);
            }
            ')' if !quote => {
                stack.pop();
                cond.pop();
            }
            ',' if !quote => {
                if let (Some(top), Some(cd)) = (stack.last_mut(), cond.last_mut()) {
                    if top.1 == 0 {
                        *cd = Some(text[top.2..i].trim().to_owned());
                    }
                    top.1 += 1;
                    top.2 = i + 1;
                }
            }
            _ => {}
        }
    }
    let k = stack.iter().rposition(|s| s.0)?;
    Some((stack[k].1.min(2), cond[k].clone()))
}

/// The hint under a field being typed in: `if(condition, value if true,
/// value if false)` with the current argument in bold, and the condition's
/// state once it is written.
pub(super) fn draw_hint(ui: &egui::Ui, below: egui::Rect, text: &str, table: &[ParamRow]) {
    let Some((arg, cond)) = if_hint(text) else {
        return;
    };
    egui::Area::new(ui.id().with("if_hint"))
        .fixed_pos(below.left_bottom() + egui::vec2(0.0, 2.0))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label("if(");
                    for (k, part) in ["condition", "value if true", "value if false"]
                        .iter()
                        .enumerate()
                    {
                        if k > 0 {
                            ui.label(", ");
                        }
                        let t = RichText::new(*part);
                        ui.label(if k == arg {
                            t.strong().underline()
                        } else {
                            t.weak()
                        });
                    }
                    ui.label(")");
                });
                if let Some(c) = cond.filter(|c| !c.is_empty()) {
                    match condition_now(table, &c) {
                        Ok(b) => ui.label(RichText::new(format!("{c}: now {b}")).small()),
                        Err(e) => ui.label(
                            RichText::new(format!("{c}: {e}"))
                                .small()
                                .color(Color32::from_rgb(230, 70, 60)),
                        ),
                    };
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(n: &str) -> Choice {
        Choice {
            name: n.into(),
            logic: Logic::Num,
            unit: "mm".into(),
            about: String::new(),
        }
    }

    /// Typed conditions come back as rows and give the same text.
    #[test]
    fn conditions_round_trip_through_rows() {
        let names = vec![
            num("Width"),
            Choice {
                name: "Heavy".into(),
                logic: Logic::Bool,
                unit: String::new(),
                about: String::new(),
            },
            Choice {
                name: "Material".into(),
                logic: Logic::Text,
                unit: String::new(),
                about: String::new(),
            },
        ];
        for t in [
            "Width < 100 mm",
            "not Heavy",
            "Heavy and Width >= 20 mm",
            "Material == \"Al\" or Width != 3 mm",
        ] {
            let b = CondBuilder::from_text(t);
            assert!(!b.text_mode, "{t} fell back to text");
            assert_eq!(b.text(&names), t);
        }
        // Brackets, if() inside: the text itself.
        for t in [
            "(Width < 1 mm or Heavy) and not Heavy",
            "if(Heavy, true, false)",
        ] {
            let b = CondBuilder::from_text(t);
            assert!(b.text_mode, "{t}");
            assert_eq!(b.text(&names), t);
        }
        // A bare word for a text parameter is quoted.
        let b = CondBuilder {
            rows: vec![Cond {
                name: "Material".into(),
                op: 0,
                value: "Al".into(),
            }],
            ..Default::default()
        };
        assert_eq!(b.text(&names), "Material == \"Al\"");
    }

    /// An if() field is read back into the builder; any other text becomes
    /// both values.
    #[test]
    fn if_equations_round_trip() {
        let names = vec![num("Width")];
        let b = IfBuilder::from_expr("if(Width > 50 mm, 8 mm, max(3 mm, Width / 10))");
        assert_eq!(b.then, "8 mm");
        assert_eq!(b.other, "max(3 mm, Width / 10)");
        assert_eq!(
            b.expr(&names),
            "if(Width > 50 mm, 8 mm, max(3 mm, Width / 10))"
        );
        let b = IfBuilder::from_expr("12 mm");
        assert_eq!((b.then.as_str(), b.other.as_str()), ("12 mm", "12 mm"));
        // Two calls side by side are not one if().
        assert!(if_args("if(a, b, c) + if(a, b, c)").is_none());
    }

    /// The hint follows the argument being typed, also in nested calls.
    #[test]
    fn hint_follows_the_argument() {
        assert_eq!(if_hint("if(Width > 5 mm"), Some((0, None)));
        assert_eq!(
            if_hint("if(Width > 5 mm, 3 mm"),
            Some((1, Some("Width > 5 mm".into())))
        );
        assert_eq!(if_hint("if(a, max(1, 2"), Some((1, Some("a".into()))));
        assert_eq!(if_hint("if(a, b, c)"), None);
        assert_eq!(if_hint("modif(3"), None);
        assert_eq!(if_hint("2 * (3 + 4"), None);
        assert_eq!(if_hint("if(a, b, if(c"), Some((0, None)));
    }
}
