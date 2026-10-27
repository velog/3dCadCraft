//! The right-click menu of what is selected in the part (rule 17,
//! `docs/spec/2.0-part-commands.md` §3.9): every command that can take the
//! selection, grouped as Inventor groups them; starting one fills its first
//! input with the selection. A right click on something not selected
//! selects it first (as Inventor does).

use eframe::egui;

use super::CadCraftApp;
use super::part_planned::PartPlanned as Pp;
use super::ribbon::Action;
use super::select::SelItem;
use crate::kernel::{FaceId, Point3, Surface};
use crate::part::{LineRef, PlaneRef, PointRef};

/// What the selection is, for its menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SelKind {
    PlanarFace,
    CurvedFace,
    Edge,
    StraightEdge,
    Vertex,
    Solid,
    SurfaceBody,
    Sketch,
    WorkPlane,
    WorkAxis,
    WorkPoint,
    /// Items of different kinds: only what takes any of them.
    Mixed,
}

/// One entry of the menu.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Entry {
    NewSketch,
    PressPull,
    Hole,
    Shell,
    FaceDraft,
    Thread,
    Thicken,
    Fillet,
    Chamfer,
    WorkPlane,
    WorkAxis,
    WorkPoint,
    Combine,
    Split,
    Extrude,
    Revolve,
    EditSketch,
    EditFeature,
    LookAt,
    Planned(Pp),
    Separator,
}

impl Entry {
    fn label(self) -> &'static str {
        match self {
            Entry::NewSketch => "New Sketch",
            Entry::PressPull => "Extrude (Press/Pull)",
            Entry::Hole => "Hole",
            Entry::Shell => "Shell (remove these faces)",
            Entry::FaceDraft => "Face Draft",
            Entry::Thread => "Thread",
            Entry::Thicken => "Thicken/Offset",
            Entry::Fillet => "Fillet",
            Entry::Chamfer => "Chamfer",
            Entry::WorkPlane => "Work Plane",
            Entry::WorkAxis => "Work Axis",
            Entry::WorkPoint => "Work Point",
            Entry::Combine => "Combine",
            Entry::Split => "Split",
            Entry::Extrude => "Extrude",
            Entry::Revolve => "Revolve",
            Entry::EditSketch => "Edit Sketch",
            Entry::EditFeature => "Edit Feature",
            Entry::LookAt => "Look At",
            Entry::Planned(p) => p.title(),
            Entry::Separator => "",
        }
    }
}

use Entry as E;

/// The menu of a selection kind, in Inventor's order (create, modify,
/// work features, edit, view); `one` when a single item is selected.
pub(super) fn entries(kind: SelKind, one: bool) -> Vec<Entry> {
    let mut v = match kind {
        SelKind::PlanarFace => vec![
            E::NewSketch,
            E::PressPull,
            E::Hole,
            E::Planned(Pp::Emboss),
            E::Separator,
            E::Shell,
            E::FaceDraft,
            E::Thicken,
            E::Planned(Pp::DeleteFace),
            E::Planned(Pp::Direct),
            E::Planned(Pp::CopyObject),
            E::Separator,
            E::WorkPlane,
        ],
        SelKind::CurvedFace => vec![
            E::Thread,
            E::Separator,
            E::Shell,
            E::FaceDraft,
            E::Thicken,
            E::Planned(Pp::DeleteFace),
            E::Planned(Pp::Direct),
            E::Planned(Pp::CopyObject),
            E::Separator,
            E::WorkAxis,
        ],
        SelKind::StraightEdge => vec![
            E::Fillet,
            E::Chamfer,
            E::Separator,
            E::Planned(Pp::RuledSurface),
            E::Separator,
            E::WorkAxis,
            E::WorkPoint,
        ],
        SelKind::Edge => vec![
            E::Fillet,
            E::Chamfer,
            E::Separator,
            E::Planned(Pp::RuledSurface),
            E::Separator,
            E::WorkPoint,
        ],
        SelKind::Vertex => vec![E::WorkPoint, E::WorkPlane],
        SelKind::Solid => vec![
            E::Combine,
            E::Split,
            E::Planned(Pp::MoveBodies),
            E::Planned(Pp::CopyObject),
            E::Planned(Pp::MakeComponents),
            E::Planned(Pp::MakePart),
        ],
        SelKind::SurfaceBody => vec![
            E::Planned(Pp::Stitch),
            E::Planned(Pp::Sculpt),
            E::Thicken,
            E::Planned(Pp::TrimSurface),
            E::Split,
        ],
        SelKind::Sketch => vec![
            E::EditSketch,
            E::Separator,
            E::Extrude,
            E::Revolve,
            E::Hole,
            E::Planned(Pp::Emboss),
            E::Planned(Pp::SketchDrivenPattern),
        ],
        SelKind::WorkPlane => vec![E::NewSketch, E::Split, E::WorkPlane],
        SelKind::WorkAxis => vec![E::WorkPlane, E::WorkPoint],
        SelKind::WorkPoint => vec![E::WorkPlane, E::WorkAxis],
        SelKind::Mixed => vec![E::Planned(Pp::CopyObject)],
    };
    // A face, an edge or a sketch also edits what made it.
    let edits: &[Entry] = match kind {
        SelKind::PlanarFace => &[E::EditFeature, E::EditSketch, E::LookAt],
        SelKind::CurvedFace | SelKind::Edge | SelKind::StraightEdge => &[E::EditFeature],
        _ => &[],
    };
    if one && !edits.is_empty() {
        v.push(E::Separator);
        v.extend_from_slice(edits);
    }
    // Commands that take one item only.
    if !one {
        v.retain(|e| !matches!(e, E::NewSketch | E::PressPull | E::LookAt | E::Thread));
    }
    v
}

impl CadCraftApp {
    /// Whether model face `name` is planar.
    fn face_is_planar(&self, name: &str) -> bool {
        let body = &self.part().body;
        body.find_face(name)
            .and_then(|f| body.face(f))
            .and_then(|x| body.surface(x.facet_of.unwrap_or(x.surface)))
            .is_some_and(|s| matches!(s, Surface::Plane(_)))
    }

    /// A point on face `name` (for commands that want the click point).
    fn face_point(&self, name: &str) -> Option<(FaceId, Point3)> {
        let body = &self.part().body;
        let f = body.find_face(name)?;
        let r = crate::part::shell::face_ref(body, f, None)?;
        Some((f, Point3::from(r.hint)))
    }

    /// The kind of one item.
    fn item_kind(&self, item: &SelItem) -> SelKind {
        let part = self.part();
        match item {
            SelItem::Face(n) if self.face_is_planar(n) => SelKind::PlanarFace,
            SelItem::Face(_) => SelKind::CurvedFace,
            SelItem::Edge(e) if crate::part::select::edge_is_straight(&part.body, *e) => {
                SelKind::StraightEdge
            }
            SelItem::Edge(_) => SelKind::Edge,
            SelItem::Vertex(_) => SelKind::Vertex,
            SelItem::Body(n) if crate::part::select::body_is_surface(&part.body, n) => {
                SelKind::SurfaceBody
            }
            SelItem::Body(_) => SelKind::Solid,
            SelItem::Sketch(_) => SelKind::Sketch,
            SelItem::Feature(id) if part.work.axes.contains_key(id) => SelKind::WorkAxis,
            SelItem::Feature(id) if part.work.points.contains_key(id) => SelKind::WorkPoint,
            SelItem::Feature(_) => SelKind::WorkPlane,
        }
    }

    /// The kind of the whole selection (`None`: nothing selected).
    pub(super) fn selection_kind(&self) -> Option<SelKind> {
        let mut kinds = self.sel.iter().map(|x| self.item_kind(x));
        let first = kinds.next()?;
        // Straight and curved edges together are edges; planar and curved
        // faces together are curved faces (the commands both take).
        Some(kinds.fold(first, |a, b| match (a, b) {
            _ if a == b => a,
            (SelKind::Edge | SelKind::StraightEdge, SelKind::Edge | SelKind::StraightEdge) => {
                SelKind::Edge
            }
            (
                SelKind::PlanarFace | SelKind::CurvedFace,
                SelKind::PlanarFace | SelKind::CurvedFace,
            ) => SelKind::CurvedFace,
            _ => SelKind::Mixed,
        }))
    }

    /// A right click on an item that is not selected selects it (alone).
    pub(super) fn select_for_menu(&mut self, item: Option<SelItem>) {
        if let Some(item) = item
            && !self.sel.contains(&item)
        {
            self.sel = vec![item];
            self.face_sel = None;
            self.edge_sel = None;
        }
    }

    /// The selection's commands (top of the right-click menu).
    pub(super) fn selection_menu(&mut self, ui: &mut egui::Ui) {
        let Some(kind) = self.selection_kind() else {
            return;
        };
        let list = entries(kind, self.sel.len() == 1);
        let mut last_sep = true;
        for e in list {
            if e == E::Separator {
                if !last_sep {
                    ui.separator();
                }
                last_sep = true;
                continue;
            }
            let ok = self.entry_available(e);
            let mut b = ui.add_enabled(ok, egui::Button::new(e.label()));
            if let E::Planned(p) = e {
                b = b.on_hover_text(format!(
                    "Opens its panel; the logic comes next (FEATURES {})",
                    p.section()
                ));
            }
            if b.clicked() {
                self.run_entry(e);
                ui.close();
            }
            last_sep = false;
        }
        if !last_sep {
            ui.separator();
        }
    }

    /// Whether entry `e` can run on the selection now.
    fn entry_available(&self, e: Entry) -> bool {
        match (e, self.sel.first()) {
            (E::EditFeature, Some(SelItem::Face(n))) => self.face_feature(n).is_some(),
            (E::EditFeature, Some(SelItem::Edge(x))) => self.edge_feature(*x).is_some(),
            (E::EditSketch, Some(SelItem::Face(n))) => self.face_sketch(n).is_some(),
            (E::Combine, _) => self.part().body.solids().filter(|(_, s)| !s.open).count() > 1,
            _ => true,
        }
    }

    /// The sketch of the feature that made face `name`.
    fn face_sketch(&self, name: &str) -> Option<usize> {
        let k = self.face_feature(name)?;
        let id = self.part().features[k].sketch()?.to_owned();
        self.part().sketches.iter().position(|s| s.id == id)
    }

    /// Runs entry `e` with the selection as its input.
    pub(super) fn run_entry(&mut self, e: Entry) {
        let sel = self.sel.clone();
        let first = sel.first().cloned();
        let faces: Vec<String> = sel
            .iter()
            .filter_map(|x| match x {
                SelItem::Face(n) => Some(n.clone()),
                _ => None,
            })
            .collect();
        match e {
            E::NewSketch => {
                let plane = match &first {
                    Some(SelItem::Face(n)) => Some(PlaneRef::Face(n.clone())),
                    Some(SelItem::Feature(id)) => Some(PlaneRef::Work(id.clone())),
                    _ => None,
                };
                if let Some(p) = plane {
                    self.sel.clear();
                    self.mode = super::Mode::PickPlane(super::PickPurpose::NewSketch);
                    self.plane_picked(p);
                }
            }
            E::PressPull => {
                if let Some(SelItem::Face(n)) = first {
                    let at = self.face_sel.as_ref().map_or(egui::Pos2::ZERO, |s| s.1);
                    self.sel.clear();
                    self.press_pull(n, at);
                }
            }
            E::Hole => self.apply(Action::Hole),
            E::Shell => self.apply(Action::Shell),
            E::FaceDraft => {
                self.apply(Action::Draft);
                for n in &faces {
                    if let Some((f, at)) = self.face_point(n)
                        && let Some(d) = self.draft_draft.as_mut()
                    {
                        let keep = d.input;
                        d.input = super::draft::DraftInput::Faces;
                        self.draft_pick(super::draft::DraftPick::Face(f, at));
                        if let Some(d) = self.draft_draft.as_mut() {
                            d.input = keep;
                        }
                    }
                }
            }
            E::Thread => {
                self.apply(Action::Thread);
                if let Some(n) = faces.first()
                    && let Some((f, at)) = self.face_point(n)
                {
                    self.thread_pick(f, at);
                }
            }
            E::Thicken => self.apply(Action::Thicken),
            E::Fillet => self.apply(Action::Fillet),
            E::Chamfer => self.apply(Action::Chamfer),
            E::WorkPlane | E::WorkAxis | E::WorkPoint => {
                let action = match e {
                    E::WorkAxis => Action::WorkAxis,
                    E::WorkPoint => Action::WorkPoint,
                    _ => Action::WorkPlane,
                };
                self.apply(action);
                for x in &sel {
                    if let Some(input) = self.work_input(x)
                        && let Some(d) = self.work_draft.as_mut()
                    {
                        d.pick(input);
                    }
                }
                self.sel.clear();
            }
            E::Combine => self.apply(Action::Combine),
            E::Split => {
                self.apply(Action::Split);
                let pick = match &first {
                    Some(SelItem::Feature(id)) => Some(super::combine_split::BodyPick::Plane(
                        PlaneRef::Work(id.clone()),
                    )),
                    Some(SelItem::Body(n))
                        if crate::part::select::body_is_surface(&self.part().body, n) =>
                    {
                        Some(super::combine_split::BodyPick::Surface(n.clone()))
                    }
                    _ => None,
                };
                if let Some(p) = pick {
                    self.body_cmd_pick(p);
                }
            }
            E::Extrude | E::Revolve => {
                if let Some(SelItem::Sketch(i)) = first {
                    self.selection = super::Selection::Sketch(i);
                }
                self.apply(if e == E::Extrude {
                    Action::Extrude
                } else {
                    Action::Revolve
                });
            }
            E::EditSketch => {
                let i = match &first {
                    Some(SelItem::Sketch(i)) => Some(*i),
                    Some(SelItem::Face(n)) => self.face_sketch(n),
                    _ => None,
                };
                if let Some(i) = i {
                    self.sel.clear();
                    self.edit_sketch(i);
                }
            }
            E::EditFeature => {
                let k = match &first {
                    Some(SelItem::Face(n)) => self.face_feature(n),
                    Some(SelItem::Edge(x)) => self.edge_feature(*x),
                    _ => None,
                };
                if let Some(k) = k {
                    self.sel.clear();
                    self.edit_feature_dialog(k);
                }
            }
            E::LookAt => {
                if let Some(SelItem::Face(n)) = first {
                    self.look_at_face(n);
                }
            }
            E::Planned(p) => self.start_part_planned(p),
            E::Separator => {}
        }
    }

    /// A selected item as a work feature input.
    fn work_input(&self, x: &SelItem) -> Option<super::work::WpInput> {
        use super::work::WpInput;
        let body = &self.part().body;
        match x {
            SelItem::Face(n) if self.face_is_planar(n) => {
                Some(WpInput::Plane(PlaneRef::Face(n.clone())))
            }
            SelItem::Edge(e) if crate::part::select::edge_is_straight(body, *e) => body
                .edge(*e)
                .and_then(|x| x.name.clone())
                .map(|n| WpInput::Line(LineRef::Edge(n))),
            SelItem::Vertex(n) => Some(WpInput::Point(PointRef::Vertex(n.clone()))),
            SelItem::Feature(id) if self.part().work.axes.contains_key(id) => {
                Some(WpInput::Line(LineRef::Work(id.clone())))
            }
            SelItem::Feature(id) if self.part().work.points.contains_key(id) => {
                Some(WpInput::Point(PointRef::Work(id.clone())))
            }
            SelItem::Feature(id) => Some(WpInput::Plane(PlaneRef::Work(id.clone()))),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind has commands; one-item commands leave a multi-selection;
    /// a face and an edge also edit their feature.
    #[test]
    fn menus_by_kind() {
        for k in [
            SelKind::PlanarFace,
            SelKind::CurvedFace,
            SelKind::Edge,
            SelKind::StraightEdge,
            SelKind::Vertex,
            SelKind::Solid,
            SelKind::SurfaceBody,
            SelKind::Sketch,
            SelKind::WorkPlane,
            SelKind::WorkAxis,
            SelKind::WorkPoint,
            SelKind::Mixed,
        ] {
            assert!(entries(k, true).iter().any(|e| *e != E::Separator), "{k:?}");
        }
        let one = entries(SelKind::PlanarFace, true);
        assert!(one.contains(&E::NewSketch) && one.contains(&E::EditFeature));
        let many = entries(SelKind::PlanarFace, false);
        assert!(!many.contains(&E::NewSketch) && many.contains(&E::Shell));
        assert!(entries(SelKind::StraightEdge, true).contains(&E::Fillet));
        assert!(entries(SelKind::CurvedFace, true).contains(&E::Thread));
        assert!(entries(SelKind::Solid, false).contains(&E::Combine));
    }
}
