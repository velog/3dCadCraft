//! Surface bodies through the exact kernel (`docs/spec/2.13-combine-split.md`,
//! Extrude / Revolve with Output Surface): SolidCraft's `extrude_sheet` and
//! `revolve_sheet` make open sheets (no caps), kept as [`ExactSolid`]s whose
//! kernel body is a surface ([`ExactSolid::is_open`]). They never take part
//! in Booleans; Split uses them as tools ([`split_by_sheet`],
//! [`super::SplitBy::Sheet`]).
//!
//! Face names: `<prefix>:Side:<segment>` (from the kernel's history, like a
//! solid extrusion's walls).

use super::{
    ExactOp, ExactSolid, FaceOrigin, Plane, Point3, Real, Vector3, guarded, names_from, p3,
    plane_of, seg2, sg, sk, union_apart,
};
use crate::kernel::extrude::ProfileSegment;

impl ExactSolid {
    /// A surface body (an open sheet; Extrude / Revolve with Output
    /// Surface). Such bodies are never combined with solids.
    pub fn is_open(&self) -> bool {
        self.sheet || self.body.is_surface()
    }

    /// Whether every shell of the body closes (it bounds a volume), whatever
    /// its surface flag says.
    pub fn closes(&self) -> bool {
        !self.body.is_surface()
    }

    /// The body flagged as a surface body (`true`) or as a solid (`false`,
    /// only when it closes — Copy Object's Solid output).
    pub fn flagged(mut self, sheet: bool) -> Result<Self, String> {
        if !sheet && !self.closes() {
            return Err("the faces do not close a volume".into());
        }
        self.sheet = sheet;
        Ok(self)
    }

    /// The display-triangle corners of the whole body (kernel orientation),
    /// for [`SheetSide`].
    pub fn triangles(&self) -> Result<Vec<[Point3; 3]>, String> {
        let tol = (self.body.size() * 1e-3).max(1e-3);
        let mesh = guarded("tessellate", || self.body.tessellate(tol))?;
        Ok(mesh
            .triangles
            .iter()
            .filter_map(|t| mesh.tri(t).map(|[a, b, c]| [p3(a), p3(b), p3(c)]))
            .collect())
    }
}

/// Our chains as the kernel's segment chains, and the name of each kernel
/// segment (numbered across the chains, as the sheet builders number them).
fn chains_of(chains: &[Vec<ProfileSegment>]) -> Result<(Vec<Vec<sg::Seg2>>, Vec<String>), String> {
    let mut names = Vec::new();
    let mut out = Vec::new();
    for c in chains {
        let mut segs = Vec::new();
        for s in c {
            let pieces = seg2(&s.geometry)?;
            names.extend(std::iter::repeat_n(s.name.clone(), pieces.len()));
            segs.extend(pieces);
        }
        out.push(segs);
    }
    Ok((out, names))
}

fn named_sheet(body: sk::Body, names: &[String], prefix: &str) -> ExactSolid {
    let mut face_names = names_from(&body, &[], &|o| match o {
        FaceOrigin::Profile { segment } => {
            names.get(*segment).map(|n| format!("{prefix}:Side:{n}"))
        }
        _ => None,
    });
    // Without a history: by order (one face per segment as a rule).
    for (k, n) in face_names.iter_mut().enumerate() {
        if n.is_none() {
            *n = Some(format!(
                "{prefix}:Side:{}",
                names.get(k).cloned().unwrap_or_else(|| k.to_string())
            ));
        }
    }
    ExactSolid {
        sheet: true,
        name: Some(prefix.to_owned()),
        body,
        face_names,
        profiles: Vec::new(),
    }
}

/// A surface extruded from `chains` (open curves, or a profile's loops) on
/// `plane` between the offsets `start` and `end` along its normal: the walls
/// only, no caps. Faces `<prefix>:Side:<segment>`.
pub fn extrude_sheet(
    plane: &Plane,
    chains: &[Vec<ProfileSegment>],
    start: Real,
    end: Real,
    prefix: &str,
) -> Result<ExactSolid, String> {
    let pl = plane_of(plane)?;
    let (segs, names) = chains_of(chains)?;
    let body = guarded("surface extrude", || {
        sk::extrude_sheet(&pl, &segs, start, end)
    })?;
    Ok(named_sheet(body, &names, prefix))
}

/// A surface revolved from `chains` on `plane` about the sketch line through
/// `axis_origin` along `axis_dir`, from `start` through `angle` radians, in
/// the same sense as [`super::revolve`] turns a profile. Faces
/// `<prefix>:Side:<segment>`.
#[allow(clippy::too_many_arguments)]
pub fn revolve_sheet(
    plane: &Plane,
    chains: &[Vec<ProfileSegment>],
    axis_origin: nalgebra::Point2<Real>,
    axis_dir: nalgebra::Vector2<Real>,
    start: Real,
    angle: Real,
    sense: Real,
    prefix: &str,
) -> Result<ExactSolid, String> {
    let pl = plane_of(plane)?;
    let (segs, names) = chains_of(chains)?;
    let angle = angle.clamp(-std::f64::consts::TAU, std::f64::consts::TAU);
    let body = guarded("surface revolve", || {
        sk::revolve_sheet(
            &pl,
            &segs,
            sg::Vec2::new(axis_origin.x, axis_origin.y),
            sg::Vec2::new(axis_dir.x, axis_dir.y),
            start * sense,
            angle * sense,
        )
    })?;
    Ok(named_sheet(body, &names, prefix))
}

/// A surface swept from `chains` on `plane` straight along `along` (any direction, not only
/// the plane's normal): a Ruled Surface of the Normal / Sweep type. Faces
/// `<prefix>:Side:<segment>`.
pub fn sweep_sheet_along(
    plane: &Plane,
    chains: &[Vec<ProfileSegment>],
    along: Vector3,
    prefix: &str,
) -> Result<ExactSolid, String> {
    let pl = plane_of(plane)?;
    let (segs, names) = chains_of(chains)?;
    let start = chains
        .first()
        .and_then(|c| c.first())
        .map(|s| s.geometry.start())
        .ok_or_else(|| "no curves".to_owned())?;
    let a =
        plane.origin + plane.x_axis.into_inner() * start.x + plane.y_axis().into_inner() * start.y;
    let b = a + along;
    let body = guarded("ruled surface", || {
        sk::sweep_sheet(
            &pl,
            &segs,
            &[sk::PathSeg::Line {
                a: super::v3(&a),
                b: super::v3(&b),
            }],
        )
    })?;
    Ok(named_sheet(body, &names, prefix))
}

/// Which side of a surface body a point is on: the signed distance to the
/// nearest of its triangles, along that triangle's normal (the kernel's face
/// orientation). Points beyond the sheet's border take the side of the
/// nearest border triangle, as if the sheet went on.
#[derive(Clone, Debug, Default)]
pub struct SheetSide {
    tris: Vec<[Point3; 3]>,
}

impl SheetSide {
    /// From the sheet's triangles (oriented).
    pub fn new(tris: Vec<[Point3; 3]>) -> Self {
        Self {
            tris: tris
                .into_iter()
                .filter(|[a, b, c]| (b - a).cross(&(c - a)).norm() > 1e-18)
                .collect(),
        }
    }

    /// No triangles.
    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    /// The nearest point of the sheet and the unit normal there.
    pub fn nearest(&self, p: &Point3) -> Option<(Point3, Vector3)> {
        let mut best: Option<(Real, Point3, Vector3)> = None;
        for [a, b, c] in &self.tris {
            let q = closest_on_triangle(p, a, b, c);
            let d = (p - q).norm_squared();
            if best.as_ref().is_none_or(|(x, _, _)| d < *x) {
                let n = (b - a).cross(&(c - a)).normalize();
                best = Some((d, q, n));
            }
        }
        best.map(|(_, q, n)| (q, n))
    }

    /// Signed distance: positive in front (along the normal).
    pub fn signed(&self, p: &Point3) -> Real {
        // Among triangles about as near, the one whose normal sees the point
        // most squarely decides (at a crease the nearest point is shared).
        let mut best: Option<(Real, Real)> = None;
        for [a, b, c] in &self.tris {
            let q = closest_on_triangle(p, a, b, c);
            let d = (p - q).norm();
            let n = (b - a).cross(&(c - a)).normalize();
            let s = (p - q).dot(&n);
            match best {
                Some((bd, _)) if d > bd + 1e-9 * (1.0 + bd) => {}
                Some((bd, bs)) if (d - bd).abs() <= 1e-9 * (1.0 + bd) && s.abs() <= bs.abs() => {}
                _ => best = Some((d, s)),
            }
        }
        best.map_or(0.0, |(_, s)| s)
    }
}

/// The point of triangle `abc` nearest `p` (Ericson, *Real-Time Collision
/// Detection*, 5.1.5).
pub(super) fn closest_on_triangle(p: &Point3, a: &Point3, b: &Point3, c: &Point3) -> Point3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(&ap), ac.dot(&ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return *a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(&bp), ac.dot(&bp));
    if d3 >= 0.0 && d4 <= d3 {
        return *b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(&cp), ac.dot(&cp));
    if d6 >= 0.0 && d5 <= d6 {
        return *c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// Which side of `side` the part `body` lies on: the edge midpoint (or
/// vertex) farthest from the sheet decides.
fn ahead_of(body: &sk::Body, side: &SheetSide) -> Result<bool, String> {
    let tol = (body.size() * 1e-2).max(1e-3);
    let edges = guarded("edges", || body.edges(tol))?;
    Ok(edges
        .iter()
        .map(|e| side.signed(&p3(e.mid)))
        .max_by(|x, y| x.abs().total_cmp(&y.abs()))
        .is_some_and(|d| d > 0.0))
}

/// Split Solid / Trim Solid by a surface body (SolidCraft's `split_body`
/// with each face of `sheet` in turn, its surface extended): the parts
/// behind (`.0`) and in front of (`.1`) the sheet, as [`SheetSide`] tells.
/// Section faces are `<back>:Cut` and `<front>:Cut`; every other face keeps
/// its name; both parts keep the body's name. Pieces a face's extension cut
/// off on the same side are joined again.
pub fn split_by_sheet(
    s: &ExactSolid,
    sheet: &ExactSolid,
    side: &SheetSide,
    back: &str,
    front: &str,
) -> Result<(Option<ExactSolid>, Option<ExactSolid>), String> {
    if side.is_empty() {
        return Err("the surface has no area".into());
    }
    let tmp = format!("{back}~section");
    // Every part split by every face of the sheet.
    let mut parts: Vec<ExactSolid> = vec![s.clone()];
    // Faces that do not reach the body's box are left out: only their
    // extended surface could meet it (and a degenerate disc of a revolved
    // sheet cannot even be evaluated).
    let body_box = super::bounds(s);
    let boxes = face_boxes(sheet)?;
    // Faces on a surface already used are skipped: a full revolve comes in
    // sectors of one cylinder, and splitting a part again along its own
    // section never returned.
    let kinds = guarded("face surfaces", || Ok(sk::face_surfaces(&sheet.body)))?;
    let tol = 1e-7 * (1.0 + sheet.body.size());
    let mut used: Vec<sk::FaceSurface> = Vec::new();
    for face in 0..sheet.body.face_count() {
        if let Some(Some(k)) = kinds.get(face) {
            if used.iter().any(|u| same_surface(u, k, tol)) {
                continue;
            }
            used.push(*k);
        }
        if let (Some((lo, hi)), Some(Some((a, b)))) = (body_box, boxes.get(face)) {
            let eps = 1e-6 * (1.0 + (hi - lo).norm());
            if (0..3).any(|k| a[k] > hi[k] + eps || b[k] < lo[k] - eps) {
                continue;
            }
        }
        let mut next = Vec::new();
        for p in parts {
            let fast = kinds.get(face).copied().flatten().and_then(analytic_tool);
            let tool = match &fast {
                Some(Analytic::Plane(pl)) => sk::SplitTool::Plane(*pl),
                Some(Analytic::Cylinder(pl, arc)) => sk::SplitTool::Curves {
                    plane: *pl,
                    curves: vec![vec![*arc]],
                },
                None => sk::SplitTool::Face {
                    body: &sheet.body,
                    face,
                },
            };
            let pieces = guarded("split", || sk::split_body(&p.body, &tool))?;
            if pieces.len() < 2 {
                next.push(p);
                continue;
            }
            for body in pieces {
                let face_names = names_from(&body, &[&p], &|_| None)
                    .into_iter()
                    .map(|n| n.or_else(|| Some(tmp.clone())))
                    .collect();
                next.push(ExactSolid {
                    sheet: false,
                    name: s.name.clone(),
                    body,
                    face_names,
                    profiles: s.profiles.clone(),
                });
            }
        }
        parts = next;
    }
    if parts.len() < 2 {
        return Ok(if ahead_of(&s.body, side)? {
            (None, Some(s.clone()))
        } else {
            (Some(s.clone()), None)
        });
    }
    let (mut b, mut f): (Vec<ExactSolid>, Vec<ExactSolid>) = (Vec::new(), Vec::new());
    for p in parts {
        if ahead_of(&p.body, side)? {
            f.push(p);
        } else {
            b.push(p);
        }
    }
    let join = |v: Vec<ExactSolid>, cut: &str| -> Result<Option<ExactSolid>, String> {
        let mut acc: Option<ExactSolid> = None;
        if v.len() > 1
            && let Some(u) = union_apart(&v)?
        {
            acc = Some(u);
        } else {
            for p in v {
                acc = Some(match acc {
                    None => p,
                    Some(a) => super::boolean(&a, &p, ExactOp::Union)?
                        .ok_or_else(|| "split: a part came out empty".to_owned())?,
                });
            }
        }
        Ok(acc.map(|mut r| {
            r.rename_prefix(&tmp, &format!("{cut}:Cut"));
            r.name = s.name.clone();
            r
        }))
    };
    Ok((join(b, back)?, join(f, front)?))
}

/// Split Face of `faces` of `body` by face `face` of `sheet` (its surface
/// extended; a plane or a cylinder through the kernel's fast tools).
pub(super) fn split_faces_by(
    body: &sk::Body,
    faces: &[usize],
    sheet: &ExactSolid,
    face: usize,
) -> Result<(sk::Body, Vec<usize>), String> {
    let kinds = guarded("face surfaces", || Ok(sk::face_surfaces(&sheet.body)))?;
    let fast = kinds.get(face).copied().flatten().and_then(analytic_tool);
    let tool = match &fast {
        Some(Analytic::Plane(pl)) => sk::SplitTool::Plane(*pl),
        Some(Analytic::Cylinder(pl, arc)) => sk::SplitTool::Curves {
            plane: *pl,
            curves: vec![vec![*arc]],
        },
        None => sk::SplitTool::Face {
            body: &sheet.body,
            face,
        },
    };
    guarded("split face", || {
        sk::split_faces_with_map(body, faces, &tool)
    })
}

/// A plane or a cylinder as the kernel's fast split tools (a revolved face
/// through `SplitTool::Face` is sampled: 10 s for one cylinder).
enum Analytic {
    Plane(sg::Plane),
    /// The plane square to the axis and the full circle on it.
    Cylinder(sg::Plane, sg::Seg2),
}

fn analytic_tool(k: sk::FaceSurface) -> Option<Analytic> {
    let frame = |o: sg::Vec3, n: sg::Vec3| {
        let n = Vector3::new(n.x, n.y, n.z).try_normalize(1e-15)?;
        let t = if n.x.abs() < 0.9 {
            Vector3::x()
        } else {
            Vector3::y()
        };
        let x = (t - n * t.dot(&n)).normalize();
        let y = n.cross(&x);
        sg::Plane::new(
            o,
            sg::Vec3::new(x.x, x.y, x.z),
            sg::Vec3::new(y.x, y.y, y.z),
        )
    };
    match k {
        sk::FaceSurface::Plane { normal, point } => frame(point, normal).map(Analytic::Plane),
        sk::FaceSurface::Cylinder {
            origin,
            axis,
            radius,
        } if radius > 0.0 => frame(origin, axis).map(|pl| {
            Analytic::Cylinder(
                pl,
                sg::Seg2::Arc {
                    center: sg::Vec2::new(0.0, 0.0),
                    radius,
                    start: 0.0,
                    sweep: std::f64::consts::TAU,
                },
            )
        }),
        _ => None,
    }
}

/// The faces of `sheet` on distinct surfaces (the first face of each; a
/// face whose surface is not recognised counts on its own).
pub(super) fn distinct_faces(sheet: &ExactSolid) -> Result<Vec<usize>, String> {
    let kinds = guarded("face surfaces", || Ok(sk::face_surfaces(&sheet.body)))?;
    let tol = 1e-7 * (1.0 + sheet.body.size());
    let mut used: Vec<sk::FaceSurface> = Vec::new();
    let mut out = Vec::new();
    for face in 0..sheet.body.face_count() {
        if let Some(Some(k)) = kinds.get(face) {
            if used.iter().any(|u| same_surface(u, k, tol)) {
                continue;
            }
            used.push(*k);
        }
        out.push(face);
    }
    Ok(out)
}

/// Whether two analytic surfaces are the same (whatever their faces' extent).
fn same_surface(a: &sk::FaceSurface, b: &sk::FaceSurface, tol: Real) -> bool {
    use sk::FaceSurface as F;
    let v = |x: sg::Vec3| Vector3::new(x.x, x.y, x.z);
    let parallel = |x: sg::Vec3, y: sg::Vec3| {
        let (x, y) = (v(x), v(y));
        match (x.try_normalize(1e-15), y.try_normalize(1e-15)) {
            (Some(x), Some(y)) => x.cross(&y).norm() < 1e-9,
            _ => false,
        }
    };
    let near = |x: sg::Vec3, y: sg::Vec3| (v(x) - v(y)).norm() < tol;
    match (a, b) {
        (
            F::Plane {
                normal: n1,
                point: p1,
            },
            F::Plane {
                normal: n2,
                point: p2,
            },
        ) => {
            let n = v(*n1).normalize();
            parallel(*n1, *n2) && (v(*p1) - v(*p2)).dot(&n).abs() < tol
        }
        (
            F::Cylinder {
                origin: o1,
                axis: a1,
                radius: r1,
            },
            F::Cylinder {
                origin: o2,
                axis: a2,
                radius: r2,
            },
        ) => {
            let a = v(*a1).normalize();
            let d = v(*o2) - v(*o1);
            parallel(*a1, *a2) && (r1 - r2).abs() < tol && (d - a * d.dot(&a)).norm() < tol
        }
        (
            F::Cone {
                apex: p1,
                axis: a1,
                half_angle: h1,
            },
            F::Cone {
                apex: p2,
                axis: a2,
                half_angle: h2,
            },
        ) => near(*p1, *p2) && parallel(*a1, *a2) && (h1 - h2).abs() < 1e-9,
        (
            F::Sphere {
                center: c1,
                radius: r1,
            },
            F::Sphere {
                center: c2,
                radius: r2,
            },
        ) => near(*c1, *c2) && (r1 - r2).abs() < tol,
        (
            F::Torus {
                center: c1,
                axis: a1,
                major: m1,
                minor: n1,
            },
            F::Torus {
                center: c2,
                axis: a2,
                major: m2,
                minor: n2,
            },
        ) => near(*c1, *c2) && parallel(*a1, *a2) && (m1 - m2).abs() < tol && (n1 - n2).abs() < tol,
        _ => false,
    }
}

/// The box of each face's display triangles (kernel face order).
fn face_boxes(s: &ExactSolid) -> Result<Vec<Option<(Point3, Point3)>>, String> {
    let tol = (s.body.size() * 1e-3).max(1e-3);
    let mesh = guarded("tessellate", || s.body.tessellate(tol))?;
    let mut out: Vec<Option<(Point3, Point3)>> = vec![None; s.body.face_count()];
    for (t, &f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        let (Some(tri), Some(slot)) = (mesh.tri(t), out.get_mut(f as usize)) else {
            continue;
        };
        for v in tri {
            let p = p3(v);
            *slot = Some(match *slot {
                None => (p, p),
                Some((a, b)) => (a.inf(&p), b.sup(&p)),
            });
        }
    }
    Ok(out)
}

/// The point of `sheet` nearest `p` and the sheet's normal there (the Trim
/// arrow).
pub fn sheet_nearest(sheet: &ExactSolid, p: Point3) -> Option<(Point3, Vector3)> {
    SheetSide::new(sheet.triangles().ok()?).nearest(&p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::Segment2;
    use nalgebra::Point2;

    fn line(a: (Real, Real), b: (Real, Real), n: &str) -> ProfileSegment {
        ProfileSegment {
            geometry: Segment2::Line {
                a: Point2::new(a.0, a.1),
                b: Point2::new(b.0, b.1),
            },
            name: n.into(),
        }
    }

    #[test]
    fn extruded_line_is_an_open_named_sheet() {
        let sh = extrude_sheet(
            &Plane::xy(),
            &[vec![line((-10.0, 5.0), (60.0, 5.0), "L")]],
            -5.0,
            30.0,
            "Extrusion2",
        )
        .unwrap();
        assert!(sh.is_open());
        assert_eq!(sh.face_names, vec![Some("Extrusion2:Side:L".to_owned())]);
        let b = super::super::to_brep(&[sh]).unwrap();
        let (_, solid) = b.solids().next().unwrap();
        assert!(solid.open);
    }

    #[test]
    fn a_plane_sheet_splits_a_box_like_the_plane() {
        let bx = super::super::box_named(Point3::origin(), Point3::new(40.0, 30.0, 20.0), "Box1")
            .unwrap();
        let sh = extrude_sheet(
            &Plane::xy(),
            &[vec![line((-10.0, 10.0), (60.0, 10.0), "L")]],
            -5.0,
            30.0,
            "Extrusion2",
        )
        .unwrap();
        let side = SheetSide::new(sh.triangles().unwrap());
        let (b, f) = split_by_sheet(&bx, &sh, &side, "Split1", "Split1#2").unwrap();
        let (b, f) = (b.unwrap(), f.unwrap());
        let vb = b.measure().unwrap().0;
        let vf = f.measure().unwrap().0;
        assert!((vb + vf - 24000.0).abs() < 1e-6, "{vb} {vf}");
        let mut v = [vb, vf];
        v.sort_by(Real::total_cmp);
        assert!(
            (v[0] - 8000.0).abs() < 1e-6 && (v[1] - 16000.0).abs() < 1e-6,
            "{v:?}"
        );
        assert!(b.face_names.iter().flatten().any(|n| n == "Split1:Cut"));
        assert!(f.face_names.iter().flatten().any(|n| n == "Split1#2:Cut"));
    }
}
