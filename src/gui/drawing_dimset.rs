//! Baseline / Chain / Ordinate Dimension (and their Sets) in a drawing
//! (`docs/spec/4.7-drawing-commands.md`): click edges and points of one
//! view (the first pick is the origin), right-click › Continue or Enter,
//! then the dimensions follow the cursor — above / below the points they
//! measure along x, beside them along y — and a click creates them.
//! Backspace takes the last pick back; Esc ends.

use eframe::egui::{self, Color32, Rect};

use super::CadCraftApp;
use super::drawing::{DrwTool, ShownView};
use super::drawing_annot::{lookup, paint_prims};
use crate::drawing::AnnotationKind;
use crate::drawing::annot::{self, PointRef};
use crate::drawing::dimset::{DimSet, SetKind};

const PICKED: Color32 = Color32::from_rgb(40, 120, 230);
const PREVIEW: Color32 = Color32::from_rgb(60, 140, 90);

/// A dimension set command in progress.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct DsDraft {
    pub kind: SetKind,
    /// Kept as one set (the "Set" commands; ordinates always are).
    pub set: bool,
    /// Picked points with their sheet places (the first is the origin).
    picks: Vec<(PointRef, [f64; 2])>,
    /// Continue pressed: the dimensions follow the cursor.
    pub placing: bool,
}

impl DsDraft {
    pub(super) fn new(kind: SetKind, set: bool) -> DsDraft {
        DsDraft {
            kind,
            set,
            picks: Vec::new(),
            placing: false,
        }
    }

    /// The command's name.
    pub(super) fn title(&self) -> String {
        format!(
            "{} Dimension{}",
            self.kind.label(),
            if self.set { " Set" } else { "" }
        )
    }

    /// The status-bar prompt.
    pub(super) fn prompt(&self) -> String {
        let t = self.title();
        if self.placing {
            format!("{t}: click where the dimensions go (above / below: along x, beside: along y)")
        } else if self.picks.is_empty() {
            format!("{t}: click edges and points of one view; the first is the origin")
        } else {
            format!(
                "{t}: {} points; more, or right-click > Continue (Enter); Backspace takes one back",
                self.picks.len()
            )
        }
    }

    /// Continue: at least two points to measure.
    pub(super) fn ready(&self) -> bool {
        self.picks.len() >= 2
    }

    pub(super) fn cont(&mut self) {
        if self.ready() {
            self.placing = true;
        }
    }

    /// The set the cursor at `at` (sheet) gives.
    fn set_at(&self, at: [f64; 2], view_pos: [f64; 2]) -> DimSet {
        let pts: Vec<[f64; 2]> = self.picks.iter().map(|p| p.1).collect();
        DimSet {
            kind: self.kind,
            dir: DimSet::dir_for(&pts, at),
            points: self.picks.iter().map(|p| p.0.clone()).collect(),
            origin: 0,
            at: [at[0] - view_pos[0], at[1] - view_pos[1]],
        }
    }
}

impl CadCraftApp {
    pub(super) fn dim_set_tool(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        views: &[ShownView],
    ) {
        let Some(DrwTool::DimSet(mut d)) = self.drw.tool.clone() else {
            return;
        };
        let cam = self.drw.cam;
        let pointer = response.hover_pos();
        let (enter, back) = if response.hovered() {
            painter.ctx().input(|i| {
                (
                    i.key_pressed(egui::Key::Enter),
                    i.key_pressed(egui::Key::Backspace),
                )
            })
        } else {
            (false, false)
        };
        if back {
            if d.placing {
                d.placing = false;
            } else {
                d.picks.pop();
            }
        }
        if enter && !d.placing {
            d.cont();
        }
        for (k, (_, at)) in d.picks.iter().enumerate() {
            let p = cam.to_screen(rect, *at);
            painter.circle_filled(p, if k == 0 { 5.0 } else { 3.5 }, PICKED);
            if k == 0 {
                painter.circle_stroke(p, 8.0, egui::Stroke::new(1.5, PICKED));
            }
        }
        if !d.placing {
            let hit = pointer.and_then(|p| self.geometry_under(views, rect, p));
            if let Some(g) = &hit {
                for (_, at) in self.pick_points(views, g) {
                    painter.circle_stroke(
                        cam.to_screen(rect, at),
                        5.0,
                        egui::Stroke::new(1.5, Color32::from_rgb(230, 120, 40)),
                    );
                }
            }
            if response.clicked()
                && let Some(g) = hit
            {
                let first = d.picks.first().map(|p| p.0.view);
                if first.is_some_and(|v| v != g.view) {
                    self.file_status = Some(Err(format!(
                        "{}: all points must be in one view",
                        d.title()
                    )));
                } else {
                    for p in self.pick_points(views, &g) {
                        if !d.picks.iter().any(|q| q.0 == p.0) {
                            d.picks.push(p);
                        }
                    }
                }
            }
            self.drw.tool = Some(DrwTool::DimSet(d));
            return;
        }
        // Placing: the set follows the cursor.
        let Some(q) = pointer.map(|p| cam.to_sheet(rect, p)) else {
            self.drw.tool = Some(DrwTool::DimSet(d));
            return;
        };
        let map = lookup(views);
        let Some(view) = d.picks.first().map(|p| p.0.view) else {
            return;
        };
        let Some(pos) = map.get(&view).map(|x| x.0.position) else {
            return;
        };
        let set = d.set_at(q, pos);
        let get = |id: u32| map.get(&id).copied();
        let kind = AnnotationKind::DimSet(set.clone());
        if let Ok(s) = annot::resolve_in(&kind, &get, &self.drw.look, &self.drw.tables) {
            paint_prims(painter, &cam, rect, &s.prims, PREVIEW);
        }
        if response.clicked() {
            let members = if d.set || d.kind == SetKind::Ordinate {
                None
            } else {
                annot::set_members(&set, &get, self.drw.look.dim.spacing).ok()
            };
            let title = d.title();
            self.edit_drawing(&title, move |doc| match members {
                Some(dims) => {
                    for m in dims {
                        doc.add_annotation(AnnotationKind::Dimension(m));
                    }
                }
                None => {
                    doc.add_annotation(kind);
                }
            });
            // The command repeats.
            self.drw.tool = Some(DrwTool::DimSet(DsDraft::new(d.kind, d.set)));
            return;
        }
        self.drw.tool = Some(DrwTool::DimSet(d));
    }
}
