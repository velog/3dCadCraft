//! The Sheet Metal environment (`docs/spec/5.6-sheet-metal.md`, §3.5 for the
//! panels): the Sheet Metal tab of a sheet metal part. Every command opens a
//! property panel like Inventor's — breadcrumb name, **Input Geometry**
//! selectors (`command_ui` rows: the active one takes the clicks in the view,
//! single picks move on), **Behavior** / **Bend Properties** / **Corner**
//! sections with expression fields, **OK / Apply / Cancel** — shows the result
//! as a see-through preview computed on a worker thread (`sm_preview`), and
//! has drag arrows where a length can be dragged (`sm_handles`): Flange height
//! and width offsets, Hem length, Contour Flange distance. Edit Feature
//! reopens the same panel (`sm_edit`). Sheet Metal Defaults is a window;
//! Flat Pattern shows the unfolded part.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, TryRecvError};

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke};

use super::CadCraftApp;
use super::command_ui::{NameCrumb, name_crumb, selector_rows};
use super::panels::{Fields, field_row, plabel, psection, warning_box};
use super::valuefield::ExprValue;
use crate::command::flow::{Flow, PickKind, Selector};
use crate::kernel::{Point3, Vector3};
use crate::part::sheet_metal::{
    CornerRelief, FlangePosition, FlangeWidth, FoldLocation, SeamType, SheetConvertFeature,
    SheetEdge, SheetFaceFeature, SheetFlangeFeature, SheetMetalDefaults, SheetOp, SheetOpFeature,
    rule_of,
};
use crate::part::{Feature, PartDocument, PartError, ProfileRef};

/// Hover highlight.
const PICK: Color32 = Color32::from_rgb(60, 190, 90);
/// What a selector holds.
const CHOSEN: Color32 = Color32::from_rgb(40, 120, 230);
/// The preview's edges.
const PREVIEW: Color32 = Color32::from_rgb(230, 120, 20);
/// Selector dots.
const C_INPUT: [u8; 3] = [40, 120, 230];
const C_SECOND: [u8; 3] = [200, 90, 40];

/// An open sheet metal command: the feature being defined.
#[derive(Clone, Debug)]
pub(super) struct SmDraft {
    /// The definition (a `SheetFace`, `SheetFlange`, `SheetConvert` or
    /// `SheetOp`; id and name are given on OK).
    pub feature: Feature,
    /// The sketch (index) of a Face, Contour Flange, Fold or Cut.
    pub sketch: Option<usize>,
    /// The active selector.
    pub active: &'static str,
    /// Edit Feature of this feature index.
    pub edit: Option<usize>,
    /// The breadcrumb (a rename is applied on OK).
    pub crumb: NameCrumb,
}

impl SmDraft {
    fn new(feature: Feature, active: &'static str) -> Self {
        Self {
            feature,
            sketch: None,
            active,
            edit: None,
            crumb: Default::default(),
        }
    }

    /// The command's name.
    pub(super) fn title(&self) -> &'static str {
        match &self.feature {
            Feature::SheetFace(_) => "Face",
            Feature::SheetFlange(_) => "Flange",
            Feature::SheetConvert(_) => "Convert to Sheet Metal",
            Feature::SheetOp(o) => match o.op {
                SheetOp::Hem { .. } => "Hem",
                SheetOp::Fold { .. } => "Fold",
                SheetOp::Contour { .. } => "Contour Flange",
                SheetOp::Cut { .. } => "Cut",
                SheetOp::CornerSeam { .. } => "Corner Seam",
                SheetOp::Unfold { .. } => "Unfold",
                SheetOp::Refold { .. } => "Refold",
            },
            _ => "Sheet Metal",
        }
    }

    /// The prefix of a new feature's id.
    fn prefix(&self) -> &'static str {
        match &self.feature {
            Feature::SheetFace(_) => "Face",
            Feature::SheetFlange(_) => "Flange",
            Feature::SheetConvert(_) => "Convert",
            Feature::SheetOp(o) => o.kind_name(),
            _ => "Feature",
        }
    }

    /// Its selectors (Inventor's Input Geometry rows).
    pub(super) fn flow(&self) -> Flow {
        let sel = |key, label, prompt, accepts, single, count| Selector {
            key,
            label,
            prompt,
            accepts,
            single,
            required: true,
            count,
            color: if key == "bends" { C_SECOND } else { C_INPUT },
        };
        let s = match &self.feature {
            Feature::SheetFace(f) => vec![sel(
                "profile",
                "Profile",
                "click the closed profile of a sketch",
                &[PickKind::Region][..],
                true,
                f.profiles.len(),
            )],
            Feature::SheetFlange(f) => vec![sel(
                "edges",
                "Edges",
                "click the sheet edges to bend (a second click removes one)",
                &[PickKind::Edge][..],
                false,
                f.edges.len(),
            )],
            Feature::SheetConvert(c) => vec![
                sel(
                    "face",
                    "Base Face",
                    "click the face that stays put",
                    &[PickKind::PlanarFace][..],
                    true,
                    usize::from(!c.face.face.is_empty()),
                ),
                sel(
                    "bends",
                    "Bends",
                    "click the edges that become bends (a second click removes one)",
                    &[PickKind::Edge][..],
                    false,
                    c.bends.len(),
                ),
            ],
            Feature::SheetOp(o) => match &o.op {
                SheetOp::Hem { edges, .. } => vec![sel(
                    "edges",
                    "Edges",
                    "click the sheet edges to hem (a second click removes one)",
                    &[PickKind::Edge][..],
                    false,
                    edges.len(),
                )],
                SheetOp::Fold { line, .. } => vec![sel(
                    "line",
                    "Bend Line",
                    "click a sketch line on a face of the sheet",
                    &[PickKind::SketchCurve][..],
                    true,
                    usize::from(!line.is_empty()),
                )],
                SheetOp::Contour { sketch, .. } => vec![sel(
                    "profile",
                    "Profile",
                    "click a line of the open profile (a sketch of lines)",
                    &[PickKind::SketchCurve][..],
                    true,
                    usize::from(!sketch.is_empty()),
                )],
                SheetOp::Cut { profiles, .. } => vec![sel(
                    "profiles",
                    "Profiles",
                    "click the closed profiles to cut (a second click removes one)",
                    &[PickKind::Region][..],
                    false,
                    profiles.len(),
                )],
                SheetOp::CornerSeam { a, .. } => vec![sel(
                    "corner",
                    "Corner",
                    "click an edge of a flange next to the corner",
                    &[PickKind::Edge][..],
                    true,
                    usize::from(!a.is_empty()),
                )],
                SheetOp::Unfold { .. } | SheetOp::Refold { .. } => Vec::new(),
            },
            _ => Vec::new(),
        };
        Flow::new(s, Some(self.active))
    }
}

/// The preview: the new or changed faces as triangles and edges.
#[derive(Clone, Debug, Default)]
pub(super) struct SmPreview {
    tris: Vec<[Point3; 3]>,
    edges: Vec<Vec<Point3>>,
}

type SmResult = Result<SmPreview, String>;

/// The environment's state.
#[derive(Default)]
pub(super) struct SheetUi {
    pub draft: Option<SmDraft>,
    /// Sheet Metal Defaults being edited.
    pub defaults: Option<SheetMetalDefaults>,
    /// The view shows the flat pattern.
    pub flat: bool,
    /// The flat pattern's DXF waiting for the Export dialog's path.
    pub dxf: Option<String>,
    /// The running preview job (its key).
    job: RefCell<Option<(String, Receiver<SmResult>)>>,
    /// The last finished preview.
    ready: RefCell<Option<(String, SmResult)>>,
}

/// A pick in the graphics window.
#[derive(Clone, Debug)]
enum Pick {
    Region(usize, ProfileRef),
    Line(usize, String),
    SheetEdge(String, SheetEdge),
    Corner(crate::part::sheet_metal::SheetCorner),
    Face(String, Point3),
    Bend(crate::part::EdgeRef),
}

/// Builds the draft's feature into `part` (a new one, or Edit Feature).
fn build(
    part: &mut PartDocument,
    feature: Feature,
    edit: Option<usize>,
    sketch: Option<usize>,
) -> Result<String, PartError> {
    if let Some(k) = edit {
        return part.replace_feature(k, feature);
    }
    match feature {
        Feature::SheetFace(f) => {
            let i = sketch.ok_or_else(|| PartError::Feature("select the profile".into()))?;
            part.add_sheet_face(i, f.profiles, f.flip)
        }
        Feature::SheetFlange(f) => part.add_sheet_flange(f),
        Feature::SheetConvert(f) => part.add_sheet_convert(f),
        Feature::SheetOp(f) => part.add_sheet_op(f),
        _ => Err(PartError::Feature("not a sheet metal feature".into())),
    }
}

/// The faces of `part` that are not in `before` (by name) or carry `id`:
/// their triangles and edges.
fn new_faces(part: &PartDocument, before: &HashSet<String>, id: &str) -> SmPreview {
    let body = &part.body;
    let opts = crate::kernel::TessellationOptions::default();
    let key = format!("{id}.");
    let key2 = format!(":{id}");
    let mut out = SmPreview::default();
    let mut seen = HashSet::new();
    for (fid, f) in body.faces() {
        let Some(n) = f.name.as_deref() else {
            continue;
        };
        if before.contains(n) && !n.contains(&key) && !n.contains(&key2) {
            continue;
        }
        if let Some(m) = &f.mesh {
            for t in &m.triangles {
                if let (Some(a), Some(b), Some(c)) = (
                    m.positions.get(t[0] as usize),
                    m.positions.get(t[1] as usize),
                    m.positions.get(t[2] as usize),
                ) {
                    out.tris.push([*a, *b, *c]);
                }
            }
        }
        for e in crate::part::select::edges_of_face(body, fid) {
            if seen.insert(e)
                && let Ok(p) = body.sample_edge(e, &opts)
            {
                out.edges.push(p);
            }
        }
    }
    out
}

impl CadCraftApp {
    /// Whether the active part is a sheet metal part.
    pub(super) fn is_sheet_metal(&self) -> bool {
        !self.is_assembly() && self.part().sheet_metal.is_some()
    }

    /// Opens a command panel (the flat view goes back to the folded part).
    fn sm_open(&mut self, d: SmDraft) {
        self.finish_sketch();
        self.mode = super::Mode::Normal;
        self.cancel_extrude();
        self.work_draft = None;
        self.blend_draft = None;
        if self.sm.flat {
            self.sm.flat = false;
            self.scene_dirty = true;
        }
        self.sm.draft = Some(d);
    }

    /// The sheet bodies by name.
    fn sm_sheets(&self) -> Vec<String> {
        let mut v: Vec<String> = self.part().sheet_models.keys().cloned().collect();
        v.sort();
        v
    }

    /// The Sheet Metal commands.
    pub(super) fn sm_apply(&mut self, a: super::ribbon::Action) {
        use super::ribbon::Action;
        let needs_sheet = !matches!(
            a,
            Action::SmConvert | Action::SmDefaults | Action::SmFace | Action::SmContour
        );
        if needs_sheet && self.part().sheet_models.is_empty() {
            self.file_status = Some(Err(format!(
                "{}: make a Face (or Convert a solid) first",
                super::command_label(a)
            )));
            return;
        }
        let rule = rule_of(self.part()).values();
        match a {
            Action::SmConvert => {
                // A solid that is not a sheet yet: its base face and bends
                // (SolidWorks / Solid Edge); else only the rule.
                let solid = self
                    .part()
                    .exact_bodies
                    .0
                    .iter()
                    .filter_map(|s| s.name.clone())
                    .find(|n| !self.part().sheet_models.contains_key(n));
                if let Some(body) = solid
                    && self.part().exact
                {
                    let def = SheetConvertFeature {
                        id: String::new(),
                        name: String::new(),
                        body,
                        face: crate::part::shell::FaceRef {
                            face: String::new(),
                            hint: [0.0; 3],
                        },
                        bends: Vec::new(),
                    };
                    self.sm_open(SmDraft::new(Feature::SheetConvert(def), "face"));
                    self.tab = super::ribbon::Tab::SheetMetal;
                    return;
                }
                if self.part().sheet_metal.is_none() {
                    self.checkpoint("Convert to Sheet Metal");
                    self.part_mut().make_sheet_metal(None);
                    self.file_status = Some(Ok(
                        "Converted to sheet metal: the Sheet Metal tab has its commands".into(),
                    ));
                }
                self.tab = super::ribbon::Tab::SheetMetal;
            }
            Action::SmDefaults => {
                self.sm.defaults = Some(rule_of(self.part()));
            }
            Action::SmFace => {
                let mut d = SmDraft::new(
                    Feature::SheetFace(SheetFaceFeature {
                        id: String::new(),
                        name: String::new(),
                        sketch: String::new(),
                        profiles: Vec::new(),
                        flip: false,
                    }),
                    "profile",
                );
                // Inventor: a single closed profile is taken by itself.
                if let Some(i) = self.sm_newest_sketch(&|ps| !ps.sketch.regions().is_empty()) {
                    let ps = &self.part().sketches[i];
                    let regions = self.sketch_regions(&ps.sketch);
                    if regions.len() == 1
                        && let Feature::SheetFace(f) = &mut d.feature
                    {
                        f.sketch = ps.id.clone();
                        f.profiles = vec![ProfileRef::of(&regions[0], regions[0].interior_point())];
                        d.sketch = Some(i);
                    }
                }
                self.sm_open(d);
            }
            Action::SmFlange => {
                self.sm_open(SmDraft::new(
                    Feature::SheetFlange(SheetFlangeFeature::new(
                        "",
                        Vec::new(),
                        rule.thickness * 10.0,
                    )),
                    "edges",
                ));
            }
            Action::SmHem => {
                self.sm_open(SmDraft::new(
                    Feature::SheetOp(SheetOpFeature::new(SheetOp::Hem {
                        body: String::new(),
                        edges: Vec::new(),
                        length: rule.thickness * 4.0,
                        gap: None,
                        flip: false,
                    })),
                    "edges",
                ));
            }
            Action::SmFold => {
                self.sm_open(SmDraft::new(
                    Feature::SheetOp(SheetOpFeature::new(SheetOp::Fold {
                        body: String::new(),
                        sketch: String::new(),
                        line: String::new(),
                        angle: std::f64::consts::FRAC_PI_2,
                        radius: None,
                        location: FoldLocation::Centerline,
                        flip: false,
                        flip_side: false,
                    })),
                    "line",
                ));
            }
            Action::SmContour => {
                let mut d = SmDraft::new(
                    Feature::SheetOp(SheetOpFeature::new(SheetOp::Contour {
                        sketch: String::new(),
                        width: 50.0,
                        flip: false,
                    })),
                    "profile",
                );
                // Inventor: an open sketch is taken by itself.
                if let Some(i) = self.sm_newest_sketch(&|ps| {
                    ps.sketch.regions().is_empty() && ps.sketch.entities().next().is_some()
                }) && let Feature::SheetOp(o) = &mut d.feature
                    && let SheetOp::Contour { sketch, .. } = &mut o.op
                {
                    *sketch = self.part().sketches[i].id.clone();
                    d.sketch = Some(i);
                }
                self.sm_open(d);
            }
            Action::SmCut => {
                let mut d = SmDraft::new(
                    Feature::SheetOp(SheetOpFeature::new(SheetOp::Cut {
                        sketch: String::new(),
                        profiles: Vec::new(),
                    })),
                    "profiles",
                );
                if let Some(i) = self.sm_newest_sketch(&|ps| !ps.sketch.regions().is_empty()) {
                    let ps = &self.part().sketches[i];
                    let regions = self.sketch_regions(&ps.sketch);
                    if regions.len() == 1
                        && let Feature::SheetOp(o) = &mut d.feature
                        && let SheetOp::Cut { sketch, profiles } = &mut o.op
                    {
                        *sketch = ps.id.clone();
                        *profiles = vec![ProfileRef::of(&regions[0], regions[0].interior_point())];
                        d.sketch = Some(i);
                    }
                }
                self.sm_open(d);
            }
            Action::SmCornerSeam => {
                self.sm_open(SmDraft::new(
                    Feature::SheetOp(SheetOpFeature::new(SheetOp::CornerSeam {
                        body: String::new(),
                        a: String::new(),
                        b: String::new(),
                        seam: SeamType::SymmetricGap,
                        gap: None,
                        ratio: 1.0,
                    })),
                    "corner",
                ));
            }
            Action::SmUnfold | Action::SmRefold => {
                let unfold = matches!(a, Action::SmUnfold);
                let Some(body) = self
                    .sm_sheets()
                    .into_iter()
                    .find(|b| self.part().sheet_is_flat(b) != unfold)
                else {
                    self.file_status = Some(Err(if unfold {
                        "Unfold: every sheet is unfolded already".into()
                    } else {
                        "Refold: no sheet is unfolded (Unfold first)".into()
                    }));
                    return;
                };
                let op = if unfold {
                    SheetOp::Unfold { body }
                } else {
                    SheetOp::Refold { body }
                };
                self.sm_open(SmDraft::new(Feature::SheetOp(SheetOpFeature::new(op)), ""));
            }
            Action::SmExportFlat => {
                let Some(body) = self.sm_sheets().into_iter().next() else {
                    return;
                };
                match self.part().flat_pattern_dxf(&body) {
                    Ok(text) => {
                        self.sm.dxf = Some(text);
                        self.open_dialog(super::files::DialogPurpose::ExportDxf);
                    }
                    Err(e) => self.file_status = Some(Err(format!("Export Flat Pattern: {e}"))),
                }
            }
            Action::SmFlatPattern => {
                self.sm.flat = !self.sm.flat;
                self.scene_dirty = true;
            }
            _ => {}
        }
    }

    /// The newest sketch that `usable` accepts (the selected one first).
    fn sm_newest_sketch(&self, usable: &dyn Fn(&crate::part::PartSketch) -> bool) -> Option<usize> {
        let p = self.part();
        let preferred = match self.selection {
            super::Selection::Sketch(i) => Some(i),
            _ => None,
        };
        preferred
            .filter(|&i| p.sketches.get(i).is_some_and(usable))
            .or_else(|| {
                (0..p.sketches.len())
                    .rev()
                    .find(|&i| p.consumers(&p.sketches[i].id).is_empty() && usable(&p.sketches[i]))
            })
    }

    /// Edit Feature of a sheet metal feature: the same panel.
    pub(super) fn sm_edit(&mut self, k: usize) {
        let Some(f) = self.part().features.get(k).cloned() else {
            return;
        };
        let sketch_id = f.sketch().map(str::to_owned);
        self.roll_back_to(k);
        let sketch = sketch_id.and_then(|id| self.part().sketches.iter().position(|s| s.id == id));
        let mut d = SmDraft::new(f, "");
        d.edit = Some(k);
        d.sketch = sketch;
        d.active = d.flow().selectors.first().map(|s| s.key).unwrap_or("");
        self.sm.flat = false;
        self.sm.draft = Some(d);
    }

    // ---- selector flow (command_ui) ----------------------------------------

    /// The open command's flow.
    pub(super) fn sm_flow(&self) -> Option<Flow> {
        self.sm.draft.as_ref().map(|d| d.flow())
    }

    /// Makes selector `key` active.
    pub(super) fn sm_activate(&mut self, key: &str) {
        if let Some(d) = self.sm.draft.as_mut()
            && let Some(s) = d.flow().selectors.iter().find(|s| s.key == key)
        {
            d.active = s.key;
        }
    }

    /// Cancel (Esc).
    pub(super) fn sm_cancel(&mut self) {
        if self.sm.draft.take().is_some() {
            self.end_feature_edit();
        }
    }

    /// The right-click menu's options (Flip).
    pub(super) fn sm_menu_options(&self) -> Vec<super::command_ui::MenuOption> {
        let flip = match self.sm.draft.as_ref().map(|d| &d.feature) {
            Some(Feature::SheetFace(f)) => Some(f.flip),
            Some(Feature::SheetFlange(f)) => Some(f.flip),
            Some(Feature::SheetOp(o)) => match &o.op {
                SheetOp::Hem { flip, .. }
                | SheetOp::Fold { flip, .. }
                | SheetOp::Contour { flip, .. } => Some(*flip),
                _ => None,
            },
            _ => None,
        };
        flip.map(|f| {
            vec![super::command_ui::MenuOption {
                label: "Flip".into(),
                checked: f,
            }]
        })
        .unwrap_or_default()
    }

    /// Option `k` of [`Self::sm_menu_options`] (Flip).
    pub(super) fn sm_menu_option(&mut self, _k: usize) {
        if let Some(d) = self.sm.draft.as_mut() {
            match &mut d.feature {
                Feature::SheetFace(f) => f.flip = !f.flip,
                Feature::SheetFlange(f) => f.flip = !f.flip,
                Feature::SheetOp(o) => match &mut o.op {
                    SheetOp::Hem { flip, .. }
                    | SheetOp::Fold { flip, .. }
                    | SheetOp::Contour { flip, .. } => *flip = !*flip,
                    _ => {}
                },
                _ => {}
            }
        }
    }

    /// OK (`keep_open` false) or Apply.
    pub(super) fn sm_commit(&mut self, keep_open: bool) {
        let Some(d) = self.sm.draft.clone() else {
            return;
        };
        if !d.flow().ready() {
            return;
        }
        let cmd = if d.edit.is_some() {
            "Edit Feature"
        } else {
            d.title()
        };
        self.checkpoint(cmd);
        let mut part = self.part().clone();
        match build(&mut part, d.feature.clone(), d.edit, d.sketch) {
            Ok(id) => {
                *self.part_mut() = part;
                if let Some(name) = d.crumb.name.clone()
                    && let Some(f) = self.part_mut().features.iter_mut().find(|f| f.id() == id)
                {
                    f.set_name(&name);
                }
                if d.edit.is_some() {
                    self.part_mut().rollback = None;
                    if let Err(e) = self.part_mut().regenerate() {
                        self.file_status = Some(Err(e.to_string()));
                    }
                }
                self.file_status = None;
                self.scene_dirty = true;
                if keep_open && d.edit.is_none() {
                    // Apply: the same command again, its inputs cleared.
                    let mut next = d.clone();
                    next.crumb = Default::default();
                    sm_clear_inputs(&mut next.feature);
                    next.active = next.flow().selectors.first().map(|s| s.key).unwrap_or("");
                    self.sm.draft = Some(next);
                } else {
                    self.sm.draft = None;
                    let k = d.edit.unwrap_or(self.part().last_inserted());
                    self.selection = super::Selection::Feature(k);
                }
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("{cmd}: {e}")));
            }
        }
    }

    /// The status-bar prompt.
    pub(super) fn sm_prompt(&self) -> Option<String> {
        let d = self.sm.draft.as_ref()?;
        let flow = d.flow();
        if flow.selectors.is_empty() {
            return Some(format!("{}: OK (Enter)", d.title()));
        }
        let ok = if flow.ready() {
            super::i18n::tr(super::command_ui::OR_OK)
        } else {
            "".into()
        };
        Some(format!(
            "{}: {}{ok}",
            d.title(),
            super::i18n::tr(flow.prompt())
        ))
    }

    // ---- the panel ---------------------------------------------------------

    /// The command's property panel.
    pub(super) fn sm_panel(&mut self, ui: &mut egui::Ui) {
        let Some(mut d) = self.sm.draft.clone() else {
            return;
        };
        self.panel_header(ui, super::icons::Icon::SheetMetal, d.title());
        let shown = match d.edit {
            Some(k) => self
                .part()
                .features
                .get(k)
                .map_or(d.title().to_owned(), |f| f.name().to_owned()),
            None => self.part().next_feature_id(d.prefix()),
        };
        name_crumb(ui, &mut d.crumb, &shown);
        let flow = d.flow();
        let table = self.part().parameter_table();
        let mut fx = Fields {
            table: &table,
            key: "sm_draft".into(),
            commits: Vec::new(),
            bodies: Vec::new(),
        };
        let snapshot = d.feature.clone();
        let rule = rule_of(self.part()).values();
        let texts = self.sm_selector_texts(&d);
        let first_edge = match &d.feature {
            Feature::SheetFlange(f) => self.sm_first_edge_length(&f.body, &f.edges),
            _ => 20.0,
        };
        let mut ev = super::command_ui::RowEvents::default();
        if !flow.selectors.is_empty() {
            psection(ui, "sm_input", "Input Geometry", |ui| {
                ev = selector_rows(ui, &flow, &texts);
            });
        }
        match &mut d.feature {
            Feature::SheetFace(f) => {
                psection(ui, "sm_face_behavior", "Behavior", |ui| {
                    plabel(ui, "Direction");
                    direction_row(
                        ui,
                        &mut f.flip,
                        "The thickness on the other side of the sketch",
                    );
                    ui.end_row();
                });
                psection(ui, "sm_face_output", "Output", |ui| {
                    plabel(ui, "Thickness");
                    ui.label(format!("{:.3} mm", rule.thickness))
                        .on_hover_text("From Sheet Metal Defaults");
                    ui.end_row();
                });
            }
            Feature::SheetFlange(f) => {
                psection(ui, "sm_flange_width", "Edge Options", |ui| {
                    plabel(ui, "Width Extents");
                    ui.horizontal(|ui| {
                        for (w, tip) in [
                            (FlangeWidth::Edge, "The whole edge"),
                            (FlangeWidth::Width, "A width, centred or from an offset"),
                            (FlangeWidth::Offset, "Offsets from both ends of the edge"),
                        ] {
                            if ui
                                .selectable_label(f.extents == w, w.label())
                                .on_hover_text(tip)
                                .clicked()
                            {
                                f.extents = w;
                                if w == FlangeWidth::Width && f.width <= 0.0 {
                                    f.width = (first_edge * 0.5).round();
                                    f.centered = true;
                                }
                            }
                        }
                    });
                    ui.end_row();
                    match f.extents {
                        FlangeWidth::Edge => {}
                        FlangeWidth::Width => {
                            field_row(ui, &mut fx, "Width", &snapshot, "width");
                            plabel(ui, "Centered");
                            ui.checkbox(&mut f.centered, "")
                                .on_hover_text("Centred on the edge; off: from Offset");
                            ui.end_row();
                            if !f.centered {
                                field_row(ui, &mut fx, "Offset", &snapshot, "offset1");
                            }
                        }
                        FlangeWidth::Offset => {
                            field_row(ui, &mut fx, "Offset 1", &snapshot, "offset1");
                            field_row(ui, &mut fx, "Offset 2", &snapshot, "offset2");
                        }
                    }
                });
                psection(ui, "sm_flange_height", "Height Extents", |ui| {
                    field_row(ui, &mut fx, "Distance", &snapshot, "height");
                    plabel(ui, "Direction");
                    direction_row(ui, &mut f.flip, "The flange to the other side");
                    ui.end_row();
                    field_row(ui, &mut fx, "Angle", &snapshot, "angle");
                    plabel(ui, "Bend Position");
                    ui.horizontal(|ui| {
                        for (p, short) in [
                            (FlangePosition::Inside, "Inside"),
                            (FlangePosition::Middle, "Middle"),
                            (FlangePosition::Outside, "Outside"),
                        ] {
                            ui.selectable_value(&mut f.position, p, short)
                                .on_hover_text(p.label());
                        }
                    });
                    ui.end_row();
                });
                psection(ui, "sm_flange_bend", "Bend Properties", |ui| {
                    radius_row(ui, &mut f.radius, rule.bend_radius);
                    plabel(ui, "Corner");
                    ui.weak(format!(
                        "gap {:.2} mm (rule; Corner Seam changes one corner)",
                        rule.gap
                    ));
                    ui.end_row();
                });
            }
            Feature::SheetConvert(_) => {
                psection(ui, "sm_convert", "Behavior", |ui| {
                    plabel(ui, "Thickness");
                    ui.label(format!("{:.3} mm", rule.thickness)).on_hover_text(
                        "From Sheet Metal Defaults; a shelled solid keeps its own walls",
                    );
                    ui.end_row();
                    plabel(ui, "Other edges");
                    ui.weak("cut open (rips)");
                    ui.end_row();
                });
            }
            Feature::SheetOp(o) => match &mut o.op {
                SheetOp::Hem { gap, flip, .. } => {
                    psection(ui, "sm_hem", "Shape", |ui| {
                        plabel(ui, "Type");
                        ui.horizontal(|ui| {
                            let _ = ui.selectable_label(true, "Single");
                            for t in ["Teardrop", "Rolled", "Double"] {
                                ui.add_enabled(false, egui::Button::new(t))
                                    .on_disabled_hover_text(
                                        "Not yet (docs/spec/5.6-sheet-metal.md)",
                                    );
                            }
                        });
                        ui.end_row();
                        plabel(ui, "Gap");
                        ui.horizontal(|ui| {
                            let mut own = gap.is_some();
                            if ui
                                .checkbox(&mut own, "")
                                .on_hover_text("Off: Thickness × 0.5")
                                .changed()
                            {
                                *gap = own.then_some(rule.thickness * 0.5);
                            }
                            if gap.is_none() {
                                ui.weak(format!("{:.3} mm", rule.thickness * 0.5));
                            }
                        });
                        ui.end_row();
                        if gap.is_some() {
                            field_row(ui, &mut fx, "Gap value", &snapshot, "gap");
                        }
                        field_row(ui, &mut fx, "Length", &snapshot, "length");
                        plabel(ui, "Direction");
                        direction_row(ui, flip, "Fold the hem to the other face");
                        ui.end_row();
                    });
                }
                SheetOp::Fold {
                    radius,
                    location,
                    flip,
                    flip_side,
                    ..
                } => {
                    psection(ui, "sm_fold", "Fold", |ui| {
                        plabel(ui, "Fold Location");
                        ui.vertical(|ui| {
                            for l in FoldLocation::ALL {
                                ui.radio_value(location, l, l.label());
                            }
                        });
                        ui.end_row();
                        field_row(ui, &mut fx, "Fold Angle", &snapshot, "angle");
                        plabel(ui, "Flip Controls");
                        ui.horizontal(|ui| {
                            ui.toggle_value(flip_side, "Flip Side")
                                .on_hover_text("The other side of the line moves");
                            ui.toggle_value(flip, "Flip Direction")
                                .on_hover_text("Fold the other way (up / down)");
                        });
                        ui.end_row();
                    });
                    psection(ui, "sm_fold_bend", "Bend Properties", |ui| {
                        radius_row(ui, radius, rule.bend_radius);
                    });
                }
                SheetOp::Contour { flip, .. } => {
                    psection(ui, "sm_contour", "Behavior", |ui| {
                        field_row(ui, &mut fx, "Distance", &snapshot, "width");
                        plabel(ui, "Position");
                        direction_row(ui, flip, "The material on the other side of the profile");
                        ui.end_row();
                    });
                    psection(ui, "sm_contour_bend", "Bend Properties", |ui| {
                        plabel(ui, "Bend Radius");
                        ui.label(format!("{:.3} mm (rule)", rule.bend_radius));
                        ui.end_row();
                        plabel(ui, "Thickness");
                        ui.label(format!("{:.3} mm (rule)", rule.thickness));
                        ui.end_row();
                    });
                }
                SheetOp::Cut { .. } => {
                    psection(ui, "sm_cut", "Behavior", |ui| {
                        plabel(ui, "Cut");
                        ui.label("Through the face the sketch lies on");
                        ui.end_row();
                        plabel(ui, "Cut Across Bend");
                        ui.weak("Unfold, Cut, Refold");
                        ui.end_row();
                    });
                }
                SheetOp::CornerSeam {
                    seam, gap, ratio, ..
                } => {
                    psection(ui, "sm_seam", "Seam", |ui| {
                        plabel(ui, "Type");
                        ui.vertical(|ui| {
                            for (s, tip) in [
                                (SeamType::SymmetricGap, "Both flanges stop the gap apart"),
                                (
                                    SeamType::Overlap,
                                    "The first flange runs over the other's edge",
                                ),
                                (
                                    SeamType::ReverseOverlap,
                                    "The other flange runs over the first's edge",
                                ),
                            ] {
                                ui.radio_value(seam, s, s.label()).on_hover_text(tip);
                            }
                        });
                        ui.end_row();
                        plabel(ui, "Gap");
                        ui.horizontal(|ui| {
                            let mut own = gap.is_some();
                            if ui
                                .checkbox(&mut own, "")
                                .on_hover_text("Off: the rule's gap")
                                .changed()
                            {
                                *gap = own.then_some(rule.gap);
                            }
                            if gap.is_none() {
                                ui.weak(format!("{:.3} mm (rule)", rule.gap));
                            }
                        });
                        ui.end_row();
                        if gap.is_some() {
                            field_row(ui, &mut fx, "Gap value", &snapshot, "gap");
                        }
                        if *seam != SeamType::SymmetricGap {
                            plabel(ui, "Percent Overlap");
                            ui.add(ExprValue::new(ratio).range(0.0..=1.0))
                                .on_hover_text(
                                    "Of the thickness past the other's inner face (1 = flush)",
                                );
                            ui.end_row();
                        }
                    });
                }
                SheetOp::Unfold { body } | SheetOp::Refold { body } => {
                    psection(ui, "sm_unfold", "Sheet", |ui| {
                        plabel(ui, "Solid");
                        ui.label(body.as_str());
                        ui.end_row();
                        plabel(ui, "Bends");
                        ui.label("All bends")
                            .on_hover_text("The whole sheet; choosing single bends is planned");
                        ui.end_row();
                    });
                }
            },
            _ => {}
        }
        // Field commits (expressions, parameters).
        let commits = std::mem::take(&mut fx.commits);
        drop(fx);
        for (field, text) in commits {
            match self.part().apply_draft_field(&mut d.feature, field, &text) {
                Ok(()) => self.file_status = None,
                Err(e) => self.file_status = Some(Err(format!("{}: {e}", d.title()))),
            }
        }
        if let Some(k) = ev.activate {
            d.active = k;
        }
        if let Some(k) = ev.clear {
            sm_clear(&mut d, k);
            d.active = k;
        }
        self.sm.draft = Some(d.clone());
        // The preview's state.
        let preview = self.sm_preview();
        let failed =
            matches!(&preview, Some(Err(e)) if e != super::PREVIEW_PENDING && !e.is_empty());
        match &preview {
            Some(Err(e)) if e == super::PREVIEW_PENDING => {
                ui.weak(e);
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(50));
            }
            Some(Err(e)) if !e.is_empty() => warning_box(ui, e),
            _ => {}
        }
        let ready = d.flow().ready() && !failed;
        ui.horizontal(|ui| {
            let size = egui::vec2(60.0, 22.0);
            if ui
                .add_enabled(ready, egui::Button::new("OK").min_size(size))
                .on_hover_text("Enter")
                .clicked()
            {
                self.sm_commit(false);
            }
            if d.edit.is_none()
                && ui
                    .add_enabled(ready, egui::Button::new("Apply").min_size(size))
                    .on_hover_text("Create it and start the same command again")
                    .clicked()
            {
                self.sm_commit(true);
            }
            if ui
                .add(egui::Button::new("Cancel").min_size(size))
                .on_hover_text("Esc")
                .clicked()
            {
                self.sm_cancel();
            }
        });
    }

    /// What each selector row shows.
    fn sm_selector_texts(&self, d: &SmDraft) -> Vec<(&'static str, String)> {
        let sketch_name = |id: &str| {
            self.part()
                .sketches
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.name.clone())
                .unwrap_or_default()
        };
        match &d.feature {
            Feature::SheetFace(f) if !f.profiles.is_empty() => {
                vec![("profile", format!("{} profile", sketch_name(&f.sketch)))]
            }
            Feature::SheetFlange(f) if !f.edges.is_empty() => {
                vec![("edges", format!("{} edge(s)", f.edges.len()))]
            }
            Feature::SheetConvert(c) => vec![
                ("face", c.face.face.clone()),
                (
                    "bends",
                    if c.bends.is_empty() {
                        String::new()
                    } else {
                        format!("{} edge(s)", c.bends.len())
                    },
                ),
            ],
            Feature::SheetOp(o) => match &o.op {
                SheetOp::Hem { edges, .. } if !edges.is_empty() => {
                    vec![("edges", format!("{} edge(s)", edges.len()))]
                }
                SheetOp::Fold { sketch, line, .. } if !line.is_empty() => {
                    vec![("line", format!("line of {}", sketch_name(sketch)))]
                }
                SheetOp::Contour { sketch, .. } if !sketch.is_empty() => {
                    vec![("profile", sketch_name(sketch))]
                }
                SheetOp::Cut { profiles, .. } if !profiles.is_empty() => {
                    vec![("profiles", format!("{} profile(s)", profiles.len()))]
                }
                SheetOp::CornerSeam { a, b, .. } if !a.is_empty() => {
                    vec![("corner", format!("{a} / {b}"))]
                }
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    fn sm_first_edge_length(&self, body: &str, edges: &[SheetEdge]) -> f64 {
        edges
            .first()
            .and_then(|e| self.part().sheet_edge_ends(body, e))
            .map_or(20.0, |(a, b)| (b - a).norm())
    }

    // ---- preview -----------------------------------------------------------

    /// The draft's result (new / changed faces), computed on a worker thread;
    /// `None` while inputs are missing.
    fn sm_preview(&self) -> Option<SmResult> {
        let d = self.sm.draft.as_ref()?;
        if !d.flow().ready() {
            return None;
        }
        let key = format!("{:?}|{:?}|{:?}", d.edit, d.sketch, d.feature);
        if let Some((k, r)) = &*self.sm.ready.borrow()
            && *k == key
        {
            return Some(r.clone());
        }
        {
            let mut job = self.sm.job.borrow_mut();
            if let Some((k, rx)) = job.as_ref() {
                match rx.try_recv() {
                    Ok(r) => {
                        let k = k.clone();
                        *job = None;
                        let same = k == key;
                        *self.sm.ready.borrow_mut() = Some((k, r.clone()));
                        if same {
                            return Some(r);
                        }
                    }
                    Err(TryRecvError::Empty) => {
                        return Some(Err(super::PREVIEW_PENDING.into()));
                    }
                    Err(TryRecvError::Disconnected) => *job = None,
                }
            }
        }
        let mut trial = self.part().clone();
        let before: HashSet<String> = trial
            .body
            .faces()
            .filter_map(|(_, f)| f.name.clone())
            .collect();
        let (feature, edit, sketch) = (d.feature.clone(), d.edit, d.sketch);
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("sheet metal preview".into())
            .stack_size(64 << 20)
            .spawn(move || {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    build(&mut trial, feature, edit, sketch)
                        .map(|id| new_faces(&trial, &before, &id))
                        .map_err(|e| e.to_string())
                }))
                .unwrap_or_else(|_| Err("internal error while computing the preview".into()));
                let _ = tx.send(r);
            });
        if spawned.is_err() {
            return Some(Err("the preview could not be started".into()));
        }
        *self.sm.job.borrow_mut() = Some((key, rx));
        Some(Err(super::PREVIEW_PENDING.into()))
    }

    // ---- the view ----------------------------------------------------------

    /// Hover, picks, what the selectors hold and the preview in the view.
    /// Returns `true` when it took the click.
    pub(super) fn sm_view(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        pointer: Option<Pos2>,
        clicked: bool,
    ) -> bool {
        if self.sm.draft.is_none() {
            return false;
        }
        // The preview first (under the highlights).
        match self.sm_preview() {
            Some(Ok(pv)) => self.sm_draw_preview(painter, rect, &pv),
            Some(Err(e)) if e == super::PREVIEW_PENDING => {
                painter
                    .ctx()
                    .request_repaint_after(std::time::Duration::from_millis(50));
            }
            _ => {}
        }
        for line in self.sm_chosen() {
            self.draw_visible_polyline(painter, rect, &line, Stroke::new(3.0, CHOSEN));
        }
        let mut took = false;
        if let Some(p) = pointer {
            let hover = self.sm_hover(p, rect);
            if let Some((_, lines)) = &hover {
                for line in lines {
                    self.draw_visible_polyline(painter, rect, line, Stroke::new(3.0, PICK));
                }
            }
            if clicked && let Some((pick, _)) = hover {
                self.sm_pick(pick);
                took = true;
            }
        }
        took
    }

    fn sm_draw_preview(&self, painter: &egui::Painter, rect: Rect, pv: &SmPreview) {
        let (back, _) = self.camera.frame();
        let step = (pv.tris.len() / 20000).max(1);
        let mut m = egui::Mesh::default();
        for [a, b, c] in pv.tris.iter().step_by(step) {
            let (Some(pa), Some(pb), Some(pc)) = (
                self.camera.project(a, rect),
                self.camera.project(b, rect),
                self.camera.project(c, rect),
            ) else {
                continue;
            };
            let n = (b - a).cross(&(c - a)).try_normalize(1e-18).unwrap_or(back);
            let s = (0.45 + 0.55 * n.dot(&back).abs()) as f32;
            let col = Color32::from_rgba_unmultiplied(
                (255.0 * s) as u8,
                (150.0 * s) as u8,
                (50.0 * s) as u8,
                90,
            );
            let i = m.vertices.len() as u32;
            for q in [pa, pb, pc] {
                m.colored_vertex(q, col);
            }
            m.add_triangle(i, i + 1, i + 2);
        }
        painter.add(egui::Shape::mesh(m));
        let stroke = Stroke::new(1.5, PREVIEW);
        for e in &pv.edges {
            let pts: Vec<Pos2> = e
                .iter()
                .filter_map(|q| self.camera.project(q, rect))
                .collect();
            painter.add(egui::Shape::line(pts, stroke));
        }
    }

    /// What the selectors hold, as polylines.
    fn sm_chosen(&self) -> Vec<Vec<Point3>> {
        let Some(d) = &self.sm.draft else {
            return Vec::new();
        };
        let part = self.part();
        let mut out = Vec::new();
        let edge_line =
            |body: &str, e: &SheetEdge| part.sheet_edge_ends(body, e).map(|(a, b)| vec![a, b]);
        let region_lines =
            |i: Option<usize>, profiles: &[ProfileRef], out: &mut Vec<Vec<Point3>>| {
                if let Some(ps) = i.and_then(|i| part.sketches.get(i)) {
                    let regions = self.sketch_regions(&ps.sketch);
                    let pl = &ps.sketch.plane;
                    for pr in profiles {
                        if let Some(r) = pr.resolve(&regions) {
                            let mut pts: Vec<Point3> = r
                                .outer
                                .polygon(1e-2)
                                .iter()
                                .map(|q| pl.point_at(q.x, q.y))
                                .collect();
                            if let Some(f) = pts.first().copied() {
                                pts.push(f);
                            }
                            out.push(pts);
                        }
                    }
                }
            };
        match &d.feature {
            Feature::SheetFace(f) => region_lines(d.sketch, &f.profiles, &mut out),
            Feature::SheetFlange(f) => {
                out.extend(f.edges.iter().filter_map(|e| edge_line(&f.body, e)));
            }
            Feature::SheetConvert(c) => {
                let opts = crate::kernel::TessellationOptions::default();
                for (fid, face) in part.body.faces() {
                    if !c.face.face.is_empty() && face.name.as_deref() == Some(c.face.face.as_str())
                    {
                        out.extend(
                            crate::part::select::edges_of_face(&part.body, fid)
                                .into_iter()
                                .filter_map(|e| part.body.sample_edge(e, &opts).ok()),
                        );
                    }
                }
                for b in &c.bends {
                    if let Some(e) = part.body.edges().map(|(e, _)| e).find(|&e| {
                        crate::part::blend::edge_ref(&part.body, e)
                            .is_some_and(|r| r.faces == b.faces)
                    }) && let Ok(p) = part.body.sample_edge(e, &opts)
                    {
                        out.push(p);
                    }
                }
            }
            Feature::SheetOp(o) => match &o.op {
                SheetOp::Hem { body, edges, .. } => {
                    out.extend(edges.iter().filter_map(|e| edge_line(body, e)));
                }
                SheetOp::Cut { profiles, .. } => region_lines(d.sketch, profiles, &mut out),
                SheetOp::Fold { sketch, line, .. } => {
                    if let Some(l) = sketch_line(part, sketch, line) {
                        out.push(l);
                    }
                }
                SheetOp::Contour { sketch, .. } => {
                    if let Some(ps) = part.sketches.iter().find(|s| s.id == *sketch) {
                        out.extend(sketch_lines(ps));
                    }
                }
                SheetOp::CornerSeam { a, b, .. } if !a.is_empty() => {
                    if let Some(c) = part
                        .sheet_corners()
                        .into_iter()
                        .find(|c| c.a == *a && c.b == *b)
                    {
                        out.push(vec![c.at, c.at + c.up * 15.0]);
                    }
                }
                _ => {}
            },
            _ => {}
        }
        out
    }

    /// What a click at `p` would pick for the active selector, with its outline.
    fn sm_hover(&self, p: Pos2, rect: Rect) -> Option<(Pick, Vec<Vec<Point3>>)> {
        use super::overlay;
        let d = self.sm.draft.as_ref()?;
        let flow = d.flow();
        let sel = flow.current()?;
        let part = self.part();
        let (o, dir) = self.camera.ray(p, rect)?;
        if sel.takes(PickKind::Region) {
            for (i, ps) in part.sketches.iter().enumerate() {
                if !(ps.visible || d.sketch == Some(i)) {
                    continue;
                }
                let sk = &ps.sketch;
                let Some(world) = self.camera.ray_plane(p, rect, &sk.plane) else {
                    continue;
                };
                let uv = sk.plane.project(&world);
                let regions = self.sketch_regions(sk);
                if let Some(r) = regions
                    .iter()
                    .filter(|r| r.contains(uv))
                    .min_by(|a, b| a.area().total_cmp(&b.area()))
                {
                    let mut pts: Vec<Point3> = r
                        .outer
                        .polygon(1e-2)
                        .iter()
                        .map(|q| sk.plane.point_at(q.x, q.y))
                        .collect();
                    if let Some(f) = pts.first().copied() {
                        pts.push(f);
                    }
                    return Some((
                        Pick::Region(i, ProfileRef::of(r, r.interior_point())),
                        vec![pts],
                    ));
                }
            }
            return None;
        }
        if sel.takes(PickKind::SketchCurve) {
            for (i, ps) in part.sketches.iter().enumerate() {
                if !(ps.visible || d.sketch == Some(i)) {
                    continue;
                }
                if let Some((e, _)) =
                    overlay::pick_entity(&self.camera, rect, &ps.sketch, p, overlay::CURVE_SNAP_PX)
                    && let Some(crate::sketch::Entity::Line { .. }) = ps.sketch.entity(e)
                {
                    let key = crate::part::extrude::entity_key(e);
                    let lines = if d.active == "line" {
                        sketch_line(part, &ps.id, &key).into_iter().collect()
                    } else {
                        sketch_lines(ps)
                    };
                    return Some((Pick::Line(i, key), lines));
                }
            }
            return None;
        }
        if sel.takes(PickKind::PlanarFace) {
            let (f, t) = self.face_hit(&o, &dir)?;
            let name = part.body.face(f)?.name.clone()?;
            let hit = o + dir.normalize() * t;
            let opts = crate::kernel::TessellationOptions::default();
            let lines = crate::part::select::edges_of_face(&part.body, f)
                .into_iter()
                .filter_map(|e| part.body.sample_edge(e, &opts).ok())
                .collect();
            return Some((Pick::Face(name, hit), lines));
        }
        if sel.takes(PickKind::Edge) {
            let e = self.edge_under(p, rect)?;
            let opts = crate::kernel::TessellationOptions::default();
            let pts = part.body.sample_edge(e, &opts).ok()?;
            let at = pts
                .windows(2)
                .map(|w| super::closest_on_segment_pub(&w[0], &w[1], &o, &dir))
                .min_by(|a, b| {
                    let da = (a - o).cross(&dir).norm();
                    let db = (b - o).cross(&dir).norm();
                    da.total_cmp(&db)
                })?;
            return match &d.feature {
                Feature::SheetConvert(_) => {
                    let r = crate::part::blend::edge_ref(&part.body, e)?;
                    Some((Pick::Bend(r), vec![pts]))
                }
                Feature::SheetOp(op) if matches!(op.op, SheetOp::CornerSeam { .. }) => {
                    let c = part.sheet_corner_near(at, 1e3)?;
                    let line = vec![c.at, c.at + c.up * 15.0];
                    Some((Pick::Corner(c), vec![pts, line]))
                }
                _ => {
                    let (body, edge) = part.sheet_edge_at(at)?;
                    let line = part
                        .sheet_edge_ends(&body, &edge)
                        .map_or(pts, |(a, b)| vec![a, b]);
                    Some((Pick::SheetEdge(body, edge), vec![line]))
                }
            };
        }
        None
    }

    /// Applies a pick to the active selector.
    fn sm_pick(&mut self, pick: Pick) {
        let fold_body = match &pick {
            Pick::Line(i, key) => self
                .part()
                .sketches
                .get(*i)
                .and_then(|ps| sketch_line(self.part(), &ps.id, key))
                .and_then(|l| {
                    let mid = Point3::from((l[0].coords + l[1].coords) * 0.5);
                    self.part().sheet_body_at(mid)
                }),
            _ => None,
        };
        let sid = match &pick {
            Pick::Region(i, _) | Pick::Line(i, _) => {
                self.part().sketches.get(*i).map(|p| p.id.clone())
            }
            _ => None,
        };
        let Some(d) = self.sm.draft.as_mut() else {
            return;
        };
        let mut status = None;
        match (&mut d.feature, pick) {
            (Feature::SheetFace(f), Pick::Region(i, pr)) => {
                f.sketch = sid.unwrap_or_default();
                f.profiles = vec![pr];
                d.sketch = Some(i);
            }
            (Feature::SheetFlange(f), Pick::SheetEdge(body, e)) => {
                toggle_edge(&mut f.body, &mut f.edges, body, e, &mut status);
            }
            (Feature::SheetConvert(c), Pick::Face(name, hit)) => {
                c.face = crate::part::shell::FaceRef {
                    face: name,
                    hint: [hit.x, hit.y, hit.z],
                };
            }
            (Feature::SheetConvert(c), Pick::Bend(r)) => {
                match c.bends.iter().position(|b| b.faces == r.faces) {
                    Some(k) => {
                        c.bends.remove(k);
                    }
                    None => c.bends.push(r),
                }
            }
            (Feature::SheetOp(o), pick) => match (&mut o.op, pick) {
                (SheetOp::Hem { body, edges, .. }, Pick::SheetEdge(b, e)) => {
                    toggle_edge(body, edges, b, e, &mut status);
                }
                (
                    SheetOp::Fold {
                        body, sketch, line, ..
                    },
                    Pick::Line(i, key),
                ) => {
                    *sketch = sid.unwrap_or_default();
                    *line = key;
                    d.sketch = Some(i);
                    match fold_body {
                        Some(b) => *body = b,
                        None => {
                            status =
                                Some("Fold: the line must lie on a face of the sheet".to_owned());
                        }
                    }
                }
                (SheetOp::Contour { sketch, .. }, Pick::Line(i, _)) => {
                    *sketch = sid.unwrap_or_default();
                    d.sketch = Some(i);
                }
                (SheetOp::Cut { sketch, profiles }, Pick::Region(i, pr)) => {
                    let id = sid.unwrap_or_default();
                    if *sketch != id {
                        *sketch = id;
                        profiles.clear();
                    }
                    match profiles.iter().position(|p| p.sources == pr.sources) {
                        Some(k) => {
                            profiles.remove(k);
                        }
                        None => profiles.push(pr),
                    }
                    d.sketch = Some(i);
                }
                (SheetOp::CornerSeam { body, a, b, .. }, Pick::Corner(c)) => {
                    *body = c.body;
                    *a = c.a;
                    *b = c.b;
                }
                _ => {}
            },
            _ => {}
        }
        // A single pick moves on.
        let mut flow = d.flow();
        if flow.picked()
            && let Some(k) = flow.active_key()
        {
            d.active = k;
        }
        if let Some(s) = status {
            self.file_status = Some(Err(s));
        }
    }

    // ---- manipulators ------------------------------------------------------

    /// Drag arrows: Flange height and width offsets, Hem length, Contour
    /// Flange distance.
    pub(super) fn sm_handles(&self) -> Vec<super::manip::Handle> {
        use super::manip::{DragGeom, Handle};
        let Some(d) = &self.sm.draft else {
            return Vec::new();
        };
        let part = self.part();
        let arrow = |field: &'static str, origin: Point3, dir: Vector3, len: f64| {
            let tip = origin + dir * len;
            Handle {
                field,
                path: vec![origin, tip],
                grip: tip,
                geom: DragGeom::Linear {
                    origin,
                    dir,
                    scale: 1.0,
                },
                arrow: Some(dir),
                angle: false,
                signed: false,
            }
        };
        let rule = rule_of(part).values();
        match &d.feature {
            Feature::SheetFlange(f) => {
                let Some(e) = f.edges.first() else {
                    return Vec::new();
                };
                let Some((a, b, n, out)) = part.sheet_edge_frame(&f.body, e) else {
                    return Vec::new();
                };
                let len = (b - a).norm();
                if len < 1e-6 {
                    return Vec::new();
                }
                let dd = (b - a) / len;
                let (o1, o2) = f.spans(len).unwrap_or((f.offset1, f.offset2));
                let reach = (len * 0.08).clamp(3.0, 20.0);
                let side = |field: &'static str, origin: Point3, dir: Vector3, off: f64| {
                    let grip = origin + dir * off;
                    Handle {
                        field,
                        path: vec![grip - dir * reach, grip],
                        grip,
                        geom: DragGeom::Linear {
                            origin,
                            dir,
                            scale: 1.0,
                        },
                        arrow: Some(-dir),
                        angle: false,
                        signed: false,
                    }
                };
                // The flange rises along out·cos θ + up·sin θ from the
                // middle of what it covers.
                let up = if f.flip { -n } else { n };
                let rise = (out * f.angle.cos() + up * f.angle.sin()).normalize();
                let mid = a + dd * (o1 + (len - o1 - o2) * 0.5);
                vec![
                    arrow("height", mid, rise, f.height),
                    side("offset1", a, dd, o1),
                    side("offset2", b, -dd, o2),
                ]
            }
            Feature::SheetOp(o) => match &o.op {
                SheetOp::Hem {
                    body,
                    edges,
                    length,
                    gap,
                    flip,
                } => {
                    let Some(e) = edges.first() else {
                        return Vec::new();
                    };
                    let Some((a, b, n, out)) = part.sheet_edge_frame(body, e) else {
                        return Vec::new();
                    };
                    // The hem folds back over the sheet on the other face.
                    let g = gap.unwrap_or(rule.thickness * 0.5);
                    let back = if *flip { n } else { -n };
                    let mid = Point3::from((a.coords + b.coords) * 0.5)
                        + out * (g + rule.thickness)
                        + back * (g + rule.thickness);
                    vec![arrow("length", mid, -out, *length)]
                }
                SheetOp::Contour { sketch, width, .. } => {
                    let Some(ps) = part.sketches.iter().find(|s| s.id == *sketch) else {
                        return Vec::new();
                    };
                    let Some(first) = sketch_lines(ps).into_iter().next() else {
                        return Vec::new();
                    };
                    let n = ps.sketch.plane.normal.into_inner();
                    vec![arrow("width", first[0], n, *width)]
                }
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// An arrow was dragged to `v`.
    pub(super) fn sm_manip(&mut self, field: &str, v: f64) {
        if matches!(field, "offset1" | "offset2") {
            let ends = match self.sm.draft.as_ref().map(|d| &d.feature) {
                Some(Feature::SheetFlange(f)) => f
                    .edges
                    .first()
                    .and_then(|e| self.part().sheet_edge_ends(&f.body, e)),
                _ => None,
            };
            let Some((a, b)) = ends else {
                return;
            };
            let len = (b - a).norm();
            if let Some(d) = self.sm.draft.as_mut()
                && let Feature::SheetFlange(f) = &mut d.feature
            {
                let (o1, o2) = f.spans(len).unwrap_or((0.0, 0.0));
                // Dragged ends: the flange is Offset-wide from now on.
                f.extents = FlangeWidth::Offset;
                (f.offset1, f.offset2) = (o1, o2);
                let room = |other: f64| (len - other - 1.0).max(0.0);
                if field == "offset1" {
                    f.offset1 = v.clamp(0.0, room(f.offset2));
                } else {
                    f.offset2 = v.clamp(0.0, room(f.offset1));
                }
                for k in ["offset1", "offset2"] {
                    if let Some(p) = f.params.0.get_mut(k) {
                        p.expr = None;
                    }
                }
            }
            return;
        }
        if v <= 1e-6 {
            return;
        }
        if let Some(d) = self.sm.draft.as_mut() {
            crate::part::params::set_feature_field_value(&mut d.feature, field, v);
        }
    }

    // ---- Sheet Metal Defaults, Flat Pattern --------------------------------

    /// Sheet Metal Defaults window (Inventor's dialog: the rule's values).
    pub(super) fn sm_defaults_window(&mut self, ctx: &egui::Context) {
        let Some(mut r) = self.sm.defaults.clone() else {
            return;
        };
        let mut open = true;
        let mut apply = false;
        let win = egui::Window::new("Sheet Metal Defaults")
            .open(&mut open)
            .resizable(false)
            .collapsible(false);
        self.layout
            .place("sm_defaults", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                egui::Grid::new("sm_rule")
                    .num_columns(2)
                    .min_col_width(super::panels::label_width(ui.ctx()))
                    .show(ui, |ui| {
                        plabel(ui, "Thickness");
                        ui.add(ExprValue::new(&mut r.thickness).suffix(" mm"));
                        ui.end_row();
                        plabel(ui, "Unfold Rule");
                        ui.label("Linear (K-factor)")
                            .on_hover_text("BA = angle × (R + K × T); bend tables are planned");
                        ui.end_row();
                        plabel(ui, "K-factor");
                        ui.add(ExprValue::new(&mut r.k_factor).range(0.0..=1.0));
                        ui.end_row();
                        let t = r.thickness;
                        for (label, slot, def) in [
                            ("Bend Radius", &mut r.bend_radius, t),
                            ("Relief Width", &mut r.relief_width, t),
                            ("Relief Depth", &mut r.relief_depth, t * 0.5),
                            ("Corner Relief Size", &mut r.corner_relief, t * 4.0),
                            ("Gap", &mut r.gap, t),
                        ] {
                            plabel(ui, label);
                            ui.horizontal(|ui| {
                                let mut own = slot.is_some();
                                if ui
                                    .checkbox(&mut own, "")
                                    .on_hover_text("Off: follows the thickness")
                                    .changed()
                                {
                                    *slot = own.then_some(def);
                                }
                                match slot.as_mut() {
                                    Some(v) => {
                                        ui.add(ExprValue::new(v).suffix(" mm"));
                                    }
                                    None => {
                                        ui.weak(format!("{def:.3} mm (from Thickness)"));
                                    }
                                }
                            });
                            ui.end_row();
                        }
                        plabel(ui, "Corner Relief Shape");
                        egui::ComboBox::from_id_salt("sm_corner")
                            .selected_text(r.corner_shape.label())
                            .show_ui(ui, |ui| {
                                for c in CornerRelief::ALL {
                                    ui.selectable_value(&mut r.corner_shape, c, c.label());
                                }
                            });
                        ui.end_row();
                    });
                if let Err(e) = r.validate() {
                    ui.colored_label(Color32::from_rgb(235, 80, 70), e);
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(r.validate().is_ok(), egui::Button::new("OK"))
                        .clicked()
                    {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        self.sm.defaults = None;
                    }
                });
            });
        if apply {
            self.checkpoint("Sheet Metal Defaults");
            self.part_mut().sheet_metal = Some(r.clone());
            if let Err(e) = self.part_mut().regenerate() {
                self.file_status = Some(Err(e.to_string()));
            }
            self.scene_dirty = true;
            self.sm.defaults = None;
            return;
        }
        if !open {
            self.sm.defaults = None;
        } else if self.sm.defaults.is_some() {
            self.sm.defaults = Some(r);
        }
    }

    /// The flat patterns for the view (when Flat Pattern is on): a part of
    /// their solids.
    pub(super) fn sm_flat_part(&self) -> Option<PartDocument> {
        if !self.sm.flat || !self.is_sheet_metal() {
            return None;
        }
        let flats = self.part().flat_patterns();
        if flats.is_empty() {
            return None;
        }
        let solids: Vec<_> = flats.into_iter().map(|f| f.solid).collect();
        let body = crate::kernel::exact::to_brep(&solids).ok()?;
        Some(PartDocument {
            body,
            material: self.part().material.clone(),
            ..Default::default()
        })
    }

    /// Flat Pattern facts in Properties.
    pub(super) fn sm_flat_info(&self, ui: &mut egui::Ui) {
        for f in self.part().flat_patterns() {
            ui.label(RichText::new(format!("Flat Pattern of {}", f.body)).strong());
            ui.label(format!(
                "Extents {:.2} × {:.2} mm",
                f.size.0.max(f.size.1),
                f.size.0.min(f.size.1)
            ));
            ui.label(format!("{} bend(s)", f.bends.len()));
            for (k, b) in f.bends.iter().enumerate() {
                ui.weak(format!(
                    "Bend {}: {:.1}° {} R {:.2}, allowance {:.3} mm",
                    k + 1,
                    b.angle_deg,
                    if b.up { "up" } else { "down" },
                    b.radius,
                    b.allowance
                ));
            }
        }
    }
}

/// Default / Flipped as two toggles (Inventor's direction buttons).
fn direction_row(ui: &mut egui::Ui, flip: &mut bool, tip: &str) {
    ui.horizontal(|ui| {
        ui.selectable_value(flip, false, "Default");
        ui.selectable_value(flip, true, "Flipped")
            .on_hover_text(tip);
    });
}

/// Bend Radius: the rule's, or an own value.
fn radius_row(ui: &mut egui::Ui, radius: &mut Option<f64>, rule: f64) {
    plabel(ui, "Bend Radius");
    ui.horizontal(|ui| {
        let mut own = radius.is_some();
        if ui
            .checkbox(&mut own, "")
            .on_hover_text("Off: the rule's radius")
            .changed()
        {
            *radius = own.then_some(rule);
        }
        match radius.as_mut() {
            Some(r) => {
                ui.add(ExprValue::new(r).suffix(" mm"));
            }
            None => {
                ui.weak(format!("{rule:.3} mm (rule)"));
            }
        }
    });
    ui.end_row();
}

/// Adds or removes a sheet edge (all on one sheet).
fn toggle_edge(
    sheet: &mut String,
    edges: &mut Vec<SheetEdge>,
    body: String,
    e: SheetEdge,
    status: &mut Option<String>,
) {
    if sheet.is_empty() || edges.is_empty() {
        *sheet = body;
    } else if *sheet != body {
        *status = Some("Sheet metal: the edges must be on one sheet".into());
        return;
    }
    match edges.iter().position(|x| x.name == e.name) {
        Some(k) => {
            edges.remove(k);
        }
        None => edges.push(e),
    }
}

/// The world line of sketch line `key` of sketch `sketch`.
fn sketch_line(part: &PartDocument, sketch: &str, key: &str) -> Option<Vec<Point3>> {
    let ps = part.sketches.iter().find(|s| s.id == sketch)?;
    let sk = &ps.sketch;
    sk.entities().find_map(|(id, e)| match e {
        crate::sketch::Entity::Line { start, end }
            if crate::part::extrude::entity_key(id) == key =>
        {
            let (a, b) = (sk.point(*start)?.position, sk.point(*end)?.position);
            Some(vec![
                sk.plane.point_at(a.x, a.y),
                sk.plane.point_at(b.x, b.y),
            ])
        }
        _ => None,
    })
}

/// Every line of a sketch in world coordinates (a Contour Flange's profile).
fn sketch_lines(ps: &crate::part::PartSketch) -> Vec<Vec<Point3>> {
    let sk = &ps.sketch;
    sk.entities()
        .filter_map(|(id, e)| match e {
            crate::sketch::Entity::Line { start, end }
                if sk.style(id) == crate::sketch::Style::Normal && !ps.is_reference(id) =>
            {
                let (a, b) = (sk.point(*start)?.position, sk.point(*end)?.position);
                Some(vec![
                    sk.plane.point_at(a.x, a.y),
                    sk.plane.point_at(b.x, b.y),
                ])
            }
            _ => None,
        })
        .collect()
}

/// Clears the selector `key` of a draft.
fn sm_clear(d: &mut SmDraft, key: &str) {
    match (&mut d.feature, key) {
        (Feature::SheetFace(f), "profile") => f.profiles.clear(),
        (Feature::SheetFlange(f), "edges") => f.edges.clear(),
        (Feature::SheetConvert(c), "face") => c.face.face.clear(),
        (Feature::SheetConvert(c), "bends") => c.bends.clear(),
        (Feature::SheetOp(_), _) => sm_clear_inputs(&mut d.feature),
        _ => {}
    }
}

/// The inputs of a feature cleared (Apply starts the next one).
fn sm_clear_inputs(f: &mut Feature) {
    match f {
        Feature::SheetFace(f) => f.profiles.clear(),
        Feature::SheetFlange(f) => f.edges.clear(),
        Feature::SheetConvert(c) => {
            c.face.face.clear();
            c.bends.clear();
        }
        Feature::SheetOp(o) => match &mut o.op {
            SheetOp::Hem { edges, .. } => edges.clear(),
            SheetOp::Fold { line, .. } => line.clear(),
            SheetOp::Contour { sketch, .. } => sketch.clear(),
            SheetOp::Cut { profiles, .. } => profiles.clear(),
            SheetOp::CornerSeam { a, b, .. } => {
                a.clear();
                b.clear();
            }
            _ => {}
        },
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::{OriginPlane, SketchOptions, SketchPlacement};
    use crate::sketch::Point2;

    /// A 100 × 60 Face on XY, 2 mm.
    fn plate() -> PartDocument {
        let mut p = PartDocument::default();
        p.make_sheet_metal(None);
        let o = SketchOptions {
            auto_project_origin: false,
            ..Default::default()
        };
        let i = p
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
            .unwrap();
        p.sketches[i]
            .sketch
            .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(100.0, 60.0))
            .unwrap();
        let r = p.sketches[i].sketch.regions();
        let prof = vec![ProfileRef::of(&r[0], r[0].interior_point())];
        // Through the panel's own path: draft, flow, build.
        let d = SmDraft {
            feature: Feature::SheetFace(SheetFaceFeature {
                id: String::new(),
                name: String::new(),
                sketch: p.sketches[i].id.clone(),
                profiles: prof,
                flip: false,
            }),
            sketch: Some(i),
            active: "profile",
            edit: None,
            crumb: Default::default(),
        };
        assert!(d.flow().ready());
        let before = HashSet::new();
        let id = build(&mut p, d.feature, None, d.sketch).unwrap();
        let pv = new_faces(&p, &before, &id);
        assert!(
            !pv.tris.is_empty() && !pv.edges.is_empty(),
            "a Face previews"
        );
        p
    }

    fn names(p: &PartDocument) -> HashSet<String> {
        p.body.faces().filter_map(|(_, f)| f.name.clone()).collect()
    }

    /// Every command through its draft: the flow asks for its inputs, then
    /// `build` makes the feature and the preview shows new faces only.
    #[test]
    fn every_command_builds_and_previews() {
        let base = plate();
        let body = base.sheet_models.keys().next().unwrap().clone();
        let (_, edge) = base.sheet_edge_at(Point3::new(50.0, 0.0, 2.0)).unwrap();
        // Flange: needs edges; previews only its own faces.
        let mut f = SheetFlangeFeature::new("", Vec::new(), 20.0);
        let mut d = SmDraft::new(Feature::SheetFlange(f.clone()), "edges");
        assert!(!d.flow().ready());
        let mut status = None;
        toggle_edge(
            &mut f.body,
            &mut f.edges,
            body.clone(),
            edge.clone(),
            &mut status,
        );
        assert_eq!(f.edges.len(), 1);
        d.feature = Feature::SheetFlange(f.clone());
        assert!(d.flow().ready());
        let mut p = base.clone();
        let before = names(&p);
        let id = build(&mut p, d.feature.clone(), None, None).unwrap();
        let pv = new_faces(&p, &before, &id);
        let all: usize = p
            .body
            .faces()
            .filter_map(|(_, f)| f.mesh.as_ref())
            .map(|m| m.triangles.len())
            .sum();
        assert!(
            !pv.tris.is_empty() && pv.tris.len() < all,
            "only the new faces"
        );
        // Edit Feature: the same draft with a new height replaces it.
        let k = p.features.iter().position(|x| x.id() == id).unwrap();
        let mut f2 = f.clone();
        f2.height = 35.0;
        let mut q = p.clone();
        q.rollback = Some(k);
        q.regenerate().unwrap();
        build(&mut q, Feature::SheetFlange(f2), Some(k), None).unwrap();
        q.rollback = None;
        q.regenerate().unwrap();
        let (lo, hi) = q.body.bounding_box().unwrap();
        assert!((hi.z - lo.z - 35.0).abs() < 0.5, "edited height: {lo} {hi}");
        assert_eq!(q.features.len(), p.features.len());
        // A second click on the same edge removes it.
        toggle_edge(
            &mut f.body,
            &mut f.edges,
            body.clone(),
            edge.clone(),
            &mut status,
        );
        assert!(f.edges.is_empty());
        // Hem, Corner Seam (on a tray), Unfold / Refold.
        let hem = SheetOp::Hem {
            body: body.clone(),
            edges: vec![edge.clone()],
            length: 8.0,
            gap: None,
            flip: false,
        };
        let mut p = base.clone();
        let before = names(&p);
        let id = build(
            &mut p,
            Feature::SheetOp(SheetOpFeature::new(hem)),
            None,
            None,
        )
        .unwrap();
        assert!(!new_faces(&p, &before, &id).tris.is_empty());
        let mut p = base.clone();
        let id = build(
            &mut p,
            Feature::SheetOp(SheetOpFeature::new(SheetOp::Unfold { body: body.clone() })),
            None,
            None,
        )
        .unwrap();
        assert!(p.sheet_is_flat(&body), "{id}");
        build(
            &mut p,
            Feature::SheetOp(SheetOpFeature::new(SheetOp::Refold { body: body.clone() })),
            None,
            None,
        )
        .unwrap();
        assert!(!p.sheet_is_flat(&body));
    }

    /// Convert: the base face is a single pick that moves on to Bends.
    #[test]
    fn convert_flow_moves_on_after_the_face() {
        let c = SheetConvertFeature {
            id: String::new(),
            name: String::new(),
            body: "Box1".into(),
            face: crate::part::shell::FaceRef {
                face: String::new(),
                hint: [0.0; 3],
            },
            bends: Vec::new(),
        };
        let mut d = SmDraft::new(Feature::SheetConvert(c), "face");
        assert_eq!(d.flow().active_key(), Some("face"));
        if let Feature::SheetConvert(c) = &mut d.feature {
            c.face.face = "Box1:Top".into();
        }
        let mut flow = d.flow();
        assert!(flow.picked());
        assert_eq!(flow.active_key(), Some("bends"));
        assert!(!flow.ready(), "bends are required");
        sm_clear(&mut d, "face");
        assert!(matches!(&d.feature, Feature::SheetConvert(c) if c.face.face.is_empty()));
    }

    /// A command without inputs (Unfold) is ready at once; Apply clears the
    /// inputs of the next one.
    #[test]
    fn unfold_is_ready_and_apply_clears_inputs() {
        let d = SmDraft::new(
            Feature::SheetOp(SheetOpFeature::new(SheetOp::Unfold {
                body: "Face1".into(),
            })),
            "",
        );
        assert!(d.flow().ready() && d.flow().selectors.is_empty());
        let mut f = Feature::SheetOp(SheetOpFeature::new(SheetOp::Cut {
            sketch: "Sketch2".into(),
            profiles: vec![ProfileRef {
                sources: Vec::new(),
                seed: Point2::new(0.0, 0.0),
            }],
        }));
        sm_clear_inputs(&mut f);
        assert!(
            matches!(&f, Feature::SheetOp(o) if matches!(&o.op, SheetOp::Cut { profiles, .. } if profiles.is_empty()))
        );
    }
}
