//! Distances between triangle meshes and the comparison of two closed meshes, through SolidCraft's
//! `solidcraft_geom::mesh_distance` (a bounding volume hierarchy over each mesh's triangles, exact
//! triangle–triangle distances) and `solidcraft_geom::compare` (each surface sampled against the
//! other; regions of material only one mesh has, with their volumes, found without a Boolean).
//!
//! Used by Measure's minimum distance (clearance between faces and bodies, spec 7.6 / 3.1) and by
//! the comparison of two versions of a part. Only our own types cross this boundary.

use super::compat::sg;
use super::{p3, v3};
use crate::diagnostics::crash::guard;
use crate::kernel::{Point3, Real};

/// The nearest points of two meshes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Closest {
    /// Their distance (mm; 0 where the meshes cross).
    pub distance: Real,
    /// The point on the first mesh.
    pub a: Point3,
    /// The point on the second.
    pub b: Point3,
}

/// A mesh's triangles prepared for distance queries (built once, asked many times).
pub struct Triangles(sg::mesh_distance::TriangleSet);

fn mesh_of(tris: &[[Point3; 3]]) -> sg::Mesh {
    let positions: Vec<sg::Vec3> = tris.iter().flat_map(|t| t.iter().map(v3)).collect();
    let triangles: Vec<[u32; 3]> = (0..tris.len() as u32)
        .map(|k| [3 * k, 3 * k + 1, 3 * k + 2])
        .collect();
    sg::Mesh {
        tri_face: vec![0; triangles.len()],
        positions,
        triangles,
        ..Default::default()
    }
}

impl Triangles {
    /// The triangles `tris` (ones with a coordinate that is not finite are left out).
    pub fn new(tris: &[[Point3; 3]]) -> Self {
        let m = mesh_of(tris);
        Triangles(guard(|| sg::mesh_distance::TriangleSet::new(&m)).unwrap_or_default())
    }

    /// How many triangles it holds.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The nearest points of these triangles and `other`'s, if they come within `limit`
    /// (`Real::INFINITY`: always).
    pub fn closest(&self, other: &Triangles, limit: Real) -> Option<Closest> {
        let m = guard(|| self.0.closest(&other.0, limit)).ok().flatten();
        m.map(|m| Closest {
            distance: m.distance,
            a: p3(m.a),
            b: p3(m.b),
        })
    }

    /// Every pair of triangles, one of each set, nearer than `d`, with their nearest points; at
    /// most `cap` of them (the first found).
    pub fn within(&self, other: &Triangles, d: Real, cap: usize) -> Vec<Closest> {
        guard(|| {
            let mut out = Vec::new();
            self.0.within(&other.0, d, |m| {
                if out.len() < cap {
                    out.push(Closest {
                        distance: m.distance,
                        a: p3(m.a),
                        b: p3(m.b),
                    });
                }
            });
            out
        })
        .unwrap_or_default()
    }
}

/// The nearest points of two triangle lists (built and asked once).
pub fn closest(a: &[[Point3; 3]], b: &[[Point3; 3]]) -> Option<Closest> {
    Triangles::new(a).closest(&Triangles::new(b), Real::INFINITY)
}

/// How far one mesh's surface strays from the other's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Deviation {
    /// The largest distance (mm) and where it was found.
    pub max: Real,
    /// Where it stands.
    pub at: Point3,
    /// The mean distance, weighted by area (mm).
    pub mean: Real,
    /// The surface further than the tolerance from the other mesh, and all of it (mm²).
    pub beyond_area: Real,
    /// All of the surface (mm²).
    pub area: Real,
}

/// A place where only one of the two meshes has material.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Difference {
    /// The first mesh has the material (else the second).
    pub only_in_a: bool,
    /// Its volume (mm³; NaN when it would take too long to measure).
    pub volume: Real,
    /// Its centre.
    pub center: Point3,
    /// The largest distance between the surfaces here (mm).
    pub max_deviation: Real,
}

/// What [`compare`] found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Comparison {
    /// The first surface measured against the second, and the other way round.
    pub a_to_b: Deviation,
    /// The second surface measured against the first.
    pub b_to_a: Deviation,
    /// The differences, largest volume first (at most 50; the rest are counted in `unlisted`).
    pub differences: Vec<Difference>,
    /// Differences beyond the first 50, not measured.
    pub unlisted: usize,
}

impl Comparison {
    /// The two meshes agree within the tolerance everywhere.
    pub fn same(&self) -> bool {
        self.differences.is_empty()
            && self.unlisted == 0
            && self.a_to_b.beyond_area == 0.0
            && self.b_to_a.beyond_area == 0.0
    }
}

/// Spreads `n` independent jobs over the computer's threads.
fn spread(n: usize, job: &(dyn Fn(usize) + Sync)) {
    let threads = std::thread::available_parallelism()
        .map_or(1, |t| t.get())
        .min(n);
    if threads <= 1 {
        (0..n).for_each(job);
        return;
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    job(i);
                }
            });
        }
    });
}

fn deviation(d: &sg::compare::Deviation) -> Deviation {
    Deviation {
        max: d.max,
        at: p3(d.at),
        mean: d.mean,
        beyond_area: d.beyond_area,
        area: d.area,
    }
}

/// Compares the closed meshes `a` and `b` (one frame): distances up to `tolerance` (mm) are
/// agreement; surfaces are sampled at most `spacing` (mm) apart.
pub fn compare(a: &[[Point3; 3]], b: &[[Point3; 3]], tolerance: Real, spacing: Real) -> Comparison {
    let o = sg::compare::CompareOptions {
        tolerance,
        spacing,
        ..Default::default()
    };
    let (ma, mb) = (mesh_of(a), mesh_of(b));
    let Ok(c) = guard(|| sg::compare::compare(&ma, &mb, &o, &|n, job| spread(n, job))) else {
        return Comparison::default();
    };
    Comparison {
        a_to_b: deviation(&c.a_to_b),
        b_to_a: deviation(&c.b_to_a),
        differences: c
            .regions
            .iter()
            .map(|r| Difference {
                only_in_a: r.side == sg::compare::Side::OnlyInA,
                volume: r.volume,
                center: p3(r.center),
                max_deviation: r.max_deviation,
            })
            .collect(),
        unlisted: c.unlisted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 12 triangles of an axis-aligned box, facing out.
    fn cuboid(lo: [Real; 3], hi: [Real; 3]) -> Vec<[Point3; 3]> {
        let p = |i: usize| {
            Point3::new(
                if i & 1 == 0 { lo[0] } else { hi[0] },
                if i & 2 == 0 { lo[1] } else { hi[1] },
                if i & 4 == 0 { lo[2] } else { hi[2] },
            )
        };
        let quads = [
            [0, 2, 3, 1],
            [4, 5, 7, 6],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 4, 6, 2],
            [1, 3, 7, 5],
        ];
        quads
            .iter()
            .flat_map(|q| [[p(q[0]), p(q[1]), p(q[2])], [p(q[0]), p(q[2]), p(q[3])]])
            .collect()
    }

    #[test]
    fn closest_of_two_boxes_is_their_gap() {
        let a = cuboid([0.0; 3], [10.0; 3]);
        let b = cuboid([13.0, 2.0, 2.0], [20.0, 8.0, 8.0]);
        let c = closest(&a, &b).unwrap();
        assert!((c.distance - 3.0).abs() < 1e-12, "{c:?}");
        assert!((c.a.x - 10.0).abs() < 1e-12 && (c.b.x - 13.0).abs() < 1e-12);
        // Edge against edge: a box turned so that only its edge faces the other's.
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let turned: Vec<[Point3; 3]> = cuboid([-1.0; 3], [1.0; 3])
            .iter()
            .map(|t| {
                t.map(|p| Point3::new(s * (p.x - p.y) + 15.0, s * (p.x + p.y) + 5.0, p.z + 5.0))
            })
            .collect();
        let c = closest(&a, &turned).unwrap();
        assert!((c.distance - (5.0 - 2.0 * s)).abs() < 1e-9, "{c:?}");
    }

    #[test]
    fn crossing_boxes_touch_and_within_lists_pairs() {
        let a = Triangles::new(&cuboid([0.0; 3], [10.0; 3]));
        let b = Triangles::new(&cuboid([5.0; 3], [15.0; 3]));
        assert_eq!(a.closest(&b, Real::INFINITY).unwrap().distance, 0.0);
        let far = Triangles::new(&cuboid([30.0; 3], [40.0; 3]));
        assert!(a.closest(&far, 5.0).is_none(), "beyond the limit");
        assert!(a.within(&far, 5.0, 100).is_empty());
        assert!(!a.within(&b, 0.5, 100).is_empty());
    }

    #[test]
    fn compare_finds_the_added_material() {
        let a = cuboid([0.0; 3], [10.0; 3]);
        // The same block 2 mm taller: 10 × 10 × 2 = 200 mm³ only in b.
        let b = cuboid([0.0; 3], [10.0, 10.0, 12.0]);
        let c = compare(&a, &b, 0.01, 0.5);
        assert!(!c.same());
        let d = c.differences.first().unwrap();
        assert!(!d.only_in_a);
        assert!((d.volume - 200.0).abs() < 2.0, "{d:?}");
        assert!((d.max_deviation - 2.0).abs() < 1e-6, "{d:?}");
        assert!(compare(&a, &a, 0.01, 0.5).same());
    }
}
