//! Surface Texture, Feature Control Frame and Datum Identifier commands
//! (spec 4.7, Symbols; `drawing::gdt`): an options window while the command
//! runs, a click on an edge gives the leader's end (or the symbol's root),
//! the next click places it; a click on empty sheet places it without a
//! leader. A placed symbol is edited in Properties.

use eframe::egui::{self, Rect};

use super::CadCraftApp;
use super::drawing::{DrwTool, ShownView};
use super::drawing_annot::paint_prims;

const PREVIEW: egui::Color32 = egui::Color32::from_rgb(60, 140, 90);
const HOVER: egui::Color32 = egui::Color32::from_rgb(230, 120, 40);
use crate::drawing::AnnotationKind;
use crate::drawing::gdt::{
    self, Characteristic, ControlFrame, FrameRow, Mark, MarkSymbol, SurfaceTexture, SurfaceType,
};

/// Which symbol a command places.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    /// Surface Texture Symbol.
    Surface,
    /// Feature Control Frame.
    Frame,
    /// Datum Identifier Symbol.
    Datum,
}

/// A symbol command in progress.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct MarkDraft {
    /// The symbol's settings (the options window edits them).
    pub mark: Mark,
    /// The leader's end once picked: its view and sheet point.
    pub tip: Option<(u32, [f64; 2])>,
}

impl MarkDraft {
    fn new(kind: MarkKind) -> Self {
        let mark = match kind {
            MarkKind::Surface => Mark::SurfaceTexture(SurfaceTexture {
                kind: SurfaceType::Removal,
                a: "Ra 3.2".into(),
                ..Default::default()
            }),
            MarkKind::Frame => Mark::ControlFrame(ControlFrame {
                rows: vec![FrameRow {
                    symbol: Characteristic::Position,
                    tolerance: "Ø0.1".into(),
                    datums: vec!["A".into()],
                }],
                note: String::new(),
            }),
            MarkKind::Datum => Mark::Datum { letter: "A".into() },
        };
        Self { mark, tip: None }
    }

    /// The status bar's prompt.
    pub(super) fn prompt(&self) -> String {
        let name = mark_name(&self.mark);
        match (self.tip, &self.mark) {
            (None, Mark::Datum { .. }) => {
                format!("{name}: click the datum feature's edge (Esc ends)")
            }
            (None, _) => format!(
                "{name}: click an edge for a leader or the symbol's place, or the sheet to place it free (Esc ends)"
            ),
            (Some(_), _) => format!(
                "{name}: click where the symbol goes (on the picked point: no leader); right click: pick again"
            ),
        }
    }
}

/// The symbol's command name.
pub(super) fn mark_name(m: &Mark) -> &'static str {
    match m {
        Mark::SurfaceTexture(_) => "Surface Texture",
        Mark::ControlFrame(_) => "Feature Control Frame",
        Mark::Datum { .. } => "Datum Identifier",
    }
}

/// The datum letter after `l` (ISO 5459 skips I, O, Q).
fn next_letter(l: &str) -> String {
    let mut c = l.chars().next().unwrap_or('@');
    loop {
        c = match c {
            'Z' => return "A".into(),
            c if c.is_ascii_uppercase() || c == '@' => (c as u8 + 1) as char,
            _ => return "A".into(),
        };
        if !"IOQ".contains(c) {
            return c.to_string();
        }
    }
}

/// The settings rows of a symbol (options window and Properties).
pub(super) fn mark_fields(ui: &mut egui::Ui, m: &mut Mark) {
    match m {
        Mark::SurfaceTexture(s) => {
            ui.label(egui::RichText::new("Surface Type").strong());
            for t in SurfaceType::ALL {
                ui.radio_value(&mut s.kind, t, t.label());
            }
            ui.checkbox(&mut s.force_tail, "Force tail");
            ui.checkbox(&mut s.all_round, "All-round");
            egui::Grid::new("st_fields").num_columns(2).show(ui, |ui| {
                for (label, v, tip) in [
                    (
                        "A",
                        &mut s.a,
                        "The surface texture requirement (Ra 3.2, Rz 6.3)",
                    ),
                    ("B", &mut s.b, "A second requirement"),
                    ("C", &mut s.c, "The production method (ground, turned)"),
                    ("E", &mut s.e, "The machining allowance"),
                ] {
                    ui.label(label).on_hover_text(tip);
                    ui.add(egui::TextEdit::singleline(v).desired_width(120.0));
                    ui.end_row();
                }
                ui.label("D (lay)");
                egui::ComboBox::from_id_salt("st_lay")
                    .selected_text(if s.lay.is_empty() {
                        "(none)"
                    } else {
                        s.lay.as_str()
                    })
                    .show_ui(ui, |ui| {
                        for l in ["", "=", "⊥", "X", "M", "C", "R", "P"] {
                            let shown = if l.is_empty() { "(none)" } else { l };
                            ui.selectable_value(&mut s.lay, l.to_string(), shown);
                        }
                    });
                ui.end_row();
            });
        }
        Mark::ControlFrame(f) => {
            ui.horizontal(|ui| {
                ui.label("Note");
                ui.add(egui::TextEdit::singleline(&mut f.note).desired_width(120.0))
                    .on_hover_text("Above the frame, e.g. 4x");
            });
            let mut remove = None;
            let n = f.rows.len();
            for (k, r) in f.rows.iter_mut().enumerate() {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(format!("Row {}", k + 1));
                    egui::ComboBox::from_id_salt(("fcf_sym", k))
                        .selected_text(r.symbol.label())
                        .show_ui(ui, |ui| {
                            for c in Characteristic::ALL {
                                ui.selectable_value(&mut r.symbol, c, c.label());
                            }
                        });
                    if n > 1 && ui.small_button("Remove").clicked() {
                        remove = Some(k);
                    }
                });
                egui::Grid::new(("fcf_row", k))
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Tolerance");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut r.tolerance).desired_width(80.0),
                            );
                            for (b, tip) in [
                                ("Ø", "Diameter zone"),
                                ("Ⓜ", "Maximum material"),
                                ("Ⓛ", "Least material"),
                            ] {
                                if ui.small_button(b).on_hover_text(tip).clicked() {
                                    if b == "Ø" {
                                        r.tolerance.insert(0, 'Ø');
                                    } else {
                                        r.tolerance.push_str(b);
                                    }
                                }
                            }
                        });
                        ui.end_row();
                        r.datums.resize(3, String::new());
                        for (i, name) in ["Datum 1", "Datum 2", "Datum 3"].iter().enumerate() {
                            ui.label(*name);
                            let form = r.symbol.is_form();
                            ui.add_enabled(
                                !form,
                                egui::TextEdit::singleline(&mut r.datums[i]).desired_width(60.0),
                            )
                            .on_disabled_hover_text("A tolerance of form has no datum");
                            ui.end_row();
                        }
                        if r.symbol.is_form() {
                            r.datums.iter_mut().for_each(String::clear);
                        }
                        while r.datums.last().is_some_and(String::is_empty) {
                            r.datums.pop();
                        }
                    });
            }
            if let Some(k) = remove {
                f.rows.remove(k);
            }
            if f.rows.len() < 3 && ui.button("Add Row").clicked() {
                f.rows.push(FrameRow::default());
            }
        }
        Mark::Datum { letter } => {
            ui.horizontal(|ui| {
                ui.label("Datum");
                ui.add(egui::TextEdit::singleline(letter).desired_width(40.0));
            });
        }
    }
}

impl CadCraftApp {
    /// Starts a symbol command.
    pub(super) fn start_mark(&mut self, kind: MarkKind) {
        let mut draft = MarkDraft::new(kind);
        // Datum letters go on: the next one after those placed.
        if kind == MarkKind::Datum
            && let Some(last) = self.drawing().and_then(|d| {
                d.sheets
                    .iter()
                    .flat_map(|s| &s.annotations)
                    .filter_map(|a| match &a.kind {
                        AnnotationKind::Mark(MarkSymbol {
                            mark: Mark::Datum { letter },
                            ..
                        }) => Some(letter.clone()),
                        _ => None,
                    })
                    .max()
            })
        {
            draft.mark = Mark::Datum {
                letter: next_letter(&last),
            };
        }
        self.drw.dialog = None;
        self.drw.sel = None;
        self.drw.sel_ann = None;
        self.drw.tool = Some(DrwTool::Mark(Box::new(draft)));
    }

    /// The options window of a running symbol command.
    pub(super) fn mark_window(&mut self, ctx: &egui::Context) {
        let Some(DrwTool::Mark(d)) = &mut self.drw.tool else {
            return;
        };
        let title = mark_name(&d.mark);
        let mut m = d.mark.clone();
        let win = egui::Window::new(title)
            .resizable(false)
            .collapsible(false)
            .default_width(260.0);
        let mut close = false;
        self.layout
            .place("drawing_mark", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                mark_fields(ui, &mut m);
                ui.separator();
                if ui.button("Done").clicked() {
                    close = true;
                }
            });
        if let Some(DrwTool::Mark(d)) = &mut self.drw.tool {
            d.mark = m;
        }
        if close {
            self.drw.tool = None;
        }
    }

    /// The symbol command on the sheet.
    pub(super) fn mark_tool(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        views: &[ShownView],
    ) {
        let Some(DrwTool::Mark(d)) = self.drw.tool.clone() else {
            return;
        };
        if response.secondary_clicked() && d.tip.is_some() {
            if let Some(DrwTool::Mark(x)) = &mut self.drw.tool {
                x.tip = None;
            }
            return;
        }
        let cam = self.drw.cam;
        let Some(p) = response.hover_pos() else {
            return;
        };
        let q = cam.to_sheet(rect, p);
        let look = self.drw.look.clone();
        let h = look.leader_text.height;
        match d.tip {
            None => {
                let hit = self.geometry_under(views, rect, p);
                if let Some(g) = &hit {
                    painter.circle_stroke(
                        cam.to_screen(rect, g.at),
                        5.0,
                        egui::Stroke::new(2.0, HOVER),
                    );
                }
                if response.clicked() {
                    match hit {
                        Some(g) => {
                            if let Some(DrwTool::Mark(x)) = &mut self.drw.tool {
                                x.tip = Some((g.view, g.at));
                            }
                        }
                        // A datum needs its feature; the others stand free.
                        None if !matches!(d.mark, Mark::Datum { .. }) => {
                            let view = self.drw.hover;
                            self.add_mark(views, view, q, None, d.mark.clone());
                        }
                        None => {}
                    }
                }
            }
            Some((view, tip)) => {
                // On the picked point (within 2 mm): the symbol stands on
                // the edge, no leader.
                let on_tip = (q[0] - tip[0]).hypot(q[1] - tip[1]) < 2.0f64.max(6.0 / cam.zoom);
                let (at, leader) = match &d.mark {
                    Mark::SurfaceTexture(_) if on_tip => (tip, None),
                    _ => (q, Some(tip)),
                };
                let preview = MarkSymbol {
                    view: None,
                    at,
                    tip: leader,
                    mark: d.mark.clone(),
                };
                paint_prims(
                    painter,
                    &cam,
                    rect,
                    &gdt::prims(&preview, [0.0, 0.0], &look),
                    PREVIEW,
                );
                let _ = h;
                if response.clicked() {
                    self.add_mark(views, Some(view), at, leader, d.mark.clone());
                    if let Some(DrwTool::Mark(x)) = &mut self.drw.tool {
                        x.tip = None;
                        if let Mark::Datum { letter } = &mut x.mark {
                            *letter = next_letter(letter);
                        }
                    }
                }
            }
        }
    }

    fn add_mark(
        &mut self,
        views: &[ShownView],
        view: Option<u32>,
        at: [f64; 2],
        tip: Option<[f64; 2]>,
        mark: Mark,
    ) {
        let origin = view
            .and_then(|id| views.iter().find(|v| v.id == id))
            .map(|v| v.position);
        let rel = |p: [f64; 2]| match origin {
            Some(o) => [p[0] - o[0], p[1] - o[1]],
            None => p,
        };
        let m = MarkSymbol {
            view: origin.and(view),
            at: rel(at),
            tip: tip.map(rel),
            mark,
        };
        let label = mark_name(&m.mark);
        self.edit_drawing(label, move |d| {
            d.add_annotation(AnnotationKind::Mark(m));
        });
    }

    /// Properties of a placed symbol: its settings, changes as one step.
    pub(super) fn mark_properties(&mut self, ui: &mut egui::Ui, id: u32, m: &MarkSymbol) {
        let mut edited = m.mark.clone();
        mark_fields(ui, &mut edited);
        let leader = m.tip.is_some();
        let mut keep = leader;
        if leader {
            ui.checkbox(&mut keep, "Leader");
        }
        if edited != m.mark || keep != leader {
            let label = format!("Edit {}", mark_name(&edited));
            self.edit_drawing(&label, move |d| {
                if let Some(x) = d
                    .sheet_mut()
                    .and_then(|s| s.annotations.iter_mut().find(|x| x.id == id))
                    && let AnnotationKind::Mark(s) = &mut x.kind
                {
                    if !keep
                        && let Some(t) = s.tip.take()
                        && matches!(s.mark, Mark::SurfaceTexture(_))
                    {
                        s.at = t;
                    }
                    s.mark = edited;
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datum_letters_skip_i_o_q() {
        assert_eq!(next_letter("A"), "B");
        assert_eq!(next_letter("H"), "J");
        assert_eq!(next_letter("N"), "P");
        assert_eq!(next_letter("P"), "R");
        assert_eq!(next_letter("Z"), "A");
    }
}
