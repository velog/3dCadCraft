//! **Measure** (`docs/spec/0.10-physical.md` §Measure): one picked item
//! gives its length, area, volume, radius or position; two give the
//! minimum distance between them (with the two closest points) and, when
//! both have a direction (planar faces, straight edges, axes of
//! cylinders), the angle.
//!
//! Adapted from SolidCraft's `crates/engine/src/cmd/measure_sel.rs`
//! (MIT OR Apache-2.0, see `ATTRIBUTION.md`): the closest pair runs over
//! sampled points and display triangles with caps that keep it interactive.
//! On top of it, analytic values where the geometry is known exactly:
//! straight edges, circles (radius, diameter, arc length), planes (normal,
//! the distance between parallel planes), cylinders and spheres (radius),
//! and a face or a circle made of several kernel pieces (the exact kernel
//! splits a full circle into arcs, a cylinder into half faces) is measured
//! whole.

use crate::kernel::{
    BRep, Curve, EdgeId, FaceId, Point3, Real, Surface, TessellationOptions, Vector3,
};

/// What can be measured, by the names the selection uses.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// A face by persistent name (every piece with that name).
    Face(String),
    /// An edge (with the other pieces of its circle, if it is one).
    Edge(EdgeId),
    /// A vertex by persistent name.
    Vertex(String),
    /// A body (solid) by persistent name.
    Body(String),
    /// A point given directly (a work point, a sketch point).
    Point(Point3),
}

/// The measurement of one item.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    /// "Face", "Edge", "Vertex", "Body", "Point".
    pub kind: &'static str,
    /// Edge length (mm).
    pub length: Option<Real>,
    /// Face area / body surface area (mm²).
    pub area: Option<Real>,
    /// Body volume (mm³).
    pub volume: Option<Real>,
    /// Radius of a circle, arc, cylinder or sphere (mm).
    pub radius: Option<Real>,
    /// Centre of a circle / sphere, or the position of a point.
    pub position: Option<Point3>,
    /// Sweep of an arc (radians) when it is not a full circle.
    pub arc: Option<Real>,
    /// Plane normal / line direction / cylinder axis (for angles).
    pub direction: Option<Vector3>,
    /// The direction is an axis (a line or a cylinder): its sign does not
    /// matter for the angle.
    pub axis: bool,
}

/// One or two measured items, with the distance and the angle of a pair.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Measurement {
    /// The measured items, in the order given.
    pub items: Vec<Item>,
    /// Minimum distance (mm) and its two closest points.
    pub distance: Option<(Real, Point3, Point3)>,
    /// Centre-to-centre distance of two circles / spheres / points (mm).
    pub center_distance: Option<Real>,
    /// Angle between the two directions (degrees).
    pub angle_deg: Option<Real>,
}

/// Caps that keep a measurement interactive on big meshes (SolidCraft's).
const MAX_POINTS: usize = 4000;
const MAX_TRIS: usize = 20000;

/// The geometry of an item for the closest pair.
enum Geo {
    Points(Vec<Point3>),
    Tris(Vec<[Point3; 3]>),
}

/// Measures one or two targets. A third is refused.
pub fn measure(body: &BRep, targets: &[Target]) -> Result<Measurement, String> {
    if targets.is_empty() || targets.len() > 2 {
        return Err("pick one or two items (faces, edges, vertices, bodies)".into());
    }
    let mut out = Measurement::default();
    let mut geos = Vec::new();
    for t in targets {
        let (item, geo) = describe(body, t).ok_or("this item can't be measured")?;
        out.items.push(item);
        geos.push(geo);
    }
    if let ([ia, ib], [ga, gb]) = (out.items.as_slice(), geos.as_slice()) {
        out.distance = exact_distance(ia, ib, targets, body)
            .or_else(|| closest(ga, gb).map(|(p, q)| ((q - p).norm(), p, q)));
        if let (Some(a), Some(b)) = (ia.position, ib.position) {
            out.center_distance = Some((b - a).norm());
        }
        if let (Some(a), Some(b)) = (ia.direction, ib.direction) {
            let c = a.dot(&b).clamp(-1.0, 1.0);
            let c = if ia.axis || ib.axis { c.abs() } else { c };
            out.angle_deg = Some(c.acos().to_degrees());
        }
    }
    Ok(out)
}

/// Two parallel planar faces: the plane-to-plane distance (exact).
fn exact_distance(
    a: &Item,
    b: &Item,
    targets: &[Target],
    body: &BRep,
) -> Option<(Real, Point3, Point3)> {
    let (Target::Face(na), Target::Face(nb)) = (&targets[0], &targets[1]) else {
        return None;
    };
    let (da, db) = (a.direction?, b.direction?);
    if a.axis || b.axis || da.dot(&db).abs() < 1.0 - 1e-9 {
        return None;
    }
    let pa = plane_of(body, na)?;
    let pb = plane_of(body, nb)?;
    let d = (pb.origin - pa.origin).dot(&da).abs();
    // The closest points: the second face's origin projected onto the first.
    let q = face_points(body, nb)
        .into_iter()
        .next()
        .unwrap_or(pb.origin);
    let p = q - da * (q - pa.origin).dot(&da);
    Some((d, p, q))
}

fn faces_named<'a>(body: &'a BRep, name: &'a str) -> impl Iterator<Item = FaceId> + 'a {
    body.faces()
        .filter(move |(_, f)| f.name.as_deref() == Some(name))
        .map(|(id, _)| id)
}

/// The analytic surface of a face (the exact one of a faceted face).
fn surface_of(body: &BRep, f: FaceId) -> Option<&Surface> {
    let face = body.face(f)?;
    body.surface(face.facet_of.unwrap_or(face.surface))
}

fn plane_of(body: &BRep, name: &str) -> Option<crate::kernel::Plane> {
    let f = faces_named(body, name).next()?;
    match surface_of(body, f)? {
        Surface::Plane(p) => Some(p.clone()),
        _ => None,
    }
}

fn face_tris(body: &BRep, f: FaceId) -> Vec<[Point3; 3]> {
    let Ok(m) = body.face_mesh(f, &TessellationOptions::default()) else {
        return Vec::new();
    };
    m.triangles
        .iter()
        .filter_map(|t| {
            Some([
                *m.positions.get(t[0] as usize)?,
                *m.positions.get(t[1] as usize)?,
                *m.positions.get(t[2] as usize)?,
            ])
        })
        .collect()
}

fn face_points(body: &BRep, name: &str) -> Vec<Point3> {
    faces_named(body, name)
        .flat_map(|f| face_tris(body, f))
        .map(|[a, b, c]| Point3::from((a.coords + b.coords + c.coords) / 3.0))
        .collect()
}

fn tri_area([a, b, c]: &[Point3; 3]) -> Real {
    (b - a).cross(&(c - a)).norm() * 0.5
}

/// The face pieces, the edge's circle pieces, … as an item + geometry.
fn describe(body: &BRep, t: &Target) -> Option<(Item, Geo)> {
    Some(match t {
        Target::Point(p) => (
            Item {
                kind: "Point",
                position: Some(*p),
                ..Item::default()
            },
            Geo::Points(vec![*p]),
        ),
        Target::Vertex(name) => {
            let v = body.vertex(body.find_vertex(name)?)?;
            (
                Item {
                    kind: "Vertex",
                    position: Some(v.point),
                    ..Item::default()
                },
                Geo::Points(vec![v.point]),
            )
        }
        Target::Face(name) => {
            let faces: Vec<FaceId> = faces_named(body, name).collect();
            let first = *faces.first()?;
            let tris: Vec<[Point3; 3]> = faces.iter().flat_map(|f| face_tris(body, *f)).collect();
            let area = tris.iter().map(tri_area).sum();
            let mut item = Item {
                kind: "Face",
                area: Some(area),
                ..Item::default()
            };
            match surface_of(body, first)? {
                Surface::Plane(p) => item.direction = Some(p.normal.into_inner()),
                Surface::Cylinder(c) => {
                    item.radius = Some(c.radius);
                    item.direction = Some(c.axis.into_inner());
                    item.axis = true;
                }
                Surface::Sphere(s) => {
                    item.radius = Some(s.radius);
                    item.position = Some(s.center);
                }
                _ => {}
            }
            (item, Geo::Tris(tris))
        }
        Target::Edge(e) => edge_item(body, *e)?,
        Target::Body(name) => {
            let (_, solid) = body
                .solids()
                .find(|(_, s)| s.name.as_deref() == Some(name.as_str()))?;
            let tris: Vec<[Point3; 3]> = solid
                .shells
                .iter()
                .flat_map(|sh| sh.faces.iter())
                .flat_map(|f| face_tris(body, *f))
                .collect();
            let area = tris.iter().map(tri_area).sum();
            // Divergence theorem over the (outward) display triangles.
            let volume: Real = tris
                .iter()
                .map(|[a, b, c]| a.coords.dot(&b.coords.cross(&c.coords)) / 6.0)
                .sum();
            (
                Item {
                    kind: "Body",
                    area: Some(area),
                    volume: (!solid.open).then_some(volume.abs()),
                    ..Item::default()
                },
                Geo::Tris(tris),
            )
        }
    })
}

/// A straight edge (exact length, direction), a circle or arc (radius,
/// centre, exact length; the pieces of one circle sharing its two faces
/// are one item), or a free-form edge (sampled length).
fn edge_item(body: &BRep, e: EdgeId) -> Option<(Item, Geo)> {
    let edge = body.edge(e)?;
    let opts = TessellationOptions::default();
    let curve = body.curve(edge.curve)?;
    match curve {
        Curve::Line(_) => {
            let a = body.vertex(edge.start)?.point;
            let b = body.vertex(edge.end)?.point;
            let dir = (b - a).try_normalize(1e-12);
            let pts = body.sample_edge(e, &opts).ok()?;
            Some((
                Item {
                    kind: "Edge",
                    length: Some((b - a).norm()),
                    direction: dir,
                    axis: true,
                    ..Item::default()
                },
                Geo::Points(densify(&pts)),
            ))
        }
        Curve::Circle(c) => {
            let pieces = circle_pieces(body, e, c);
            let mut pts = Vec::new();
            let mut sweep = 0.0;
            for p in &pieces {
                let Ok(s) = body.sample_edge(*p, &opts) else {
                    continue;
                };
                // Exact sweep from the samples' angles about the centre.
                for w in s.windows(2) {
                    let (u, v) = (w[0] - c.center, w[1] - c.center);
                    sweep += u.cross(&v).norm().atan2(u.dot(&v));
                }
                pts.extend(densify(&s));
            }
            let full = sweep >= std::f64::consts::TAU - 1e-6;
            let sweep = sweep.min(std::f64::consts::TAU);
            Some((
                Item {
                    kind: "Edge",
                    length: Some(c.radius * sweep),
                    radius: Some(c.radius),
                    position: Some(c.center),
                    arc: (!full).then_some(sweep),
                    direction: Some(c.normal.into_inner()),
                    axis: true,
                    ..Item::default()
                },
                Geo::Points(pts),
            ))
        }
        Curve::BSpline(_) => {
            let pts = body.sample_edge(e, &opts).ok()?;
            let len = pts.windows(2).map(|w| (w[1] - w[0]).norm()).sum();
            Some((
                Item {
                    kind: "Edge",
                    length: Some(len),
                    ..Item::default()
                },
                Geo::Points(densify(&pts)),
            ))
        }
    }
}

/// The edges on the same circle as `e` between the same two faces (by
/// name): the kernel's pieces of one visible circle.
fn circle_pieces(body: &BRep, e: EdgeId, c: &crate::kernel::Circle) -> Vec<EdgeId> {
    let names = |x: EdgeId| {
        let mut n: Vec<Option<String>> = body
            .edge_faces(x)
            .into_iter()
            .map(|f| body.face(f).and_then(|f| f.name.clone()))
            .collect();
        n.sort();
        n
    };
    let own = names(e);
    let tol = 1e-6 * c.radius.max(1.0);
    let same = |d: &crate::kernel::Circle| {
        (d.center - c.center).norm() < tol
            && (d.radius - c.radius).abs() < tol
            && d.normal.dot(&c.normal).abs() > 1.0 - 1e-9
    };
    let mut out: Vec<EdgeId> = body
        .edges()
        .filter(|(id, x)| {
            *id == e
                || matches!(body.curve(x.curve), Some(Curve::Circle(d)) if same(d))
                    && names(*id) == own
        })
        .map(|(id, _)| id)
        .collect();
    if out.is_empty() {
        out.push(e);
    }
    out
}

fn sample<T: Copy>(v: &[T], max: usize) -> Vec<T> {
    let step = v.len().div_ceil(max).max(1);
    v.iter().step_by(step).copied().collect()
}

/// A polyline with extra points so no gap is longer than 1/64 of it.
fn densify(pts: &[Point3]) -> Vec<Point3> {
    let total: Real = pts.windows(2).map(|w| (w[1] - w[0]).norm()).sum();
    let step = (total / 64.0).max(1e-9);
    let mut out = Vec::new();
    for w in pts.windows(2) {
        let n = (((w[1] - w[0]).norm() / step).ceil() as usize).clamp(1, 64);
        for k in 0..n {
            out.push(w[0] + (w[1] - w[0]) * (k as Real / n as Real));
        }
    }
    out.extend(pts.last().copied());
    out
}

fn closest_on_triangle(p: Point3, a: Point3, b: Point3, c: Point3) -> Point3 {
    let n = (b - a).cross(&(c - a));
    if let Some(nn) = n.try_normalize(1e-18) {
        let q = p - nn * (p - a).dot(&nn);
        if [(a, b), (b, c), (c, a)]
            .iter()
            .all(|(u, v)| (v - u).cross(&(q - u)).dot(&n) >= -1e-12)
        {
            return q;
        }
    }
    let on_seg = |u: Point3, v: Point3| {
        let d = v - u;
        let l2 = d.dot(&d);
        let t = if l2 > 0.0 {
            ((p - u).dot(&d) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        u + d * t
    };
    [on_seg(a, b), on_seg(b, c), on_seg(c, a)]
        .into_iter()
        .min_by(|x, y| (x - p).norm().total_cmp(&(y - p).norm()))
        .unwrap_or(a)
}

/// Closest pair of points between two pieces of geometry.
fn closest(a: &Geo, b: &Geo) -> Option<(Point3, Point3)> {
    // Each point against every triangle, but a triangle whose box is
    // farther than the best pair so far is skipped, and touching ends the
    // search (a body against its own face took 1 s brute force).
    let pts_to_tris = |pts: &[Point3], tris: &[[Point3; 3]]| -> Option<(Point3, Point3)> {
        let pts = sample(pts, MAX_POINTS);
        let tris = sample(tris, MAX_TRIS);
        let boxes: Vec<(Point3, Point3)> = tris
            .iter()
            .map(|[a, b, c]| (a.inf(b).inf(c), a.sup(b).sup(c)))
            .collect();
        let box_dist = |p: &Point3, (lo, hi): &(Point3, Point3)| {
            let d = (lo - p).sup(&(p - hi)).sup(&Vector3::zeros());
            d.norm()
        };
        let mut best: Option<(Real, Point3, Point3)> = None;
        for p in &pts {
            for (t, bx) in tris.iter().zip(&boxes) {
                if best.is_some_and(|(bd, _, _)| box_dist(p, bx) >= bd) {
                    continue;
                }
                let q = closest_on_triangle(*p, t[0], t[1], t[2]);
                let d = (q - p).norm();
                if best.is_none_or(|(bd, _, _)| d < bd) {
                    best = Some((d, *p, q));
                }
            }
            if best.is_some_and(|(bd, _, _)| bd < 1e-9) {
                break;
            }
        }
        best.map(|(_, p, q)| (p, q))
    };
    let verts = |tris: &[[Point3; 3]]| -> Vec<Point3> {
        tris.iter().flat_map(|t| t.iter().copied()).collect()
    };
    match (a, b) {
        (Geo::Points(p), Geo::Points(q)) => {
            let (p, q) = (sample(p, MAX_POINTS), sample(q, MAX_POINTS));
            let mut best: Option<(Real, Point3, Point3)> = None;
            for x in &p {
                for y in &q {
                    let d = (y - x).norm();
                    if best.is_none_or(|(bd, _, _)| d < bd) {
                        best = Some((d, *x, *y));
                    }
                }
            }
            best.map(|(_, x, y)| (x, y))
        }
        (Geo::Points(p), Geo::Tris(t)) => pts_to_tris(p, t),
        (Geo::Tris(t), Geo::Points(p)) => pts_to_tris(p, t).map(|(x, y)| (y, x)),
        (Geo::Tris(t1), Geo::Tris(t2)) => {
            let ab = pts_to_tris(&verts(t1), t2);
            let ba = pts_to_tris(&verts(t2), t1).map(|(x, y)| (y, x));
            [ab, ba]
                .into_iter()
                .flatten()
                .min_by(|x, y| (x.1 - x.0).norm().total_cmp(&(y.1 - y.0).norm()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::PartDocument;

    fn close(a: Real, b: Real, tol: Real) -> bool {
        (a - b).abs() <= tol
    }

    fn edge_between(body: &BRep, f1: &str, f2: &str) -> EdgeId {
        let (a, b) = (body.find_face(f1).unwrap(), body.find_face(f2).unwrap());
        body.edges()
            .map(|(id, _)| id)
            .find(|e| {
                let fs = body.edge_faces(*e);
                fs.contains(&a) && fs.contains(&b)
            })
            .unwrap()
    }

    #[test]
    fn box_face_edge_body_and_pairs() {
        for (l, w, h) in [(40.0, 30.0, 20.0), (10.0, 10.0, 10.0), (100.0, 5.0, 2.0)] {
            let p = PartDocument::with_box("Box1", [l, w, h]).unwrap();
            let b = &p.body;
            let top = measure(b, &[Target::Face("Box1:Top".into())]).unwrap();
            assert!(close(top.items[0].area.unwrap(), l * w, 1e-9 * l * w));
            let body = measure(b, &[Target::Body("Box1".into())]).unwrap();
            assert!(close(
                body.items[0].volume.unwrap(),
                l * w * h,
                1e-9 * l * w * h
            ));
            // Top and bottom: parallel planes, exact distance h, angle 180°.
            let pair = measure(
                b,
                &[
                    Target::Face("Box1:Top".into()),
                    Target::Face("Box1:Bottom".into()),
                ],
            )
            .unwrap();
            assert!(close(pair.distance.unwrap().0, h, 1e-9));
            assert!(close(pair.angle_deg.unwrap(), 180.0, 1e-9));
            // Top and a side: 90°, touching.
            let side = measure(
                b,
                &[
                    Target::Face("Box1:Top".into()),
                    Target::Face("Box1:Front".into()),
                ],
            )
            .unwrap();
            assert!(close(side.angle_deg.unwrap(), 90.0, 1e-9));
            assert!(side.distance.unwrap().0 < 1e-6);
            let e = edge_between(b, "Box1:Top", "Box1:Front");
            let len = measure(b, &[Target::Edge(e)]).unwrap().items[0]
                .length
                .unwrap();
            assert!(close(len, l, 1e-9) || close(len, w, 1e-9));
        }
    }

    /// A cylinder R on the box top: the side face gives the radius and the
    /// axis; its top circle (kernel pieces) is measured whole, 2πR.
    #[test]
    fn cylinder_radius_and_whole_circle() {
        use crate::part::{ExtrudeFeature, Operation, ProfileRef, SketchOptions, SketchPlacement};
        for r in [2.0, 5.0, 8.0] {
            let mut p = PartDocument::with_box("Box1", [40.0, 30.0, 10.0]).unwrap();
            let opts = SketchOptions {
                auto_project_origin: false,
                auto_reference_edges: false,
                ..Default::default()
            };
            let j = p
                .create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)
                .unwrap();
            let c = p.sketches[j]
                .sketch
                .plane
                .project(&Point3::new(20.0, 15.0, 10.0));
            p.sketches[j].sketch.add_circle(c, r).unwrap();
            let rg = p.sketches[j].sketch.regions();
            let mut ex =
                ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], 6.0);
            ex.operation = Operation::Join;
            p.add_extrude(j, ex).unwrap();
            let b = &p.body;
            let side = b
                .faces()
                .find(|(_, f)| {
                    matches!(
                        b.surface(f.facet_of.unwrap_or(f.surface)),
                        Some(Surface::Cylinder(_))
                    )
                })
                .and_then(|(_, f)| f.name.clone())
                .unwrap();
            let m = measure(b, &[Target::Face(side.clone())]).unwrap();
            assert!(close(m.items[0].radius.unwrap(), r, 1e-9));
            let side_area = m.items[0].area.unwrap();
            let want = std::f64::consts::TAU * r * 6.0;
            assert!(close(side_area, want, 2e-3 * want), "{side_area} vs {want}");
            let top_circle = b
                .edges()
                .find(|(_, e)| {
                    matches!(b.curve(e.curve), Some(Curve::Circle(c))
                        if (c.center.z - 16.0).abs() < 1e-6)
                })
                .map(|(id, _)| id)
                .unwrap();
            let m = measure(b, &[Target::Edge(top_circle)]).unwrap();
            let it = &m.items[0];
            assert!(close(it.radius.unwrap(), r, 1e-9));
            assert!(it.arc.is_none(), "the whole circle, not one piece");
            assert!(close(it.length.unwrap(), std::f64::consts::TAU * r, 1e-6));
            // Circle centre to a point above it.
            let m = measure(
                b,
                &[
                    Target::Edge(top_circle),
                    Target::Point(Point3::new(20.0, 15.0, 26.0)),
                ],
            )
            .unwrap();
            assert!(close(m.center_distance.unwrap(), 10.0, 1e-9));
        }
    }

    #[test]
    fn point_to_face_and_three_items_refused() {
        let p = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let m = measure(
            &p.body,
            &[
                Target::Point(Point3::new(20.0, 15.0, 50.0)),
                Target::Face("Box1:Top".into()),
            ],
        )
        .unwrap();
        assert!(close(m.distance.unwrap().0, 30.0, 1e-6));
        let three = [
            Target::Face("Box1:Top".into()),
            Target::Face("Box1:Bottom".into()),
            Target::Face("Box1:Front".into()),
        ];
        assert!(measure(&p.body, &three).is_err());
        assert!(measure(&p.body, &[Target::Face("nope".into())]).is_err());
    }
}
