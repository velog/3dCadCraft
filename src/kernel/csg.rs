//! Constructive solid geometry on polygons with a BSP tree (the classic
//! algorithm of Naylor / csg.js, written iteratively so deep trees cannot
//! overflow the stack).
//!
//! Every polygon carries a `tag` (the index of the face it comes from), so
//! the result can be rebuilt into a B-Rep whose faces keep their names and
//! exact surfaces (see `facet.rs` and `docs/spec/2.13-booleans.md`).
//! Polygons must be convex (triangles are); splitting keeps them convex.

use super::{Point3, Real, Vector3};

/// Distance below which a point lies on a plane (mm).
const EPS: Real = 1e-6;

/// A convex planar polygon, counter-clockwise seen from outside.
#[derive(Clone, Debug, PartialEq)]
pub struct CsgPolygon {
    /// Corners.
    pub pts: Vec<Point3>,
    /// Unit outward normal.
    pub normal: Vector3,
    /// `normal · p` for points of the plane.
    pub w: Real,
    /// Index of the face it comes from.
    pub tag: usize,
}

impl CsgPolygon {
    /// A polygon from corners (`None` if degenerate).
    pub fn new(pts: Vec<Point3>, tag: usize) -> Option<Self> {
        if pts.len() < 3 {
            return None;
        }
        // Newell's normal: robust for any (near-)planar polygon.
        let mut n = Vector3::zeros();
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            n += Vector3::new(
                (a.y - b.y) * (a.z + b.z),
                (a.z - b.z) * (a.x + b.x),
                (a.x - b.x) * (a.y + b.y),
            );
        }
        let len = n.norm();
        if len < 1e-12 {
            return None;
        }
        let normal = n / len;
        Some(Self {
            w: normal.dot(&pts[0].coords),
            normal,
            pts,
            tag,
        })
    }

    fn flip(&mut self) {
        self.pts.reverse();
        self.normal = -self.normal;
        self.w = -self.w;
    }
}

#[derive(Clone, Copy)]
struct Plane {
    n: Vector3,
    w: Real,
}

impl Plane {
    fn of(p: &CsgPolygon) -> Self {
        Plane {
            n: p.normal,
            w: p.w,
        }
    }
    fn flip(&mut self) {
        self.n = -self.n;
        self.w = -self.w;
    }
}

const COPLANAR: u8 = 0;
const FRONT: u8 = 1;
const BACK: u8 = 2;
const SPANNING: u8 = 3;

/// Splits `poly` by `plane` into the four lists (csg.js `splitPolygon`).
fn split(
    plane: &Plane,
    poly: &CsgPolygon,
    cop_front: &mut Vec<CsgPolygon>,
    cop_back: &mut Vec<CsgPolygon>,
    front: &mut Vec<CsgPolygon>,
    back: &mut Vec<CsgPolygon>,
) {
    let mut kind = 0u8;
    let types: Vec<u8> = poly
        .pts
        .iter()
        .map(|p| {
            let t = plane.n.dot(&p.coords) - plane.w;
            let k = if t < -EPS {
                BACK
            } else if t > EPS {
                FRONT
            } else {
                COPLANAR
            };
            kind |= k;
            k
        })
        .collect();
    match kind {
        COPLANAR => {
            if plane.n.dot(&poly.normal) > 0.0 {
                cop_front.push(poly.clone());
            } else {
                cop_back.push(poly.clone());
            }
        }
        FRONT => front.push(poly.clone()),
        BACK => back.push(poly.clone()),
        _ => {
            let (mut f, mut b) = (Vec::new(), Vec::new());
            let n = poly.pts.len();
            for i in 0..n {
                let j = (i + 1) % n;
                let (ti, tj) = (types[i], types[j]);
                let (vi, vj) = (poly.pts[i], poly.pts[j]);
                if ti != BACK {
                    f.push(vi);
                }
                if ti != FRONT {
                    b.push(vi);
                }
                if (ti | tj) == SPANNING {
                    let t = (plane.w - plane.n.dot(&vi.coords)) / plane.n.dot(&(vj - vi));
                    let v = vi + (vj - vi) * t;
                    f.push(v);
                    b.push(v);
                }
            }
            // Keep the original plane: splitting does not change it.
            let keep = |pts: Vec<Point3>| -> Option<CsgPolygon> {
                (pts.len() >= 3).then_some(CsgPolygon {
                    pts,
                    normal: poly.normal,
                    w: poly.w,
                    tag: poly.tag,
                })
            };
            front.extend(keep(f));
            back.extend(keep(b));
        }
    }
}

/// A BSP tree stored in an arena (indices instead of boxes).
struct Bsp {
    nodes: Vec<Node>,
}

#[derive(Clone)]
struct Node {
    plane: Option<Plane>,
    front: Option<usize>,
    back: Option<usize>,
    polygons: Vec<CsgPolygon>,
}

impl Bsp {
    fn new(polys: Vec<CsgPolygon>) -> Self {
        let mut t = Bsp {
            nodes: vec![Node {
                plane: None,
                front: None,
                back: None,
                polygons: Vec::new(),
            }],
        };
        t.build(0, polys);
        t
    }

    fn new_node(&mut self) -> usize {
        self.nodes.push(Node {
            plane: None,
            front: None,
            back: None,
            polygons: Vec::new(),
        });
        self.nodes.len() - 1
    }

    /// Adds polygons below `root` (csg.js `build`, iterative).
    fn build(&mut self, root: usize, polys: Vec<CsgPolygon>) {
        let mut stack = vec![(root, polys)];
        while let Some((id, polys)) = stack.pop() {
            if polys.is_empty() {
                continue;
            }
            let plane = *self.nodes[id]
                .plane
                .get_or_insert_with(|| Plane::of(&polys[0]));
            let (mut f, mut b) = (Vec::new(), Vec::new());
            let (mut cf, mut cb) = (Vec::new(), Vec::new());
            for p in &polys {
                split(&plane, p, &mut cf, &mut cb, &mut f, &mut b);
            }
            self.nodes[id].polygons.extend(cf);
            self.nodes[id].polygons.extend(cb);
            if !f.is_empty() {
                let c = match self.nodes[id].front {
                    Some(c) => c,
                    None => {
                        let c = self.new_node();
                        self.nodes[id].front = Some(c);
                        c
                    }
                };
                stack.push((c, f));
            }
            if !b.is_empty() {
                let c = match self.nodes[id].back {
                    Some(c) => c,
                    None => {
                        let c = self.new_node();
                        self.nodes[id].back = Some(c);
                        c
                    }
                };
                stack.push((c, b));
            }
        }
    }

    /// Solid ↔ empty space.
    fn invert(&mut self) {
        for n in &mut self.nodes {
            for p in &mut n.polygons {
                p.flip();
            }
            if let Some(pl) = n.plane.as_mut() {
                pl.flip();
            }
            std::mem::swap(&mut n.front, &mut n.back);
        }
    }

    /// The parts of `polys` outside this solid (csg.js `clipPolygons`).
    fn clip_polygons(&self, polys: Vec<CsgPolygon>) -> Vec<CsgPolygon> {
        let mut out = Vec::new();
        let mut stack = vec![(0usize, polys)];
        while let Some((id, polys)) = stack.pop() {
            let n = &self.nodes[id];
            let Some(plane) = n.plane else {
                out.extend(polys);
                continue;
            };
            let (mut f, mut b) = (Vec::new(), Vec::new());
            for p in &polys {
                let (mut cf, mut cb) = (Vec::new(), Vec::new());
                split(&plane, p, &mut cf, &mut cb, &mut f, &mut b);
                f.extend(cf);
                b.extend(cb);
            }
            match n.front {
                Some(c) => stack.push((c, f)),
                None => out.extend(f),
            }
            // Behind a leaf is inside: dropped.
            if let Some(c) = n.back {
                stack.push((c, b));
            }
        }
        out
    }

    /// Removes the parts of this tree's polygons inside `other`.
    fn clip_to(&mut self, other: &Bsp) {
        for i in 0..self.nodes.len() {
            let polys = std::mem::take(&mut self.nodes[i].polygons);
            self.nodes[i].polygons = other.clip_polygons(polys);
        }
    }

    fn all_polygons(&self) -> Vec<CsgPolygon> {
        self.nodes
            .iter()
            .flat_map(|n| n.polygons.iter().cloned())
            .collect()
    }
}

/// A ∪ B.
pub fn union(a: Vec<CsgPolygon>, b: Vec<CsgPolygon>) -> Vec<CsgPolygon> {
    let (mut a, mut b) = (Bsp::new(a), Bsp::new(b));
    a.clip_to(&b);
    b.clip_to(&a);
    b.invert();
    b.clip_to(&a);
    b.invert();
    let mut out = a.all_polygons();
    out.extend(b.all_polygons());
    out
}

/// A − B.
pub fn subtract(a: Vec<CsgPolygon>, b: Vec<CsgPolygon>) -> Vec<CsgPolygon> {
    let (mut a, mut b) = (Bsp::new(a), Bsp::new(b));
    a.invert();
    a.clip_to(&b);
    b.clip_to(&a);
    b.invert();
    b.clip_to(&a);
    b.invert();
    let mut out = a.all_polygons();
    out.extend(b.all_polygons());
    let mut r = Bsp::new(out);
    r.invert();
    r.all_polygons()
}

/// A ∩ B.
pub fn intersect(a: Vec<CsgPolygon>, b: Vec<CsgPolygon>) -> Vec<CsgPolygon> {
    let (mut a, mut b) = (Bsp::new(a), Bsp::new(b));
    a.invert();
    b.clip_to(&a);
    b.invert();
    a.clip_to(&b);
    b.clip_to(&a);
    let mut out = a.all_polygons();
    out.extend(b.all_polygons());
    let mut r = Bsp::new(out);
    r.invert();
    r.all_polygons()
}

/// Enclosed volume of a closed polygon set (divergence theorem).
pub fn volume(polys: &[CsgPolygon]) -> Real {
    polys
        .iter()
        .map(|p| {
            (1..p.pts.len() - 1)
                .map(|i| {
                    let (a, b, c) = (p.pts[0].coords, p.pts[i].coords, p.pts[i + 1].coords);
                    a.dot(&b.cross(&c)) / 6.0
                })
                .sum::<Real>()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Axis-aligned box as 6 quads.
    pub(crate) fn cube(lo: [Real; 3], hi: [Real; 3], tag: usize) -> Vec<CsgPolygon> {
        let c = |i: usize| {
            Point3::new(
                if i & 1 == 0 { lo[0] } else { hi[0] },
                if i & 2 == 0 { lo[1] } else { hi[1] },
                if i & 4 == 0 { lo[2] } else { hi[2] },
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
        .enumerate()
        .map(|(k, f)| CsgPolygon::new(f.iter().map(|&i| c(i)).collect(), tag * 10 + k).unwrap())
        .collect()
    }

    #[test]
    fn boolean_volumes() {
        let a = cube([0.0; 3], [10.0; 3], 0);
        let b = cube([5.0, 5.0, 5.0], [15.0, 15.0, 15.0], 1);
        assert!((volume(&a) - 1000.0).abs() < 1e-9);
        assert!((volume(&union(a.clone(), b.clone())) - 1875.0).abs() < 1e-6);
        assert!((volume(&subtract(a.clone(), b.clone())) - 875.0).abs() < 1e-6);
        assert!((volume(&intersect(a.clone(), b.clone())) - 125.0).abs() < 1e-6);
        // A hole through a plate.
        let plate = cube([0.0, 0.0, 0.0], [20.0, 20.0, 2.0], 0);
        let pin = cube([8.0, 8.0, -1.0], [12.0, 12.0, 3.0], 1);
        assert!((volume(&subtract(plate, pin)) - (800.0 - 32.0)).abs() < 1e-6);
        // Tags survive.
        let r = subtract(
            cube([0.0; 3], [10.0; 3], 0),
            cube([5.0, 5.0, 5.0], [15.0; 3], 1),
        );
        assert!(
            r.iter().any(|p| p.tag >= 10),
            "faces of the tool appear in the cut"
        );
    }
}
