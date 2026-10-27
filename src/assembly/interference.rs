//! Analyze Interference (`docs/spec/3.0-assembly-commands.md` §3.4, FEATURES
//! 3.5): the volumes where components overlap, GUI-free.
//!
//! Each component is its solids' display triangles moved into assembly
//! coordinates. A pair whose boxes overlap is intersected with the BSP CSG
//! of `kernel::csg`, each body first cut to the common box (so the last
//! tree holds only what lies near the overlap). Faces that only touch
//! (a Mate, a Flush) leave a sheet of no volume, which is not an
//! interference. Volumes and centroids come from the triangles: exact for
//! planar parts, an inscribed approximation on curved faces (as the display).

use crate::document::Occurrence;
use crate::kernel::csg::{self, CsgPolygon};
use crate::kernel::{Point3, Real, TessellationOptions};
use crate::part::PartDocument;

/// One component as triangles in assembly coordinates.
#[derive(Clone, Debug)]
pub struct Body {
    /// The occurrence's id.
    pub occurrence: u32,
    /// Closed solids' triangles (counter-clockwise from outside).
    pub tris: Vec<[Point3; 3]>,
    /// Their box.
    pub bbox: (Point3, Point3),
}

/// One overlap found.
#[derive(Clone, Debug)]
pub struct Interference {
    /// The two occurrences (ids, the first set's first).
    pub a: u32,
    /// The other one.
    pub b: u32,
    /// Volume of the overlap (mm³).
    pub volume: Real,
    /// Its centre (assembly coordinates).
    pub centroid: Point3,
    /// Its boundary as triangles (drawn red).
    pub tris: Vec<[Point3; 3]>,
}

/// The closed solids of `part` as triangles in its own coordinates
/// (surface bodies and hidden bodies are left out: they hold no volume).
pub fn part_triangles(part: &PartDocument) -> Vec<[Point3; 3]> {
    let body = &part.body;
    let opts = TessellationOptions::default();
    let mut out = Vec::new();
    for (sid, solid) in body.solids() {
        if solid.open || solid.name.as_deref().is_some_and(|n| !part.body_visible(n)) {
            continue;
        }
        let Ok(m) = body.tessellate_solid(sid, &opts) else {
            continue;
        };
        out.extend(
            m.faces
                .triangles
                .iter()
                .map(|t| t.map(|i| m.faces.positions[i as usize])),
        );
    }
    out
}

impl Body {
    /// `o`'s triangles (`local`, part coordinates) moved by its transform;
    /// `None` without triangles.
    pub fn of(o: &Occurrence, local: &[[Point3; 3]]) -> Option<Self> {
        let tris: Vec<[Point3; 3]> = local.iter().map(|t| t.map(|p| o.transform * p)).collect();
        let bbox = boxed(tris.iter().flatten())?;
        Some(Self {
            occurrence: o.id,
            tris,
            bbox,
        })
    }
}

fn boxed<'a>(pts: impl Iterator<Item = &'a Point3>) -> Option<(Point3, Point3)> {
    pts.fold(None, |b: Option<(Point3, Point3)>, p| {
        Some(b.map_or((*p, *p), |(lo, hi)| (lo.inf(p), hi.sup(p))))
    })
}

/// The pairs to check: set 1 against set 2, or within set 1 when set 2 is
/// empty (Inventor). A component in both sets is not checked against
/// itself; each pair once.
pub fn pairs(set1: &[u32], set2: &[u32]) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    let mut add = |a: u32, b: u32| {
        if a != b
            && !out
                .iter()
                .any(|&(x, y)| (x, y) == (a, b) || (x, y) == (b, a))
        {
            out.push((a, b));
        }
    };
    if set2.is_empty() {
        for (i, &a) in set1.iter().enumerate() {
            for &b in &set1[i + 1..] {
                add(a, b);
            }
        }
    } else {
        for &a in set1 {
            for &b in set2 {
                add(a, b);
            }
        }
    }
    out
}

/// The overlaps of `pairs` among `bodies`, in pair order. Pairs whose
/// boxes are apart cost nothing; the others are shared out to the cores.
pub fn analyze(bodies: &[Body], pairs: &[(u32, u32)]) -> Vec<Interference> {
    let find = |id: u32| bodies.iter().find(|b| b.occurrence == id);
    // Only pairs whose boxes overlap go to the threads.
    let work: Vec<(usize, &Body, &Body)> = pairs
        .iter()
        .enumerate()
        .filter_map(|(k, &(a, b))| Some((k, find(a)?, find(b)?)))
        .filter(|(_, a, b)| boxes_meet(a, b))
        .collect();
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(work.len().max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut found: Vec<(usize, Interference)> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(&(k, a, b)) = work.get(i) else {
                            break;
                        };
                        if let Some(x) = overlap(a, b) {
                            mine.push((k, x));
                        }
                    }
                    mine
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    });
    found.sort_by_key(|(k, _)| *k);
    found.into_iter().map(|(_, x)| x).collect()
}

/// Whether the boxes of `a` and `b` share some volume (touching is not).
fn boxes_meet(a: &Body, b: &Body) -> bool {
    let lo = a.bbox.0.sup(&b.bbox.0);
    let hi = a.bbox.1.inf(&b.bbox.1);
    (0..3).all(|k| hi[k] - lo[k] > 1e-6)
}

/// The overlap of two bodies, if it holds volume.
pub fn overlap(a: &Body, b: &Body) -> Option<Interference> {
    // Boxes that only touch hold no common volume.
    if !boxes_meet(a, b) {
        return None;
    }
    let lo = a.bbox.0.sup(&b.bbox.0);
    let hi = a.bbox.1.inf(&b.bbox.1);
    // The common box a little larger, so no face of it lies on a face of
    // the bodies (coplanar BSP splits are the fragile case).
    let pad = 1e-3 * (hi - lo).norm().max(1e-3);
    let (lo, hi) = (
        lo - Point3::new(pad, pad, pad).coords,
        hi + Point3::new(pad, pad, pad).coords,
    );
    // Every triangle goes in: the BSP classifies by its planes, so a mesh
    // cut down to the triangles near the box is no longer closed and
    // misjudges the inside (a plate with holes lost 20 % of the overlap).
    let polys = |tris: &[[Point3; 3]]| -> Vec<CsgPolygon> {
        tris.iter()
            .filter_map(|t| CsgPolygon::new(t.to_vec(), 0))
            .collect()
    };
    let cube = box_polygons(lo, hi);
    // Each body cut to the box first: the final tree is small.
    let pa = csg::intersect(polys(&a.tris), cube.clone());
    let pb = csg::intersect(polys(&b.tris), cube);
    if pa.is_empty() || pb.is_empty() {
        return None;
    }
    let common = csg::intersect(pa, pb);
    let volume = csg::volume(&common);
    // Below a millionth of the smaller body's box (and 1e-6 mm³): touching
    // faces, round-off.
    let size = |b: &Body| (b.bbox.1 - b.bbox.0).norm();
    let floor = (1e-6 * size(a).min(size(b)).powi(3)).max(1e-6);
    if volume <= floor {
        return None;
    }
    let mut tris = Vec::new();
    let mut moment = nalgebra::Vector3::zeros();
    for p in &common {
        for i in 1..p.pts.len() - 1 {
            let (x, y, z) = (p.pts[0], p.pts[i], p.pts[i + 1]);
            let v = x.coords.dot(&y.coords.cross(&z.coords)) / 6.0;
            moment += v * (x.coords + y.coords + z.coords) / 4.0;
            tris.push([x, y, z]);
        }
    }
    Some(Interference {
        a: a.occurrence,
        b: b.occurrence,
        volume,
        centroid: Point3::from(moment / volume),
        tris,
    })
}

/// An axis-aligned box as six outward quads.
fn box_polygons(lo: Point3, hi: Point3) -> Vec<CsgPolygon> {
    let c = |i: usize| {
        Point3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        )
    };
    [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ]
    .iter()
    .filter_map(|q| CsgPolygon::new(q.iter().map(|&i| c(i)).collect(), 0))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::DocRef;
    use crate::document::DocumentId;
    use crate::kernel::{Transform3, Vector3};

    fn occ(id: u32, t: Transform3) -> Occurrence {
        let mut o = Occurrence::new(
            format!("B:{id}"),
            DocRef::local(DocumentId::new_random()),
            t,
        );
        o.id = id;
        o
    }

    fn block() -> Vec<[Point3; 3]> {
        let p = PartDocument::with_box("Box1", [20.0, 10.0, 5.0]).unwrap();
        part_triangles(&p)
    }

    fn cylinder() -> Vec<[Point3; 3]> {
        use crate::part::primitive::{PrimitiveFeature, PrimitiveKind, add_primitive};
        use crate::part::{OriginPlane, SketchPlacement};
        use crate::sketch::Point2;
        let mut p = PartDocument::default();
        let f = PrimitiveFeature::extrude(&p, 30.0);
        add_primitive(
            &mut p,
            PrimitiveKind::Cylinder,
            SketchPlacement::on_origin_plane(OriginPlane::XY),
            &[Point2::new(0.0, 0.0), Point2::new(4.0, 0.0)],
            f,
        )
        .unwrap();
        part_triangles(&p)
    }

    #[test]
    fn pairs_within_one_set_or_between_two() {
        assert_eq!(pairs(&[1, 2, 3], &[]), vec![(1, 2), (1, 3), (2, 3)]);
        assert_eq!(pairs(&[1, 2], &[2, 3]), vec![(1, 2), (1, 3), (2, 3)]);
        assert!(pairs(&[1], &[]).is_empty());
    }

    /// Two blocks overlapping by 5 × 10 × 5 mm; touching and apart ones
    /// are no interference.
    #[test]
    fn overlapping_touching_and_apart_blocks() {
        let tris = block();
        let b1 = Body::of(&occ(1, Transform3::identity()), &tris).unwrap();
        let b2 = Body::of(&occ(2, Transform3::translation(15.0, 0.0, 0.0)), &tris).unwrap();
        let b3 = Body::of(&occ(3, Transform3::translation(0.0, 0.0, 5.0)), &tris).unwrap();
        let b4 = Body::of(&occ(4, Transform3::translation(100.0, 0.0, 0.0)), &tris).unwrap();
        let bodies = [b1, b2, b3, b4];
        let found = analyze(&bodies, &pairs(&[1, 2, 3, 4], &[]));
        assert_eq!(found.len(), 1, "{found:?}");
        let i = &found[0];
        assert_eq!((i.a, i.b), (1, 2));
        assert!((i.volume - 250.0).abs() < 1e-6, "{}", i.volume);
        assert!((i.centroid - Point3::new(17.5, 5.0, 2.5)).norm() < 1e-6);
        assert!(!i.tris.is_empty());
    }

    /// A shaft through a block: the overlap is the cylinder's piece inside
    /// (π r² · 5 within the display mesh's inscribed error).
    #[test]
    fn shaft_through_a_block() {
        let block = block();
        let shaft = cylinder();
        let b1 = Body::of(&occ(1, Transform3::identity()), &block).unwrap();
        let b2 = Body::of(&occ(2, Transform3::translation(10.0, 5.0, -10.0)), &shaft).unwrap();
        let found = analyze(&[b1, b2], &[(1, 2)]);
        assert_eq!(found.len(), 1);
        let want = std::f64::consts::PI * 16.0 * 5.0;
        let got = found[0].volume;
        assert!(got < want && got > 0.99 * want, "{got} vs {want}");
        let c = found[0].centroid;
        assert!((c - Point3::new(10.0, 5.0, 2.5)).norm() < 1e-3, "{c}");
        // Turned so it lies along X through the block: still found.
        let lying = Transform3::new(
            Vector3::new(-5.0, 5.0, 2.5),
            Vector3::new(0.0, 1.0, 0.0) * std::f64::consts::FRAC_PI_2,
        );
        let b2 = Body::of(&occ(2, lying), &shaft).unwrap();
        let b1 = Body::of(&occ(1, Transform3::identity()), &block).unwrap();
        let found = analyze(&[b1, b2], &[(1, 2)]);
        assert_eq!(found.len(), 1);
    }
}
