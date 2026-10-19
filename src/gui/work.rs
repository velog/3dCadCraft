//! The Work Plane / Work Axis / Work Point commands
//! (`docs/spec/2.10-work-features.md`): one command per kind whose method
//! follows from what is picked (Inventor's general Plane / Axis / Point).
//! Also draws the work features and picks them in the view.

use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke};

use super::CadCraftApp;
use crate::kernel::{Plane, Point3, Real, Vector3};
use crate::part::{
    Feature, LineRef, PlaneRef, PointRef, WorkAxisDef, WorkDef, WorkPlaneDef, WorkPointDef,
};

/// Which work feature a command makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WorkKind {
    Plane,
    Axis,
    Point,
}

impl WorkKind {
    pub(super) fn title(self) -> &'static str {
        match self {
            WorkKind::Plane => "Work Plane",
            WorkKind::Axis => "Work Axis",
            WorkKind::Point => "Work Point",
        }
    }
}

/// One input of a work feature command.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum WpInput {
    Plane(PlaneRef),
    Line(LineRef),
    Point(PointRef),
    /// A circular edge (by name).
    Curve(String),
    /// A face of a Revolve feature (by feature id).
    Revolved(String),
}

fn plane_text(r: &PlaneRef) -> String {
    match r {
        PlaneRef::Origin(p) => p.label().to_owned(),
        PlaneRef::Face(f) => format!("Face {f}"),
        PlaneRef::Work(id) => id.replace("WorkPlane", "Work Plane"),
        PlaneRef::Fixed => "Plane".to_owned(),
    }
}

impl WpInput {
    pub(super) fn describe(&self) -> String {
        match self {
            WpInput::Plane(p) => plane_text(p),
            WpInput::Line(l) => l.describe(),
            WpInput::Point(p) => p.describe(),
            WpInput::Curve(e) => format!("Circular edge {e}"),
            WpInput::Revolved(f) => format!("Revolved feature {f}"),
        }
    }
}

/// The command dialog's state.
#[derive(Clone, Debug)]
pub(super) struct WorkDraft {
    pub kind: WorkKind,
    /// Picked inputs, in order.
    pub picks: Vec<WpInput>,
    /// Offset distance (mm).
    pub distance: Real,
    /// Angle (radians).
    pub angle: Real,
    /// Work point: ground it at `xyz`.
    pub grounded: bool,
    /// Grounded point position (mm).
    pub xyz: [Real; 3],
}

impl WorkDraft {
    pub(super) fn new(kind: WorkKind) -> Self {
        Self {
            kind,
            picks: Vec::new(),
            distance: 10.0,
            angle: std::f64::consts::FRAC_PI_2,
            grounded: false,
            xyz: [0.0; 3],
        }
    }

    /// The definition the picks make, or what is still needed.
    pub(super) fn def(&self) -> Result<WorkDef, &'static str> {
        use WpInput as I;
        match self.kind {
            WorkKind::Plane => {
                let d = match &self.picks[..] {
                    [] => return Err("select a plane, an edge or a point"),
                    [I::Plane(p)] => WorkPlaneDef::Offset {
                        base: p.clone(),
                        distance: self.distance,
                    },
                    [I::Plane(a), I::Plane(b)] => WorkPlaneDef::Midplane {
                        a: a.clone(),
                        b: b.clone(),
                    },
                    [I::Plane(p), I::Line(l)] | [I::Line(l), I::Plane(p)] => WorkPlaneDef::Angle {
                        base: p.clone(),
                        axis: l.clone(),
                        angle: self.angle,
                    },
                    [I::Plane(p), I::Point(q)] | [I::Point(q), I::Plane(p)] => {
                        WorkPlaneDef::ThroughPoint {
                            base: p.clone(),
                            point: q.clone(),
                        }
                    }
                    [I::Point(a), I::Point(b), I::Point(c)] => WorkPlaneDef::ThreePoints {
                        points: [a.clone(), b.clone(), c.clone()],
                    },
                    [I::Line(a), I::Line(b)] => WorkPlaneDef::TwoLines {
                        a: a.clone(),
                        b: b.clone(),
                    },
                    [I::Line(l), I::Point(p)] | [I::Point(p), I::Line(l)] => {
                        WorkPlaneDef::NormalToAxis {
                            axis: l.clone(),
                            point: p.clone(),
                        }
                    }
                    [I::Line(_)] => return Err("select a plane (angle), a second edge or a point"),
                    [I::Point(_)] => return Err("select a plane, an axis, or two more points"),
                    [I::Point(_), I::Point(_)] => return Err("select the third point"),
                    _ => return Err(NO_METHOD),
                };
                Ok(WorkDef::Plane(d))
            }
            WorkKind::Axis => {
                let d = match &self.picks[..] {
                    [] => return Err("select an edge, two points, two planes or a circular edge"),
                    [I::Line(l)] => WorkAxisDef::OnLine { line: l.clone() },
                    [I::Curve(e)] => WorkAxisDef::CircularEdge { edge: e.clone() },
                    [I::Revolved(f)] => WorkAxisDef::Revolved { feature: f.clone() },
                    [I::Point(a), I::Point(b)] => WorkAxisDef::TwoPoints {
                        a: a.clone(),
                        b: b.clone(),
                    },
                    [I::Plane(a), I::Plane(b)] => WorkAxisDef::TwoPlanes {
                        a: a.clone(),
                        b: b.clone(),
                    },
                    [I::Plane(p), I::Point(q)] | [I::Point(q), I::Plane(p)] => {
                        WorkAxisDef::NormalToPlane {
                            plane: p.clone(),
                            point: q.clone(),
                        }
                    }
                    [I::Point(q), I::Line(l)] | [I::Line(l), I::Point(q)] => {
                        WorkAxisDef::ParallelThroughPoint {
                            line: l.clone(),
                            point: q.clone(),
                        }
                    }
                    [I::Line(l), I::Plane(p)] | [I::Plane(p), I::Line(l)] => {
                        WorkAxisDef::LineOnPlane {
                            line: l.clone(),
                            plane: p.clone(),
                        }
                    }
                    [I::Point(_)] => return Err("select a second point, a line or a plane"),
                    [I::Plane(_)] => return Err("select a second plane or a point"),
                    _ => return Err(NO_METHOD),
                };
                Ok(WorkDef::Axis(d))
            }
            WorkKind::Point => {
                if self.grounded {
                    let [x, y, z] = self.xyz;
                    return Ok(WorkDef::Point(WorkPointDef::Fixed { x, y, z }));
                }
                let d = match &self.picks[..] {
                    [] => return Err("select a vertex, a midpoint, planes or lines"),
                    [I::Point(p)] => WorkPointDef::OnPoint { point: p.clone() },
                    [I::Curve(e)] => WorkPointDef::LoopCenter {
                        edge: e.clone(),
                        face: None,
                    },
                    [I::Plane(a), I::Plane(b), I::Plane(c)] => WorkPointDef::ThreePlanes {
                        planes: [a.clone(), b.clone(), c.clone()],
                    },
                    [I::Line(a), I::Line(b)] => WorkPointDef::TwoLines {
                        a: a.clone(),
                        b: b.clone(),
                    },
                    [I::Plane(p), I::Line(l)] | [I::Line(l), I::Plane(p)] => {
                        WorkPointDef::PlaneLine {
                            plane: p.clone(),
                            line: l.clone(),
                        }
                    }
                    [I::Plane(_)] | [I::Plane(_), I::Plane(_)] => {
                        return Err("select more planes (three) or a line");
                    }
                    [I::Line(_)] => return Err("select a second line or a plane"),
                    _ => return Err(NO_METHOD),
                };
                Ok(WorkDef::Point(d))
            }
        }
    }

    /// Adds a pick (a second click on the same input removes it); a pick
    /// that fits no method starts over with it.
    pub(super) fn pick(&mut self, input: WpInput) {
        if let Some(k) = self.picks.iter().position(|p| *p == input) {
            self.picks.remove(k);
            return;
        }
        self.picks.push(input.clone());
        if self.def().is_err_and(|e| e == NO_METHOD) || self.picks.len() > 3 {
            self.picks = vec![input];
        }
    }
}

const NO_METHOD: &str = "this combination is no method of the command";

/// Corners of the drawn rectangle of `plane` around `center`.
fn quad(plane: &Plane, center: &Point3, half: Real) -> [Point3; 4] {
    let x = plane.x_axis.into_inner() * half;
    let y = plane.y_axis().into_inner() * half;
    [
        center - x - y,
        center + x - y,
        center + x + y,
        center - x + y,
    ]
}

/// Ray hit of the rectangle (ray parameter).
fn hit(plane: &Plane, center: &Point3, half: Real, origin: &Point3, dir: &Vector3) -> Option<Real> {
    let n = plane.normal.into_inner();
    let denom = dir.dot(&n);
    if denom.abs() < 1e-12 {
        return None;
    }
    let t = (plane.origin - origin).dot(&n) / denom;
    let p = origin + dir * t;
    let d = p - center;
    (t > 0.0
        && d.dot(&plane.x_axis.into_inner()).abs() <= half
        && d.dot(&plane.y_axis().into_inner()).abs() <= half)
        .then_some(t)
}

/// Screen distance from `p` to the segment `a`–`b`.
fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

impl CadCraftApp {
    /// Centre of the model and the half size of drawn work features.
    fn work_extent(&self) -> (Point3, Real) {
        let (c, half) = match self.part().body.bounding_box() {
            Some((lo, hi)) => (nalgebra::center(&lo, &hi), (hi - lo).norm() * 0.42),
            None => (Point3::origin(), 30.0),
        };
        (c, half.max(self.origin_plane_half()))
    }

    /// Where a work plane is drawn: around the model's centre projected
    /// onto it, a bit larger than the model.
    pub(super) fn work_plane_frame(&self, plane: &Plane) -> (Point3, Real) {
        let (c, half) = self.work_extent();
        let uv = plane.project(&c);
        (plane.point_at(uv.x, uv.y), half)
    }

    /// The drawn segment of a work axis.
    fn axis_segment(&self, p: &Point3, d: &Vector3) -> (Point3, Point3) {
        let (c, half) = self.work_extent();
        let foot = p + d * (c - p).dot(d);
        (foot - d * half, foot + d * half)
    }

    fn visible_work(&self) -> impl Iterator<Item = &crate::part::WorkFeature> {
        self.part().features.iter().filter_map(|f| match f {
            Feature::Work(w) if w.visible => Some(w),
            _ => None,
        })
    }

    /// The visible work plane hit by the eye ray at `pos`: (id, ray t).
    pub(super) fn work_plane_under(&self, pos: Pos2, rect: Rect) -> Option<(String, Real)> {
        let (origin, dir) = self.camera.ray(pos, rect)?;
        let part = self.part();
        self.visible_work()
            .filter_map(|w| {
                let plane = part.work.planes.get(&w.id)?;
                let (c, half) = self.work_plane_frame(plane);
                Some((w.id.clone(), hit(plane, &c, half, &origin, &dir)?))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// The visible work point (8 px) or work axis (6 px) under `pos`.
    pub(super) fn work_axis_or_point_under(&self, pos: Pos2, rect: Rect) -> Option<WpInput> {
        let part = self.part();
        let mut best: Option<(f32, WpInput)> = None;
        for w in self.visible_work() {
            let cand = if let Some(p) = part.work.points.get(&w.id) {
                let s = self.camera.project(p, rect)?;
                let d = s.distance(pos);
                (d <= 8.0).then(|| (d - 2.0, WpInput::Point(PointRef::Work(w.id.clone()))))
            } else if let Some((p, d)) = part.work.axes.get(&w.id) {
                let (a, b) = self.axis_segment(p, d);
                let (a, b) = (
                    self.camera.project(&a, rect)?,
                    self.camera.project(&b, rect)?,
                );
                let d = seg_dist(pos, a, b);
                (d <= 6.0).then(|| (d, WpInput::Line(LineRef::Work(w.id.clone()))))
            } else {
                None
            };
            if let Some((d, i)) = cand
                && best.as_ref().is_none_or(|(bd, _)| d < *bd)
            {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    }

    /// The id of the visible work feature under `pos` (points, axes, then
    /// planes): for selection in the view.
    pub(super) fn work_feature_under(&self, pos: Pos2, rect: Rect) -> Option<String> {
        match self.work_axis_or_point_under(pos, rect) {
            Some(WpInput::Point(PointRef::Work(id)) | WpInput::Line(LineRef::Work(id))) => Some(id),
            _ => self.work_plane_under(pos, rect).map(|(id, _)| id),
        }
    }

    /// The named model vertex or straight-edge midpoint under `pos` (8 px),
    /// not hidden by a face.
    pub(super) fn vertex_under(&self, pos: Pos2, rect: Rect) -> Option<PointRef> {
        let (origin, dir) = self.camera.ray(pos, rect)?;
        let dir = dir.try_normalize(1e-12)?;
        let body = &self.part().body;
        let face_t = self.picker().pick(&origin, &dir).map(|(_, t)| t);
        let visible = |p: &Point3| {
            let t = (p - origin).dot(&dir) / dir.norm_squared();
            !face_t.is_some_and(|ft| ft < t - 0.01 * t.abs().max(1.0))
        };
        let mut best: Option<(f32, PointRef)> = None;
        let mut consider = |p: Point3, r: PointRef, bias: f32| {
            if let Some(s) = self.camera.project(&p, rect) {
                let d = s.distance(pos) + bias;
                if d <= 8.0 && visible(&p) && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                    best = Some((d, r));
                }
            }
        };
        for (_, v) in body.vertices() {
            if let Some(name) = &v.name {
                consider(v.point, PointRef::Vertex(name.clone()), 0.0);
            }
        }
        for (e, x) in body.edges() {
            if let (Some(name), Some(_)) = (&x.name, body.edge_direction(e))
                && let (Some(a), Some(b)) = (body.vertex(x.start), body.vertex(x.end))
            {
                // Vertices win over a midpoint at the same spot.
                consider(
                    nalgebra::center(&a.point, &b.point),
                    PointRef::EdgeMid(name.clone()),
                    1.0,
                );
            }
        }
        best.map(|(_, r)| r)
    }

    /// What a work feature command takes under `pos`: work point/axis >
    /// vertex/midpoint > edge > face / work plane / origin plane.
    pub(super) fn wp_input_under(&self, pos: Pos2, rect: Rect, kind: WorkKind) -> Option<WpInput> {
        if let Some(i) = self.work_axis_or_point_under(pos, rect) {
            return Some(i);
        }
        if let Some(v) = self.vertex_under(pos, rect) {
            return Some(WpInput::Point(v));
        }
        let body = &self.part().body;
        if let Some(e) = self.edge_under(pos, rect)
            && let Some(name) = body.edge(e).and_then(|x| x.name.clone())
        {
            if body.edge_direction(e).is_some() {
                return Some(WpInput::Line(LineRef::Edge(name)));
            }
            if let Some(x) = body.edge(e)
                && matches!(body.curve(x.curve), Some(crate::kernel::Curve::Circle(_)))
            {
                return Some(WpInput::Curve(name));
            }
        }
        let plane = self.plane_under(pos, rect, true)?;
        // An axis through a revolved (curved) face: its Revolve feature.
        if kind == WorkKind::Axis
            && let PlaneRef::Face(name) = &plane
            && let Some(f) = body.find_face(name).and_then(|f| body.face(f))
            && f.facet_of.is_some()
            && let Some(k) = self.face_feature(name)
            && let Feature::Revolve(r) = &self.part().features[k]
        {
            return Some(WpInput::Revolved(r.id.clone()));
        }
        Some(WpInput::Plane(plane))
    }

    /// Draws the visible work features (and the open command's preview).
    pub(super) fn draw_work_features(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        hovered: Option<&str>,
    ) {
        let part = self.part();
        let selected = match self.selection {
            super::Selection::Feature(k) => part.features.get(k).map(|f| f.id().to_owned()),
            _ => None,
        };
        for w in self.visible_work() {
            let hot = hovered == Some(w.id.as_str()) || selected.as_deref() == Some(w.id.as_str());
            if let Some(plane) = part.work.planes.get(&w.id) {
                // Only the outline while a sketch is edited (no tint on it).
                let fill = if self.editing.is_some() {
                    Color32::TRANSPARENT
                } else if hot {
                    Color32::from_rgba_unmultiplied(255, 160, 40, 80)
                } else {
                    Color32::from_rgba_unmultiplied(240, 200, 90, 40)
                };
                self.draw_plane_rect(painter, rect, plane, fill, hot, &w.name);
            } else if let Some((p, d)) = part.work.axes.get(&w.id) {
                self.draw_axis(painter, rect, p, d, hot, &w.name, AXIS_COLOR);
            } else if let Some(p) = part.work.points.get(&w.id) {
                let grounded = matches!(w.def, WorkDef::Point(WorkPointDef::Fixed { .. }));
                self.draw_point(painter, rect, p, hot, &w.name, grounded);
            }
        }
        if let Some(d) = &self.work_draft
            && let Ok(def) = d.def()
        {
            let preview = Color32::from_rgb(60, 150, 255);
            match self.compute_work(&def) {
                Ok(crate::part::work::WorkResult::Plane(_, plane)) => self.draw_plane_rect(
                    painter,
                    rect,
                    &plane,
                    Color32::from_rgba_unmultiplied(60, 150, 255, 70),
                    true,
                    d.kind.title(),
                ),
                Ok(crate::part::work::WorkResult::Axis(_, p, v)) => {
                    self.draw_axis(painter, rect, &p, &v, true, d.kind.title(), preview);
                }
                Ok(crate::part::work::WorkResult::Point(_, p)) => {
                    self.draw_point(painter, rect, &p, true, d.kind.title(), d.grounded);
                }
                Err(_) => {}
            }
        }
    }

    /// Computes a definition against the current part (preview, checks).
    pub(super) fn compute_work(
        &self,
        def: &WorkDef,
    ) -> Result<crate::part::work::WorkResult, String> {
        let part = self.part();
        let revolve_axis = |id: &str| {
            part.features.iter().find_map(|f| match f {
                Feature::Revolve(r) if r.id == id => {
                    let ps = part.sketches.iter().find(|s| s.id == r.sketch)?;
                    r.world_axis(&ps.sketch).ok()
                }
                _ => None,
            })
        };
        let ctx = crate::part::work::Ctx {
            body: &part.body,
            works: &part.work,
            revolve_axis: &revolve_axis,
        };
        def.build("preview", &ctx)
    }

    fn draw_plane_rect(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        plane: &Plane,
        fill: Color32,
        hot: bool,
        label: &str,
    ) {
        let (c, half) = self.work_plane_frame(plane);
        let pts: Option<Vec<Pos2>> = quad(plane, &c, half)
            .iter()
            .map(|p| self.camera.project(p, rect))
            .collect();
        let Some(pts) = pts else { return };
        let stroke = Stroke::new(if hot { 2.0 } else { 1.0 }, Color32::from_rgb(200, 140, 30));
        painter.add(Shape::convex_polygon(pts.clone(), fill, stroke));
        painter.text(
            pts[3],
            egui::Align2::LEFT_BOTTOM,
            label,
            egui::FontId::proportional(12.0),
            Color32::from_rgb(130, 85, 0),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_axis(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        p: &Point3,
        d: &Vector3,
        hot: bool,
        label: &str,
        color: Color32,
    ) {
        let (a, b) = self.axis_segment(p, d);
        let (Some(a), Some(b)) = (self.camera.project(&a, rect), self.camera.project(&b, rect))
        else {
            return;
        };
        let w = if hot { 3.0 } else { 1.5 };
        // Dash-dot like a centre line.
        let len = a.distance(b).max(1.0);
        let dir = (b - a) / len;
        let mut t = 0.0;
        let pattern = [14.0, 4.0, 3.0, 4.0];
        let mut k = 0;
        while t < len {
            let seg = pattern[k % 4];
            if k % 2 == 0 {
                painter.line_segment(
                    [a + dir * t, a + dir * (t + seg).min(len)],
                    Stroke::new(w, color),
                );
            }
            t += seg;
            k += 1;
        }
        painter.text(
            b,
            egui::Align2::LEFT_BOTTOM,
            label,
            egui::FontId::proportional(12.0),
            color,
        );
    }

    fn draw_point(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        p: &Point3,
        hot: bool,
        label: &str,
        grounded: bool,
    ) {
        let Some(s) = self.camera.project(p, rect) else {
            return;
        };
        let r = if hot { 6.0 } else { 4.5 };
        let color = Color32::from_rgb(200, 120, 20);
        // A diamond; grounded points are filled (Inventor's pushpin idea).
        let pts = vec![
            s + egui::vec2(0.0, -r),
            s + egui::vec2(r, 0.0),
            s + egui::vec2(0.0, r),
            s + egui::vec2(-r, 0.0),
        ];
        let fill = if grounded {
            color
        } else {
            Color32::TRANSPARENT
        };
        painter.add(Shape::convex_polygon(pts, fill, Stroke::new(1.5, color)));
        painter.text(
            s + egui::vec2(6.0, -6.0),
            egui::Align2::LEFT_BOTTOM,
            label,
            egui::FontId::proportional(12.0),
            color,
        );
    }

    /// Highlights an input under the cursor (or picked) in the view.
    pub(super) fn draw_wp_input(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        input: &WpInput,
        color: Color32,
    ) {
        let part = self.part();
        let body = &part.body;
        let opts = crate::kernel::TessellationOptions::default();
        let poly = |pts: &[Point3]| {
            let s: Vec<_> = pts
                .iter()
                .filter_map(|q| self.camera.project(q, rect))
                .collect();
            painter.add(Shape::line(s, Stroke::new(4.0, color)));
        };
        match input {
            WpInput::Point(r) => {
                if let Ok(p) = crate::part::placement::point_of(r, body, &part.work)
                    && let Some(s) = self.camera.project(&p, rect)
                {
                    painter.circle_filled(s, 5.0, color);
                }
            }
            WpInput::Line(LineRef::Edge(name)) | WpInput::Curve(name) => {
                if let Some(e) = body.find_edge(name)
                    && let Ok(pts) = body.sample_edge(e, &opts)
                {
                    poly(&pts);
                }
            }
            WpInput::Line(l) => {
                if let Ok((p, d)) = l.resolve(body, &part.work) {
                    let (a, b) = self.axis_segment(&p, &d);
                    poly(&[a, b]);
                }
            }
            WpInput::Revolved(f) => {
                if let Ok(crate::part::work::WorkResult::Axis(_, p, d)) =
                    self.compute_work(&WorkDef::Axis(WorkAxisDef::Revolved { feature: f.clone() }))
                {
                    let (a, b) = self.axis_segment(&p, &d);
                    poly(&[a, b]);
                }
            }
            WpInput::Plane(PlaneRef::Face(name)) => {
                if let Some(mesh) = body
                    .find_face(name)
                    .and_then(|f| body.face_mesh(f, &opts).ok())
                {
                    super::overlay::draw_face_fill(painter, &self.camera, rect, &mesh, color);
                }
            }
            WpInput::Plane(r) => {
                if let Ok(plane) = crate::part::placement::plane_of(r, body, &part.work) {
                    let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 60);
                    self.draw_plane_rect(painter, rect, &plane, fill, true, "");
                }
            }
        }
    }

    /// The command dialog's OK.
    pub(super) fn commit_work(&mut self) {
        let Some(d) = self.work_draft.clone() else {
            return;
        };
        let title = d.kind.title();
        let def = match d.def() {
            Ok(def) => def,
            Err(e) => {
                self.file_status = Some(Err(format!("{title}: {e}")));
                return;
            }
        };
        self.checkpoint(title);
        match self.part_mut().add_work(crate::part::WorkFeature::new(def)) {
            Ok(_) => {
                self.work_draft = None;
                self.selection = super::Selection::Feature(self.part().last_inserted());
                self.file_status = None;
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("{title}: {e}")));
            }
        }
    }
}

/// Colour of work axes.
pub(super) const AXIS_COLOR: Color32 = Color32::from_rgb(200, 120, 20);
