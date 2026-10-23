//! Polygon ↔ B-Rep conversion for Boolean results and revolves (stage 1 of
//! `docs/spec/2.13-booleans.md`).
//!
//! - [`BRep::solid_polygons`] breaks a solid into triangles tagged with the
//!   face they come from ([`FaceTag`]: name + exact surface).
//! - [`BRep::add_polygons`] rebuilds a closed B-Rep from tagged polygons:
//!   vertices are welded, T-junctions repaired (so every edge is shared by
//!   exactly two faces), coplanar fragments merged into planar faces with
//!   holes, and fragments of curved faces kept as facets that remember
//!   their exact surface (`Face::facet_of`).
//! - [`BRep::boolean`] = both plus the BSP CSG of `csg.rs`.

use std::collections::HashMap;

use nalgebra as na;

use super::csg::{self, CsgPolygon};
use super::{
    BRep, Coedge, EdgeId, KernelError, KernelResult, Loop, Plane, Point3, Real, Sense, Shell,
    Solid, SolidId, Surface, SurfaceId, TessellationOptions, Vector3, VertexId,
};

/// Where a polygon comes from.
#[derive(Clone, Debug, PartialEq)]
pub struct FaceTag {
    /// Persistent name of the face.
    pub name: Option<String>,
    /// Its exact surface (`None` when not representable, e.g. a revolved
    /// spline: the facets are then shaded flat).
    pub surface: Option<Surface>,
    /// Curved (its fragments stay facets) vs planar (fragments merge).
    pub curved: bool,
}

/// A Boolean operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolOp {
    /// A ∪ B.
    Union,
    /// A − B.
    Subtract,
    /// A ∩ B.
    Intersect,
}

/// Welding tolerance (mm).
const WELD: Real = 1e-5;

impl BRep {
    /// The faces of a solid as triangles tagged with their face (tags are
    /// appended to `tags`; polygon tags index into it).
    pub fn solid_polygons(
        &self,
        id: SolidId,
        tags: &mut Vec<FaceTag>,
    ) -> KernelResult<Vec<CsgPolygon>> {
        let solid = self
            .solid(id)
            .ok_or(KernelError::DanglingReference("solid"))?;
        let opts = TessellationOptions::default();
        let mut out = Vec::new();
        // Facets of one curved face share a tag (so they stay one smooth
        // face after the rebuild).
        let mut facet_tag: HashMap<(SurfaceId, Option<String>), usize> = HashMap::new();
        for shell in &solid.shells {
            for &fid in &shell.faces {
                let face = self
                    .face(fid)
                    .ok_or(KernelError::DanglingReference("face"))?;
                if let Some(s) = face.facet_of
                    && let Some(&tag) = facet_tag.get(&(s, face.name.clone()))
                {
                    let mesh = self.face_mesh(fid, &opts)?;
                    for t in &mesh.triangles {
                        let pts = t.iter().map(|&i| mesh.positions[i as usize]).collect();
                        out.extend(CsgPolygon::new(pts, tag));
                    }
                    continue;
                }
                let exact = face
                    .facet_of
                    .or(Some(face.surface))
                    .and_then(|s| self.surface(s))
                    .cloned()
                    .ok_or(KernelError::DanglingReference("surface"))?;
                let curved = face.facet_of.is_some() || !matches!(exact, Surface::Plane(_));
                let tag = tags.len();
                if let Some(s) = face.facet_of {
                    facet_tag.insert((s, face.name.clone()), tag);
                }
                tags.push(FaceTag {
                    name: face.name.clone(),
                    surface: Some(exact),
                    curved,
                });
                let mesh = self.face_mesh(fid, &opts)?;
                for t in &mesh.triangles {
                    let pts = t.iter().map(|&i| mesh.positions[i as usize]).collect();
                    out.extend(CsgPolygon::new(pts, tag));
                }
            }
        }
        Ok(out)
    }

    /// Removes a solid and everything only it uses.
    pub fn remove_solid(&mut self, id: SolidId) {
        let Some(solid) = self.solids.remove(id) else {
            return;
        };
        for shell in solid.shells {
            for fid in shell.faces {
                let Some(face) = self.faces.remove(fid) else {
                    continue;
                };
                self.surfaces.remove(face.surface);
                if let Some(s) = face.facet_of {
                    self.surfaces.remove(s);
                }
                for lp in std::iter::once(&face.outer).chain(&face.inner) {
                    for ce in &lp.coedges {
                        if let Some(e) = self.edges.remove(ce.edge) {
                            self.curves.remove(e.curve);
                            self.vertices.remove(e.start);
                            self.vertices.remove(e.end);
                        }
                    }
                }
            }
        }
    }

    /// `target` ⟵ `target` op (`tool`'s solid `tool_solid`): the target is
    /// replaced by the result (new solid ids: one per connected piece).
    /// Fails (and changes nothing) if the result is empty or a cut misses.
    pub fn boolean(
        &mut self,
        target: SolidId,
        tool: &BRep,
        tool_solid: SolidId,
        op: BoolOp,
    ) -> KernelResult<Vec<SolidId>> {
        let mut tags = Vec::new();
        let a = self.solid_polygons(target, &mut tags)?;
        let b = tool.solid_polygons(tool_solid, &mut tags)?;
        let va = csg::volume(&a);
        let r = match op {
            BoolOp::Union => csg::union(a, b),
            BoolOp::Subtract => csg::subtract(a, b),
            BoolOp::Intersect => csg::intersect(a, b),
        };
        let vr = csg::volume(&r);
        if r.is_empty() || vr <= 1e-9 {
            return Err(KernelError::Degenerate("the result is empty"));
        }
        if op == BoolOp::Subtract && (va - vr).abs() <= 1e-9 * va.max(1.0) {
            return Err(KernelError::Degenerate(
                "the cut does not intersect the body",
            ));
        }
        let mut trial = self.clone();
        let name = trial.solid(target).and_then(|s| s.name.clone());
        trial.remove_solid(target);
        let mut ids = trial.add_polygons(&r, &tags, false)?;
        // The result keeps the target body's name.
        for &id in &ids {
            if let Some(s) = trial.solids.get_mut(id) {
                s.name = name.clone();
            }
        }
        // Coincident faces (e.g. the same cylinder in both operands) leave
        // zero-volume slivers: drop pieces with no real volume.
        let min = 1e-6 * vr.max(1e-9);
        ids.retain(|&id| {
            let mut t = Vec::new();
            let v = trial
                .solid_polygons(id, &mut t)
                .map(|p| csg::volume(&p))
                .unwrap_or(0.0);
            if v <= min {
                trial.remove_solid(id);
                false
            } else {
                true
            }
        });
        if ids.is_empty() {
            return Err(KernelError::Degenerate("the result is empty"));
        }
        *self = trial;
        Ok(ids)
    }

    /// Builds a solid (or, with `open`, a surface body) from tagged
    /// polygons (outward counter-clockwise). See the module docs.
    pub fn add_polygons(
        &mut self,
        polys: &[CsgPolygon],
        tags: &[FaceTag],
        open: bool,
    ) -> KernelResult<Vec<SolidId>> {
        // 1. Weld vertices.
        let mut pos: Vec<Point3> = Vec::new();
        let mut grid: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
        let cell = |p: &Point3| {
            (
                (p.x / WELD / 4.0).floor() as i64,
                (p.y / WELD / 4.0).floor() as i64,
                (p.z / WELD / 4.0).floor() as i64,
            )
        };
        let mut weld = |p: Point3, pos: &mut Vec<Point3>| -> usize {
            let (cx, cy, cz) = cell(&p);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        if let Some(list) = grid.get(&(cx + dx, cy + dy, cz + dz)) {
                            for &i in list {
                                if (pos[i] - p).norm() <= WELD {
                                    return i;
                                }
                            }
                        }
                    }
                }
            }
            pos.push(p);
            grid.entry((cx, cy, cz)).or_default().push(pos.len() - 1);
            pos.len() - 1
        };
        let mut rings: Vec<(Vec<usize>, usize, Vector3)> = Vec::new();
        for p in polys {
            let mut ids: Vec<usize> = p.pts.iter().map(|q| weld(*q, &mut pos)).collect();
            ids.dedup();
            while ids.len() > 1 && ids.first() == ids.last() {
                ids.pop();
            }
            if ids.len() >= 3 {
                rings.push((ids, p.tag, p.normal));
            }
        }
        // 2. Repair T-junctions: insert every vertex lying inside an edge.
        let size = pos
            .iter()
            .fold(0.0, |m: Real, p| m.max(p.coords.amax()))
            .max(1.0);
        let h = (size / 64.0).max(WELD * 10.0);
        let mut vgrid: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
        let key = |p: &Point3| {
            (
                (p.x / h).floor() as i64,
                (p.y / h).floor() as i64,
                (p.z / h).floor() as i64,
            )
        };
        for (i, p) in pos.iter().enumerate() {
            vgrid.entry(key(p)).or_default().push(i);
        }
        let on_segment = |a: usize, b: usize| -> Vec<usize> {
            let (pa, pb) = (pos[a], pos[b]);
            let d = pb - pa;
            let len2 = d.norm_squared();
            if len2 < 1e-24 {
                return Vec::new();
            }
            let (lo, hi) = (pa.inf(&pb), pa.sup(&pb));
            let (k0, k1) = (key(&lo), key(&hi));
            let mut hits: Vec<(Real, usize)> = Vec::new();
            for x in k0.0 - 1..=k1.0 + 1 {
                for y in k0.1 - 1..=k1.1 + 1 {
                    for z in k0.2 - 1..=k1.2 + 1 {
                        let Some(list) = vgrid.get(&(x, y, z)) else {
                            continue;
                        };
                        for &v in list {
                            if v == a || v == b {
                                continue;
                            }
                            let t = (pos[v] - pa).dot(&d) / len2;
                            if t <= 1e-9 || t >= 1.0 - 1e-9 {
                                continue;
                            }
                            if (pa + d * t - pos[v]).norm() <= WELD {
                                hits.push((t, v));
                            }
                        }
                    }
                }
            }
            hits.sort_by(|x, y| x.0.total_cmp(&y.0));
            hits.dedup_by_key(|x| x.1);
            hits.into_iter().map(|(_, v)| v).collect()
        };
        for (ids, _, _) in &mut rings {
            let mut full = Vec::with_capacity(ids.len());
            for i in 0..ids.len() {
                let (a, b) = (ids[i], ids[(i + 1) % ids.len()]);
                full.push(a);
                full.extend(on_segment(a, b));
            }
            *ids = full;
        }
        // 3. Group: coplanar fragments of planar faces merge; curved
        // fragments stay facets.
        let mut groups: HashMap<(i64, i64, i64, i64), Vec<usize>> = HashMap::new();
        let mut facets: Vec<usize> = Vec::new();
        for (k, (ids, tag, n)) in rings.iter().enumerate() {
            if tags.get(*tag).is_some_and(|t| t.curved) {
                facets.push(k);
                continue;
            }
            let w = n.dot(&pos[ids[0]].coords);
            let q = |v: Real, s: Real| (v * s).round() as i64;
            groups
                .entry((q(n.x, 1e4), q(n.y, 1e4), q(n.z, 1e4), q(w, 1e3)))
                .or_default()
                .push(k);
        }
        // 4. Faces.
        let mut vid: HashMap<usize, VertexId> = HashMap::new();
        let mut edge_of: HashMap<(usize, usize), EdgeId> = HashMap::new();
        let mut facet_surface: HashMap<usize, SurfaceId> = HashMap::new();
        let mut faces = Vec::new();
        let mut mk_loop = |me: &mut BRep, ids: &[usize]| -> KernelResult<Loop> {
            // Repeated consecutive vertices (merges, T-junction repair)
            // would make zero-length edges.
            let mut ids = ids.to_vec();
            ids.dedup();
            while ids.len() > 1 && ids.first() == ids.last() {
                ids.pop();
            }
            if ids.len() < 3 {
                return Err(KernelError::Degenerate(
                    "loop with fewer than three vertices",
                ));
            }
            let ids = &ids[..];
            let mut coedges = Vec::with_capacity(ids.len());
            for i in 0..ids.len() {
                let (a, b) = (ids[i], ids[(i + 1) % ids.len()]);
                for v in [a, b] {
                    if let std::collections::hash_map::Entry::Vacant(e) = vid.entry(v) {
                        e.insert(me.add_vertex(pos[v]));
                    }
                }
                let k = (a.min(b), a.max(b));
                let edge = match edge_of.get(&k) {
                    Some(&e) => e,
                    None => {
                        let e = me.add_line_edge(vid[&k.0], vid[&k.1])?;
                        edge_of.insert(k, e);
                        e
                    }
                };
                let sense = if a == k.0 {
                    Sense::Same
                } else {
                    Sense::Reversed
                };
                coedges.push(Coedge { edge, sense });
            }
            Ok(Loop { coedges })
        };
        struct Planned {
            outer: Vec<usize>,
            holes: Vec<Vec<usize>>,
            normal: Vector3,
            name: Option<String>,
            facet: Option<usize>,
        }
        let mut planned: Vec<Planned> = Vec::new();
        let mut group_list: Vec<Vec<usize>> = groups.into_values().collect();
        group_list.sort(); // deterministic
        for members in group_list {
            let n = rings[members[0]].2;
            // Directed edges; opposite pairs cancel (inner seams).
            let mut count: HashMap<(usize, usize), i32> = HashMap::new();
            for &k in &members {
                let ids = &rings[k].0;
                for i in 0..ids.len() {
                    let (a, b) = (ids[i], ids[(i + 1) % ids.len()]);
                    if let Some(c) = count.get_mut(&(b, a))
                        && *c > 0
                    {
                        *c -= 1;
                        continue;
                    }
                    *count.entry((a, b)).or_default() += 1;
                }
            }
            let mut out_edges: HashMap<usize, Vec<usize>> = HashMap::new();
            let mut remaining = 0;
            let mut keys: Vec<(usize, usize)> = count
                .iter()
                .filter(|(_, c)| **c > 0)
                .map(|(k, _)| *k)
                .collect();
            keys.sort();
            for (a, b) in keys {
                for _ in 0..count[&(a, b)] {
                    out_edges.entry(a).or_default().push(b);
                    remaining += 1;
                }
            }
            // Frame of the plane.
            let x = (pos[rings[members[0]].0[1]] - pos[rings[members[0]].0[0]])
                .try_normalize(1e-12)
                .unwrap_or_else(|| {
                    super::any_perpendicular(&na::Unit::new_normalize(n)).into_inner()
                });
            let y = n.cross(&x);
            let uv = |p: usize| na::Point2::new(pos[p].coords.dot(&x), pos[p].coords.dot(&y));
            // Trace loops (at a pinch take the sharpest right turn).
            let mut loops: Vec<Vec<usize>> = Vec::new();
            while remaining > 0 {
                // `remaining > 0` guarantees a non-empty edge list.
                let Some(start) = out_edges
                    .iter()
                    .filter(|(_, v)| !v.is_empty())
                    .map(|(k, _)| *k)
                    .min()
                else {
                    break;
                };
                let Some(mut cur) = out_edges.get_mut(&start).and_then(Vec::pop) else {
                    break;
                };
                let mut lp = vec![start];
                let mut prev = start;
                remaining -= 1;
                let mut guard = 0;
                while cur != start && guard < 1_000_000 {
                    guard += 1;
                    lp.push(cur);
                    let outs = out_edges.get_mut(&cur);
                    let Some(outs) = outs.filter(|o| !o.is_empty()) else {
                        break; // open chain: shouldn't happen for closed input
                    };
                    let k = if outs.len() == 1 {
                        0
                    } else {
                        let din = uv(cur) - uv(prev);
                        let ang = |t: usize| {
                            let d = uv(t) - uv(cur);
                            // Clockwise turn from the incoming direction.
                            -(din.perp(&d)).atan2(din.dot(&d))
                        };
                        (0..outs.len())
                            .max_by(|&i, &j| ang(outs[i]).total_cmp(&ang(outs[j])))
                            .unwrap_or(0)
                    };
                    let next = outs.swap_remove(k);
                    remaining -= 1;
                    prev = cur;
                    cur = next;
                }
                if lp.len() >= 3 {
                    loops.push(lp);
                }
            }
            let area = |lp: &[usize]| {
                let pts: Vec<na::Point2<Real>> = lp.iter().map(|&p| uv(p)).collect();
                (0..pts.len())
                    .map(|i| pts[i].coords.perp(&pts[(i + 1) % pts.len()].coords))
                    .sum::<Real>()
                    / 2.0
            };
            let (outers, holes): (Vec<Vec<usize>>, Vec<Vec<usize>>) =
                loops.into_iter().partition(|l| area(l) > 0.0);
            // The face's name: the tag contributing most area.
            let mut by_tag: HashMap<usize, Real> = HashMap::new();
            for &k in &members {
                *by_tag.entry(rings[k].1).or_default() += area(&rings[k].0).abs();
            }
            let tag = by_tag
                .into_iter()
                .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
                .map(|(t, _)| t);
            let name = tag.and_then(|t| tags.get(t)).and_then(|t| t.name.clone());
            let inside = |p: na::Point2<Real>, poly: &[usize]| {
                let mut c = false;
                for i in 0..poly.len() {
                    let (a, b) = (uv(poly[i]), uv(poly[(i + 1) % poly.len()]));
                    if (a.y > p.y) != (b.y > p.y)
                        && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x)
                    {
                        c = !c;
                    }
                }
                c
            };
            let mut assigned: Vec<Vec<Vec<usize>>> = vec![Vec::new(); outers.len()];
            for h in holes {
                // A point just inside the hole's boundary, to test with.
                let probe = {
                    let (a, b) = (uv(h[0]), uv(h[1]));
                    let m = na::center(&a, &b);
                    let d = (b - a).normalize();
                    m + na::Vector2::new(d.y, -d.x) * 1e-4
                };
                let host = (0..outers.len())
                    .filter(|&o| inside(probe, &outers[o]))
                    .min_by(|&i, &j| area(&outers[i]).total_cmp(&area(&outers[j])));
                if let Some(o) = host {
                    assigned[o].push(h);
                }
            }
            for (o, outer) in outers.into_iter().enumerate() {
                planned.push(Planned {
                    outer,
                    holes: std::mem::take(&mut assigned[o]),
                    normal: n,
                    name: name.clone(),
                    facet: None,
                });
            }
        }
        for k in facets {
            let (ids, tag, n) = &rings[k];
            planned.push(Planned {
                outer: ids.clone(),
                holes: Vec::new(),
                normal: *n,
                name: tags[*tag].name.clone(),
                facet: Some(*tag),
            });
        }
        // 5. Drop straight vertices: a vertex with exactly two neighbours,
        // collinear with them, is a split artefact (BSP planes, T-junction
        // repair) — removing it everywhere keeps the shell watertight.
        let mut nbrs: HashMap<usize, Vec<usize>> = HashMap::new();
        for p in &planned {
            for lp in std::iter::once(&p.outer).chain(&p.holes) {
                for i in 0..lp.len() {
                    let (a, b) = (lp[i], lp[(i + 1) % lp.len()]);
                    for (u, v) in [(a, b), (b, a)] {
                        let e = nbrs.entry(u).or_default();
                        if !e.contains(&v) {
                            e.push(v);
                        }
                    }
                }
            }
        }
        let straight = |v: usize| {
            nbrs.get(&v).is_some_and(|n| {
                n.len() == 2 && {
                    let (d1, d2) = (pos[v] - pos[n[0]], pos[n[1]] - pos[v]);
                    d1.dot(&d2) > 0.0 && d1.cross(&d2).norm() <= 1e-9 * d1.norm() * d2.norm()
                }
            })
        };
        for p in &mut planned {
            for lp in std::iter::once(&mut p.outer).chain(p.holes.iter_mut()) {
                let kept: Vec<usize> = lp.iter().copied().filter(|&v| !straight(v)).collect();
                if kept.len() >= 3 {
                    *lp = kept;
                }
            }
        }
        // 6. Topology.
        for p in planned {
            let x = super::any_perpendicular(&na::Unit::new_normalize(p.normal)).into_inner();
            let plane = Plane::new(pos[p.outer[0]], p.normal, x)?;
            let s = self.add_surface(Surface::Plane(plane));
            // A loop that collapsed to fewer than three vertices bounds no
            // area: the face (outer) or the hole is dropped.
            let Ok(ol) = mk_loop(self, &p.outer) else {
                continue;
            };
            let il: Vec<Loop> = p
                .holes
                .iter()
                .filter_map(|h| mk_loop(self, h).ok())
                .collect();
            let f = self.add_face(s, ol, il, Sense::Same)?;
            self.faces[f].name = p.name;
            if let Some(tag) = p.facet
                && let Some(surface) = &tags[tag].surface
            {
                let exact = match facet_surface.get(&tag) {
                    Some(&e) => e,
                    None => {
                        let e = self.add_surface(surface.clone());
                        facet_surface.insert(tag, e);
                        e
                    }
                };
                self.faces[f].facet_of = Some(exact);
            }
            faces.push(f);
        }
        // 7. One solid per connected component (faces sharing edges).
        let mut parent: Vec<usize> = (0..faces.len()).collect();
        fn root(p: &mut [usize], mut i: usize) -> usize {
            while p[i] != i {
                p[i] = p[p[i]];
                i = p[i];
            }
            i
        }
        let mut by_edge: HashMap<EdgeId, usize> = HashMap::new();
        for (i, &f) in faces.iter().enumerate() {
            let face = &self.faces[f];
            for lp in std::iter::once(&face.outer).chain(&face.inner) {
                for ce in &lp.coedges {
                    if let Some(&j) = by_edge.get(&ce.edge) {
                        let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                        parent[a] = b;
                    } else {
                        by_edge.insert(ce.edge, i);
                    }
                }
            }
        }
        let mut comps: Vec<(usize, Vec<super::FaceId>)> = Vec::new();
        for (i, &f) in faces.iter().enumerate() {
            let r = root(&mut parent, i);
            match comps.iter_mut().find(|c| c.0 == r) {
                Some(c) => c.1.push(f),
                None => comps.push((r, vec![f])),
            }
        }
        let mut out = Vec::new();
        for (_, faces) in comps {
            let shell = Shell { faces };
            if !open && let Err(e) = self.check_closed_shell(&shell) {
                log::warn!("rebuilt body is not watertight: {e}");
            }
            out.push(self.solids.insert(Solid {
                shells: vec![shell],
                open,
                name: None,
            }));
        }
        Ok(out)
    }

    /// The faces that use edge `e` (two for a closed solid).
    pub fn edge_faces(&self, e: EdgeId) -> Vec<super::FaceId> {
        self.faces()
            .filter(|(_, f)| {
                std::iter::once(&f.outer)
                    .chain(&f.inner)
                    .any(|lp| lp.coedges.iter().any(|ce| ce.edge == e))
            })
            .map(|(id, _)| id)
            .collect()
    }

    /// Every visible edge of the body: without the hidden edges between
    /// facets of one curved face.
    pub fn visible_edges(&self) -> Vec<EdgeId> {
        self.feature_edges("")
    }

    /// The visible edges of the faces a feature made (names `prefix:…` or
    /// `prefix#…`; an empty prefix: all faces), without the hidden edges
    /// between facets of one curved face.
    pub fn feature_edges(&self, prefix: &str) -> Vec<EdgeId> {
        if prefix.is_empty() {
            return self.edges_where(|_| true);
        }
        let (a, b) = (format!("{prefix}:"), format!("{prefix}#"));
        self.edges_where(|n| n.starts_with(&a) || n.starts_with(&b))
    }

    fn edges_where(&self, mine: impl Fn(&str) -> bool) -> Vec<EdgeId> {
        type Use = (bool, Option<SurfaceId>, super::FaceId);
        let mut uses: HashMap<EdgeId, Vec<Use>> = HashMap::new();
        for (fid, face) in self.faces() {
            let mine = mine(face.name.as_deref().unwrap_or(""));
            for lp in std::iter::once(&face.outer).chain(&face.inner) {
                for ce in &lp.coedges {
                    uses.entry(ce.edge)
                        .or_default()
                        .push((mine, face.facet_of, fid));
                }
            }
        }
        // Hidden: edges between facets of one curved face, and seams between
        // kernel pieces of one curved surface (a cylinder in halves).
        let mut out: Vec<EdgeId> = uses
            .into_iter()
            .filter(|(_, u)| {
                u.iter().any(|x| x.0)
                    && !(u.len() == 2 && u[0].1.is_some() && u[0].1 == u[1].1)
                    && !(u.len() == 2 && self.is_smooth_seam(u[0].2, u[1].2))
            })
            .map(|(e, _)| e)
            .collect();
        out.sort();
        out
    }

    /// Turns solid `id` into an open surface body without the faces named
    /// in `drop` (e.g. the caps of an extrusion: Surface output).
    pub fn open_up(&mut self, id: SolidId, drop: &[String]) {
        let Some(solid) = self.solids.get_mut(id) else {
            return;
        };
        solid.open = true;
        let mut removed = Vec::new();
        for shell in &mut solid.shells {
            shell.faces.retain(|f| {
                let keep = !self.faces[*f]
                    .name
                    .as_ref()
                    .is_some_and(|n| drop.contains(n));
                if !keep {
                    removed.push(*f);
                }
                keep
            });
        }
        for f in removed {
            if let Some(face) = self.faces.remove(f) {
                self.surfaces.remove(face.surface);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cut_a_hole_and_rebuild_a_valid_named_solid() {
        let mut body = BRep::new();
        let plate = body
            .make_box_named(Point3::origin(), Point3::new(40.0, 20.0, 5.0), "Plate")
            .unwrap();
        let mut tool = BRep::new();
        let pin = tool
            .make_box_named(
                Point3::new(15.0, 5.0, -1.0),
                Point3::new(25.0, 15.0, 6.0),
                "Pin",
            )
            .unwrap();
        let r = body.boolean(plate, &tool, pin, BoolOp::Subtract).unwrap()[0];
        assert_eq!(body.solids().count(), 1);
        let st = body.validate_solid(r).unwrap();
        assert_eq!(st.genus(), Some(1), "a plate with a square hole");
        // Top face: one face with one hole, still named.
        let top = body.find_face("Plate:Top").unwrap();
        let tf = body.face(top).unwrap();
        assert_eq!(tf.inner.len(), 1);
        assert_eq!(tf.outer.coedges.len(), 4, "no split artefacts left");
        assert_eq!(tf.inner[0].coedges.len(), 4);
        assert!(
            body.find_face("Pin:Left").is_some(),
            "the hole's walls keep the tool's names"
        );
        // Join a boss on top: coplanar side faces merge.
        let mut boss = BRep::new();
        let b = boss
            .make_box_named(
                Point3::new(0.0, 0.0, 5.0),
                Point3::new(10.0, 20.0, 10.0),
                "Boss",
            )
            .unwrap();
        let r = body.boolean(r, &boss, b, BoolOp::Union).unwrap()[0];
        body.validate_solid(r).unwrap();
        // Cut that misses is refused; the body is unchanged.
        let mut far = BRep::new();
        let f = far
            .make_box_named(
                Point3::new(100.0, 0.0, 0.0),
                Point3::new(110.0, 10.0, 10.0),
                "Far",
            )
            .unwrap();
        let before = body.faces().count();
        assert!(body.boolean(r, &far, f, BoolOp::Subtract).is_err());
        assert_eq!(body.faces().count(), before);
        assert!(body.solid(r).is_some(), "the body is unchanged");
    }
}
