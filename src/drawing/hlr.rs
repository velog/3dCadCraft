//! Hidden line removal of a view (spec §6.1, option A2): the model's edges
//! and the silhouettes of its curved faces, cut into visible and hidden
//! pieces by testing points against the display triangles.
//!
//! The view is orthographic, so a point is hidden when a triangle that faces
//! the viewer covers it on the view plane and lies nearer. Triangles are
//! binned into a uniform grid on the view plane; each piece of an edge is
//! classified by its middle. First version: the edges come from the display
//! mesh (polylines), so circles are polylines here; exact curves and edge
//! names follow with dimensions (D3).

use std::collections::HashMap;

use super::ViewFrame;
use crate::kernel::{Point3, SolidMesh, TessellationOptions, Vector3};
use crate::part::PartDocument;

/// The lines of one view, on the view plane (model mm, not scaled).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewLines {
    /// Visible pieces.
    pub visible: Vec<Vec<[f64; 2]>>,
    /// Hidden pieces.
    pub hidden: Vec<Vec<[f64; 2]>>,
    /// Lower left of everything (visible and hidden).
    pub min: [f64; 2],
    /// Upper right.
    pub max: [f64; 2],
}

impl ViewLines {
    /// Middle of the extent: the point placed at the view's position.
    pub fn center(&self) -> [f64; 2] {
        [
            (self.min[0] + self.max[0]) * 0.5,
            (self.min[1] + self.max[1]) * 0.5,
        ]
    }

    /// Width and height of the extent.
    pub fn size(&self) -> (f64, f64) {
        (self.max[0] - self.min[0], self.max[1] - self.min[1])
    }

    /// Whether nothing was found.
    pub fn is_empty(&self) -> bool {
        self.visible.is_empty() && self.hidden.is_empty()
    }
}

/// Display meshes of the visible solid bodies of `part`.
pub fn part_meshes(part: &PartDocument) -> Vec<SolidMesh> {
    let opts = TessellationOptions::default();
    part.body
        .solids()
        .filter(|(_, s)| s.name.as_deref().is_none_or(|n| part.body_visible(n)))
        .filter(|(_, s)| !s.open)
        .filter_map(|(sid, _)| part.body.tessellate_solid(sid, &opts).ok())
        .collect()
}

/// One triangle on the view plane.
struct Tri {
    p: [[f64; 2]; 3],
    d: [f64; 3],
}

impl Tri {
    /// Depth at `q` when `q` lies inside (edges included).
    fn depth_at(&self, q: [f64; 2]) -> Option<f64> {
        let [a, b, c] = self.p;
        let det = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
        if det.abs() < 1e-18 {
            return None;
        }
        let l1 = ((q[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (q[1] - a[1])) / det;
        let l2 = ((b[0] - a[0]) * (q[1] - a[1]) - (q[0] - a[0]) * (b[1] - a[1])) / det;
        let l0 = 1.0 - l1 - l2;
        const E: f64 = -1e-9;
        (l0 >= E && l1 >= E && l2 >= E).then(|| l0 * self.d[0] + l1 * self.d[1] + l2 * self.d[2])
    }
}

/// Triangles binned on the view plane.
struct Grid {
    tris: Vec<Tri>,
    lo: [f64; 2],
    cell: f64,
    n: [usize; 2],
    cells: Vec<Vec<u32>>,
}

impl Grid {
    fn new(tris: Vec<Tri>, lo: [f64; 2], hi: [f64; 2]) -> Self {
        let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-6);
        let per = ((tris.len() as f64).sqrt() * 1.5).clamp(8.0, 256.0);
        let cell = span / per;
        let n = [
            (((hi[0] - lo[0]) / cell).ceil() as usize).clamp(1, 512),
            (((hi[1] - lo[1]) / cell).ceil() as usize).clamp(1, 512),
        ];
        let mut cells = vec![Vec::new(); n[0] * n[1]];
        for (k, t) in tris.iter().enumerate() {
            let (mut a, mut b) = ([f64::MAX; 2], [f64::MIN; 2]);
            for p in &t.p {
                for i in 0..2 {
                    a[i] = a[i].min(p[i]);
                    b[i] = b[i].max(p[i]);
                }
            }
            let (i0, j0) = Self::index_of(lo, cell, n, a);
            let (i1, j1) = Self::index_of(lo, cell, n, b);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    cells[j * n[0] + i].push(k as u32);
                }
            }
        }
        Self {
            tris,
            lo,
            cell,
            n,
            cells,
        }
    }

    fn index_of(lo: [f64; 2], cell: f64, n: [usize; 2], p: [f64; 2]) -> (usize, usize) {
        let i = ((p[0] - lo[0]) / cell).floor().max(0.0) as usize;
        let j = ((p[1] - lo[1]) / cell).floor().max(0.0) as usize;
        (i.min(n[0] - 1), j.min(n[1] - 1))
    }

    /// Whether a triangle nearer than `depth + eps` covers `q`.
    fn hidden(&self, q: [f64; 2], depth: f64, eps: f64) -> bool {
        let (i, j) = Self::index_of(self.lo, self.cell, self.n, q);
        self.cells[j * self.n[0] + i].iter().any(|&k| {
            self.tris[k as usize]
                .depth_at(q)
                .is_some_and(|d| d > depth + eps)
        })
    }
}

/// The visible and hidden lines of `meshes` seen in `frame`.
pub fn view_lines(meshes: &[SolidMesh], frame: &ViewFrame) -> ViewLines {
    let t = frame.toward;
    let p2 = |p: &Point3| frame.project(p);
    let dp = |p: &Point3| frame.depth(p);

    // Extent of everything (3D box → diagonal for tolerances).
    let (mut lo3, mut hi3) = (Point3::new(f64::MAX, f64::MAX, f64::MAX), Point3::origin());
    hi3.coords = Vector3::repeat(f64::MIN);
    for m in meshes {
        for p in &m.faces.positions {
            lo3 = lo3.inf(p);
            hi3 = hi3.sup(p);
        }
        for e in &m.edges {
            for p in e {
                lo3 = lo3.inf(p);
                hi3 = hi3.sup(p);
            }
        }
    }
    if lo3.x > hi3.x {
        return ViewLines::default();
    }
    let diag = (hi3 - lo3).norm().max(1e-6);
    let eps = 0.02 + diag * 1e-5;

    // Front-facing triangles occlude (the solids are closed).
    let mut tris = Vec::new();
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for m in meshes {
        let pos = &m.faces.positions;
        for tri in &m.faces.triangles {
            let [a, b, c] = tri.map(|i| pos[i as usize]);
            let nrm = (b - a).cross(&(c - a));
            if nrm.dot(&t) <= 1e-12 * nrm.norm().max(1e-30) {
                continue;
            }
            let p = [p2(&a), p2(&b), p2(&c)];
            for q in &p {
                for i in 0..2 {
                    lo[i] = lo[i].min(q[i]);
                    hi[i] = hi[i].max(q[i]);
                }
            }
            tris.push(Tri {
                p,
                d: [dp(&a), dp(&b), dp(&c)],
            });
        }
    }
    let grid = (!tris.is_empty()).then(|| Grid::new(tris, lo, hi));

    // Model edges and silhouettes as 3D polylines.
    let mut curves: Vec<Vec<Point3>> = meshes.iter().flat_map(|m| m.edges.clone()).collect();
    curves.extend(silhouettes(meshes, &t, diag));

    let step = diag / 600.0;
    // Every curve cut into short pieces, each classified by its middle.
    let mut pieces: Vec<Vec<Piece>> = Vec::with_capacity(curves.len());
    for c in &curves {
        let mut list = Vec::new();
        for w in c.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (a2, b2) = (p2(&a), p2(&b));
            let len = (b2[0] - a2[0]).hypot(b2[1] - a2[1]);
            if len < 1e-9 {
                continue; // an edge seen end-on
            }
            let n = ((len / step).ceil() as usize).clamp(1, 4096);
            for k in 0..n {
                let s0 = k as f64 / n as f64;
                let s1 = (k + 1) as f64 / n as f64;
                let mid = a + (b - a) * ((s0 + s1) * 0.5);
                let hidden = grid
                    .as_ref()
                    .is_some_and(|g| g.hidden(p2(&mid), dp(&mid), eps));
                list.push(Piece {
                    a: p2(&(a + (b - a) * s0)),
                    b: p2(&(a + (b - a) * s1)),
                    hidden,
                });
            }
        }
        pieces.push(list);
    }
    // A piece lying on a visible one is not drawn again, and a hidden piece
    // on a visible one not at all (the visible line wins, as on paper).
    // Edges of one circle sampled by different features differ by the
    // display tolerance (0.01 mm).
    let tol = 0.025 + diag * 1e-5;
    let cell = step.max(1e-6) * 4.0;
    let mut seen = SegGrid::new(cell);
    let mut drop = vec![Vec::new(); pieces.len()];
    for (k, list) in pieces.iter().enumerate() {
        for p in list {
            let dup = !p.hidden && seen.covers(p, tol);
            drop[k].push(dup);
            if !p.hidden && !dup {
                seen.insert(p);
            }
        }
    }
    let mut seen_hidden = SegGrid::new(cell);
    for (k, list) in pieces.iter().enumerate() {
        for (i, p) in list.iter().enumerate() {
            if p.hidden {
                let gone = seen.covers(p, tol) || seen_hidden.covers(p, tol);
                drop[k][i] = gone;
                if !gone {
                    seen_hidden.insert(p);
                }
            }
        }
    }
    let mut out = ViewLines::default();
    let (mut olo, mut ohi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for (list, gone) in pieces.iter().zip(&drop) {
        let mut run: Vec<[f64; 2]> = Vec::new();
        let mut run_hidden = false;
        for (p, &covered) in list.iter().zip(gone) {
            for q in [p.a, p.b] {
                for i in 0..2 {
                    olo[i] = olo[i].min(q[i]);
                    ohi[i] = ohi[i].max(q[i]);
                }
            }
            let joined = run
                .last()
                .is_some_and(|l| (l[0] - p.a[0]).hypot(l[1] - p.a[1]) < 1e-12);
            if covered || p.hidden != run_hidden || !joined {
                flush(&mut run, run_hidden, &mut out);
            }
            if covered {
                continue;
            }
            if run.is_empty() {
                run.push(p.a);
                run_hidden = p.hidden;
            }
            run.push(p.b);
        }
        flush(&mut run, run_hidden, &mut out);
    }
    if olo[0] <= ohi[0] {
        out.min = olo;
        out.max = ohi;
    }
    out
}

/// A short piece of a projected curve.
struct Piece {
    a: [f64; 2],
    b: [f64; 2],
    hidden: bool,
}

/// Moves a finished run into the visible or hidden lines.
fn flush(run: &mut Vec<[f64; 2]>, hidden: bool, out: &mut ViewLines) {
    if run.len() >= 2 {
        let piece = simplify(std::mem::take(run));
        if hidden {
            out.hidden.push(piece);
        } else {
            out.visible.push(piece);
        }
    }
    run.clear();
}

/// Drops the points of a run that lie on the line through their
/// neighbours (the pieces of one straight segment): fewer points to draw,
/// and dash patterns run on instead of restarting at every sub-pixel piece.
fn simplify(run: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    if run.len() < 3 {
        return run;
    }
    let mut out = vec![run[0]];
    for k in 1..run.len() - 1 {
        let (a, b, c) = (out[out.len() - 1], run[k], run[k + 1]);
        let (ux, uy) = (b[0] - a[0], b[1] - a[1]);
        let (vx, vy) = (c[0] - a[0], c[1] - a[1]);
        let len = vx.hypot(vy);
        // Distance of b from the chord a–c, and b between a and c.
        let off = (ux * vy - uy * vx).abs() / len.max(1e-300);
        let along = ux * vx + uy * vy;
        if len < 1e-12 || off > 1e-9 * (1.0 + len) || along < 0.0 || along > len * len {
            out.push(b);
        }
    }
    out.push(run[run.len() - 1]);
    out
}

/// A segment on the view plane.
type Seg = ([f64; 2], [f64; 2]);

/// Pieces binned on the view plane, to find pieces lying on them.
struct SegGrid {
    cell: f64,
    map: HashMap<(i64, i64), Vec<Seg>>,
}

impl SegGrid {
    fn new(cell: f64) -> Self {
        Self {
            cell,
            map: HashMap::new(),
        }
    }

    /// Adds a piece to every cell its box touches.
    fn insert(&mut self, p: &Piece) {
        let cell = self.cell;
        let (x0, x1) = (p.a[0].min(p.b[0]), p.a[0].max(p.b[0]));
        let (y0, y1) = (p.a[1].min(p.b[1]), p.a[1].max(p.b[1]));
        let (i0, i1) = ((x0 / cell).floor() as i64, (x1 / cell).floor() as i64);
        let (j0, j1) = ((y0 / cell).floor() as i64, (y1 / cell).floor() as i64);
        for i in i0..=i1 {
            for j in j0..=j1 {
                self.map.entry((i, j)).or_default().push((p.a, p.b));
            }
        }
    }

    /// Whether both ends and the middle of `p` lie within `tol` of visible
    /// pieces.
    fn covers(&self, p: &Piece, tol: f64) -> bool {
        let mid = [(p.a[0] + p.b[0]) * 0.5, (p.a[1] + p.b[1]) * 0.5];
        [p.a, mid, p.b].iter().all(|q| self.near(*q, tol))
    }

    fn near(&self, q: [f64; 2], tol: f64) -> bool {
        let (i, j) = (
            (q[0] / self.cell).floor() as i64,
            (q[1] / self.cell).floor() as i64,
        );
        (i - 1..=i + 1).any(|i| {
            (j - 1..=j + 1).any(|j| {
                self.map
                    .get(&(i, j))
                    .is_some_and(|v| v.iter().any(|(a, b)| seg_dist(q, *a, *b) <= tol))
            })
        })
    }
}

/// Distance from `q` to the segment `a`–`b`.
fn seg_dist(q: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-24 {
        0.0
    } else {
        (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
    };
    (q[0] - a[0] - t * dx).hypot(q[1] - a[1] - t * dy)
}

/// Mesh edges between two smoothly joined triangles where one faces the
/// viewer and the other does not: the outline of curved faces.
fn silhouettes(meshes: &[SolidMesh], toward: &Vector3, diag: f64) -> Vec<Vec<Point3>> {
    let q = diag * 1e-7;
    let key = |p: &Point3| {
        (
            (p.x / q).round() as i64,
            (p.y / q).round() as i64,
            (p.z / q).round() as i64,
        )
    };
    // Welded vertex ids across faces (a cylinder comes in several faces).
    let mut ids: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut pts: Vec<Point3> = Vec::new();
    let mut normals: Vec<Vector3> = Vec::new();
    let mut edges: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
    for m in meshes {
        let pos = &m.faces.positions;
        for tri in &m.faces.triangles {
            let p = tri.map(|i| pos[i as usize]);
            let n = (p[1] - p[0]).cross(&(p[2] - p[0]));
            if n.norm() < 1e-18 {
                continue;
            }
            let t = normals.len() as u32;
            normals.push(n.normalize());
            let w = p.map(|x| {
                let len = pts.len() as u32;
                *ids.entry(key(&x)).or_insert_with(|| {
                    pts.push(x);
                    len
                })
            });
            for (a, b) in [(w[0], w[1]), (w[1], w[2]), (w[2], w[0])] {
                if a != b {
                    edges.entry((a.min(b), a.max(b))).or_default().push(t);
                }
            }
        }
    }
    let mut out = Vec::new();
    for ((a, b), ts) in &edges {
        if ts.len() != 2 {
            continue;
        }
        let (n1, n2) = (normals[ts[0] as usize], normals[ts[1] as usize]);
        // Sharp edges are model edges already.
        if n1.dot(&n2) < 0.7 {
            continue;
        }
        if n1.dot(toward) * n2.dot(toward) < 0.0 {
            out.push(vec![pts[*a as usize], pts[*b as usize]]);
        }
    }
    out
}
