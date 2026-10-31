//! Surface tools through the exact kernel (`docs/spec/5.4-surfaces.md`): SolidCraft's `stitch`,
//! `patch_edges`, `patch_region`, `trim` and `extend` on our [`ExactSolid`]s.
//!
//! Faces that come from an operand keep its names (the kernel's history); new faces are named
//! `<prefix>:<role>` (`Stitch1:Face`, `Patch1:Patch`).

use super::{ExactProfile, ExactSolid, Plane, Point3, Real, guarded, names_from, plane_of, sk, v3};

/// `body` with names: faces traced back to `operands` keep theirs, the rest are `new_face`.
fn named(
    body: sk::Body,
    operands: &[&ExactSolid],
    new_face: &str,
    name: Option<String>,
    sheet: bool,
) -> ExactSolid {
    let mut face_names = names_from(&body, operands, &|_| None);
    for n in face_names.iter_mut().filter(|n| n.is_none()) {
        *n = Some(new_face.to_owned());
    }
    ExactSolid {
        name,
        body,
        face_names,
        profiles: Vec::new(),
        sheet,
    }
}

/// **Stitch**: surface bodies sewn along edges that meet within `tol` (mm): one body named
/// `name`, a solid when the faces close a volume (unless `keep_surface`).
pub fn stitch(
    parts: &[&ExactSolid],
    tol: Real,
    keep_surface: bool,
    name: &str,
) -> Result<ExactSolid, String> {
    let bodies: Vec<&sk::Body> = parts.iter().map(|p| &p.body).collect();
    let body = guarded("stitch", || sk::stitch(&bodies, tol))?;
    let sheet = keep_surface || body.is_surface();
    Ok(named(
        body,
        parts,
        &format!("{name}:Face"),
        Some(name.to_owned()),
        sheet,
    ))
}

/// **Loft between edges**: a surface bridging an open chain of `a`'s edges and one of `b`'s
/// (each edge picked by a point on it), G0 to both, or tangent (G1) to the surface of a section
/// whose `weights` entry is `Some(weight)`: a new surface body `name` (one face
/// `<name>:Bridge`).
/// Whole closed boundaries of the two surfaces: one face per pair of sides, all sharing their
/// corner lines (one surface body).
pub fn bridge_edges(
    a: &ExactSolid,
    at_a: &[Point3],
    b: &ExactSolid,
    at_b: &[Point3],
    weights: [Option<Real>; 2],
    name: &str,
) -> Result<ExactSolid, String> {
    let pa: Vec<_> = at_a.iter().map(v3).collect();
    let pb: Vec<_> = at_b.iter().map(v3).collect();
    let body = guarded("bridge", || {
        sk::bridge_edges_tangent(&a.body, &pa, &b.body, &pb, weights)
    })?;
    Ok(named(
        body,
        &[],
        &format!("{name}:Bridge"),
        Some(name.to_owned()),
        true,
    ))
}

/// **Boundary Patch** of a closed loop of `s`'s edges, each picked by a point on it: a new
/// surface body `name` (one face `<name>:Patch`, planar when the loop is); `tangent`: G1 to
/// the faces the loop bounds, with that weight.
pub fn patch_edges(
    s: &ExactSolid,
    at: &[Point3],
    tangent: Option<Real>,
    name: &str,
) -> Result<ExactSolid, String> {
    let pts: Vec<_> = at.iter().map(v3).collect();
    let body = guarded("patch", || sk::patch_edges_tangent(&s.body, &pts, tangent))?;
    Ok(named(
        body,
        &[],
        &format!("{name}:Patch"),
        Some(name.to_owned()),
        true,
    ))
}

/// **Boundary Patch** of a sketch region: a planar surface body `name`.
pub fn patch_region(plane: &Plane, prof: &ExactProfile, name: &str) -> Result<ExactSolid, String> {
    let pl = plane_of(plane)?;
    let body = guarded("patch", || sk::patch_region(&pl, &prof.region))?;
    Ok(named(
        body,
        &[],
        &format!("{name}:Patch"),
        Some(name.to_owned()),
        true,
    ))
}

/// **Freeform** (spec 5.5): the cubic B-spline sheet of the control net `net` (rows along u): a
/// new surface body `name` (one face `<name>:Face`).
pub fn freeform_sheet(net: &[Vec<Point3>], name: &str) -> Result<ExactSolid, String> {
    let net: Vec<Vec<_>> = net.iter().map(|r| r.iter().map(v3).collect()).collect();
    let body = guarded("freeform", || sk::bspline_sheet(&net))?;
    Ok(named(
        body,
        &[],
        &format!("{name}:Face"),
        Some(name.to_owned()),
        true,
    ))
}

/// **Freeform Cylinder** (spec 5.5): the cubic B-spline tube of the rings `net[i]` (closed
/// around): a new surface body `name` (faces `<name>:Face`).
pub fn freeform_tube(net: &[Vec<Point3>], name: &str) -> Result<ExactSolid, String> {
    let net: Vec<Vec<_>> = net.iter().map(|r| r.iter().map(v3).collect()).collect();
    let body = guarded("freeform", || sk::bspline_tube(&net))?;
    Ok(named(
        body,
        &[],
        &format!("{name}:Face"),
        Some(name.to_owned()),
        true,
    ))
}

/// What trims a surface.
pub enum TrimBy<'a> {
    /// A plane (a work plane, an origin plane, a planar face), unbounded.
    Plane(&'a Plane),
    /// The surface of face `face` (kernel order) of another body, extended.
    Face(&'a ExactSolid, usize),
    /// Sketch curves on a plane, swept along its normal (each extended past its ends).
    Curves(&'a Plane, &'a [Vec<crate::kernel::ProfileSegment>]),
}

/// **Trim**: the pieces of surface body `s` on the side of `keep` of the tool are kept; they
/// keep their names.
pub fn trim(s: &ExactSolid, tool: &TrimBy, keep: Point3) -> Result<ExactSolid, String> {
    let body = match tool {
        TrimBy::Plane(p) => {
            let pl = plane_of(p)?;
            guarded("trim", || {
                sk::trim(&s.body, &sk::SplitTool::Plane(pl), v3(&keep))
            })?
        }
        TrimBy::Curves(p, chains) => {
            let pl = plane_of(p)?;
            let mut curves = Vec::new();
            for c in chains.iter() {
                let mut segs = Vec::new();
                for s in c {
                    segs.extend(super::seg2(&s.geometry)?);
                }
                curves.push(segs);
            }
            guarded("trim", || {
                sk::trim(
                    &s.body,
                    &sk::SplitTool::Curves { plane: pl, curves },
                    v3(&keep),
                )
            })?
        }
        TrimBy::Face(t, face) => guarded("trim", || {
            sk::trim(
                &s.body,
                &sk::SplitTool::Face {
                    body: &t.body,
                    face: *face,
                },
                v3(&keep),
            )
        })?,
    };
    Ok(named(body, &[s], "", s.name.clone(), true))
}

/// **Extend**: straight edges of a planar surface (picked by points on them) moved outward in
/// its plane by `distance`; faces keep their names.
pub fn extend(s: &ExactSolid, at: &[Point3], distance: Real) -> Result<ExactSolid, String> {
    let pts: Vec<_> = at.iter().map(v3).collect();
    let body = guarded("extend", || sk::extend(&s.body, &pts, distance))?;
    Ok(named(body, &[s], "", s.name.clone(), true))
}

/// Every face of `s` turned over (a closed body whose faces point inwards).
pub fn reversed(s: &ExactSolid) -> Result<ExactSolid, String> {
    let pts: Vec<_> = (0..s.body.face_count())
        .filter_map(|i| super::face_point(s, i))
        .map(|p| v3(&p))
        .collect();
    let body = guarded("reverse", || sk::reverse_faces(&s.body, &pts))?;
    Ok(named(body, &[s], "", s.name.clone(), s.sheet))
}

/// The kernel face count of a body.
pub fn face_count(s: &ExactSolid) -> usize {
    s.body.face_count()
}
