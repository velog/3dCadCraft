//! Hidden line removal of a view (spec §6.1, option A2): the model's edges
//! and the silhouettes of its curved faces, cut into visible and hidden
//! pieces by testing points against the display triangles.
//!
//! The view is orthographic, so a point is hidden when a triangle that faces
//! the viewer covers it on the view plane and lies nearer. Triangles are
//! binned into a uniform grid on the view plane; each piece of an edge is
//! classified by its middle. Each piece keeps the model edge it comes from
//! (`ViewLines::edges`: persistent name, projected end points, a circle seen
//! face-on with its exact centre and radius), so annotations attach to edges
//! by name and survive a model change.

use std::collections::HashMap;

use super::ViewFrame;
use crate::kernel::{Curve, EdgeId, Point3, SolidMesh, TessellationOptions, Vector3};
use crate::part::PartDocument;

/// The lines of one view, on the view plane (model mm, not scaled).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewLines {
    /// Visible pieces.
    pub visible: Vec<Vec<[f64; 2]>>,
    /// The model edge of each visible piece (index into `edges`; `None`:
    /// a silhouette).
    pub visible_src: Vec<Option<u32>>,
    /// Hidden pieces.
    pub hidden: Vec<Vec<[f64; 2]>>,
    /// The model edge of each hidden piece.
    pub hidden_src: Vec<Option<u32>>,
    /// The model's edges as seen in this view.
    pub edges: Vec<ViewEdge>,
    /// Section views: the cut faces' outlines (closed loops), hatched.
    pub section: Vec<Vec<[f64; 2]>>,
    /// The view-plane point placed at the view's position when it is not
    /// the extent's middle (section and auxiliary views stay aligned with
    /// their parent; detail views centre on their fence).
    pub anchor: Option<[f64; 2]>,
    /// Lower left of everything (visible and hidden).
    pub min: [f64; 2],
    /// Upper right.
    pub max: [f64; 2],
}

impl ViewLines {
    /// The point placed at the view's position: the anchor, else the
    /// middle of the extent.
    pub fn center(&self) -> [f64; 2] {
        self.anchor.unwrap_or([
            (self.min[0] + self.max[0]) * 0.5,
            (self.min[1] + self.max[1]) * 0.5,
        ])
    }

    /// Width and height of the extent.
    pub fn size(&self) -> (f64, f64) {
        (self.max[0] - self.min[0], self.max[1] - self.min[1])
    }

    /// Whether nothing was found.
    pub fn is_empty(&self) -> bool {
        self.visible.is_empty() && self.hidden.is_empty()
    }

    /// The edge named `name`.
    pub fn edge(&self, name: &str) -> Option<&ViewEdge> {
        self.edges.iter().find(|e| e.name.as_deref() == Some(name))
    }
}

/// A model edge seen in a view (view-plane mm, not scaled).
#[derive(Clone, Debug, PartialEq)]
pub struct ViewEdge {
    /// Persistent name (`Box1:E000-100`); `None` for unnamed edges.
    pub name: Option<String>,
    /// Start point.
    pub a: [f64; 2],
    /// End point (equal to `a` for a whole circle).
    pub b: [f64; 2],
    /// A circle or arc seen face-on: centre and radius.
    pub circle: Option<([f64; 2], f64)>,
    /// Its pieces together close the circle (the kernel splits circles into
    /// halves or quarters): a whole circle for dimensions.
    pub full_circle: bool,
    /// Straight in the view (a line, or a circle seen edge-on).
    pub straight: bool,
    /// Some piece of it is visible.
    pub visible: bool,
    /// A hole's note (Hole and Thread notes), when the edge is a hole's
    /// circle.
    pub note: Option<String>,
}

/// What a view needs of a part: the display triangles and the model's
/// visible edges with their names and curves.
#[derive(Clone, Debug, Default)]
pub struct ViewModel {
    /// Display meshes of the visible closed bodies.
    pub meshes: Vec<SolidMesh>,
    /// The edges drawn (no seams inside one smooth face).
    pub edges: Vec<ModelEdge>,
}

/// One model edge.
#[derive(Clone, Debug)]
pub struct ModelEdge {
    /// Persistent name.
    pub name: Option<String>,
    /// Sampled points, start to end.
    pub pts: Vec<Point3>,
    /// The circle it lies on: centre, unit normal, radius.
    pub circle: Option<(Point3, Vector3, f64)>,
    /// A straight line.
    pub line: bool,
    /// The note of the hole whose axis this circle sits on.
    pub note: Option<String>,
}

/// The note of the hole whose axis a circle (centre, normal) lies on.
fn hole_note(part: &PartDocument, (center, normal, _): (Point3, Vector3, f64)) -> Option<String> {
    part.holes().iter().find_map(|h| {
        let d = h.direction.normalize();
        let off = center - h.origin;
        let across = (off - d * off.dot(&d)).norm();
        (normal.dot(&d).abs() > 0.999 && across < 1e-4).then(|| {
            h.spec
                .note_lines(2)
                .iter()
                .map(|l| crate::part::hole::ascii(l))
                .collect::<Vec<_>>()
                .join("\n")
        })
    })
}

/// Display meshes of the visible solid bodies of `part`.
pub fn part_meshes(part: &PartDocument) -> Vec<SolidMesh> {
    part_model(part).meshes
}

/// The meshes and named edges of the visible closed bodies of `part`.
pub fn part_model(part: &PartDocument) -> ViewModel {
    let opts = TessellationOptions::default();
    let body = &part.body;
    let mut out = ViewModel::default();
    let mut seen: std::collections::HashSet<EdgeId> = Default::default();
    for (sid, solid) in body.solids() {
        if solid.open || solid.name.as_deref().is_some_and(|n| !part.body_visible(n)) {
            continue;
        }
        let Ok(mesh) = body.tessellate_solid(sid, &opts) else {
            continue;
        };
        out.meshes.push(mesh);
        // The faces of each edge, to leave out seams (as the display does).
        let mut uses: HashMap<
            EdgeId,
            Vec<(crate::kernel::FaceId, Option<crate::kernel::SurfaceId>)>,
        > = HashMap::new();
        for shell in &solid.shells {
            for &fid in &shell.faces {
                if let Some(face) = body.face(fid) {
                    for lp in std::iter::once(&face.outer).chain(&face.inner) {
                        for ce in &lp.coedges {
                            uses.entry(ce.edge).or_default().push((fid, face.facet_of));
                        }
                    }
                }
            }
        }
        let mut ids: Vec<EdgeId> = uses.keys().copied().collect();
        ids.sort();
        for e in ids {
            let f = &uses[&e];
            let seam = f.len() == 2
                && (f[0].0 == f[1].0
                    || (f[0].1.is_some() && f[0].1 == f[1].1)
                    || body.is_smooth_seam(f[0].0, f[1].0));
            if seam || !seen.insert(e) {
                continue;
            }
            let Some(edge) = body.edge(e) else {
                continue;
            };
            let Ok(pts) = body.sample_edge(e, &opts) else {
                continue;
            };
            let curve = body.curve(edge.curve);
            let circle = match curve {
                Some(Curve::Circle(c)) => Some((c.center, c.normal.into_inner(), c.radius)),
                _ => None,
            };
            out.edges.push(ModelEdge {
                name: edge.name.clone(),
                pts,
                circle,
                line: matches!(curve, Some(Curve::Line(_))),
                note: circle.and_then(|c| hole_note(part, c)),
            });
        }
    }
    out
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

/// A view square to a line of its parent (section, auxiliary): the
/// parent's frame and the line's direction there keep the two aligned, and
/// a section removes the material on the viewer's side of the cut.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aligned {
    /// The parent's frame.
    pub parent: ViewFrame,
    /// The line's direction in the parent's view plane (unit).
    pub line: [f64; 2],
    /// Section views: a point of the cutting plane (its normal is the
    /// view's `toward`).
    pub cut: Option<Point3>,
}

/// The visible and hidden lines of `model` seen in `frame`.
pub fn view_lines(model: &ViewModel, frame: &ViewFrame) -> ViewLines {
    view_lines_with(model, frame, None)
}

/// [`view_lines`] of a section or auxiliary view.
pub fn view_lines_with(
    model: &ViewModel,
    frame: &ViewFrame,
    aligned: Option<&Aligned>,
) -> ViewLines {
    let meshes = &model.meshes;
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
    }
    for e in &model.edges {
        for p in &e.pts {
            lo3 = lo3.inf(p);
            hi3 = hi3.sup(p);
        }
    }
    if lo3.x > hi3.x {
        return ViewLines::default();
    }
    let diag = (hi3 - lo3).norm().max(1e-6);
    let eps = 0.02 + diag * 1e-5;
    // A section keeps the material behind the cutting plane (seen from the
    // viewer, who stands on the removed side).
    let cut = aligned.and_then(|a| a.cut);
    let cut_depth = cut.map(|p| dp(&p));
    let side = |q: &Point3| cut_depth.map_or(-1.0, |c| dp(q) - c);

    // Front-facing triangles occlude (the solids are closed); a section
    // clips them to the kept side and collects where they cross the plane.
    let mut tris = Vec::new();
    let mut crossings: Vec<([f64; 2], [f64; 2])> = Vec::new();
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for m in meshes {
        let pos = &m.faces.positions;
        for tri in &m.faces.triangles {
            let [a, b, c] = tri.map(|i| pos[i as usize]);
            let nrm = (b - a).cross(&(c - a));
            let poly: Vec<Point3> = if cut.is_some() {
                let (kept, cross) = clip_polygon(&[a, b, c], &side);
                if let Some((x, y)) = cross {
                    crossings.push((p2(&x), p2(&y)));
                }
                kept
            } else {
                vec![a, b, c]
            };
            if poly.len() < 3 || nrm.dot(&t) <= 1e-12 * nrm.norm().max(1e-30) {
                continue;
            }
            for k in 1..poly.len() - 1 {
                let (a, b, c) = (poly[0], poly[k], poly[k + 1]);
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
    }
    let grid = (!tris.is_empty()).then(|| Grid::new(tris, lo, hi));
    // The cut faces: the crossings chained into loops.
    let section = chain_loops(&crossings, diag * 1e-7);
    let closed: Vec<&Vec<[f64; 2]>> = section
        .iter()
        .filter(|l| l.len() > 3 && l.first() == l.last())
        .collect();
    // Behind the plane and inside a cut face: hidden by the cut face.
    let behind_cap = |q: [f64; 2], d: f64| {
        cut_depth.is_some_and(|c| d < c - eps)
            && closed.iter().filter(|l| inside(l, q)).count() % 2 == 1
    };

    // Model edges, silhouettes and the cut faces' outlines as 3D polylines,
    // each with its model edge.
    let mut curves: Vec<(Vec<Point3>, Option<u32>)> = Vec::new();
    for (k, e) in model.edges.iter().enumerate() {
        for part in clip_polyline(&e.pts, &side) {
            curves.push((part, Some(k as u32)));
        }
    }
    for s in silhouettes(meshes, &t, diag) {
        for part in clip_polyline(&s, &side) {
            curves.push((part, None));
        }
    }
    if let Some(c) = cut_depth {
        let lift =
            |q: &[f64; 2]| Point3::from(frame.right * q[0] + frame.up * q[1] + frame.toward * c);
        for l in &section {
            curves.push((l.iter().map(lift).collect(), None));
        }
    }

    let step = diag / 600.0;
    // Every curve cut into short pieces, each classified by its middle.
    let mut pieces: Vec<Vec<Piece>> = Vec::with_capacity(curves.len());
    for (c, _) in &curves {
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
                let (m2, md) = (p2(&mid), dp(&mid));
                let hidden =
                    grid.as_ref().is_some_and(|g| g.hidden(m2, md, eps)) || behind_cap(m2, md);
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
    for (k, (list, gone)) in pieces.iter().zip(&drop).enumerate() {
        let src = curves[k].1;
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
                flush(&mut run, run_hidden, src, &mut out);
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
        flush(&mut run, run_hidden, src, &mut out);
    }
    if olo[0] <= ohi[0] {
        out.min = olo;
        out.max = ohi;
    }
    // The edges as the view sees them.
    let visible_edges: std::collections::HashSet<u32> =
        out.visible_src.iter().flatten().copied().collect();
    out.edges = model
        .edges
        .iter()
        .enumerate()
        .map(|(k, e)| {
            let a = e.pts.first().map_or([0.0; 2], p2);
            let b = e.pts.last().map_or([0.0; 2], p2);
            let face_on = e.circle.filter(|(_, n, _)| n.dot(&t).abs() > 1.0 - 1e-9);
            let edge_on = e.circle.is_some_and(|(_, n, _)| n.dot(&t).abs() < 1e-9);
            ViewEdge {
                name: e.name.clone(),
                a,
                b,
                circle: face_on.map(|(c, _, r)| (p2(&c), r)),
                straight: e.line || edge_on,
                visible: visible_edges.contains(&(k as u32)),
                note: e.note.clone(),
                full_circle: false,
            }
        })
        .collect();
    out.section = closed.into_iter().cloned().collect();
    mark_full_circles(&mut out.edges);
    // Aligned with the parent: the coordinate along the line is the
    // parent's (its middle), the one across it the view's own.
    if let Some(al) = aligned {
        let (mut plo, mut phi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for e in &model.edges {
            for p in &e.pts {
                let q = al.parent.project(p);
                for i in 0..2 {
                    plo[i] = plo[i].min(q[i]);
                    phi[i] = phi[i].max(q[i]);
                }
            }
        }
        if plo[0] <= phi[0] {
            let pc = [(plo[0] + phi[0]) * 0.5, (plo[1] + phi[1]) * 0.5];
            let p3 = Point3::from(al.parent.right * pc[0] + al.parent.up * pc[1]);
            let pcv = frame.project(&p3);
            let own = [
                (out.min[0] + out.max[0]) * 0.5,
                (out.min[1] + out.max[1]) * 0.5,
            ];
            let l = al.line;
            let m = [-l[1], l[0]];
            let a = pcv[0] * l[0] + pcv[1] * l[1];
            let b = own[0] * m[0] + own[1] * m[1];
            out.anchor = Some([l[0] * a + m[0] * b, l[1] * a + m[1] * b]);
        }
    }
    out
}

/// Marks the pieces of circles that close: the angles their pieces span
/// (end to end, each at most half a turn) add up to a full turn.
fn mark_full_circles(edges: &mut [ViewEdge]) {
    let key = |c: [f64; 2], r: f64| {
        let q = (r * 1e-4).max(1e-6);
        (
            (c[0] / q).round() as i64,
            (c[1] / q).round() as i64,
            (r / q).round() as i64,
        )
    };
    let mut span: HashMap<(i64, i64, i64), f64> = HashMap::new();
    for e in edges.iter() {
        let Some((c, r)) = e.circle else {
            continue;
        };
        let (u, v) = (
            [e.a[0] - c[0], e.a[1] - c[1]],
            [e.b[0] - c[0], e.b[1] - c[1]],
        );
        let ang = if (e.a[0] - e.b[0]).hypot(e.a[1] - e.b[1]) < r * 1e-6 {
            std::f64::consts::TAU
        } else {
            (u[0] * v[1] - u[1] * v[0])
                .atan2(u[0] * v[0] + u[1] * v[1])
                .abs()
        };
        *span.entry(key(c, r)).or_default() += ang;
    }
    for e in edges.iter_mut() {
        if let Some((c, r)) = e.circle {
            e.full_circle = span
                .get(&key(c, r))
                .is_some_and(|a| *a > std::f64::consts::TAU - 0.01);
        }
    }
}

/// Clips a convex polygon to `side(p) <= 0`; also returns the segment where
/// it crosses `side = 0` (two points), if it does.
fn clip_polygon(
    poly: &[Point3],
    side: &dyn Fn(&Point3) -> f64,
) -> (Vec<Point3>, Option<(Point3, Point3)>) {
    let n = poly.len();
    let d: Vec<f64> = poly.iter().map(side).collect();
    if d.iter().all(|v| *v <= 0.0) {
        return (poly.to_vec(), None);
    }
    let mut out = Vec::new();
    let mut cross = Vec::new();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let (da, db) = (d[i], d[(i + 1) % n]);
        if da <= 0.0 {
            out.push(a);
        }
        if (da <= 0.0) != (db <= 0.0) {
            let t = da / (da - db);
            let x = a + (b - a) * t;
            out.push(x);
            cross.push(x);
        }
    }
    let seg = (cross.len() == 2).then(|| (cross[0], cross[1]));
    (out, seg)
}

/// The parts of a polyline on the kept side (`side <= 0`).
fn clip_polyline(pts: &[Point3], side: &dyn Fn(&Point3) -> f64) -> Vec<Vec<Point3>> {
    let mut out = Vec::new();
    let mut cur: Vec<Point3> = Vec::new();
    for (k, p) in pts.iter().enumerate() {
        let dp = side(p);
        if k > 0 {
            let a = pts[k - 1];
            let da = side(&a);
            if (da <= 0.0) != (dp <= 0.0) {
                let t = da / (da - dp);
                let x = a + (*p - a) * t;
                cur.push(x);
                if dp > 0.0 {
                    out.push(std::mem::take(&mut cur));
                }
            }
        }
        if dp <= 0.0 {
            cur.push(*p);
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out.retain(|c| c.len() >= 2);
    out
}

/// Chains segments into polylines; closed loops repeat their first point.
fn chain_loops(segs: &[([f64; 2], [f64; 2])], q: f64) -> Vec<Vec<[f64; 2]>> {
    let q = q.max(1e-9);
    let key = |p: [f64; 2]| ((p[0] / q).round() as i64, (p[1] / q).round() as i64);
    let mut at: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (k, (a, b)) in segs.iter().enumerate() {
        if key(*a) == key(*b) {
            continue;
        }
        at.entry(key(*a)).or_default().push(k);
        at.entry(key(*b)).or_default().push(k);
    }
    let mut used = vec![false; segs.len()];
    let mut out = Vec::new();
    for start in 0..segs.len() {
        if used[start] || key(segs[start].0) == key(segs[start].1) {
            continue;
        }
        used[start] = true;
        let mut line = vec![segs[start].0, segs[start].1];
        // Grow forward, then backward.
        for forward in [true, false] {
            loop {
                let end = if forward {
                    line[line.len() - 1]
                } else {
                    line[0]
                };
                let next = at
                    .get(&key(end))
                    .and_then(|v| v.iter().find(|&&k| !used[k]).copied());
                let Some(k) = next else {
                    break;
                };
                used[k] = true;
                let (a, b) = segs[k];
                let other = if key(a) == key(end) { b } else { a };
                if forward {
                    line.push(other);
                } else {
                    line.insert(0, other);
                }
                if key(line[0]) == key(line[line.len() - 1]) {
                    break;
                }
            }
        }
        if key(line[0]) == key(line[line.len() - 1]) {
            let first = line[0];
            if let Some(l) = line.last_mut() {
                *l = first;
            }
        }
        out.push(line);
    }
    out
}

/// Even-odd point in polygon.
fn inside(poly: &[[f64; 2]], q: [f64; 2]) -> bool {
    let mut c = false;
    for w in poly.windows(2) {
        let (a, b) = (w[0], w[1]);
        if (a[1] > q[1]) != (b[1] > q[1]) {
            let x = a[0] + (q[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if q[0] < x {
                c = !c;
            }
        }
    }
    c
}

/// The lines of a view clipped to a circle (Detail View): `center` and
/// `radius` in the view plane; the result is centred on the fence.
pub fn clip_circle(lines: &ViewLines, center: [f64; 2], radius: f64) -> ViewLines {
    let clip = |polys: &[Vec<[f64; 2]>], srcs: &[Option<u32>]| {
        let mut out = Vec::new();
        let mut out_src = Vec::new();
        // Pieces finer than the fence, so a segment passing through the
        // circle with both ends outside is cut too.
        let step = (radius / 16.0).max(1e-6);
        let dense: Vec<Vec<[f64; 2]>> = polys
            .iter()
            .map(|pl| {
                let mut out = Vec::with_capacity(pl.len());
                for w in pl.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    let n = (((b[0] - a[0]).hypot(b[1] - a[1]) / step).ceil() as usize)
                        .clamp(1, 10_000);
                    for i in 0..n {
                        let t = i as f64 / n as f64;
                        out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
                    }
                }
                if let Some(last) = pl.last() {
                    out.push(*last);
                }
                out
            })
            .collect();
        for (k, pl) in dense.iter().enumerate() {
            let src = srcs.get(k).copied().flatten();
            let d = |p: [f64; 2]| (p[0] - center[0]).hypot(p[1] - center[1]) - radius;
            let mut cur: Vec<[f64; 2]> = Vec::new();
            for (i, p) in pl.iter().enumerate() {
                let dpv = d(*p);
                if i > 0 {
                    let a = pl[i - 1];
                    let da = d(a);
                    if (da <= 0.0) != (dpv <= 0.0) {
                        // Bisection for the crossing (the circle is not a line).
                        let (mut lo, mut hi) = (0.0, 1.0);
                        for _ in 0..40 {
                            let m = (lo + hi) * 0.5;
                            let x = [a[0] + (p[0] - a[0]) * m, a[1] + (p[1] - a[1]) * m];
                            if (d(x) <= 0.0) == (da <= 0.0) {
                                lo = m;
                            } else {
                                hi = m;
                            }
                        }
                        let x = [a[0] + (p[0] - a[0]) * lo, a[1] + (p[1] - a[1]) * lo];
                        cur.push(x);
                        if dpv > 0.0 {
                            if cur.len() >= 2 {
                                out.push(std::mem::take(&mut cur));
                                out_src.push(src);
                            }
                            cur.clear();
                        }
                    }
                }
                if dpv <= 0.0 {
                    cur.push(*p);
                }
            }
            if cur.len() >= 2 {
                out.push(cur);
                out_src.push(src);
            }
        }
        (out, out_src)
    };
    let (visible, visible_src) = clip(&lines.visible, &lines.visible_src);
    let (hidden, hidden_src) = clip(&lines.hidden, &lines.hidden_src);
    let (section, _) = clip(&lines.section, &[]);
    ViewLines {
        visible,
        visible_src,
        hidden,
        hidden_src,
        edges: lines.edges.clone(),
        section: section
            .into_iter()
            .filter(|l| l.first() == l.last())
            .collect(),
        min: [center[0] - radius, center[1] - radius],
        max: [center[0] + radius, center[1] + radius],
        anchor: None,
    }
}

/// A short piece of a projected curve.
struct Piece {
    a: [f64; 2],
    b: [f64; 2],
    hidden: bool,
}

/// Moves a finished run into the visible or hidden lines.
fn flush(run: &mut Vec<[f64; 2]>, hidden: bool, src: Option<u32>, out: &mut ViewLines) {
    if run.len() >= 2 {
        let piece = simplify(std::mem::take(run));
        if hidden {
            out.hidden.push(piece);
            out.hidden_src.push(src);
        } else {
            out.visible.push(piece);
            out.visible_src.push(src);
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
