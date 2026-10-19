//! The Hole and Thread commands (`docs/spec/2.3-hole.md`,
//! `docs/spec/2.9-thread.md`).
//!
//! Hole (Inventor 2027 placement): a click on a planar face places a centre
//! there (the feature's sketch is made on that face at OK; further clicks
//! add centres on the same face, a click on a centre removes it); a click on
//! a point of a visible sketch uses that sketch's points; a click on a
//! circular edge places a concentric centre. Thread: a click on a
//! cylindrical face, near the end where the thread starts.
//!
//! The dialog's state is the feature definition itself (`HoleSpec` /
//! `ThreadSpec`, both serde types): presets can store and restore it.

use eframe::egui::{self, Color32, Rect, Stroke};

use super::CadCraftApp;
use crate::kernel::{EdgeId, FaceId, Point3, Real};
use crate::part::hole::{self, HoleCenter};
use crate::part::standards::Standards;
use crate::part::{Feature, HoleFeature, HoleSpec, PartDocument, ThreadFeature, ThreadSpec};
use crate::sketch::PointId;

/// What a click in the view means for Hole.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum HolePick {
    /// A point of a visible sketch (index, point).
    SketchPoint(usize, PointId),
    /// A circular edge: a concentric centre.
    Circle(crate::part::EdgeRef),
    /// A planar face (name) at a model point.
    Face(String, Point3),
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
    /// Edit Feature of this feature index.
    pub edit: Option<usize>,
    /// The next face click sets the termination face (To).
    pub to_pick: bool,
}

impl HoleDraft {
    /// Inventor's defaults: Simple, Ø6 × 10, 118° drill point.
    pub(super) fn new() -> Self {
        Self {
            def: HoleFeature::new(HoleSpec::default()),
            face: None,
            points: Vec::new(),
            edit: None,
            to_pick: false,
        }
    }

    pub(super) fn editing(def: HoleFeature, k: usize) -> Self {
        Self {
            def,
            face: None,
            points: Vec::new(),
            edit: Some(k),
            to_pick: false,
        }
    }

    /// How many centres.
    pub(super) fn count(&self) -> usize {
        self.def.centers.len() + self.points.len()
    }

    /// The feature as it will be added to `part` (its face centres become
    /// a sketch on the face): (part with the sketch, definition).
    fn build(&self, part: &mut PartDocument) -> Result<HoleFeature, String> {
        let mut def = self.def.clone();
        if let (Some(face), false) = (&self.face, self.points.is_empty()) {
            let (i, ids) = part
                .create_hole_sketch(face, &self.points)
                .map_err(|e| e.to_string())?;
            def.sketch = part.sketches[i].id.clone();
            let mut centers: Vec<HoleCenter> = ids.into_iter().map(HoleCenter::Point).collect();
            centers.extend(
                def.centers
                    .iter()
                    .filter(|c| matches!(c, HoleCenter::Concentric(_)))
                    .cloned(),
            );
            def.centers = centers;
        }
        Ok(def)
    }
}

/// The open Thread command.
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
    pub(super) fn start_hole(&mut self) {
        self.finish_sketch();
        self.mode = super::Mode::Normal;
        self.cancel_extrude();
        self.work_draft = None;
        self.blend_draft = None;
        let mut d = HoleDraft::new();
        if let [super::select::SelItem::Face(name)] = self.sel.as_slice() {
            d.face = Some(name.clone());
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

    /// What Hole would take under the cursor: a sketch point, a circular
    /// edge, or a planar face.
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
        if let Some(e) = edge {
            let body = &part.body;
            let circle = body
                .edge(e)
                .and_then(|x| body.curve(x.curve))
                .is_some_and(|c| matches!(c, crate::kernel::Curve::Circle(_)));
            if circle && let Some(r) = crate::part::blend::edge_ref(body, e) {
                return Some(HolePick::Circle(r));
            }
        }
        let (f, at) = face?;
        let body = &part.body;
        let planar = body
            .face(f)
            .and_then(|x| body.surface(x.surface))
            .is_some_and(|s| matches!(s, crate::kernel::Surface::Plane(_)));
        let name = body.face(f).and_then(|x| x.name.clone())?;
        planar.then_some(HolePick::Face(name, at))
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
            HolePick::Face(name, at) => {
                if !d.def.sketch.is_empty() {
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
                        // A click on a centre removes it.
                        if let Some(k) = d.points.iter().position(|p| (p - at).norm() < 0.01 * size)
                        {
                            d.points.remove(k);
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
                    // Apply: the same settings, new centres.
                    let mut next = d;
                    next.def.centers.clear();
                    next.def.sketch.clear();
                    next.points.clear();
                    next.face = None;
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

    /// The cosmetic threads of the part (a display hint: a helix at the
    /// thread's other diameter), and the open Hole's centres.
    pub(super) fn draw_holes_and_threads(&self, painter: &egui::Painter, rect: Rect) {
        let stroke = Stroke::new(1.0, Color32::from_rgb(70, 90, 200));
        for c in self.part().cosmetic_threads.iter().filter(|c| c.shown) {
            for line in thread_lines(c) {
                self.draw_visible_polyline(painter, rect, &line, stroke);
            }
        }
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
        }
    }
}

/// A cosmetic thread as lines: a helix of its pitch (at most 40 turns
/// drawn) and the circles at both ends.
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
