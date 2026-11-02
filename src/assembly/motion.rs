//! Motion and contact of an assembly (spec 3.1b): contact sets (components that
//! stop each other when something moves), motion links (one relationship's value
//! follows another's: gears, rack and pinion), motion studies (values keyed along
//! a timeline) and exploded views (components moved apart for show only).
//!
//! Adapted from SolidCraft (`crates/doc/src/motion.rs`, `crates/engine/src/cmd/motion.rs`,
//! `contact_sweep.rs`; MIT OR Apache-2.0, see ATTRIBUTION.md) to our occurrences,
//! relationships and display triangles.

use serde::{Deserialize, Serialize};

use super::PickMesh;
use crate::kernel::{Point3, Real, Transform3, Vector3};

/// Occurrences whose bodies may touch but not pass through each other.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContactSet {
    /// Stable id.
    pub id: u32,
    /// Its name.
    pub name: String,
    /// The occurrences in it.
    pub occurrences: Vec<u32>,
    /// Kept, but not checked.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
}

/// How a motion link reads its ratio.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LinkKind {
    /// Inventor's Rotation: a turn drives a turn (gears).
    #[default]
    Rotation,
    /// Inventor's Rotation-Translation: a turn drives a slide (rack and pinion,
    /// a screw); the ratio is mm per radian.
    RotationTranslation,
    /// A slide drives a slide, or any other pair of values.
    Linear,
}

/// The driven relationship's value is `ratio · driver + offset`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MotionLink {
    /// Stable id.
    pub id: u32,
    /// The driver: a relationship's id.
    pub driver: u32,
    /// The driven relationship's id.
    pub driven: u32,
    /// What the ratio means.
    #[serde(default)]
    pub kind: LinkKind,
    /// The ratio (negative: Reverse).
    pub ratio: Real,
    /// The driven value when the driver reads 0.
    #[serde(default)]
    pub offset: Real,
    /// Kept, but not followed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
    /// The driver's last reading, to follow a turn past ±180° (runtime only).
    #[serde(skip)]
    pub last: Option<Real>,
}

impl MotionLink {
    /// The driven value for a driver value.
    pub fn driven_value(&self, driver: Real) -> Real {
        self.ratio * driver + self.offset
    }

    /// The driver value that gives a driven value (`None`: a ratio of 0).
    pub fn driver_value(&self, driven: Real) -> Option<Real> {
        (self.ratio.abs() > 1e-12).then(|| (driven - self.offset) / self.ratio)
    }
}

/// A reading of a turn continued from the last one (no jump at ±180°).
pub fn unwrapped(last: Option<Real>, now: Real) -> Real {
    let Some(last) = last else {
        return now;
    };
    let tau = std::f64::consts::TAU;
    let mut d = (now - last) % tau;
    if d > tau / 2.0 {
        d -= tau;
    } else if d < -tau / 2.0 {
        d += tau;
    }
    last + d
}

/// One relationship's value along a motion study: (step, value) points in internal
/// units (radians, mm), linear in between and held before the first and after the last.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StudyKey {
    /// The relationship's id.
    pub constraint: u32,
    /// Its keys, by step.
    pub points: Vec<(Real, Real)>,
}

impl StudyKey {
    /// The value at `step`.
    pub fn value_at(&self, step: Real) -> Option<Real> {
        let first = self.points.first()?;
        let last = self.points.last()?;
        if step <= first.0 {
            return Some(first.1);
        }
        if step >= last.0 {
            return Some(last.1);
        }
        self.points.windows(2).find_map(|w| {
            let (a, b) = (w.first()?, w.get(1)?);
            (step >= a.0 && step <= b.0).then(|| {
                if b.0 - a.0 < 1e-12 {
                    b.1
                } else {
                    a.1 + (b.1 - a.1) * (step - a.0) / (b.0 - a.0)
                }
            })
        })
    }
}

/// A motion study: a timeline of `steps` steps along which relationships follow
/// their keys.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MotionStudy {
    /// Stable id.
    pub id: u32,
    /// Its name.
    pub name: String,
    /// The timeline's length.
    pub steps: u32,
    /// One track per relationship.
    pub keys: Vec<StudyKey>,
}

/// An exploded view: occurrences moved apart for show (the design keeps its places).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExplodedView {
    /// Stable id.
    pub id: u32,
    /// Its name.
    pub name: String,
    /// (occurrence, translation).
    pub moves: Vec<(u32, [Real; 3])>,
}

/// Everything of this module an assembly stores.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MotionData {
    /// Motion links.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<MotionLink>,
    /// Activate Contact Solver: contact stops what moves.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub contact_enabled: bool,
    /// Every component is in contact with every other.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub contact_all: bool,
    /// Contact sets.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub contact_sets: Vec<ContactSet>,
    /// Motion studies.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub studies: Vec<MotionStudy>,
    /// Exploded views.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exploded: Vec<ExplodedView>,
}

impl MotionData {
    /// Nothing stored (skipped in the file).
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// An id none of its items has.
    pub fn next_id(&self) -> u32 {
        let links = self.links.iter().map(|x| x.id);
        let sets = self.contact_sets.iter().map(|x| x.id);
        let studies = self.studies.iter().map(|x| x.id);
        let views = self.exploded.iter().map(|x| x.id);
        links
            .chain(sets)
            .chain(studies)
            .chain(views)
            .max()
            .unwrap_or(0)
            + 1
    }

    /// The pairs of occurrences that must not pass through each other, among
    /// `occurrences` (their ids).
    pub fn contact_pairs(&self, occurrences: &[u32]) -> Vec<(u32, u32)> {
        if !self.contact_enabled {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut push = |set: &[u32]| {
            for (i, a) in set.iter().enumerate() {
                for b in &set[i + 1..] {
                    let pair = ((*a).min(*b), (*a).max(*b));
                    if a != b
                        && !out.contains(&pair)
                        && occurrences.contains(a)
                        && occurrences.contains(b)
                    {
                        out.push(pair);
                    }
                }
            }
        };
        if self.contact_all {
            push(occurrences);
        } else {
            for s in self.contact_sets.iter().filter(|s| !s.suppressed) {
                push(&s.occurrences);
            }
        }
        out
    }
}

/// How deep one placed mesh reaches into the other, when more than a touch (`None`:
/// apart or touching). Sample points of each (vertices, edge midpoints, triangle
/// centres) are tested against the other closed mesh: inside by ray parity, deeper
/// than `tol`. It samples the display triangles: not exact continuous collision
/// detection, and very thin details can slip through.
pub fn mesh_overlap(
    a: &PickMesh,
    ta: &Transform3,
    b: &PickMesh,
    tb: &Transform3,
    tol: Real,
) -> Option<Real> {
    let (ba, bb) = (placed_box(a, ta)?, placed_box(b, tb)?);
    for k in 0..3 {
        if ba.1[k] < bb.0[k] + tol || bb.1[k] < ba.0[k] + tol {
            return None;
        }
    }
    let depth = deepest(a, ta, b, tb, &bb, tol).max(deepest(b, tb, a, ta, &ba, tol));
    (depth > tol).then_some(depth)
}

/// The tolerance a touch is forgiven: the display meshes of a shaft and its hole
/// are chords of one circle, so they cross by their sag.
pub fn contact_tolerance(a: &PickMesh, b: &PickMesh) -> Real {
    let size = |m: &PickMesh| m.bbox.map_or(0.0, |(lo, hi)| (hi - lo).norm());
    0.05 + 2e-4 * size(a).max(size(b))
}

fn placed_box(m: &PickMesh, t: &Transform3) -> Option<(Point3, Point3)> {
    let (lo, hi) = m.bbox?;
    let mut out: Option<(Point3, Point3)> = None;
    for k in 0..8 {
        let q = Point3::new(
            if k & 1 == 0 { lo.x } else { hi.x },
            if k & 2 == 0 { lo.y } else { hi.y },
            if k & 4 == 0 { lo.z } else { hi.z },
        );
        let w = t * q;
        out = Some(out.map_or((w, w), |(l, h)| (l.inf(&w), h.sup(&w))));
    }
    out
}

/// Points tested per mesh pair (a drag checks every frame), evenly over the candidates.
const MAX_POINTS: usize = 1200;

/// A point of `a` inside `b` deeper than `tol` (the first found: what stops a motion is
/// that one exists), else 0.
fn deepest(
    a: &PickMesh,
    ta: &Transform3,
    b: &PickMesh,
    tb: &Transform3,
    bx: &(Point3, Point3),
    tol: Real,
) -> Real {
    let inv = tb.inverse();
    let inner = |p: &Point3| (0..3).all(|k| p[k] >= bx.0[k] - tol && p[k] <= bx.1[k] + tol);
    let tris = b.triangles();
    let mut candidates: Vec<Point3> = Vec::new();
    for (_, [p, q, r]) in a.triangles() {
        let (p, q, r) = (ta * p, ta * q, ta * r);
        for w in [
            p,
            Point3::from((p.coords + q.coords) * 0.5),
            Point3::from((q.coords + r.coords) * 0.5),
            Point3::from((r.coords + p.coords) * 0.5),
            Point3::from((p.coords + q.coords + r.coords) / 3.0),
        ] {
            if inner(&w) {
                candidates.push(w);
            }
        }
    }
    let stride = (candidates.len() / MAX_POINTS).max(1);
    for w in candidates.iter().step_by(stride) {
        // In `b`'s own coordinates: its triangles stay as they are.
        let p = inv * *w;
        if !inside(&p, tris) {
            continue;
        }
        let d = tris
            .iter()
            .map(|(_, [x, y, z])| (closest(&p, x, y, z) - p).norm())
            .fold(Real::MAX, Real::min);
        if d > tol {
            return d;
        }
    }
    0.0
}

/// Ray parity along a skew direction (no axis-aligned edge hits).
fn inside(p: &Point3, tris: &[(crate::kernel::FaceId, [Point3; 3])]) -> bool {
    let dir = Vector3::new(0.577_215_664_9, 0.327_118_441_9, 0.751_891_438_7);
    let mut n = 0;
    for (_, [a, b, c]) in tris {
        let (e1, e2) = (b - a, c - a);
        let h = dir.cross(&e2);
        let det = e1.dot(&h);
        if det.abs() < 1e-14 {
            continue;
        }
        let f = 1.0 / det;
        let sv = p - a;
        let u = f * sv.dot(&h);
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let q = sv.cross(&e1);
        let v = f * dir.dot(&q);
        if v < 0.0 || u + v > 1.0 {
            continue;
        }
        if f * e2.dot(&q) > 0.0 {
            n += 1;
        }
    }
    n % 2 == 1
}

/// The point of triangle abc nearest p.
fn closest(p: &Point3, a: &Point3, b: &Point3, c: &Point3) -> Point3 {
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
    let den = va + vb + vc;
    if den.abs() < 1e-300 {
        return *a;
    }
    a + ab * (vb / den) + ac * (vc / den)
}

/// The moves that push boxes apart from their common middle (`scale` times their
/// distance from it): Auto Explode.
pub fn auto_explode(centres: &[(u32, Point3)], scale: Real) -> Vec<(u32, [Real; 3])> {
    if centres.is_empty() {
        return Vec::new();
    }
    let mid = centres
        .iter()
        .fold(Vector3::zeros(), |a, (_, p)| a + p.coords)
        / centres.len() as Real;
    centres
        .iter()
        .map(|(o, p)| {
            let v = (p.coords - mid) * scale;
            (*o, [v.x, v.y, v.z])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::PartDocument;

    fn cube(size: f64) -> PickMesh {
        let p = PartDocument::with_box("Box1", [size, size, size]).unwrap();
        PickMesh::of(&p)
    }

    fn at(x: f64) -> Transform3 {
        Transform3::new(Vector3::new(x, 0.0, 0.0), Vector3::zeros())
    }

    #[test]
    fn keys_interpolate_and_hold() {
        let k = StudyKey {
            constraint: 1,
            points: vec![(0.0, 0.0), (10.0, 1.0), (20.0, -1.0)],
        };
        assert_eq!(k.value_at(-5.0), Some(0.0));
        assert_eq!(k.value_at(5.0), Some(0.5));
        assert_eq!(k.value_at(15.0), Some(0.0));
        assert_eq!(k.value_at(30.0), Some(-1.0));
        assert_eq!(
            StudyKey {
                constraint: 1,
                points: Vec::new()
            }
            .value_at(1.0),
            None
        );
    }

    #[test]
    fn links_map_both_ways_and_past_a_half_turn() {
        let l = MotionLink {
            id: 1,
            driver: 1,
            driven: 2,
            kind: LinkKind::Rotation,
            ratio: -2.0,
            offset: 1.0,
            suppressed: false,
            last: None,
        };
        assert!((l.driven_value(3.0) + 5.0).abs() < 1e-12);
        assert!((l.driver_value(-5.0).unwrap() - 3.0).abs() < 1e-12);
        let pi = std::f64::consts::PI;
        let a = unwrapped(Some(pi - 0.1), -pi + 0.1);
        assert!((a - (pi + 0.1)).abs() < 1e-9, "{a}");
        let b = unwrapped(Some(-pi + 0.1), pi - 0.1);
        assert!((b + pi + 0.1).abs() < 1e-9, "{b}");
    }

    #[test]
    fn apart_touching_and_passing_through() {
        let (a, b) = (cube(20.0), cube(20.0));
        let tol = contact_tolerance(&a, &b);
        // Apart.
        assert!(mesh_overlap(&a, &at(0.0), &b, &at(30.0), tol).is_none());
        // Face to face: touching is allowed.
        assert!(mesh_overlap(&a, &at(0.0), &b, &at(20.0), tol).is_none());
        // 5 mm into each other.
        let d = mesh_overlap(&a, &at(0.0), &b, &at(15.0), tol);
        assert!(d.is_some_and(|d| d > 1.0), "{d:?}");
        // Turned: still detected.
        let t = Transform3::new(Vector3::new(10.0, 0.0, 0.0), Vector3::new(0.3, 0.2, 0.1));
        assert!(mesh_overlap(&a, &at(0.0), &b, &t, tol).is_some());
    }

    #[test]
    fn pairs_come_from_the_active_sets_only() {
        let mut m = MotionData {
            contact_enabled: true,
            ..Default::default()
        };
        m.contact_sets.push(ContactSet {
            id: 1,
            name: "A".into(),
            occurrences: vec![1, 2, 3],
            suppressed: false,
        });
        m.contact_sets.push(ContactSet {
            id: 2,
            name: "B".into(),
            occurrences: vec![3, 4],
            suppressed: true,
        });
        assert_eq!(m.contact_pairs(&[1, 2, 3, 4]), vec![(1, 2), (1, 3), (2, 3)]);
        // A deleted occurrence leaves its pairs.
        assert_eq!(m.contact_pairs(&[1, 3]), vec![(1, 3)]);
        m.contact_enabled = false;
        assert!(m.contact_pairs(&[1, 2, 3]).is_empty());
        m.contact_enabled = true;
        m.contact_all = true;
        assert_eq!(m.contact_pairs(&[1, 2, 4]).len(), 3);
    }

    #[test]
    fn auto_explode_pushes_from_the_middle() {
        let v = auto_explode(
            &[
                (1, Point3::new(-10.0, 0.0, 0.0)),
                (2, Point3::new(10.0, 0.0, 0.0)),
            ],
            2.0,
        );
        assert_eq!(v, vec![(1, [-20.0, 0.0, 0.0]), (2, [20.0, 0.0, 0.0])]);
    }

    #[test]
    fn saved_and_read_back() {
        let mut m = MotionData::default();
        assert!(m.is_empty());
        m.links.push(MotionLink {
            id: 1,
            driver: 1,
            driven: 2,
            kind: LinkKind::RotationTranslation,
            ratio: 0.5,
            offset: 0.0,
            suppressed: false,
            last: Some(3.0),
        });
        let text = serde_json::to_string(&m).unwrap();
        let back: MotionData = serde_json::from_str(&text).unwrap();
        assert_eq!(back.links[0].ratio, 0.5);
        assert_eq!(back.links[0].last, None);
        assert_eq!(m.next_id(), 2);
    }
}
