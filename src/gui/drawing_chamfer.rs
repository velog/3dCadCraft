//! Chamfer Note (`docs/spec/4.7-drawing-commands.md` §3.3): click the
//! chamfer's edge in a view, then a reference edge of the same view that
//! touches or crosses it, then where the note goes; the command goes on
//! with the next chamfer until Esc or right-click. Only straight edges are
//! offered (a short chamfer is hard to hit beside its end points, so this
//! command picks lines, never points). A double-click on a note opens Edit
//! Chamfer Note (its text with the value tokens).

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke};

use super::CadCraftApp;
use super::drawing::{DrwTool, ShownView};
use super::drawing_annot::{lookup, paint_prims};
use crate::drawing::AnnotationKind;
use crate::drawing::annot::{self, Placement};
use crate::drawing::chamfer_note::{self, ChamferNote, DEFAULT_FORMAT, TOKENS};

const PICKED: Color32 = Color32::from_rgb(40, 120, 230);
const PREVIEW: Color32 = Color32::from_rgb(60, 140, 90);
const HOVER: Color32 = Color32::from_rgb(230, 120, 40);
const REFUSED: Color32 = Color32::from_rgb(210, 40, 40);

/// Line pick radius (px).
const LINE_PX: f32 = 7.0;

/// A Chamfer Note in progress.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct ChDraft {
    /// The chamfer's edge (view, name).
    chamfer: Option<(u32, String)>,
    /// The reference edge (same view).
    reference: Option<String>,
}

impl ChDraft {
    /// The status-bar prompt.
    pub(super) fn prompt(&self) -> String {
        match (&self.chamfer, &self.reference) {
            (None, _) => "Chamfer Note: click the chamfer's edge in a view".into(),
            (Some(_), None) => {
                "Chamfer Note: click the reference edge (touching or crossing the chamfer)".into()
            }
            (Some(_), Some(_)) => {
                "Chamfer Note: click where the note goes (Esc or right-click ends)".into()
            }
        }
    }

    /// One step back (right-click): the last pick goes; `false` when there
    /// was none (the command ends).
    pub(super) fn back(&mut self) -> bool {
        if self.reference.take().is_some() {
            return true;
        }
        self.chamfer.take().is_some()
    }
}

/// Edit Chamfer Note.
#[derive(Clone, Debug)]
pub(super) struct ChEdit {
    id: u32,
    note: ChamferNote,
    text: String,
}

/// A straight edge of a view: (view, name, segment in the view).
type ViewLine = (u32, String, [[f64; 2]; 2]);

/// The straight edge under the pointer.
fn line_under(
    views: &[ShownView],
    cam: super::drawing::SheetCam,
    rect: Rect,
    pointer: Pos2,
    only: Option<u32>,
) -> Option<ViewLine> {
    let mut best: Option<(f32, ViewLine)> = None;
    for v in views.iter().filter(|v| only.is_none_or(|o| o == v.id)) {
        let Some(l) = v.lines.as_deref() else {
            continue;
        };
        let pl = Placement {
            position: v.position,
            scale: v.scale,
            center: l.center(),
        };
        let scr = |q: [f64; 2]| cam.to_screen(rect, pl.to_sheet(q));
        for (k, piece) in l.visible.iter().enumerate() {
            let Some(e) = l
                .visible_src
                .get(k)
                .copied()
                .flatten()
                .and_then(|i| l.edges.get(i as usize))
            else {
                continue;
            };
            let Some(name) = e.name.clone() else {
                continue;
            };
            if !e.straight || e.circle.is_some() {
                continue;
            }
            let d = piece
                .windows(2)
                .map(|w| {
                    let (a, b) = (scr(w[0]), scr(w[1]));
                    let ab = b - a;
                    let t = ((pointer - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                    (a + ab * t).distance(pointer)
                })
                .fold(f32::MAX, f32::min);
            if d <= LINE_PX && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, (v.id, name, [e.a, e.b])));
            }
        }
    }
    best.map(|(_, g)| g)
}

impl CadCraftApp {
    /// The edge `name` of view `view` as a segment.
    fn view_segment(views: &[ShownView], view: u32, name: &str) -> Option<[[f64; 2]; 2]> {
        let e = views
            .iter()
            .find(|v| v.id == view)?
            .lines
            .as_deref()?
            .edge(name)?;
        Some([e.a, e.b])
    }

    /// Paints a view segment thick.
    fn paint_segment(
        &self,
        p: &egui::Painter,
        rect: Rect,
        views: &[ShownView],
        view: u32,
        seg: [[f64; 2]; 2],
        color: Color32,
    ) {
        let Some(v) = views.iter().find(|v| v.id == view) else {
            return;
        };
        let Some(l) = v.lines.as_deref() else {
            return;
        };
        let pl = Placement {
            position: v.position,
            scale: v.scale,
            center: l.center(),
        };
        let cam = self.drw.cam;
        p.line_segment(
            [
                cam.to_screen(rect, pl.to_sheet(seg[0])),
                cam.to_screen(rect, pl.to_sheet(seg[1])),
            ],
            Stroke::new(3.0, color),
        );
    }

    /// The running Chamfer Note command.
    pub(super) fn chamfer_note_tool(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        views: &[ShownView],
    ) {
        let Some(DrwTool::ChamferNote(mut d)) = self.drw.tool.clone() else {
            return;
        };
        let pointer = response.hover_pos();
        let cam = self.drw.cam;
        let at = pointer.map(|p| cam.to_sheet(rect, p));
        if let Some((v, c)) = &d.chamfer
            && let Some(seg) = Self::view_segment(views, *v, c)
        {
            self.paint_segment(painter, rect, views, *v, seg, PICKED);
        }
        match (d.chamfer.clone(), d.reference.clone()) {
            (None, _) => {
                let hit = pointer.and_then(|p| line_under(views, cam, rect, p, None));
                if let Some((v, _, seg)) = &hit {
                    self.paint_segment(painter, rect, views, *v, *seg, HOVER);
                }
                if response.clicked()
                    && let Some((v, name, _)) = hit
                {
                    d.chamfer = Some((v, name));
                }
            }
            (Some((v, c)), None) => {
                let Some(cseg) = Self::view_segment(views, v, &c) else {
                    d = ChDraft::default();
                    self.drw.tool = Some(DrwTool::ChamferNote(d));
                    return;
                };
                let hit = pointer
                    .and_then(|p| line_under(views, cam, rect, p, Some(v)))
                    .filter(|(_, n, _)| *n != c);
                if let Some((_, _, seg)) = &hit {
                    let fits = chamfer_note::values(cseg, *seg);
                    let color = if fits.is_ok() { HOVER } else { REFUSED };
                    self.paint_segment(painter, rect, views, v, *seg, color);
                    if response.clicked() {
                        match fits {
                            Ok(_) => d.reference = hit.map(|(_, n, _)| n),
                            Err(e) => {
                                self.file_status =
                                    Some(Err(format!("Chamfer Note: {}", e.message())))
                            }
                        }
                    }
                }
            }
            (Some((v, c)), Some(r)) => {
                let Some(view) = views.iter().find(|x| x.id == v) else {
                    return;
                };
                if let Some(seg) = Self::view_segment(views, v, &r) {
                    self.paint_segment(painter, rect, views, v, seg, PICKED);
                }
                let Some(q) = at else {
                    self.drw.tool = Some(DrwTool::ChamferNote(d));
                    return;
                };
                let note = ChamferNote::new(
                    v,
                    &c,
                    &r,
                    [q[0] - view.position[0], q[1] - view.position[1]],
                );
                let kind = AnnotationKind::ChamferNote(note);
                let map = lookup(views);
                let look = |id: u32| map.get(&id).copied();
                if let Ok(s) = annot::resolve_with(&kind, &look, &self.drw.look) {
                    paint_prims(painter, &cam, rect, &s.prims, PREVIEW);
                }
                if response.clicked() {
                    self.edit_drawing("Chamfer Note", |doc| doc.add_annotation(kind));
                    d = ChDraft::default();
                }
            }
        }
        self.drw.tool = Some(DrwTool::ChamferNote(d));
    }

    /// Double-click on a chamfer note.
    pub(super) fn open_chamfer_edit(&mut self, id: u32) {
        let Some(note) = self.drawing().and_then(|d| {
            d.sheet()?
                .annotations
                .iter()
                .find(|a| a.id == id)
                .and_then(|a| match &a.kind {
                    AnnotationKind::ChamferNote(n) => Some(n.clone()),
                    _ => None,
                })
        }) else {
            return;
        };
        let text = note.format().to_string();
        self.drw.chamfer_edit = Some(ChEdit { id, note, text });
    }

    /// The Edit Chamfer Note window.
    pub(super) fn chamfer_window(&mut self, ctx: &egui::Context, views: &[ShownView]) {
        let Some(mut e) = self.drw.chamfer_edit.clone() else {
            return;
        };
        let (mut ok, mut cancel, mut open) = (false, false, true);
        // The values now, for the preview line.
        let values = views
            .iter()
            .find(|v| v.id == e.note.view)
            .and_then(|v| v.lines.as_deref())
            .and_then(|l| e.note.measure(l).ok())
            .map(|(_, v)| v);
        let win = egui::Window::new("Edit Chamfer Note")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(360.0);
        let look = self.drw.look.dim.clone();
        self.layout
            .place("drawing_chamfer_note", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                let edit = ui.add(
                    egui::TextEdit::multiline(&mut e.text)
                        .desired_rows(2)
                        .desired_width(330.0)
                        .id_salt("chamfer_note_text"),
                );
                ui.label(RichText::new("Values and Symbols").strong());
                ui.horizontal(|ui| {
                    for (token, tip) in TOKENS {
                        let label = match token {
                            "<DISTANCE1>" => "D1",
                            "<DISTANCE2>" => "D2",
                            _ => "A1",
                        };
                        if ui.button(label).on_hover_text(tip).clicked() {
                            e.text.push_str(token);
                            edit.request_focus();
                        }
                    }
                    if ui
                        .button("D1 X A1")
                        .on_hover_text("Distance / Angle")
                        .clicked()
                    {
                        e.text = DEFAULT_FORMAT.into();
                    }
                    if ui
                        .button("D1 X D2")
                        .on_hover_text("Distance / Distance")
                        .clicked()
                    {
                        e.text = "<DISTANCE1> X <DISTANCE2>".into();
                    }
                    for s in ["Ø", "°", "±"] {
                        if ui.button(s).clicked() {
                            e.text.push_str(s);
                        }
                    }
                });
                ui.add_enabled(false, egui::Button::new("Precision and Tolerance"))
                    .on_disabled_hover_text(
                        "Spec 4.7 §3.3: Chamfer Note precision and tolerance (to do; the dimension style's precision is used)",
                    );
                ui.add_space(4.0);
                match values {
                    Some(v) => {
                        ui.label(format!(
                            "Shows: {}",
                            chamfer_note::text(&e.text, v, &look)
                        ));
                    }
                    None => {
                        ui.colored_label(REFUSED, "The note's edges are missing in the view");
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() || super::keys::panel_ok_key(ui, false) {
                        ok = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel || !open {
            self.drw.chamfer_edit = None;
            return;
        }
        if ok {
            let id = e.id;
            let mut note = e.note.clone();
            note.format = (e.text != DEFAULT_FORMAT).then(|| e.text.clone());
            self.edit_drawing("Edit Chamfer Note", move |d| {
                if let Some(a) = d
                    .sheet_mut()
                    .and_then(|s| s.annotations.iter_mut().find(|a| a.id == id))
                {
                    a.kind = AnnotationKind::ChamferNote(note);
                }
            });
            self.drw.chamfer_edit = None;
            return;
        }
        self.drw.chamfer_edit = Some(e);
    }
}
