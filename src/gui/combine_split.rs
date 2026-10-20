//! The Combine and Split commands (`docs/spec/2.13-combine-split.md`).
//!
//! Combine: Base (active first), then Toolbodies (auto-advance), Join / Cut
//! / Intersect, Keep Toolbody, a toggle that keeps the tools visible.
//! Split: Split Face / Trim Solid / Split Solid, the tool (a work or origin
//! plane, a sketch or a surface body — in the view or the browser; the
//! origin planes are shown while the tool selector is active), the faces (or
//! All Faces + solid) or the solid, Keep
//! Default / Keep Flip with an arrow on the side that goes. Both follow the
//! shared selector flow (`command::flow`, `gui/command_ui.rs`): Continue /
//! Back / Enter, the right-click menu, the cursor hint. Preview on a worker
//! thread, OK / Apply / Cancel, Edit Feature.

use eframe::egui::{self, Color32, Rect, RichText, Stroke};

use super::CadCraftApp;
use super::breadcrumb::{self, BreadcrumbState, Crumb, CrumbEvent};
use super::command_ui::{MenuOption, RowEvents, selector_rows};
use super::icons::Icon;
use super::panels::{plabel, psection, warning_box};
use crate::command::flow::{Flow, PickKind, combine_selectors, split_selectors};
use crate::kernel::{FaceId, Point3, TessellationOptions};
use crate::part::select as psel;
use crate::part::{
    CombineFeature, Feature, Operation, OriginPlane, PartDocument, PlaneRef, SplitFeature,
    SplitMethod, SplitToolRef,
};

/// What the cursor is over, as the active selector takes it.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum BodyPick {
    /// A solid body (persistent name).
    Body(String),
    /// A model face and the model point.
    Face(FaceId, Point3),
    /// A work or origin plane.
    Plane(PlaneRef),
    /// A sketch (by id).
    Sketch(String),
    /// A surface body (persistent name).
    Surface(String),
}

/// The open Combine command.
#[derive(Clone, Debug)]
pub(super) struct CombineDraft {
    /// The feature being defined.
    pub def: CombineFeature,
    /// Edit Feature of this feature index.
    pub edit: Option<usize>,
    /// The active selector (`base` / `tools`).
    pub input: &'static str,
    /// Keep the toolbodies visible after OK (Inventor's visibility toggle).
    pub tools_visible: bool,
    /// The breadcrumb (the feature's name).
    pub crumbs: BreadcrumbState,
    /// A new name typed in the breadcrumb.
    pub name: Option<String>,
}

/// The open Split command.
#[derive(Clone, Debug)]
pub(super) struct SplitDraft {
    /// The feature being defined.
    pub def: SplitFeature,
    /// Edit Feature of this feature index.
    pub edit: Option<usize>,
    /// The active selector (`tool` / `faces` / `solid`).
    pub input: &'static str,
    /// The breadcrumb (the feature's name).
    pub crumbs: BreadcrumbState,
    /// A new name typed in the breadcrumb.
    pub name: Option<String>,
}

fn key_of(flow: &Flow, k: &str) -> &'static str {
    flow.selectors
        .iter()
        .find(|s| s.key == k)
        .map_or("", |s| s.key)
}

impl CombineDraft {
    pub(super) fn new() -> Self {
        Self {
            def: CombineFeature::new(),
            edit: None,
            input: "base",
            tools_visible: false,
            crumbs: BreadcrumbState::at(0),
            name: None,
        }
    }

    pub(super) fn editing(def: CombineFeature, k: usize) -> Self {
        Self {
            def,
            edit: Some(k),
            input: "tools",
            tools_visible: false,
            crumbs: BreadcrumbState::at(0),
            name: None,
        }
    }

    pub(super) fn flow(&self) -> Flow {
        Flow::new(combine_selectors(&self.def), Some(self.input))
    }

    pub(super) fn set_active(&mut self, key: &str) {
        let f = self.flow();
        let k = key_of(&f, key);
        if !k.is_empty() {
            self.input = k;
        }
    }

    fn picked(&mut self) {
        let mut f = self.flow();
        if f.picked()
            && let Some(k) = f.active_key()
        {
            self.input = k;
        }
    }

    /// The right-click menu's options: the operations.
    pub(super) fn menu_options(&self) -> Vec<MenuOption> {
        OPS.iter()
            .map(|o| MenuOption {
                label: o.label().to_owned(),
                checked: self.def.operation == *o,
            })
            .collect()
    }

    pub(super) fn menu_option(&mut self, k: usize) {
        if let Some(o) = OPS.get(k) {
            self.def.operation = *o;
        }
    }
}

const OPS: [Operation; 3] = [Operation::Join, Operation::Cut, Operation::Intersect];
const METHODS: [SplitMethod; 3] = [
    SplitMethod::SplitFace,
    SplitMethod::TrimSolid,
    SplitMethod::SplitSolid,
];

impl SplitDraft {
    pub(super) fn new() -> Self {
        Self {
            def: SplitFeature::new(SplitMethod::SplitFace),
            edit: None,
            input: "tool",
            crumbs: BreadcrumbState::at(0),
            name: None,
        }
    }

    pub(super) fn editing(def: SplitFeature, k: usize) -> Self {
        Self {
            def,
            edit: Some(k),
            input: "tool",
            crumbs: BreadcrumbState::at(0),
            name: None,
        }
    }

    pub(super) fn flow(&self, many: bool) -> Flow {
        Flow::new(split_selectors(&self.def, many), Some(self.input))
    }

    pub(super) fn set_active(&mut self, key: &str, many: bool) {
        let f = self.flow(many);
        let k = key_of(&f, key);
        if !k.is_empty() {
            self.input = k;
        }
    }

    fn picked(&mut self, many: bool) {
        let mut f = self.flow(many);
        if f.picked()
            && let Some(k) = f.active_key()
        {
            self.input = k;
        }
    }

    /// Changes the method (the active selector follows).
    pub(super) fn set_method(&mut self, m: SplitMethod, many: bool) {
        if self.def.method != m {
            self.def.method = m;
            let f = Flow::new(split_selectors(&self.def, many), None);
            self.input = f.active_key().unwrap_or("tool");
        }
    }

    /// The right-click menu's options: the methods, then Flip (Trim).
    pub(super) fn menu_options(&self) -> Vec<MenuOption> {
        let mut v: Vec<MenuOption> = METHODS
            .iter()
            .map(|m| MenuOption {
                label: m.label().to_owned(),
                checked: self.def.method == *m,
            })
            .collect();
        if self.def.method == SplitMethod::TrimSolid {
            v.push(MenuOption {
                label: "Keep Flip side".into(),
                checked: self.def.flip,
            });
        }
        v
    }

    pub(super) fn menu_option(&mut self, k: usize, many: bool) {
        match METHODS.get(k) {
            Some(m) => self.set_method(*m, many),
            None => self.def.flip = !self.def.flip,
        }
    }
}

/// Edge polylines of the faces `keep` chooses (the preview).
fn face_edges(part: &PartDocument, keep: impl Fn(&str) -> bool) -> Vec<Vec<Point3>> {
    let body = &part.body;
    let opts = TessellationOptions::default();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (f, face) in body.faces() {
        if !face.name.as_deref().is_some_and(&keep) {
            continue;
        }
        for e in psel::edges_of_face(body, f) {
            if seen.insert(e)
                && let Ok(p) = body.sample_edge(e, &opts)
            {
                out.push(p);
            }
        }
    }
    out
}

/// The faces of body `name` (preview of Combine, highlight).
fn body_faces(part: &PartDocument, name: &str) -> Vec<FaceId> {
    psel::faces_of_body(&part.body, name)
}

/// A short description of a split tool.
pub(super) fn describe_tool(part: &PartDocument, t: &SplitToolRef) -> String {
    match t {
        SplitToolRef::Plane(PlaneRef::Origin(o)) => format!("{o:?} Plane"),
        SplitToolRef::Plane(PlaneRef::Work(id)) => part
            .features
            .iter()
            .find(|f| f.id() == id)
            .map_or_else(|| id.clone(), |f| f.name().to_owned()),
        SplitToolRef::Plane(PlaneRef::Face(n)) => n.clone(),
        SplitToolRef::Plane(PlaneRef::Fixed) => "fixed".into(),
        SplitToolRef::Sketch(id) => part
            .sketches
            .iter()
            .find(|s| s.id == *id)
            .map_or_else(|| id.clone(), |s| s.name.clone()),
        SplitToolRef::Surface(n) => format!("{} (surface)", body_label(part, n)),
    }
}

/// Display name of body `name` (`SolidN`).
pub(super) fn body_label(part: &PartDocument, name: &str) -> String {
    part.bodies_before(usize::MAX)
        .into_iter()
        .find(|(n, _)| n == name)
        .map_or_else(|| name.to_owned(), |(_, l)| l)
}

impl CadCraftApp {
    fn solid_count(&self) -> usize {
        self.part().body.solids().filter(|(_, s)| !s.open).count()
    }

    /// Closes both commands.
    pub(super) fn close_body_cmds(&mut self) {
        self.combine_draft = None;
        self.split_draft = None;
    }

    fn body_cmd_reset(&mut self) {
        self.finish_sketch();
        self.mode = super::Mode::Normal;
        self.cancel_extrude();
        self.work_draft = None;
        self.blend_draft = None;
        self.face_sel = None;
        self.edge_sel = None;
    }

    /// **Combine** (3D Model → Modify): multi-body parts only. A body
    /// selected before becomes the base.
    pub(super) fn start_combine(&mut self) {
        if self.solid_count() < 2 {
            self.file_status = Some(Err(
                "Combine: the part has one solid body (Combine works in multi-body parts)".into(),
            ));
            return;
        }
        let pre = self.sel.iter().find_map(|s| match s {
            super::select::SelItem::Body(n) => Some(n.clone()),
            _ => None,
        });
        self.body_cmd_reset();
        self.sel.clear();
        let mut d = CombineDraft::new();
        if let Some(b) = pre {
            d.def.base = b;
            d.input = "tools";
        }
        self.combine_draft = Some(d);
    }

    /// **Split** (3D Model → Modify).
    pub(super) fn start_split(&mut self) {
        self.body_cmd_reset();
        self.sel.clear();
        self.split_draft = Some(SplitDraft::new());
    }

    /// The open Combine's / Split's selector flow.
    pub(super) fn body_cmd_flow(&self) -> Option<Flow> {
        if let Some(d) = &self.combine_draft {
            return Some(d.flow());
        }
        let many = self.solid_count() > 1;
        self.split_draft.as_ref().map(|d| d.flow(many))
    }

    /// Makes selector `key` active.
    pub(super) fn body_cmd_activate(&mut self, key: &str) {
        let many = self.solid_count() > 1;
        if let Some(d) = self.combine_draft.as_mut() {
            d.set_active(key);
        }
        if let Some(d) = self.split_draft.as_mut() {
            d.set_active(key, many);
        }
    }

    /// What the active selector takes under `p`.
    pub(super) fn body_cmd_hover(&self, p: egui::Pos2, rect: Rect) -> Option<BodyPick> {
        let flow = self.body_cmd_flow()?;
        let face = self.camera.ray(p, rect).and_then(|(o, dir)| {
            let dir = dir.try_normalize(1e-12)?;
            let (f, t) = self.picker().pick(&o, &dir)?;
            Some((f, o + dir * t))
        });
        let part = self.part();
        let open = |f: FaceId| {
            psel::body_of_face(&part.body, f).filter(|n| {
                part.body
                    .solids()
                    .any(|(_, s)| s.open && s.name.as_deref() == Some(n.as_str()))
            })
        };
        if flow.accepts(PickKind::Body) {
            // Solids only (a surface is not a body to split).
            return face
                .and_then(|(f, _)| psel::body_of_face(&part.body, f).filter(|_| open(f).is_none()))
                .map(BodyPick::Body);
        }
        if flow.accepts(PickKind::Face) {
            return face.map(|(f, at)| BodyPick::Face(f, at));
        }
        if flow.accepts(PickKind::Sketch)
            && let Some(i) = self.sketch_under(p, rect)
        {
            return part.sketches.get(i).map(|s| BodyPick::Sketch(s.id.clone()));
        }
        // A surface body's face under the cursor (nearest along the ray).
        let surface = face
            .filter(|_| flow.accepts(PickKind::Surface))
            .and_then(|(f, at)| open(f).map(|n| (at, n)));
        if flow.accepts(PickKind::Plane) {
            // Work and origin planes (Inventor's split tools; model faces
            // are not), even through a solid face.
            let (o, dir) = self.camera.ray(p, rect)?;
            let work = self
                .work_plane_under(p, rect)
                .map(|(id, t)| (t, BodyPick::Plane(PlaneRef::Work(id))));
            let half = self.origin_plane_half();
            let unit = dir.norm().max(1e-12);
            let surf = surface.map(|(at, n)| ((at - o).norm() / unit, BodyPick::Surface(n)));
            return OriginPlane::ALL
                .into_iter()
                .filter_map(|pl| {
                    Some((
                        super::overlay::hit_origin_plane(pl, half, &o, &dir)?,
                        BodyPick::Plane(PlaneRef::Origin(pl)),
                    ))
                })
                .chain(work)
                .chain(surf)
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, pick)| pick);
        }
        surface.map(|(_, n)| BodyPick::Surface(n))
    }

    /// A browser row clicked while Combine / Split is open: an origin or
    /// work plane, a sketch or a body for the active selector. Returns
    /// whether the command took it.
    pub(super) fn body_cmd_browser_pick(&mut self, pick: BodyPick) -> bool {
        let Some(flow) = self.body_cmd_flow() else {
            return false;
        };
        let part = self.part();
        let takes = match &pick {
            BodyPick::Plane(_) => flow.accepts(PickKind::Plane),
            BodyPick::Sketch(_) => flow.accepts(PickKind::Sketch),
            BodyPick::Surface(_) => flow.accepts(PickKind::Surface),
            BodyPick::Body(n) => {
                flow.accepts(PickKind::Body)
                    && part
                        .body
                        .solids()
                        .any(|(_, s)| !s.open && s.name.as_deref() == Some(n.as_str()))
            }
            BodyPick::Face(..) => false,
        };
        if takes {
            self.body_cmd_pick(pick);
        }
        takes
    }

    /// A click on what [`Self::body_cmd_hover`] found.
    pub(super) fn body_cmd_pick(&mut self, pick: BodyPick) {
        let many = self.solid_count() > 1;
        let body = self.part().body.clone();
        if let Some(d) = self.combine_draft.as_mut() {
            if let BodyPick::Body(n) = pick {
                let base = d.input == "base";
                if d.def.pick(&n, base) && base {
                    d.picked();
                }
            }
            return;
        }
        let Some(d) = self.split_draft.as_mut() else {
            return;
        };
        match (d.input, pick) {
            ("tool", BodyPick::Plane(p)) => {
                d.def.tool = Some(SplitToolRef::Plane(p));
                d.picked(many);
            }
            ("tool", BodyPick::Sketch(id)) => {
                d.def.tool = Some(SplitToolRef::Sketch(id));
                d.picked(many);
            }
            ("tool", BodyPick::Surface(n)) => {
                d.def.tool = Some(SplitToolRef::Surface(n));
                d.picked(many);
            }
            ("faces", BodyPick::Face(f, at)) => {
                if let Some(r) = crate::part::shell::face_ref(&body, f, Some(at)) {
                    match d.def.faces.iter().position(|x| x.face == r.face) {
                        Some(k) => {
                            d.def.faces.remove(k);
                        }
                        None => d.def.faces.push(r),
                    }
                }
            }
            ("solid", BodyPick::Body(n)) => {
                d.def.solid = n;
                d.picked(many);
            }
            _ => {}
        }
    }

    /// The prehighlight of what the cursor is over.
    pub(super) fn body_cmd_hover_item(
        &self,
        h: Option<&BodyPick>,
    ) -> Option<super::select::SelItem> {
        use super::select::SelItem;
        match h? {
            BodyPick::Body(n) | BodyPick::Surface(n) => Some(SelItem::Body(n.clone())),
            BodyPick::Face(f, _) => self
                .part()
                .body
                .face(*f)
                .and_then(|x| x.name.clone())
                .map(SelItem::Face),
            BodyPick::Plane(_) | BodyPick::Sketch(_) => None,
        }
    }

    /// Faces lit as the command's selection.
    pub(super) fn body_cmd_faces(&self) -> Vec<FaceId> {
        let part = self.part();
        if let Some(d) = &self.combine_draft {
            let mut v = body_faces(part, &d.def.base);
            for t in &d.def.tools {
                v.extend(body_faces(part, t));
            }
            return v;
        }
        let Some(d) = &self.split_draft else {
            return Vec::new();
        };
        let mut v: Vec<FaceId> = d
            .def
            .faces
            .iter()
            .filter_map(|r| {
                crate::part::shell::face_near(&part.body, &r.face, Point3::from(r.hint))
            })
            .collect();
        if let Some(SplitToolRef::Surface(n)) = &d.def.tool {
            v.extend(body_faces(part, n));
        }
        if !d.def.solid.is_empty() {
            v.extend(body_faces(part, &d.def.solid));
        }
        v
    }

    /// The result as edge polylines (worker thread), or why it fails.
    pub(super) fn body_cmd_preview(&self) -> Result<Vec<Vec<Point3>>, String> {
        if let Some(d) = &self.combine_draft {
            if !self.body_cmd_flow().is_some_and(|f| f.ready()) {
                return Err(String::new());
            }
            let key = format!("combine|{:?}|{:?}", d.edit, d.def);
            if let Some(r) = self.preview_ready(&key) {
                return r;
            }
            let mut trial = self.part().clone();
            let (def, edit) = (d.def.clone(), d.edit);
            return self.preview_async(key, move || {
                let base = def.base.clone();
                let built = match edit {
                    Some(k) => trial.replace_feature(k, Feature::Combine(def)),
                    None => trial.add_combine(def),
                };
                built
                    .map(|_| {
                        let faces: std::collections::HashSet<String> =
                            psel::faces_of_body(&trial.body, &base)
                                .into_iter()
                                .filter_map(|f| trial.body.face(f).and_then(|x| x.name.clone()))
                                .collect();
                        face_edges(&trial, |n| faces.contains(n))
                    })
                    .map_err(|e| e.to_string())
            });
        }
        let Some(d) = &self.split_draft else {
            return Err(String::new());
        };
        if !self.body_cmd_flow().is_some_and(|f| f.ready()) {
            return Err(String::new());
        }
        let key = format!("split|{:?}|{:?}", d.edit, d.def);
        if let Some(r) = self.preview_ready(&key) {
            return r;
        }
        let mut trial = self.part().clone();
        let (def, edit) = (d.def.clone(), d.edit);
        self.preview_async(key, move || {
            let built = match edit {
                Some(k) => trial.replace_feature(k, Feature::Split(def)),
                None => trial.add_split(def),
            };
            built
                .map(|id| {
                    let piece = format!("~{id}.");
                    let cut = (format!("{id}:"), format!("{id}#"));
                    face_edges(&trial, |n| {
                        n.contains(&piece) || n.starts_with(&cut.0) || n.starts_with(&cut.1)
                    })
                })
                .map_err(|e| e.to_string())
        })
    }

    /// Trim Solid: the arrow on the side that goes (Keep Default: the
    /// tool's front; Keep Flip: its back).
    pub(super) fn draw_split_arrow(&self, painter: &egui::Painter, rect: Rect) {
        let Some(d) = &self.split_draft else { return };
        if d.def.method == SplitMethod::SplitFace {
            return;
        }
        let part = self.part();
        let Ok(tool) = crate::part::split::resolve_tool(
            d.def.tool.as_ref(),
            &part.body,
            &part.work,
            &part.sketches,
        ) else {
            return;
        };
        let pts: Vec<Point3> = part.body.vertices().map(|(_, v)| v.point).collect();
        if pts.is_empty() {
            return;
        }
        let c = pts
            .iter()
            .fold(Point3::origin(), |a, p| a + p.coords / pts.len() as f64);
        let size = pts.iter().map(|p| (p - c).norm()).fold(1.0, f64::max);
        let Some((at, n)) = crate::part::split::arrow(&tool, c) else {
            return;
        };
        let col = Color32::from_rgb(230, 120, 20);
        let draw = |dir: crate::kernel::Vector3| {
            if let (Some(a), Some(b)) = (
                self.camera.project(&at, rect),
                self.camera.project(&(at + dir * size * 0.5), rect),
            ) {
                painter.arrow(a, b - a, Stroke::new(2.5, col));
            }
        };
        match d.def.method {
            SplitMethod::TrimSolid => draw(if d.def.flip { -n } else { n }),
            _ => {
                draw(n);
                draw(-n);
            }
        }
    }

    /// OK (`keep_open` false) or Apply.
    pub(super) fn commit_combine(&mut self, keep_open: bool) {
        let Some(d) = self.combine_draft.clone() else {
            return;
        };
        let cmd = if d.edit.is_some() {
            "Edit Feature"
        } else {
            "Combine"
        };
        self.checkpoint(cmd);
        let built = match d.edit {
            Some(k) => self
                .part_mut()
                .replace_feature(k, Feature::Combine(d.def.clone())),
            None => self.part_mut().add_combine(d.def.clone()),
        };
        self.after_body_cmd(cmd, d.edit, d.name.clone(), built, |app| {
            if d.tools_visible {
                app.part_mut()
                    .hidden_bodies
                    .retain(|h| !d.def.tools.contains(h));
            }
            if keep_open && d.edit.is_none() && app.solid_count() > 1 {
                let mut next = CombineDraft::new();
                next.def.operation = d.def.operation;
                next.def.keep_tools = d.def.keep_tools;
                next.tools_visible = d.tools_visible;
                app.combine_draft = Some(next);
            } else {
                app.combine_draft = None;
            }
        });
    }

    /// OK (`keep_open` false) or Apply.
    pub(super) fn commit_split(&mut self, keep_open: bool) {
        let Some(d) = self.split_draft.clone() else {
            return;
        };
        let cmd = if d.edit.is_some() {
            "Edit Feature"
        } else {
            "Split"
        };
        self.checkpoint(cmd);
        let built = match d.edit {
            Some(k) => self
                .part_mut()
                .replace_feature(k, Feature::Split(d.def.clone())),
            None => self.part_mut().add_split(d.def.clone()),
        };
        self.after_body_cmd(cmd, d.edit, d.name.clone(), built, |app| {
            if keep_open && d.edit.is_none() {
                let mut next = SplitDraft::new();
                next.def.method = d.def.method;
                next.def.flip = d.def.flip;
                next.def.keep_sketch_visible = d.def.keep_sketch_visible;
                app.split_draft = Some(next);
            } else {
                app.split_draft = None;
            }
        });
    }

    fn after_body_cmd(
        &mut self,
        cmd: &str,
        edit: Option<usize>,
        name: Option<String>,
        built: Result<String, crate::part::PartError>,
        next: impl FnOnce(&mut Self),
    ) {
        if built.is_err()
            && let Some(k) = edit
        {
            self.part_mut().rollback = Some(k);
            let _ = self.part_mut().regenerate();
        }
        match built {
            Ok(_) => {
                let k = edit.unwrap_or(self.part().last_inserted());
                if let Some(n) = name {
                    let _ = self.part_mut().rename_feature(k, &n);
                }
                self.file_status = None;
                self.scene_dirty = true;
                next(self);
                if self.combine_draft.is_none() && self.split_draft.is_none() {
                    self.end_feature_edit();
                    self.selection = super::Selection::Feature(k);
                }
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("{cmd}: {e}")));
            }
        }
    }

    fn body_crumb(ui: &mut egui::Ui, crumbs: &mut BreadcrumbState, name: &str) -> Option<String> {
        let c = [Crumb::feature(name)];
        match breadcrumb::show(ui, crumbs, &c) {
            Some(CrumbEvent::Rename(_, n)) => Some(n),
            _ => None,
        }
    }

    fn ok_row(&mut self, ui: &mut egui::Ui, ready: bool, editing: bool, combine: bool) {
        let preview = self.body_cmd_preview();
        match &preview {
            Err(e) if e == super::PREVIEW_PENDING => {
                ui.label(e);
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(50));
            }
            Err(e) if ready && !e.is_empty() => warning_box(ui, e),
            _ => {}
        }
        let ok =
            ready && (preview.is_ok() || matches!(&preview, Err(e) if e == super::PREVIEW_PENDING));
        ui.horizontal(|ui| {
            let commit = |app: &mut Self, keep: bool| {
                if combine {
                    app.commit_combine(keep);
                } else {
                    app.commit_split(keep);
                }
            };
            if ui
                .add_enabled(ok, egui::Button::new("OK").min_size(egui::vec2(60.0, 22.0)))
                .clicked()
            {
                commit(self, false);
            }
            if !editing
                && ui
                    .add_enabled(
                        ok,
                        egui::Button::new("Apply").min_size(egui::vec2(60.0, 22.0)),
                    )
                    .on_hover_text("Create it and keep the command open")
                    .clicked()
            {
                commit(self, true);
            }
            if ui
                .add(egui::Button::new("Cancel").min_size(egui::vec2(60.0, 22.0)))
                .clicked()
            {
                self.close_body_cmds();
                self.end_feature_edit();
            }
        });
    }

    /// The Combine panel.
    pub(super) fn combine_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(mut d) = self.combine_draft.clone() else {
            return;
        };
        self.panel_header(ui, Icon::Combine, "Combine");
        let shown = d.name.clone().unwrap_or_else(|| match d.edit {
            Some(k) => self
                .part()
                .features
                .get(k)
                .map_or_else(String::new, |f| f.name().to_owned()),
            None => self.part().next_feature_id("Combine"),
        });
        if let Some(n) = Self::body_crumb(ui, &mut d.crumbs, &shown) {
            d.name = Some(n);
        }
        let part = self.part();
        let flow = d.flow();
        let tools = match d.def.tools.len() {
            0 => String::new(),
            1 => body_label(part, &d.def.tools[0]),
            n => format!("{n} bodies"),
        };
        let texts = vec![
            (
                "base",
                if d.def.base.is_empty() {
                    String::new()
                } else {
                    body_label(part, &d.def.base)
                },
            ),
            ("tools", tools),
        ];
        let mut ev = RowEvents::default();
        psection(ui, "combine_input", "Input Geometry", |ui| {
            ev = selector_rows(ui, &flow, &texts);
        });
        if let Some(k) = ev.activate {
            d.set_active(k);
        }
        match ev.clear {
            Some("base") => {
                d.def.base.clear();
                d.input = "base";
            }
            Some("tools") => d.def.tools.clear(),
            _ => {}
        }
        psection(ui, "combine_output", "Behavior", |ui| {
            plabel(ui, "Operation");
            ui.horizontal(|ui| {
                for (o, tip) in [
                    (Operation::Join, "Add the toolbodies to the base"),
                    (Operation::Cut, "Remove the toolbodies from the base"),
                    (
                        Operation::Intersect,
                        "Keep what the base shares with the toolbodies",
                    ),
                ] {
                    ui.radio_value(&mut d.def.operation, o, o.label())
                        .on_hover_text(tip);
                }
            });
            ui.end_row();
            plabel(ui, "Keep Toolbody");
            ui.checkbox(&mut d.def.keep_tools, "")
                .on_hover_text("The toolbodies stay as independent bodies, hidden after OK");
            ui.end_row();
            plabel(ui, "Visibility");
            ui.checkbox(&mut d.tools_visible, "Keep toolbodies visible")
                .on_hover_text("Leave the kept toolbodies visible for the next selection");
            ui.end_row();
        });
        let ready = d.flow().ready() && d.def.validate().is_ok();
        let editing = d.edit.is_some();
        self.combine_draft = Some(d);
        self.ok_row(ui, ready, editing, true);
    }

    /// The Split panel.
    pub(super) fn split_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(mut d) = self.split_draft.clone() else {
            return;
        };
        let many = self.solid_count() > 1;
        self.panel_header(ui, Icon::SplitBody, "Split");
        let shown = d.name.clone().unwrap_or_else(|| match d.edit {
            Some(k) => self
                .part()
                .features
                .get(k)
                .map_or_else(String::new, |f| f.name().to_owned()),
            None => self.part().next_feature_id("Split"),
        });
        if let Some(n) = Self::body_crumb(ui, &mut d.crumbs, &shown) {
            d.name = Some(n);
        }
        psection(ui, "split_method", "Method", |ui| {
            plabel(ui, "Method");
            let mut m = d.def.method;
            ui.vertical(|ui| {
                for (x, tip) in [
                    (
                        SplitMethod::SplitFace,
                        "Divide faces; the solid stays as it is",
                    ),
                    (SplitMethod::TrimSolid, "Remove one side of the solid"),
                    (SplitMethod::SplitSolid, "Keep both sides as two solids"),
                ] {
                    ui.radio_value(&mut m, x, x.label()).on_hover_text(tip);
                }
            });
            ui.end_row();
            d.set_method(m, many);
        });
        let part = self.part();
        let sketches: Vec<(String, String)> = part
            .sketches
            .iter()
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect();
        let flow = d.flow(many);
        let texts = vec![
            (
                "tool",
                d.def
                    .tool
                    .as_ref()
                    .map_or_else(String::new, |t| describe_tool(part, t)),
            ),
            (
                "faces",
                match d.def.faces.len() {
                    0 => String::new(),
                    n => format!("{n} face(s)"),
                },
            ),
            (
                "solid",
                if d.def.solid.is_empty() {
                    String::new()
                } else {
                    body_label(part, &d.def.solid)
                },
            ),
        ];
        let mut ev = RowEvents::default();
        let mut pick_sketch: Option<String> = None;
        psection(ui, "split_input", "Input Geometry", |ui| {
            ev = selector_rows(ui, &flow, &texts);
            plabel(ui, "Sketch");
            egui::ComboBox::from_id_salt("split_sketch")
                .selected_text(match &d.def.tool {
                    Some(SplitToolRef::Sketch(id)) => sketches
                        .iter()
                        .find(|(i, _)| i == id)
                        .map_or(id.clone(), |(_, n)| n.clone()),
                    _ => "choose…".into(),
                })
                .show_ui(ui, |ui| {
                    for (id, name) in &sketches {
                        if ui.selectable_label(false, name).clicked() {
                            pick_sketch = Some(id.clone());
                        }
                    }
                })
                .response
                .on_hover_text("A 2D sketch as the tool (or click its curves in the view)");
            ui.end_row();
            if d.def.method == SplitMethod::SplitFace {
                plabel(ui, "All Faces");
                if ui
                    .checkbox(&mut d.def.all_faces, "")
                    .on_hover_text("Split every face of the solid the tool meets")
                    .changed()
                {
                    let f = Flow::new(split_selectors(&d.def, many), None);
                    d.input = f.active_key().unwrap_or("tool");
                }
                ui.end_row();
            }
        });
        if let Some(id) = pick_sketch {
            d.def.tool = Some(SplitToolRef::Sketch(id));
            d.picked(many);
        }
        if let Some(k) = ev.activate {
            d.set_active(k, many);
        }
        match ev.clear {
            Some("tool") => {
                d.def.tool = None;
                d.input = "tool";
            }
            Some("faces") => d.def.faces.clear(),
            Some("solid") => d.def.solid.clear(),
            _ => {}
        }
        if d.def.method == SplitMethod::TrimSolid {
            psection(ui, "split_side", "Behavior", |ui| {
                plabel(ui, "Keep");
                ui.vertical(|ui| {
                    ui.radio_value(&mut d.def.flip, false, "Keep Default side")
                        .on_hover_text("The side the arrow points to is removed");
                    ui.radio_value(&mut d.def.flip, true, "Keep Flip side")
                        .on_hover_text("The arrow's side stays, the other goes");
                });
                ui.end_row();
            });
        }
        if d.def.sketch().is_some() {
            psection(ui, "split_adv", "Advanced Settings", |ui| {
                plabel(ui, "Sketch");
                ui.checkbox(&mut d.def.keep_sketch_visible, "Keep sketch visible on (+)")
                    .on_hover_text("The tool sketch stays visible after the feature");
                ui.end_row();
            });
        }
        if d.input == "tool" {
            ui.label(
                RichText::new(
                    "Tool: click a plane, a sketch or a surface in the view, or Origin → XY / XZ / YZ, a work plane, a sketch or a surface body in the browser",
                )
                .weak(),
            );
        }
        let ready = d.flow(many).ready();
        let editing = d.edit.is_some();
        self.split_draft = Some(d);
        self.ok_row(ui, ready, editing, false);
    }

    /// Properties of a Combine or Split selected in the browser.
    pub(super) fn body_cmd_properties(&mut self, ui: &mut egui::Ui, k: usize) {
        let Some(f) = self.part().features.get(k).cloned() else {
            return;
        };
        let part = self.part();
        let (icon, rows): (Icon, Vec<(&str, String)>) = match &f {
            Feature::Combine(c) => (
                Icon::Combine,
                vec![
                    ("Base", body_label(part, &c.base)),
                    (
                        "Toolbodies",
                        c.tools
                            .iter()
                            .map(|t| body_label(part, t))
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                    ("Operation", c.operation.label().to_owned()),
                    (
                        "Keep Toolbody",
                        if c.keep_tools { "yes" } else { "no" }.to_owned(),
                    ),
                ],
            ),
            Feature::Split(s) => (
                Icon::SplitBody,
                vec![
                    ("Method", s.method.label().to_owned()),
                    (
                        "Split Tool",
                        s.tool
                            .as_ref()
                            .map_or_else(String::new, |t| describe_tool(part, t)),
                    ),
                    (
                        "Faces",
                        if s.all_faces {
                            "All Faces".to_owned()
                        } else {
                            format!("{}", s.faces.len())
                        },
                    ),
                    (
                        "Solid",
                        if s.solid.is_empty() {
                            "-".into()
                        } else {
                            body_label(part, &s.solid)
                        },
                    ),
                ],
            ),
            _ => return,
        };
        let health = part.feature_health.get(f.id()).cloned();
        self.panel_header(ui, icon, f.name());
        if let Some(h) = health {
            warning_box(ui, &h);
        }
        psection(ui, &format!("bodycmd_{k}"), "Input", |ui| {
            for (l, v) in rows {
                plabel(ui, l);
                ui.label(v);
                ui.end_row();
            }
        });
        if ui.button("Edit Feature").clicked() {
            self.edit_feature_dialog(k);
        }
        if ui.button("Delete").clicked() {
            self.delete_feature(k);
        }
    }

    /// The right-click menu options of the open command.
    pub(super) fn body_cmd_menu_options(&self) -> Vec<MenuOption> {
        if let Some(d) = &self.combine_draft {
            return d.menu_options();
        }
        self.split_draft
            .as_ref()
            .map(SplitDraft::menu_options)
            .unwrap_or_default()
    }

    pub(super) fn body_cmd_menu_option(&mut self, k: usize) {
        let many = self.solid_count() > 1;
        if let Some(d) = self.combine_draft.as_mut() {
            d.menu_option(k);
        }
        if let Some(d) = self.split_draft.as_mut() {
            d.menu_option(k, many);
        }
    }

    /// The faces of hidden bodies (not drawn, not picked).
    pub(super) fn hidden_faces(&self) -> std::collections::HashSet<FaceId> {
        let part = self.part();
        part.hidden_bodies
            .iter()
            .flat_map(|n| psel::faces_of_body(&part.body, n))
            .collect()
    }

    /// The edges of the shown faces, or `None` when every body is shown.
    pub(super) fn shown_edges(&self) -> Option<std::collections::HashSet<crate::kernel::EdgeId>> {
        let hidden = self.hidden_faces();
        if hidden.is_empty() {
            return None;
        }
        let body = &self.part().body;
        Some(
            body.faces()
                .filter(|(f, _)| !hidden.contains(f))
                .flat_map(|(f, _)| psel::edges_of_face(body, f))
                .collect(),
        )
    }

    /// Body visibility (browser): hides or shows body `name`.
    pub(super) fn set_body_visible(&mut self, name: &str, visible: bool) {
        self.checkpoint("Visibility");
        let part = self.part_mut();
        part.hidden_bodies.retain(|h| h != name);
        if !visible {
            part.hidden_bodies.push(name.to_owned());
        }
        self.scene_dirty = true;
    }
}
