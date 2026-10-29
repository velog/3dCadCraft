//! Convert to Sheet Metal of a body that is already sheet metal (an imported
//! part, spec 5.6 §6.1e): from the fixed face the sheet's panels are walked
//! across its round bends — a cylinder whose axis runs along the edge it
//! meets — and the sheet model is built from them: the fixed face is the
//! base, every further panel a flange with the bend's own radius and angle,
//! its outline and holes taken over (Inventor detects the thickness and the
//! bends the same way). The model is folded again and must give the body's
//! volume; else the body is no sheet of constant thickness, or has features
//! the walk does not know yet.

use std::collections::HashSet;
use std::sync::Arc;

use solidcraft_doc::sheet as sd;
use solidcraft_geom::{Loop2, Plane as SPlane, Region2, Seg2, Vec2, Vec3};

use super::{ExactSolid, RuleValues, SheetModel, named_solid, rule_of};
use crate::kernel::{BRep, Curve, EdgeId, FaceId, Loop, Point3, Real, Sense, Surface, Vector3};

/// Most panels one walk takes (a loop of bends never ends otherwise).
const MAX_PANELS: usize = 500;

fn s3(p: Point3) -> Vec3 {
    Vec3::new(p.x, p.y, p.z)
}

fn d3(v: Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn snapped(v: Vec2) -> Vec2 {
    let g = |x: f64| (x * 1e9).round() / 1e9;
    Vec2::new(g(v.x), g(v.y))
}

/// A planar face's outward normal and a point on it.
fn plane_of(b: &BRep, f: FaceId) -> Option<(Vec3, Vec3)> {
    let face = b.face(f)?;
    let Surface::Plane(pl) = b.surface(face.surface)? else {
        return None;
    };
    let n = d3(pl.normal.into_inner());
    let n = if face.sense == Sense::Same {
        n
    } else {
        n * -1.0
    };
    Some((n, s3(pl.origin)))
}

/// The point of `v`.
fn at(b: &BRep, v: crate::kernel::VertexId) -> Option<Vec3> {
    b.vertex(v).map(|x| s3(x.point))
}

/// A face loop in the 2D coordinates of `pl` (lines and arcs).
fn loop2(b: &BRep, lp: &Loop, pl: &SPlane) -> Result<Loop2, String> {
    let n = pl.normal();
    let to2 = |q: Vec3| {
        let d = q - pl.origin;
        Vec2::new(d.dot(pl.x), d.dot(pl.y))
    };
    let mut segs = Vec::new();
    for ce in &lp.coedges {
        let e = b.edge(ce.edge).ok_or("edge")?;
        let (s, t) = (
            at(b, e.start).ok_or("vertex")?,
            at(b, e.end).ok_or("vertex")?,
        );
        let (s, t) = if ce.sense == Sense::Same {
            (s, t)
        } else {
            (t, s)
        };
        match b.curve(e.curve).ok_or("curve")? {
            Curve::Line(_) => segs.push(Seg2::Line {
                a: to2(s),
                b: to2(t),
            }),
            Curve::Circle(c) => {
                let centre = to2(s3(c.center));
                let start = (to2(s) - centre).angle();
                let turn = e.range.max - e.range.min;
                let ccw = d3(c.normal.into_inner()).dot(n) > 0.0;
                let sweep = if ccw == (ce.sense == Sense::Same) {
                    turn
                } else {
                    -turn
                };
                segs.push(Seg2::Arc {
                    center: centre,
                    radius: c.radius,
                    start,
                    sweep,
                });
            }
            Curve::BSpline(_) => {
                return Err("not yet: free-form edges on the sheet's faces".into());
            }
        }
    }
    Ok(Loop2 { segs })
}

/// A plane frame on face `f` (outward normal `n`, a point `o` on it).
fn frame_on(n: Vec3, o: Vec3) -> SPlane {
    let x = n.any_perp().normalized().unwrap_or(Vec3::X);
    SPlane {
        origin: o,
        x,
        y: n.cross(x),
    }
}

/// The face's region (outer loop and holes) in `pl`.
fn region_of(b: &BRep, f: FaceId, pl: &SPlane) -> Result<Region2, String> {
    let face = b.face(f).ok_or("face")?;
    let outer = loop2(b, &face.outer, pl)?.ccw();
    let mut holes = Vec::new();
    for h in &face.inner {
        holes.push(loop2(b, h, pl)?.ccw().reversed());
    }
    Ok(Region2 { outer, holes })
}

/// A straight edge's ends.
fn line_ends(b: &BRep, e: EdgeId) -> Option<(Vec3, Vec3)> {
    let ed = b.edge(e)?;
    matches!(b.curve(ed.curve)?, Curve::Line(_)).then_some(())?;
    Some((at(b, ed.start)?, at(b, ed.end)?))
}

/// The face across edge `e` from `f`.
fn across(b: &BRep, f: FaceId, e: EdgeId) -> Option<FaceId> {
    b.edge_faces(e).into_iter().find(|&g| g != f)
}

/// The straight edges of a face's outer loop.
fn straight_edges(b: &BRep, f: FaceId) -> Vec<EdgeId> {
    b.face(f)
        .map(|face| {
            face.outer
                .coedges
                .iter()
                .map(|c| c.edge)
                .filter(|&e| line_ends(b, e).is_some())
                .collect()
        })
        .unwrap_or_default()
}

/// A bend found from a panel's edge: the cylinder pieces crossed, the face
/// on the far side (the next panel's face on the same skin), the radius of
/// the cylinder on this skin, its axis direction and a point on it.
struct Crossing {
    pieces: Vec<FaceId>,
    next: FaceId,
    radius: Real,
    axis_o: Vec3,
    axis: Vec3,
}

/// From panel face `f` across its straight edge `e`: a round bend (one or
/// more cylinder pieces of one axis running along `e`) to the next planar
/// face, or `None` (a rim face, a hole wall, anything else).
fn crossing(b: &BRep, f: FaceId, e: EdgeId) -> Option<Crossing> {
    let (ea, eb) = line_ends(b, e)?;
    let dir = (eb - ea).normalized()?;
    let mut cur = across(b, f, e)?;
    let mut via = e;
    let mut pieces = Vec::new();
    let mut cyl: Option<(Vec3, Vec3, Real)> = None;
    for _ in 0..16 {
        let face = b.face(cur)?;
        match b.surface(face.surface)? {
            Surface::Cylinder(c) => {
                let (o, a, r) = (s3(c.origin), d3(c.axis.into_inner()), c.radius);
                if a.cross(dir).len() > 1e-6 {
                    return None;
                }
                if let Some((o0, a0, r0)) = cyl {
                    let off = (o - o0) - a0 * (o - o0).dot(a0);
                    if (r - r0).abs() > 1e-6 || off.len() > 1e-6 || a.cross(a0).len() > 1e-6 {
                        return None;
                    }
                } else {
                    cyl = Some((o, a, r));
                }
                pieces.push(cur);
                // On to the straight edge along the axis on the piece's far side.
                let far = face
                    .outer
                    .coedges
                    .iter()
                    .map(|c| c.edge)
                    .filter(|&g| g != via)
                    .find(|&g| {
                        line_ends(b, g).is_some_and(|(p, q)| {
                            (q - p)
                                .normalized()
                                .is_some_and(|d| d.cross(dir).len() < 1e-6)
                                && (p - ea).cross(dir).len() > 1e-6
                        })
                    })?;
                via = far;
                cur = across(b, cur, far)?;
            }
            Surface::Plane(_) if !pieces.is_empty() => {
                let (o, a, r) = cyl?;
                return Some(Crossing {
                    pieces,
                    next: cur,
                    radius: r,
                    axis_o: o,
                    axis: a,
                });
            }
            _ => return None,
        }
    }
    None
}

/// The thickness: the nearest planar face facing away from `f` behind it.
fn thickness(b: &BRep, f: FaceId) -> Option<Real> {
    let (n, o) = plane_of(b, f)?;
    b.faces()
        .filter(|(g, _)| *g != f)
        .filter_map(|(g, _)| {
            let (m, q) = plane_of(b, g)?;
            (m.dot(n) < -1.0 + 1e-9).then(|| (o - q).dot(n))
        })
        .filter(|d| *d > 1e-6)
        .min_by(|a, b| a.total_cmp(b))
}

/// The sheet model of `solid` (an imported sheet metal part) with the planar
/// face containing `face` as its base; faces named under `prefix`.
pub fn recognize(
    solid: &ExactSolid,
    prefix: &str,
    rv: &RuleValues,
    face: Point3,
) -> Result<(ExactSolid, SheetModel), String> {
    let b = super::super::to_brep(std::slice::from_ref(solid))?;
    let p = s3(face);
    // The base: the planar face whose plane holds the point, nearest inside.
    let base = b
        .faces()
        .filter_map(|(f, _)| {
            let (n, o) = plane_of(&b, f)?;
            let d = (p - o).dot(n).abs();
            (d < 1e-4).then_some(f)
        })
        .find(|&f| {
            plane_of(&b, f).is_some_and(|(n, o)| {
                let pl = frame_on(n, o);
                region_of(&b, f, &pl).is_ok_and(|r| {
                    let d = p - pl.origin;
                    let q = Vec2::new(d.dot(pl.x), d.dot(pl.y));
                    let poly = r.outer.polyline(1e-3);
                    point_in(&poly, q)
                })
            })
        })
        .ok_or("pick a planar face of the sheet as its base")?;
    let t = thickness(&b, base)
        .ok_or("no face of the sheet's other side found behind the base face")?;
    let mut rv = *rv;
    rv.thickness = t;
    let (n0, o0) = plane_of(&b, base).ok_or("base face")?;
    // The flat frame: on the base face, z into the material.
    let frame = frame_on(n0 * -1.0, o0);
    let region = region_of(&b, base, &frame)?;
    let mut sheet = sd::base_flange(prefix, "rule", &rule_of(&rv), frame, region);
    let mut seen: HashSet<FaceId> = HashSet::new();
    seen.insert(base);
    // Panels to walk on from: (face, its panel, its outward normal).
    let mut todo: Vec<(FaceId, Option<usize>, Vec3)> = vec![(base, None, n0)];
    while let Some((f, panel, nf)) = todo.pop() {
        if sheet.flanges.len() > MAX_PANELS {
            return Err("too many bends for one sheet".into());
        }
        let inv = solidcraft_doc::mat_inverse(&sheet.world(panel)).ok_or("panel frame")?;
        let flat = |q: Vec3| {
            let r = solidcraft_doc::apply_point(&inv, q);
            Vec2::new(r.x, r.y)
        };
        // A point inside the panel, in the flat (to tell its outward side).
        let inside = {
            let pl = frame_on(nf, plane_of(&b, f).ok_or("face")?.1);
            let r = region_of(&b, f, &pl)?;
            let poly = r.outer.polyline(1e-2);
            let c = poly.iter().fold(Vec2::new(0.0, 0.0), |s, q| s + *q)
                * (1.0 / poly.len().max(1) as f64);
            flat(pl.to_world(c))
        };
        for e in straight_edges(&b, f) {
            let Some(cr) = crossing(&b, f, e) else {
                continue;
            };
            if cr.pieces.iter().any(|g| seen.contains(g)) || seen.contains(&cr.next) {
                continue;
            }
            let (ea, eb) = line_ends(&b, e).ok_or("edge")?;
            let (nn, no) = plane_of(&b, cr.next).ok_or("panel face")?;
            // Convex on this skin (the axis inside the material): it turns away from it.
            let mid = (ea + eb) * 0.5;
            let w = (mid - cr.axis_o) - cr.axis * (mid - cr.axis_o).dot(cr.axis);
            let convex = w.dot(nf) > 0.0;
            let radius = if convex { cr.radius - t } else { cr.radius };
            if radius.is_nan() || radius <= -1e-9 {
                return Err(format!(
                    "a bend of radius {:.3} mm is tighter than the sheet ({t:.3} mm)",
                    cr.radius
                ));
            }
            let angle = nf.dot(nn).clamp(-1.0, 1.0).acos();
            if angle.is_nan() || angle <= 1e-6 {
                continue;
            }
            let (mut qa, mut qb) = (flat(ea), flat(eb));
            let mut d = (qb - qa).normalized().ok_or("edge")?;
            if (inside - qa).dot(Vec2::new(d.y, -d.x)) > 0.0 {
                std::mem::swap(&mut qa, &mut qb);
                d = d * -1.0;
            }
            let n = Vec2::new(d.y, -d.x);
            sheet.flanges.push(sd::SheetFlange {
                parent: panel,
                p: snapped(qa),
                d,
                len: qa.dist(qb),
                n,
                angle: if convex { angle } else { -angle },
                radius: radius.max(0.0),
                leg: 0.0,
                ext: (0.0, 0.0),
                slope: (0.0, 0.0),
                tear: (false, false),
                mitre: (Vec::new(), Vec::new()),
                shape: None,
                tag: format!("{prefix}.{}", sheet.flanges.len() + 1),
            });
            let k = sheet.flanges.len() - 1;
            let ba = sheet
                .flanges
                .get(k)
                .map(|fl| sheet.allowance(fl))
                .unwrap_or(0.0);
            // The next panel's face, through the bend's fold into the flat.
            let pl = frame_on(nn, no);
            let reg = region_of(&b, cr.next, &pl)?;
            let mapped = sheet
                .region_to_flat(Some(k), &pl, &reg)
                .map_err(|e| e.to_string())?;
            let pa = snapped(qa);
            let mut shape: Vec<Vec2> = mapped
                .outer
                .polyline(1e-2)
                .into_iter()
                .map(|q| {
                    let off = (q - pa).dot(n) - ba;
                    snapped(if off.abs() < 1e-6 { q - n * off } else { q })
                })
                .collect();
            // No two points closer than the kernel's tolerance (arcs come as fine chords).
            shape.dedup_by(|b, a| a.dist(*b) < 1e-5);
            while shape.len() > 2
                && shape
                    .first()
                    .zip(shape.last())
                    .is_some_and(|(a, b)| a.dist(*b) < 1e-5)
            {
                shape.pop();
            }
            let near = shape
                .iter()
                .map(|q| (*q - pa).dot(n) - ba)
                .fold(f64::MAX, f64::min);
            if near.abs() > 1e-3 {
                return Err(format!(
                    "a bend of radius {radius:.3} mm does not unfold to its panel ({near:.4} mm off): \
                     the sheet is not of constant thickness {t:.3} mm there"
                ));
            }
            let leg = shape
                .iter()
                .map(|q| (*q - pa).dot(n) - ba)
                .fold(0.0, f64::max);
            if let Some(fl) = sheet.flanges.get_mut(k) {
                fl.shape = Some(shape);
                fl.leg = leg;
            }
            for h in &mapped.holes {
                sheet.holes.push(Region2 {
                    outer: h.clone().ccw(),
                    holes: Vec::new(),
                });
            }
            seen.extend(cr.pieces.iter().copied());
            seen.insert(cr.next);
            todo.push((cr.next, Some(k), nn));
        }
    }
    if sheet.flanges.is_empty() {
        return Err("no round bends found next to the base face".into());
    }
    // Edge names as a Convert's: the base outline `E<k>`.
    let pts = sheet.base.outer.ccw().polyline(1e-3);
    let n = pts.len();
    sheet.base_curves = (0..n)
        .filter_map(|k| {
            let (a, b) = (*pts.get(k)?, *pts.get((k + 1) % n)?);
            (a.dist(b) > 1e-9).then(|| (format!("E{}", k + 1), a, b))
        })
        .collect();
    let mut s = named_solid(&sheet, prefix, false)?;
    let (want, got) = (solid.volume_at(1e-3), s.volume_at(1e-3));
    if let (Some(w), Some(g)) = (want, got)
        && (w - g).abs() > w.abs() * 2e-3
    {
        return Err(format!(
            "the sheet found ({g:.1} mm³) is not the body ({w:.1} mm³): not a sheet of constant \
             thickness {t:.3} mm, or features the conversion does not know yet (cut-outs across \
             bends, formed features, bends that are not round)"
        ));
    }
    s.name = solid.name.clone();
    Ok((s, SheetModel(Arc::new(sheet))))
}

fn point_in(poly: &[Vec2], p: Vec2) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}
