//! The surface commands' logic behind their panels (`docs/spec/5.4-surfaces.md`): Stitch,
//! Boundary Patch, Trim, Extend, Ruled Surface, Replace Face and Sculpt — and Copy Object
//! (`docs/spec/2.13-copy-object.md`) — use the planned-panel interface of `part_planned`
//! (selectors, options); OK turns its picks into a [`BodyOp`](crate::part::BodyOp)
//! (`part::surface_op`, `part::copy_object`) and adds or replaces the feature.

use eframe::egui;

use super::CadCraftApp;
use super::part_planned::{PartDialog, PartPlanned, Planned};
use super::select::SelItem;
use crate::kernel::{Point3, Real};
use crate::part::copy_object::{CopyObjectOp, CopyOutput, CopySelect};
use crate::part::surface_op::{
    ExtendOp, PatchOp, ReplaceFaceOp, ReplaceWith, RuledKind, RuledOp, SculptOp, SculptOperation,
    StitchOp, SurfEdge, TrimOp, TrimTool,
};
use crate::part::{BodyOp, BodyOpFeature, FaceRef, Feature, LineRef, PlaneRef};

type Picks<'a> = &'a [(SelItem, Option<[f64; 3]>)];

impl CadCraftApp {
    /// The model point of a click on `item`: on a face where the eye ray meets it, on an edge
    /// the point of the edge nearest the ray.
    pub(super) fn click_point(
        &self,
        item: &SelItem,
        p: egui::Pos2,
        rect: egui::Rect,
    ) -> Option<Point3> {
        let (o, d) = self.camera.ray(p, rect)?;
        let d = d.try_normalize(1e-12)?;
        match item {
            SelItem::Edge(e) => {
                let pts = self
                    .part()
                    .body
                    .sample_edge(*e, &crate::kernel::TessellationOptions::default())
                    .ok()?;
                // The point of each sample segment nearest the ray.
                pts.windows(2)
                    .map(|w| {
                        let (a, b) = (w[0], w[1]);
                        let u = b - a;
                        let (uu, ud) = (u.dot(&u).max(1e-24), u.dot(&d));
                        let w0 = a - o;
                        let den = (uu - ud * ud).max(1e-24);
                        let t = ((ud * w0.dot(&d) - w0.dot(&u)) / den).clamp(0.0, 1.0);
                        let q = a + u * t;
                        let along = (q - o).dot(&d);
                        ((q - (o + d * along)).norm(), q)
                    })
                    .min_by(|x, y| x.0.total_cmp(&y.0))
                    .map(|(_, q)| q)
            }
            _ => {
                let (_, t) = self.picker().pick(&o, &d)?;
                Some(o + d * t)
            }
        }
    }

    /// A point of a preselected item (no click): a face's mesh centre, an edge's middle.
    pub(super) fn item_point(&self, item: &SelItem) -> Option<Point3> {
        let body = &self.part().body;
        match item {
            SelItem::Face(name) => {
                let f = body
                    .faces()
                    .find(|(_, f)| f.name.as_deref() == Some(name.as_str()))?
                    .1;
                let m = f.mesh.as_ref()?;
                let c = m
                    .positions
                    .iter()
                    .fold(crate::kernel::Vector3::zeros(), |a, q| a + q.coords)
                    / m.positions.len().max(1) as Real;
                // The mesh vertex nearest the centre: a point on the face.
                m.positions
                    .iter()
                    .min_by(|a, b| (a.coords - c).norm().total_cmp(&(b.coords - c).norm()))
                    .copied()
            }
            SelItem::Edge(e) => {
                let pts = body
                    .sample_edge(*e, &crate::kernel::TessellationOptions::default())
                    .ok()?;
                let len: Real = pts.windows(2).map(|w| (w[1] - w[0]).norm()).sum();
                let mut run = 0.0;
                for w in pts.windows(2) {
                    let l = (w[1] - w[0]).norm();
                    if run + l >= len / 2.0 && l > 0.0 {
                        return Some(w[0] + (w[1] - w[0]) * ((len / 2.0 - run) / l));
                    }
                    run += l;
                }
                pts.first().copied()
            }
            _ => None,
        }
    }

    /// The name of the body (solid or surface) holding face `name` nearest `at`.
    pub(super) fn body_of_face(&self, name: &str, at: Option<[f64; 3]>) -> Option<String> {
        let all = &self.part().exact_bodies.0;
        let at = at.map_or_else(Point3::origin, |h| Point3::new(h[0], h[1], h[2]));
        crate::kernel::exact::faces_in_solids(all, &[(name, at)])
            .pop()
            .flatten()
            .and_then(|(b, _)| all[b].name.clone())
    }

    /// Whether body `name` is a surface body.
    fn is_surface(&self, name: &str) -> bool {
        self.part()
            .exact_bodies
            .0
            .iter()
            .any(|s| s.is_open() && s.name.as_deref() == Some(name))
    }

    /// The surface bodies of picks (faces of them, or the bodies), each once.
    fn surfaces_of(&self, picks: Picks) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (item, at) in picks {
            let name = match item {
                SelItem::Body(b) => Some(b.clone()),
                SelItem::Face(f) => self.body_of_face(f, *at),
                _ => None,
            };
            if let Some(n) = name.filter(|n| self.is_surface(n))
                && !out.contains(&n)
            {
                out.push(n);
            }
        }
        out
    }

    /// The solid bodies of picks.
    fn solids_of(&self, picks: Picks) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (item, at) in picks {
            let name = match item {
                SelItem::Body(b) => Some(b.clone()),
                SelItem::Face(f) => self.body_of_face(f, *at),
                _ => None,
            };
            if let Some(n) = name.filter(|n| !self.is_surface(n))
                && !out.contains(&n)
            {
                out.push(n);
            }
        }
        out
    }

    /// Edges as [`SurfEdge`]s (a named face along each, the point picked).
    fn surf_edges(&self, picks: Picks) -> Vec<SurfEdge> {
        let body = &self.part().body;
        picks
            .iter()
            .filter_map(|(item, at)| {
                let SelItem::Edge(e) = item else {
                    return None;
                };
                let face = self
                    .picker()
                    .faces_of(*e)
                    .into_iter()
                    .find_map(|f| body.face(f).and_then(|x| x.name.clone()))?;
                let hint = at.or_else(|| self.item_point(item).map(|q| [q.x, q.y, q.z]))?;
                Some(SurfEdge { face, hint })
            })
            .collect()
    }

    /// [`Self::surf_edges`] where each picked free edge (one face) brings the free edges joined to
    /// it end to end — the whole loop (a hole's rim in arcs from one click, even where the rim
    /// crosses several faces of the surface: Patch's Automatic Edge Chain).
    fn surf_edges_chained(&self, picks: Picks) -> Vec<SurfEdge> {
        let body = &self.part().body;
        let opts = crate::kernel::TessellationOptions::default();
        let mut out = self.surf_edges(picks);
        // Every free edge of the model, by the vertices at its ends.
        let free: Vec<crate::kernel::EdgeId> = body
            .edges()
            .filter(|(e, _)| body.edge_faces(*e).len() == 1)
            .map(|(e, _)| e)
            .collect();
        let ends = |e: crate::kernel::EdgeId| body.edge(e).map(|x| (x.start, x.end));
        let mut taken: std::collections::HashSet<crate::kernel::EdgeId> =
            std::collections::HashSet::new();
        for (item, _) in picks {
            let SelItem::Edge(e) = item else {
                continue;
            };
            if !free.contains(e) || !taken.insert(*e) {
                continue;
            }
            // Walk from both ends along free edges sharing a vertex.
            let mut stack = vec![*e];
            while let Some(x) = stack.pop() {
                let Some((a, b)) = ends(x) else {
                    continue;
                };
                for &y in &free {
                    if taken.contains(&y) {
                        continue;
                    }
                    let Some((c, d)) = ends(y) else {
                        continue;
                    };
                    if c == a || c == b || d == a || d == b {
                        taken.insert(y);
                        stack.push(y);
                        let face = body
                            .edge_faces(y)
                            .first()
                            .and_then(|f| body.face(*f))
                            .and_then(|f| f.name.clone());
                        if let (Some(face), Ok(pts)) = (face, body.sample_edge(y, &opts))
                            && let Some(m) = crate::part::select::polyline_middle(&pts)
                        {
                            out.push(SurfEdge {
                                face,
                                hint: [m.x, m.y, m.z],
                            });
                        }
                    }
                }
            }
        }
        out
    }

    /// A plane from a pick: a planar face, a work plane.
    fn plane_of_pick(&self, item: &SelItem) -> Option<PlaneRef> {
        match item {
            SelItem::Face(f) => Some(PlaneRef::Face(f.clone())),
            SelItem::Feature(id) if self.part().work.planes.contains_key(id) => {
                Some(PlaneRef::Work(id.clone()))
            }
            _ => None,
        }
    }

    /// The surface command's definition from the panel, or why not.
    fn surface_op_from(&self, d: &PartDialog, old: Option<&BodyOp>) -> Result<BodyOp, String> {
        let Planned::Part(kind) = d.kind else {
            return Err("not a part command".into());
        };
        let t = 0;
        if kind == PartPlanned::Import {
            return self.import_op_from(d, old);
        }
        Ok(match kind {
            PartPlanned::Stitch => {
                let mut surfaces = self.surfaces_of(d.items_of("surfaces"));
                if surfaces.is_empty()
                    && let Some(BodyOp::Stitch(o)) = old
                {
                    surfaces = o.surfaces.clone();
                }
                BodyOp::Stitch(StitchOp {
                    surfaces,
                    tolerance: d.num(t, "Maximum Tolerance").unwrap_or(0.00025),
                    keep_surface: d.check(t, "Keep as Surface"),
                })
            }
            PartPlanned::Patch => {
                let solid = d.index(t, "Output") == 1;
                let picks = d.items_of("boundary");
                let sketch = picks.iter().find_map(|(item, at)| match item {
                    SelItem::Sketch(i) => Some((*i, *at)),
                    _ => None,
                });
                if let Some((i, at)) = sketch {
                    let ps = self
                        .part()
                        .sketches
                        .get(i)
                        .ok_or_else(|| "the sketch is gone".to_owned())?;
                    let regions = ps.sketch.regions();
                    let q = at.map(|h| ps.sketch.plane.project(&Point3::new(h[0], h[1], h[2])));
                    let region = q
                        .and_then(|q| regions.iter().find(|r| r.contains(q)))
                        .or(regions.first())
                        .ok_or_else(|| "the sketch has no closed region".to_owned())?;
                    BodyOp::Patch(PatchOp {
                        edges: Vec::new(),
                        sketch: Some(ps.id.clone()),
                        profile: Some(crate::part::ProfileRef::of(region, region.interior_point())),
                        solid,
                    })
                } else {
                    // Automatic Edge Chain: a picked free edge brings the rest of its loop.
                    let mut edges = if d.check(t, "Automatic Edge Chain") {
                        self.surf_edges_chained(picks)
                    } else {
                        self.surf_edges(picks)
                    };
                    if edges.is_empty()
                        && let Some(BodyOp::Patch(o)) = old
                    {
                        return Ok(BodyOp::Patch(o.clone()));
                    }
                    edges.dedup();
                    BodyOp::Patch(PatchOp {
                        edges,
                        sketch: None,
                        profile: None,
                        solid,
                    })
                }
            }
            PartPlanned::TrimSurface => {
                let prev = match old {
                    Some(BodyOp::Trim(o)) => Some(o),
                    _ => None,
                };
                let tool = match d.items_of("tool").first() {
                    Some((SelItem::Face(f), at)) => match self.body_of_face(f, *at) {
                        Some(b) if self.is_surface(&b) => TrimTool::Surface(b),
                        _ => TrimTool::Plane(PlaneRef::Face(f.clone())),
                    },
                    Some((SelItem::Body(b), _)) if self.is_surface(b) => {
                        TrimTool::Surface(b.clone())
                    }
                    Some((SelItem::Sketch(i), _)) => TrimTool::Sketch(
                        self.part()
                            .sketches
                            .get(*i)
                            .map(|s| s.id.clone())
                            .ok_or_else(|| "the sketch is gone".to_owned())?,
                    ),
                    Some((item, _)) => {
                        TrimTool::Plane(self.plane_of_pick(item).ok_or_else(|| {
                            "the cutting tool must be a surface or a plane".to_owned()
                        })?)
                    }
                    None => prev
                        .map(|o| o.tool.clone())
                        .ok_or_else(|| "select the cutting tool".to_owned())?,
                };
                let (surface, remove) = match d.items_of("remove").first() {
                    Some((item, at)) => {
                        let name = match item {
                            SelItem::Face(f) => self.body_of_face(f, *at),
                            SelItem::Body(b) => Some(b.clone()),
                            _ => None,
                        }
                        .filter(|n| self.is_surface(n))
                        .ok_or_else(|| "pick the piece of a surface to remove".to_owned())?;
                        let at = at.ok_or_else(|| "pick the piece in the view".to_owned())?;
                        (name, at)
                    }
                    None => prev
                        .map(|o| (o.surface.clone(), o.remove))
                        .ok_or_else(|| "select the piece to remove".to_owned())?,
                };
                BodyOp::Trim(TrimOp {
                    surface,
                    tool,
                    remove,
                    invert: d.check(t, "Invert Selection"),
                })
            }
            PartPlanned::ExtendSurface => {
                let prev = match old {
                    Some(BodyOp::Extend(o)) => Some(o),
                    _ => None,
                };
                let mut edges = self.surf_edges(d.items_of("edges"));
                if edges.is_empty()
                    && let Some(o) = prev
                {
                    edges = o.edges.clone();
                }
                let to = if d.index(t, "Extents") == 1 {
                    Some(match d.items_of("to").first() {
                        Some((item, _)) => self
                            .plane_of_pick(item)
                            .ok_or_else(|| "extend up to a plane or a planar face".to_owned())?,
                        None => prev
                            .and_then(|o| o.to.clone())
                            .ok_or_else(|| "select the face or plane to extend to".to_owned())?,
                    })
                } else {
                    None
                };
                BodyOp::Extend(ExtendOp {
                    edges,
                    distance: d.num(t, "Distance").unwrap_or(5.0),
                    to,
                })
            }
            PartPlanned::RuledSurface => {
                let kind = match d.index(t, "Type") {
                    1 => RuledKind::Tangent,
                    2 => RuledKind::Sweep,
                    _ => RuledKind::Normal,
                };
                let mut edges = self.surf_edges(d.items_of("edges"));
                let prev = match old {
                    Some(BodyOp::Ruled(o)) => Some(o),
                    _ => None,
                };
                if edges.is_empty()
                    && let Some(o) = prev
                {
                    edges = o.edges.clone();
                }
                let vector = match d.items_of("vector").first() {
                    Some((SelItem::Edge(e), _)) => self
                        .part()
                        .body
                        .edge(*e)
                        .and_then(|x| x.name.clone())
                        .map(LineRef::Edge),
                    Some((SelItem::Feature(id), _)) => Some(LineRef::Work(id.clone())),
                    _ => prev.and_then(|o| o.vector.clone()),
                };
                BodyOp::Ruled(RuledOp {
                    kind,
                    edges,
                    distance: d.num(t, "Distance").unwrap_or(5.0),
                    flip: d.check(t, "Flip"),
                    vector,
                    angle: d.num(t, "Angle").unwrap_or(0.0).to_radians(),
                    alternate: d.check(t, "Alternate All Faces"),
                })
            }
            PartPlanned::ReplaceFace => {
                let prev = match old {
                    Some(BodyOp::ReplaceFace(o)) => Some(o),
                    _ => None,
                };
                let mut faces: Vec<FaceRef> = d
                    .items_of("existing")
                    .iter()
                    .filter_map(|(item, at)| match item {
                        SelItem::Face(f) => Some(FaceRef {
                            face: f.clone(),
                            hint: (*at)?,
                        }),
                        _ => None,
                    })
                    .collect();
                if faces.is_empty()
                    && let Some(o) = prev
                {
                    faces = o.faces.clone();
                }
                let new = match d.items_of("new").first() {
                    Some((SelItem::Face(f), at)) => match self.body_of_face(f, *at) {
                        Some(b) if self.is_surface(&b) => ReplaceWith::Surface(b),
                        _ => ReplaceWith::Plane(PlaneRef::Face(f.clone())),
                    },
                    Some((SelItem::Body(b), _)) if self.is_surface(b) => {
                        ReplaceWith::Surface(b.clone())
                    }
                    Some((item, _)) => {
                        ReplaceWith::Plane(self.plane_of_pick(item).ok_or_else(|| {
                            "the new face must be a surface or a plane".to_owned()
                        })?)
                    }
                    None => prev
                        .map(|o| o.new.clone())
                        .ok_or_else(|| "select the new face".to_owned())?,
                };
                BodyOp::ReplaceFace(ReplaceFaceOp { faces, new })
            }
            PartPlanned::Sculpt => {
                let prev = match old {
                    Some(BodyOp::Sculpt(o)) => Some(o),
                    _ => None,
                };
                let mut surfaces = self.surfaces_of(d.items_of("surfaces"));
                if surfaces.is_empty()
                    && let Some(o) = prev
                {
                    surfaces = o.surfaces.clone();
                }
                BodyOp::Sculpt(SculptOp {
                    surfaces,
                    operation: match d.index(t, "Operation") {
                        1 => SculptOperation::Remove,
                        2 => SculptOperation::NewSolid,
                        _ => SculptOperation::Add,
                    },
                    solids: self.solids_of(d.items_of("solids")),
                    side: None,
                    flip: d.check(t, "Flip"),
                })
            }
            PartPlanned::CopyObject => {
                let prev = match old {
                    Some(BodyOp::CopyObject(o)) => Some(o),
                    _ => None,
                };
                let select = if d.index(t, "Select") == 1 {
                    CopySelect::Body
                } else {
                    CopySelect::Face
                };
                let picks = d.items_of("objects");
                let mut faces: Vec<FaceRef> = Vec::new();
                let mut bodies: Vec<String> = Vec::new();
                for (item, at) in picks {
                    match (select, item) {
                        (CopySelect::Face, SelItem::Face(f)) => {
                            let hint = at
                                .or_else(|| self.item_point(item).map(|q| [q.x, q.y, q.z]))
                                .ok_or_else(|| format!("no point on face {f}"))?;
                            faces.push(FaceRef {
                                face: f.clone(),
                                hint,
                            });
                        }
                        (CopySelect::Body, SelItem::Body(b)) => bodies.push(b.clone()),
                        (CopySelect::Body, SelItem::Face(f)) => {
                            if let Some(b) = self.body_of_face(f, *at) {
                                bodies.push(b);
                            }
                        }
                        _ => {}
                    }
                }
                bodies.dedup();
                // Selectors left empty keep the feature's own picks (Edit Feature).
                if picks.is_empty()
                    && let Some(o) = prev.filter(|o| o.select == select)
                {
                    faces = o.faces.clone();
                    bodies = o.bodies.clone();
                }
                let output = match d.index(t, "Create New") {
                    0 => {
                        return Err(
                            "Group needs the construction environment (not available yet)".into(),
                        );
                    }
                    2 => CopyOutput::Composite,
                    3 => CopyOutput::Solid,
                    4 => CopyOutput::MultipleSolids,
                    _ => CopyOutput::Surface,
                };
                BodyOp::CopyObject(CopyObjectOp {
                    select,
                    faces,
                    bodies,
                    output,
                    delete_original: d.check(t, "Delete Original"),
                })
            }
            _ => return Err("only the interface exists yet".into()),
        })
    }

    /// OK of a surface command: the feature added (or, in Edit Feature, replaced). Returns
    /// whether the panel closes.
    pub(super) fn surface_ok(&mut self, d: &PartDialog) -> bool {
        let title = d.kind.title();
        let old = d.feature.and_then(|k| match self.part().features.get(k) {
            Some(Feature::BodyOp(b)) => Some(b.clone()),
            _ => None,
        });
        let op = match self.surface_op_from(d, old.as_ref().map(|b| &b.op)) {
            Ok(op) => op,
            Err(e) => {
                self.file_status = Some(Err(format!("{title}: {e}")));
                return false;
            }
        };
        let fit = matches!(op, BodyOp::Import(_));
        let cmd = if old.is_some() { "Edit Feature" } else { title };
        self.checkpoint(cmd);
        let r = match (d.feature, old) {
            (Some(k), Some(mut b)) => {
                b.op = op;
                self.part_mut()
                    .replace_feature(k, Feature::BodyOp(b))
                    .map(|_| ())
            }
            _ => self
                .part_mut()
                .add_body_op(BodyOpFeature::new(op))
                .map(|_| ()),
        };
        match r {
            Ok(()) => {
                self.file_status = None;
                self.scene_dirty = true;
                if fit {
                    // A new file is anywhere and any size.
                    self.fit_view();
                }
                true
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("{title}: {e}")));
                false
            }
        }
    }

    /// A live button of a surface command's panel: the line it shows.
    pub(super) fn surface_button(&self, d: &PartDialog, button: &str) -> Option<String> {
        if button != "Find Remaining Free Edges" {
            return None;
        }
        // The picked surfaces' free edges (one face) and, for each, how far the nearest free
        // edge of another picked surface runs from its middle: within the tolerance Stitch
        // joins them (it cuts an edge where another one's end lies on it), beyond it a gap stays.
        let names = self.surfaces_of(d.items_of("surfaces"));
        let tol = d.num(0, "Maximum Tolerance").unwrap_or(0.00025).max(1e-6);
        let body = &self.part().body;
        let opts = crate::kernel::TessellationOptions::default();
        let mut free: Vec<(usize, Point3, Vec<Point3>)> = Vec::new();
        for (k, n) in names.iter().enumerate() {
            for f in crate::part::select::faces_of_body(body, n) {
                let Some(face) = body.face(f) else {
                    continue;
                };
                for lp in std::iter::once(&face.outer).chain(&face.inner) {
                    for ce in &lp.coedges {
                        if body.edge_faces(ce.edge).len() != 1 {
                            continue;
                        }
                        if let Ok(pts) = body.sample_edge(ce.edge, &opts)
                            && let Some(m) = crate::part::select::polyline_middle(&pts)
                        {
                            free.push((k, m, pts));
                        }
                    }
                }
            }
        }
        let gap = |k: usize, m: &Point3| -> Real {
            free.iter()
                .filter(|(j, ..)| *j != k)
                .flat_map(|(_, _, pts)| pts.windows(2))
                .map(|w| {
                    let ab = w[1] - w[0];
                    let t = ((m - w[0]).dot(&ab) / ab.norm_squared().max(1e-24)).clamp(0.0, 1.0);
                    (w[0] + ab * t - m).norm()
                })
                .fold(Real::MAX, Real::min)
        };
        let gaps: Vec<Real> = free.iter().map(|(k, m, _)| gap(*k, m)).collect();
        // Nothing within 1 mm: an opening (a hole, an end), not a gap between surfaces.
        let openings = gaps.iter().filter(|g| **g > 1.0).count();
        let open: Vec<Real> = gaps
            .iter()
            .copied()
            .filter(|g| *g > tol && *g <= 1.0)
            .collect();
        let widest = open.iter().copied().fold(0.0, Real::max);
        Some(match (names.len(), open.len(), openings) {
            (0, ..) => "Select the surfaces first".into(),
            (_, 0, 0) => "No free edges: Stitch closes a solid".into(),
            (_, 0, o) => format!("{o} free edge(s) with nothing near them (an opening: patch it)"),
            (_, n, o) => {
                let mut t = format!(
                    "{n} free edge(s) stay apart; the widest gap is {widest:.4} mm (a Maximum Tolerance above it joins them)"
                );
                if o > 0 {
                    t.push_str(&format!("; {o} with nothing near (an opening)"));
                }
                t
            }
        })
    }

    /// Edit Feature of a surface command: its panel again, with the feature's options; picks
    /// left empty keep the feature's references.
    pub(super) fn surface_op_edit(&mut self, k: usize) {
        let Some(Feature::BodyOp(b)) = self.part().features.get(k).cloned() else {
            return;
        };
        let kind = match &b.op {
            BodyOp::Stitch(_) => PartPlanned::Stitch,
            BodyOp::Patch(_) => PartPlanned::Patch,
            BodyOp::Trim(_) => PartPlanned::TrimSurface,
            BodyOp::Extend(_) => PartPlanned::ExtendSurface,
            BodyOp::Ruled(_) => PartPlanned::RuledSurface,
            BodyOp::ReplaceFace(_) => PartPlanned::ReplaceFace,
            BodyOp::Sculpt(_) => PartPlanned::Sculpt,
            BodyOp::CopyObject(_) => PartPlanned::CopyObject,
            BodyOp::Import(_) => PartPlanned::Import,
            _ => return,
        };
        self.start_part_planned(kind);
        let Some(d) = self.part_planned.as_mut() else {
            return;
        };
        d.feature = Some(k);
        match &b.op {
            BodyOp::Stitch(o) => {
                d.set_num(0, "Maximum Tolerance", o.tolerance);
                d.set_check(0, "Keep as Surface", o.keep_surface);
            }
            BodyOp::Extend(o) => {
                d.set_num(0, "Distance", o.distance);
                d.set_index(0, "Extents", usize::from(o.to.is_some()));
            }
            BodyOp::Ruled(o) => {
                d.set_num(0, "Distance", o.distance);
                d.set_num(0, "Angle", o.angle.to_degrees());
                d.set_check(0, "Flip", o.flip);
                d.set_check(0, "Alternate All Faces", o.alternate);
                d.set_index(
                    0,
                    "Type",
                    match o.kind {
                        RuledKind::Normal => 0,
                        RuledKind::Tangent => 1,
                        RuledKind::Sweep => 2,
                    },
                );
            }
            BodyOp::Trim(o) => d.set_check(0, "Invert Selection", o.invert),
            BodyOp::Import(o) => {
                d.set_text(0, "File", &o.file);
                d.set_check(0, "Solids", o.solids);
                d.set_check(0, "Surfaces", o.surfaces);
            }
            BodyOp::CopyObject(o) => {
                d.set_index(0, "Select", usize::from(o.select == CopySelect::Body));
                d.set_index(
                    0,
                    "Create New",
                    match o.output {
                        CopyOutput::Surface => 1,
                        CopyOutput::Composite => 2,
                        CopyOutput::Solid => 3,
                        CopyOutput::MultipleSolids => 4,
                    },
                );
                d.set_check(0, "Delete Original", o.delete_original);
            }
            BodyOp::Sculpt(o) => {
                d.set_check(0, "Flip", o.flip);
                d.set_index(
                    0,
                    "Operation",
                    match o.operation {
                        SculptOperation::Add => 0,
                        SculptOperation::Remove => 1,
                        SculptOperation::NewSolid => 2,
                    },
                );
            }
            _ => {}
        }
        if kind == PartPlanned::Import {
            self.file_status = Some(Ok(format!(
                "{}: Browse... replaces the embedded file",
                b.describe()
            )));
            return;
        }
        self.file_status = Some(Ok(format!(
            "{}: {} (selectors left empty keep the feature's own picks)",
            kind_title(kind),
            b.describe()
        )));
    }
}

fn kind_title(k: PartPlanned) -> &'static str {
    Planned::Part(k).title()
}
