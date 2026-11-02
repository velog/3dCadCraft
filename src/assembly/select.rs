//! Component selection modes of an assembly (`docs/spec/0.9a-selection-filters.md`
//! §6b), GUI-free: Component Size, Component Offset, Sphere Offset, Select by
//! Plane, External / Internal and All in Camera choose components by their
//! boxes and by what can be seen of them.

use std::collections::HashMap;
use std::sync::Arc;

use super::PickMesh;
use crate::kernel::{Point3, Real, Transform3, Vector3};

/// A top-level occurrence's box in assembly coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompBox {
    /// The occurrence.
    pub id: u32,
    /// Lowest corner.
    pub lo: Point3,
    /// Highest corner.
    pub hi: Point3,
}

impl CompBox {
    /// The box's diagonal: a component's size.
    pub fn size(&self) -> Real {
        (self.hi - self.lo).norm()
    }

    fn corners(&self) -> [Point3; 8] {
        let (a, b) = (self.lo, self.hi);
        [0, 1, 2, 3, 4, 5, 6, 7].map(|k| {
            Point3::new(
                if k & 1 == 0 { a.x } else { b.x },
                if k & 2 == 0 { a.y } else { b.y },
                if k & 4 == 0 { a.z } else { b.z },
            )
        })
    }
}

/// The largest component's size.
pub fn largest(boxes: &[CompBox]) -> Real {
    boxes.iter().map(CompBox::size).fold(0.0, Real::max)
}

/// Component Size: at least (or at most) `size`.
pub fn by_size(boxes: &[CompBox], size: Real, at_least: bool) -> Vec<u32> {
    boxes
        .iter()
        .filter(|b| {
            if at_least {
                b.size() >= size
            } else {
                b.size() <= size
            }
        })
        .map(|b| b.id)
        .collect()
}

/// The box around components `ids`.
pub fn union_of(boxes: &[CompBox], ids: &[u32]) -> Option<(Point3, Point3)> {
    boxes
        .iter()
        .filter(|b| ids.contains(&b.id))
        .map(|b| (b.lo, b.hi))
        .reduce(|(a, b), (c, d)| (a.inf(&c), b.sup(&d)))
}

/// Component Offset: inside the box `lo`..`hi` grown by `offset` (`partial`:
/// also what crosses its boundary).
pub fn in_box(boxes: &[CompBox], lo: Point3, hi: Point3, offset: Real, partial: bool) -> Vec<u32> {
    let grow = Vector3::new(offset, offset, offset);
    let (lo, hi) = (lo - grow, hi + grow);
    boxes
        .iter()
        .filter(|b| {
            let touches = b.lo.x <= hi.x
                && b.hi.x >= lo.x
                && b.lo.y <= hi.y
                && b.hi.y >= lo.y
                && b.lo.z <= hi.z
                && b.hi.z >= lo.z;
            let inside = b.lo.x >= lo.x
                && b.hi.x <= hi.x
                && b.lo.y >= lo.y
                && b.hi.y <= hi.y
                && b.lo.z >= lo.z
                && b.hi.z <= hi.z;
            if partial { touches } else { inside }
        })
        .map(|b| b.id)
        .collect()
}

/// Sphere Offset: inside the sphere (`partial`: also what crosses it).
pub fn in_sphere(boxes: &[CompBox], center: Point3, radius: Real, partial: bool) -> Vec<u32> {
    boxes
        .iter()
        .filter(|b| {
            if partial {
                let near = Point3::new(
                    center.x.clamp(b.lo.x, b.hi.x),
                    center.y.clamp(b.lo.y, b.hi.y),
                    center.z.clamp(b.lo.z, b.hi.z),
                );
                (near - center).norm() <= radius
            } else {
                b.corners().iter().all(|c| (c - center).norm() <= radius)
            }
        })
        .map(|b| b.id)
        .collect()
}

/// Select by Plane: wholly on the side `normal` points to.
pub fn by_plane(boxes: &[CompBox], point: Point3, normal: Vector3) -> Vec<u32> {
    boxes
        .iter()
        .filter(|b| b.corners().iter().all(|c| (c - point).dot(&normal) >= 0.0))
        .map(|b| b.id)
        .collect()
}

/// Whichever share of the rectangle `a` lies inside `view` (screen boxes as
/// `[min x, min y, max x, max y]`), 0..=1.
pub fn share_in_view(a: [f32; 4], view: [f32; 4]) -> f32 {
    let w = (a[2].min(view[2]) - a[0].max(view[0])).max(0.0);
    let h = (a[3].min(view[3]) - a[1].max(view[1])).max(0.0);
    let area = (a[2] - a[0]).max(1e-3) * (a[3] - a[1]).max(1e-3);
    (w * h / area).min(1.0)
}

/// A leaf part of a top-level occurrence, ready for rays on another thread.
#[derive(Clone)]
pub struct RayPart {
    /// The top-level occurrence it belongs to.
    pub top: u32,
    /// Part → assembly.
    pub place: Transform3,
    /// The part's triangles.
    pub mesh: Arc<PickMesh>,
    /// Its box in assembly coordinates.
    pub lo: Point3,
    /// Its box in assembly coordinates.
    pub hi: Point3,
}

fn ray_hits_box(o: &Point3, d: &Vector3, lo: &Point3, hi: &Point3) -> bool {
    let (mut near, mut far) = (Real::NEG_INFINITY, Real::INFINITY);
    for k in 0..3 {
        if d[k].abs() < 1e-12 {
            if o[k] < lo[k] || o[k] > hi[k] {
                return false;
            }
        } else {
            let (a, b) = ((lo[k] - o[k]) / d[k], (hi[k] - o[k]) / d[k]);
            near = near.max(a.min(b));
            far = far.min(a.max(b));
        }
    }
    near <= far && far >= 0.0
}

/// How much of each component can be seen from outside, in percent: rays
/// from the six sides of the assembly's box, on a grid of `n` × `n`; of the
/// rays that reach a component's box, the share that hits it first.
pub fn exposure(parts: &[RayPart], n: usize) -> HashMap<u32, f64> {
    let mut seen: HashMap<u32, (u32, u32)> = HashMap::new();
    let Some((lo, hi)) = parts
        .iter()
        .map(|p| (p.lo, p.hi))
        .reduce(|(a, b), (c, d)| (a.inf(&c), b.sup(&d)))
    else {
        return HashMap::new();
    };
    let size = hi - lo;
    let margin = size.norm().max(1.0);
    let n = n.max(1);
    for axis in 0..3 {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        for side in [false, true] {
            let mut d = Vector3::zeros();
            d[axis] = if side { -1.0 } else { 1.0 };
            for i in 0..n {
                for j in 0..n {
                    let mut o = lo;
                    o[axis] = if side {
                        hi[axis] + margin
                    } else {
                        lo[axis] - margin
                    };
                    o[u] = lo[u] + size[u] * (i as Real + 0.5) / n as Real;
                    o[v] = lo[v] + size[v] * (j as Real + 0.5) / n as Real;
                    let mut first: Option<(u32, Real)> = None;
                    let mut through: Vec<u32> = Vec::new();
                    for p in parts {
                        if !ray_hits_box(&o, &d, &p.lo, &p.hi) {
                            continue;
                        }
                        if !through.contains(&p.top) {
                            through.push(p.top);
                        }
                        let inv = p.place.inverse();
                        if let Some((_, t)) = p.mesh.hit(&(inv * o), &(inv * d))
                            && first.is_none_or(|(_, b)| t < b)
                        {
                            first = Some((p.top, t));
                        }
                    }
                    for id in through {
                        seen.entry(id).or_default().1 += 1;
                    }
                    if let Some((id, _)) = first {
                        seen.entry(id).or_default().0 += 1;
                    }
                }
            }
        }
    }
    parts
        .iter()
        .map(|p| {
            let (hit, all) = seen.get(&p.top).copied().unwrap_or((0, 0));
            let pct = if all == 0 {
                0.0
            } else {
                100.0 * f64::from(hit) / f64::from(all)
            };
            (p.top, pct)
        })
        .collect()
}

/// External: at least `percent` of it can be seen from outside.
pub fn external(seen: &HashMap<u32, f64>, percent: f64) -> Vec<u32> {
    let mut v: Vec<u32> = seen
        .iter()
        .filter(|(_, p)| **p >= percent)
        .map(|(id, _)| *id)
        .collect();
    v.sort_unstable();
    v
}

/// Internal: the inverse of [`external`].
pub fn internal(seen: &HashMap<u32, f64>, percent: f64) -> Vec<u32> {
    let mut v: Vec<u32> = seen
        .iter()
        .filter(|(_, p)| **p < percent)
        .map(|(id, _)| *id)
        .collect();
    v.sort_unstable();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cb(id: u32, x: Real, s: Real) -> CompBox {
        CompBox {
            id,
            lo: Point3::new(x, 0.0, 0.0),
            hi: Point3::new(x + s, s, s),
        }
    }

    #[test]
    fn size_picks_at_least_and_at_most() {
        let b = [cb(1, 0.0, 10.0), cb(2, 20.0, 1.0)];
        let big = b[0].size();
        assert_eq!(largest(&b), big);
        assert_eq!(by_size(&b, 5.0, true), vec![1]);
        assert_eq!(by_size(&b, 5.0, false), vec![2]);
        assert_eq!(by_size(&b, big, true), vec![1], "a boundary value counts");
    }

    #[test]
    fn box_offset_and_partial() {
        let b = [cb(1, 0.0, 10.0), cb(2, 12.0, 2.0), cb(3, 100.0, 2.0)];
        let (lo, hi) = union_of(&b, &[1]).unwrap_or((Point3::origin(), Point3::origin()));
        assert_eq!(in_box(&b, lo, hi, 0.0, false), vec![1]);
        assert_eq!(in_box(&b, lo, hi, 5.0, false), vec![1, 2]);
        assert_eq!(in_box(&b, lo, hi, 3.0, false), vec![1]);
        assert_eq!(in_box(&b, lo, hi, 3.0, true), vec![1, 2], "2 crosses it");
    }

    #[test]
    fn sphere_inside_and_crossing() {
        let b = [cb(1, 0.0, 1.0), cb(2, 10.0, 1.0)];
        let c = Point3::origin();
        assert_eq!(in_sphere(&b, c, 2.0, false), vec![1]);
        assert_eq!(in_sphere(&b, c, 10.5, false), vec![1]);
        assert_eq!(in_sphere(&b, c, 10.5, true), vec![1, 2]);
    }

    #[test]
    fn plane_takes_only_whole_components_and_flips() {
        let b = [cb(1, 0.0, 1.0), cb(2, 10.0, 1.0), cb(3, 4.5, 1.0)];
        let p = Point3::new(5.0, 0.0, 0.0);
        assert_eq!(by_plane(&b, p, Vector3::x()), vec![2]);
        assert_eq!(by_plane(&b, p, -Vector3::x()), vec![1], "3 is on the plane");
    }

    #[test]
    fn share_in_view_is_the_visible_part() {
        let view = [0.0, 0.0, 100.0, 100.0];
        assert!((share_in_view([10.0, 10.0, 20.0, 20.0], view) - 1.0).abs() < 1e-4);
        assert!((share_in_view([90.0, 0.0, 110.0, 10.0], view) - 0.5).abs() < 1e-4);
        assert_eq!(share_in_view([200.0, 0.0, 210.0, 10.0], view), 0.0);
    }

    #[test]
    fn inverse_of_external_is_internal() {
        let seen: HashMap<u32, f64> = [(1, 50.0), (2, 0.0), (3, 5.0)].into_iter().collect();
        assert_eq!(external(&seen, 5.0), vec![1, 3]);
        assert_eq!(internal(&seen, 5.0), vec![2]);
    }

    /// Measure between two points of two components (the owner's example):
    /// the same corner of two boxes placed 30, 40 and 0 mm apart.
    #[test]
    fn two_points_of_two_components_give_their_distance() {
        use crate::part::PartDocument;
        use crate::part::measure::{Target, measure_placed};
        let Ok(mut p) = PartDocument::with_box("Box1", [10.0, 10.0, 10.0]) else {
            panic!("the test part did not build");
        };
        if p.regenerate().is_err() {
            panic!("the test part did not regenerate");
        }
        let Some(name) = p.body.vertices().find_map(|(_, v)| v.name.clone()) else {
            panic!("no named vertex");
        };
        let a = Transform3::identity();
        let b = Transform3::translation(30.0, 40.0, 0.0);
        let items = [
            (&p.body, Target::Vertex(name.clone()), a),
            (&p.body, Target::Vertex(name), b),
        ];
        let Ok(m) = measure_placed(&items) else {
            panic!("two vertices must measure");
        };
        let Some((d, from, to)) = m.distance else {
            panic!("no distance: {m:?}");
        };
        assert!((d - 50.0).abs() < 1e-9, "{d}");
        assert!(((to - from) - Vector3::new(30.0, 40.0, 0.0)).norm() < 1e-9);
        // The way along the axes: X and Y, no Z, ending at the second point.
        let legs = crate::part::measure::axis_legs(&from, &to, 1e-6);
        assert_eq!(legs.iter().map(|l| l.0).collect::<Vec<_>>(), vec![0, 1]);
        assert!((legs[0].3 - 30.0).abs() < 1e-9 && (legs[1].3 - 40.0).abs() < 1e-9);
        assert!((legs[1].2 - to).norm() < 1e-9);
    }

    #[test]
    fn rays_find_a_box_inside_a_shell_hidden() {
        use crate::part::PartDocument;
        // One big box; a small one inside it cannot be seen.
        let solid = |id: &str, size: Real| -> Option<Arc<PickMesh>> {
            let mut p = PartDocument::with_box(id, [size, size, size]).ok()?;
            p.regenerate().ok()?;
            Some(Arc::new(PickMesh::of(&p)))
        };
        let (Some(big), Some(small)) = (solid("Box1", 100.0), solid("Box1", 10.0)) else {
            panic!("the test parts did not build");
        };
        let part = |top, mesh: &Arc<PickMesh>, at: Vector3, s: Real| RayPart {
            top,
            place: Transform3::translation(at.x, at.y, at.z),
            mesh: mesh.clone(),
            lo: Point3::from(at),
            hi: Point3::from(at) + Vector3::new(s, s, s),
        };
        let parts = [
            part(1, &big, Vector3::zeros(), 100.0),
            part(2, &small, Vector3::new(45.0, 45.0, 45.0), 10.0),
        ];
        let seen = exposure(&parts, 8);
        assert!(seen[&1] > 50.0, "{seen:?}");
        assert_eq!(seen[&2], 0.0, "{seen:?}");
        assert_eq!(external(&seen, 1.0), vec![1]);
        assert_eq!(internal(&seen, 1.0), vec![2]);
    }
}
