//! Drawing annotations in the GUI (`docs/spec/4-drawings.md` §3.3, stage D3
//! first part): Center Mark, Centerline, General Dimension and Text tools,
//! picking view geometry (end points, centres, edges), selecting, dragging
//! and deleting annotations, and File › Export › PDF. The model lives in
//! `crate::drawing::annot`; this file only picks and paints.

use std::collections::HashMap;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Shape, Stroke};

use super::CadCraftApp;
use super::drawing::{DrwTool, SheetCam, ShownView};
use super::valuefield::ExprValue;
use crate::drawing::annot::{self, Orphan, Placement, point_of};
use crate::drawing::hlr::ViewLines;
use crate::drawing::sheet::{Anchor, Prim};
use crate::drawing::{AnnotationKind, DimKind, Dimension, PointRef, PointRole};

const SEL: Color32 = Color32::from_rgb(40, 120, 230);
const HOVER: Color32 = Color32::from_rgb(230, 120, 40);
const PREVIEW: Color32 = Color32::from_rgb(60, 140, 90);
const ORPHAN: Color32 = Color32::from_rgb(210, 40, 40);

/// Point pick radius (px), as in sketches.
const POINT_PX: f32 = 9.0;
/// Curve pick radius (px).
const CURVE_PX: f32 = 7.0;

/// What a click on view geometry took.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct GeoPick {
    pub(super) view: u32,
    pub(super) edge: String,
    what: PickWhat,
    /// The picked point / the circle centre on the sheet (mm).
    pub(super) at: [f64; 2],
}

impl GeoPick {
    /// The pick as text (a planned command's selector list).
    pub(super) fn describe(&self) -> String {
        let what = match self.what {
            PickWhat::Point(_) => "point",
            PickWhat::Line => "edge",
            PickWhat::Circle(..) => "circle",
        };
        format!("View {}: {what} {}", self.view, self.edge)
    }
}

/// Kind of geometry picked.
#[derive(Clone, Copy, Debug, PartialEq)]
enum PickWhat {
    /// An end point or a centre.
    Point(PointRole),
    /// A straight edge.
    Line,
    /// A circle or arc (radius on the sheet).
    Circle(f64, bool),
}

/// General Dimension in progress.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct DimDraft {
    first: Option<GeoPick>,
    second: Option<GeoPick>,
}

/// The Text window (new or Edit Text).
#[derive(Clone, Debug)]
pub(super) struct TextDialog {
    at: [f64; 2],
    text: String,
    height: f64,
    pub(super) edit: Option<u32>,
    focus: bool,
    /// Leader Text: the arrow's tip.
    leader: Option<[f64; 2]>,
    /// Its formatting (Format Text).
    rich: crate::drawing::rich::RichText,
    /// Selection and cursor of the editor.
    ed: super::format_text::Editor,
}

impl TextDialog {
    fn new(at: [f64; 2], leader: Option<[f64; 2]>) -> Self {
        Self {
            at,
            text: String::new(),
            height: 3.5,
            edit: None,
            focus: true,
            leader,
            rich: Default::default(),
            ed: Default::default(),
        }
    }

    /// The annotation it makes (plain when nothing is formatted, so older
    /// builds show the same).
    fn kind(&self, text: String) -> AnnotationKind {
        let rich =
            (self.rich != crate::drawing::rich::RichText::default()).then(|| self.rich.clone());
        match self.leader {
            Some(tip) => AnnotationKind::LeaderText {
                tip,
                at: self.at,
                text,
                rich,
            },
            None => AnnotationKind::Text {
                at: self.at,
                text,
                height: self.height,
                rich,
            },
        }
    }

    /// The note as it will look (the live preview on the sheet).
    pub(super) fn preview(&self) -> Option<AnnotationKind> {
        (!self.text.trim().is_empty()).then(|| self.kind(self.text.trim_end().to_string()))
    }
}

/// Which sheets go to the PDF.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PdfRange {
    Current,
    All,
    Range,
}

/// The PDF export options (Inventor's PDF Drawing options).
#[derive(Clone, Debug)]
pub(super) struct PdfDialog {
    range: PdfRange,
    from: usize,
    to: usize,
    excluded: bool,
    remove_weights: bool,
    all_black: bool,
    open_after: bool,
}

/// The part of an annotation a drag holds (Inventor's grips): a
/// dimension's text slides along its line and moves the line, its line only
/// moves the line; a leader text's arrow tip moves alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum DragPart {
    /// The whole annotation (or a dimension's line).
    #[default]
    Whole,
    /// A dimension's text.
    Text,
    /// A leader's arrow tip.
    Tip,
}

/// Distance from `q` to the nearest text of `prims` (sheet mm).
fn text_distance(prims: &[Prim], q: [f64; 2]) -> f64 {
    let texts: Vec<Prim> = prims
        .iter()
        .filter(|p| matches!(p, Prim::Text { .. } | Prim::RunText { .. }))
        .cloned()
        .collect();
    if texts.is_empty() {
        f64::MAX
    } else {
        prims_distance(&texts, q)
    }
}

/// A resolved annotation of this frame (for painting and picking).
pub(super) struct ShownAnn {
    id: u32,
    prims: Vec<Prim>,
    /// Where an orphaned annotation is marked.
    orphan: Option<[f64; 2]>,
    /// A table takes clicks anywhere inside its outline.
    area: Option<([f64; 2], [f64; 2])>,
    /// The measured value (dimensions).
    pub value: Option<f64>,
    /// Its layer's colour.
    ink: Color32,
}

impl ShownAnn {
    /// The annotation's id.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// What it draws.
    pub fn prims(&self) -> &[Prim] {
        &self.prims
    }
}

/// The box around the lines of `prims`.
fn lines_box(prims: &[Prim]) -> Option<([f64; 2], [f64; 2])> {
    let mut lo = [f64::MAX; 2];
    let mut hi = [f64::MIN; 2];
    for p in prims {
        if let Prim::Line { a, b, .. } = p {
            for q in [a, b] {
                lo = [lo[0].min(q[0]), lo[1].min(q[1])];
                hi = [hi[0].max(q[0]), hi[1].max(q[1])];
            }
        }
    }
    (lo[0] <= hi[0]).then_some((lo, hi))
}

/// The views of this frame by id, for `annot::resolve`.
pub(super) fn lookup(views: &[ShownView]) -> HashMap<u32, (Placement, &ViewLines)> {
    views
        .iter()
        .filter_map(|v| {
            let l = v.lines.as_deref()?;
            Some((
                v.id,
                (
                    Placement {
                        position: v.position,
                        scale: v.scale,
                        center: l.center(),
                    },
                    l,
                ),
            ))
        })
        .collect()
}

/// Paints sheet primitives in `color` (a highlight or preview: the
/// layers' pens are ignored).
pub(super) fn paint_prims(
    p: &egui::Painter,
    cam: &SheetCam,
    rect: Rect,
    prims: &[Prim],
    color: Color32,
) {
    paint_prims_with(p, cam, rect, prims, color, false);
}

/// Paints sheet primitives in their layers' colours (`Prim::Pen`), starting
/// with `color`.
pub(super) fn paint_prims_styled(
    p: &egui::Painter,
    cam: &SheetCam,
    rect: Rect,
    prims: &[Prim],
    color: Color32,
) {
    paint_prims_with(p, cam, rect, prims, color, true);
}

fn paint_prims_with(
    p: &egui::Painter,
    cam: &SheetCam,
    rect: Rect,
    prims: &[Prim],
    mut color: Color32,
    follow: bool,
) {
    let to = |q: [f64; 2]| cam.to_screen(rect, q);
    let px = |mm: f64| ((mm * cam.zoom) as f32).clamp(1.0, 6.0);
    // The layer's colour, while a formatted run overrides `color`.
    let mut layer = color;
    for prim in prims {
        match prim {
            Prim::Pen { color: c, .. } => {
                if follow {
                    color = Color32::from_rgb(c[0], c[1], c[2]);
                    layer = color;
                }
            }
            Prim::Ink { color: c } => {
                if follow {
                    color = c.map_or(layer, |c| Color32::from_rgb(c[0], c[1], c[2]));
                }
            }
            Prim::RunText {
                at,
                height,
                text,
                angle,
                bold,
                italic,
                stretch,
                center,
            } => {
                let size = (*height * cam.zoom * crate::drawing::rich::EM_PER_CAP) as f32;
                if size < 4.0 {
                    continue;
                }
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    text,
                    0.0,
                    egui::TextFormat {
                        font_id: FontId::proportional(size),
                        color,
                        italics: *italic,
                        // The width factor as letter spacing (egui cannot
                        // scale glyphs along x).
                        extra_letter_spacing: ((*stretch - 1.0) * *height * 0.6 * cam.zoom) as f32,
                        ..Default::default()
                    },
                );
                let galley = p.fonts_mut(|f| f.layout_job(job));
                let w = galley.size().x;
                let h = galley.size().y;
                let shift = if *center { -w * 0.5 } else { 0.0 };
                let (s, c) = (-*angle as f32).sin_cos();
                let base = to(*at);
                let off = egui::vec2(shift, -h * 0.8);
                let rot = egui::vec2(off.x * c - off.y * s, off.x * s + off.y * c);
                let at = base + rot;
                // Bold: drawn twice, a hair apart.
                if *bold {
                    let d = (size * 0.05).max(0.6);
                    p.add(
                        egui::epaint::TextShape::new(
                            at + egui::vec2(d * c, d * s),
                            galley.clone(),
                            color,
                        )
                        .with_angle(-*angle as f32),
                    );
                }
                p.add(egui::epaint::TextShape::new(at, galley, color).with_angle(-*angle as f32));
            }
            Prim::Line { a, b, weight } => {
                p.line_segment([to(*a), to(*b)], Stroke::new(px(weight.mm()), color));
            }
            Prim::Path { pts, width, dashed } => {
                let pts: Vec<Pos2> = pts.iter().map(|q| to(*q)).collect();
                let stroke = Stroke::new(px(*width), color);
                if *dashed {
                    let dash = ((3.0 * cam.zoom) as f32).clamp(3.0, 40.0);
                    let gap = ((1.5 * cam.zoom) as f32).clamp(2.0, 20.0);
                    p.extend(Shape::dashed_line(&pts, stroke, dash, gap));
                } else {
                    p.add(Shape::line(pts, stroke));
                }
            }
            Prim::Center { a, b, width, chain } => {
                let stroke = Stroke::new(px(*width), color);
                if *chain {
                    let long = ((12.0 * cam.zoom) as f32).clamp(6.0, 120.0);
                    let short = ((1.5 * cam.zoom) as f32).clamp(1.5, 15.0);
                    p.extend(Shape::dashed_line_with_offset(
                        &[to(*a), to(*b)],
                        stroke,
                        &[long, short],
                        &[short, short],
                        0.0,
                    ));
                } else {
                    p.line_segment([to(*a), to(*b)], stroke);
                }
            }
            Prim::Fill { pts } => {
                let mut pts: Vec<Pos2> = pts.iter().map(|q| to(*q)).collect();
                // egui fills convex polygons wound one way only.
                let area: f32 = (0..pts.len())
                    .map(|i| {
                        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                        a.x * b.y - b.x * a.y
                    })
                    .sum();
                if area > 0.0 {
                    pts.reverse();
                }
                p.add(Shape::convex_polygon(pts, color, Stroke::NONE));
            }
            Prim::Text {
                at,
                height,
                text,
                anchor,
                angle,
            } => {
                let size = (*height * cam.zoom * 1.35) as f32;
                if size < 4.0 {
                    continue;
                }
                let galley = p.layout_no_wrap(text.clone(), FontId::proportional(size), color);
                let w = galley.size().x;
                let h = galley.size().y;
                // Anchor on the baseline, then turn about it.
                let shift = match anchor {
                    Anchor::Left => 0.0,
                    Anchor::Center => -w * 0.5,
                };
                let (s, c) = (-*angle as f32).sin_cos();
                let base = to(*at);
                let off = egui::vec2(shift, -h * 0.8);
                let rot = egui::vec2(off.x * c - off.y * s, off.x * s + off.y * c);
                p.add(
                    egui::epaint::TextShape::new(base + rot, galley, color)
                        .with_angle(-*angle as f32),
                );
            }
        }
    }
}

/// Distance (sheet mm) from `q` to the primitives.
fn prims_distance(prims: &[Prim], q: [f64; 2]) -> f64 {
    let seg = |a: [f64; 2], b: [f64; 2]| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dy * dy;
        let t = if l2 < 1e-24 {
            0.0
        } else {
            (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
        };
        (q[0] - a[0] - t * dx).hypot(q[1] - a[1] - t * dy)
    };
    let mut best = f64::MAX;
    for p in prims {
        let d = match p {
            Prim::Pen { .. } | Prim::Ink { .. } => f64::MAX,
            Prim::RunText {
                at,
                height,
                text,
                stretch,
                center,
                ..
            } => {
                let w = annot::text_width(text, *height) * stretch;
                let c = [
                    if *center { at[0] } else { at[0] + w * 0.5 },
                    at[1] + height * 0.5,
                ];
                let dx = ((q[0] - c[0]).abs() - w * 0.5).max(0.0);
                let dy = ((q[1] - c[1]).abs() - height * 0.7).max(0.0);
                dx.hypot(dy)
            }
            Prim::Line { a, b, .. } | Prim::Center { a, b, .. } => seg(*a, *b),
            Prim::Path { pts, .. } | Prim::Fill { pts } => pts
                .windows(2)
                .map(|w| seg(w[0], w[1]))
                .fold(f64::MAX, f64::min),
            Prim::Text {
                at, height, text, ..
            } => {
                // A box around the text (unrotated approximation).
                let w = annot::text_width(text, *height);
                let c = [at[0], at[1] + height * 0.5];
                let dx = ((q[0] - c[0]).abs() - w * 0.5).max(0.0);
                let dy = ((q[1] - c[1]).abs() - height * 0.7).max(0.0);
                dx.hypot(dy)
            }
        };
        best = best.min(d);
    }
    best
}

impl CadCraftApp {
    /// Every annotation of the active sheet resolved against this frame's
    /// views.
    pub(super) fn shown_annotations(&self, views: &[ShownView]) -> Vec<ShownAnn> {
        let Some(sheet) = self.drawing().and_then(|d| d.sheet()) else {
            return Vec::new();
        };
        let map = lookup(views);
        let view = |id: u32| map.get(&id).copied();
        let tables = self.drw.tables.clone();
        let fallback = crate::drawing::style::DrawingStyles::default();
        let styles = self.drawing().map_or(&fallback, |d| &d.styles);
        sheet
            .annotations
            .iter()
            .filter_map(|a| {
                // A layer that is off hides its annotations (not pickable).
                match styles.pen(a.layer_name(&styles.defaults)) {
                    Some(Prim::Pen { color, .. }) => {
                        Some((a, Color32::from_rgb(color[0], color[1], color[2])))
                    }
                    _ => None,
                }
            })
            .map(
                |(a, ink)| match annot::resolve_in(&a.kind, &view, &self.drw.look, &tables) {
                    Ok(s) => ShownAnn {
                        id: a.id,
                        ink,
                        area: matches!(
                            a.kind,
                            AnnotationKind::PartsList(_)
                                | AnnotationKind::BendTable(_)
                                | AnnotationKind::HoleTable(_)
                                | AnnotationKind::Table(_)
                                | AnnotationKind::RevisionTable(_)
                        )
                        .then(|| lines_box(&s.prims))
                        .flatten(),
                        value: s.value,
                        prims: s.prims,
                        orphan: None,
                    },
                    Err(why) => {
                        // Mark it where it would be: the dimension's text, else
                        // the view's middle.
                        let at = match &a.kind {
                            AnnotationKind::Dimension(d) => views
                                .iter()
                                .find(|v| v.id == d.a.view)
                                .map(|v| [v.position[0] + d.text[0], v.position[1] + d.text[1]]),
                            _ => a
                                .view()
                                .and_then(|id| views.iter().find(|v| v.id == id))
                                .map(|v| v.position),
                        };
                        ShownAnn {
                            id: a.id,
                            ink,
                            area: None,
                            value: None,
                            prims: Vec::new(),
                            orphan: match why {
                                Orphan::NoView => None,
                                Orphan::Missing(_) => at,
                            },
                        }
                    }
                },
            )
            .collect()
    }

    /// Paints the annotations (selected blue, orphans red).
    pub(super) fn paint_annotations(&self, p: &egui::Painter, rect: Rect, anns: &[ShownAnn]) {
        let cam = self.drw.cam;
        // The Text window previews its note live; the note it edits waits.
        let editing = self.drw.text_dialog.as_ref().and_then(|d| d.edit);
        if let Some(k) = self.drw.text_dialog.as_ref().and_then(|d| d.preview())
            && let Ok(s) = annot::resolve_with(&k, &|_| None, &self.drw.look)
        {
            paint_prims_styled(p, &cam, rect, &s.prims, PREVIEW);
        }
        for a in anns {
            if editing == Some(a.id) {
                continue;
            }
            if self.drw.sel_ann == Some(a.id) {
                paint_prims(p, &cam, rect, &a.prims, SEL);
            } else if self.drw.hover_ann == Some(a.id) {
                paint_prims(p, &cam, rect, &a.prims, HOVER);
            } else {
                // Formatted runs keep their own colours.
                paint_prims_styled(p, &cam, rect, &a.prims, a.ink);
            }
            if let Some(o) = a.orphan {
                let c = cam.to_screen(rect, o);
                p.circle_stroke(c, 9.0, Stroke::new(2.0, ORPHAN));
                p.text(
                    c,
                    Align2::CENTER_CENTER,
                    "?",
                    FontId::proportional(13.0),
                    ORPHAN,
                );
            }
        }
    }

    /// The annotation under `q` (sheet mm), within a few pixels.
    pub(super) fn annotation_at(&self, anns: &[ShownAnn], q: [f64; 2]) -> Option<u32> {
        let tol = 5.0 / self.drw.cam.zoom;
        anns.iter()
            .filter_map(|a| {
                let inside = a.area.is_some_and(|(lo, hi)| {
                    q[0] >= lo[0] && q[0] <= hi[0] && q[1] >= lo[1] && q[1] <= hi[1]
                });
                let d = match a.orphan {
                    Some(o) => (o[0] - q[0]).hypot(o[1] - q[1]) - 9.0 / self.drw.cam.zoom,
                    None if inside => 0.0,
                    None => prims_distance(&a.prims, q),
                };
                (d <= tol).then_some((a.id, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// The view geometry under the pointer: end points and centres first,
    /// then visible edges.
    pub(super) fn geometry_under(
        &self,
        views: &[ShownView],
        rect: Rect,
        pointer: Pos2,
    ) -> Option<GeoPick> {
        let cam = self.drw.cam;
        let mut best: Option<(f32, GeoPick)> = None;
        let mut take = |d: f32, g: GeoPick| {
            if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, g));
            }
        };
        for v in views {
            let Some(l) = v.lines.as_deref() else {
                continue;
            };
            let pl = Placement {
                position: v.position,
                scale: v.scale,
                center: l.center(),
            };
            let scr = |q: [f64; 2]| cam.to_screen(rect, pl.to_sheet(q));
            for e in l.edges.iter().filter(|e| e.visible) {
                let Some(name) = e.name.clone() else {
                    continue;
                };
                for role in [PointRole::Start, PointRole::End, PointRole::Center] {
                    let Some(q) = point_of(e, role) else {
                        continue;
                    };
                    // A circle's ends are where the kernel split it (a
                    // seam): only its centre is a point to pick.
                    if role != PointRole::Center && e.circle.is_some() {
                        continue;
                    }
                    let d = scr(q).distance(pointer);
                    if d <= POINT_PX {
                        take(
                            d - POINT_PX, // points win over curves
                            GeoPick {
                                view: v.id,
                                edge: name.clone(),
                                what: PickWhat::Point(role),
                                at: pl.to_sheet(q),
                            },
                        );
                    }
                }
            }
            // Edges: the distance to their visible pieces.
            for (k, piece) in l.visible.iter().enumerate() {
                let Some(src) = l.visible_src.get(k).copied().flatten() else {
                    continue;
                };
                let Some(e) = l.edges.get(src as usize) else {
                    continue;
                };
                let Some(name) = e.name.clone() else {
                    continue;
                };
                let d = piece
                    .windows(2)
                    .map(|w| {
                        let (a, b) = (scr(w[0]), scr(w[1]));
                        let ab = b - a;
                        let t = ((pointer - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                        (a + ab * t).distance(pointer)
                    })
                    .fold(f32::MAX, f32::min);
                if d <= CURVE_PX {
                    let what = match e.circle {
                        Some((c, r)) => PickWhat::Circle(
                            r * pl.scale,
                            e.full_circle || whole_circle(e.a, e.b, c),
                        ),
                        None if e.straight => PickWhat::Line,
                        None => continue,
                    };
                    let at = match e.circle {
                        Some((c, _)) => pl.to_sheet(c),
                        None => pl.to_sheet(e.a),
                    };
                    take(
                        d,
                        GeoPick {
                            view: v.id,
                            edge: name,
                            what,
                            at,
                        },
                    );
                }
            }
        }
        best.map(|(_, g)| g)
    }

    /// The points a pick stands for (dimension sets): a point itself, a
    /// straight edge's two ends, a circle's centre; with their sheet places.
    pub(super) fn pick_points(
        &self,
        views: &[ShownView],
        g: &GeoPick,
    ) -> Vec<(annot::PointRef, [f64; 2])> {
        let r = |role| annot::PointRef {
            view: g.view,
            edge: g.edge.clone(),
            role,
        };
        match g.what {
            PickWhat::Point(role) => vec![(r(role), g.at)],
            PickWhat::Circle(..) => vec![(r(PointRole::Center), g.at)],
            PickWhat::Line => {
                let Some((pl, l)) = lookup(views).get(&g.view).copied() else {
                    return Vec::new();
                };
                let Some(e) = l.edge(&g.edge) else {
                    return Vec::new();
                };
                vec![
                    (r(PointRole::Start), pl.to_sheet(e.a)),
                    (r(PointRole::End), pl.to_sheet(e.b)),
                ]
            }
        }
    }

    /// An end point or centre of view geometry under the pointer (sheet mm).
    pub(super) fn point_under(
        &self,
        views: &[ShownView],
        rect: Rect,
        pointer: Pos2,
    ) -> Option<[f64; 2]> {
        self.geometry_under(views, rect, pointer)
            .filter(|g| matches!(g.what, PickWhat::Point(_)))
            .map(|g| g.at)
    }

    /// A straight edge under the pointer: its view and its ends in that
    /// view's plane.
    pub(super) fn straight_edge_under(
        &self,
        views: &[ShownView],
        rect: Rect,
        pointer: Pos2,
    ) -> Option<(u32, [f64; 2], [f64; 2])> {
        let g = self
            .geometry_under(views, rect, pointer)
            .filter(|g| g.what == PickWhat::Line)?;
        let v = views.iter().find(|v| v.id == g.view)?;
        let e = v.lines.as_deref()?.edge(&g.edge)?;
        Some((g.view, e.a, e.b))
    }

    /// Highlights a pick: the edge's visible pieces, or the point.
    fn paint_pick(
        &self,
        p: &egui::Painter,
        rect: Rect,
        views: &[ShownView],
        g: &GeoPick,
        color: Color32,
    ) {
        let cam = self.drw.cam;
        match g.what {
            PickWhat::Point(_) => {
                p.circle_filled(cam.to_screen(rect, g.at), 4.0, color);
            }
            PickWhat::Line | PickWhat::Circle(..) => {
                let Some(v) = views.iter().find(|v| v.id == g.view) else {
                    return;
                };
                let Some(l) = v.lines.as_deref() else {
                    return;
                };
                let Some(src) = l
                    .edges
                    .iter()
                    .position(|e| e.name.as_deref() == Some(&g.edge))
                else {
                    return;
                };
                let pl = Placement {
                    position: v.position,
                    scale: v.scale,
                    center: l.center(),
                };
                for (k, piece) in l.visible.iter().enumerate() {
                    // Every piece of the same circle lights up with it.
                    let same = |i: u32| {
                        i == src as u32
                            || l.edges.get(i as usize).zip(l.edges.get(src)).is_some_and(
                                |(x, y)| match (x.circle, y.circle) {
                                    (Some((c1, r1)), Some((c2, r2))) => {
                                        (c1[0] - c2[0]).hypot(c1[1] - c2[1]) < r1 * 1e-4 + 1e-6
                                            && (r1 - r2).abs() < r1 * 1e-4 + 1e-6
                                    }
                                    _ => false,
                                },
                            )
                    };
                    if l.visible_src.get(k).copied().flatten().is_some_and(same) {
                        let pts: Vec<Pos2> = piece
                            .iter()
                            .map(|q| cam.to_screen(rect, pl.to_sheet(*q)))
                            .collect();
                        p.add(Shape::line(pts, Stroke::new(3.0, color)));
                    }
                }
            }
        }
    }

    /// The running annotation tool (Center Mark, Centerline, Dimension,
    /// Text).
    pub(super) fn annotation_tool(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        views: &[ShownView],
    ) {
        let Some(tool) = self.drw.tool.clone() else {
            return;
        };
        let pointer = response.hover_pos();
        let at = pointer.map(|p| self.drw.cam.to_sheet(rect, p));
        let under = pointer.and_then(|p| self.geometry_under(views, rect, p));
        match tool {
            DrwTool::CenterMark => {
                let circle = under.filter(|g| {
                    matches!(
                        g.what,
                        PickWhat::Circle(..) | PickWhat::Point(PointRole::Center)
                    )
                });
                if let Some(g) = &circle {
                    self.paint_pick(painter, rect, views, g, HOVER);
                }
                if response.clicked()
                    && let Some(g) = circle
                {
                    let exists = self.drawing().and_then(|d| d.sheet()).is_some_and(|s| {
                        s.annotations.iter().any(|a| {
                            matches!(&a.kind, AnnotationKind::CenterMark { view, edge }
                                if *view == g.view && *edge == g.edge)
                        })
                    });
                    if exists {
                        self.file_status =
                            Some(Err("Center Mark: this circle has one already".into()));
                    } else {
                        self.edit_drawing("Center Mark", |d| {
                            d.add_annotation(AnnotationKind::CenterMark {
                                view: g.view,
                                edge: g.edge.clone(),
                            })
                        });
                    }
                }
            }
            DrwTool::Centerline(first) => {
                let circle = under.filter(|g| {
                    matches!(
                        g.what,
                        PickWhat::Circle(..) | PickWhat::Point(PointRole::Center)
                    ) && first
                        .as_ref()
                        .is_none_or(|(v, e)| g.view == *v && g.edge != *e)
                });
                if let Some(g) = &circle {
                    self.paint_pick(painter, rect, views, g, HOVER);
                }
                if let (Some((v, e)), Some(p)) = (&first, at) {
                    let g = GeoPick {
                        view: *v,
                        edge: e.clone(),
                        what: PickWhat::Circle(0.0, true),
                        at: p,
                    };
                    self.paint_pick(painter, rect, views, &g, SEL);
                }
                if response.clicked()
                    && let Some(g) = circle
                {
                    match first {
                        None => self.drw.tool = Some(DrwTool::Centerline(Some((g.view, g.edge)))),
                        Some((v, a)) => {
                            self.edit_drawing("Centerline", |d| {
                                d.add_annotation(AnnotationKind::Centerline {
                                    view: v,
                                    a,
                                    b: g.edge.clone(),
                                })
                            });
                            self.drw.tool = Some(DrwTool::Centerline(None));
                        }
                    }
                }
            }
            DrwTool::Dimension(draft) => {
                self.dimension_tool(painter, rect, response, views, draft, under, at)
            }
            DrwTool::Text => {
                if response.clicked()
                    && let Some(q) = at
                {
                    self.drw.text_dialog = Some(TextDialog::new(super::drawing::snap(q), None));
                }
            }
            DrwTool::LeaderText(tip) => {
                let snapped = pointer
                    .and_then(|p| self.point_under(views, rect, p))
                    .or(at);
                match tip {
                    None => {
                        if let Some(q) = snapped {
                            painter.circle_filled(self.drw.cam.to_screen(rect, q), 4.0, HOVER);
                            if response.clicked() {
                                self.drw.tool = Some(DrwTool::LeaderText(Some(q)));
                            }
                        }
                    }
                    Some(t) => {
                        if let Some(q) = at {
                            let cam = self.drw.cam;
                            painter.line_segment(
                                [cam.to_screen(rect, t), cam.to_screen(rect, q)],
                                Stroke::new(1.5, PREVIEW),
                            );
                            if response.clicked() {
                                self.drw.text_dialog =
                                    Some(TextDialog::new(super::drawing::snap(q), Some(t)));
                                self.drw.tool = Some(DrwTool::LeaderText(None));
                            }
                        }
                    }
                }
            }
            DrwTool::HoleNote(picked) => match picked {
                None => {
                    // Only circles that belong to a hole.
                    let hole = under.clone().filter(|g| {
                        matches!(g.what, PickWhat::Circle(..))
                            && views
                                .iter()
                                .find(|v| v.id == g.view)
                                .and_then(|v| v.lines.as_deref())
                                .and_then(|l| l.edge(&g.edge))
                                .is_some_and(|e| e.note.is_some())
                    });
                    if let Some(g) = &hole {
                        self.paint_pick(painter, rect, views, g, HOVER);
                    } else if response.clicked() && under.is_some() {
                        self.file_status = Some(Err(
                            "Hole and Thread: this is not a hole made by a Hole feature".into(),
                        ));
                    }
                    if response.clicked()
                        && let Some(g) = hole
                    {
                        self.drw.tool = Some(DrwTool::HoleNote(Some((g.view, g.edge))));
                    }
                }
                Some((view, edge)) => {
                    let Some(v) = views.iter().find(|v| v.id == view) else {
                        return;
                    };
                    let Some(q) = at else {
                        return;
                    };
                    let kind = AnnotationKind::HoleNote {
                        view,
                        edge: edge.clone(),
                        text: [q[0] - v.position[0], q[1] - v.position[1]],
                    };
                    let map = lookup(views);
                    let look = |id: u32| map.get(&id).copied();
                    if let Ok(s) = annot::resolve_with(&kind, &look, &self.drw.look) {
                        paint_prims(painter, &self.drw.cam, rect, &s.prims, PREVIEW);
                    }
                    if response.clicked() {
                        self.edit_drawing("Hole and Thread", |d| d.add_annotation(kind));
                        self.drw.tool = Some(DrwTool::HoleNote(None));
                    }
                }
            },
            _ => {}
        }
    }

    /// General Dimension: pick one or two items, then click to place.
    #[allow(clippy::too_many_arguments)]
    fn dimension_tool(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        views: &[ShownView],
        draft: DimDraft,
        under: Option<GeoPick>,
        at: Option<[f64; 2]>,
    ) {
        for g in draft.first.iter().chain(draft.second.iter()) {
            self.paint_pick(painter, rect, views, g, SEL);
        }
        // What the next click does: take more geometry, or place.
        let wants_more = match (&draft.first, &draft.second) {
            (None, _) => true,
            (Some(f), None) => matches!(f.what, PickWhat::Point(_) | PickWhat::Line),
            _ => false,
        };
        let candidate = under.filter(|g| {
            wants_more
                && match &draft.first {
                    None => true,
                    // The second item: a point, or a line after a line, of
                    // the same view (a line's own points are no second item).
                    Some(f) => {
                        g.view == f.view
                            && match (&f.what, &g.what) {
                                (PickWhat::Point(_), PickWhat::Point(_)) => *g != *f,
                                (PickWhat::Line, PickWhat::Line | PickWhat::Point(_)) => {
                                    g.edge != f.edge
                                }
                                _ => false,
                            }
                    }
                }
        });
        if let Some(g) = &candidate {
            self.paint_pick(painter, rect, views, g, HOVER);
        }
        let preview = at.and_then(|t| dimension_from(&draft, t, views));
        if let Some(d) = &preview
            && candidate.is_none()
        {
            let map = lookup(views);
            let view = |id: u32| map.get(&id).copied();
            if let Ok(s) =
                annot::resolve_with(&AnnotationKind::Dimension(d.clone()), &view, &self.drw.look)
            {
                paint_prims(painter, &self.drw.cam, rect, &s.prims, PREVIEW);
            }
        }
        if !response.clicked() {
            return;
        }
        if let Some(g) = candidate {
            let mut next = draft;
            if next.first.is_none() {
                next.first = Some(g);
            } else {
                next.second = Some(g);
            }
            self.drw.tool = Some(DrwTool::Dimension(next));
            return;
        }
        if let Some(d) = preview {
            let value = {
                let map = lookup(views);
                let view = |id: u32| map.get(&id).copied();
                annot::resolve_with(&AnnotationKind::Dimension(d.clone()), &view, &self.drw.look)
                    .ok()
                    .and_then(|s| s.value)
            };
            self.edit_drawing("Dimension", |doc| {
                doc.add_annotation(AnnotationKind::Dimension(d))
            });
            if let Some(v) = value {
                self.file_status = Some(Ok(format!(
                    "Dimension: {} placed; pick the next geometry (Esc ends)",
                    annot::format_value(v, 2)
                )));
            }
            self.drw.tool = Some(DrwTool::Dimension(DimDraft::default()));
        }
    }

    // ---- selection of annotations ------------------------------------------------

    /// No command: hover, select, drag the text of a dimension / a note,
    /// double-click a note to edit it. Returns whether the click or drag was
    /// taken (the views are not picked then).
    pub(super) fn annotation_select(
        &mut self,
        rect: Rect,
        response: &egui::Response,
        anns: &[ShownAnn],
        views: &[ShownView],
    ) -> bool {
        let cam = self.drw.cam;
        let at = response.hover_pos().map(|p| cam.to_sheet(rect, p));
        self.drw.hover_ann = at.and_then(|q| self.annotation_at(anns, q));
        let mut taken = false;
        if response.drag_started_by(egui::PointerButton::Primary) {
            let origin = response
                .ctx
                .input(|i| i.pointer.press_origin())
                .map(|p| cam.to_sheet(rect, p));
            if let Some(q) = origin
                && let Some(id) = self.annotation_at(anns, q)
            {
                self.drw.sel_ann = Some(id);
                self.drw.sel = None;
                self.drw.ann_drag = Some((id, q));
                let kind = self
                    .drawing()
                    .and_then(|d| d.sheet())
                    .and_then(|s| s.annotations.iter().find(|a| a.id == id))
                    .map(|a| a.kind.clone());
                let near = 2.0f64.max(6.0 / cam.zoom);
                self.drw.ann_part = match kind {
                    Some(AnnotationKind::Dimension(_)) => {
                        let prims = anns.iter().find(|a| a.id == id).map(|a| a.prims.clone());
                        if prims.is_some_and(|p| text_distance(&p, q) < near * 0.5) {
                            DragPart::Text
                        } else {
                            DragPart::Whole
                        }
                    }
                    Some(AnnotationKind::LeaderText { tip, .. })
                        if (tip[0] - q[0]).hypot(tip[1] - q[1]) < near =>
                    {
                        DragPart::Tip
                    }
                    _ => DragPart::Whole,
                };
                taken = true;
            }
        }
        if let (Some((id, last)), Some(q)) = (self.drw.ann_drag, at)
            && response.dragged_by(egui::PointerButton::Primary)
        {
            let delta = [q[0] - last[0], q[1] - last[1]];
            // A dimension's text slides along its line by the drag's part
            // along it.
            let part = self.drw.ann_part;
            let along = {
                let map = lookup(views);
                let view = |vid: u32| map.get(&vid).copied();
                self.drawing()
                    .and_then(|d| d.sheet())
                    .and_then(|s| s.annotations.iter().find(|a| a.id == id))
                    .and_then(|a| match &a.kind {
                        AnnotationKind::Dimension(dim) => annot::linear_ends(dim, &view),
                        _ => None,
                    })
                    .map_or(0.0, |(_, _, v)| delta[0] * v[0] + delta[1] * v[1])
            };
            if delta[0].hypot(delta[1]) > 1e-9 {
                self.edit_drawing("Move Annotation", |d| {
                    if let Some(a) = d
                        .sheet_mut()
                        .and_then(|s| s.annotations.iter_mut().find(|a| a.id == id))
                    {
                        match &mut a.kind {
                            AnnotationKind::Dimension(dim) => {
                                dim.text[0] += delta[0];
                                dim.text[1] += delta[1];
                                if part == DragPart::Text {
                                    dim.shift += along;
                                }
                            }
                            AnnotationKind::LeaderText { tip, .. } if part == DragPart::Tip => {
                                tip[0] += delta[0];
                                tip[1] += delta[1];
                            }
                            AnnotationKind::Text { at, .. }
                            | AnnotationKind::LeaderText { at, .. } => {
                                at[0] += delta[0];
                                at[1] += delta[1];
                            }
                            AnnotationKind::HoleNote { text, .. } => {
                                text[0] += delta[0];
                                text[1] += delta[1];
                            }
                            AnnotationKind::ChamferNote(n) => {
                                n.text[0] += delta[0];
                                n.text[1] += delta[1];
                            }
                            AnnotationKind::BendNote { offset, .. } => {
                                offset[0] += delta[0];
                                offset[1] += delta[1];
                            }
                            AnnotationKind::DimSet(s) => {
                                s.at[0] += delta[0];
                                s.at[1] += delta[1];
                            }
                            AnnotationKind::HoleTable(h) => {
                                h.at[0] += delta[0];
                                h.at[1] += delta[1];
                            }
                            AnnotationKind::PartsList(pl) => {
                                pl.at[0] += delta[0];
                                pl.at[1] += delta[1];
                            }
                            AnnotationKind::BendTable(bt) => {
                                bt.at[0] += delta[0];
                                bt.at[1] += delta[1];
                            }
                            AnnotationKind::Balloon(b) => {
                                b.at[0] += delta[0];
                                b.at[1] += delta[1];
                            }
                            AnnotationKind::Table(t) => {
                                t.at[0] += delta[0];
                                t.at[1] += delta[1];
                            }
                            AnnotationKind::RevisionTable(t) => {
                                t.at[0] += delta[0];
                                t.at[1] += delta[1];
                            }
                            AnnotationKind::RevisionTag(t) => {
                                t.at[0] += delta[0];
                                t.at[1] += delta[1];
                            }
                            AnnotationKind::RevisionCloud(c) => {
                                for p in &mut c.pts {
                                    p[0] += delta[0];
                                    p[1] += delta[1];
                                }
                            }
                            _ => {}
                        }
                    }
                });
                self.drw.ann_drag = Some((id, q));
            }
            taken = true;
        }
        if response.drag_stopped()
            && let Some((id, _)) = self.drw.ann_drag.take()
        {
            // A text left near the middle snaps to it.
            let snap = 1.5f64.max(8.0 / cam.zoom);
            let near_middle = self
                .drawing()
                .and_then(|d| d.sheet())
                .and_then(|s| s.annotations.iter().find(|a| a.id == id))
                .is_some_and(|a| {
                    matches!(&a.kind, AnnotationKind::Dimension(d)
                        if d.shift != 0.0 && d.shift.abs() < snap)
                });
            if near_middle {
                self.edit_drawing("Move Annotation", |d| {
                    if let Some(AnnotationKind::Dimension(dim)) = d
                        .sheet_mut()
                        .and_then(|s| s.annotations.iter_mut().find(|a| a.id == id))
                        .map(|a| &mut a.kind)
                    {
                        dim.shift = 0.0;
                    }
                });
            }
            self.drw.ann_part = DragPart::Whole;
            self.history.break_coalescing();
            taken = true;
        }
        if response.double_clicked()
            && let Some(id) = self.drw.hover_ann
        {
            self.open_text_edit(id);
            return true;
        }
        // A click on an arrowhead of the selected dimension flips it inside /
        // outside (Inventor's arrowhead grip).
        if response.clicked()
            && let (Some(id), Some(q)) = (self.drw.sel_ann, at)
            && self.drw.hover_ann == Some(id)
            && self.flip_arrow_at(id, q, views)
        {
            return true;
        }
        if response.clicked() || response.secondary_clicked() {
            if let Some(id) = self.drw.hover_ann {
                self.drw.sel_ann = Some(id);
                self.drw.sel = None;
                taken = true;
            } else {
                self.drw.sel_ann = None;
            }
        }
        let _ = views;
        taken
    }

    /// Flips the arrowhead of dimension `id` nearest `q` when `q` is on it.
    fn flip_arrow_at(&mut self, id: u32, q: [f64; 2], views: &[ShownView]) -> bool {
        let map = lookup(views);
        let view = |vid: u32| map.get(&vid).copied();
        let Some(dim) = self
            .drawing()
            .and_then(|d| d.sheet())
            .and_then(|s| s.annotations.iter().find(|a| a.id == id))
            .and_then(|a| match &a.kind {
                AnnotationKind::Dimension(d) => Some(d.clone()),
                _ => None,
            })
        else {
            return false;
        };
        let Some((la, lb, _)) = annot::linear_ends(&dim, &view) else {
            return false;
        };
        let near = 2.0f64.max(7.0 / self.drw.cam.zoom);
        let k = if (la[0] - q[0]).hypot(la[1] - q[1]) < near {
            0
        } else if (lb[0] - q[0]).hypot(lb[1] - q[1]) < near {
            1
        } else {
            return false;
        };
        let ds = &self.drw.look.dim;
        let span = (lb[0] - la[0]).hypot(lb[1] - la[1]);
        let out_now = match dim.arrows[k] {
            annot::ArrowSide::Auto => span < 2.0 * ds.arrow_size + 1.0,
            annot::ArrowSide::Inside => false,
            annot::ArrowSide::Outside => true,
        };
        self.edit_drawing("Flip Arrowhead", move |d| {
            if let Some(AnnotationKind::Dimension(x)) = d
                .sheet_mut()
                .and_then(|s| s.annotations.iter_mut().find(|a| a.id == id))
                .map(|a| &mut a.kind)
            {
                x.arrows[k] = if out_now {
                    annot::ArrowSide::Inside
                } else {
                    annot::ArrowSide::Outside
                };
            }
        });
        true
    }

    fn open_text_edit(&mut self, id: u32) {
        let kind = self
            .drawing()
            .and_then(|d| d.sheet())
            .and_then(|s| s.annotations.iter().find(|a| a.id == id))
            .map(|a| a.kind.clone());
        let (at, text, height, leader, rich) = match kind {
            Some(AnnotationKind::Text {
                at,
                text,
                height,
                rich,
            }) => (at, text, height, None, rich),
            Some(AnnotationKind::LeaderText {
                tip,
                at,
                text,
                rich,
            }) => (at, text, 3.5, Some(tip), rich),
            Some(AnnotationKind::PartsList(_)) => {
                self.open_parts_list_editor(id);
                return;
            }
            Some(AnnotationKind::BendTable(_)) => {
                self.open_bend_table_edit(id);
                return;
            }
            Some(AnnotationKind::HoleTable(_)) => {
                self.open_hole_table_edit(id);
                return;
            }
            Some(AnnotationKind::ChamferNote(_)) => {
                self.open_chamfer_edit(id);
                return;
            }
            Some(AnnotationKind::Balloon(b)) => {
                self.open_balloon_edit(id, &b);
                return;
            }
            Some(AnnotationKind::Dimension(_)) => {
                self.open_dim_edit(id);
                return;
            }
            _ => {
                self.open_rev_edit(id);
                return;
            }
        };
        let mut d = TextDialog::new(at, leader);
        d.text = text;
        d.height = height;
        d.edit = Some(id);
        d.rich = rich.unwrap_or_default();
        self.drw.text_dialog = Some(d);
    }

    /// Deletes annotation `id` (one Undo step).
    pub(super) fn delete_annotation(&mut self, id: u32) {
        if self.edit_drawing("Delete Annotation", |d| d.delete_annotation(id)) != Some(true) {
            self.history.cancel();
        }
        self.drw.sel_ann = None;
    }

    // ---- windows ------------------------------------------------------------------------

    /// The Text and PDF windows.
    pub(super) fn annotation_windows(&mut self, ctx: &egui::Context) {
        self.text_window(ctx);
        self.pdf_window(ctx);
    }

    fn text_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.drw.text_dialog.clone() else {
            return;
        };
        let mut open = true;
        let (mut ok, mut cancel) = (false, false);
        let title = if d.edit.is_some() {
            "Edit Text"
        } else {
            "Format Text"
        };
        let win = egui::Window::new(title)
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(520.0);
        self.layout
            .place("drawing_text", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                if d.focus {
                    d.ed.focus = true;
                    d.focus = false;
                }
                super::format_text::editor(ui, &mut d.text, &mut d.rich, &mut d.ed, &mut d.height);
                ui.weak("Enter starts a new line; Ctrl+Enter or OK places the text");
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() || super::keys::panel_ok_key(ui, true) {
                        ok = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel || !open {
            self.drw.text_dialog = None;
            return;
        }
        if ok {
            self.drw.text_dialog = None;
            let text = d.text.trim_end().to_string();
            if text.is_empty() {
                return;
            }
            let kind = d.kind(text);
            match d.edit {
                Some(id) => {
                    self.edit_drawing("Edit Text", |doc| {
                        if let Some(a) = doc
                            .sheet_mut()
                            .and_then(|s| s.annotations.iter_mut().find(|a| a.id == id))
                        {
                            a.kind = kind;
                        }
                    });
                }
                None => {
                    self.edit_drawing("Text", |doc| doc.add_annotation(kind));
                }
            }
            return;
        }
        self.drw.text_dialog = Some(d);
    }

    /// File › Export › PDF: the options first (Inventor's PDF Drawing
    /// options), then the file name.
    pub(super) fn open_pdf_export(&mut self) {
        let n = self.drawing().map_or(1, |d| d.sheets.len());
        self.drw.pdf_dialog = Some(PdfDialog {
            range: if n > 1 {
                PdfRange::All
            } else {
                PdfRange::Current
            },
            from: 1,
            to: n,
            excluded: false,
            remove_weights: false,
            all_black: false,
            open_after: false,
        });
    }

    fn pdf_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.drw.pdf_dialog.clone() else {
            return;
        };
        let n = self.drawing().map_or(1, |x| x.sheets.len());
        let mut open = true;
        let (mut ok, mut cancel) = (false, false);
        let win = egui::Window::new("PDF Drawing Options")
            .open(&mut open)
            .resizable(false)
            .collapsible(false);
        self.layout
            .place("drawing_pdf", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                ui.label(RichText::new("Print Range").strong());
                ui.radio_value(&mut d.range, PdfRange::Current, "Current Sheet");
                ui.radio_value(&mut d.range, PdfRange::All, "All Sheets");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut d.range, PdfRange::Range, "Sheets in Range");
                    ui.add_enabled_ui(d.range == PdfRange::Range, |ui| {
                        ui.label("From");
                        ui.add(ExprValue::new(&mut d.from).range(1..=n));
                        ui.label("To");
                        ui.add(ExprValue::new(&mut d.to).range(1..=n));
                    });
                });
                ui.checkbox(&mut d.excluded, "Print Excluded Sheets");
                ui.separator();
                ui.label(RichText::new("Settings").strong());
                ui.checkbox(&mut d.remove_weights, "Remove Object Line Weights");
                ui.checkbox(&mut d.all_black, "All Colors as Black")
                    .on_hover_text("Every layer colour prints black");
                ui.add_enabled_ui(false, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Vector Resolution");
                        ui.label("vector (no raster parts)");
                    });
                })
                .response
                .on_disabled_hover_text("Matters with shaded / raster views (planned)");
                ui.checkbox(&mut d.open_after, "Display Published File in Viewer");
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
            self.drw.pdf_dialog = None;
            return;
        }
        self.drw.pdf_dialog = Some(d);
        if ok {
            self.open_dialog(super::files::DialogPurpose::ExportPdf);
        }
    }

    /// Writes the PDF chosen in the options to `path`.
    pub(super) fn write_drawing_pdf(&mut self, path: &std::path::Path) {
        let Some(opts) = self.drw.pdf_dialog.take() else {
            return;
        };
        let Some(doc) = self.drawing().cloned() else {
            return;
        };
        let indices: Vec<usize> = match opts.range {
            PdfRange::Current => vec![doc.active_sheet],
            PdfRange::All => (0..doc.sheets.len()).collect(),
            PdfRange::Range => {
                let (a, b) = (opts.from.min(opts.to).max(1), opts.from.max(opts.to));
                (a - 1..b.min(doc.sheets.len())).collect()
            }
        };
        let indices: Vec<usize> = indices
            .into_iter()
            .filter(|&i| opts.excluded || !doc.sheets[i].exclude_from_printing)
            .collect();
        if indices.is_empty() {
            self.file_status = Some(Err("Export PDF: no sheet to print".into()));
            return;
        }
        let mut pages = Vec::new();
        for i in indices {
            let sheet = &doc.sheets[i];
            // Every view's lines, computed now when the cache has none.
            let lines = self.sheet_lines(None, &doc, i);
            let title = self.title_values(&doc, i);
            let prims = crate::drawing::render::sheet_list(
                &doc,
                i,
                &|f| {
                    title
                        .iter()
                        .find(|(k, _)| *k == f)
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default()
                },
                &|id| lines.get(&id).cloned(),
                &self.bom_tables(&doc, i),
            );
            pages.push(crate::drawing::pdf::Page {
                size: sheet.dims(),
                prims,
            });
        }
        let font = egui::FontDefinitions::default()
            .font_data
            .get("Ubuntu-Light")
            .map(|f| f.font.to_vec());
        let result = crate::drawing::pdf::write(
            &pages,
            font,
            crate::drawing::pdf::PdfOptions {
                remove_line_weights: opts.remove_weights,
                all_black: opts.all_black,
            },
        )
        .and_then(|bytes| std::fs::write(path, bytes).map_err(|e| format!("Export PDF: {e}")));
        self.file_status = Some(match result {
            Ok(()) => {
                if opts.open_after {
                    let opener = if cfg!(target_os = "windows") {
                        "explorer"
                    } else if cfg!(target_os = "macos") {
                        "open"
                    } else {
                        "xdg-open"
                    };
                    let _ = std::process::Command::new(opener).arg(path).spawn();
                }
                Ok(format!(
                    "Exported {} sheet(s) to {}",
                    pages.len(),
                    path.display()
                ))
            }
            Err(e) => Err(e),
        });
    }
}

/// A circle edge counts as a whole circle (diameter) when it closes or is a
/// half (the kernel splits circles into halves); smaller arcs get a radius.
fn whole_circle(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    let (u, v) = ([a[0] - c[0], a[1] - c[1]], [b[0] - c[0], b[1] - c[1]]);
    let r = u[0].hypot(u[1]).max(1e-12);
    let closed = (a[0] - b[0]).hypot(a[1] - b[1]) < r * 1e-6;
    let cos = (u[0] * v[0] + u[1] * v[1]) / (r * v[0].hypot(v[1]).max(1e-12));
    closed || cos < -0.9999
}

/// The dimension the draft would make with the text at `t` (sheet mm).
fn dimension_from(draft: &DimDraft, t: [f64; 2], views: &[ShownView]) -> Option<Dimension> {
    let f = draft.first.as_ref()?;
    let v = views.iter().find(|v| v.id == f.view)?;
    let text = [t[0] - v.position[0], t[1] - v.position[1]];
    let pr = |g: &GeoPick, role: PointRole| PointRef {
        view: g.view,
        edge: g.edge.clone(),
        role,
    };
    // Horizontal when the text is above / below the span, vertical when
    // beside it, aligned in the corners.
    let orient = |p1: [f64; 2], p2: [f64; 2]| {
        let (x0, x1) = (p1[0].min(p2[0]), p1[0].max(p2[0]));
        let (y0, y1) = (p1[1].min(p2[1]), p1[1].max(p2[1]));
        if (x1 - x0) < 1e-6 {
            DimKind::Vertical
        } else if (y1 - y0) < 1e-6 || (t[0] > x0 && t[0] < x1) {
            DimKind::Horizontal
        } else if t[1] > y0 && t[1] < y1 {
            DimKind::Vertical
        } else {
            DimKind::Aligned
        }
    };
    match (&f.what, &draft.second) {
        (PickWhat::Circle(_, full), _) => Some(Dimension {
            shift: 0.0,
            arrows: Default::default(),
            dim: if *full {
                DimKind::Diameter
            } else {
                DimKind::Radius
            },
            a: pr(f, PointRole::Center),
            b: None,
            text,
            precision: 2,
            format: Default::default(),
        }),
        (PickWhat::Line, Some(s)) => {
            // Line to line or line to point: the distance across the first
            // line (horizontal for an upright line, vertical for a level
            // one), else as the text's place says.
            let l = v.lines.as_deref()?;
            let e = l.edge(&f.edge)?;
            let pl = Placement {
                position: v.position,
                scale: v.scale,
                center: l.center(),
            };
            let (a, b) = (pl.to_sheet(e.a), pl.to_sheet(e.b));
            let (dx, dy) = ((b[0] - a[0]).abs(), (b[1] - a[1]).abs());
            let (role, q) = match s.what {
                PickWhat::Point(r) => (r, s.at),
                PickWhat::Line => {
                    let e2 = l.edge(&s.edge)?;
                    // Two edges that are not parallel: their angle.
                    let (c, d) = (pl.to_sheet(e2.a), pl.to_sheet(e2.b));
                    let (u1, u2) = ([b[0] - a[0], b[1] - a[1]], [d[0] - c[0], d[1] - c[1]]);
                    let cross = u1[0] * u2[1] - u1[1] * u2[0];
                    let len = u1[0].hypot(u1[1]) * u2[0].hypot(u2[1]);
                    if s.view == f.view
                        && len > 1e-12
                        && (cross / len).abs() > 0.5f64.to_radians().sin()
                    {
                        return Some(Dimension {
                            shift: 0.0,
                            arrows: Default::default(),
                            dim: DimKind::Angle,
                            a: pr(f, PointRole::Start),
                            b: Some(pr(s, PointRole::Start)),
                            text,
                            precision: 2,
                            format: Default::default(),
                        });
                    }
                    (PointRole::Start, pl.to_sheet(e2.a))
                }
                PickWhat::Circle(..) => return None,
            };
            let dim = if dx < 1e-6 * dy.max(1.0) {
                DimKind::Horizontal
            } else if dy < 1e-6 * dx.max(1.0) {
                DimKind::Vertical
            } else {
                orient(a, q)
            };
            Some(Dimension {
                shift: 0.0,
                arrows: Default::default(),
                dim,
                a: pr(f, PointRole::Start),
                b: Some(pr(s, role)),
                text,
                precision: 2,
                format: Default::default(),
            })
        }
        (PickWhat::Line, None) => {
            let l = v.lines.as_deref()?;
            let e = l.edge(&f.edge)?;
            let pl = Placement {
                position: v.position,
                scale: v.scale,
                center: l.center(),
            };
            Some(Dimension {
                shift: 0.0,
                arrows: Default::default(),
                dim: orient(pl.to_sheet(e.a), pl.to_sheet(e.b)),
                a: pr(f, PointRole::Start),
                b: Some(pr(f, PointRole::End)),
                text,
                precision: 2,
                format: Default::default(),
            })
        }
        (PickWhat::Point(r1), Some(s)) => {
            let PickWhat::Point(r2) = s.what else {
                return None;
            };
            Some(Dimension {
                shift: 0.0,
                arrows: Default::default(),
                dim: orient(f.at, s.at),
                a: pr(f, *r1),
                b: Some(pr(s, r2)),
                text,
                precision: 2,
                format: Default::default(),
            })
        }
        (PickWhat::Point(_), None) => None,
    }
}
