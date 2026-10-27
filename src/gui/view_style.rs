//! View › Appearance: **Visual Style** (how faces and edges are drawn) and
//! **Section View** (Quarter / Half / Three Quarter / End: a live cut of the
//! view by one or two planes). `docs/spec/0.3-view.md`. Both are view state:
//! nothing in the part changes, so they need no Undo step.

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::renderer::{Cut, ViewStyle};
use super::valuefield::ExprValue;
use crate::kernel::{Real, Vector3};
use crate::part::PlaneRef;

/// Inventor's visual styles we draw (Realistic, Watercolor, Illustration
/// and Technical Illustration need rendering, FEATURES 7.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VisualStyle {
    Shaded,
    #[default]
    ShadedWithEdges,
    ShadedWithHiddenEdges,
    Wireframe,
    WireframeWithHiddenEdges,
    WireframeWithVisibleEdgesOnly,
    Monochrome,
}

impl VisualStyle {
    /// Inventor's name.
    pub fn label(self) -> &'static str {
        self.label_const()
    }

    /// [`Self::label`] for constants (the command registry).
    pub const fn label_const(self) -> &'static str {
        match self {
            VisualStyle::Shaded => "Shaded",
            VisualStyle::ShadedWithEdges => "Shaded with Edges",
            VisualStyle::ShadedWithHiddenEdges => "Shaded with Hidden Edges",
            VisualStyle::Wireframe => "Wireframe",
            VisualStyle::WireframeWithHiddenEdges => "Wireframe with Hidden Edges",
            VisualStyle::WireframeWithVisibleEdgesOnly => "Wireframe with Visible Edges Only",
            VisualStyle::Monochrome => "Monochrome",
        }
    }

    /// What the renderer draws.
    pub fn style(self) -> ViewStyle {
        let s = ViewStyle::default();
        match self {
            VisualStyle::Shaded => ViewStyle { edges: false, ..s },
            VisualStyle::ShadedWithEdges => s,
            VisualStyle::ShadedWithHiddenEdges => ViewStyle {
                hidden_edges: true,
                ..s
            },
            VisualStyle::Wireframe => ViewStyle { faces: false, ..s },
            VisualStyle::WireframeWithHiddenEdges => ViewStyle {
                faces: false,
                depth_only: true,
                hidden_edges: true,
                ..s
            },
            VisualStyle::WireframeWithVisibleEdgesOnly => ViewStyle {
                faces: false,
                depth_only: true,
                ..s
            },
            VisualStyle::Monochrome => ViewStyle {
                monochrome: true,
                ..s
            },
        }
    }
}

/// The Section View commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SectionKind {
    Quarter,
    Half,
    ThreeQuarter,
    End,
}

impl SectionKind {
    pub fn label(self) -> &'static str {
        self.label_const()
    }

    /// [`Self::label`] for constants (the command registry).
    pub const fn label_const(self) -> &'static str {
        match self {
            SectionKind::Quarter => "Quarter Section View",
            SectionKind::Half => "Half Section View",
            SectionKind::ThreeQuarter => "Three Quarter Section View",
            SectionKind::End => "End Section View",
        }
    }

    /// How many planes it cuts with.
    pub fn planes(self) -> usize {
        match self {
            SectionKind::Half => 1,
            SectionKind::Quarter | SectionKind::ThreeQuarter => 2,
            SectionKind::End => 0,
        }
    }
}

/// One cutting plane of a section: the plane, an offset along its normal
/// and which side is cut away.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SectionPlane {
    pub plane: PlaneRef,
    pub offset: Real,
    pub flip: bool,
}

/// The view's section (it stays after its panel is closed).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Section {
    pub kind: SectionKind,
    pub planes: Vec<SectionPlane>,
    /// The panel is open (picks go to the active plane).
    pub editing: bool,
    /// The plane the next pick sets.
    pub active: usize,
}

/// A plane's display name.
fn plane_label(p: &PlaneRef) -> String {
    match p {
        PlaneRef::Origin(o) => format!("{o:?} Plane"),
        PlaneRef::Face(n) => format!("Face {n}"),
        PlaneRef::Work(id) => id.clone(),
        other => format!("{other:?}"),
    }
}

impl CadCraftApp {
    /// View › Section View › `kind`.
    pub(super) fn section_command(&mut self, kind: SectionKind) {
        if kind == SectionKind::End {
            self.section = None;
            self.file_status = Some(Ok("Section View ended".into()));
            return;
        }
        // A planar face or plane selected before is the first plane.
        let pre = self.sel.iter().find_map(|x| match x {
            super::select::SelItem::Face(n) => Some(PlaneRef::Face(n.clone())),
            super::select::SelItem::Feature(id) if self.part().work.planes.contains_key(id) => {
                Some(PlaneRef::Work(id.clone()))
            }
            _ => None,
        });
        let mut planes: Vec<SectionPlane> = match &self.section {
            Some(s) => s.planes.clone(),
            None => Vec::new(),
        };
        if let Some(p) = pre {
            planes.insert(
                0,
                SectionPlane {
                    plane: p,
                    offset: 0.0,
                    flip: false,
                },
            );
        }
        planes.truncate(kind.planes());
        let active = planes.len().min(kind.planes() - 1);
        self.sel.clear();
        self.section = Some(Section {
            kind,
            planes,
            editing: true,
            active,
        });
    }

    /// A click in the view while the section's panel is open: the plane
    /// under the cursor (planar face, work plane, origin plane).
    pub(super) fn section_click(&mut self, p: egui::Pos2, rect: egui::Rect) {
        let Some(plane) = self.plane_under(p, rect, true) else {
            return;
        };
        let Some(s) = self.section.as_mut().filter(|s| s.editing) else {
            return;
        };
        let k = s.active.min(s.kind.planes().saturating_sub(1));
        let new = SectionPlane {
            plane,
            offset: 0.0,
            flip: false,
        };
        if k < s.planes.len() {
            s.planes[k] = new;
        } else {
            s.planes.push(new);
        }
        if s.planes.len() < s.kind.planes() {
            s.active = s.planes.len();
        }
    }

    /// The cut the view shows: Slice Graphics in a sketch first, else the
    /// section.
    pub(super) fn view_cut(&self) -> Cut {
        if let Some(a) = self.slice_plane() {
            return Cut {
                a: Some(a),
                ..Default::default()
            };
        }
        let Some(s) = &self.section else {
            return Cut::default();
        };
        let part = self.part();
        let planes: Vec<(Vector3, Real)> = s
            .planes
            .iter()
            .filter_map(|sp| {
                let pl =
                    crate::part::placement::plane_of(&sp.plane, &part.body, &part.work).ok()?;
                let mut n = pl.normal.into_inner();
                let mut w = n.dot(&pl.origin.coords) + sp.offset;
                if sp.flip {
                    n = -n;
                    w = -w;
                }
                Some((n, w))
            })
            .collect();
        Cut {
            a: planes.first().copied(),
            b: planes.get(1).copied(),
            union: s.kind == SectionKind::ThreeQuarter,
        }
    }

    /// The Section View panel (Properties).
    pub(super) fn section_panel(&mut self, ui: &mut egui::Ui) {
        let Some(mut s) = self.section.clone() else {
            return;
        };
        self.panel_header(ui, super::icons::Icon::Section, s.kind.label());
        let mut close = false;
        let mut end = false;
        egui::Grid::new("section_panel")
            .num_columns(2)
            .min_col_width(super::panels::label_width(ui.ctx()))
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                for k in 0..s.kind.planes() {
                    super::panels::plabel(ui, if k == 0 { "Plane" } else { "Second Plane" });
                    let text = s
                        .planes
                        .get(k)
                        .map_or_else(|| "Select".to_owned(), |p| plane_label(&p.plane));
                    let t = if s.active == k {
                        RichText::new(text).strong().underline()
                    } else {
                        RichText::new(text)
                    };
                    if ui
                        .selectable_label(s.active == k, t)
                        .on_hover_text("Click a planar face, a work plane or an origin plane")
                        .clicked()
                    {
                        s.active = k;
                    }
                    ui.end_row();
                    if let Some(p) = s.planes.get_mut(k) {
                        super::panels::plabel(ui, "Offset");
                        ui.add(ExprValue::new(&mut p.offset).suffix(" mm"));
                        ui.end_row();
                        super::panels::plabel(ui, "");
                        ui.checkbox(&mut p.flip, "Flip Section")
                            .on_hover_text("Cut away the other side of this plane");
                        ui.end_row();
                    }
                }
            });
        ui.separator();
        ui.horizontal(|ui| {
            let ready = s.planes.len() == s.kind.planes();
            if ui
                .add_enabled(ready, egui::Button::new("OK"))
                .on_disabled_hover_text("Pick the plane(s) first")
                .clicked()
                || (ready && super::keys::panel_ok_key(ui, true))
            {
                close = true;
            }
            if ui.button("End Section View").clicked() {
                end = true;
            }
        });
        ui.label(
            RichText::new(
                "The section stays after OK; View › Section View › End Section View removes it.",
            )
            .weak()
            .small(),
        );
        if end {
            self.section = None;
            return;
        }
        if close {
            s.editing = false;
        }
        self.section = Some(s);
    }

    /// The status-bar prompt while the section's panel is open.
    pub(super) fn section_prompt(&self) -> Option<String> {
        let s = self.section.as_ref().filter(|s| s.editing)?;
        Some(if s.planes.len() < s.kind.planes() {
            format!(
                "{}: click a planar face, a work plane or an origin plane",
                s.kind.label()
            )
        } else {
            format!("{}: set the offset, Flip, then OK (Enter)", s.kind.label())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each style draws what Inventor's does.
    #[test]
    fn styles_draw_their_parts() {
        let s = |v: VisualStyle| v.style();
        assert!(s(VisualStyle::ShadedWithEdges).faces && s(VisualStyle::ShadedWithEdges).edges);
        assert!(!s(VisualStyle::Shaded).edges);
        assert!(s(VisualStyle::ShadedWithHiddenEdges).hidden_edges);
        assert!(!s(VisualStyle::Wireframe).faces && !s(VisualStyle::Wireframe).depth_only);
        let h = s(VisualStyle::WireframeWithHiddenEdges);
        assert!(!h.faces && h.depth_only && h.hidden_edges && h.edges);
        let v = s(VisualStyle::WireframeWithVisibleEdgesOnly);
        assert!(v.depth_only && !v.hidden_edges);
        assert!(s(VisualStyle::Monochrome).monochrome);
        assert_eq!(VisualStyle::default(), VisualStyle::ShadedWithEdges);
    }

    /// Plane counts of the section commands.
    #[test]
    fn section_planes() {
        assert_eq!(SectionKind::Half.planes(), 1);
        assert_eq!(SectionKind::Quarter.planes(), 2);
        assert_eq!(SectionKind::ThreeQuarter.planes(), 2);
        assert_eq!(SectionKind::End.planes(), 0);
    }
}
