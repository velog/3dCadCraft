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
use crate::kernel::{Curve, EdgeId, Point3, SolidMesh, TessellationOptions, Transform3, Vector3};
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
    /// Flat pattern views: the bends.
    pub bends: Vec<ViewBend>,
    /// Break views: the removed stretches at the view's scale (empty
    /// until [`super::brk::apply`]); the lines stay in true coordinates.
    pub breaks: super::brk::BreakMap,
}

/// A bend of a flat pattern view (view plane).
#[derive(Clone, Debug, PartialEq)]
pub struct ViewBend {
    /// Centre line ends.
    pub centre: [[f64; 2]; 2],
    /// The bend zone's two edges (Bend Extents).
    pub zone: [[[f64; 2]; 2]; 2],
    /// Bends up (towards the viewer of the flat's top face).
    pub up: bool,
    /// Angle (degrees).
    pub angle: f64,
    /// Inner radius (mm).
    pub radius: f64,
    /// K-factor (from the bend allowance and the thickness).
    pub kfactor: f64,
}

impl ViewBend {
    /// The centre line's middle.
    pub fn middle(&self) -> [f64; 2] {
        [
            (self.centre[0][0] + self.centre[1][0]) * 0.5,
            (self.centre[0][1] + self.centre[1][1]) * 0.5,
        ]
    }
}

impl ViewLines {
    /// The point placed at the view's position: the anchor, else the
    /// middle of the extent.
    /// In a broken view it is a point of the shortened view
    /// ([`super::brk::BreakMap::shorten`]).
    pub fn center(&self) -> [f64; 2] {
        let b = &self.breaks;
        self.anchor.map(|a| b.shorten(a)).unwrap_or_else(|| {
            let (lo, hi) = (b.shorten(self.min), b.shorten(self.max));
            [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5]
        })
    }

    /// Width and height of the extent (shortened by breaks).
    pub fn size(&self) -> (f64, f64) {
        let (lo, hi) = (self.breaks.shorten(self.min), self.breaks.shorten(self.max));
        (hi[0] - lo[0], hi[1] - lo[1])
    }

    /// Whether nothing was found.
    pub fn is_empty(&self) -> bool {
        self.visible.is_empty() && self.hidden.is_empty()
    }

    /// The edge named `name`.
    pub fn edge(&self, name: &str) -> Option<&ViewEdge> {
        self.edges.iter().find(|e| e.name.as_deref() == Some(name))
    }

    /// The longest drawn piece (visible or hidden) of the edge named
    /// `name`: its points as the view shows them.
    pub fn edge_piece(&self, name: &str) -> Option<&[[f64; 2]]> {
        let k = self
            .edges
            .iter()
            .position(|e| e.name.as_deref() == Some(name))?;
        let k = u32::try_from(k).ok()?;
        let len = |pl: &[[f64; 2]]| -> f64 {
            pl.windows(2)
                .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
                .sum()
        };
        self.visible
            .iter()
            .zip(&self.visible_src)
            .chain(self.hidden.iter().zip(&self.hidden_src))
            .filter(|(_, s)| **s == Some(k))
            .map(|(pl, _)| pl.as_slice())
            .max_by(|a, b| len(a).total_cmp(&len(b)))
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
    /// That hole (`HoleInstance::key`): every circle of one hole shares it.
    pub hole: Option<String>,
}

/// What a view needs of a part: the display triangles and the model's
/// visible edges with their names and curves.
#[derive(Clone, Debug, Default)]
pub struct ViewModel {
    /// Display meshes of the visible closed bodies.
    pub meshes: Vec<SolidMesh>,
    /// The edges drawn (no seams inside one smooth face).
    pub edges: Vec<ModelEdge>,
    /// The meshes' smooth edges (silhouette candidates), computed once;
    /// `None`: computed by each view.
    pub smooth: Option<Vec<SmoothEdge>>,
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
    /// That hole's key (`HoleInstance::key`).
    pub hole: Option<String>,
}

/// One occurrence of a component in an assembly view: its id (edge names
/// get the prefix `o<id>/`), placement and the component's model (shared
/// by every occurrence of that component).
#[derive(Clone, Debug)]
pub struct Instance {
    /// Occurrence id (the top-level one for a part inside a subassembly).
    pub id: u32,
    /// The path below it to a part inside a subassembly (spec 3.1a; empty
    /// for a first-level part): its edges are `o<id>/s<a>.<b>/<edge name>`.
    pub sub: Vec<u32>,
    /// Placement in assembly coordinates.
    pub transform: Transform3,
    /// The component's view model (computed once per component).
    pub model: std::sync::Arc<ViewModel>,
}

/// What a view leaves out to stay fast on large models (spec 4.7 §3.7;
/// Inventor's *Hide small parts* of the Drawing View dialog).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Simplify {
    /// Components whose bounding box diagonal is below this (model mm) are
    /// not drawn; 0 keeps all.
    pub min_part: f64,
    /// Edges whose length is below this (model mm) are not drawn; 0 keeps
    /// all.
    pub min_edge: f64,
    /// Hidden lines are not computed (Hidden Line Removed views).
    pub no_hidden: bool,
}

/// The occurrence an assembly view's edge name belongs to (`o<id>/…`).
pub fn occurrence_of(edge: &str) -> Option<u32> {
    edge.strip_prefix('o')?.split_once('/')?.0.parse().ok()
}

/// Edge `n` of instance `inst` in an assembly view.
fn instance_edge(inst: &Instance, n: &str) -> String {
    if inst.sub.is_empty() {
        return format!("o{}/{n}", inst.id);
    }
    let sub: Vec<String> = inst.sub.iter().map(u32::to_string).collect();
    format!("o{}/s{}/{n}", inst.id, sub.join("."))
}

/// The diagonal of a model's bounding box (mm).
pub fn model_size(m: &ViewModel) -> f64 {
    let (mut lo, mut hi) = (Vector3::repeat(f64::MAX), Vector3::repeat(f64::MIN));
    for p in m.meshes.iter().flat_map(|m| &m.faces.positions) {
        lo = lo.inf(&p.coords);
        hi = hi.sup(&p.coords);
    }
    if lo.x > hi.x { 0.0 } else { (hi - lo).norm() }
}

/// An assembly's view model: every visible occurrence's component model
/// moved into place, its edges named `o<occurrence id>/<edge name>`.
/// Components smaller than `simplify.min_part` are left out.
pub fn assembly_model(instances: &[Instance], simplify: Simplify) -> ViewModel {
    let mut out = ViewModel::default();
    let mut sizes: HashMap<*const ViewModel, f64> = HashMap::new();
    for inst in instances {
        let m = inst.model.as_ref();
        if simplify.min_part > 0.0 {
            let size = *sizes
                .entry(m as *const ViewModel)
                .or_insert_with(|| model_size(m));
            if size < simplify.min_part {
                continue;
            }
        }
        let t = &inst.transform;
        // Only what hidden line removal reads: positions and triangles
        // (normals and edges stay with the component).
        for mesh in &m.meshes {
            let mut moved = SolidMesh::default();
            moved.faces.positions = mesh.faces.positions.iter().map(|q| t * *q).collect();
            moved.faces.triangles = mesh.faces.triangles.clone();
            out.meshes.push(moved);
        }
        let smooth = out.smooth.get_or_insert_with(Vec::new);
        let local;
        let own = match &m.smooth {
            Some(v) => v,
            None => {
                local = m.meshes.iter().flat_map(smooth_edges).collect::<Vec<_>>();
                &local
            }
        };
        smooth.extend(own.iter().map(|e| SmoothEdge {
            a: t * e.a,
            b: t * e.b,
            n1: t * e.n1,
            n2: t * e.n2,
        }));
        for e in &m.edges {
            out.edges.push(ModelEdge {
                name: e.name.as_ref().map(|n| instance_edge(inst, n)),
                pts: e.pts.iter().map(|q| t * *q).collect(),
                circle: e.circle.map(|(c, n, r)| (t * c, t * n, r)),
                line: e.line,
                note: e.note.clone(),
                hole: e.hole.as_ref().map(|h| format!("o{}/{h}", inst.id)),
            });
        }
    }
    out
}

/// `m` moved by `t` (a flat pattern into its own frame).
pub fn transformed(m: &ViewModel, t: &Transform3) -> ViewModel {
    let moved = assembly_model(
        &[Instance {
            id: 0,
            sub: Vec::new(),
            transform: *t,
            model: std::sync::Arc::new(m.clone()),
        }],
        Simplify::default(),
    );
    ViewModel {
        edges: moved
            .edges
            .into_iter()
            .map(|mut e| {
                e.name = e.name.map(|n| n.trim_start_matches("o0/").to_owned());
                e
            })
            .collect(),
        ..moved
    }
}

/// The flat pattern of `part`'s first sheet body: its view model moved into
/// the flat's own frame (top face towards +Z, flat x along +X), that move,
/// and its bend lines (part coordinates).
pub fn flat_model(
    part: &PartDocument,
) -> Option<(
    ViewModel,
    Transform3,
    Vec<crate::part::sheet_metal::FlatBendLines>,
)> {
    use crate::kernel::exact::sheet_metal as ksm;
    let f = part.flat_patterns().into_iter().next()?;
    let st = part.sheet_models.get(&f.body)?;
    let body = crate::kernel::exact::to_brep(std::slice::from_ref(&f.solid)).ok()?;
    let flat_part = PartDocument {
        body,
        ..Default::default()
    };
    let o = ksm::flat_point(&st.model, [0.0, 0.0], 0.0);
    let x = (ksm::flat_point(&st.model, [1.0, 0.0], 0.0) - o).normalize();
    let n = (ksm::flat_point(&st.model, [0.0, 0.0], 1.0) - o).normalize();
    let y = n.cross(&x);
    let rot = nalgebra::Rotation3::from_basis_unchecked(&[x, y, n]);
    let to_world = Transform3::from_parts(
        nalgebra::Translation3::from(o.coords),
        nalgebra::UnitQuaternion::from_rotation_matrix(&rot),
    );
    let to_flat = to_world.inverse();
    let model = transformed(&part_model(&flat_part), &to_flat);
    Some((model, to_flat, part.flat_bend_lines(&f.body)))
}

/// A flat pattern view's lines: the flat seen in `frame` with its bends
/// (`None`: `part` has no flat pattern).
pub fn flat_lines(
    part: &PartDocument,
    frame: &ViewFrame,
    aligned: Option<&Aligned>,
) -> Option<ViewLines> {
    let (model, to_flat, bends) = flat_model(part)?;
    let thickness = part
        .flat_patterns()
        .first()
        .and_then(|f| part.sheet_models.get(&f.body))
        .map_or(0.0, |st| {
            crate::kernel::exact::sheet_metal::thickness(&st.model)
        });
    let mut lines = view_lines_with(&model, frame, aligned);
    let p = |q: &Point3| frame.project(&(to_flat * q));
    lines.bends = bends
        .iter()
        .map(|b| ViewBend {
            centre: [p(&b.centre[0]), p(&b.centre[1])],
            zone: [
                [p(&b.zone[0][0]), p(&b.zone[0][1])],
                [p(&b.zone[1][0]), p(&b.zone[1][1])],
            ],
            up: b.bend.up,
            angle: b.bend.angle_deg,
            radius: b.bend.radius,
            // BA = θ (R + k T).
            kfactor: if thickness > 1e-9 && b.bend.angle_deg > 1e-9 {
                (b.bend.allowance / b.bend.angle_deg.to_radians() - b.bend.radius) / thickness
            } else {
                0.0
            },
        })
        .collect();
    Some(lines)
}

/// The hole whose axis a circle (centre, normal) lies on.
fn hole_of(
    part: &PartDocument,
    (center, normal, _): (Point3, Vector3, f64),
) -> Option<&crate::part::hole::HoleInstance> {
    part.holes().iter().find(|h| {
        let d = h.direction.normalize();
        let off = center - h.origin;
        let across = (off - d * off.dot(&d)).norm();
        normal.dot(&d).abs() > 0.999 && across < 1e-4
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
    // How many identical holes share each hole's plane (the notes' "4x").
    let counts: HashMap<String, usize> = part
        .hole_groups()
        .iter()
        .flat_map(|g| g.keys.iter().map(move |k| (k.clone(), g.count())))
        .collect();
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
            let hole = circle.and_then(|c| hole_of(part, c));
            out.edges.push(ModelEdge {
                name: edge.name.clone(),
                pts,
                circle,
                line: matches!(curve, Some(Curve::Line(_))),
                note: hole.map(|h| {
                    let mut lines: Vec<String> = h
                        .spec
                        .note_lines(2)
                        .iter()
                        .map(|l| crate::part::hole::ascii(l))
                        .collect();
                    // Every identical hole on the plane, from any feature.
                    let n = counts.get(&h.key()).copied().unwrap_or(1);
                    if n > 1
                        && let Some(first) = lines.first_mut()
                    {
                        *first = format!("{n}x {first}");
                    }
                    lines.join("\n")
                }),
                hole: hole.map(|h| h.key()),
            });
        }
    }
    out.smooth = Some(out.meshes.iter().flat_map(smooth_edges).collect());
    out
}

/// One triangle on the view plane.
struct Tri {
    p: [[f64; 2]; 3],
    d: [f64; 3],
}

impl Tri {
    /// The nearest depth of its corners.
    fn dmax(&self) -> f64 {
        self.d[0].max(self.d[1]).max(self.d[2])
    }

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
        // Square cells about two triangles each over the area (an assembly
        // seen from the front is long and flat: cells from the long side
        // only held thousands of triangles each), at most 4 M cells.
        let (w, h) = ((hi[0] - lo[0]).max(1e-6), (hi[1] - lo[1]).max(1e-6));
        let target = (tris.len() as f64 * 0.5).clamp(64.0, 4.0e6);
        let mut cell = (w * h / target).sqrt().max(w.max(h) * 1e-6);
        let mut n;
        loop {
            n = [
                ((w / cell).ceil() as usize).max(1),
                ((h / cell).ceil() as usize).max(1),
            ];
            if n[0] * n[1] <= 4_000_000 && n[0] <= 16_384 && n[1] <= 16_384 {
                break;
            }
            cell *= 1.5;
        }
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
        // Nearest first: a lookup stops at the first triangle that cannot
        // be in front of the point.
        for c in &mut cells {
            c.sort_unstable_by(|&x, &y| {
                tris[y as usize].dmax().total_cmp(&tris[x as usize].dmax())
            });
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
        for &k in &self.cells[j * self.n[0] + i] {
            let t = &self.tris[k as usize];
            if t.dmax() <= depth + eps {
                return false;
            }
            if t.depth_at(q).is_some_and(|d| d > depth + eps) {
                return true;
            }
        }
        false
    }
}

/// A view square to a line of its parent (section, auxiliary): the
/// parent's frame and the line's direction there keep the two aligned, and
/// a section removes the material on the viewer's side of the cut.
#[derive(Clone, Debug, PartialEq)]
pub struct Aligned {
    /// The parent's frame.
    pub parent: ViewFrame,
    /// The line's direction in the parent's view plane (unit).
    pub line: [f64; 2],
    /// Section views: a point of the cutting plane (its normal is the
    /// view's `toward`).
    pub cut: Option<Point3>,
    /// A section line of several segments in the parent's view plane
    /// (offset section, projected): each segment cuts along its own plane,
    /// square to the parent's plane; empty: the one plane through `cut`.
    pub poly: Vec<[f64; 2]>,
    /// Aligned method: the material behind each later segment is turned
    /// about its bends into the first segment's plane (true lengths of
    /// inclined segments) instead of being projected.
    pub unfold: bool,
    /// Section Depth Distance: what lies farther than this behind the
    /// first segment's plane is left out (model mm); `None`: Full.
    pub depth: Option<f64>,
    /// Break outs of the view (spec 4.11), in this view's plane. A view
    /// with only break outs has no parent line (`line` zero, no anchor).
    pub outs: Vec<OutCut>,
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
    view_lines_simplified(model, frame, aligned, Simplify::default())
}

/// [`view_lines_with`] leaving out edges shorter than `simplify.min_edge`
/// (components are left out before, in [`assembly_model`]).
pub fn view_lines_simplified(
    model: &ViewModel,
    frame: &ViewFrame,
    aligned: Option<&Aligned>,
    simplify: Simplify,
) -> ViewLines {
    if let Some(a) = aligned.filter(|a| a.unfold && a.cut.is_some() && a.poly.len() >= 3) {
        // Aligned: turn the model straight, then one plane through the
        // first segment.
        let (unfolded, start) = unfold_model(model, &a.parent, &a.poly);
        let straight = Aligned {
            parent: a.parent,
            line: a.line,
            cut: Some(start),
            poly: Vec::new(),
            unfold: false,
            depth: a.depth,
            outs: a.outs.clone(),
        };
        return view_lines_simplified(&unfolded, frame, Some(&straight), simplify);
    }
    let meshes = &model.meshes;
    let clock = std::time::Instant::now();
    let times = std::env::var_os("CADCRAFT3D_HLR_TIMES").is_some();
    let lap = |what: &str| {
        if times {
            eprintln!("hlr {what}: {:.0} ms", clock.elapsed().as_secs_f64() * 1e3);
        }
    };
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
    // An offset section: the removed region as convex cells of the
    // parent's plane, one per segment of the extended line.
    let cells: Option<(Vec<Cell>, ViewFrame)> = aligned
        .filter(|a| a.cut.is_some() && a.poly.len() >= 3)
        .map(|a| {
            let far = 20.0 * (diag + (hi3 - lo3).norm()) + 1.0;
            (section_cells(&a.poly, &a.parent, frame, far), a.parent)
        });
    // Depth Distance: a second plane, parallel to the view, `depth` behind
    // the first segment's; the material beyond it is left out.
    let far_depth = cut
        .zip(aligned.and_then(|a| a.depth))
        .map(|(p, d)| dp(&p) - d);
    let beyond = |q: &Point3| far_depth.map_or(-1.0, |f| f - dp(q));
    let cut = if cells.is_some() { None } else { cut };
    let cut_depth = cut.map(|p| dp(&p));
    let side = |q: &Point3| cut_depth.map_or(-1.0, |c| dp(q) - c);
    // Break outs: the profile's prism in front of each cut is removed.
    let outs: &[OutCut] = aligned.map_or(&[], |a| a.outs.as_slice());
    let out_cells: Vec<Vec<Cell>> = outs.iter().map(OutCut::cells).collect();
    let mut out_cross: Vec<Vec<([f64; 2], [f64; 2])>> = vec![Vec::new(); outs.len()];
    // The kept parts of a curve (model edges, silhouettes).
    let clip_curve = |pts: &[Point3]| -> Vec<Vec<Point3>> {
        let mut kept = match &cells {
            Some((c, pf)) => clip_curve_cells(pts, c, pf),
            None => clip_polyline(pts, &side),
        };
        if far_depth.is_some() {
            kept = kept
                .iter()
                .flat_map(|k| clip_polyline(k, &beyond))
                .collect();
        }
        if !outs.is_empty() {
            kept = kept
                .iter()
                .flat_map(|k| clip_outs(k, outs, frame))
                .collect();
        }
        kept
    };
    // Where the far plane cuts the faces: the outline of what is left
    // there (drawn, never hatched).
    let mut far_crossings: Vec<([f64; 2], [f64; 2])> = Vec::new();
    let mut chords3: Vec<(Point3, Point3)> = Vec::new();

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
            let polys: Vec<Vec<Point3>> = if let Some((cs, pf)) = &cells {
                let mut pieces = vec![vec![a, b, c]];
                for cell in cs {
                    let mut next = Vec::new();
                    for p in pieces {
                        next.extend(subtract_cell(p, cell, pf, &mut chords3));
                    }
                    pieces = next;
                    if pieces.is_empty() {
                        break;
                    }
                }
                pieces
            } else if cut.is_some() {
                let (kept, cross) = clip_polygon(&[a, b, c], &side);
                if let Some((x, y)) = cross {
                    crossings.push((p2(&x), p2(&y)));
                }
                vec![kept]
            } else {
                vec![vec![a, b, c]]
            };
            let polys: Vec<Vec<Point3>> = if far_depth.is_some() {
                polys
                    .into_iter()
                    .filter(|p| p.len() >= 3)
                    .map(|p| {
                        let (kept, cross) = clip_polygon(&p, &beyond);
                        if let Some((x, y)) = cross {
                            far_crossings.push((p2(&x), p2(&y)));
                        }
                        kept
                    })
                    .collect()
            } else {
                polys
            };
            let polys = if outs.is_empty() {
                polys
            } else {
                cut_outs(polys, outs, &out_cells, frame, &mut out_cross)
            };
            if nrm.dot(&t) <= 1e-12 * nrm.norm().max(1e-30) {
                continue;
            }
            for poly in polys.iter().filter(|p| p.len() >= 3) {
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
    }
    // Offset section: the cut faces' chords seen in the view, each end's
    // depth kept (its segment's plane).
    let mut chord_depth: std::collections::HashMap<(i64, i64), f64> = Default::default();
    let qk = (diag * 1e-7).max(1e-9);
    let key = |p: [f64; 2]| ((p[0] / qk).round() as i64, (p[1] / qk).round() as i64);
    for (x, y) in &chords3 {
        let (px, py) = (p2(x), p2(y));
        if (px[0] - py[0]).hypot(px[1] - py[1]) < 1e-12 {
            continue; // a jog's cut face, seen edge-on
        }
        chord_depth.insert(key(px), dp(x));
        chord_depth.insert(key(py), dp(y));
        crossings.push((px, py));
    }
    lap("tris");
    let grid = (!tris.is_empty()).then(|| Grid::new(tris, lo, hi));
    lap("grid");
    // The cut faces: the crossings chained into loops.
    let section = chain_loops(&crossings, diag * 1e-7);
    let closed: Vec<&Vec<[f64; 2]>> = section
        .iter()
        .filter(|l| l.len() > 3 && l.first() == l.last())
        .collect();
    // The depth of each cut face (offset sections: the farthest of its
    // planes).
    let loop_depth: Vec<f64> = closed
        .iter()
        .map(|l| {
            l.iter()
                .filter_map(|p| chord_depth.get(&key(*p)).copied())
                .fold(f64::MAX, f64::min)
        })
        .collect();
    // Break outs' cut faces: the section of the faces by each plane,
    // clipped to the profile (hatched pieces, one per profile triangle) and
    // their outlines inside the profile.
    type Caps<'a> = (
        &'a OutCut,
        Vec<Vec<[f64; 2]>>,
        Vec<([f64; 2], [f64; 2])>,
        ([f64; 2], [f64; 2]),
    );
    let mut out_caps: Vec<Caps> = Vec::new();
    let mut out_edges: Vec<(&OutCut, Vec<[f64; 2]>)> = Vec::new();
    for (o, segs) in outs.iter().zip(&out_cross) {
        let loops: Vec<Vec<[f64; 2]>> = chain_loops(segs, diag * 1e-7)
            .into_iter()
            .filter(|l| l.len() > 3 && l.first() == l.last())
            .collect();
        let mut caps = Vec::new();
        let loop_boxes: Vec<_> = loops.iter().map(box_of).collect();
        for tri in &o.tris {
            let tb = box_of(tri);
            for (l, lb) in loops.iter().zip(&loop_boxes) {
                if !boxes_meet(tb, *lb) {
                    continue;
                }
                let c = clip_to_triangle(l, tri);
                if c.len() >= 4 {
                    caps.push(c);
                }
            }
        }
        for l in &loops {
            for piece in inside_profile(l, o) {
                out_edges.push((o, piece));
            }
        }
        let boxes = caps.iter().map(box_of).collect();
        out_caps.push((o, caps, boxes, o.bounds()));
    }
    // Behind the plane and inside a cut face: hidden by the cut face.
    let behind_cap = |q: [f64; 2], d: f64| {
        let at = (q, q);
        let under_out = out_caps.iter().any(|(o, caps, boxes, all)| {
            d < o.depth_at(q) - eps
                && boxes_meet(at, *all)
                && caps
                    .iter()
                    .zip(boxes)
                    .filter(|(c, b)| boxes_meet(at, **b) && inside(c, q))
                    .count()
                    % 2
                    == 1
        });
        if under_out {
            return true;
        }
        if cells.is_some() {
            return closed
                .iter()
                .zip(&loop_depth)
                .any(|(l, ld)| *ld < f64::MAX && d < ld - eps && inside(l, q));
        }
        cut_depth.is_some_and(|c| d < c - eps)
            && closed.iter().filter(|l| inside(l, q)).count() % 2 == 1
    };

    // Model edges, silhouettes and the cut faces' outlines as 3D polylines,
    // each with its model edge.
    let mut curves: Vec<(Vec<Point3>, Option<u32>)> = Vec::new();
    let short = |pts: &[Point3]| {
        simplify.min_edge > 0.0
            && pts.windows(2).map(|w| (w[1] - w[0]).norm()).sum::<f64>() < simplify.min_edge
    };
    for (k, e) in model.edges.iter().enumerate() {
        if short(&e.pts) {
            continue;
        }
        for part in clip_curve(&e.pts) {
            curves.push((part, Some(k as u32)));
        }
    }
    lap("edges");
    for s in silhouettes(model, &t) {
        for part in clip_curve(&s) {
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
    if let Some(f) = far_depth {
        let lift =
            |q: &[f64; 2]| Point3::from(frame.right * q[0] + frame.up * q[1] + frame.toward * f);
        for l in chain_loops(&far_crossings, diag * 1e-7) {
            curves.push((l.iter().map(lift).collect(), None));
        }
    }
    for (o, l) in &out_edges {
        let lift = |q: &[f64; 2]| {
            Point3::from(frame.right * q[0] + frame.up * q[1] + frame.toward * o.depth_at(*q))
        };
        curves.push((l.iter().map(lift).collect(), None));
    }
    if cells.is_some() {
        // Each vertex at its own plane's depth.
        let lift = |q: &[f64; 2]| {
            let c = chord_depth.get(&key(*q)).copied().unwrap_or(0.0);
            Point3::from(frame.right * q[0] + frame.up * q[1] + frame.toward * c)
        };
        for l in &section {
            curves.push((l.iter().map(lift).collect(), None));
        }
    }

    lap("silhouettes");
    let step = diag / 600.0;
    // Every curve cut into short pieces, each classified by its middle (on
    // every core: the curves are independent).
    let classify = |c: &Vec<Point3>| {
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
                if hidden && simplify.no_hidden {
                    continue;
                }
                list.push(Piece {
                    a: p2(&(a + (b - a) * s0)),
                    b: p2(&(a + (b - a) * s1)),
                    hidden,
                });
            }
        }
        list
    };
    let pieces: Vec<Vec<Piece>> = par_map(&curves, |(c, _)| classify(c));
    lap("classify");
    // A piece lying on a visible one is not drawn again, and a hidden piece
    // on a visible one not at all (the visible line wins, as on paper).
    // Edges of one circle sampled by different features differ by the
    // display tolerance (0.01 mm).
    let tol = 0.025 + diag * 1e-5;
    // Only pieces within `tol` of each other are compared: small cells keep
    // a crowded view (an assembly seen isometric) from comparing thousands.
    let cell = (step * 0.25).max(tol * 8.0).max(1e-6);
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
    lap("dedup");
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
    lap("runs");
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
                hole: e.hole.clone(),
                full_circle: false,
            }
        })
        .collect();
    out.section = closed.into_iter().cloned().collect();
    // Break outs: the cut faces hatched, the profiles drawn.
    for (_, caps, _, _) in out_caps {
        out.section.extend(caps);
    }
    for o in outs {
        for l in &o.loops {
            for q in l {
                out.min = [out.min[0].min(q[0]), out.min[1].min(q[1])];
                out.max = [out.max[0].max(q[0]), out.max[1].max(q[1])];
            }
            out.visible.push(l.clone());
            out.visible_src.push(None);
        }
    }
    mark_full_circles(&mut out.edges);
    // Aligned with the parent: the coordinate along the line is the
    // parent's (its middle), the one across it the view's own.
    if let Some(al) = aligned.filter(|a| a.line != [0.0, 0.0]) {
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
/// A break out of a view (spec 4.11): inside `loops` (closed, the view
/// plane), the material in front of the cut plane (towards the viewer) is
/// cut away. The plane's depth at view-plane point `q` is
/// `depth + slope · q` (a plane square to the view has slope zero).
#[derive(Clone, Debug, PartialEq)]
pub struct OutCut {
    /// The profile's closed loops (view plane; even-odd inside).
    pub loops: Vec<Vec<[f64; 2]>>,
    /// The cut plane's depth (`ViewFrame::depth`): what is nearer the
    /// viewer is removed inside the profile.
    pub depth: f64,
    /// How the plane's depth changes along the view's x and y (Break Out
    /// To Sketch with an inclined line).
    pub slope: [f64; 2],
    /// The profile in counter-clockwise triangles (convex pieces).
    tris: Vec<[[f64; 2]; 3]>,
    /// The profile's box: lower left, upper right.
    lo: [f64; 2],
    hi: [f64; 2],
}

/// The box of view-plane points: lower left, upper right.
fn box_of<'a>(pts: impl IntoIterator<Item = &'a [f64; 2]>) -> ([f64; 2], [f64; 2]) {
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for q in pts {
        lo = [lo[0].min(q[0]), lo[1].min(q[1])];
        hi = [hi[0].max(q[0]), hi[1].max(q[1])];
    }
    (lo, hi)
}

/// Whether two boxes meet.
fn boxes_meet(a: ([f64; 2], [f64; 2]), b: ([f64; 2], [f64; 2])) -> bool {
    a.0[0] <= b.1[0] && b.0[0] <= a.1[0] && a.0[1] <= b.1[1] && b.0[1] <= a.1[1]
}

impl OutCut {
    /// A break out of `loops` (each closed or not; holes are loops inside
    /// others) cut at `depth`.
    pub fn new(loops: Vec<Vec<[f64; 2]>>, depth: f64) -> Self {
        let loops: Vec<Vec<[f64; 2]>> = loops
            .into_iter()
            .filter(|l| l.len() >= 3)
            .map(|mut l| {
                if l.first() != l.last()
                    && let Some(f) = l.first().copied()
                {
                    l.push(f);
                }
                l
            })
            .collect();
        let pt = |q: &[f64; 2]| nalgebra::Point2::new(q[0], q[1]);
        let mut tris = Vec::new();
        // Outer loops with the loops inside them as holes.
        for (k, l) in loops.iter().enumerate() {
            let probe = l[0];
            let depth_in = loops
                .iter()
                .enumerate()
                .filter(|(j, o)| *j != k && inside(o, probe))
                .count();
            if depth_in % 2 == 1 {
                continue; // a hole
            }
            let outer: Vec<_> = l[..l.len() - 1].iter().map(pt).collect();
            let holes: Vec<Vec<_>> = loops
                .iter()
                .enumerate()
                .filter(|(j, o)| *j != k && inside(l, o[0]))
                .map(|(_, o)| o[..o.len() - 1].iter().map(pt).collect())
                .collect();
            let (pts, ts) = crate::kernel::triangulate_with_holes(&outer, &holes);
            for t in ts {
                let [a, b, c] = t.map(|i| [pts[i].x, pts[i].y]);
                let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
                if area.abs() < 1e-12 {
                    continue;
                }
                tris.push(if area > 0.0 { [a, b, c] } else { [a, c, b] });
            }
        }
        let (lo, hi) = box_of(loops.iter().flatten());
        Self {
            loops,
            depth,
            slope: [0.0, 0.0],
            tris,
            lo,
            hi,
        }
    }

    /// The same break out cut by an inclined plane: its depth at `q` is
    /// `depth + slope · q`.
    pub fn sloped(mut self, slope: [f64; 2]) -> Self {
        self.slope = slope;
        self
    }

    /// The cut plane's depth at view-plane point `q`.
    pub fn depth_at(&self, q: [f64; 2]) -> f64 {
        self.depth + self.slope[0] * q[0] + self.slope[1] * q[1]
    }

    /// How far model point `p` lies in front of the cut plane (positive:
    /// on the removed side).
    fn front(&self, p: &Point3, frame: &ViewFrame) -> f64 {
        frame.depth(p) - self.depth_at(frame.project(p))
    }

    /// Whether view-plane point `q` lies inside the profile.
    pub fn contains(&self, q: [f64; 2]) -> bool {
        q[0] >= self.lo[0]
            && q[0] <= self.hi[0]
            && q[1] >= self.lo[1]
            && q[1] <= self.hi[1]
            && self.loops.iter().filter(|l| inside(l, q)).count() % 2 == 1
    }

    /// Its box (view plane).
    fn bounds(&self) -> ([f64; 2], [f64; 2]) {
        (self.lo, self.hi)
    }

    /// The profile's triangles as cells (the removed prism, seen along the
    /// view).
    fn cells(&self) -> Vec<Cell> {
        self.tris
            .iter()
            .map(|t| Cell {
                edges: (0..3)
                    .map(|i| {
                        let (a, b) = (t[i], t[(i + 1) % 3]);
                        (a, [b[1] - a[1], a[0] - b[0]], false)
                    })
                    .collect(),
            })
            .collect()
    }
}

/// Sutherland–Hodgman: the part of loop `lp` (closed or not) inside the
/// counter-clockwise triangle `t`, closed.
fn clip_to_triangle(lp: &[[f64; 2]], t: &[[f64; 2]; 3]) -> Vec<[f64; 2]> {
    let mut poly: Vec<[f64; 2]> = lp.to_vec();
    if poly.len() > 1 && poly.first() == poly.last() {
        poly.pop();
    }
    for i in 0..3 {
        let (a, b) = (t[i], t[(i + 1) % 3]);
        let side = |q: [f64; 2]| (b[0] - a[0]) * (q[1] - a[1]) - (b[1] - a[1]) * (q[0] - a[0]);
        let mut next = Vec::new();
        for k in 0..poly.len() {
            let (p, q) = (poly[k], poly[(k + 1) % poly.len()]);
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                next.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let s = sp / (sp - sq);
                next.push([p[0] + (q[0] - p[0]) * s, p[1] + (q[1] - p[1]) * s]);
            }
        }
        poly = next;
        if poly.len() < 3 {
            return Vec::new();
        }
    }
    if let Some(f) = poly.first().copied() {
        poly.push(f);
    }
    poly
}

/// The parts of view-plane polyline `pl` inside break out `o`.
fn inside_profile(pl: &[[f64; 2]], o: &OutCut) -> Vec<Vec<[f64; 2]>> {
    let mut out = Vec::new();
    let mut cur: Vec<[f64; 2]> = Vec::new();
    for w in pl.windows(2) {
        let (a, b) = (w[0], w[1]);
        let mut ts = vec![0.0, 1.0];
        for l in &o.loops {
            for e in l.windows(2) {
                if let Some(t) = seg_cross(a, b, e[0], e[1]) {
                    ts.push(t);
                }
            }
        }
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
        let at = |t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        for t in ts.windows(2) {
            if o.contains(at((t[0] + t[1]) * 0.5)) {
                if cur.is_empty() {
                    cur.push(at(t[0]));
                }
                cur.push(at(t[1]));
            } else if cur.len() >= 2 {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

/// Where segment `a`–`b` crosses segment `c`–`d`: the parameter on `a`–`b`.
fn seg_cross(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> Option<f64> {
    let r = [b[0] - a[0], b[1] - a[1]];
    let s = [d[0] - c[0], d[1] - c[1]];
    let den = r[0] * s[1] - r[1] * s[0];
    if den.abs() < 1e-15 {
        return None;
    }
    let ca = [c[0] - a[0], c[1] - a[1]];
    let t = (ca[0] * s[1] - ca[1] * s[0]) / den;
    let u = (ca[0] * r[1] - ca[1] * r[0]) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(t)
}

/// The parts of a 3D curve outside every break out's removed region.
fn clip_outs(pts: &[Point3], outs: &[OutCut], frame: &ViewFrame) -> Vec<Vec<Point3>> {
    let removed = |q: &Point3| {
        let v = frame.project(q);
        outs.iter()
            .any(|o| o.front(q, frame) > 0.0 && o.contains(v))
    };
    let mut out: Vec<Vec<Point3>> = Vec::new();
    let mut cur: Vec<Point3> = Vec::new();
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (va, vb) = (frame.project(&a), frame.project(&b));
        let mut ts = vec![0.0, 1.0];
        let seg_box = box_of([&va, &vb]);
        for o in outs {
            let (fa, fb) = (o.front(&a, frame), o.front(&b, frame));
            if fa * fb < 0.0 {
                ts.push(fa / (fa - fb));
            }
            if !boxes_meet(seg_box, o.bounds()) {
                continue;
            }
            for l in &o.loops {
                for e in l.windows(2) {
                    if let Some(t) = seg_cross(va, vb, e[0], e[1]) {
                        ts.push(t);
                    }
                }
            }
        }
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
        for t in ts.windows(2) {
            let mid = a + (b - a) * ((t[0] + t[1]) * 0.5);
            if removed(&mid) {
                if cur.len() >= 2 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
                continue;
            }
            let p = a + (b - a) * t[0];
            let q = a + (b - a) * t[1];
            if cur.last().is_none_or(|l| (l - p).norm() > 1e-12) {
                if cur.len() >= 2 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
                cur.push(p);
            }
            cur.push(q);
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

/// Break outs on the occluders: each polygon behind a break out's plane is
/// kept, the part in front of it loses the profile's prism; where the
/// polygons cross the plane is collected per break out (the cut faces).
fn cut_outs(
    polys: Vec<Vec<Point3>>,
    outs: &[OutCut],
    cells: &[Vec<Cell>],
    frame: &ViewFrame,
    cross: &mut [Vec<([f64; 2], [f64; 2])>],
) -> Vec<Vec<Point3>> {
    let mut cur = polys;
    let mut scratch = Vec::new();
    for (i, o) in outs.iter().enumerate() {
        let mut next = Vec::new();
        for p in cur.into_iter().filter(|p| p.len() >= 3) {
            let (back, c) = clip_polygon(&p, &|q: &Point3| o.front(q, frame));
            if let Some((x, y)) = c
                && let Some(list) = cross.get_mut(i)
            {
                list.push((frame.project(&x), frame.project(&y)));
            }
            if back.len() >= 3 {
                next.push(back);
            }
            let (front, _) = clip_polygon(&p, &|q: &Point3| -o.front(q, frame));
            if front.len() < 3 {
                continue;
            }
            let flat: Vec<[f64; 2]> = front.iter().map(|q| frame.project(q)).collect();
            if !boxes_meet(box_of(&flat), o.bounds()) {
                next.push(front);
                continue;
            }
            let mut pieces = vec![front];
            for cell in cells.get(i).map_or(&[][..], |c| c.as_slice()) {
                let mut left = Vec::new();
                for piece in pieces {
                    left.extend(subtract_cell(piece, cell, frame, &mut scratch));
                }
                pieces = left;
                if pieces.is_empty() {
                    break;
                }
            }
            next.extend(pieces);
        }
        cur = next;
    }
    cur
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

/// A convex cell of an offset section's removed region, in the parent's
/// view plane: its edges as (a point, the outward normal, lies on the
/// section line), the section line's edge last.
#[derive(Clone, Debug)]
pub struct Cell {
    edges: Vec<([f64; 2], [f64; 2], bool)>,
}

impl Cell {
    fn f(e: &([f64; 2], [f64; 2], bool), v: [f64; 2]) -> f64 {
        (v[0] - e.0[0]) * e.1[0] + (v[1] - e.0[1]) * e.1[1]
    }

    /// Whether `v` lies inside (or on) the cell.
    pub fn contains(&self, v: [f64; 2]) -> bool {
        self.edges.iter().all(|e| Self::f(e, v) <= 1e-12)
    }
}

/// The removed region of a section line `poly` (the parent's view plane):
/// the line extended along its end segments, then one convex cell per
/// segment on the viewer's side, bounded by the segment, the bisectors at
/// its ends and a far edge (`far` mm away).
pub fn section_cells(
    poly: &[[f64; 2]],
    parent: &ViewFrame,
    frame: &ViewFrame,
    far: f64,
) -> Vec<Cell> {
    let unit = |v: [f64; 2]| {
        let l = v[0].hypot(v[1]);
        if l < 1e-12 {
            [0.0, 0.0]
        } else {
            [v[0] / l, v[1] / l]
        }
    };
    let mut p: Vec<[f64; 2]> = Vec::new();
    for q in poly {
        if p.last()
            .is_none_or(|l: &[f64; 2]| (l[0] - q[0]).hypot(l[1] - q[1]) > 1e-9)
        {
            p.push(*q);
        }
    }
    if p.len() < 2 {
        return Vec::new();
    }
    let n = p.len();
    let d0 = unit([p[1][0] - p[0][0], p[1][1] - p[0][1]]);
    let dn = unit([p[n - 1][0] - p[n - 2][0], p[n - 1][1] - p[n - 2][1]]);
    p.insert(0, [p[0][0] - d0[0] * far, p[0][1] - d0[1] * far]);
    let last = p[p.len() - 1];
    p.push([last[0] + dn[0] * far, last[1] + dn[1] * far]);
    // The viewer's side, the same along the whole line.
    let tw = [
        frame.toward.dot(&parent.right),
        frame.toward.dot(&parent.up),
    ];
    let left = |d: [f64; 2]| [-d[1], d[0]];
    let s = if left(d0)[0] * tw[0] + left(d0)[1] * tw[1] >= 0.0 {
        1.0
    } else {
        -1.0
    };
    let dirs: Vec<[f64; 2]> = p
        .windows(2)
        .map(|w| unit([w[1][0] - w[0][0], w[1][1] - w[0][1]]))
        .collect();
    let normals: Vec<[f64; 2]> = dirs
        .iter()
        .map(|d| {
            let l = left(*d);
            [l[0] * s, l[1] * s]
        })
        .collect();
    let rays: Vec<[f64; 2]> = (0..p.len())
        .map(|i| {
            if i == 0 {
                normals[0]
            } else if i == p.len() - 1 {
                normals[normals.len() - 1]
            } else {
                let b = [
                    normals[i - 1][0] + normals[i][0],
                    normals[i - 1][1] + normals[i][1],
                ];
                if b[0].hypot(b[1]) < 1e-9 {
                    normals[i]
                } else {
                    unit(b)
                }
            }
        })
        .collect();
    let mut cells = Vec::new();
    for k in 0..dirs.len() {
        let (a, b) = (p[k], p[k + 1]);
        let (ra, rb) = (rays[k], rays[k + 1]);
        // Rays meeting in front of the far edge close the cell there.
        let den = ra[0] * rb[1] - ra[1] * rb[0];
        let w = [b[0] - a[0], b[1] - a[1]];
        let mut verts = vec![a, b];
        let meet = (den.abs() > 1e-12).then(|| {
            let t = (w[0] * rb[1] - w[1] * rb[0]) / den;
            let u = (w[0] * ra[1] - w[1] * ra[0]) / den;
            (t, u)
        });
        match meet {
            Some((t, u)) if t > 0.0 && u > 0.0 && t < far => {
                verts.push([a[0] + ra[0] * t, a[1] + ra[1] * t]);
            }
            _ => {
                verts.push([b[0] + rb[0] * far, b[1] + rb[1] * far]);
                verts.push([a[0] + ra[0] * far, a[1] + ra[1] * far]);
            }
        }
        let area: f64 = (0..verts.len())
            .map(|i| {
                let (x, y) = (verts[i], verts[(i + 1) % verts.len()]);
                x[0] * y[1] - y[0] * x[1]
            })
            .sum();
        if area.abs() < 1e-12 {
            continue;
        }
        let ccw = area > 0.0;
        let mut edges: Vec<([f64; 2], [f64; 2], bool)> = (0..verts.len())
            .map(|i| {
                let (x, y) = (verts[i], verts[(i + 1) % verts.len()]);
                let e = unit([y[0] - x[0], y[1] - x[1]]);
                let out = if ccw { [e[1], -e[0]] } else { [-e[1], e[0]] };
                (x, out, i == 0)
            })
            .collect();
        // The section line's edge last: its chord is then inside the cell.
        edges.rotate_left(1);
        cells.push(Cell { edges });
    }
    cells
}

/// A rigid motion of the parent's view plane (depth kept): `x ↦ m·x + t`.
#[derive(Clone, Copy, Debug)]
struct Turn {
    m: [[f64; 2]; 2],
    t: [f64; 2],
}

impl Turn {
    const ID: Turn = Turn {
        m: [[1.0, 0.0], [0.0, 1.0]],
        t: [0.0, 0.0],
    };

    /// A rotation by `a` about `c`.
    fn about(c: [f64; 2], a: f64) -> Turn {
        let (s, k) = a.sin_cos();
        let m = [[k, -s], [s, k]];
        Turn {
            m,
            t: [
                c[0] - m[0][0] * c[0] - m[0][1] * c[1],
                c[1] - m[1][0] * c[0] - m[1][1] * c[1],
            ],
        }
    }

    fn vec(&self, v: [f64; 2]) -> [f64; 2] {
        [
            self.m[0][0] * v[0] + self.m[0][1] * v[1],
            self.m[1][0] * v[0] + self.m[1][1] * v[1],
        ]
    }

    fn pt(&self, v: [f64; 2]) -> [f64; 2] {
        let w = self.vec(v);
        [w[0] + self.t[0], w[1] + self.t[1]]
    }

    /// `self` after `o`.
    fn then_from(&self, o: &Turn) -> Turn {
        let c = |i: usize, j: usize| self.m[i][0] * o.m[0][j] + self.m[i][1] * o.m[1][j];
        Turn {
            m: [[c(0, 0), c(0, 1)], [c(1, 0), c(1, 1)]],
            t: self.pt(o.t),
        }
    }

    fn point3(&self, f: &ViewFrame, p: &Point3) -> Point3 {
        let q = f.project(p);
        let w = self.pt(q);
        let rest = p.coords - f.right * q[0] - f.up * q[1];
        Point3::from(rest + f.right * w[0] + f.up * w[1])
    }

    fn vector3(&self, f: &ViewFrame, v: &Vector3) -> Vector3 {
        let q = [v.dot(&f.right), v.dot(&f.up)];
        let w = self.vec(q);
        v - f.right * q[0] - f.up * q[1] + f.right * w[0] + f.up * w[1]
    }
}

/// The pieces of an Aligned section line: the boundaries between the
/// segments (a point and the side the later segment lies on) and the motion
/// of each segment's material into the first segment's line.
struct Unfold {
    bounds: Vec<([f64; 2], [f64; 2])>,
    turns: Vec<Turn>,
}

impl Unfold {
    fn new(poly: &[[f64; 2]]) -> Unfold {
        let unit = |a: [f64; 2], b: [f64; 2]| {
            let d = [b[0] - a[0], b[1] - a[1]];
            let l = d[0].hypot(d[1]).max(1e-12);
            [d[0] / l, d[1] / l]
        };
        let mut p: Vec<[f64; 2]> = Vec::new();
        for q in poly {
            if p.last()
                .is_none_or(|l: &[f64; 2]| (l[0] - q[0]).hypot(l[1] - q[1]) > 1e-9)
            {
                p.push(*q);
            }
        }
        let dirs: Vec<[f64; 2]> = p.windows(2).map(|w| unit(w[0], w[1])).collect();
        let (mut bounds, mut turns) = (Vec::new(), vec![Turn::ID]);
        for i in 1..dirs.len() {
            let (d0, d1) = (dirs[i - 1], dirs[i]);
            // Square to the bisector of the two directions: the boundary is
            // the bisector of the corner.
            let s = [d0[0] + d1[0], d0[1] + d1[1]];
            bounds.push((p[i], if s[0].hypot(s[1]) < 1e-9 { d1 } else { s }));
            let a = d0[1].atan2(d0[0]) - d1[1].atan2(d1[0]);
            let prev = turns[i - 1];
            turns.push(prev.then_from(&Turn::about(p[i], a)));
        }
        Unfold { bounds, turns }
    }

    fn side(&self, i: usize, v: [f64; 2]) -> f64 {
        let (c, s) = self.bounds[i];
        (v[0] - c[0]) * s[0] + (v[1] - c[1]) * s[1]
    }

    /// The segment whose material holds `v`.
    fn sector(&self, v: [f64; 2]) -> usize {
        let mut k = 0;
        for i in 0..self.bounds.len() {
            if self.side(i, v) > 0.0 {
                k = i + 1;
            } else {
                break;
            }
        }
        k
    }

    /// `pts` cut at the boundaries, each part with its sector.
    fn split_curve(&self, f: &ViewFrame, pts: &[Point3]) -> Vec<(usize, Vec<Point3>)> {
        let mut out: Vec<(usize, Vec<Point3>)> = Vec::new();
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (va, vb) = (f.project(&a), f.project(&b));
            let mut ts = vec![0.0, 1.0];
            for i in 0..self.bounds.len() {
                let (fa, fb) = (self.side(i, va), self.side(i, vb));
                if (fa > 0.0) != (fb > 0.0) {
                    ts.push(fa / (fa - fb));
                }
            }
            ts.sort_by(f64::total_cmp);
            ts.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
            for t in ts.windows(2) {
                let mid = a + (b - a) * ((t[0] + t[1]) * 0.5);
                let k = self.sector(f.project(&mid));
                let (p0, p1) = (a + (b - a) * t[0], a + (b - a) * t[1]);
                match out.last_mut() {
                    Some((s, cur))
                        if *s == k && cur.last().is_some_and(|l| (*l - p0).norm() < 1e-12) =>
                    {
                        cur.push(p1)
                    }
                    _ => out.push((k, vec![p0, p1])),
                }
            }
        }
        out
    }
}

/// An Aligned section's model turned straight: everything behind a later
/// segment of `poly` (the parent's view plane, split at the bisectors of
/// its bends) turned about the bends into the first segment's line. Returns
/// the model and a point of the first segment (the one cutting plane).
fn unfold_model(model: &ViewModel, parent: &ViewFrame, poly: &[[f64; 2]]) -> (ViewModel, Point3) {
    let u = Unfold::new(poly);
    let f = parent;
    let smooth: Vec<SmoothEdge> = match &model.smooth {
        Some(v) => v.clone(),
        None => model.meshes.iter().flat_map(smooth_edges).collect(),
    };
    let mut meshes = Vec::with_capacity(model.meshes.len());
    for m in &model.meshes {
        let (pos, nrm) = (&m.faces.positions, &m.faces.normals);
        let mut out = TriangleMeshOut::default();
        for tri in &m.faces.triangles {
            let ids = tri.map(|i| i as usize);
            let mut pieces = vec![ids.map(|i| pos[i]).to_vec()];
            for i in 0..u.bounds.len() {
                let g = |q: &Point3| -u.side(i, f.project(q));
                let h = |q: &Point3| u.side(i, f.project(q));
                let mut next = Vec::new();
                for p in pieces {
                    for half in [clip_polygon(&p, &h).0, clip_polygon(&p, &g).0] {
                        if half.len() >= 3 {
                            next.push(half);
                        }
                    }
                }
                pieces = next;
            }
            // One normal per triangle (the pieces of a split one share it).
            let n = ids
                .iter()
                .map(|i| nrm.get(*i).copied().unwrap_or_default())
                .fold(Vector3::zeros(), |s, v| s + v);
            let n = if n.norm() > 1e-12 { n.normalize() } else { n };
            for p in pieces {
                let c = p.iter().fold(Vector3::zeros(), |s, q| s + q.coords) / p.len() as f64;
                let turn = u.turns[u.sector(f.project(&Point3::from(c)))];
                out.push(
                    &p.iter().map(|q| turn.point3(f, q)).collect::<Vec<_>>(),
                    turn.vector3(f, &n),
                );
            }
        }
        meshes.push(SolidMesh {
            faces: out.into_mesh(),
            edges: Vec::new(),
            face_triangles: Vec::new(),
        });
    }
    let mut edges = Vec::with_capacity(model.edges.len());
    for e in &model.edges {
        let parts = u.split_curve(f, &e.pts);
        let whole = parts.len() == 1;
        for (k, pts) in parts {
            let turn = u.turns[k];
            let mut ne = e.clone();
            ne.pts = pts.iter().map(|q| turn.point3(f, q)).collect();
            ne.circle = if whole {
                e.circle
                    .map(|(c, n, r)| (turn.point3(f, &c), turn.vector3(f, &n), r))
            } else {
                None
            };
            edges.push(ne);
        }
    }
    let mut sm = Vec::with_capacity(smooth.len());
    for s in &smooth {
        for (k, pts) in u.split_curve(f, &[s.a, s.b]) {
            let turn = u.turns[k];
            if let (Some(a), Some(b)) = (pts.first(), pts.last()) {
                sm.push(SmoothEdge {
                    a: turn.point3(f, a),
                    b: turn.point3(f, b),
                    n1: turn.vector3(f, &s.n1),
                    n2: turn.vector3(f, &s.n2),
                });
            }
        }
    }
    let start = poly.first().copied().unwrap_or_default();
    (
        ViewModel {
            meshes,
            edges,
            smooth: Some(sm),
        },
        Point3::from(f.right * start[0] + f.up * start[1]),
    )
}

/// Triangles collected from convex polygons (a fan each).
#[derive(Default)]
struct TriangleMeshOut {
    positions: Vec<Point3>,
    normals: Vec<Vector3>,
    triangles: Vec<[u32; 3]>,
}

impl TriangleMeshOut {
    fn push(&mut self, poly: &[Point3], n: Vector3) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(poly);
        self.normals.extend(std::iter::repeat_n(n, poly.len()));
        for k in 1..poly.len() as u32 - 1 {
            self.triangles.push([base, base + k, base + k + 1]);
        }
    }

    fn into_mesh(self) -> crate::kernel::TriangleMesh {
        crate::kernel::TriangleMesh {
            positions: self.positions,
            normals: self.normals,
            triangles: self.triangles,
        }
    }
}

/// The parts of a convex polygon outside `cell`; the chords where it meets
/// the cell's section-line edge go to `chords`.
fn subtract_cell(
    poly: Vec<Point3>,
    cell: &Cell,
    parent: &ViewFrame,
    chords: &mut Vec<(Point3, Point3)>,
) -> Vec<Vec<Point3>> {
    let mut out = Vec::new();
    let mut rest = poly;
    for e in &cell.edges {
        let f = |q: &Point3| Cell::f(e, parent.project(q));
        let g = |q: &Point3| -f(q);
        if rest.iter().all(|q| f(q) >= 0.0) {
            out.push(rest);
            return out;
        }
        let (outside, _) = clip_polygon(&rest, &g);
        if outside.len() >= 3 {
            out.push(outside);
        }
        let (inner, cross) = clip_polygon(&rest, &f);
        if e.2
            && let Some(c) = cross
        {
            chords.push(c);
        }
        rest = inner;
        if rest.len() < 3 {
            return out;
        }
    }
    out
}

/// The parts of a curve outside every cell.
fn clip_curve_cells(pts: &[Point3], cells: &[Cell], parent: &ViewFrame) -> Vec<Vec<Point3>> {
    let removed = |q: &Point3| {
        let v = parent.project(q);
        cells.iter().any(|c| c.contains(v))
    };
    let mut out: Vec<Vec<Point3>> = Vec::new();
    let mut cur: Vec<Point3> = Vec::new();
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (va, vb) = (parent.project(&a), parent.project(&b));
        let mut ts = vec![0.0, 1.0];
        for c in cells {
            for e in &c.edges {
                let (fa, fb) = (Cell::f(e, va), Cell::f(e, vb));
                if (fa > 0.0) != (fb > 0.0) {
                    ts.push(fa / (fa - fb));
                }
            }
        }
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|x, y| (*x - *y).abs() < 1e-12);
        for t in ts.windows(2) {
            let mid = a + (b - a) * ((t[0] + t[1]) * 0.5);
            if removed(&mid) {
                if cur.len() >= 2 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
            } else {
                let p0 = a + (b - a) * t[0];
                if cur.last().is_none_or(|l| (*l - p0).norm() > 1e-12) {
                    cur.push(p0);
                }
                cur.push(a + (b - a) * t[1]);
            }
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

/// Even-odd point in polygon.
/// Whether `q` lies inside closed loop `poly` (crossing count; for tests
/// and callers outside the module).
pub fn inside_loop(poly: &[[f64; 2]], q: [f64; 2]) -> bool {
    inside(poly, q)
}

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

/// A detail view's fence in its parent's view plane: a circle or a
/// rectangle about `center`, cut smooth or jagged (Inventor's Cutout
/// Shape).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fence {
    /// Centre (view plane).
    pub center: [f64; 2],
    /// Circle radius (a rectangle's size is `half`).
    pub radius: f64,
    /// A rectangle: half its width and height (`None`: a circle).
    pub half: Option<[f64; 2]>,
    /// Jagged cut: the boundary wanders in and out by [`JAG`] of the
    /// fence's size, so the cut ends look torn.
    pub jagged: bool,
}

/// How far a jagged cut wanders (share of the fence's size).
pub const JAG: f64 = 0.06;

impl Fence {
    /// A smooth circle.
    pub fn circle(center: [f64; 2], radius: f64) -> Self {
        Self {
            center,
            radius,
            half: None,
            jagged: false,
        }
    }

    /// The jag at angle `t` about the centre: in [-1, 1], the same every
    /// time (a sum of a few waves).
    fn jag(&self, t: f64) -> f64 {
        if !self.jagged {
            return 0.0;
        }
        0.5 * (7.0 * t + 0.3).sin() + 0.3 * (13.0 * t + 1.1).sin() + 0.2 * (23.0 * t + 2.0).sin()
    }

    /// Negative inside, positive outside (clipping uses its sign).
    pub fn side(&self, p: [f64; 2]) -> f64 {
        let (dx, dy) = (p[0] - self.center[0], p[1] - self.center[1]);
        match self.half {
            None => {
                let t = dy.atan2(dx);
                dx.hypot(dy) - self.radius * (1.0 + JAG * self.jag(t))
            }
            Some([w, h]) => {
                let (w, h) = (w.max(1e-9), h.max(1e-9));
                let t = (dy / h).atan2(dx / w);
                ((dx / w).abs().max((dy / h).abs()) - (1.0 + JAG * self.jag(t))) * w.min(h)
            }
        }
    }

    /// Half the box round its smooth outline.
    pub fn half_size(&self) -> [f64; 2] {
        self.half.unwrap_or([self.radius, self.radius])
    }

    /// Its smooth outline, closed (view plane).
    pub fn outline(&self) -> Vec<[f64; 2]> {
        let c = self.center;
        match self.half {
            None => (0..=96)
                .map(|k| {
                    let t = std::f64::consts::TAU * k as f64 / 96.0;
                    [c[0] + self.radius * t.cos(), c[1] + self.radius * t.sin()]
                })
                .collect(),
            Some([w, h]) => vec![
                [c[0] - w, c[1] - h],
                [c[0] + w, c[1] - h],
                [c[0] + w, c[1] + h],
                [c[0] - w, c[1] + h],
                [c[0] - w, c[1] - h],
            ],
        }
    }

    /// How far the ray from the centre along unit `u` runs to the smooth
    /// outline.
    pub fn reach(&self, u: [f64; 2]) -> f64 {
        match self.half {
            None => self.radius,
            Some([w, h]) => {
                let tx = if u[0].abs() > 1e-12 {
                    w / u[0].abs()
                } else {
                    f64::MAX
                };
                let ty = if u[1].abs() > 1e-12 {
                    h / u[1].abs()
                } else {
                    f64::MAX
                };
                tx.min(ty)
            }
        }
    }
}

/// [`clip_fence`] with a smooth circle.
pub fn clip_circle(lines: &ViewLines, center: [f64; 2], radius: f64) -> ViewLines {
    clip_fence(lines, &Fence::circle(center, radius))
}

/// A detail view's lines: the parent's inside `fence` (curves, cut faces
/// and bend lines cut where they leave it).
pub fn clip_fence(lines: &ViewLines, fence: &Fence) -> ViewLines {
    let half = fence.half_size();
    let size = half[0].min(half[1]);
    let d = |p: [f64; 2]| fence.side(p);
    let clip = |polys: &[Vec<[f64; 2]>], srcs: &[Option<u32>]| {
        let mut out = Vec::new();
        let mut out_src = Vec::new();
        // Pieces finer than the fence (and its jags), so a segment passing
        // through it with both ends outside is cut too.
        let step = (size / if fence.jagged { 64.0 } else { 16.0 }).max(1e-6);
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
            let mut cur: Vec<[f64; 2]> = Vec::new();
            for (i, p) in pl.iter().enumerate() {
                let dpv = d(*p);
                if i > 0 {
                    let a = pl[i - 1];
                    let da = d(a);
                    if (da <= 0.0) != (dpv <= 0.0) {
                        // Bisection for the crossing (the fence is no line).
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
    // A straight line's part inside the fence (its longest piece).
    let inside = |seg: [[f64; 2]; 2]| -> Option<[[f64; 2]; 2]> {
        let len = |s: &[[f64; 2]; 2]| (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1]);
        let (pieces, _) = clip(&[seg.to_vec()], &[]);
        pieces
            .into_iter()
            .filter_map(|p| Some([*p.first()?, *p.last()?]))
            .max_by(|a, b| len(a).total_cmp(&len(b)))
    };
    // Bend lines: centre line and extents cut by the fence too (a bend
    // whose centre line misses it is left out).
    let bends = lines
        .bends
        .iter()
        .filter_map(|b| {
            let centre = inside(b.centre)?;
            let zone = b.zone.map(|z| inside(z).unwrap_or([centre[0], centre[0]]));
            Some(ViewBend {
                centre,
                zone,
                ..b.clone()
            })
        })
        .collect();
    let reach = 1.0 + if fence.jagged { JAG } else { 0.0 };
    let c = fence.center;
    ViewLines {
        breaks: Default::default(),
        visible,
        visible_src,
        hidden,
        hidden_src,
        edges: lines.edges.clone(),
        section: section
            .into_iter()
            .filter(|l| l.first() == l.last())
            .collect(),
        min: [c[0] - half[0] * reach, c[1] - half[1] * reach],
        max: [c[0] + half[0] * reach, c[1] + half[1] * reach],
        anchor: None,
        bends,
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
fn silhouettes(model: &ViewModel, toward: &Vector3) -> Vec<Vec<Point3>> {
    let computed;
    let smooth = match &model.smooth {
        Some(v) => v,
        None => {
            computed = model
                .meshes
                .iter()
                .flat_map(smooth_edges)
                .collect::<Vec<_>>();
            &computed
        }
    };
    smooth
        .iter()
        .filter(|e| e.n1.dot(toward) * e.n2.dot(toward) < 0.0)
        .map(|e| vec![e.a, e.b])
        .collect()
}

/// An edge between two triangles of one curved face (they turn by less
/// than ~45°): a silhouette where one faces the viewer and the other not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmoothEdge {
    /// Start point.
    pub a: Point3,
    /// End point.
    pub b: Point3,
    /// The first triangle's unit normal.
    pub n1: Vector3,
    /// The second triangle's unit normal.
    pub n2: Vector3,
}

/// The smooth edges of a mesh (welded across its faces: a cylinder comes in
/// several faces), in a fixed order. Computed once per component.
pub fn smooth_edges(m: &SolidMesh) -> Vec<SmoothEdge> {
    // A mesh of planes only has none (its edges are model edges).
    let n = &m.faces.normals;
    if n.first()
        .is_none_or(|first| n.iter().all(|v| v == first) || all_flat(m))
    {
        return Vec::new();
    }
    let (mut lo, mut hi) = (Vector3::repeat(f64::MAX), Vector3::repeat(f64::MIN));
    for p in &m.faces.positions {
        lo = lo.inf(&p.coords);
        hi = hi.sup(&p.coords);
    }
    let q = ((hi - lo).norm() * 1e-7).max(1e-12);
    let key = |p: &Point3| {
        (
            (p.x / q).round() as i64,
            (p.y / q).round() as i64,
            (p.z / q).round() as i64,
        )
    };
    let mut ids: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut pts: Vec<Point3> = Vec::new();
    let mut normals: Vec<Vector3> = Vec::new();
    let mut edges: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
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
    let mut keys: Vec<(u32, u32)> = edges
        .iter()
        .filter(|(_, ts)| {
            ts.len() == 2 && normals[ts[0] as usize].dot(&normals[ts[1] as usize]) >= 0.7
        })
        .map(|(k, _)| *k)
        .collect();
    keys.sort_unstable();
    keys.into_iter()
        .map(|(a, b)| {
            let ts = &edges[&(a, b)];
            SmoothEdge {
                a: pts[a as usize],
                b: pts[b as usize],
                n1: normals[ts[0] as usize],
                n2: normals[ts[1] as usize],
            }
        })
        .collect()
}

/// Every triangle's vertex normals equal its face normal (no curved face):
/// nothing can turn away from the viewer inside a face.
fn all_flat(m: &SolidMesh) -> bool {
    let (p, n) = (&m.faces.positions, &m.faces.normals);
    m.faces.triangles.iter().all(|t| {
        let [a, b, c] = t.map(|i| i as usize);
        let f = (p[b] - p[a]).cross(&(p[c] - p[a]));
        let len = f.norm();
        len < 1e-18 || {
            let f = f / len;
            [a, b, c]
                .iter()
                .all(|&i| n.get(i).is_some_and(|v| v.dot(&f) > 0.9999))
        }
    })
}

/// `f` over `items` on every core, in order (small inputs on this thread).
fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    if cores < 2 || items.len() < 2048 {
        return items.iter().map(&f).collect();
    }
    let chunk = items.len().div_ceil(cores);
    let f = &f;
    let done: Option<Vec<R>> = std::thread::scope(|sc| {
        let jobs: Vec<_> = items
            .chunks(chunk)
            .map(|part| sc.spawn(move || part.iter().map(f).collect::<Vec<R>>()))
            .collect();
        let mut out = Vec::with_capacity(items.len());
        for j in jobs {
            out.extend(j.join().ok()?);
        }
        Some(out)
    });
    // A worker that failed: the whole job again on this thread (the result
    // must keep one entry per item).
    done.unwrap_or_else(|| items.iter().map(f).collect())
}
