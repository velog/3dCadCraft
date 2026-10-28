//! The surface commands' logic behind their panels (`docs/spec/5.4-surfaces.md`): Stitch,
//! Boundary Patch, Trim, Extend, Ruled Surface, Replace Face and Sculpt use the planned-panel
//! interface of `part_planned` (selectors, options); OK turns its picks into a
//! [`BodyOp`](crate::part::BodyOp) (`part::surface_op`) and adds or replaces the feature.

use eframe::egui;

use super::CadCraftApp;
use super::part_planned::{PartDialog, PartPlanned, Planned};
use super::select::SelItem;
use crate::kernel::{Point3, Real};
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
    fn body_of_face(&self, name: &str, at: Option<[f64; 3]>) -> Option<String> {
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
                if d.index(t, "Output") == 1 {
                    return Err("Output Solid is not there yet: the patch is a surface".into());
                }
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
                    })
                } else {
                    let mut edges = self.surf_edges(picks);
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
                if d.index(t, "Extents") == 1 {
                    return Err("Extents To is not there yet: use Distance".into());
                }
                let mut edges = self.surf_edges(d.items_of("edges"));
                if edges.is_empty()
                    && let Some(BodyOp::Extend(o)) = old
                {
                    edges = o.edges.clone();
                }
                BodyOp::Extend(ExtendOp {
                    edges,
                    distance: d.num(t, "Distance").unwrap_or(5.0),
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
                true
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("{title}: {e}")));
                false
            }
        }
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
            BodyOp::Extend(o) => d.set_num(0, "Distance", o.distance),
            BodyOp::Ruled(o) => {
                d.set_num(0, "Distance", o.distance);
                d.set_check(0, "Flip", o.flip);
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
