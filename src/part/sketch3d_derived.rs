//! **Associative 3D sketch curves** (`docs/spec/1.10-3d-sketch.md` §3,
//! Intersection / Silhouette / Project to Surface / Equation Curve): a curve
//! taken from the model keeps its definition — the inputs by persistent name
//! (faces, planes, 2D sketches) or the equation — and is computed again at
//! every regeneration ([`Sketch3d::update_derived`]). Its output is ordinary
//! fixed geometry of the sketch (lines and interpolation splines), so paths,
//! bends and constraints work on it unchanged; when the output keeps its shape
//! (same pieces, same point counts) the points move in place and keep their
//! ids. An input that is gone keeps the last geometry and is reported as the
//! sketch's health (rule 8). GUI-free.

use serde::{Deserialize, Serialize};

use super::placement::{PlaneRef, WorkGeom, plane_of};
use super::sketch3d::{SegId, Sketch3d};
use super::sketch3d_curves::{self as curves, EquationKind, Projection, Tri};
use crate::kernel::{BRep, FaceId, Point3, Real, Vector3};

/// A face-like input of an Intersection Curve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SurfaceInput {
    /// Faces of the body by persistent name (every piece with the name).
    Faces(Vec<String>),
    /// A plane (an origin or work plane, a planar face).
    Plane(PlaneRef),
}

/// How Project to Surface carries the curves onto the faces.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ProjectHow {
    /// Along a fixed direction (part coordinates), the nearer hit either way.
    Along([Real; 3]),
    /// Along the normal of the (first) sketch the curves come from, as it is now.
    SketchNormal,
    /// To the faces' closest point.
    Closest,
}

/// What a derived curve is computed from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum CurveDef {
    /// Where two surfaces meet.
    Intersection {
        /// The first input.
        a: SurfaceInput,
        /// The second input.
        b: SurfaceInput,
    },
    /// The outline of the part's bodies seen along `dir`.
    Silhouette {
        /// The direction (part coordinates).
        dir: [Real; 3],
    },
    /// The curves of 2D sketches projected onto faces.
    Project {
        /// The faces by persistent name.
        faces: Vec<String>,
        /// The 2D sketches (ids) whose curves are projected.
        sketches: Vec<String>,
        /// How.
        how: ProjectHow,
    },
    /// x(t), y(t), z(t) (or cylindrical / spherical) of the part's parameters.
    Equation {
        /// Which coordinates the expressions give.
        coords: EquationKind,
        /// The three expressions.
        exprs: [String; 3],
        /// t min and t max.
        t: (Real, Real),
    },
}

impl CurveDef {
    /// The command's name (messages, the browser).
    pub fn label(&self) -> &'static str {
        match self {
            CurveDef::Intersection { .. } => "Intersection Curve",
            CurveDef::Silhouette { .. } => "Silhouette Curve",
            CurveDef::Project { .. } => "Project to Surface",
            CurveDef::Equation { .. } => "Equation Curve",
        }
    }
}

/// One piece of a derived curve's output in the sketch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutCurve {
    /// A line (its id).
    Line(u32),
    /// An interpolation spline (its id).
    Spline(u32),
}

/// A curve of the sketch that follows its definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DerivedCurve {
    /// Its id in the sketch (from the sketch's ids).
    pub id: u32,
    /// What it is computed from.
    pub def: CurveDef,
    /// Points along a curved piece (Output › Points).
    pub steps: usize,
    /// The geometry it made, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub out: Vec<OutCurve>,
}

/// What the definitions read: the body at the sketch's place in history, the
/// work geometry, the 2D sketches and the parameters.
pub struct Inputs<'a> {
    /// The body.
    pub body: &'a BRep,
    /// Work planes etc.
    pub works: &'a WorkGeom,
    /// The part's 2D sketches.
    pub sketches: &'a [super::PartSketch],
    /// The part's parameters by name.
    pub vars: &'a crate::part::expr::Vars<'a>,
}

impl Inputs<'_> {
    /// The body's middle and size (planes become squares this large).
    fn frame(&self) -> (Point3, Real) {
        self.body
            .bounding_box()
            .map_or((Point3::origin(), 100.0), |(lo, hi)| {
                (lo + (hi - lo) / 2.0, (hi - lo).norm().max(10.0))
            })
    }
}

/// The faces of `body` with any of `names` (every piece); an error names a
/// face that is gone.
pub fn faces_named(body: &BRep, names: &[String]) -> Result<Vec<FaceId>, String> {
    let mut out = Vec::new();
    for n in names {
        let got: Vec<FaceId> = body
            .faces()
            .filter(|(_, f)| f.name.as_deref() == Some(n.as_str()))
            .map(|(id, _)| id)
            .collect();
        if got.is_empty() {
            return Err(format!("the face {n} is gone"));
        }
        out.extend(got);
    }
    Ok(out)
}

/// A plane as two large triangles (a square of half side `size` about the
/// point nearest `centre`), so it meets triangles like a face.
pub fn plane_tris(pl: &crate::kernel::Plane, centre: Point3, size: Real) -> Vec<Tri> {
    let n = pl.normal.into_inner();
    let c = centre - n * (centre - pl.origin).dot(&n);
    let (x, y) = (pl.x_axis.into_inner(), pl.y_axis().into_inner());
    let p = |a: Real, b: Real| c + x * (a * size) + y * (b * size);
    vec![
        [p(-1.0, -1.0), p(1.0, -1.0), p(1.0, 1.0)],
        [p(-1.0, -1.0), p(1.0, 1.0), p(-1.0, 1.0)],
    ]
}

/// Every face's display triangles with their normals (Silhouette Curve).
pub fn faces_with_normals(body: &BRep) -> Vec<Vec<(Tri, Vector3)>> {
    body.faces()
        .filter_map(|(_, f)| {
            let m = f.mesh.as_ref()?;
            Some(
                m.triangles
                    .iter()
                    .filter_map(|t| {
                        let t = t.map(|i| m.positions[i as usize]);
                        let n = (t[1] - t[0]).cross(&(t[2] - t[0])).try_normalize(1e-14)?;
                        Some((t, n))
                    })
                    .collect(),
            )
        })
        .collect()
}

/// The curves of 2D sketch `ps` (construction left out) as world polylines.
pub fn sketch_polylines(ps: &super::PartSketch) -> Vec<Vec<Point3>> {
    let mut got = Vec::new();
    for (id, _) in ps.sketch.entities() {
        if ps.sketch.style(id) == crate::sketch::Style::Construction {
            continue;
        }
        if let Some(c) = ps.sketch.curve_of(id) {
            got.push(
                c.sample(0.05)
                    .iter()
                    .map(|q| ps.sketch.to_world(q))
                    .collect(),
            );
        }
    }
    got
}

fn surface(inp: &Inputs<'_>, s: &SurfaceInput) -> Result<Vec<Tri>, String> {
    match s {
        SurfaceInput::Faces(names) => Ok(curves::face_triangles(
            inp.body,
            &faces_named(inp.body, names)?,
        )),
        SurfaceInput::Plane(r) => {
            let pl = plane_of(r, inp.body, inp.works).map_err(|e| format!("its plane: {e}"))?;
            let (c, size) = inp.frame();
            Ok(plane_tris(&pl, c, size))
        }
    }
}

/// The polylines of definition `def` (before they become sketch curves).
pub fn compute(def: &CurveDef, inp: &Inputs<'_>, steps: usize) -> Result<Vec<Vec<Point3>>, String> {
    let (_, size) = inp.frame();
    let lines = match def {
        CurveDef::Intersection { a, b } => {
            let (ta, tb) = (surface(inp, a)?, surface(inp, b)?);
            curves::chain_segments(&curves::mesh_intersection(&ta, &tb), 1e-5 * size)
        }
        CurveDef::Silhouette { dir } => {
            let d = Vector3::new(dir[0], dir[1], dir[2]);
            curves::chain_segments(
                &curves::silhouette(&faces_with_normals(inp.body), d),
                1e-5 * size,
            )
        }
        CurveDef::Project {
            faces,
            sketches,
            how,
        } => {
            let tris = curves::face_triangles(inp.body, &faces_named(inp.body, faces)?);
            let mut src = Vec::new();
            let mut normal = None;
            for id in sketches {
                let ps = inp
                    .sketches
                    .iter()
                    .find(|s| s.id == *id)
                    .ok_or_else(|| format!("the sketch {id} is gone"))?;
                normal.get_or_insert(ps.sketch.plane.normal.into_inner());
                src.extend(sketch_polylines(ps));
            }
            let how = match how {
                ProjectHow::Along(d) => Projection::Along(Vector3::new(d[0], d[1], d[2])),
                ProjectHow::SketchNormal => Projection::Along(normal.unwrap_or_else(Vector3::z)),
                ProjectHow::Closest => Projection::Closest,
            };
            curves::project_onto(&src, &tris, how)
        }
        CurveDef::Equation { coords, exprs, t } => {
            let e = [exprs[0].as_str(), exprs[1].as_str(), exprs[2].as_str()];
            vec![curves::equation_points(*coords, e, *t, steps, inp.vars)?]
        }
    };
    Ok(lines)
}

/// The polylines as the points of sketch curves: tiny ones dropped, a
/// straight one its two ends, any other `steps` + 1 points along it.
pub fn curve_points(lines: Vec<Vec<Point3>>, steps: usize, size: Real) -> Vec<Vec<Point3>> {
    let tol = 1e-6 * size;
    lines
        .into_iter()
        .filter(|l| l.len() >= 2 && l.windows(2).map(|w| (w[1] - w[0]).norm()).sum::<Real>() > tol)
        .map(|l| {
            if curves::is_straight(&l, tol.max(1e-6)) {
                vec![l[0], l[l.len() - 1]]
            } else {
                curves::resample(&l, steps.clamp(2, 200))
            }
        })
        .collect()
}

impl Sketch3d {
    fn out_points(&self, o: OutCurve) -> Option<Vec<u32>> {
        match o {
            OutCurve::Line(id) => self.line(id).map(|l| vec![l.a, l.b]),
            OutCurve::Spline(id) => self
                .splines
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.points.clone()),
        }
    }

    /// Adds curves through `pieces` (from [`curve_points`]) as fixed geometry
    /// that follows `def`; returns the derived curve's id.
    pub fn add_derived(
        &mut self,
        def: CurveDef,
        steps: usize,
        pieces: &[Vec<Point3>],
    ) -> Result<u32, String> {
        if pieces.is_empty() {
            return Err(format!("{}: no curve found", def.label()));
        }
        let mut next = self.clone();
        let out = next
            .make_pieces(pieces)
            .map_err(|e| format!("{}: {e}", def.label()))?;
        let id = next.next_id.max(1);
        next.next_id = id + 1;
        next.derived.push(DerivedCurve {
            id,
            def,
            steps,
            out,
        });
        *self = next;
        Ok(id)
    }

    fn make_pieces(&mut self, pieces: &[Vec<Point3>]) -> Result<Vec<OutCurve>, String> {
        let mut out = Vec::new();
        for p in pieces {
            out.push(match self.include(p, false)? {
                SegId::Line(id) => OutCurve::Line(id),
                SegId::Spline(id) => OutCurve::Spline(id),
                _ => return Err("an unexpected curve".into()),
            });
        }
        Ok(out)
    }

    /// Puts derived curve `k`'s output on `pieces`: the points move in place
    /// when the shape is the same (lines and splines keep their ids, so what
    /// is joined to them follows); otherwise the old curves (and the points
    /// and constraints only they used) are replaced.
    fn place_derived(&mut self, k: usize, pieces: &[Vec<Point3>]) -> Result<(), String> {
        let out = self.derived[k].out.clone();
        let old: Vec<Option<Vec<u32>>> = out.iter().map(|&o| self.out_points(o)).collect();
        let same = old.len() == pieces.len()
            && old.iter().zip(pieces).zip(&out).all(|((ids, p), o)| {
                ids.as_ref().is_some_and(|ids| ids.len() == p.len())
                    && matches!(o, OutCurve::Line(_)) == (p.len() == 2)
            });
        if same {
            for (ids, p) in old.iter().zip(pieces) {
                for (id, q) in ids.iter().flatten().zip(p) {
                    if let Some(pt) = self.points.iter_mut().find(|x| x.id == *id) {
                        pt.at = [q.x, q.y, q.z];
                    }
                }
            }
            return Ok(());
        }
        // Replace: drop the old curves, then the points nothing else uses.
        let lines: Vec<u32> = out
            .iter()
            .filter_map(|o| match o {
                OutCurve::Line(id) => Some(*id),
                _ => None,
            })
            .collect();
        let splines: Vec<u32> = out
            .iter()
            .filter_map(|o| match o {
                OutCurve::Spline(id) => Some(*id),
                _ => None,
            })
            .collect();
        self.lines.retain(|l| !lines.contains(&l.id));
        self.splines.retain(|s| !splines.contains(&s.id));
        self.constraints
            .retain(|c| !c.lines().iter().any(|l| lines.contains(l)));
        let olds: Vec<u32> = old.into_iter().flatten().flatten().collect();
        let used = |s: &Sketch3d, id: u32| {
            s.lines.iter().any(|l| l.a == id || l.b == id)
                || s.arcs.iter().any(|a| a.a == id || a.b == id || a.via == id)
                || s.splines.iter().any(|x| x.points.contains(&id))
                || s.helices
                    .iter()
                    .any(|h| h.center == id || h.start == id || h.end == id)
                || s.constraints.iter().any(|c| c.points().contains(&id))
        };
        let gone: Vec<u32> = olds.into_iter().filter(|&id| !used(self, id)).collect();
        self.points.retain(|p| !gone.contains(&p.id));
        self.bends.retain(|b| !gone.contains(&b.corner));
        let new = self.make_pieces(pieces)?;
        self.derived[k].out = new;
        Ok(())
    }

    /// Computes every derived curve again from `inp` and moves the sketch's
    /// geometry onto it, then solves the rest of the sketch. Returns the
    /// problems (an input gone, a curve not found): those curves keep their
    /// last geometry (rule 8).
    pub fn update_derived(&mut self, inp: &Inputs<'_>) -> Vec<String> {
        let mut problems = Vec::new();
        if self.derived.is_empty() {
            return problems;
        }
        let (_, size) = inp.frame();
        let before = self.clone();
        for k in 0..self.derived.len() {
            let d = &self.derived[k];
            let label = d.def.label();
            let pieces = match compute(&d.def, inp, d.steps) {
                Ok(lines) => curve_points(lines, d.steps, size),
                Err(e) => {
                    problems.push(format!("{label}: {e}"));
                    continue;
                }
            };
            if pieces.is_empty() {
                problems.push(format!("{label}: no curve found"));
                continue;
            }
            let keep = self.clone();
            if let Err(e) = self.place_derived(k, &pieces) {
                *self = keep;
                problems.push(format!("{label}: {e}"));
            }
        }
        if *self == before {
            return problems;
        }
        self.refresh_helices();
        let moved = self.clone();
        if self.solve().is_err() {
            // The constraints cannot follow: the curves move, the rest stays.
            *self = moved;
            problems.push("3D Sketch: the constraints cannot follow the moved curves".into());
        }
        problems
    }

    /// The derived curve that made sketch curve `seg`, if any.
    pub fn derived_of(&self, seg: SegId) -> Option<&DerivedCurve> {
        let o = match seg {
            SegId::Line(id) => OutCurve::Line(id),
            SegId::Spline(id) => OutCurve::Spline(id),
            _ => return None,
        };
        self.derived.iter().find(|d| d.out.contains(&o))
    }
}

#[cfg(test)]
#[path = "sketch3d_derived_tests.rs"]
mod tests;
