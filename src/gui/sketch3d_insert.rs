//! The 3D sketch's commands that take curves from elsewhere and Mirror
//! (`docs/spec/1.10-3d-sketch.md`): Mirror, Equation Curve, Project to
//! Surface, Intersection Curve, Silhouette Curve; their panel rows and
//! picks. The geometry is `part::sketch3d_curves` (display triangles) and
//! `Sketch3d::mirror`.

use eframe::egui::{self, Pos2, Rect};

use super::CadCraftApp;
use super::panels::{plabel, psection};
use super::sketch3d::Tool3;
use crate::kernel::{FaceId, Plane, Point3, Real, Vector3};
use crate::part::sketch3d_curves::{self as curves, EquationKind, Projection, Tri};

/// The state of the insert commands while the 3D sketch is open.
#[derive(Clone, Debug)]
pub(super) struct Sk3Insert {
    /// Equation Curve: coordinates, the three expressions, t min / max, steps.
    pub eq_kind: EquationKind,
    /// Its expressions.
    pub eq: [String; 3],
    /// t from / to.
    pub t: (Real, Real),
    /// Points along it.
    pub steps: usize,
    /// Project to Surface: the faces.
    pub faces: Vec<FaceId>,
    /// Project to Surface: the curves (world polylines) and what they are.
    pub curves: Vec<(String, Vec<Point3>)>,
    /// Project to Surface: picking faces (else curves).
    pub pick_faces: bool,
    /// Project to Surface: to the closest point (else along a direction).
    pub closest: bool,
    /// Project to Surface: the direction (0 X, 1 Y, 2 Z, 3 the curves'
    /// sketch normal).
    pub dir: usize,
    /// Intersection Curve: the first input (a face's triangles, or a plane).
    pub first: Option<(String, Vec<Tri>)>,
    /// Project to Surface: the normal of the sketch the curves came from.
    pub sketch_normal: Option<Vector3>,
    /// Silhouette Curve: the direction (0 X, 1 Y, 2 Z, 3 the view).
    pub sil_dir: usize,
    /// Mirror: the points to mirror.
    pub mirror: Vec<u32>,
}

impl Default for Sk3Insert {
    fn default() -> Self {
        Self {
            eq_kind: EquationKind::Cartesian,
            eq: [
                "20 * cos(t * 360 deg)".into(),
                "20 * sin(t * 360 deg)".into(),
                "10 * t".into(),
            ],
            t: (0.0, 2.0),
            steps: 48,
            faces: Vec::new(),
            curves: Vec::new(),
            pick_faces: true,
            closest: false,
            dir: 3,
            sketch_normal: None,
            first: None,
            sil_dir: 3,
            mirror: Vec::new(),
        }
    }
}

/// A plane as two large triangles (square `size` about the point nearest
/// `centre`), so it meets triangles like a face.
fn plane_tris(pl: &Plane, centre: Point3, size: Real) -> Vec<Tri> {
    let n = pl.normal.into_inner();
    let c = centre - n * (centre - pl.origin).dot(&n);
    let (x, y) = (pl.x_axis.into_inner(), pl.y_axis().into_inner());
    let p = |a: Real, b: Real| c + x * (a * size) + y * (b * size);
    vec![
        [p(-1.0, -1.0), p(1.0, -1.0), p(1.0, 1.0)],
        [p(-1.0, -1.0), p(1.0, 1.0), p(-1.0, 1.0)],
    ]
}

fn axis(k: usize) -> Vector3 {
    let mut a = Vector3::zeros();
    a[k.min(2)] = 1.0;
    a
}

impl CadCraftApp {
    /// The part's box: its middle and size (for planes as triangles).
    fn sk3_body_frame(&self) -> (Point3, Real) {
        self.part()
            .body
            .bounding_box()
            .map_or((Point3::origin(), 100.0), |(lo, hi)| {
                (lo + (hi - lo) / 2.0, (hi - lo).norm().max(10.0))
            })
    }

    /// The face under `p` (its id and name).
    fn sk3_face_under(&self, p: Pos2, rect: Rect) -> Option<(FaceId, String)> {
        let (o, d) = self.camera.ray(p, rect)?;
        let d = d.try_normalize(1e-12)?;
        let (f, _) = self.picker().pick(&o, &d)?;
        let name = self
            .part()
            .body
            .face(f)
            .and_then(|x| x.name.clone())
            .unwrap_or_else(|| "a face".into());
        Some((f, name))
    }

    /// A face or a plane under `p` as triangles (Intersection Curve).
    fn sk3_surface_under(&self, p: Pos2, rect: Rect) -> Option<(String, Vec<Tri>)> {
        if let Some((f, name)) = self.sk3_face_under(p, rect) {
            return Some((name, curves::face_triangles(&self.part().body, &[f])));
        }
        let r = self.plane_under_pub(p, rect)?;
        let part = self.part();
        let pl = crate::part::placement::plane_of(&r, &part.body, &part.work).ok()?;
        let (c, size) = self.sk3_body_frame();
        Some((format!("{r:?}"), plane_tris(&pl, c, size)))
    }

    /// The polylines added to the open sketch as fixed curves: a straight
    /// one as a line, any other an interpolation spline through ≤ `steps` + 1
    /// points (one Undo step).
    fn sk3_add_polylines(&mut self, label: &str, lines: Vec<Vec<Point3>>, steps: usize) {
        let (_, size) = self.sk3_body_frame();
        let tol = 1e-6 * size;
        let lines: Vec<Vec<Point3>> = lines
            .into_iter()
            .filter(|l| {
                l.len() >= 2 && l.windows(2).map(|w| (w[1] - w[0]).norm()).sum::<Real>() > tol
            })
            .collect();
        if lines.is_empty() {
            self.file_status = Some(Err(format!("{label}: no curve found")));
            return;
        }
        let label_s = label.to_owned();
        self.edit3d(label, move |s, _| {
            for l in &lines {
                let pts = if curves::is_straight(l, tol.max(1e-6)) {
                    vec![l[0], l[l.len() - 1]]
                } else {
                    curves::resample(l, steps.clamp(2, 200))
                };
                s.include(&pts, false)
                    .map_err(|e| format!("{label_s}: {e}"))?;
            }
            Ok(())
        });
    }

    /// A click while one of these tools runs; `true` when it took it.
    pub(super) fn sk3_insert_click(&mut self, tool: Tool3, p: Pos2, rect: Rect) -> bool {
        match tool {
            Tool3::Mirror => {
                self.sk3_mirror_click(p, rect);
                true
            }
            Tool3::Intersection => {
                let Some(hit) = self.sk3_surface_under(p, rect) else {
                    return true;
                };
                let first = self.sketch3d.as_mut().and_then(|e| e.ins.first.take());
                match first {
                    None => {
                        if let Some(e) = self.sketch3d.as_mut() {
                            e.ins.first = Some(hit);
                        }
                    }
                    Some((_, a)) => {
                        let (_, size) = self.sk3_body_frame();
                        let segs = curves::mesh_intersection(&a, &hit.1);
                        let lines = curves::chain_segments(&segs, 1e-5 * size);
                        let steps = self.sketch3d.as_ref().map_or(24, |e| e.ins.steps);
                        self.sk3_add_polylines("Intersection Curve", lines, steps);
                    }
                }
                true
            }
            Tool3::ProjectSurface => {
                let pick_faces = self.sketch3d.as_ref().is_some_and(|e| e.ins.pick_faces);
                if pick_faces {
                    if let Some((f, _)) = self.sk3_face_under(p, rect)
                        && let Some(e) = self.sketch3d.as_mut()
                    {
                        if let Some(i) = e.ins.faces.iter().position(|x| *x == f) {
                            e.ins.faces.remove(i);
                        } else {
                            e.ins.faces.push(f);
                        }
                    }
                } else if let Some(k) = self.sketch_under(p, rect) {
                    let ps = &self.part().sketches[k];
                    let mut got = Vec::new();
                    for (id, _) in ps.sketch.entities() {
                        if ps.sketch.style(id) == crate::sketch::Style::Construction {
                            continue;
                        }
                        if let Some(c) = ps.sketch.curve_of(id) {
                            let pts: Vec<Point3> = c
                                .sample(0.05)
                                .iter()
                                .map(|q| ps.sketch.to_world(q))
                                .collect();
                            got.push((ps.name.clone(), pts));
                        }
                    }
                    let normal = ps.sketch.plane.normal.into_inner();
                    let name = ps.name.clone();
                    if let Some(e) = self.sketch3d.as_mut() {
                        e.ins.curves.retain(|(n, _)| *n != name);
                        e.ins.curves.extend(got);
                        e.ins.sketch_normal = Some(normal);
                    }
                }
                true
            }
            Tool3::Silhouette => {
                if self.sk3_face_under(p, rect).is_none() {
                    return true;
                }
                let k = self.sketch3d.as_ref().map_or(3, |e| e.ins.sil_dir);
                let d = if k < 3 {
                    axis(k)
                } else {
                    self.camera.frame().0
                };
                let body = &self.part().body;
                let faces: Vec<Vec<(Tri, Vector3)>> = body
                    .faces()
                    .filter_map(|(_, f)| {
                        let m = f.mesh.as_ref()?;
                        Some(
                            m.triangles
                                .iter()
                                .filter_map(|t| {
                                    let t = t.map(|i| m.positions[i as usize]);
                                    let n =
                                        (t[1] - t[0]).cross(&(t[2] - t[0])).try_normalize(1e-14)?;
                                    Some((t, n))
                                })
                                .collect(),
                        )
                    })
                    .collect();
                let (_, size) = self.sk3_body_frame();
                let lines = curves::chain_segments(&curves::silhouette(&faces, d), 1e-5 * size);
                let steps = self.sketch3d.as_ref().map_or(24, |e| e.ins.steps);
                self.sk3_add_polylines("Silhouette Curve", lines, steps);
                true
            }
            _ => false,
        }
    }

    /// Mirror: a click on a curve or point of the sketch adds its points to
    /// what is mirrored; a click on a plane (a planar face, a work or origin
    /// plane) mirrors them in it.
    fn sk3_mirror_click(&mut self, p: Pos2, rect: Rect) {
        let Some(e) = self.sketch3d.as_ref() else {
            return;
        };
        let Some(s) = self.part().sketches3d.get(e.index) else {
            return;
        };
        let pr = |q: Point3| self.camera.project(&q, rect);
        let point = s
            .points
            .iter()
            .filter_map(|q| Some((pr(Point3::from(q.at))?.distance(p), q.id)))
            .filter(|(d, _)| *d <= super::overlay::POINT_SNAP_PX)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| vec![id]);
        let curve = || {
            s.segs()
                .into_iter()
                .filter_map(|(seg, a, b)| {
                    let poly = s.seg_piece(seg, a).ok()?;
                    let pts = crate::part::sketch3d::points_of(std::slice::from_ref(&poly));
                    let d = pts
                        .windows(2)
                        .filter_map(|w| Some(seg_dist(p, pr(w[0])?, pr(w[1])?)))
                        .fold(f32::MAX, f32::min);
                    let mut ids = vec![a, b];
                    match seg {
                        crate::part::sketch3d::SegId::Arc(id) => {
                            ids.extend(s.arcs.iter().find(|x| x.id == id).map(|x| x.via));
                        }
                        crate::part::sketch3d::SegId::Spline(id) => {
                            ids.extend(
                                s.splines
                                    .iter()
                                    .find(|x| x.id == id)
                                    .map(|x| x.points.clone())
                                    .unwrap_or_default(),
                            );
                        }
                        _ => {}
                    }
                    (d <= super::overlay::CURVE_SNAP_PX).then_some((d, ids))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, v)| v)
        };
        if let Some(ids) = point.or_else(curve) {
            if let Some(e) = self.sketch3d.as_mut() {
                for id in ids {
                    if !e.ins.mirror.contains(&id) {
                        e.ins.mirror.push(id);
                    }
                }
            }
            return;
        }
        let pts = e.ins.mirror.clone();
        if pts.is_empty() {
            self.file_status = Some(Err(
                "Mirror: click the curves to mirror first, then the mirror plane".into(),
            ));
            return;
        }
        let Some(r) = self.plane_under_pub(p, rect) else {
            return;
        };
        let part = self.part();
        let Ok(pl) = crate::part::placement::plane_of(&r, &part.body, &part.work) else {
            return;
        };
        self.edit3d("Mirror", move |s, e| {
            s.mirror(&pts, pl.origin, pl.normal.into_inner())?;
            e.ins.mirror.clear();
            Ok(())
        });
    }

    /// Equation Curve's Create: the curve as a fixed spline.
    fn sk3_equation_create(&mut self) {
        let Some(e) = self.sketch3d.as_ref() else {
            return;
        };
        let ins = e.ins.clone();
        let table = self.part().parameter_table();
        let vars = |n: &str| {
            table
                .iter()
                .find(|r| r.name == n)
                .map(crate::part::params::ParamRow::as_value)
        };
        let exprs = [ins.eq[0].as_str(), ins.eq[1].as_str(), ins.eq[2].as_str()];
        match curves::equation_points(ins.eq_kind, exprs, ins.t, ins.steps, &vars) {
            Ok(pts) => self.sk3_add_polylines("Equation Curve", vec![pts], ins.steps),
            Err(err) => self.file_status = Some(Err(err)),
        }
    }

    /// Project to Surface's Create: the curves onto the faces.
    fn sk3_project_create(&mut self) {
        let Some(e) = self.sketch3d.as_ref() else {
            return;
        };
        let ins = e.ins.clone();
        if ins.faces.is_empty() || ins.curves.is_empty() {
            self.file_status = Some(Err(
                "Project to Surface: pick the faces and a sketch's curves".into(),
            ));
            return;
        }
        let tris = curves::face_triangles(&self.part().body, &ins.faces);
        let how = if ins.closest {
            Projection::Closest
        } else if ins.dir < 3 {
            Projection::Along(axis(ins.dir))
        } else {
            Projection::Along(ins.sketch_normal.unwrap_or_else(Vector3::z))
        };
        let lines: Vec<Vec<Point3>> = ins.curves.iter().map(|(_, c)| c.clone()).collect();
        let out = curves::project_onto(&lines, &tris, how);
        self.sk3_add_polylines("Project to Surface", out, ins.steps);
        if let Some(e) = self.sketch3d.as_mut() {
            e.ins.curves.clear();
        }
    }

    /// The panel rows of the running insert tool.
    pub(super) fn sk3_insert_panel(
        &mut self,
        ui: &mut egui::Ui,
        e: &mut super::sketch3d::Sk3dEdit,
    ) {
        let mut create_eq = false;
        let mut create_proj = false;
        match e.tool {
            Some(Tool3::Equation) => {
                psection(ui, "sk3_eq", "Equation Curve", |ui| {
                    plabel(ui, "Type");
                    ui.horizontal_wrapped(|ui| {
                        for (k, n) in [
                            (EquationKind::Cartesian, "Cartesian"),
                            (EquationKind::Cylindrical, "Cylindrical"),
                            (EquationKind::Spherical, "Spherical"),
                        ] {
                            ui.selectable_value(&mut e.ins.eq_kind, k, n);
                        }
                    });
                    ui.end_row();
                    let names = match e.ins.eq_kind {
                        EquationKind::Cartesian => ["x(t)", "y(t)", "z(t)"],
                        EquationKind::Cylindrical => ["r(t)", "θ(t)", "z(t)"],
                        EquationKind::Spherical => ["r(t)", "θ(t)", "φ(t)"],
                    };
                    for (k, n) in names.iter().enumerate() {
                        plabel(ui, n);
                        ui.add(egui::TextEdit::singleline(&mut e.ins.eq[k]).desired_width(170.0))
                            .on_hover_text("An expression of t and the part's parameters (lengths in mm, angles in degrees when bare)");
                        ui.end_row();
                    }
                    plabel(ui, "t min");
                    ui.add(super::valuefield::ExprValue::new(&mut e.ins.t.0).cursor("t min"));
                    ui.end_row();
                    plabel(ui, "t max");
                    ui.add(super::valuefield::ExprValue::new(&mut e.ins.t.1).cursor("t max"));
                    ui.end_row();
                    plabel(ui, "");
                    if ui
                        .button("Create")
                        .on_hover_text("The curve as a spline through its points")
                        .clicked()
                    {
                        create_eq = true;
                    }
                    ui.end_row();
                });
            }
            Some(Tool3::ProjectSurface) => {
                psection(ui, "sk3_proj", "Project to Surface", |ui| {
                    plabel(ui, "Pick");
                    ui.horizontal_wrapped(|ui| {
                        ui.selectable_value(
                            &mut e.ins.pick_faces,
                            true,
                            format!("Faces ({})", e.ins.faces.len()),
                        )
                        .on_hover_text("Clicks add (or remove) the faces to project onto");
                        ui.selectable_value(
                            &mut e.ins.pick_faces,
                            false,
                            format!("Curves ({})", e.ins.curves.len()),
                        )
                        .on_hover_text("A click on a visible 2D sketch takes its curves");
                    });
                    ui.end_row();
                    plabel(ui, "Output");
                    ui.horizontal_wrapped(|ui| {
                        ui.selectable_value(&mut e.ins.closest, false, "Project along vector");
                        ui.selectable_value(&mut e.ins.closest, true, "Project to closest point");
                    });
                    ui.end_row();
                    if !e.ins.closest {
                        plabel(ui, "Direction");
                        ui.horizontal_wrapped(|ui| {
                            for (k, n) in ["X", "Y", "Z", "Sketch normal"].iter().enumerate() {
                                ui.selectable_value(&mut e.ins.dir, k, *n);
                            }
                        });
                        ui.end_row();
                    }
                    plabel(ui, "Wrap to surface");
                    ui.add_enabled(false, egui::Checkbox::new(&mut false, ""))
                        .on_disabled_hover_text("Comes later (spec 1.10 Project to Surface: Wrap)");
                    ui.end_row();
                    plabel(ui, "");
                    if ui
                        .add_enabled(
                            !e.ins.faces.is_empty() && !e.ins.curves.is_empty(),
                            egui::Button::new("Create"),
                        )
                        .clicked()
                    {
                        create_proj = true;
                    }
                    ui.end_row();
                });
            }
            Some(Tool3::Intersection) => {
                psection(ui, "sk3_inter", "Intersection Curve", |ui| {
                    plabel(ui, "First");
                    ui.label(
                        e.ins
                            .first
                            .as_ref()
                            .map_or("click a face or a plane".into(), |(n, _)| n.clone()),
                    );
                    ui.end_row();
                    plabel(ui, "Second");
                    ui.label("then a second face or plane");
                    ui.end_row();
                });
            }
            Some(Tool3::Silhouette) => {
                psection(ui, "sk3_sil", "Silhouette Curve", |ui| {
                    plabel(ui, "Direction");
                    ui.horizontal(|ui| {
                        for (k, n) in ["X", "Y", "Z", "View"].iter().enumerate() {
                            ui.selectable_value(&mut e.ins.sil_dir, k, *n);
                        }
                    });
                    ui.end_row();
                    plabel(ui, "Body");
                    ui.label("click a face of the body");
                    ui.end_row();
                });
            }
            Some(Tool3::Mirror) => {
                psection(ui, "sk3_mirror", "Mirror", |ui| {
                    plabel(ui, "Select");
                    ui.label(format!("{} point(s)", e.ins.mirror.len()));
                    ui.end_row();
                    plabel(ui, "Mirror plane");
                    ui.label("click a planar face, a work or origin plane");
                    ui.end_row();
                });
            }
            _ => return,
        }
        if matches!(
            e.tool,
            Some(Tool3::Equation | Tool3::ProjectSurface | Tool3::Intersection | Tool3::Silhouette)
        ) {
            psection(ui, "sk3_ins_steps", "Output", |ui| {
                plabel(ui, "Points");
                let mut n = e.ins.steps as Real;
                ui.add(
                    super::valuefield::ExprValue::new(&mut n)
                        .range(2.0..=200.0)
                        .cursor("Points"),
                );
                e.ins.steps = n.round().clamp(2.0, 200.0) as usize;
                ui.end_row();
            });
        }
        if create_eq || create_proj {
            self.sketch3d = Some(e.clone());
            if create_eq {
                self.sk3_equation_create();
            } else {
                self.sk3_project_create();
            }
            if let Some(x) = self.sketch3d.as_ref() {
                *e = x.clone();
            }
        }
    }
}

fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-9)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}
