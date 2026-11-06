//! A Cut of prisms straight through a prism, without the general Boolean (spec 0.19 §3.2).
//!
//! An extrusion without taper remembers its section ([`Prism`]): the plane, the two offsets,
//! the region and the face name of each of the region's segments and of each cap, as indices
//! into its `face_names`. When a Cut's target and every tool are such prisms along one
//! direction, every tool passes through both caps and the tools stand inside the target's
//! outline, apart from one another and from the holes already there, the result is the
//! target's section with the tools' sections added as holes, extruded between the same caps.
//! Its faces take their names from the faces they would come from in the Boolean: the caps and
//! walls from the target's, each hole wall from its tool's. Anything else is `Ok(None)`: the
//! caller takes the general path.

use std::sync::Arc;

use super::{ExactSolid, FaceOrigin, Plane, Real, guarded, names_from, plane_of, sg, sk};

/// The section of an extrusion without taper.
#[derive(Clone, Debug)]
pub(super) struct Prism {
    pub plane: Plane,
    /// The offsets of the caps along the plane's normal, `lo < hi`.
    pub lo: Real,
    pub hi: Real,
    pub region: sg::Region2,
    /// Per segment of `region` (outer loop first, then the holes): the index of its wall in the
    /// solid's `face_names`.
    pub sides: Vec<Option<usize>>,
    /// The caps at `lo` and at `hi`.
    pub caps: [Option<usize>; 2],
}

impl Prism {
    /// The prism of a fresh extrusion `body` of `region` between `start` and `end`: its faces
    /// are found through the body's history.
    pub fn of_extrusion(
        body: &sk::Body,
        plane: &Plane,
        region: &sg::Region2,
        start: Real,
        end: Real,
    ) -> Option<Arc<Self>> {
        let h = body.history()?;
        let n = segment_count(region);
        let mut sides = vec![None; n];
        let mut caps = [None, None];
        for (i, f) in h.faces.iter().enumerate() {
            for o in &f.origins {
                match o {
                    FaceOrigin::Profile { segment } if *segment < n => {
                        if sides[*segment].is_some() {
                            // a wall in two faces: not a plain prism
                            return None;
                        }
                        sides[*segment] = Some(i);
                    }
                    // the kernel's start cap lies at the lower offset
                    FaceOrigin::Cap { end } => caps[usize::from(*end)] = Some(i),
                    _ => {}
                }
            }
        }
        if caps.iter().any(Option::is_none) || sides.iter().any(Option::is_none) {
            return None;
        }
        Some(Arc::new(Self {
            plane: plane.clone(),
            lo: start.min(end),
            hi: start.max(end),
            region: region.clone(),
            sides,
            caps,
        }))
    }

    /// The prism moved by a rigid motion `m` that keeps face indices (`same_faces`); a mirror
    /// would turn the section's loops round, so it gives none.
    pub fn moved(&self, m: &nalgebra::Matrix4<Real>) -> Option<Arc<Self>> {
        let lin = m.fixed_view::<3, 3>(0, 0).into_owned();
        if lin.determinant() < 0.0 {
            return None;
        }
        let o = m.transform_point(&self.plane.origin);
        let normal = nalgebra::Unit::new_normalize(lin * self.plane.normal.into_inner());
        let x_axis = nalgebra::Unit::new_normalize(lin * self.plane.x_axis.into_inner());
        Some(Arc::new(Self {
            plane: Plane {
                origin: o,
                normal,
                x_axis,
            },
            ..self.clone()
        }))
    }
}

fn segment_count(r: &sg::Region2) -> usize {
    r.outer.segs.len() + r.holes.iter().map(|h| h.segs.len()).sum::<usize>()
}

/// A 2D box `[min, max]`.
#[derive(Clone, Copy, Debug)]
struct Box2 {
    min: [Real; 2],
    max: [Real; 2],
}

impl Box2 {
    fn grow(&mut self, x: Real, y: Real) {
        self.min[0] = self.min[0].min(x);
        self.min[1] = self.min[1].min(y);
        self.max[0] = self.max[0].max(x);
        self.max[1] = self.max[1].max(y);
    }
    fn meets(&self, o: &Box2, gap: Real) -> bool {
        self.min[0] <= o.max[0] + gap
            && o.min[0] <= self.max[0] + gap
            && self.min[1] <= o.max[1] + gap
            && o.min[1] <= self.max[1] + gap
    }
}

/// A box around a line or arc (an arc's whole circle: larger, never smaller).
fn seg_box(s: &sg::Seg2) -> Option<Box2> {
    match s {
        sg::Seg2::Line { a, b } => {
            let mut bx = Box2 {
                min: [a.x, a.y],
                max: [a.x, a.y],
            };
            bx.grow(b.x, b.y);
            Some(bx)
        }
        sg::Seg2::Arc { center, radius, .. } => Some(Box2 {
            min: [center.x - radius, center.y - radius],
            max: [center.x + radius, center.y + radius],
        }),
        _ => None,
    }
}

fn loop_box(l: &sg::Loop2) -> Option<Box2> {
    let mut it = l.segs.iter();
    let mut b = seg_box(it.next()?)?;
    for s in it {
        let o = seg_box(s)?;
        b.grow(o.min[0], o.min[1]);
        b.grow(o.max[0], o.max[1]);
    }
    Some(b)
}

/// Points along a loop (lines by their ends, arcs every 5°), for an inside test.
fn loop_points(l: &sg::Loop2) -> Option<Vec<[Real; 2]>> {
    let mut out = Vec::new();
    for s in &l.segs {
        match s {
            sg::Seg2::Line { a, .. } => out.push([a.x, a.y]),
            sg::Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let n = ((sweep.abs() / 5f64.to_radians()).ceil() as usize).max(2);
                for k in 0..n {
                    let t = start + sweep * k as Real / n as Real;
                    out.push([center.x + radius * t.cos(), center.y + radius * t.sin()]);
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

/// Whether `p` is inside the polygon `poly` (even-odd).
fn inside(poly: &[[Real; 2]], p: [Real; 2]) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            c = !c;
        }
    }
    c
}

/// The 2D map of a tool's plane coordinates into the target's: rotation `r` (2×2 columns),
/// shift `t`, and whether it mirrors (the normals stand opposite).
struct Map2 {
    r: [[Real; 2]; 2],
    t: [Real; 2],
    mirror: bool,
}

impl Map2 {
    fn new(from: &Plane, to: &Plane) -> Self {
        let (tx, ty) = (to.x_axis.into_inner(), to.y_axis().into_inner());
        let (fx, fy) = (from.x_axis.into_inner(), from.y_axis().into_inner());
        let d = from.origin - to.origin;
        Self {
            r: [[fx.dot(&tx), fx.dot(&ty)], [fy.dot(&tx), fy.dot(&ty)]],
            t: [d.dot(&tx), d.dot(&ty)],
            mirror: from.normal.dot(&to.normal) < 0.0,
        }
    }
    fn point(&self, p: sg::Vec2) -> sg::Vec2 {
        sg::Vec2::new(
            self.t[0] + self.r[0][0] * p.x + self.r[1][0] * p.y,
            self.t[1] + self.r[0][1] * p.x + self.r[1][1] * p.y,
        )
    }
    /// The angle in the target's plane of the direction at `a` in the tool's.
    fn angle(&self, a: Real) -> Real {
        let (c, s) = (a.cos(), a.sin());
        let x = self.r[0][0] * c + self.r[1][0] * s;
        let y = self.r[0][1] * c + self.r[1][1] * s;
        y.atan2(x)
    }
    fn seg(&self, s: &sg::Seg2) -> Option<sg::Seg2> {
        match s {
            sg::Seg2::Line { a, b } => Some(sg::Seg2::Line {
                a: self.point(*a),
                b: self.point(*b),
            }),
            sg::Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => Some(sg::Seg2::Arc {
                center: self.point(*center),
                radius: *radius,
                start: self.angle(*start),
                sweep: if self.mirror { -sweep } else { *sweep },
            }),
            _ => None,
        }
    }
}

/// A segment run the other way.
fn reversed(s: &sg::Seg2) -> Option<sg::Seg2> {
    match s {
        sg::Seg2::Line { a, b } => Some(sg::Seg2::Line { a: *b, b: *a }),
        sg::Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => Some(sg::Seg2::Arc {
            center: *center,
            radius: *radius,
            start: start + sweep,
            sweep: -sweep,
        }),
        _ => None,
    }
}

/// `target` minus `tools` when every tool is a prism straight through the prism `target`
/// (module docs); `Ok(None)` when the conditions do not hold.
///
/// # Errors
/// The kernel's extrusion failing.
pub(super) fn cut_through(
    target: &ExactSolid,
    tools: &[ExactSolid],
) -> Result<Option<ExactSolid>, String> {
    let Some(tp) = target.prism.as_deref() else {
        return Ok(None);
    };
    if tools.is_empty() || target.sheet {
        return Ok(None);
    }
    let n = tp.plane.normal.into_inner();
    let Some(outer_box) = loop_box(&tp.region.outer) else {
        return Ok(None);
    };
    let size = (outer_box.max[0] - outer_box.min[0])
        .max(outer_box.max[1] - outer_box.min[1])
        .max(tp.hi - tp.lo);
    // clearances: apart in the section, reaching the caps along the direction
    let gap = size * 1e-6;
    let past = size * 1e-6;
    // the tools' sections as holes of the target's, with each segment's wall name
    let mut holes: Vec<(sg::Loop2, Vec<Option<String>>, Box2)> = Vec::with_capacity(tools.len());
    for t in tools {
        let Some(p) = t.prism.as_deref() else {
            return Ok(None);
        };
        if t.sheet || !p.region.holes.is_empty() {
            return Ok(None);
        }
        let tn = p.plane.normal.into_inner();
        let along = tn.dot(&n);
        if along.abs() < 1.0 - 1e-12 {
            return Ok(None);
        }
        // the tool's caps along the target's normal
        let base = (p.plane.origin - tp.plane.origin).dot(&n);
        let (a, b) = (base + along * p.lo, base + along * p.hi);
        // flush with a cap counts as through (Through All ends on the caps)
        if a.min(b) > tp.lo + past || a.max(b) < tp.hi - past {
            return Ok(None);
        }
        let map = Map2::new(&p.plane, &tp.plane);
        let mut segs = Vec::with_capacity(p.region.outer.segs.len());
        for s in &p.region.outer.segs {
            let Some(s) = map.seg(s) else {
                return Ok(None);
            };
            segs.push(s);
        }
        let mut names: Vec<Option<String>> = p
            .sides
            .iter()
            .map(|i| i.and_then(|i| t.face_names.get(i).cloned().flatten()))
            .collect();
        // a hole runs clockwise; the tool's outline runs counter-clockwise about its normal
        if !map.mirror {
            let mut back = Vec::with_capacity(segs.len());
            for s in segs.iter().rev() {
                let Some(r) = reversed(s) else {
                    return Ok(None);
                };
                back.push(r);
            }
            segs = back;
            names.reverse();
        }
        let l = sg::Loop2 { segs };
        let Some(bx) = loop_box(&l) else {
            return Ok(None);
        };
        holes.push((l, names, bx));
    }
    // inside the outline, clear of it and of the holes already there
    let mut fixed: Vec<Box2> = Vec::new();
    for s in &tp.region.outer.segs {
        let Some(b) = seg_box(s) else {
            return Ok(None);
        };
        fixed.push(b);
    }
    for h in &tp.region.holes {
        let Some(b) = loop_box(h) else {
            return Ok(None);
        };
        fixed.push(b);
    }
    let Some(outline) = loop_points(&tp.region.outer) else {
        return Ok(None);
    };
    for (_, _, b) in &holes {
        if fixed.iter().any(|f| f.meets(b, gap)) {
            return Ok(None);
        }
        let c = [(b.min[0] + b.max[0]) / 2.0, (b.min[1] + b.max[1]) / 2.0];
        if !inside(&outline, c) {
            return Ok(None);
        }
    }
    // apart from one another (boxes, swept along x)
    let mut order: Vec<usize> = (0..holes.len()).collect();
    order.sort_by(|&i, &j| holes[i].2.min[0].total_cmp(&holes[j].2.min[0]));
    for (k, &i) in order.iter().enumerate() {
        for &j in &order[k + 1..] {
            if holes[j].2.min[0] > holes[i].2.max[0] + gap {
                break;
            }
            if holes[i].2.meets(&holes[j].2, gap) {
                return Ok(None);
            }
        }
    }
    // the new section and the name of each of its walls
    let mut region = tp.region.clone();
    let mut wall_names: Vec<Option<String>> = tp
        .sides
        .iter()
        .map(|i| i.and_then(|i| target.face_names.get(i).cloned().flatten()))
        .collect();
    for (l, names, _) in holes {
        region.holes.push(l);
        wall_names.extend(names);
    }
    let cap_names: Vec<Option<String>> = tp
        .caps
        .iter()
        .map(|i| i.and_then(|i| target.face_names.get(i).cloned().flatten()))
        .collect();
    let pl = plane_of(&tp.plane)?;
    let mut v = guarded("through cut", || {
        sk::extrude(&pl, std::slice::from_ref(&region), tp.lo, tp.hi)
    })?;
    if v.len() != 1 {
        return Ok(None);
    }
    let body = v.remove(0);
    let face_names = names_from(&body, &[], &|o| match o {
        FaceOrigin::Profile { segment } => wall_names.get(*segment).cloned().flatten(),
        FaceOrigin::Cap { end } => cap_names.get(usize::from(*end)).cloned().flatten(),
        _ => None,
    });
    let prism = Prism::of_extrusion(&body, &tp.plane, &region, tp.lo, tp.hi);
    Ok(Some(ExactSolid {
        sheet: false,
        name: target.name.clone(),
        body,
        face_names,
        profiles: target
            .profiles
            .iter()
            .chain(tools.iter().flat_map(|t| t.profiles.iter()))
            .cloned()
            .collect(),
        prism,
    }))
}
