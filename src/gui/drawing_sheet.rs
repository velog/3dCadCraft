//! Sheet metal in drawings (`docs/spec/4.8-sheet-metal-drawings.md`): the
//! Bend Notes command. The flat pattern view itself is the Drawing View
//! dialog's Sheet Metal View option (`gui/drawing.rs`) computed by
//! `drawing_bom::flat_model`; its bend lines are drawn by
//! `drawing::render::view_extras`.

use eframe::egui::{self, Color32, Rect, Stroke};

use super::CadCraftApp;
use super::drawing::ShownView;
use crate::drawing::AnnotationKind;
use crate::drawing::annot::{Placement, nearest_bend};

const HOVER: Color32 = Color32::from_rgb(230, 120, 40);

/// A bend line under the pointer: (view, the bend's middle in the view
/// plane, its ends on the sheet).
type BendHit = (u32, [f64; 2], [[f64; 2]; 2]);

impl CadCraftApp {
    /// The bend centre line nearest sheet point `q`, within a few pixels.
    fn bend_at(&self, views: &[ShownView], q: [f64; 2]) -> Option<BendHit> {
        let tol = 6.0 / self.drw.cam.zoom;
        let mut best: Option<(f64, BendHit)> = None;
        for v in views {
            let Some(l) = v.lines.as_deref() else {
                continue;
            };
            let pl = Placement {
                position: v.position,
                scale: v.scale,
                center: l.center(),
            };
            for b in &l.bends {
                let (a, c) = (pl.to_sheet(b.centre[0]), pl.to_sheet(b.centre[1]));
                let d = [c[0] - a[0], c[1] - a[1]];
                let l2 = (d[0] * d[0] + d[1] * d[1]).max(1e-12);
                let t = (((q[0] - a[0]) * d[0] + (q[1] - a[1]) * d[1]) / l2).clamp(0.0, 1.0);
                let dist = (a[0] + d[0] * t - q[0]).hypot(a[1] + d[1] * t - q[1]);
                if dist <= tol && best.as_ref().is_none_or(|(bd, _)| dist < *bd) {
                    best = Some((dist, (v.id, b.middle(), [a, c])));
                }
            }
        }
        best.map(|(_, h)| h)
    }

    /// Bend Notes: a click on a bend line adds its note (once per bend).
    pub(super) fn bend_note_tool(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        views: &[ShownView],
    ) {
        let cam = self.drw.cam;
        let Some(q) = response.hover_pos().map(|p| cam.to_sheet(rect, p)) else {
            return;
        };
        let hit = self.bend_at(views, q);
        if let Some((_, _, [a, b])) = &hit {
            painter.line_segment(
                [cam.to_screen(rect, *a), cam.to_screen(rect, *b)],
                Stroke::new(3.0, HOVER),
            );
        }
        if !response.clicked() {
            return;
        }
        let Some((view, middle, _)) = hit else {
            if !views
                .iter()
                .any(|v| v.lines.as_ref().is_some_and(|l| !l.bends.is_empty()))
            {
                self.file_status = Some(Err(
                    "Bend Notes: no flat pattern view on this sheet (Drawing View > Sheet Metal View > Flat Pattern)".into(),
                ));
            }
            return;
        };
        // One note per bend.
        let taken = self.drawing().and_then(|d| d.sheet()).is_some_and(|s| {
            s.annotations.iter().any(|a| match &a.kind {
                AnnotationKind::BendNote { view: v, bend, .. } if *v == view => views
                    .iter()
                    .find(|x| x.id == view)
                    .and_then(|x| x.lines.as_deref())
                    .and_then(|l| nearest_bend(l, *bend))
                    .is_some_and(|b| b.middle() == middle),
                _ => false,
            })
        });
        if taken {
            self.file_status = Some(Err("Bend Notes: this bend has a note already".into()));
            return;
        }
        self.edit_drawing("Bend Notes", |d| {
            d.add_annotation(AnnotationKind::BendNote {
                view,
                bend: middle,
                offset: [0.0, 0.0],
            })
        });
    }
}
