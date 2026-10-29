//! Frame member sections (`docs/spec/6.1-frame-generator.md`, profile
//! library 6.3 L3), GUI-free: the section table (`data/profiles/profiles.csv`,
//! built in, plus a user file overriding sizes by family + size), each
//! shape's outline (lines and arcs, its bounding box centred on the origin),
//! the section properties in closed form over the outline, and the member
//! part — the section sketched on XY and extruded along +Z by the length.
//!
//! A member part holds everything it needs (rule 8): the numbers are copied
//! into its sketch, so it regenerates the same without the library.

use crate::document::iprops::{CustomProperty, IProperties, PropValue};
use crate::kernel::{Point3, Real, Vector3};
use crate::part::PlaneRef;
use crate::part::split::{SplitFeature, SplitMethod, SplitToolRef};
use crate::part::work::{UcsDef, WorkDef, WorkFeature};
use crate::part::{
    ExtrudeFeature, OriginPlane, PartDocument, PartError, ProfileRef, SketchOptions,
    SketchPlacement,
};
use crate::sketch::{Point2, Sketch, Vector2};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// The built-in table.
const BUILTIN: &str = include_str!("../../data/profiles/profiles.csv");

/// The custom property holding a member's length (Inventor's `G_L`).
pub const LENGTH_PROPERTY: &str = "G_L";

/// The kind of section: which template draws it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// Square hollow section (rounded corners).
    Shs,
    /// Rectangular hollow section.
    Rhs,
    /// Circular hollow section.
    Chs,
    /// Angle (L), root and toe radii.
    Angle,
    /// I / H beam with parallel flanges and root radii.
    IBeam,
    /// Flat bar.
    Flat,
    /// Round bar.
    Bar,
    /// Channel (U) with parallel flanges: web on the left, root and toe radii.
    Channel,
    /// T section, flange on top, stem down.
    Tee,
    /// The user's own outline (`Section::custom`, spec 6.1 §3.11).
    Custom,
}

impl Shape {
    fn parse(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "shs" => Shape::Shs,
            "rhs" => Shape::Rhs,
            "chs" => Shape::Chs,
            "angle" => Shape::Angle,
            "ibeam" => Shape::IBeam,
            "flat" => Shape::Flat,
            "bar" => Shape::Bar,
            "channel" => Shape::Channel,
            "tee" => Shape::Tee,
            "custom" => Shape::Custom,
            _ => return None,
        })
    }
}

/// One size of a family.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    /// Family (`Square hollow section (EN 10219)`).
    pub family: String,
    /// Standard (`EN`).
    pub standard: String,
    /// Which template.
    pub shape: Shape,
    /// Designation within the family (`40x40x3`, `IPE 200`).
    pub size: String,
    /// Height / outside diameter.
    pub h: Real,
    /// Width.
    pub b: Real,
    /// Wall / leg / bar thickness.
    pub t: Real,
    /// Web thickness (beams).
    pub tw: Real,
    /// Flange thickness (beams).
    pub tf: Real,
    /// Outer radius (hollow) / root radius (angle, beam).
    pub r1: Real,
    /// Inner radius (hollow) / toe radius (angle).
    pub r2: Real,
    /// The outline of a [`Shape::Custom`] section (bounding box centred on
    /// the origin); empty for the templates.
    pub custom: Vec<Loop>,
}

/// A closed loop of the outline: corners with their fillet radius (0 =
/// sharp), counter-clockwise for the outside, clockwise for a hole; or a
/// circle; or a path of lines and arcs (a user's outline).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Loop {
    /// Corners and fillet radii.
    Corners(Vec<(Point2, Real)>),
    /// A whole circle (centre, radius); `hole` = a hole.
    Circle(Point2, Real, bool),
    /// Vertices in loop order; each runs to the next by a line, or by an
    /// arc about the given centre (`true` = counter-clockwise).
    Path(Vec<(Point2, Option<(Point2, bool)>)>),
}

/// Section properties (mm units): area, centroid, second moments about
/// the centroid's axes (x horizontal, y vertical), section moduli and radii
/// of gyration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionProps {
    /// Area, mm².
    pub area: Real,
    /// Centroid in section coordinates (the bounding box is centred on 0).
    pub centroid: Point2,
    /// Second moment about the horizontal axis through the centroid, mm⁴.
    pub ix: Real,
    /// Second moment about the vertical axis through the centroid, mm⁴.
    pub iy: Real,
    /// Elastic section modulus about x, mm³ (`ix` / farthest fibre).
    pub wx: Real,
    /// Elastic section modulus about y, mm³.
    pub wy: Real,
}

impl SectionProps {
    /// Radius of gyration about x, mm.
    pub fn rx(&self) -> Real {
        (self.ix / self.area).sqrt()
    }
    /// Radius of gyration about y, mm.
    pub fn ry(&self) -> Real {
        (self.iy / self.area).sqrt()
    }
}

impl Section {
    /// The designation the BOM shows: `<size>` (with the standard's family
    /// prefix where the size has none: `SHS 40x40x3`).
    pub fn stock_number(&self) -> String {
        let prefix = match self.shape {
            Shape::Shs => "SHS ",
            Shape::Rhs => "RHS ",
            Shape::Chs => "CHS ",
            Shape::Flat => "FL ",
            Shape::Bar => "RD ",
            Shape::Angle | Shape::IBeam | Shape::Channel | Shape::Tee | Shape::Custom => "",
        };
        format!("{prefix}{}", self.size)
    }

    /// Width and height of the bounding box (section x, y).
    pub fn extent(&self) -> (Real, Real) {
        match self.shape {
            Shape::Chs | Shape::Bar => (self.h, self.h),
            Shape::Flat => (self.b, self.t),
            _ => (self.b, self.h),
        }
    }

    /// The outline, bounding box centred on the origin.
    pub fn loops(&self) -> Vec<Loop> {
        let (w, h) = self.extent();
        let (x0, y0, x1, y1) = (-w / 2.0, -h / 2.0, w / 2.0, h / 2.0);
        let p = Point2::new;
        let rect = |x0: Real, y0: Real, x1: Real, y1: Real, r: Real, hole: bool| {
            let mut c = vec![
                (p(x0, y0), r),
                (p(x1, y0), r),
                (p(x1, y1), r),
                (p(x0, y1), r),
            ];
            if hole {
                c.reverse();
            }
            Loop::Corners(c)
        };
        match self.shape {
            Shape::Shs | Shape::Rhs => {
                let t = self.t;
                vec![
                    rect(x0, y0, x1, y1, self.r1, false),
                    rect(x0 + t, y0 + t, x1 - t, y1 - t, self.r2, true),
                ]
            }
            Shape::Chs => vec![
                Loop::Circle(Point2::origin(), self.h / 2.0, false),
                Loop::Circle(Point2::origin(), self.h / 2.0 - self.t, true),
            ],
            Shape::Bar => vec![Loop::Circle(Point2::origin(), self.h / 2.0, false)],
            Shape::Flat => vec![rect(x0, y0, x1, y1, 0.0, false)],
            Shape::Angle => {
                // Heel at the bottom left, legs along +x and +y.
                let t = self.t;
                vec![Loop::Corners(vec![
                    (p(x0, y0), 0.0),
                    (p(x1, y0), 0.0),
                    (p(x1, y0 + t), self.r2),
                    (p(x0 + t, y0 + t), self.r1),
                    (p(x0 + t, y1), self.r2),
                    (p(x0, y1), 0.0),
                ])]
            }
            Shape::Channel => {
                let (tw, tf) = (self.tw, self.tf);
                vec![Loop::Corners(vec![
                    (p(x0, y0), 0.0),
                    (p(x1, y0), 0.0),
                    (p(x1, y0 + tf), self.r2),
                    (p(x0 + tw, y0 + tf), self.r1),
                    (p(x0 + tw, y1 - tf), self.r1),
                    (p(x1, y1 - tf), self.r2),
                    (p(x1, y1), 0.0),
                    (p(x0, y1), 0.0),
                ])]
            }
            Shape::Tee => {
                let tw = self.tee_web() / 2.0;
                let tf = self.tee_flange();
                vec![Loop::Corners(vec![
                    (p(-tw, y0), 0.0),
                    (p(tw, y0), 0.0),
                    (p(tw, y1 - tf), self.r1),
                    (p(x1, y1 - tf), self.r2),
                    (p(x1, y1), 0.0),
                    (p(x0, y1), 0.0),
                    (p(x0, y1 - tf), self.r2),
                    (p(-tw, y1 - tf), self.r1),
                ])]
            }
            Shape::Custom => self.custom.clone(),
            Shape::IBeam => {
                let (tw, tf, r) = (self.tw / 2.0, self.tf, self.r1);
                vec![Loop::Corners(vec![
                    (p(x0, y0), 0.0),
                    (p(x1, y0), 0.0),
                    (p(x1, y0 + tf), 0.0),
                    (p(tw, y0 + tf), r),
                    (p(tw, y1 - tf), r),
                    (p(x1, y1 - tf), 0.0),
                    (p(x1, y1), 0.0),
                    (p(x0, y1), 0.0),
                    (p(x0, y1 - tf), 0.0),
                    (p(-tw, y1 - tf), r),
                    (p(-tw, y0 + tf), r),
                    (p(x0, y0 + tf), 0.0),
                ])]
            }
        }
    }

    /// The outer outline as points (arcs sampled), section coordinates.
    pub fn outline(&self) -> Vec<Point2> {
        self.loops()
            .first()
            .map(|l| polygon(l, 96))
            .unwrap_or_default()
    }

    /// A point inside the material (to choose the profile region).
    pub fn material_point(&self) -> Point2 {
        let (w, _) = self.extent();
        match self.shape {
            Shape::Shs | Shape::Rhs => Point2::new(w / 2.0 - self.t / 2.0, 0.0),
            Shape::Chs => Point2::new(self.h / 2.0 - self.t / 2.0, 0.0),
            Shape::Angle => Point2::new(-w / 2.0 + self.t / 2.0, 0.0),
            Shape::IBeam => Point2::new(0.0, 0.0),
            Shape::Flat | Shape::Bar => Point2::origin(),
            Shape::Channel => Point2::new(-w / 2.0 + self.tw / 2.0, 0.0),
            Shape::Tee => Point2::new(0.0, self.extent().1 / 2.0 - self.tee_flange() / 2.0),
            Shape::Custom => self.inside_point().unwrap_or_else(Point2::origin),
        }
    }

    /// A T section's stem thickness (EN 10055 gives one `t` for both).
    fn tee_web(&self) -> Real {
        if self.tw > 0.0 { self.tw } else { self.t }
    }

    /// A T section's flange thickness.
    fn tee_flange(&self) -> Real {
        if self.tf > 0.0 { self.tf } else { self.t }
    }

    /// A point inside a custom outline's material (outside its holes): the
    /// first of a fine grid over the bounding box.
    fn inside_point(&self) -> Option<Point2> {
        let polys: Vec<Vec<Point2>> = self.custom.iter().map(|l| polygon(l, 360)).collect();
        let (w, h) = self.extent();
        let n = 64;
        for j in 0..n {
            for i in 0..n {
                let q = Point2::new(
                    -w / 2.0 + w * (i as Real + 0.5) / n as Real,
                    -h / 2.0 + h * (j as Real + 0.5) / n as Real,
                );
                // Inside an odd number of loops = in the material.
                let inside = polys.iter().filter(|p| contains(p, q)).count() % 2 == 1;
                let clear = polys.iter().all(|p| edge_distance(p, q) > 1e-3 * (w + h));
                if inside && clear {
                    return Some(q);
                }
            }
        }
        None
    }

    /// A section of the user's own outline (spec 6.1 §3.11): a closed
    /// region of a sketch (lines, arcs, circles; holes allowed), moved so
    /// its bounding box is centred on the origin.
    pub fn custom(
        standard: &str,
        family: &str,
        size: &str,
        region: &crate::sketch::Region,
    ) -> Result<Section, String> {
        use crate::sketch::ProfileCurve;
        let to_loop = |l: &crate::sketch::ProfileLoop, hole: bool| -> Result<Loop, String> {
            if let [e] = l.edges.as_slice()
                && let ProfileCurve::Arc { c, r, .. } = e.curve
            {
                return Ok(Loop::Circle(c, r, hole));
            }
            l.edges
                .iter()
                .map(|e| match &e.curve {
                    ProfileCurve::Line { a, .. } => Ok((*a, None)),
                    ProfileCurve::Arc { c, sweep, .. } => {
                        Ok((e.curve.point(0.0), Some((*c, *sweep > 0.0))))
                    }
                    ProfileCurve::Spline { .. } => {
                        Err("splines cannot be part of a frame profile yet".to_owned())
                    }
                })
                .collect::<Result<Vec<_>, String>>()
                .map(Loop::Path)
        };
        if family.trim().is_empty() || size.trim().is_empty() {
            return Err("give the profile a family and a size".into());
        }
        let mut loops = vec![to_loop(&region.outer, false)?];
        for h in &region.holes {
            loops.push(to_loop(h, true)?);
        }
        let pts = polygon(&loops[0], 360);
        let mut lo = Point2::new(Real::MAX, Real::MAX);
        let mut hi = Point2::new(Real::MIN, Real::MIN);
        for q in &pts {
            lo = Point2::new(lo.x.min(q.x), lo.y.min(q.y));
            hi = Point2::new(hi.x.max(q.x), hi.y.max(q.y));
        }
        let (b, h) = (hi.x - lo.x, hi.y - lo.y);
        if !(b > 1e-6 && h > 1e-6) {
            return Err("the region has no size".into());
        }
        let shift = Vector2::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
        let mv = |q: Point2| q - shift;
        let loops = loops
            .into_iter()
            .map(|l| match l {
                Loop::Circle(c, r, hole) => Loop::Circle(mv(c), r, hole),
                Loop::Corners(v) => Loop::Corners(v.into_iter().map(|(q, r)| (mv(q), r)).collect()),
                Loop::Path(v) => Loop::Path(
                    v.into_iter()
                        .map(|(q, a)| (mv(q), a.map(|(c, ccw)| (mv(c), ccw))))
                        .collect(),
                ),
            })
            .collect();
        Ok(Section {
            family: family.trim().to_owned(),
            standard: standard.trim().to_owned(),
            shape: Shape::Custom,
            size: size.trim().to_owned(),
            h,
            b,
            t: 0.0,
            tw: 0.0,
            tf: 0.0,
            r1: 0.0,
            r2: 0.0,
            custom: loops,
        })
    }

    /// The section properties, from the outline (arcs integrated exactly
    /// enough by fine sampling).
    pub fn properties(&self) -> SectionProps {
        // Green's theorem over each loop's polygon (holes run clockwise, so
        // their terms subtract).
        let (mut a, mut sx, mut sy, mut sxx, mut syy) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for l in self.loops() {
            let pts = polygon(&l, 2000);
            for i in 0..pts.len() {
                let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
                let c = p.x * q.y - q.x * p.y;
                a += c / 2.0;
                sx += (p.x + q.x) * c / 6.0;
                sy += (p.y + q.y) * c / 6.0;
                // ∫ y² dA and ∫ x² dA.
                syy += (p.y * p.y + p.y * q.y + q.y * q.y) * c / 12.0;
                sxx += (p.x * p.x + p.x * q.x + q.x * q.x) * c / 12.0;
            }
        }
        let c = Point2::new(sx / a, sy / a);
        let ix = syy - a * c.y * c.y;
        let iy = sxx - a * c.x * c.x;
        let (w, h) = self.extent();
        let ymax = (h / 2.0 - c.y).max(h / 2.0 + c.y);
        let xmax = (w / 2.0 - c.x).max(w / 2.0 + c.x);
        SectionProps {
            area: a,
            centroid: c,
            ix,
            iy,
            wx: ix / ymax,
            wy: iy / xmax,
        }
    }

    /// Draws the outline into `sk` (sketch coordinates = section
    /// coordinates). Corners with a radius become tangent arcs; points are
    /// shared along each loop.
    pub fn draw(&self, sk: &mut Sketch) -> Result<(), PartError> {
        let err = |e: crate::sketch::SketchError| PartError::Feature(e.to_string());
        for l in self.loops() {
            match l {
                Loop::Circle(c, r, _) => {
                    let _ = sk.add_circle(c, r).map_err(err)?;
                }
                Loop::Path(v) => {
                    let ids: Vec<_> = v.iter().map(|(q, _)| sk.add_point(*q)).collect();
                    let n = v.len();
                    for i in 0..n {
                        let (a, b) = (ids[i], ids[(i + 1) % n]);
                        match v[i].1 {
                            Some((c, ccw)) => {
                                let c = sk.add_point(c);
                                if ccw {
                                    sk.add_arc(c, a, b).map_err(err)?;
                                } else {
                                    sk.add_arc(c, b, a).map_err(err)?;
                                }
                            }
                            None => {
                                sk.add_line_between(a, b).map_err(err)?;
                            }
                        }
                    }
                }
                Loop::Corners(corners) => {
                    // Each corner: where it starts and ends (tangent points),
                    // and its arc centre when rounded.
                    let n = corners.len();
                    let mut ends = Vec::with_capacity(n);
                    for i in 0..n {
                        let (prev, (p, r), next) = (
                            corners[(i + n - 1) % n].0,
                            corners[i],
                            corners[(i + 1) % n].0,
                        );
                        ends.push(fillet(prev, p, next, r));
                    }
                    let ids: Vec<_> = ends
                        .iter()
                        .map(|e| match e {
                            Corner::Sharp(p) => {
                                let id = sk.add_point(*p);
                                (id, id, None)
                            }
                            Corner::Round { a, b, c, ccw } => {
                                let (ia, ib, ic) =
                                    (sk.add_point(*a), sk.add_point(*b), sk.add_point(*c));
                                (ia, ib, Some((ic, *ccw)))
                            }
                        })
                        .collect();
                    for i in 0..n {
                        let (a, b, arc) = ids[i];
                        if let Some((c, ccw)) = arc {
                            if ccw {
                                sk.add_arc(c, a, b).map_err(err)?;
                            } else {
                                sk.add_arc(c, b, a).map_err(err)?;
                            }
                        }
                        let next = ids[(i + 1) % n].0;
                        sk.add_line_between(b, next).map_err(err)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// A member part: this section extruded by `length` along +Z from its
    /// XY plane, with the member's iProperties (Stock Number, Description,
    /// `G_L`) and `material` when the library knows it.
    pub fn member_part(
        &self,
        length: Real,
        material: Option<&str>,
    ) -> Result<PartDocument, PartError> {
        self.member_part_cut(length, [0.0, 0.0], &[], material)
    }

    /// A member part with end treatments: the section extruded from
    /// `-ext[0]` to `length + ext[1]` along +Z (past the skeleton's ends,
    /// so a cut has material to take), then each plane of `cuts` trims what
    /// lies on its normal's side (a work plane + Trim Solid per cut, so the
    /// part keeps its cuts without the frame — rule 8).
    pub fn member_part_cut(
        &self,
        length: Real,
        ext: [Real; 2],
        cuts: &[EndCut],
        material: Option<&str>,
    ) -> Result<PartDocument, PartError> {
        if !(length.is_finite() && length > 1e-6) {
            return Err(PartError::Feature("the member has no length".into()));
        }
        let mut part = PartDocument::default();
        let opts = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let i = part.create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)?;
        self.draw(&mut part.sketches[i].sketch)?;
        let seed = self.material_point();
        let regions = part.sketches[i].sketch.regions();
        let r = regions
            .iter()
            .find(|r| r.contains(seed))
            .ok_or_else(|| PartError::Feature("the section has no closed profile".into()))?;
        let mut e = ExtrudeFeature::new(vec![ProfileRef::of(r, seed)], length + ext[1]);
        if ext[0] > 1e-9 {
            e.direction = crate::part::ExtentDirection::Asymmetric;
            e.distance_b = ext[0];
        }
        part.add_extrude(i, e)?;
        for c in cuts {
            let n = c
                .normal
                .try_normalize(1e-12)
                .ok_or_else(|| PartError::Feature("an end cut has no direction".into()))?;
            // A UCS whose XY plane is the cut (its Z = the normal: Ry·Rx·ẑ).
            let w = part.add_work(WorkFeature::new(WorkDef::Ucs(UcsDef::Offsets {
                x: c.point.x,
                y: c.point.y,
                z: c.point.z,
                rx: (-n.y).clamp(-1.0, 1.0).asin(),
                ry: n.x.atan2(n.z),
                rz: 0.0,
            })))?;
            let mut sp = SplitFeature::new(SplitMethod::TrimSolid);
            sp.tool = Some(SplitToolRef::Plane(PlaneRef::Work(format!("{w}:XY"))));
            part.add_split(sp)?;
        }
        if let Some(m) = material {
            let _ = part.set_material(Some(m), crate::materials::builtin());
        }
        Ok(part)
    }

    /// The iProperties a member of this section and `length` carries.
    pub fn member_properties(&self, length: Real) -> IProperties {
        let mut p = IProperties::default();
        p.set("stock_number", &self.stock_number());
        p.set("description", &format!("{} {}", self.family, self.size));
        p.custom.push(CustomProperty {
            name: LENGTH_PROPERTY.into(),
            value: PropValue::Number(length),
        });
        p
    }
}

/// A planar end cut of a member, in its part's coordinates: the material
/// on the side `normal` points to is removed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EndCut {
    /// A point of the plane.
    pub point: Point3,
    /// Towards the removed side.
    pub normal: Vector3,
}

/// A corner of an outline after filleting.
#[derive(Clone, Copy, Debug)]
enum Corner {
    Sharp(Point2),
    /// From `a` to `b` round centre `c`; `ccw` = the arc runs
    /// counter-clockwise from `a` to `b` (a convex corner of a CCW loop).
    Round {
        a: Point2,
        b: Point2,
        c: Point2,
        ccw: bool,
    },
}

/// The fillet of radius `r` at corner `p` between the edges from `prev`
/// and to `next`.
fn fillet(prev: Point2, p: Point2, next: Point2, r: Real) -> Corner {
    let (u, v) = ((prev - p).normalize(), (next - p).normalize());
    let cos = u.dot(&v).clamp(-1.0, 1.0);
    let half = cos.acos() / 2.0;
    if r <= 1e-9 || half < 1e-6 {
        return Corner::Sharp(p);
    }
    let along = r / half.tan();
    let a = p + u * along;
    let b = p + v * along;
    let bis: Vector2 = (u + v).normalize();
    let c = p + bis * (r / half.sin());
    // Turning left (CCW) from the incoming edge to the outgoing one.
    let din = p - prev;
    let left = din.perp(&(next - p)) > 0.0;
    Corner::Round { a, b, c, ccw: left }
}

/// The loop as a polygon (arcs in `per_turn` steps a full turn), in the
/// loop's own direction.
fn polygon(l: &Loop, per_turn: usize) -> Vec<Point2> {
    use std::f64::consts::TAU;
    match l {
        Loop::Circle(c, r, hole) => {
            let mut v: Vec<Point2> = (0..per_turn)
                .map(|k| {
                    let t = TAU * k as Real / per_turn as Real;
                    c + Vector2::new(t.cos(), t.sin()) * *r
                })
                .collect();
            if *hole {
                v.reverse();
            }
            v
        }
        Loop::Corners(corners) => {
            let n = corners.len();
            let mut out = Vec::new();
            for i in 0..n {
                let (prev, (p, r), next) = (
                    corners[(i + n - 1) % n].0,
                    corners[i],
                    corners[(i + 1) % n].0,
                );
                match fillet(prev, p, next, r) {
                    Corner::Sharp(q) => out.push(q),
                    Corner::Round { a, b, c, ccw } => {
                        let rad = (a - c).norm();
                        let a0 = (a.y - c.y).atan2(a.x - c.x);
                        let a1 = (b.y - c.y).atan2(b.x - c.x);
                        let mut sweep = a1 - a0;
                        if ccw && sweep < 0.0 {
                            sweep += TAU;
                        }
                        if !ccw && sweep > 0.0 {
                            sweep -= TAU;
                        }
                        let steps =
                            ((sweep.abs() / TAU) * per_turn as Real).ceil().max(2.0) as usize;
                        for k in 0..=steps {
                            let t = a0 + sweep * k as Real / steps as Real;
                            out.push(c + Vector2::new(t.cos(), t.sin()) * rad);
                        }
                    }
                }
            }
            out
        }
        Loop::Path(v) => {
            let n = v.len();
            let mut out = Vec::new();
            for i in 0..n {
                let (a, arc) = v[i];
                out.push(a);
                if let Some((c, ccw)) = arc {
                    let b = v[(i + 1) % n].0;
                    let rad = (a - c).norm();
                    let a0 = (a.y - c.y).atan2(a.x - c.x);
                    let a1 = (b.y - c.y).atan2(b.x - c.x);
                    let mut sweep = a1 - a0;
                    if ccw && sweep <= 0.0 {
                        sweep += TAU;
                    }
                    if !ccw && sweep >= 0.0 {
                        sweep -= TAU;
                    }
                    let steps = ((sweep.abs() / TAU) * per_turn as Real).ceil().max(2.0) as usize;
                    for k in 1..steps {
                        let t = a0 + sweep * k as Real / steps as Real;
                        out.push(c + Vector2::new(t.cos(), t.sin()) * rad);
                    }
                }
            }
            out
        }
    }
}

/// Is `q` inside the closed polygon `p` (even-odd)?
fn contains(p: &[Point2], q: Point2) -> bool {
    let mut inside = false;
    let n = p.len();
    for i in 0..n {
        let (a, b) = (p[i], p[(i + 1) % n]);
        if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// Distance from `q` to the nearest edge of the closed polygon `p`.
fn edge_distance(p: &[Point2], q: Point2) -> Real {
    let n = p.len();
    (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            let d = b - a;
            let t = ((q - a).dot(&d) / d.norm_squared().max(1e-300)).clamp(0.0, 1.0);
            (a + d * t - q).norm()
        })
        .fold(Real::MAX, Real::min)
}

/// The user's profile folder (`%APPDATA%\3dcadcraft\profiles`; tests and
/// scripted runs move it with `CADCRAFT3D_PROFILES_DIR`).
pub fn user_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("CADCRAFT3D_PROFILES_DIR") {
        return Some(PathBuf::from(d));
    }
    crate::materials::user_dir().and_then(|d| d.parent().map(|p| p.join("profiles")))
}

/// One family of the user's own outlines, as stored in its JSON file.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct UserFamily {
    standard: String,
    family: String,
    sizes: Vec<UserSize>,
}

/// One size of a [`UserFamily`].
#[derive(Clone, Debug, Serialize, Deserialize)]
struct UserSize {
    size: String,
    width: Real,
    height: Real,
    loops: Vec<Loop>,
}

/// The library the commands use: built in + the user's folder, read once
/// and again on [`Library::reload`].
static CURRENT: RwLock<Option<Arc<Library>>> = RwLock::new(None);

/// The section table: built in, then a user file's sizes over it.
#[derive(Clone, Debug, Default)]
pub struct Library {
    /// Every size, in file order.
    pub sections: Vec<Section>,
}

impl Library {
    /// The built-in table with the user's folder over it (spec 6.1
    /// §3.11): what Insert Frame and Change list.
    pub fn current() -> Arc<Library> {
        if let Ok(g) = CURRENT.read()
            && let Some(l) = g.as_ref()
        {
            return l.clone();
        }
        Library::reload().0
    }

    /// Reads the user's folder again (Refresh Categories); returns the new
    /// library and what could not be read.
    pub fn reload() -> (Arc<Library>, Vec<String>) {
        let (lib, bad) = match user_dir() {
            Some(d) => Library::with_user(&d),
            None => (Library::builtin().clone(), Vec::new()),
        };
        let lib = Arc::new(lib);
        if let Ok(mut g) = CURRENT.write() {
            *g = Some(lib.clone());
        }
        (lib, bad)
    }

    /// The built-in table, then every `*.csv` (the built-in columns) and
    /// `*.json` (outlines saved by Author Frame Profile) of folder `dir`;
    /// the same family + size replaces a size. Returns what could not be
    /// read (`file: line`).
    pub fn with_user(dir: &Path) -> (Library, Vec<String>) {
        let mut lib = Library::builtin().clone();
        let mut bad = Vec::new();
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|r| r.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default();
        files.sort();
        for f in files {
            let name = f
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let Ok(text) = std::fs::read_to_string(&f) else {
                continue;
            };
            match f.extension().and_then(|e| e.to_str()) {
                Some("csv") => {
                    let (l, b) = Library::parse(&text);
                    for s in l.sections.into_iter().filter(|s| s.shape != Shape::Custom) {
                        lib.add(s);
                    }
                    bad.extend(b.into_iter().map(|line| format!("{name}: {line}")));
                }
                Some("json") => match serde_json::from_str::<UserFamily>(&text) {
                    Ok(u) => {
                        for z in u.sizes {
                            lib.add(Section {
                                family: u.family.clone(),
                                standard: u.standard.clone(),
                                shape: Shape::Custom,
                                size: z.size,
                                h: z.height,
                                b: z.width,
                                t: 0.0,
                                tw: 0.0,
                                tf: 0.0,
                                r1: 0.0,
                                r2: 0.0,
                                custom: z.loops,
                            });
                        }
                    }
                    Err(e) => bad.push(format!("{name}: {e}")),
                },
                _ => {}
            }
        }
        (lib, bad)
    }

    /// Saves custom section `s` into its family's JSON file in `dir` (the
    /// same size replaced); returns the file.
    pub fn save_custom(dir: &Path, s: &Section) -> std::io::Result<PathBuf> {
        let safe: String = s
            .family
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{safe}.json"));
        let mut fam = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<UserFamily>(&t).ok())
            .unwrap_or_default();
        fam.standard = s.standard.clone();
        fam.family = s.family.clone();
        fam.sizes.retain(|z| z.size != s.size);
        fam.sizes.push(UserSize {
            size: s.size.clone(),
            width: s.b,
            height: s.h,
            loops: s.custom.clone(),
        });
        let text = serde_json::to_string_pretty(&fam).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &path)?;
        Ok(path)
    }

    /// The built-in table.
    pub fn builtin() -> &'static Library {
        static LIB: std::sync::OnceLock<Library> = std::sync::OnceLock::new();
        LIB.get_or_init(|| Library::parse(BUILTIN).0)
    }

    /// Reads a table; returns it and the lines it could not read.
    pub fn parse(text: &str) -> (Library, Vec<String>) {
        let mut lib = Library::default();
        let mut bad = Vec::new();
        let num = |s: Option<&str>| -> Real {
            s.map(str::trim)
                .filter(|s| !s.is_empty())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0)
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("family,") {
                continue;
            }
            let f: Vec<&str> = line.split(',').collect();
            let Some(shape) = f.get(2).and_then(|s| Shape::parse(s)) else {
                bad.push(line.to_owned());
                continue;
            };
            lib.add(Section {
                family: f[0].trim().to_owned(),
                standard: f.get(1).map_or("", |s| s.trim()).to_owned(),
                shape,
                size: f.get(3).map_or("", |s| s.trim()).to_owned(),
                h: num(f.get(4).copied()),
                b: num(f.get(5).copied()),
                t: num(f.get(6).copied()),
                tw: num(f.get(7).copied()),
                tf: num(f.get(8).copied()),
                r1: num(f.get(9).copied()),
                r2: num(f.get(10).copied()),
                custom: Vec::new(),
            });
        }
        (lib, bad)
    }

    /// Adds a size, replacing one of the same family and size.
    pub fn add(&mut self, s: Section) {
        match self
            .sections
            .iter_mut()
            .find(|x| x.family == s.family && x.size == s.size)
        {
            Some(x) => *x = s,
            None => self.sections.push(s),
        }
    }

    /// The standards, in table order.
    pub fn standards(&self) -> Vec<&str> {
        let mut v: Vec<&str> = Vec::new();
        for s in &self.sections {
            if !v.contains(&s.standard.as_str()) {
                v.push(&s.standard);
            }
        }
        v
    }

    /// The families of `standard`, in table order.
    pub fn families(&self, standard: &str) -> Vec<&str> {
        let mut v: Vec<&str> = Vec::new();
        for s in self.sections.iter().filter(|s| s.standard == standard) {
            if !v.contains(&s.family.as_str()) {
                v.push(&s.family);
            }
        }
        v
    }

    /// The sizes of `family`.
    pub fn sizes(&self, family: &str) -> Vec<&str> {
        self.sections
            .iter()
            .filter(|s| s.family == family)
            .map(|s| s.size.as_str())
            .collect()
    }

    /// One size.
    pub fn find(&self, family: &str, size: &str) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| s.family == family && s.size == size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sec(family: &str, size: &str) -> Section {
        Library::builtin()
            .find(family, size)
            .unwrap_or_else(|| panic!("{family} {size}"))
            .clone()
    }

    fn close(got: Real, want: Real, rel: Real, what: &str) {
        assert!(
            (got - want).abs() <= rel * want.abs(),
            "{what}: {got} vs {want}"
        );
    }

    #[test]
    fn the_table_reads_every_line() {
        let (lib, bad) = Library::parse(BUILTIN);
        assert!(bad.is_empty(), "{bad:?}");
        assert!(lib.sections.len() > 50);
        assert_eq!(lib.standards(), vec!["EN"]);
        assert!(lib.families("EN").contains(&"IPE beam (EN 10365)"));
        assert!(lib.sizes("IPE beam (EN 10365)").contains(&"IPE 200"));
    }

    /// Areas and second moments against the standards' tables.
    #[test]
    fn section_properties_match_the_standards_tables() {
        let ipe = sec("IPE beam (EN 10365)", "IPE 200").properties();
        close(ipe.area, 2848.0, 0.002, "IPE 200 A");
        close(ipe.ix, 1943e4, 0.003, "IPE 200 Iy (strong axis)");
        close(ipe.iy, 142.4e4, 0.005, "IPE 200 Iz (weak axis)");
        close(ipe.wx, 194.3e3, 0.003, "IPE 200 Wel,y");
        let shs = sec("Square hollow section (EN 10219)", "40x40x3").properties();
        close(shs.area, 421.0, 0.003, "SHS 40x40x3 A");
        close(shs.ix, 9.32e4, 0.01, "SHS 40x40x3 I");
        let l = sec("Equal angle (EN 10056-1)", "L 50x50x5").properties();
        close(l.area, 480.0, 0.003, "L 50x50x5 A");
        close(l.ix, 11.0e4, 0.01, "L 50x50x5 Iy");
        // Centroid 14.0 mm from the heel (e).
        close(l.centroid.y + 25.0, 14.0, 0.01, "L 50x50x5 e");
        let chs = sec("Circular hollow section (EN 10219)", "48.3x3.2").properties();
        close(chs.area, 453.4, 0.003, "CHS 48.3x3.2 A");
        let fl = sec("Flat bar (EN 10058)", "40x5").properties();
        close(fl.area, 200.0, 1e-9, "FL 40x5 A");
        close(fl.ix, 40.0 * 125.0 / 12.0, 1e-9, "FL 40x5 Ix");
    }

    /// A member is the section times its length; the sketch has one region
    /// of material (a tube's inside is not in it).
    #[test]
    fn a_member_part_is_the_section_extruded_by_its_length() {
        for (f, s) in [
            ("Square hollow section (EN 10219)", "40x40x3"),
            ("IPE beam (EN 10365)", "IPE 200"),
            ("Equal angle (EN 10056-1)", "L 50x50x5"),
            ("Circular hollow section (EN 10219)", "48.3x3.2"),
            ("Round bar (EN 10060)", "D20"),
            ("UPE channel (EN 10365)", "UPE 120"),
            ("T section (EN 10055)", "T 50"),
            ("Unequal angle (EN 10056-1)", "L 60x40x5"),
        ] {
            let sec = sec(f, s);
            let part = sec.member_part(500.0, Some("Steel S235")).unwrap();
            assert!(
                part.feature_health.is_empty(),
                "{s}: {:?}",
                part.feature_health
            );
            let v: Real = part
                .exact_bodies
                .0
                .iter()
                .filter_map(|b| b.volume_at(1e-3))
                .sum();
            close(v, sec.properties().area * 500.0, 0.003, s);
            assert_eq!(part.material.as_deref(), Some("Steel S235"));
        }
    }

    /// A 45° cut at each end of a flat bar (a mitred piece of a square
    /// frame corner): the part is the trapezoid between the cut planes.
    #[test]
    fn end_cuts_trim_the_member() {
        let s = sec("Flat bar (EN 10058)", "40x5");
        // Section x across 40 mm; cuts in the x-z plane at 45°: the
        // outer edge (x = +20) is 500 + 2·20 long, the inner one 500 − 2·20.
        let r = std::f64::consts::FRAC_1_SQRT_2;
        let cuts = [
            EndCut {
                point: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(r, 0.0, -r),
            },
            EndCut {
                point: Point3::new(0.0, 0.0, 500.0),
                normal: Vector3::new(r, 0.0, r),
            },
        ];
        let part = s.member_part_cut(500.0, [30.0, 30.0], &cuts, None).unwrap();
        assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
        let v: Real = part
            .exact_bodies
            .0
            .iter()
            .filter_map(|b| b.volume_at(1e-3))
            .sum();
        // Mean length 500 (the trapezoid's mid-line) × 40 × 5.
        close(v, 500.0 * 200.0, 0.002, "mitred flat");
        let (lo, hi) = part.body.bounding_box().unwrap();
        close(hi.z - lo.z, 540.0, 1e-3, "outer length");
    }

    #[test]
    fn a_member_without_length_is_refused() {
        let s = sec("Flat bar (EN 10058)", "40x5");
        assert!(s.member_part(0.0, None).is_err());
    }

    /// The added families against the standards' tables (areas, cm²).
    #[test]
    fn channels_tees_and_unequal_angles_match_their_tables() {
        for (f, s, a) in [
            ("UPE channel (EN 10365)", "UPE 200", 2900.0),
            ("HEM beam (EN 10365)", "HEM 200", 13130.0),
            ("T section (EN 10055)", "T 50", 566.0),
            ("Unequal angle (EN 10056-1)", "L 60x40x5", 479.0),
        ] {
            close(sec(f, s).properties().area, a, 0.02, s);
        }
        // An unequal angle's centroid lies towards the long leg's side.
        let p = sec("Unequal angle (EN 10056-1)", "L 60x40x5").properties();
        assert!(p.centroid.x < 0.0 && p.centroid.y < 0.0);
    }

    /// A D-shaped outline (a 40 mm line closed by a half circle) with a
    /// Ø10 hole as the user's profile: area, member volume, save / load.
    #[test]
    fn a_profile_from_a_sketch_region() {
        use crate::sketch::Sketch;
        let mut sk = Sketch::new(crate::kernel::Plane::xy());
        let a = sk.add_point(Point2::new(100.0, 0.0));
        let b = sk.add_point(Point2::new(140.0, 0.0));
        let c = sk.add_point(Point2::new(120.0, 0.0));
        sk.add_line_between(a, b).unwrap();
        sk.add_arc(c, b, a).unwrap();
        sk.add_circle(Point2::new(120.0, 8.0), 5.0).unwrap();
        let regions = sk.regions();
        let r = regions
            .iter()
            .find(|r| r.contains(Point2::new(120.0, 2.0)))
            .unwrap();
        let s = Section::custom("Own", "D profile", "D40", r).unwrap();
        assert_eq!(s.shape, Shape::Custom);
        assert!((s.b - 40.0).abs() < 1e-6 && (s.h - 20.0).abs() < 1e-6);
        let want = std::f64::consts::PI * (400.0 / 2.0 - 25.0);
        close(s.properties().area, want, 0.002, "D40");
        let part = s.member_part(100.0, None).unwrap();
        let v: Real = part
            .exact_bodies
            .0
            .iter()
            .filter_map(|b| b.volume_at(1e-3))
            .sum();
        close(v, want * 100.0, 0.005, "D40 volume");
        // Saved into the user folder and read back with the built-in table.
        let dir = std::env::temp_dir().join(format!("cc3d-profiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Library::save_custom(&dir, &s).unwrap();
        std::fs::write(
            dir.join("mine.csv"),
            "family,standard,shape,size,h,b,t,tw,tf,r1,r2
Flat bar (EN 10058),EN,flat,45x7,,45,7,,,,
",
        )
        .unwrap();
        let (lib, bad) = Library::with_user(&dir);
        assert!(bad.is_empty(), "{bad:?}");
        assert!(lib.standards().contains(&"Own"));
        let back = lib.find("D profile", "D40").unwrap();
        assert_eq!(back, &s);
        assert!(lib.find("Flat bar (EN 10058)", "45x7").is_some());
        assert!(lib.find("IPE beam (EN 10365)", "IPE 200").is_some());
        // A profile without a family is refused.
        assert!(Section::custom("Own", "", "x", r).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
