//! The Hole and Thread commands (`docs/spec/2.3-hole.md`,
//! `docs/spec/2.9-thread.md`).
//!
//! Hole (Inventor 2027 placement): a click on a planar face places a centre
//! there (Allow Center Point Creation; the feature's sketch is made on that
//! face at OK with Center Points; further clicks add centres on the same
//! face, a click on a centre removes it); a click on a straight model edge
//! then dimensions the last centre to it (a projected edge and a
//! point-to-line dimension in the hole's sketch); a click on a point of a
//! visible sketch uses that sketch's points (its unused Center Points are
//! preselected); a click on a circular edge or a cylindrical face places a
//! concentric centre. Thread: a click on a cylindrical face, near the end
//! where the thread starts.
//!
//! The dialog's state is the feature definition itself (`HoleSpec` /
//! `ThreadSpec`, both serde types): presets can store and restore it.

use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{self, Color32, Rect, Stroke};

use super::CadCraftApp;
use crate::kernel::{EdgeId, FaceId, Point3, Real};
use crate::part::hole::{self, HoleCenter};
use crate::part::standards::Standards;
use crate::part::{Feature, HoleFeature, HoleSpec, PartDocument, ThreadFeature, ThreadSpec};
use crate::sketch::{Constraint, PointId};

/// Allow Center Point Creation (remembered for the session, Inventor).
pub(super) static ALLOW_CREATE: AtomicBool = AtomicBool::new(true);
/// Panel menu: Preselect sketch center points.
pub(super) static PRESELECT: AtomicBool = AtomicBool::new(true);
/// Panel menu: Keep sketch visible on (+).
pub(super) static KEEP_VISIBLE: AtomicBool = AtomicBool::new(false);
/// Panel menu: Window select center points only.
pub(super) static WINDOW_CENTERS: AtomicBool = AtomicBool::new(true);

/// Reads one of the option flags.
pub(super) fn option(flag: &AtomicBool) -> bool {
    flag.load(Ordering::Relaxed)
}

/// Sets one of the option flags.
pub(super) fn set_option(flag: &AtomicBool, on: bool) {
    flag.store(on, Ordering::Relaxed);
}

/// What a click in the view means for Hole.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum HolePick {
    /// A point of a visible sketch (index, point).
    SketchPoint(usize, PointId),
    /// A circular edge: a concentric centre.
    Circle(crate::part::EdgeRef),
    /// A cylindrical face: a centre on its axis.
    Cylinder(crate::part::FaceRef),
    /// A straight model edge: a dimension from the last centre to it
    /// (names of its end vertices, its ends).
    Edge(String, String, [Point3; 2]),
    /// A planar face (name) at a model point.
    Face(String, Point3),
}

/// A linear dimension from a clicked centre to a model edge (Inventor:
/// click a reference edge after the centre).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct HoleDim {
    /// The centre (index into [`HoleDraft::points`]).
    pub center: usize,
    /// Names of the edge's end vertices.
    pub a: String,
    /// See `a`.
    pub b: String,
    /// The edge's ends (model coordinates, for the display).
    pub ends: [Point3; 2],
    /// The distance (mm).
    pub value: Real,
    /// Its equation, if typed as one.
    pub expr: Option<String>,
}

impl HoleDim {
    /// The foot of the perpendicular from `p` on the edge's line.
    pub(super) fn foot(&self, p: Point3) -> Point3 {
        let [a, b] = self.ends;
        let d = b - a;
        let t = (p - a).dot(&d) / d.norm_squared().max(1e-300);
        a + d * t
    }
}

/// The open Hole command.
#[derive(Clone, Debug)]
pub(super) struct HoleDraft {
    /// The definition (its `spec` is the dialog state; presets hook in
    /// here). Its centres: points of an existing sketch and concentric
    /// ones; centres clicked on a face are in `points`.
    pub def: HoleFeature,
    /// The face the clicked centres lie on.
    pub face: Option<String>,
    /// Centres clicked on `face` (model coordinates).
    pub points: Vec<Point3>,
    /// Dimensions of the clicked centres to model edges.
    pub dims: Vec<HoleDim>,
    /// Edit Feature of this feature index.
    pub edit: Option<usize>,
    /// The next face click sets the termination face (To).
    pub to_pick: bool,
    /// A sketch shared for this hole by Keep sketch visible on (+): Cancel
    /// unshares it (sketch id).
    pub kept: Option<String>,
}

impl HoleDraft {
    /// Inventor's defaults: Simple, Ø6 × 10, 118° drill point.
    pub(super) fn new() -> Self {
        Self {
            def: HoleFeature::new(HoleSpec::default()),
            face: None,
            points: Vec::new(),
            dims: Vec::new(),
            edit: None,
            to_pick: false,
            kept: None,
        }
    }

    pub(super) fn editing(def: HoleFeature, k: usize) -> Self {
        Self {
            def,
            edit: Some(k),
            ..Self::new()
        }
    }

    /// How many centres.
    pub(super) fn count(&self) -> usize {
        self.def.centers.len() + self.points.len()
    }

    /// Moves clicked centre `k` so that its dimension `j` reads `value`.
    pub(super) fn set_dim(&mut self, j: usize, value: Real, expr: Option<String>) {
        let Some(d) = self.dims.get_mut(j) else {
            return;
        };
        let Some(p) = self.points.get_mut(d.center) else {
            return;
        };
        let f = d.foot(*p);
        if let Some(u) = (*p - f).try_normalize(1e-12) {
            *p = f + u * value;
        }
        d.value = value;
        d.expr = expr;
    }

    /// The feature as it will be added to `part`: its face centres become
    /// Center Points of a sketch on the face, their edge dimensions
    /// projected edges and point-to-line dimensions of that sketch.
    pub(super) fn build(&self, part: &mut PartDocument) -> Result<HoleFeature, String> {
        let mut def = self.def.clone();
        if let (Some(face), false) = (&self.face, self.points.is_empty()) {
            let (i, ids) = part
                .create_hole_sketch(face, &self.points)
                .map_err(|e| e.to_string())?;
            for d in &self.dims {
                let Some(&point) = ids.get(d.center) else {
                    continue;
                };
                let line = part
                    .project_segment(i, &d.a, &d.b)
                    .map_err(|e| e.to_string())?;
                let ps = &mut part.sketches[i];
                let c = ps
                    .sketch
                    .add_constraint(Constraint::PointLineDistance {
                        point,
                        line,
                        value: d.value,
                    })
                    .map_err(|e| e.to_string())?;
                let plane = ps.sketch.plane.clone();
                let p = self.points[d.center];
                let mid = Point3::from((p.coords + d.foot(p).coords) / 2.0);
                ps.sketch.set_dimension_label(c, Some(plane.project(&mid)));
                if let Some(dp) = ps.sketch.dim_param_mut(c) {
                    dp.expr = d.expr.clone();
                }
            }
            let _ = part.sketches[i].sketch.solve();
            def.sketch = part.sketches[i].id.clone();
            let mut centers: Vec<HoleCenter> = ids.into_iter().map(HoleCenter::Point).collect();
            centers.extend(
                def.centers
                    .iter()
                    .filter(|c| !matches!(c, HoleCenter::Point(_)))
                    .cloned(),
            );
            def.centers = centers;
        }
        Ok(def)
    }
}
// The open Thread command.
#[derive(Clone, Debug)]
pub(super) struct ThreadDraft {
    /// The face picked (none yet: the definition is not complete).
    pub face: Option<crate::part::FaceRef>,
    /// The thread (dialog state, a serde type).
    pub spec: ThreadSpec,
    /// Offset from the start end (when not full length).
    pub offset: Real,
    /// Display thread in model.
    pub display: bool,
    /// Edit Feature of this feature index.
    pub edit: Option<usize>,
}

impl ThreadDraft {
    pub(super) fn new() -> Self {
        let mut spec = ThreadSpec::default_metric();
        spec.depth = 10.0;
        Self {
            face: None,
            spec,
            offset: 0.0,
            display: true,
            edit: None,
        }
    }

    pub(super) fn editing(t: ThreadFeature, k: usize) -> Self {
        Self {
            face: Some(t.face),
            spec: t.spec,
            offset: t.offset,
            display: t.display,
            edit: Some(k),
        }
    }

    /// The feature (when a face is picked).
    pub(super) fn def(&self) -> Option<ThreadFeature> {
        let mut t = ThreadFeature::new(self.face.clone()?, self.spec.clone());
        t.offset = self.offset;
        t.display = self.display;
        Some(t)
    }
}

/// The thread size of `table` that fits a cylinder of `diameter`: a shaft
/// by its major diameter, a hole by its minor diameter or tap drill.
/// The visible sketch (the newest) with Center Points that no Hole uses
/// yet, and those points (Inventor: Preselect sketch center points).
pub(super) fn unused_center_points(part: &PartDocument) -> Option<(String, Vec<PointId>)> {
    part.sketches
        .iter()
        .rev()
        .filter(|ps| ps.visible)
        .find_map(|ps| {
            let used: Vec<PointId> = part
                .features
                .iter()
                .filter_map(|f| match f {
                    Feature::Hole(h) if h.sketch == ps.id => Some(h.centers.iter()),
                    _ => None,
                })
                .flatten()
                .filter_map(|c| match c {
                    HoleCenter::Point(p) => Some(*p),
                    _ => None,
                })
                .collect();
            let free: Vec<PointId> = ps
                .sketch
                .center_points()
                .into_iter()
                .filter(|p| !used.contains(p) && !ps.is_projected(*p))
                .collect();
            (!free.is_empty()).then(|| (ps.id.clone(), free))
        })
}

pub(super) fn fitting_thread(
    tables: &Standards,
    table: &str,
    diameter: Real,
    internal: bool,
) -> Option<ThreadSpec> {
    let t = tables.thread_table(table)?;
    let score = |r: &crate::part::standards::ThreadRow| {
        if internal {
            (r.minor - diameter)
                .abs()
                .min((r.tap_drill - diameter).abs())
        } else {
            (r.major - diameter).abs()
        }
    };
    // Coarse rows first among equals (file order).
    let best = t.rows.iter().min_by(|a, b| score(a).total_cmp(&score(b)))?;
    ThreadSpec::from_table(tables, table, &best.designation, internal)
}

impl CadCraftApp {
    /// Starts Hole; a face selected beforehand is where the centres go.
    /// With Preselect sketch center points, the unused Center Points of a
    /// visible sketch become the centres (Inventor).
    pub(super) fn start_hole(&mut self) {
        self.finish_sketch();
        self.mode = super::Mode::Normal;
        self.cancel_extrude();
        self.work_draft = None;
        self.blend_draft = None;
        let mut d = HoleDraft::new();
        if let [super::select::SelItem::Face(name)] = self.sel.as_slice() {
            d.face = Some(name.clone());
        } else if option(&PRESELECT)
            && let Some((id, pts)) = unused_center_points(self.part())
        {
            d.def.sketch = id;
            d.def.centers = pts.into_iter().map(HoleCenter::Point).collect();
        }
        self.sel.clear();
        self.face_sel = None;
        self.edge_sel = None;
        self.hole_draft = Some(d);
    }

    /// Starts Thread.
    pub(super) fn start_thread(&mut self) {
        self.finish_sketch();
        self.mode = super::Mode::Normal;
        self.cancel_extrude();
        self.work_draft = None;
        self.blend_draft = None;
        self.sel.clear();
        self.face_sel = None;
        self.edge_sel = None;
        self.thread_draft = Some(ThreadDraft::new());
    }

    /// What Hole would take under the cursor: a sketch point, an edge
    /// (circular: concentric; straight, after a clicked centre: a
    /// dimension), a cylindrical face (concentric), or a planar face.
    pub(super) fn hole_pick_under(
        &self,
        pos: egui::Pos2,
        rect: Rect,
        edge: Option<EdgeId>,
        face: Option<(FaceId, Point3)>,
    ) -> Option<HolePick> {
        let part = self.part();
        // Points of visible sketches (not their projected references).
        let mut best: Option<(f32, HolePick)> = None;
        for (i, ps) in part.sketches.iter().enumerate() {
            if !ps.visible {
                continue;
            }
            for (pid, p) in ps.sketch.points() {
                if ps.is_projected(pid) {
                    continue;
                }
                let w = ps.sketch.to_world(&p.position);
                let Some(s) = self.camera.project(&w, rect) else {
                    continue;
                };
                let d = s.distance(pos);
                if d < super::overlay::POINT_SNAP_PX && best.as_ref().is_none_or(|b| d < b.0) {
                    best = Some((d, HolePick::SketchPoint(i, pid)));
                }
            }
        }
        if let Some((_, p)) = best {
            return Some(p);
        }
        let clicked = self
            .hole_draft
            .as_ref()
            .is_some_and(|d| !d.points.is_empty() && d.edit.is_none());
        if let Some(e) = edge {
            let body = &part.body;
            let curve = body.edge(e).and_then(|x| body.curve(x.curve));
            if matches!(curve, Some(crate::kernel::Curve::Circle(_)))
                && let Some(r) = crate::part::blend::edge_ref(body, e)
            {
                return Some(HolePick::Circle(r));
            }
            if clicked
                && body.edge_direction(e).is_some()
                && let Some(x) = body.edge(e)
                && let (Some(va), Some(vb)) = (body.vertex(x.start), body.vertex(x.end))
                && let (Some(a), Some(b)) = (va.name.clone(), vb.name.clone())
            {
                return Some(HolePick::Edge(a, b, [va.point, vb.point]));
            }
        }
        let (f, at) = face?;
        let body = &part.body;
        let surface = body
            .face(f)
            .and_then(|x| body.surface(x.facet_of.unwrap_or(x.surface)));
        let name = body.face(f).and_then(|x| x.name.clone())?;
        match surface {
            Some(crate::kernel::Surface::Plane(_)) => Some(HolePick::Face(name, at)),
            Some(crate::kernel::Surface::Cylinder(_)) if !clicked => {
                crate::part::shell::face_ref(body, f, Some(at)).map(HolePick::Cylinder)
            }
            _ => None,
        }
    }

    /// A click while Hole is open.
    pub(super) fn hole_pick(&mut self, pick: HolePick) {
        let ids: Vec<String> = self.part().sketches.iter().map(|s| s.id.clone()).collect();
        let size = 1.0f64.max(
            self.part()
                .body
                .bounding_box()
                .map_or(1.0, |(a, b)| (b - a).norm()),
        );
        let Some(d) = self.hole_draft.as_mut() else {
            return;
        };
        if d.to_pick {
            if let HolePick::Face(name, at) = pick {
                d.def.spec.termination = crate::part::Termination::To(crate::part::FaceRef {
                    face: name,
                    hint: [at.x, at.y, at.z],
                });
                d.to_pick = false;
            }
            return;
        }
        if d.edit.is_some() {
            // Centres of an existing hole are edited in its sketch.
            self.file_status = Some(Err(
                "Hole: edit the centres in the hole's sketch (Edit Sketch)".into(),
            ));
            return;
        }
        match pick {
            HolePick::SketchPoint(i, pid) => {
                let Some(id) = ids.get(i) else { return };
                if d.face.is_some() || (!d.def.sketch.is_empty() && d.def.sketch != *id) {
                    return;
                }
                d.def.sketch = id.clone();
                let c = HoleCenter::Point(pid);
                if let Some(k) = d.def.centers.iter().position(|x| *x == c) {
                    d.def.centers.remove(k);
                } else {
                    d.def.centers.push(c);
                }
                if !d
                    .def
                    .centers
                    .iter()
                    .any(|c| matches!(c, HoleCenter::Point(_)))
                {
                    d.def.sketch.clear();
                }
            }
            HolePick::Circle(r) => {
                let c = HoleCenter::Concentric(r.clone());
                if let Some(k) = d.def.centers.iter().position(|x| match x {
                    HoleCenter::Concentric(e) => e.faces == r.faces,
                    _ => false,
                }) {
                    d.def.centers.remove(k);
                } else {
                    d.def.centers.push(c);
                }
            }
            HolePick::Cylinder(r) => {
                if let Some(k) = d.def.centers.iter().position(|x| match x {
                    HoleCenter::Axis(f) => f.face == r.face,
                    _ => false,
                }) {
                    d.def.centers.remove(k);
                } else {
                    d.def.centers.push(HoleCenter::Axis(r));
                }
            }
            HolePick::Edge(a, b, ends) => {
                // A dimension from the last clicked centre (two at most).
                let Some(k) = d.points.len().checked_sub(1) else {
                    return;
                };
                let p = d.points[k];
                let mut dim = HoleDim {
                    center: k,
                    a,
                    b,
                    ends,
                    value: 0.0,
                    expr: None,
                };
                dim.value = (p - dim.foot(p)).norm();
                if dim.value < 1e-9 {
                    return;
                }
                if let Some(j) = d
                    .dims
                    .iter()
                    .position(|x| x.center == k && x.a == dim.a && x.b == dim.b)
                {
                    d.dims.remove(j);
                } else if d.dims.iter().filter(|x| x.center == k).count() < 2 {
                    d.dims.push(dim);
                }
            }
            HolePick::Face(name, at) => {
                if !d.def.sketch.is_empty() || !option(&ALLOW_CREATE) {
                    return;
                }
                match &d.face {
                    Some(f) if *f != name => {
                        if d.points.is_empty() {
                            d.face = Some(name);
                            d.points.push(at);
                        }
                    }
                    _ => {
                        d.face = Some(name);
                        // A click on a centre removes it (and its dimensions).
                        if let Some(k) = d.points.iter().position(|p| (p - at).norm() < 0.01 * size)
                        {
                            d.points.remove(k);
                            d.dims.retain(|x| x.center != k);
                            for x in &mut d.dims {
                                if x.center > k {
                                    x.center -= 1;
                                }
                            }
                        } else {
                            d.points.push(at);
                        }
                        if d.points.is_empty() {
                            d.face = None;
                        }
                    }
                }
            }
        }
    }
    /// A click on face `f` at `at` while Thread is open: the face and its
    /// start end; the size follows the face's diameter.
    pub(super) fn thread_pick(&mut self, f: FaceId, at: Point3) {
        let body = self.part().body.clone();
        let Some(r) = crate::part::shell::face_ref(&body, f, Some(at)) else {
            return;
        };
        let cf = match hole::cylinder_face(&body, &r) {
            Ok(c) => c,
            Err(e) => {
                self.file_status = Some(Err(format!("Thread: {e}")));
                return;
            }
        };
        let Some(d) = self.thread_draft.as_mut() else {
            return;
        };
        if let Some(t) = fitting_thread(
            Standards::global(),
            &d.spec.thread_type,
            2.0 * cf.radius,
            cf.internal,
        ) {
            let keep = d.spec.clone();
            d.spec = ThreadSpec {
                full_depth: keep.full_depth,
                depth: keep.depth,
                right_hand: keep.right_hand,
                modeled: keep.modeled,
                clearance: keep.clearance,
                ..t
            };
        }
        d.face = Some(r);
        self.file_status = None;
    }

    /// The centres to show (model points).
    pub(super) fn hole_centres(&self) -> Vec<Point3> {
        let Some(d) = &self.hole_draft else {
            return Vec::new();
        };
        let mut v = d.points.clone();
        if let Ok(places) = self.part().hole_placements(&d.def) {
            v.extend(places.into_iter().map(|(o, _, _)| o));
        }
        v
    }

    pub(super) fn hole_preview(&self) -> Result<Vec<Vec<Point3>>, String> {
        let Some(d) = &self.hole_draft else {
            return Err(String::new());
        };
        if d.count() == 0 {
            return Err(
                "Hole: click a face, a sketch point or a circular edge for the centres".into(),
            );
        }
        let key = format!("hole|{:?}|{:?}|{:?}|{:?}", d.edit, d.def, d.face, d.points);
        // Cached or still computing: no copy of the part this frame.
        if let Some(r) = self.preview_ready(&key) {
            return r;
        }
        let mut trial = self.part().clone();
        let d = d.clone();
        self.preview_async(key, move || {
            let built = match d.edit {
                Some(k) => trial.replace_feature(k, Feature::Hole(d.def.clone())),
                None => {
                    let def = d.build(&mut trial)?;
                    trial.add_hole(def)
                }
            };
            built
                .map(|id| Self::feature_polylines(&trial, &id))
                .map_err(|e| e.to_string())
        })
    }

    pub(super) fn thread_preview(&self) -> Result<Vec<Vec<Point3>>, String> {
        let Some(d) = &self.thread_draft else {
            return Err(String::new());
        };
        let Some(def) = d.def() else {
            return Err("Thread: click a cylindrical face".into());
        };
        let key = format!("thread|{:?}|{def:?}", d.edit);
        // Cached or still computing: no copy of the part this frame.
        if let Some(r) = self.preview_ready(&key) {
            return r;
        }
        let mut trial = self.part().clone();
        let edit = d.edit;
        self.preview_async(key, move || {
            let built = match edit {
                Some(k) => trial.replace_feature(k, Feature::Thread(def)),
                None => trial.add_thread(def),
            };
            built
                .map(|id| {
                    let mut v = Self::feature_polylines(&trial, &id);
                    for c in trial.cosmetic_threads.iter().filter(|c| c.feature == id) {
                        v.extend(thread_lines(c));
                    }
                    v
                })
                .map_err(|e| e.to_string())
        })
    }

    /// OK (`keep_open` false) or Apply.
    pub(super) fn commit_hole(&mut self, keep_open: bool) {
        let Some(d) = self.hole_draft.clone() else {
            return;
        };
        let cmd = if d.edit.is_some() {
            "Edit Feature"
        } else {
            "Hole"
        };
        self.checkpoint(cmd);
        let n_sketches = self.part().sketches.len();
        let built = match d.edit {
            Some(k) => self
                .part_mut()
                .replace_feature(k, Feature::Hole(d.def.clone())),
            None => match d.build(self.part_mut()) {
                Ok(def) => self.part_mut().add_hole(def),
                Err(e) => Err(crate::part::PartError::Feature(e)),
            },
        };
        if built.is_err() {
            // The sketch made for the centres goes with the failure.
            while self.part().sketches.len() > n_sketches {
                let last = self.part().sketches.len() - 1;
                let _ = self.part_mut().delete_sketch(last);
            }
            if let Some(k) = d.edit {
                self.part_mut().rollback = Some(k);
                let _ = self.part_mut().regenerate();
            }
        }
        match built {
            Ok(_) => {
                self.file_status = None;
                self.scene_dirty = true;
                if keep_open && d.edit.is_none() {
                    // Apply (+): the same settings, new centres. Keep sketch
                    // visible on (+): the hole's sketch is shared for the
                    // next hole (Cancel unshares it).
                    let mut next = d;
                    next.def.centers.clear();
                    next.def.sketch.clear();
                    next.points.clear();
                    next.dims.clear();
                    next.face = None;
                    next.kept = None;
                    if option(&KEEP_VISIBLE)
                        && let Some(Feature::Hole(h)) = self
                            .part()
                            .features
                            .get(self.part().last_inserted())
                            .cloned()
                        && !h.sketch.is_empty()
                        && let Some(ps) = self
                            .part_mut()
                            .sketches
                            .iter_mut()
                            .find(|s| s.id == h.sketch)
                    {
                        ps.shared = true;
                        ps.visible = true;
                        next.def.sketch = h.sketch.clone();
                        next.kept = Some(h.sketch);
                    }
                    self.hole_draft = Some(next);
                } else {
                    self.hole_draft = None;
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

    /// The breadcrumb's Sketch: the panel's edits are committed (OK) and the
    /// hole's sketch opens for editing (dimensions, more centre points);
    /// Finish Sketch regenerates the hole.
    pub(super) fn hole_to_sketch(&mut self) {
        let Some(d) = self.hole_draft.clone() else {
            return;
        };
        let n = self.part().insert_index();
        self.commit_hole(false);
        if self.hole_draft.is_some() {
            return; // the commit failed; its reason is in the status bar
        }
        self.edit_hole_sketch(d.edit.unwrap_or(n));
    }

    /// **Edit Sketch** of Hole feature `k` (browser menu, breadcrumb).
    pub(super) fn edit_hole_sketch(&mut self, k: usize) {
        let Some(id) = self
            .part()
            .features
            .get(k)
            .and_then(|f| f.sketch().map(str::to_owned))
        else {
            self.file_status = Some(Err("Edit Sketch: the feature has no sketch".into()));
            return;
        };
        if let Some(i) = self.part().sketches.iter().position(|s| s.id == id) {
            self.edit_sketch(i);
        }
    }

    /// Project Geometry: a click at `p` on a straight model edge projects
    /// it into sketch `i` (one Undo step). Returns whether the click was
    /// taken.
    pub(super) fn project_click(&mut self, i: usize, p: egui::Pos2, rect: Rect) -> bool {
        let Some(e) = self.edge_under(p, rect) else {
            return false;
        };
        self.checkpoint("Project Geometry");
        match self.part_mut().project_edge(i, e) {
            Ok(_) => {
                let _ = self.part_mut().sketches[i].sketch.solve();
                self.file_status = None;
                self.scene_dirty = true;
            }
            Err(err) => {
                self.history.cancel();
                self.file_status = Some(Err(err.to_string()));
            }
        }
        true
    }

    /// Cancel / Esc of Hole: a sketch kept visible for this hole is
    /// unshared again (Inventor).
    pub(super) fn cancel_hole(&mut self) {
        if let Some(d) = self.hole_draft.take()
            && let Some(id) = d.kept
            && d.def.centers.is_empty()
            && let Some(i) = self.part().sketches.iter().position(|s| s.id == id)
            && self.part().consumers(&id).len() <= 1
        {
            let _ = self.part_mut().unshare_sketch(i);
        }
    }

    pub(super) fn commit_thread(&mut self, keep_open: bool) {
        let Some(d) = self.thread_draft.clone() else {
            return;
        };
        let Some(def) = d.def() else {
            self.file_status = Some(Err("Thread: click a cylindrical face".into()));
            return;
        };
        let cmd = if d.edit.is_some() {
            "Edit Feature"
        } else {
            "Thread"
        };
        self.checkpoint(cmd);
        let built = match d.edit {
            Some(k) => self.part_mut().replace_feature(k, Feature::Thread(def)),
            None => self.part_mut().add_thread(def),
        };
        if built.is_err()
            && let Some(k) = d.edit
        {
            self.part_mut().rollback = Some(k);
            let _ = self.part_mut().regenerate();
        }
        match built {
            Ok(_) => {
                self.file_status = None;
                self.scene_dirty = true;
                if keep_open && d.edit.is_none() {
                    let mut next = d;
                    next.face = None;
                    self.thread_draft = Some(next);
                } else {
                    self.thread_draft = None;
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

    /// The open Hole's centres and dimensions. (Cosmetic threads are a
    /// texture in the 3D scene: `renderer::SceneGeometry::add_thread_bands`.)
    pub(super) fn draw_holes_and_threads(&self, painter: &egui::Painter, rect: Rect) {
        if self.hole_draft.is_some() {
            let col = Color32::from_rgb(230, 120, 20);
            for p in self.hole_centres() {
                if let Some(s) = self.camera.project(&p, rect) {
                    painter.circle_stroke(s, 5.0, Stroke::new(2.0, col));
                    painter.line_segment(
                        [s - egui::vec2(7.0, 0.0), s + egui::vec2(7.0, 0.0)],
                        Stroke::new(1.0, col),
                    );
                    painter.line_segment(
                        [s - egui::vec2(0.0, 7.0), s + egui::vec2(0.0, 7.0)],
                        Stroke::new(1.0, col),
                    );
                }
            }
            // The centres' dimensions to model edges: the edge, the
            // perpendicular and the value (edited in the panel).
            if let Some(d) = &self.hole_draft {
                let dim = Color32::from_rgb(20, 90, 200);
                for x in &d.dims {
                    let Some(&p) = d.points.get(x.center) else {
                        continue;
                    };
                    let f = x.foot(p);
                    let (Some(a), Some(b)) =
                        (self.camera.project(&p, rect), self.camera.project(&f, rect))
                    else {
                        continue;
                    };
                    if let (Some(e0), Some(e1)) = (
                        self.camera.project(&x.ends[0], rect),
                        self.camera.project(&x.ends[1], rect),
                    ) {
                        painter.line_segment([e0, e1], Stroke::new(3.0, dim));
                    }
                    painter.arrow(b, a - b, Stroke::new(1.5, dim));
                    let text = match &x.expr {
                        Some(e) => format!("{e} = {}", hole::num(x.value, 3)),
                        None => hole::num(x.value, 3),
                    };
                    painter.text(
                        egui::pos2((a.x + b.x) / 2.0, (a.y + b.y) / 2.0) - egui::vec2(0.0, 8.0),
                        egui::Align2::CENTER_BOTTOM,
                        text,
                        egui::FontId::proportional(13.0),
                        dim,
                    );
                }
            }
        }
    }
}

/// A cosmetic thread as lines: a helix of its pitch (at most 40 turns
/// drawn) and the circles at both ends (the Thread command's preview; the
/// finished thread is a texture in the scene).
pub(super) fn thread_lines(c: &hole::CosmeticThread) -> Vec<Vec<Point3>> {
    use crate::kernel::Vector3;
    let k = c.axis;
    let any = if k.x.abs() < 0.9 {
        Vector3::x()
    } else {
        Vector3::y()
    };
    let u = (any - k * any.dot(&k)).normalize();
    let w = k.cross(&u);
    let at = |t: Real, h: Real| c.start + k * h + (u * t.cos() + w * t.sin()) * c.radius;
    let turns = (c.length / c.pitch.max(1e-3)).min(40.0);
    let n = (turns * 24.0).ceil().max(2.0) as usize;
    let helix: Vec<Point3> = (0..=n)
        .map(|i| {
            let f = i as Real / n as Real;
            at(f * turns * std::f64::consts::TAU, f * c.length)
        })
        .collect();
    let circle = |h: Real| -> Vec<Point3> {
        (0..=48)
            .map(|i| at(i as Real / 48.0 * std::f64::consts::TAU, h))
            .collect()
    };
    vec![helix, circle(0.0), circle(c.length)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::standards::ISO_METRIC;

    fn thread_faces(part: &PartDocument) -> usize {
        part.body
            .faces()
            .filter(|(_, f)| f.name.as_deref().is_some_and(|n| n.contains(":Thread")))
            .count()
    }

    #[test]
    fn modeled_thread_through_the_gui_path() {
        // New hole: Tapped, Modeled, as the dialog sets it.
        let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let mut d = HoleDraft::new();
        d.face = Some("Box1:Top".into());
        d.points = vec![Point3::new(20.0, 15.0, 20.0)];
        let mut t = ThreadSpec::default_metric();
        t.modeled = true;
        d.def.spec.hole_type = crate::part::HoleType::Tapped;
        d.def.spec.diameter = t.minor;
        d.def.spec.thread = Some(t);
        let mut plain = part.clone();
        let def = d.build(&mut part).unwrap();
        part.add_hole(def).unwrap();
        eprintln!("health {:?}", part.feature_health);
        assert!(thread_faces(&part) > 50, "new: {}", thread_faces(&part));
        // The scene the view draws has the groove's triangles.
        let mut cos = d.clone();
        if let Some(t) = cos.def.spec.thread.as_mut() {
            t.modeled = false;
        }
        let def = cos.build(&mut plain).unwrap();
        plain.add_hole(def).unwrap();
        let tris = |p: &PartDocument| {
            let mut g = super::super::renderer::SceneGeometry::default();
            super::super::material::add_part_meshes(&mut g, p);
            g.counts().0
        };
        eprintln!("scene cosmetic {} modeled {}", tris(&plain), tris(&part));
        assert!(tris(&part) > tris(&plain) + 3000, "the groove is drawn");
        // Edit Feature: cosmetic → modeled, rolled back as the GUI does.
        let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let mut d = HoleDraft::new();
        d.face = Some("Box1:Top".into());
        d.points = vec![Point3::new(20.0, 15.0, 20.0)];
        d.def.spec = HoleSpec::tapped(ThreadSpec::default_metric(), 12.0);
        let def = d.build(&mut part).unwrap();
        part.add_hole(def).unwrap();
        assert_eq!(thread_faces(&part), 0);
        let k = part.features.len() - 1;
        part.rollback = Some(k);
        part.regenerate().unwrap();
        let Some(Feature::Hole(mut h)) = part.features.get(k).cloned() else {
            panic!("hole")
        };
        if let Some(t) = h.spec.thread.as_mut() {
            t.modeled = true;
        }
        part.replace_feature(k, Feature::Hole(h)).unwrap();
        eprintln!("health {:?}", part.feature_health);
        assert!(thread_faces(&part) > 50, "edit: {}", thread_faces(&part));
    }

    #[test]
    fn modeled_thread_command_through_the_gui_path() {
        use crate::part::{
            ExtrudeFeature, OriginPlane, ProfileRef, SketchOptions, SketchPlacement,
        };
        // A Ø8 shaft 20 long, Thread full length, Modeled.
        let mut part = PartDocument::default();
        let i = part
            .create_sketch(
                SketchPlacement::on_origin_plane(OriginPlane::XY),
                &SketchOptions::default(),
            )
            .unwrap();
        part.sketches[i]
            .sketch
            .add_circle(crate::sketch::Point2::new(0.0, 0.0), 4.0)
            .unwrap();
        let r = part.sketches[i].sketch.regions();
        let disc = r.iter().find(|r| r.holes.is_empty()).unwrap();
        part.add_extrude(
            i,
            ExtrudeFeature::new(vec![ProfileRef::of(disc, disc.interior_point())], 20.0),
        )
        .unwrap();
        let side = part
            .body
            .faces()
            .find(|(_, f)| f.name.as_deref().is_some_and(|n| n.contains(":Side")))
            .map(|(fid, _)| fid)
            .unwrap();
        let fr = crate::part::shell::face_ref(&part.body, side, Some(Point3::new(4.0, 0.0, 19.0)))
            .unwrap();
        let cf = hole::cylinder_face(&part.body, &fr).unwrap();
        let mut d = ThreadDraft::new();
        let t = fitting_thread(
            Standards::global(),
            &d.spec.thread_type,
            2.0 * cf.radius,
            cf.internal,
        )
        .unwrap();
        d.spec = ThreadSpec {
            modeled: true,
            full_depth: true,
            ..t
        };
        d.face = Some(fr);
        part.add_thread(d.def().unwrap()).unwrap();
        eprintln!("health {:?}", part.feature_health);
        assert!(thread_faces(&part) > 50, "shaft: {}", thread_faces(&part));
    }

    #[test]
    fn edge_dimensions_become_sketch_dimensions() {
        let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let mut d = HoleDraft::new();
        d.face = Some("Box1:Top".into());
        d.points = vec![Point3::new(12.0, 9.0, 20.0)];
        // The box's front top edge (y 0, z 20): Box1:V001 … V101.
        let (a, b) = ("Box1:V001", "Box1:V101");
        let pa = part
            .body
            .vertices()
            .find(|(_, v)| v.name.as_deref() == Some(a))
            .unwrap()
            .1
            .point;
        let pb = part
            .body
            .vertices()
            .find(|(_, v)| v.name.as_deref() == Some(b))
            .unwrap()
            .1
            .point;
        d.dims.push(HoleDim {
            center: 0,
            a: a.into(),
            b: b.into(),
            ends: [pa, pb],
            value: 9.0,
            expr: None,
        });
        // The panel's field: 12 from the edge.
        d.set_dim(0, 12.0, None);
        assert!((d.points[0].y - 12.0).abs() < 1e-9);
        let def = d.build(&mut part).unwrap();
        part.add_hole(def).unwrap();
        let ps = &part.sketches[0];
        assert_eq!(ps.reference_lines.len(), 1, "the projected edge");
        assert!(ps.sketch.constraints().any(|(_, c)| matches!(
            c,
            Constraint::PointLineDistance { value, .. } if (*value - 12.0).abs() < 1e-9
        )));
        assert!((part.holes()[0].origin.y - 12.0).abs() < 1e-6);
    }

    #[test]
    fn unused_center_points_are_preselected() {
        let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        assert!(unused_center_points(&part).is_none());
        let (i, ids) = part
            .create_hole_sketch(
                "Box1:Top",
                &[Point3::new(10.0, 10.0, 20.0), Point3::new(30.0, 10.0, 20.0)],
            )
            .unwrap();
        let id = part.sketches[i].id.clone();
        let (sid, pts) = unused_center_points(&part).unwrap();
        assert_eq!((sid, pts.len()), (id.clone(), 2));
        // One used by a hole (shared sketch, still visible): one left.
        let mut def = HoleFeature::new(HoleSpec::simple(5.0, 5.0));
        def.sketch = id.clone();
        def.centers = vec![HoleCenter::Point(ids[0])];
        part.add_hole(def).unwrap();
        part.sketches[i].visible = true;
        let (_, left) = unused_center_points(&part).unwrap();
        assert_eq!(left, vec![ids[1]]);
    }

    #[test]
    fn the_size_follows_the_face() {
        let t = Standards::builtin();
        let shaft = fitting_thread(&t, ISO_METRIC, 8.0, false).unwrap();
        assert_eq!(
            (shaft.designation.as_str(), shaft.class.as_str()),
            ("M8", "6g")
        );
        let hole = fitting_thread(&t, ISO_METRIC, 6.647, true).unwrap();
        assert_eq!(
            (hole.designation.as_str(), hole.class.as_str()),
            ("M8", "6H")
        );
        let tap = fitting_thread(&t, ISO_METRIC, 5.0, true).unwrap();
        assert_eq!(tap.size, "M6", "a 5 mm tap drill is M6");
    }

    #[test]
    fn face_centres_become_the_hole_sketch() {
        let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let mut d = HoleDraft::new();
        d.face = Some("Box1:Top".into());
        d.points = vec![Point3::new(10.0, 10.0, 20.0), Point3::new(30.0, 20.0, 20.0)];
        let def = d.build(&mut part).unwrap();
        assert_eq!(def.centers.len(), 2);
        assert_eq!(def.sketch, part.sketches[0].id);
        part.add_hole(def).unwrap();
        assert_eq!(part.holes().len(), 2);
        assert!(!part.sketches[0].visible, "consumed by the hole");
        // The cosmetic display of a thread: a helix and two circles.
        let c = hole::CosmeticThread {
            feature: "Hole1".into(),
            start: Point3::origin(),
            axis: -crate::kernel::Vector3::z(),
            radius: 4.0,
            length: 10.0,
            pitch: 1.25,
            shown: true,
            face_radius: 3.4,
            internal: true,
            right_hand: true,
        };
        let l = thread_lines(&c);
        assert_eq!(l.len(), 3);
        assert!(
            l[0].iter()
                .all(|p| ((p.x * p.x + p.y * p.y).sqrt() - 4.0).abs() < 1e-9)
        );
        assert!((l[2][0].z + 10.0).abs() < 1e-9);
    }
}
