//! Grill (`docs/spec/5.8-plastic-parts.md`): a vent in a thin wall. The opening inside the
//! boundary's wall is cut through the wall; the boundary, the islands, the ribs and the spars are
//! joined at their levels, measured from the wall's outside. On a flat wall square to the sketch
//! the levels are planes; on any other wall they follow its outside (its offsets, cut to the
//! outlines).

use serde::{Deserialize, Serialize};

use super::common::{self, Piece, wall_at};
use super::{is_false, is_zero};
use crate::kernel::exact::{self, ExactOp, ExactSolid};
use crate::kernel::{BRep, Plane, Point3, Real, Vector3};
use crate::part::boolean::Operation;
use crate::part::emboss::{face_at, skin};
use crate::part::exact::{Fail, combine};
use crate::part::extrude::ProfileRef;
use crate::part::rib::Rays;
use crate::sketch::{EntityId, Point2, Sketch};

/// Curves of a sketch used as centre lines.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GrillCurves {
    /// The sketch.
    pub sketch: String,
    /// Its curves; empty = none.
    pub curves: Vec<EntityId>,
    /// Thickness.
    pub thickness: Real,
    /// Height (ribs).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub height: Real,
    /// Ribs: below the boundary's top. Spars: below the ribs' top.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub top_offset: Real,
    /// Spars: past the ribs' bottom.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub bottom_offset: Real,
}

/// Grill.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GrillOp {
    /// The boundary's sketch.
    pub sketch: String,
    /// Its region; `None` = the sketch's largest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary: Option<ProfileRef>,
    /// Target bodies; empty = whichever it reaches.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<String>,
    /// Against the default direction (towards the nearest body).
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip: bool,
    /// The boundary's wall.
    pub thickness: Real,
    /// Its height.
    pub height: Real,
    /// Its top above the wall's outside.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub outside_height: Real,
    /// Islands: regions of the boundary's sketch or of `island_sketch`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub islands: Vec<ProfileRef>,
    /// The islands' sketch when it is not the boundary's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub island_sketch: Option<String>,
    /// Their wall; 0 = solid.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub island_thickness: Real,
    /// The ribs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ribs: Option<GrillCurves>,
    /// The spars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spars: Option<GrillCurves>,
    /// Draft (radians): the outside stays neutral, walls widen inwards.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub draft: Real,
}

impl GrillOp {
    /// A grill in the largest region of `sketch`.
    pub fn new(sketch: impl Into<String>) -> Self {
        Self {
            sketch: sketch.into(),
            boundary: None,
            solids: Vec::new(),
            flip: false,
            thickness: 1.0,
            height: 2.0,
            outside_height: 0.0,
            islands: Vec::new(),
            island_sketch: None,
            island_thickness: 0.0,
            ribs: None,
            spars: None,
            draft: 0.0,
        }
    }

    /// Refuses a definition that cannot be built (before any geometry).
    pub fn validate(&self) -> Result<(), String> {
        if self.sketch.is_empty() {
            return Err("select the boundary profile".into());
        }
        if self.thickness <= 0.0 || self.height <= 0.0 {
            return Err("the boundary's thickness and height must be positive".into());
        }
        if self.island_thickness < 0.0 {
            return Err("the island's thickness cannot be negative".into());
        }
        for (c, name) in [(&self.ribs, "rib"), (&self.spars, "spar")] {
            if let Some(c) = c
                && c.thickness <= 0.0
            {
                return Err(format!("the {name} thickness must be positive"));
            }
        }
        if let Some(r) = &self.ribs
            && r.height <= 0.0
        {
            return Err("the rib height must be positive".into());
        }
        if !(0.0..45f64.to_radians()).contains(&self.draft) {
            return Err("the draft must lie between 0 and 45 degrees".into());
        }
        Ok(())
    }

    /// The browser's tooltip.
    pub fn describe(&self) -> String {
        format!("Grill on {}", self.sketch)
    }

    /// The sketches it reads besides the boundary's.
    pub fn other_sketches(&self) -> Vec<&str> {
        let mut v: Vec<&str> = Vec::new();
        if let Some(s) = &self.island_sketch {
            v.push(s);
        }
        for c in [&self.ribs, &self.spars].into_iter().flatten() {
            if c.sketch != self.sketch {
                v.push(&c.sketch);
            }
        }
        v
    }
}

/// Chains of a sketch's curves, as pieces in order; closed ones marked.
fn chains(sk: &Sketch, curves: &[EntityId]) -> Result<Vec<(Vec<Piece>, bool)>, String> {
    let tol = 1e-6;
    let mut left: Vec<Piece> = Vec::new();
    for &c in curves {
        let curve = sk
            .curve_of(c)
            .ok_or("a curve no longer exists in its sketch")?;
        left.extend(common::pieces_of_curve(&curve));
    }
    let mut out = Vec::new();
    while let Some(first) = left.pop() {
        let mut chain = vec![first];
        loop {
            let (Some(a), Some(b)) = (chain.first().map(Piece::start), chain.last().map(Piece::end))
            else {
                break;
            };
            if (a - b).norm() < tol {
                break;
            }
            let at = left.iter().position(|p| {
                (p.start() - b).norm() < tol
                    || (p.end() - b).norm() < tol
                    || (p.start() - a).norm() < tol
                    || (p.end() - a).norm() < tol
            });
            let Some(k) = at else {
                break;
            };
            let p = left.remove(k);
            if (p.start() - b).norm() < tol {
                chain.push(p);
            } else if (p.end() - b).norm() < tol {
                chain.push(p.reversed());
            } else if (p.end() - a).norm() < tol {
                chain.insert(0, p);
            } else {
                chain.insert(0, p.reversed());
            }
        }
        let closed = match (chain.first(), chain.last()) {
            (Some(a), Some(b)) => (a.start() - b.end()).norm() < tol,
            _ => false,
        };
        out.push((chain, closed));
    }
    Ok(out)
}

/// A chain's band of thickness `t` as outlines: `(outer, holes)`.
fn band(chain: &[Piece], closed: bool, t: Real) -> Result<(Vec<Piece>, Vec<Vec<Piece>>), String> {
    if closed {
        let (a, b) = (
            common::offset(chain, true, t / 2.0)?,
            common::offset(chain, true, -t / 2.0)?,
        );
        Ok(if common::area(&a).abs() >= common::area(&b).abs() {
            (a, vec![b])
        } else {
            (b, vec![a])
        })
    } else {
        Ok((common::open_band(chain, t / 2.0, -t / 2.0)?, Vec::new()))
    }
}

/// How the levels are made.
enum Levels<'a> {
    /// A flat outside at this distance along the direction from the sketch.
    Flat(Real),
    /// A curved outside: those faces of `cur[.0]`.
    Curved(&'a ExactSolid, Vec<usize>, Real),
}

struct Maker<'a> {
    plane: &'a Plane,
    dn: Real,
    levels: Levels<'a>,
    draft: Real,
}

impl Maker<'_> {
    /// The solid over an outline between `top` and `bottom`, depths into the wall from its
    /// outside (a negative `top` stands above it).
    fn make(
        &self,
        outer: &[Piece],
        holes: &[Vec<Piece>],
        (top, bottom): (Real, Real),
        prefix: &str,
    ) -> Result<ExactSolid, Fail> {
        match &self.levels {
            Levels::Flat(at) => self.prism(outer, holes, (at + top, at + bottom), prefix),
            Levels::Curved(s, faces, reach) => {
                // A tall prism cut by the slab between the wall's offsets.
                let tall = self.prism(outer, holes, (-*reach, *reach), prefix)?;
                let sheet = exact::offset_sheet(s, faces, -top, None, prefix).map_err(Fail::User)?;
                let slab = exact::thicken_sheet(&sheet, -(bottom - top), false, None, prefix)
                    .map_err(Fail::User)?;
                exact::boolean(&slab, &tall, ExactOp::Intersect)
                    .map_err(Fail::Kernel)?
                    .ok_or_else(|| Fail::User("an outline misses the wall".into()))
            }
        }
    }

    fn prism(
        &self,
        outer: &[Piece],
        holes: &[Vec<Piece>],
        (a0, a1): (Real, Real),
        prefix: &str,
    ) -> Result<ExactSolid, Fail> {
        // Walls widen away from the outside: the material grows, the holes shrink.
        let grow = (a1 - a0).abs() * self.draft.tan();
        let turn = |p: &[Piece]| if common::area(p) >= 0.0 { 1.0 } else { -1.0 };
        let out1 = common::offset(outer, true, -grow * turn(outer)).map_err(Fail::User)?;
        let mut hs = Vec::with_capacity(holes.len());
        for h in holes {
            // The hole gets smaller: inwards, as seen from inside the hole.
            hs.push((
                h.clone(),
                common::offset(h, true, grow * turn(h)).map_err(Fail::User)?,
            ));
        }
        common::prism(
            self.plane,
            (a0 * self.dn, outer),
            (a1 * self.dn, &out1),
            &hs,
            prefix,
        )
    }
}

/// The regions of a sketch without the ribs' and spars' curves: centre lines drawn across the
/// boundary must not cut it (Inventor lets them share its sketch).
pub fn regions_of(op: &GrillOp, sk: &Sketch, id: &str) -> Vec<crate::sketch::Region> {
    let mut clean = sk.clone();
    for c in [&op.ribs, &op.spars].into_iter().flatten() {
        if c.sketch == id {
            for &e in &c.curves {
                let _ = clean.set_style(e, crate::sketch::Style::Construction);
            }
        }
    }
    clean.regions()
}

/// The boundary's outline, in the sketch.
fn boundary_of(op: &GrillOp, sk: &Sketch) -> Result<Vec<Piece>, String> {
    let regions = regions_of(op, sk, &op.sketch);
    let found = match &op.boundary {
        Some(r) => r.resolve(&regions),
        None => regions.iter().max_by(|a, b| {
            a.outer
                .area()
                .abs()
                .total_cmp(&b.outer.area().abs())
        }),
    };
    let r = found.ok_or("the boundary profile no longer exists")?;
    Ok(common::pieces_of(&r.outer))
}

/// The area of the openings (mm²), drafts left out: the boundary's inside less islands, ribs and
/// spars, scanned along the sketch's Y.
pub fn flow_area(op: &GrillOp, sketches: &[crate::part::PartSketch]) -> Result<Real, String> {
    let find = |id: &str| {
        sketches
            .iter()
            .find(|s| s.id == id)
            .map(|s| &s.sketch)
            .ok_or_else(|| format!("the sketch {id} no longer exists"))
    };
    let sk = find(&op.sketch)?;
    let outline = boundary_of(op, sk)?;
    let turn = if common::area(&outline) >= 0.0 { 1.0 } else { -1.0 };
    let inside = common::polygon(&common::offset(&outline, true, op.thickness * turn)?);
    // What fills the opening: polygons with holes.
    let mut solid: Vec<(Vec<Point2>, Vec<Vec<Point2>>)> = Vec::new();
    let island_id = op.island_sketch.as_deref().unwrap_or(&op.sketch);
    let regions = regions_of(op, find(island_id)?, island_id);
    for i in &op.islands {
        if let Some(r) = i.resolve(&regions) {
            let o = common::pieces_of(&r.outer);
            solid.push((common::polygon(&o), Vec::new()));
        }
    }
    for c in [&op.ribs, &op.spars].into_iter().flatten() {
        let s = find(&c.sketch)?;
        for (chain, closed) in chains(s, &c.curves)? {
            let (o, h) = band(&chain, closed, c.thickness)?;
            solid.push((
                common::polygon(&o),
                h.iter().map(|x| common::polygon(x)).collect(),
            ));
        }
    }
    let (lo, hi) = inside.iter().fold((Real::MAX, Real::MIN), |(a, b), p| {
        (a.min(p.y), b.max(p.y))
    });
    const LINES: usize = 4000;
    let dy = (hi - lo) / LINES as Real;
    let mut total = 0.0;
    for k in 0..LINES {
        let y = lo + dy * (k as Real + 0.5);
        let open = crossings(&inside, y);
        let mut spans = pairs(&open);
        for (o, hs) in &solid {
            let mut cover = pairs(&crossings(o, y));
            for h in hs {
                cover = subtract(&cover, &pairs(&crossings(h, y)));
            }
            spans = subtract(&spans, &cover);
        }
        total += spans.iter().map(|s| s.1 - s.0).sum::<Real>() * dy;
    }
    Ok(total)
}

fn crossings(poly: &[Point2], y: Real) -> Vec<Real> {
    let mut xs = Vec::new();
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a.y <= y) != (b.y <= y) {
            xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
        }
    }
    xs.sort_by(Real::total_cmp);
    xs
}

fn pairs(xs: &[Real]) -> Vec<(Real, Real)> {
    xs.chunks_exact(2).map(|c| (c[0], c[1])).collect()
}

fn subtract(a: &[(Real, Real)], b: &[(Real, Real)]) -> Vec<(Real, Real)> {
    let mut out = a.to_vec();
    for &(s, e) in b {
        let mut next = Vec::with_capacity(out.len() + 1);
        for &(x, y) in &out {
            if e <= x || s >= y {
                next.push((x, y));
                continue;
            }
            if s > x {
                next.push((x, s));
            }
            if e < y {
                next.push((e, y));
            }
        }
        out = next;
    }
    out
}

/// Builds Grill (`cur`: the solids before it; surface bodies stay outside).
pub(in crate::part) fn grill(
    op: &GrillOp,
    id: &str,
    body: &BRep,
    sketches: &[crate::part::PartSketch],
    cur: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    let user = |m: String| Fail::User(m);
    let find = |sid: &str| {
        sketches
            .iter()
            .find(|s| s.id == sid)
            .map(|s| &s.sketch)
            .ok_or_else(|| Fail::User(format!("the sketch {sid} no longer exists")))
    };
    let sk = find(&op.sketch)?;
    let plane = &sk.plane;
    let n = plane.normal.into_inner();
    let outline = boundary_of(op, sk).map_err(user)?;
    let turn = if common::area(&outline) >= 0.0 { 1.0 } else { -1.0 };
    let t = op.thickness;
    let inner = common::offset(&outline, true, t * turn).map_err(user)?;
    let trim = common::offset(&outline, true, t / 2.0 * turn).map_err(user)?;
    let rays = Rays::of(body, &op.solids);
    // Towards the nearest wall; every point of the outline must have one.
    let samples: Vec<Point3> = common::polygon(&outline)
        .iter()
        .map(|q| plane.point_at(q.x, q.y))
        .collect();
    let step = (samples.len() / 64).max(1);
    let toward = |d: Vector3| {
        samples
            .iter()
            .filter_map(|p| rays.entry(*p, d).map(|x| x.0))
            .fold(None, |a: Option<Real>, v| Some(a.map_or(v, |x| x.min(v))))
    };
    let dn0 = match (toward(n), toward(-n)) {
        (Some(a), Some(b)) => {
            if a <= b {
                1.0
            } else {
                -1.0
            }
        }
        (Some(_), None) => 1.0,
        (None, Some(_)) => -1.0,
        (None, None) => {
            return Err(Fail::User("the boundary must lie over the body".into()));
        }
    };
    let dn = if op.flip { -dn0 } else { dn0 };
    let d = n * dn;
    let (mut near, mut far) = (Real::INFINITY, Real::NEG_INFINITY);
    let (mut flat_at, mut flat) = (None, true);
    for p in samples.iter().step_by(step) {
        let Some((enter, exit)) = wall_at(&rays, *p, d) else {
            return Err(Fail::User(
                "the boundary's outline must lie entirely on the part".into(),
            ));
        };
        near = near.min(enter);
        far = far.max(exit);
        match flat_at {
            None => flat_at = Some(enter),
            Some(a) if (enter - a as Real).abs() > 1e-6 => flat = false,
            _ => {}
        }
    }
    // The faces of the outside, when it is not one flat level.
    let tris = skin(body);
    let mut faces = Vec::new();
    let mut solid = None;
    for p in samples.iter().step_by(step) {
        let Some((enter, _)) = wall_at(&rays, *p, d) else {
            continue;
        };
        if let Some(found) = face_at(body, &tris, cur, *p + d * enter, d) {
            flat = flat && found.2;
            solid.get_or_insert(found.0);
            if !faces.contains(&found.1) {
                faces.push(found.1);
            }
        }
    }
    let levels = match (flat, solid) {
        (true, _) => Levels::Flat(near),
        (false, Some(s)) => Levels::Curved(&cur[s], faces, rays.size * 2.0),
        (false, None) => {
            return Err(Fail::User(
                "the boundary's outline must lie entirely on the part".into(),
            ));
        }
    };
    let make = Maker {
        plane,
        dn,
        levels,
        draft: op.draft,
    };
    let top = -op.outside_height;
    let bottom = top + op.height;
    // The opening, through the wall.
    let opening = common::prism(
        plane,
        ((near - op.outside_height - 1.0) * dn, &inner),
        ((far + 1.0) * dn, &inner),
        &[],
        &format!("{id}:Opening"),
    )?;
    let out = combine(cur.to_vec(), vec![opening], Operation::Cut, &op.solids, false)?;
    // The boundary's wall.
    let mut joined = vec![make.make(
        &outline,
        std::slice::from_ref(&inner),
        (top, bottom),
        &format!("{id}:Boundary"),
    )?];
    // Islands, trimmed to the boundary by intersection; ribs and spars stop at them.
    let island_id = op.island_sketch.as_deref().unwrap_or(&op.sketch);
    let regions = regions_of(op, find(island_id)?, island_id);
    let tall = |pieces: &[Piece], holes: &[Vec<Piece>], prefix: &str| {
        let hs: Vec<(Vec<Piece>, Vec<Piece>)> =
            holes.iter().map(|h| (h.clone(), h.clone())).collect();
        common::prism(
            plane,
            ((near - op.outside_height - 2.0) * dn, pieces),
            ((far + 2.0) * dn, pieces),
            &hs,
            prefix,
        )
    };
    let frame = tall(&trim, &[], &format!("{id}:Frame"))?;
    let mut island_cut: Vec<ExactSolid> = Vec::new();
    for (k, r) in op.islands.iter().enumerate() {
        let region = r
            .resolve(&regions)
            .ok_or_else(|| Fail::User("an island no longer exists".into()))?;
        let o = common::pieces_of(&region.outer);
        let prefix = format!("{id}:Island{}", k + 1);
        island_cut.push(tall(&o, &[], &format!("{prefix}:Stop"))?);
        let holes = if op.island_thickness > 0.0 {
            let turn = if common::area(&o) >= 0.0 { 1.0 } else { -1.0 };
            vec![common::offset(&o, true, op.island_thickness * turn).map_err(user)?]
        } else {
            Vec::new()
        };
        let body = make.make(&o, &holes, (top, bottom), &prefix)?;
        let cutdown = exact::boolean(&body, &frame, ExactOp::Intersect)
            .map_err(Fail::Kernel)?
            .ok_or_else(|| Fail::User("an island lies outside the boundary".into()))?;
        joined.push(cutdown);
    }
    let rib_levels = op.ribs.as_ref().map(|r| {
        let a = top + r.top_offset;
        (a, a + r.height)
    });
    let spar_levels = op.spars.as_ref().map(|s| {
        let (a, b) = rib_levels.unwrap_or((top, bottom));
        (a + s.top_offset, b + s.bottom_offset)
    });
    for (curves, levels, tag) in [
        (&op.ribs, rib_levels, "Rib"),
        (&op.spars, spar_levels, "Spar"),
    ] {
        let (Some(c), Some(levels)) = (curves, levels) else {
            continue;
        };
        if levels.1 <= levels.0 {
            return Err(Fail::User(format!(
                "the {} has no height left between its offsets",
                tag.to_lowercase()
            )));
        }
        let s = find(&c.sketch)?;
        for (k, (chain, closed)) in chains(s, &c.curves).map_err(user)?.iter().enumerate() {
            let prefix = format!("{id}:{tag}{}", k + 1);
            let (o, h) = band(chain, *closed, c.thickness).map_err(user)?;
            let piece = make.make(&o, &h, levels, &prefix)?;
            let Some(mut cut) = exact::boolean(&piece, &frame, ExactOp::Intersect)
                .map_err(Fail::Kernel)?
            else {
                continue;
            };
            let mut gone = false;
            for stop in &island_cut {
                match exact::boolean(&cut, stop, ExactOp::Cut).map_err(Fail::Kernel)? {
                    Some(left) => cut = left,
                    None => {
                        gone = true;
                        break;
                    }
                }
            }
            if !gone {
                joined.push(cut);
            }
        }
    }
    // One after the other: they cross each other, the fast path for tools apart does not apply.
    let mut solids = out;
    for tool in joined {
        solids = combine(solids, vec![tool], Operation::Join, &op.solids, false)?;
    }
    Ok(solids)
}
