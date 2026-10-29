//! Selection helpers on a part's body (Inventor's selection modes and
//! Select Tangencies, `docs/spec/0.9-selection.md`): which faces belong to a
//! feature or a body, and which faces or edges continue a picked one
//! tangentially. Everything is found by persistent names (rule 2); the
//! arena ids returned are valid for the current body only.

use std::collections::{HashSet, VecDeque};

use crate::kernel::{BRep, EdgeId, FaceId, Point3, Real, TessellationOptions, Vector3};

/// The feature id a face name belongs to (`Extrusion1:End` → `Extrusion1`,
/// `Fillet1#2:RoundA` → `Fillet1`).
pub fn feature_id_of_face(name: &str) -> Option<&str> {
    name.split(':').next()?.split('#').next()
}

/// Faces created (named) by feature `id`.
pub fn faces_of_feature(body: &BRep, id: &str) -> Vec<FaceId> {
    body.faces()
        .filter(|(_, f)| f.name.as_deref().and_then(feature_id_of_face) == Some(id))
        .map(|(fid, _)| fid)
        .collect()
}

/// The persistent name of the body (solid) holding face `f`.
pub fn body_of_face(body: &BRep, f: FaceId) -> Option<String> {
    body.solids()
        .find(|(_, s)| s.shells.iter().any(|sh| sh.faces.contains(&f)))
        .and_then(|(_, s)| s.name.clone())
}

/// Faces of the body named `name`.
pub fn faces_of_body(body: &BRep, name: &str) -> Vec<FaceId> {
    body.solids()
        .filter(|(_, s)| s.name.as_deref() == Some(name))
        .flat_map(|(_, s)| s.shells.iter().flat_map(|sh| sh.faces.iter().copied()))
        .collect()
}

/// Whether face `f` belongs to a surface body (`Solid::open`).
pub fn face_on_surface(body: &BRep, f: FaceId) -> bool {
    body.solids()
        .any(|(_, s)| s.open && s.shells.iter().any(|sh| sh.faces.contains(&f)))
}

/// Whether the body named `name` is a surface body.
pub fn body_is_surface(body: &BRep, name: &str) -> bool {
    body.solids()
        .any(|(_, s)| s.open && s.name.as_deref() == Some(name))
}

/// A face touching the vertex named `name` (for Features / Bodies
/// priority: a vertex resolves through one of its faces).
pub fn face_of_vertex(body: &BRep, name: &str) -> Option<FaceId> {
    let v = body
        .vertices()
        .find(|(_, v)| v.name.as_deref() == Some(name))
        .map(|(id, _)| id)?;
    body.faces()
        .find(|(_, f)| {
            std::iter::once(&f.outer)
                .chain(&f.inner)
                .flat_map(|l| &l.coedges)
                .any(|c| {
                    body.edge(c.edge)
                        .is_some_and(|e| e.start == v || e.end == v)
                })
        })
        .map(|(id, _)| id)
}

/// The edges of the outer loop of face `f` (Select Loop on a face).
pub fn outer_loop_edges(body: &BRep, f: FaceId) -> Vec<EdgeId> {
    body.face(f)
        .map(|x| x.outer.coedges.iter().map(|c| c.edge).collect())
        .unwrap_or_default()
}

/// The point halfway along a polyline (by length).
pub fn polyline_middle(pts: &[Point3]) -> Option<Point3> {
    let total: Real = pts.windows(2).map(|w| (w[1] - w[0]).norm()).sum();
    let mut left = total / 2.0;
    for w in pts.windows(2) {
        let l = (w[1] - w[0]).norm();
        if l >= left && l > 0.0 {
            return Some(w[0] + (w[1] - w[0]) * (left / l));
        }
        left -= l;
    }
    pts.first().copied()
}

/// Whether edge `e` is straight (its sampled points lie on the line
/// through its ends): converted edges may carry a degree-1 B-spline
/// instead of a line.
pub fn edge_is_straight(body: &BRep, e: EdgeId) -> bool {
    let Ok(pts) = body.sample_edge(e, &TessellationOptions::default()) else {
        return false;
    };
    let (Some(a), Some(b)) = (pts.first(), pts.last()) else {
        return false;
    };
    let d = b - a;
    let l = d.norm();
    l > 1e-9
        && pts
            .iter()
            .all(|p| (p - a).cross(&d).norm() / l < 1e-6 * l.max(1.0))
}

/// Edges bounding face `f`.
pub fn edges_of_face(body: &BRep, f: FaceId) -> Vec<EdgeId> {
    body.face(f)
        .map(|face| {
            std::iter::once(&face.outer)
                .chain(&face.inner)
                .flat_map(|l| l.coedges.iter().map(|c| c.edge))
                .collect()
        })
        .unwrap_or_default()
}

/// Select Tangencies on a face: `f` and every face reached from it across
/// edges where the two faces meet tangentially (normals within 1°), like a
/// side wall, its fillets and the next wall.
pub fn tangent_faces(body: &BRep, f: FaceId) -> Vec<FaceId> {
    let opts = TessellationOptions::default();
    let mut meshes = std::collections::HashMap::new();
    let mut normal_at = |face: FaceId, p: &Point3| -> Option<Vector3> {
        let mesh = meshes
            .entry(face)
            .or_insert_with(|| body.face_mesh(face, &opts).ok())
            .as_ref()?;
        let k = (0..mesh.positions.len()).min_by(|&a, &b| {
            (mesh.positions[a] - p)
                .norm()
                .total_cmp(&(mesh.positions[b] - p).norm())
        })?;
        mesh.normals.get(k).copied()
    };
    let facet_of = |face: FaceId| body.face(face).and_then(|x| x.facet_of);
    let mut seen: HashSet<FaceId> = HashSet::from([f]);
    let mut queue = VecDeque::from([f]);
    let mut out = vec![f];
    while let Some(a) = queue.pop_front() {
        for e in edges_of_face(body, a) {
            let Some(p) = body
                .sample_edge(e, &opts)
                .ok()
                .and_then(|pts| polyline_middle(&pts))
            else {
                continue;
            };
            for b in body.edge_faces(e) {
                if seen.contains(&b) {
                    continue;
                }
                let same_surface = facet_of(a).is_some() && facet_of(a) == facet_of(b);
                let tangent = same_surface
                    || match (normal_at(a, &p), normal_at(b, &p)) {
                        (Some(na), Some(nb)) => {
                            na.dot(&nb) > 1.0_f64.to_radians().cos() * na.norm() * nb.norm()
                        }
                        _ => false,
                    };
                if tangent {
                    seen.insert(b);
                    queue.push_back(b);
                    out.push(b);
                }
            }
        }
    }
    out
}

/// Select Tangencies on an edge: the edges of its tangent chain
/// ([`super::blend::tangent_chain`]) on the current body.
pub fn tangent_edges(body: &BRep, e: EdgeId) -> Vec<EdgeId> {
    let chain = super::blend::tangent_chain(body, e);
    let pair = |x: EdgeId| -> Option<[String; 2]> {
        let mut v: Vec<String> = body
            .edge_faces(x)
            .iter()
            .filter_map(|f| body.face(*f).and_then(|f| f.name.clone()))
            .collect();
        v.sort();
        (v.len() == 2).then(|| [v[0].clone(), v[1].clone()])
    };
    let wanted: HashSet<[String; 2]> = chain
        .iter()
        .map(|r| {
            let mut f = r.faces.clone();
            f.sort();
            f
        })
        .collect();
    let mut out: Vec<EdgeId> = body
        .edges()
        .map(|(x, _)| x)
        .filter(|&x| pair(x).is_some_and(|p| wanted.contains(&p)))
        .collect();
    if !out.contains(&e) {
        out.push(e);
    }
    out
}

/// Loop selection (Fillet/Chamfer Select Mode **Loop**): the edges of the
/// loop of face `f` (outer or a hole) nearest to `near`.
pub fn loop_edges(body: &BRep, f: FaceId, near: Point3) -> Vec<EdgeId> {
    let opts = TessellationOptions::default();
    let Some(face) = body.face(f) else {
        return Vec::new();
    };
    let dist = |e: EdgeId| {
        body.sample_edge(e, &opts)
            .map(|pts| {
                pts.iter()
                    .map(|p| (p - near).norm())
                    .fold(Real::MAX, Real::min)
            })
            .unwrap_or(Real::MAX)
    };
    std::iter::once(&face.outer)
        .chain(&face.inner)
        .map(|l| l.coedges.iter().map(|c| c.edge).collect::<Vec<_>>())
        .min_by(|a, b| {
            let da = a.iter().map(|&e| dist(e)).fold(Real::MAX, Real::min);
            let db = b.iter().map(|&e| dist(e)).fold(Real::MAX, Real::min);
            da.total_cmp(&db)
        })
        .unwrap_or_default()
}

/// Feature selection (Select Mode **Feature**): edges where a face of
/// feature `id` meets a face of another feature.
pub fn feature_edges(body: &BRep, id: &str) -> Vec<EdgeId> {
    let of = |f: FaceId| {
        body.face(f)
            .and_then(|x| x.name.as_deref())
            .and_then(feature_id_of_face)
            .map(str::to_owned)
    };
    body.edges()
        .map(|(e, _)| e)
        .filter(|&e| {
            let owners: Vec<Option<String>> = body.edge_faces(e).into_iter().map(of).collect();
            owners.len() == 2
                && owners.iter().any(|o| o.as_deref() == Some(id))
                && owners.iter().any(|o| o.as_deref() != Some(id))
        })
        .collect()
}

/// Whether edge `e` is convex (a round removes material there) or concave
/// (a fillet adds it): the second face bends away below the first face's
/// tangent plane at a convex edge. `None` for a smooth (tangent) edge or
/// one not between two faces.
pub fn edge_convex(body: &BRep, e: EdgeId) -> Option<bool> {
    let opts = TessellationOptions::default();
    let fs = body.edge_faces(e);
    let [f1, f2] = fs[..] else {
        return None;
    };
    let p = polyline_middle(&body.sample_edge(e, &opts).ok()?)?;
    let m1 = body.face_mesh(f1, &opts).ok()?;
    let m2 = body.face_mesh(f2, &opts).ok()?;
    let k = (0..m1.positions.len()).min_by(|&a, &b| {
        (m1.positions[a] - p)
            .norm()
            .total_cmp(&(m1.positions[b] - p).norm())
    })?;
    let n1 = *m1.normals.get(k)?;
    // A point of the second face near the edge: its triangle centroid
    // nearest to p (inside the face, off the edge).
    let c2 = m2
        .triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| m2.positions[i as usize]);
            Point3::from((a.coords + b.coords + c.coords) / 3.0)
        })
        .min_by(|a, b| (a - p).norm().total_cmp(&(b - p).norm()))?;
    let v = c2 - p;
    let h = n1.dot(&v) / v.norm().max(1e-12);
    if h.abs() < 1e-3 {
        return None;
    }
    Some(h < 0.0)
}

/// Which edges of a body Select Mode **Solid** takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolidEdges {
    /// Every sharp edge.
    All,
    /// Concave edges only (fillets add material).
    Fillets,
    /// Convex edges only (rounds remove material).
    Rounds,
}

/// The sharp edges of the body named `solid` (or of every body when `None`).
pub fn solid_edges(body: &BRep, solid: Option<&str>, which: SolidEdges) -> Vec<EdgeId> {
    let faces: Option<HashSet<FaceId>> =
        solid.map(|n| faces_of_body(body, n).into_iter().collect());
    body.edges()
        .map(|(e, _)| e)
        .filter(|&e| {
            faces
                .as_ref()
                .is_none_or(|fs| body.edge_faces(e).iter().all(|f| fs.contains(f)))
        })
        .filter(|&e| match (edge_convex(body, e), which) {
            (None, _) => false,
            (Some(_), SolidEdges::All) => true,
            (Some(c), SolidEdges::Rounds) => c,
            (Some(c), SolidEdges::Fillets) => !c,
        })
        .collect()
}

/// Full Round of the planar face named `center`: the two neighbouring
/// planar faces that are parallel to each other and perpendicular to it
/// (the sides of a rib or wall), the edges it shares with them, and the
/// radius that makes the round touch all three (half the distance between
/// the sides). Errors name what is missing.
pub fn full_round_edges(body: &BRep, center: &str) -> Result<(Vec<super::EdgeRef>, Real), String> {
    let c = body
        .find_face(center)
        .ok_or_else(|| format!("the middle face {center} no longer exists"))?;
    let cp = body.face_plane(c).ok_or("the middle face must be planar")?;
    let nc = cp.normal.into_inner();
    let mut sides: Vec<(FaceId, crate::kernel::Plane)> = Vec::new();
    for e in edges_of_face(body, c) {
        for f in body.edge_faces(e) {
            if f == c || sides.iter().any(|(x, _)| *x == f) {
                continue;
            }
            if let Some(p) = body.face_plane(f)
                && p.normal.dot(&nc).abs() < 1e-6
            {
                sides.push((f, p));
            }
        }
    }
    // The pair of parallel sides (opposite faces of the wall).
    let mut pair = None;
    for (i, (a, pa)) in sides.iter().enumerate() {
        for (b, pb) in sides.iter().skip(i + 1) {
            if pa.normal.dot(&pb.normal).abs() > 1.0 - 1e-9 {
                let w = (pb.origin - pa.origin).dot(&pa.normal).abs();
                if w > 1e-9 {
                    pair = Some((*a, *b, w));
                }
            }
        }
    }
    let (a, b, w) = pair.ok_or("the middle face needs two parallel side faces")?;
    let edges: Vec<super::EdgeRef> = edges_of_face(body, c)
        .into_iter()
        .filter(|&e| {
            let fs = body.edge_faces(e);
            fs.contains(&a) || fs.contains(&b)
        })
        .filter_map(|e| super::blend::edge_ref(body, e))
        .collect();
    if edges.is_empty() {
        return Err("no edges between the middle face and the sides".into());
    }
    Ok((edges, w / 2.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::blend::edge_ref;
    use crate::part::{BlendFeature, PartDocument};

    /// Helpers of the selection model (spec 0.9a): a vertex resolves to a
    /// face of its feature, a face's outer loop, solid vs surface body.
    #[test]
    fn vertex_face_loop_and_surface() {
        let part = PartDocument::with_box("Box1", [10.0, 10.0, 10.0]).unwrap();
        let body = &part.body;
        let v = body.vertices().find_map(|(_, v)| v.name.clone()).unwrap();
        let f = face_of_vertex(body, &v).unwrap();
        let name = body.face(f).unwrap().name.clone().unwrap();
        assert_eq!(feature_id_of_face(&name), Some("Box1"));
        assert_eq!(feature_id_of_face(&v), Some("Box1"));
        assert!(face_of_vertex(body, "Nope:V1").is_none());
        let top = body.find_face("Box1:Top").unwrap();
        assert_eq!(outer_loop_edges(body, top).len(), 4);
        assert!(!face_on_surface(body, top));
        let solid = body_of_face(body, top).unwrap();
        assert!(!body_is_surface(body, &solid));
    }

    /// A 40 mm box with its 4 vertical edges rounded: the side walls and
    /// the 4 rounds form one tangent ring (8 faces); top and bottom are not
    /// tangent to it.
    fn rounded_sides() -> PartDocument {
        let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
        let vertical: Vec<_> = part
            .body
            .edges()
            .filter_map(|(e, _)| edge_ref(&part.body, e))
            .filter(|r| {
                r.faces
                    .iter()
                    .all(|f| !f.ends_with(":Top") && !f.ends_with(":Bottom"))
            })
            .collect();
        assert_eq!(vertical.len(), 4);
        part.add_blend(BlendFeature::fillet(vertical, 3.0)).unwrap();
        part
    }

    #[test]
    fn tangent_faces_of_a_wall_with_rounds() {
        let part = rounded_sides();
        let body = &part.body;
        let name = |f: FaceId| {
            body.face(f)
                .and_then(|x| x.name.clone())
                .unwrap_or_default()
        };
        let wall = body
            .faces()
            .find(|(_, f)| {
                f.name.as_deref().is_some_and(|n| {
                    n.starts_with("Box1:") && !n.ends_with(":Top") && !n.ends_with(":Bottom")
                })
            })
            .map(|(id, _)| id)
            .unwrap();
        let ring = tangent_faces(body, wall);
        assert_eq!(
            ring.len(),
            8,
            "{:?}",
            ring.iter().map(|&f| name(f)).collect::<Vec<_>>()
        );
        let top = body.find_face("Box1:Top").unwrap();
        assert_eq!(
            tangent_faces(body, top),
            vec![top],
            "top meets the walls at 90°"
        );
    }

    #[test]
    fn faces_of_feature_and_body() {
        let part = rounded_sides();
        let body = &part.body;
        assert_eq!(faces_of_feature(body, "Box1").len(), 6);
        let rounds = faces_of_feature(body, part.features[1].id());
        assert_eq!(rounds.len(), 4);
        let top = body.find_face("Box1:Top").unwrap();
        let solid = body_of_face(body, top).unwrap();
        assert_eq!(faces_of_body(body, &solid).len(), 10);
    }

    /// A plate with a blind pocket: the box's 12 edges are convex, the
    /// pocket's rim convex and its floor edges concave.
    #[test]
    fn convex_and_concave_edges() {
        use crate::part::{
            ExtentDirection, ExtrudeFeature, Operation, ProfileRef, SketchOptions, SketchPlacement,
        };
        let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 10.0]).unwrap();
        let opts = SketchOptions {
            auto_project_origin: false,
            ..Default::default()
        };
        let j = part
            .create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)
            .unwrap();
        let plane = part.sketches[j].sketch.plane.clone();
        let at = |x: f64, y: f64| plane.project(&Point3::new(x, y, 10.0));
        part.sketches[j]
            .sketch
            .add_rectangle_two_point(at(10.0, 10.0), at(30.0, 20.0))
            .unwrap();
        let r = part.sketches[j].sketch.regions();
        let pocket = r.iter().find(|x| x.holes.is_empty()).unwrap();
        let mut ex =
            ExtrudeFeature::new(vec![ProfileRef::of(pocket, pocket.interior_point())], 4.0);
        ex.operation = Operation::Cut;
        ex.direction = ExtentDirection::Flipped;
        part.add_extrude(j, ex).unwrap();
        let body = &part.body;
        let all = solid_edges(body, None, SolidEdges::All);
        let rounds = solid_edges(body, None, SolidEdges::Rounds);
        let fillets = solid_edges(body, None, SolidEdges::Fillets);
        // 12 box edges + 4 rim + 4 vertical pocket corners (concave) + 4 floor.
        let dbg: Vec<String> = all
            .iter()
            .map(|&e| {
                let n: Vec<String> = body
                    .edge_faces(e)
                    .iter()
                    .filter_map(|f| body.face(*f).and_then(|x| x.name.clone()))
                    .collect();
                format!("{n:?} {:?}", edge_convex(body, e))
            })
            .collect();
        // The box's vertical edges come in two pieces each (kernel split):
        // count face pairs. 12 box edges + 4 rim + 4 pocket corners + 4 floor.
        let pairs = |v: &[EdgeId]| {
            let mut p: Vec<Vec<String>> = v
                .iter()
                .map(|&e| {
                    let mut n: Vec<String> = body
                        .edge_faces(e)
                        .iter()
                        .filter_map(|f| body.face(*f).and_then(|x| x.name.clone()))
                        .collect();
                    n.sort();
                    n
                })
                .collect();
            p.sort();
            p.dedup();
            p.len()
        };
        assert_eq!(pairs(&all), 24, "{dbg:#?}");
        assert_eq!(pairs(&rounds), 16);
        assert_eq!(pairs(&fillets), 8);
        // Loop: the rim loop of the top face (its hole), near a rim point.
        let top = body.find_face("Box1:Top").unwrap();
        assert_eq!(
            loop_edges(body, top, Point3::new(20.0, 10.0, 10.0)).len(),
            4
        );
        assert_eq!(loop_edges(body, top, Point3::new(0.0, 15.0, 10.0)).len(), 4);
        // Feature: the pocket's edges against the box (rim) only.
        let id = part.features[1].id();
        assert_eq!(feature_edges(body, id).len(), 4);
    }

    /// The top outline of the rounded box: 4 lines and 4 arcs, tangent all
    /// the way round.
    #[test]
    fn tangent_edge_chain_round_the_top() {
        let part = rounded_sides();
        let body = &part.body;
        let top = body.find_face("Box1:Top").unwrap();
        let e = edges_of_face(body, top)[0];
        let chain = tangent_edges(body, e);
        assert_eq!(chain.len(), edges_of_face(body, top).len());
    }
}
