//! Arrange Dimensions (AD, `docs/spec/4.7-drawing-commands.md`): click
//! dimensions (a selected one is taken at the start), then Enter or
//! right-click arranges them — with none picked, every dimension of the
//! sheet (`drawing::arrange`). One Undo step; the command ends.

use eframe::egui::{self, Color32, Rect};

use super::CadCraftApp;
use super::drawing::{DrwTool, ShownView};
use super::drawing_annot::{lookup, paint_prims};
use crate::drawing::AnnotationKind;

const PICKED: Color32 = Color32::from_rgb(40, 120, 230);

impl CadCraftApp {
    /// Arrange Dimensions starts; a selected dimension is the first pick.
    pub(super) fn start_arrange(&mut self) {
        let first = self.drw.sel_ann.filter(|id| self.is_dimension(*id));
        self.drw.dialog = None;
        self.drw.sel = None;
        self.drw.tool = Some(DrwTool::Arrange(first.into_iter().collect()));
    }

    fn is_dimension(&self, id: u32) -> bool {
        self.drawing().and_then(|d| d.sheet()).is_some_and(|s| {
            s.annotations
                .iter()
                .any(|a| a.id == id && matches!(a.kind, AnnotationKind::Dimension(_)))
        })
    }

    pub(super) fn arrange_prompt(picked: &[u32]) -> String {
        if picked.is_empty() {
            "Arrange Dimensions: click dimensions, then Enter or right-click (none: all of the sheet)"
                .into()
        } else {
            format!(
                "Arrange Dimensions: {} picked; more, or Enter / right-click to arrange",
                picked.len()
            )
        }
    }

    pub(super) fn arrange_tool(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        views: &[ShownView],
    ) {
        let Some(DrwTool::Arrange(mut picked)) = self.drw.tool.clone() else {
            return;
        };
        let cam = self.drw.cam;
        let anns = self.shown_annotations(views);
        for a in anns.iter().filter(|a| picked.contains(&a.id())) {
            paint_prims(painter, &cam, rect, a.prims(), PICKED);
        }
        if response.clicked()
            && let Some(q) = response.hover_pos().map(|p| cam.to_sheet(rect, p))
            && let Some(id) = self.annotation_at(&anns, q)
            && self.is_dimension(id)
        {
            if let Some(k) = picked.iter().position(|x| *x == id) {
                picked.remove(k);
            } else {
                picked.push(id);
            }
        }
        let go = response.secondary_clicked()
            || (response.hovered() && painter.ctx().input(|i| i.key_pressed(egui::Key::Enter)));
        if !go {
            self.drw.tool = Some(DrwTool::Arrange(picked));
            return;
        }
        self.drw.tool = None;
        let Some(sheet) = self.drawing().and_then(|d| d.sheet()).cloned() else {
            return;
        };
        let dims: Vec<(u32, &crate::drawing::Dimension)> = sheet
            .annotations
            .iter()
            .filter(|a| picked.is_empty() || picked.contains(&a.id))
            .filter_map(|a| match &a.kind {
                AnnotationKind::Dimension(d) => Some((a.id, d)),
                _ => None,
            })
            .collect();
        let map = lookup(views);
        let get = |id: u32| map.get(&id).copied();
        let ds = &self.drw.look.dim;
        let moves = crate::drawing::arrange::arrange(&dims, &get, ds.part_offset, ds.spacing);
        if moves.is_empty() {
            self.file_status = Some(Err(
                "Arrange Dimensions: no horizontal or vertical dimension outside a view".into(),
            ));
            return;
        }
        let n = moves.len();
        self.edit_drawing("Arrange Dimensions", move |d| {
            if let Some(s) = d.sheet_mut() {
                for (id, t) in moves {
                    if let Some(AnnotationKind::Dimension(x)) = s
                        .annotations
                        .iter_mut()
                        .find(|a| a.id == id)
                        .map(|a| &mut a.kind)
                    {
                        x.text = t;
                    }
                }
            }
        });
        self.file_status = Some(Ok(format!("Arrange Dimensions: {n} arranged")));
    }
}
