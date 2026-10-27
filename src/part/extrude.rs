//! The **Extrude** feature (Inventor: 3D Model → Create → Extrude).
//!
//! An extrusion references a sketch (by its immutable `id`) and one or more
//! of its closed regions. A region is remembered by the sketch curves of
//! its outer boundary ([`ProfileRef::sources`]) — that survives dimension
//! edits — with a point inside it ([`ProfileRef::seed`]) as fallback when
//! the boundary curves changed (e.g. a line was split).
//!
//! The full behaviour (Inventor's options and our status) is specified in
//! `docs/spec/2.1-extrude.md`. Until Boolean operations exist (FEATURES
//! 2.13) every extrusion is a **new solid** of the part's body.

use serde::{Deserialize, Serialize};
use slotmap::Key;

use crate::kernel::{
    BRep, Cylinder, ExtrudeLevel, Plane, Point3, ProfileSegment, Real, Segment2, Surface, Vector3,
};
use crate::sketch::{EntityId, Point2, ProfileCurve, ProfileLoop, Region, Sketch};

use super::placement::{PlaneRef, PointRef, WorkGeom};

type Vector2 = nalgebra::Vector2<Real>;

/// Which way an extrusion goes from its sketch plane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtentDirection {
    /// Along the sketch normal.
    #[default]
    Default,
    /// Against the sketch normal.
    Flipped,
    /// Half the distance to each side.
    Symmetric,
    /// Distance A along the normal, distance B against it.
    Asymmetric,
}

/// Where an extrusion ends (Inventor's extents).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Extent {
    /// At a distance (A, and B for Asymmetric).
    #[default]
    Distance,
    /// At a face, work plane or vertex (with `from`: Between).
    To(TargetRef),
    /// At the next face of the body, in the direction, that covers the
    /// whole profile.
    ToNext,
    /// Through all of the body in the direction.
    ThroughAll,
}

/// Model geometry an extent refers to, by persistent name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetRef {
    /// A face: planar (parallel or inclined to the sketch) or cylindrical.
    Face(String),
    /// A vertex: the termination plane through it, parallel to the sketch.
    Vertex(String),
    /// An origin or work plane (format revision 11).
    Plane(PlaneRef),
    /// A work point, the part origin or an edge midpoint: the termination
    /// plane through it, parallel to the sketch (format revision 11).
    Point(PointRef),
}

/// Where an extent ends, resolved against the model.
#[derive(Clone, Debug)]
pub(crate) enum Termination {
    /// A plane parallel to the sketch, at this offset along its normal.
    Offset(Real),
    /// A plane that is not parallel to the sketch: the extrusion is cut
    /// by it.
    Plane(Plane),
    /// A cylindrical face (its whole, infinite surface).
    Cylinder(Cylinder),
}

impl TargetRef {
    /// Display text: the persistent name, or the plane / point it means.
    pub fn name(&self) -> String {
        match self {
            TargetRef::Face(n) | TargetRef::Vertex(n) => n.clone(),
            TargetRef::Plane(PlaneRef::Origin(p)) => format!("{p:?} Plane"),
            TargetRef::Plane(PlaneRef::Work(id)) | TargetRef::Point(PointRef::Work(id)) => {
                id.clone()
            }
            TargetRef::Plane(PlaneRef::Face(n)) => n.clone(),
            TargetRef::Plane(PlaneRef::Fixed) => "Fixed plane".into(),
            TargetRef::Point(PointRef::PartOrigin) => "Center Point".into(),
            TargetRef::Point(PointRef::Vertex(n)) => n.clone(),
            TargetRef::Point(PointRef::EdgeMid(n)) => format!("{n} (midpoint)"),
        }
    }

    /// Offset of the target along `plane`'s normal (from its origin).
    /// Only targets parallel to the sketch have one.
    fn offset(&self, body: &BRep, works: &WorkGeom, plane: &Plane) -> Result<Real, String> {
        match self.termination(body, works, plane)? {
            Termination::Offset(t) => Ok(t),
            Termination::Plane(_) | Termination::Cylinder(_) => Err(format!(
                "{} is not parallel to the sketch (only parallel faces are supported here)",
                self.name()
            )),
        }
    }

    /// The termination this target gives an extrusion from `plane`.
    pub(crate) fn termination(
        &self,
        body: &BRep,
        works: &WorkGeom,
        plane: &Plane,
    ) -> Result<Termination, String> {
        let n = plane.normal.into_inner();
        let through = |p: Point3| Termination::Offset((p - plane.origin).dot(&n));
        let of_plane = |fp: Plane| {
            if fp.normal.cross(&plane.normal).norm() <= 1e-9 {
                Ok(Termination::Offset((fp.origin - plane.origin).dot(&n)))
            } else if fp.normal.dot(&plane.normal).abs() < 1e-6 {
                Err(format!(
                    "{} is parallel to the extrusion direction and never meets it",
                    self.name()
                ))
            } else {
                Ok(Termination::Plane(fp))
            }
        };
        match self {
            TargetRef::Vertex(name) => body
                .find_vertex(name)
                .and_then(|v| body.vertex(v))
                .map(|v| through(v.point))
                .ok_or_else(|| format!("vertex {name} not found")),
            TargetRef::Point(p) => super::placement::point_of(p, body, works)
                .map(through)
                .map_err(|e| e.to_string()),
            TargetRef::Plane(p) => super::placement::plane_of(p, body, works)
                .map_err(|e| e.to_string())
                .and_then(of_plane),
            TargetRef::Face(name) => {
                let f = body
                    .find_face(name)
                    .ok_or_else(|| format!("face {name} not found"))?;
                if let Some(fp) = body.face_plane(f) {
                    return of_plane(fp);
                }
                match body.face(f).and_then(|fc| body.surface(fc.surface)) {
                    Some(Surface::Cylinder(c)) => Ok(Termination::Cylinder(c.clone())),
                    _ => Err(format!(
                        "face {name} is neither planar nor cylindrical (not supported yet)"
                    )),
                }
            }
        }
    }

    /// The model face this target names, if it is one.
    pub(crate) fn face(&self, body: &BRep) -> Option<crate::kernel::FaceId> {
        match self {
            TargetRef::Face(n) | TargetRef::Plane(PlaneRef::Face(n)) => body.find_face(n),
            _ => None,
        }
    }

    /// Whether the target is a curved face (Alternate Solution applies).
    pub fn is_curved_face(&self, body: &BRep) -> bool {
        self.face(body)
            .is_some_and(|f| body.face_plane(f).is_none())
    }
}

/// A closed region of a sketch chosen as (part of) a profile.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileRef {
    /// The sketch curves of the region's outer boundary (sorted, unique).
    pub sources: Vec<EntityId>,
    /// A point inside the region (sketch coordinates), used when no region
    /// has exactly these boundary curves any more.
    pub seed: Point2,
}

/// An extrusion of sketch regions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtrudeFeature {
    /// Immutable identity, the prefix of its topology names.
    pub id: String,
    /// Display name (free to rename).
    pub name: String,
    /// `id` of the profile sketch.
    pub sketch: String,
    /// The chosen regions.
    pub profiles: Vec<ProfileRef>,
    /// Distance A (mm, > 0).
    pub distance: Real,
    /// Distance B for [`ExtentDirection::Asymmetric`] (mm).
    #[serde(default)]
    pub distance_b: Real,
    /// Direction of the extent.
    #[serde(default)]
    pub direction: ExtentDirection,
    /// Where it ends.
    #[serde(default)]
    pub extent: Extent,
    /// Start face instead of the sketch plane (with `To`: Between).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<TargetRef>,
    /// Taper A (radians; positive grows the section along the extrusion).
    #[serde(default)]
    pub taper: Real,
    /// Taper B, the second side of an Asymmetric extrusion (radians).
    #[serde(default)]
    pub taper_b: Real,
    /// Boolean operation with the body.
    #[serde(default)]
    pub operation: super::Operation,
    /// Solid or surface.
    #[serde(default)]
    pub output: super::Output,
    /// Bodies the operation works on (Inventor's **Solids**), by body name;
    /// empty = the default (Join: one body, Cut/Intersect: all).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<String>,
    /// Parameter names and equations of the numeric fields (see
    /// [`super::params`]).
    #[serde(
        default,
        skip_serializing_if = "super::params::FeatureParams::is_empty"
    )]
    pub params: super::params::FeatureParams,
    /// The preset the feature was made with (`docs/spec/0.11-presets.md`):
    /// only its name, Edit Feature shows it again; the values are the
    /// feature's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// **Alternate Solution**: on a curved To face, stop at its near side
    /// (off: the far side, Inventor's default).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub alternate: bool,
    /// **Extend face to end feature**: a From / To face that does not cover
    /// the whole profile is extended (its plane is used); off, such a face
    /// is an error. Older files always extended (hence the default).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub extend_face: bool,
    /// **Terminator** of To Next: only faces of this body (by its persistent
    /// name) end the extrusion; `None` = any body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminator: Option<String>,
    /// An open profile (Match Shape), extruded besides `profiles`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<OpenProfile>,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

/// An open profile (Inventor: open sketch curves in a part that already
/// has a body). Its ends are extended along their tangents to a frame
/// around the body, and the frame closes it on the chosen side.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpenProfile {
    /// The sketch curves of the chain (lines and arcs, connected end to
    /// end; any order).
    pub curves: Vec<EntityId>,
    /// Keep the other side of the chain.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    /// **Match Shape**: on, a Join floods up to the body's walls (the tool
    /// is also limited to the body's bounding box along the extrusion);
    /// off, the closed side is extruded by the full extent.
    #[serde(default = "yes")]
    pub match_shape: bool,
}

/// Outer-boundary curves of a region: its identity in a [`ProfileRef`].
pub fn region_sources(region: &Region) -> Vec<EntityId> {
    let mut s: Vec<EntityId> = region.outer.edges.iter().map(|e| e.source).collect();
    s.sort();
    s.dedup();
    s
}

impl ProfileRef {
    /// A reference to `region`, seeded at `seed`.
    pub fn of(region: &Region, seed: Point2) -> Self {
        Self {
            sources: region_sources(region),
            seed,
        }
    }

    /// Finds the referenced region among `regions`.
    pub fn resolve<'a>(&self, regions: &'a [Region]) -> Option<&'a Region> {
        let same: Vec<&Region> = regions
            .iter()
            .filter(|r| region_sources(r) == self.sources)
            .collect();
        match same.as_slice() {
            [one] => Some(*one),
            // Several regions bounded by the same curves (the two crescents
            // of two overlapping circles, one of them projected): the one
            // holding the seed.
            [first, ..] => same
                .iter()
                .find(|r| r.contains(self.seed))
                .copied()
                .or(Some(*first)),
            [] => regions.iter().find(|r| r.contains(self.seed)),
        }
    }
}

/// Name of a sketch entity in topology names, e.g. `"3v1"`.
pub fn entity_key(e: EntityId) -> String {
    format!("{:?}", e.data())
}

fn segment(curve: &ProfileCurve) -> Segment2 {
    match curve {
        ProfileCurve::Line { a, b } => Segment2::Line { a: *a, b: *b },
        ProfileCurve::Arc { c, r, a0, sweep } => Segment2::Arc {
            c: *c,
            r: *r,
            a0: *a0,
            sweep: *sweep,
        },
        ProfileCurve::Spline {
            points,
            reversed,
            knots,
        } => Segment2::Spline {
            points: points.clone(),
            reversed: *reversed,
            knots: knots.clone(),
        },
    }
}

/// Kernel profile of a region: segments named `"<entity>:<k>"`, k counting
/// the pieces of the same sketch curve in the region.
pub(super) fn kernel_profile(region: &Region) -> (Vec<ProfileSegment>, Vec<Vec<ProfileSegment>>) {
    let mut seen: Vec<EntityId> = Vec::new();
    let mut convert = |lp: &ProfileLoop| -> Vec<ProfileSegment> {
        lp.edges
            .iter()
            .map(|e| {
                let k = seen.iter().filter(|&&s| s == e.source).count();
                seen.push(e.source);
                ProfileSegment {
                    geometry: segment(&e.curve),
                    name: format!("{}:{k}", entity_key(e.source)),
                }
            })
            .collect()
    };
    let outer = convert(&region.outer);
    let holes = region.holes.iter().map(&mut convert).collect();
    (outer, holes)
}

/// The profiles a new Extrude / Revolve starts with: the only region of
/// the sketch (Inventor), or else every region the user's own curves close
/// (no projected reference edge on its boundary) — 40 rectangles drawn on a
/// face are all taken at once — except one that fills a hole of another such
/// region (a plate with a hole keeps the hole).
pub fn default_profiles(ps: &super::PartSketch, regions: &[Region]) -> Vec<ProfileRef> {
    if let [r] = regions {
        return vec![ProfileRef::of(r, r.interior_point())];
    }
    let own = |r: &Region| {
        std::iter::once(&r.outer)
            .chain(&r.holes)
            .flat_map(|l| l.edges.iter())
            .all(|e| !ps.is_reference(e.source))
    };
    let islands: Vec<&Region> = regions.iter().filter(|r| own(r)).collect();
    let fills_a_hole = |r: &Region| {
        let p = r.interior_point();
        islands.iter().any(|o| {
            !std::ptr::eq(*o, r)
                && o.holes.iter().any(|h| {
                    let poly = h.polygon(1e-3);
                    let mut inside = false;
                    for k in 0..poly.len() {
                        let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
                        if (a.y > p.y) != (b.y > p.y)
                            && p.x < a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y)
                        {
                            inside = !inside;
                        }
                    }
                    inside
                })
        })
    };
    islands
        .iter()
        .filter(|r| !fills_a_hole(r))
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect()
}

impl ExtrudeFeature {
    /// An extrusion of `profiles` by `distance` with default options (id,
    /// name and sketch are set by [`super::PartDocument::add_extrude`]).
    pub fn new(profiles: Vec<ProfileRef>, distance: Real) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            sketch: String::new(),
            profiles,
            distance,
            distance_b: distance,
            direction: ExtentDirection::Default,
            extent: Extent::Distance,
            from: None,
            taper: 0.0,
            taper_b: 0.0,
            operation: Default::default(),
            output: Default::default(),
            solids: Vec::new(),
            params: Default::default(),
            preset: None,
            alternate: false,
            extend_face: true,
            terminator: None,
            open: None,
        }
    }

    /// **Predict Boolean Operation** (Inventor's Advanced setting): New
    /// Solid without a body; else Cut when the extrusion goes into the
    /// material (sampled along its path from the first profile), Join when
    /// it goes out of it. Symmetric / Asymmetric cut only when both sides
    /// start in the material.
    pub fn predict_operation(&self, body: &BRep, sketch: &Sketch) -> super::Operation {
        use super::Operation;
        if !body.solids().any(|(_, s)| !s.open) {
            return Operation::NewSolid;
        }
        let regions = sketch.regions();
        let p = match self.profiles.first().and_then(|pr| pr.resolve(&regions)) {
            Some(r) => r.interior_point(),
            None => return Operation::Join,
        };
        let plane = &sketch.plane;
        let n = plane.normal.into_inner();
        let at = plane.point_at(p.x, p.y);
        let reach = match self.extent {
            Extent::Distance if self.distance.is_finite() && self.distance > 0.0 => self.distance,
            _ => body
                .bounding_box()
                .map_or(20.0, |(lo, hi)| nalgebra::distance(&lo, &hi)),
        };
        let into = |sign: Real| {
            (0..24).any(|k| {
                let t = 1e-3 + reach * k as Real / 24.0;
                body.contains_point(&(at + n * (sign * t)))
            })
        };
        let cut = match self.direction {
            ExtentDirection::Default => into(1.0),
            ExtentDirection::Flipped => into(-1.0),
            ExtentDirection::Symmetric | ExtentDirection::Asymmetric => into(1.0) && into(-1.0),
        };
        if cut { Operation::Cut } else { Operation::Join }
    }

    /// Whether a From or To face of this extrusion does not cover the whole
    /// profile, so **Extend face to end feature** is needed (the command
    /// turns it on by itself, as Inventor does).
    pub fn needs_extended_face(&self, body: &BRep, works: &WorkGeom, sketch: &Sketch) -> bool {
        let regions = sketch.regions();
        let probes: Vec<Point2> = self
            .profiles
            .iter()
            .filter_map(|pr| pr.resolve(&regions))
            .flat_map(probe_points)
            .collect();
        let plane = &sketch.plane;
        let s0 = match &self.from {
            Some(t) => t.offset(body, works, plane).unwrap_or(0.0),
            None => 0.0,
        };
        let short = |t: &TargetRef, from_offset: Real| {
            t.face(body)
                .is_some_and(|f| !face_covers(body, f, plane, from_offset, &probes))
        };
        self.from.as_ref().is_some_and(|t| short(t, 0.0))
            || matches!(&self.extent, Extent::To(t) if short(t, s0))
    }

    /// Whether the body's material lies just below the first profile (on
    /// the side opposite the sketch normal) and not above it: a cut there
    /// should go against the normal. `None`: no profile / no body.
    pub fn material_below(&self, body: &BRep, sketch: &Sketch) -> Option<bool> {
        let regions = sketch.regions();
        let r = self.profiles.first()?.resolve(&regions)?;
        let p = r.interior_point();
        let plane = &sketch.plane;
        let n = plane.normal.into_inner();
        let at = plane.point_at(p.x, p.y);
        let eps = 1e-3;
        let (below, above) = (
            body.contains_point(&(at - n * eps)),
            body.contains_point(&(at + n * eps)),
        );
        Some(below && !above)
    }

    /// Builds the extrusion into `body` from the (already up-to-date)
    /// profile sketch. Each region becomes one solid; with several regions
    /// the topology names get a `#<n>` suffix on the prefix.
    pub fn build(&self, body: &mut BRep, sketch: &Sketch) -> Result<(), String> {
        self.build_in(body, &WorkGeom::new(), sketch)
    }

    /// [`Self::build`] with the part's work geometry (work planes and
    /// points as targets).
    pub fn build_in(
        &self,
        body: &mut BRep,
        works: &WorkGeom,
        sketch: &Sketch,
    ) -> Result<(), String> {
        let plane: &Plane = &sketch.plane;
        for piece in self.pieces(body, works, sketch)? {
            if piece.span.clip.is_some() || piece.bounds.is_some() {
                return Err(
                    "this extent needs the exact geometry kernel (an earlier feature uses an option it does not cover yet)"
                        .into(),
                );
            }
            let id = body
                .extrude_profile(
                    plane,
                    &piece.outer,
                    &piece.holes,
                    &piece.span.levels,
                    &piece.prefix,
                )
                .map_err(|e| e.to_string())?;
            if self.output == super::Output::Surface {
                let p = &piece.prefix;
                body.open_up(id, &[format!("{p}:Start"), format!("{p}:End")]);
            }
        }
        Ok(())
    }

    /// What the extrusion is made of: one piece per profile (and one for
    /// the open profile), each with its kernel profile and extent.
    pub(crate) fn pieces(
        &self,
        body: &BRep,
        works: &WorkGeom,
        sketch: &Sketch,
    ) -> Result<Vec<Piece>, String> {
        let regions = sketch.regions();
        let plane: &Plane = &sketch.plane;
        let mut out = Vec::new();
        for (n, pr) in self.profiles.iter().enumerate() {
            let region = pr
                .resolve(&regions)
                .ok_or_else(|| format!("profile #{} no longer exists in the sketch", n + 1))?;
            let (outer, holes) = kernel_profile(region);
            out.push(Piece {
                prefix: piece_prefix(&self.id, n),
                span: self.span(body, works, plane, &probe_points(region))?,
                outer,
                holes,
                bounds: None,
            });
        }
        if let Some(op) = &self.open {
            let (outer, bounds) = open_loop(op, body, sketch, self.operation)?;
            let probes = segments_polygon(&outer);
            out.push(Piece {
                prefix: piece_prefix(&self.id, out.len()),
                span: self.span(body, works, plane, &probes)?,
                outer,
                holes: Vec::new(),
                bounds,
            });
        }
        if out.is_empty() {
            return Err("select at least one profile".into());
        }
        Ok(out)
    }

    /// The extent of one profile (given by sample points `probes` of its
    /// outline, sketch coordinates): levels and, for a termination that is
    /// not parallel to the sketch, the clip that ends it.
    pub(crate) fn span(
        &self,
        body: &BRep,
        works: &WorkGeom,
        plane: &Plane,
        probes: &[Point2],
    ) -> Result<Span, String> {
        let plain = |levels: Vec<ExtrudeLevel>| Span { levels, clip: None };
        let positive = |v: Real, what: &str| {
            if v.is_finite() && v > 0.0 {
                Ok(v)
            } else {
                Err(format!("{what} must be positive"))
            }
        };
        let n = plane.normal.into_inner();
        let s0 = match &self.from {
            Some(t) => {
                let s = t
                    .offset(body, works, plane)
                    .map_err(|e| format!("From: {e}"))?;
                if !self.extend_face
                    && let Some(f) = t.face(body)
                    && !face_covers(body, f, plane, 0.0, probes)
                {
                    return Err(
                        "From: the face does not cover the profile (turn on Extend face to end feature)"
                            .into(),
                    );
                }
                s
            }
            None => 0.0,
        };
        let sign = if self.direction == ExtentDirection::Flipped {
            -1.0
        } else {
            1.0
        };
        let (ta, tb) = (self.taper.tan(), self.taper_b.tan());
        let one = |end: Real| -> Result<Vec<ExtrudeLevel>, String> {
            if (end - s0).abs() < 1e-9 {
                return Err("the extrusion has zero depth".into());
            }
            Ok(vec![
                ExtrudeLevel {
                    offset: s0,
                    inset: 0.0,
                },
                ExtrudeLevel {
                    offset: end,
                    inset: (end - s0).abs() * ta,
                },
            ])
        };
        // Two-sided around s0: `up` along the normal (taper A), `down`
        // against it (taper B); kinked at s0 only when tapered.
        let two = |up: Real, down: Real, tb: Real| {
            let mut v = vec![ExtrudeLevel {
                offset: s0 - down,
                inset: down * tb,
            }];
            if ta != 0.0 || tb != 0.0 {
                v.push(ExtrudeLevel {
                    offset: s0,
                    inset: 0.0,
                });
            }
            v.push(ExtrudeLevel {
                offset: s0 + up,
                inset: up * ta,
            });
            v
        };
        match &self.extent {
            Extent::Distance => {
                let a = positive(self.distance, "the distance")?;
                match self.direction {
                    ExtentDirection::Default | ExtentDirection::Flipped => {
                        one(s0 + sign * a).map(plain)
                    }
                    ExtentDirection::Symmetric => Ok(plain(two(a / 2.0, a / 2.0, ta))),
                    ExtentDirection::Asymmetric => {
                        let b = positive(self.distance_b, "distance B")?;
                        Ok(plain(two(a, b, tb)))
                    }
                }
            }
            Extent::To(t) => {
                let base = |p: &Point2| plane.point_at(p.x, p.y) + n * s0;
                match t
                    .termination(body, works, plane)
                    .map_err(|e| format!("To: {e}"))?
                {
                    Termination::Offset(end) => {
                        if !self.extend_face
                            && let Some(f) = t.face(body)
                            && !face_covers(body, f, plane, s0, probes)
                        {
                            return Err("To: the face does not cover the profile (turn on Extend face to end feature)".into());
                        }
                        one(end).map(plain)
                    }
                    Termination::Plane(fp) => {
                        let m = fp.normal.into_inner();
                        let nm = n.dot(&m);
                        let ts: Vec<Real> = probes
                            .iter()
                            .map(|p| (fp.origin - base(p)).dot(&m) / nm)
                            .collect();
                        let (lo, hi) = ts
                            .iter()
                            .fold((Real::MAX, Real::MIN), |(a, b), &t| (a.min(t), b.max(t)));
                        let sg = if lo > 1e-9 {
                            1.0
                        } else if hi < -1e-9 {
                            -1.0
                        } else {
                            return Err("To: the plane crosses the profile".into());
                        };
                        if !self.extend_face
                            && let Some(f) = t.face(body)
                            && !face_covers(body, f, plane, s0, probes)
                        {
                            return Err("To: the face does not cover the profile (turn on Extend face to end feature)".into());
                        }
                        let reach = lo.abs().max(hi.abs());
                        let size = probes_size(probes);
                        let tilt = (m - n * nm).norm() / nm.abs();
                        let margin = (size + 2.0 * reach * ta.abs()) * tilt + 1.0 + 0.1 * reach;
                        let keep = probes.first().map(base).unwrap_or(plane.origin);
                        Ok(Span {
                            levels: one(s0 + sg * (reach + margin))?,
                            clip: Some(Clip::Plane { plane: fp, keep }),
                        })
                    }
                    Termination::Cylinder(cyl) => {
                        if self.taper.abs() > 1e-12 {
                            return Err("To: a taper to a curved face is not supported yet".into());
                        }
                        // The rays from every probe must meet the cylinder
                        // ahead, all in one direction.
                        let hits = |sg: Real| -> Option<Real> {
                            let d = n * sg;
                            let mut far: Real = 0.0;
                            for p in probes {
                                let (t1, t2) = ray_cylinder(&base(p), &d, &cyl)?;
                                if t1 <= 1e-9 {
                                    return None;
                                }
                                far = far.max(t2);
                            }
                            Some(far)
                        };
                        let (sg, far) = match (hits(1.0), hits(-1.0)) {
                            (Some(f), _) => (1.0, f),
                            (None, Some(f)) => (-1.0, f),
                            (None, None) => {
                                return Err(
                                    "To: the profile does not meet the curved face in either direction"
                                        .into(),
                                );
                            }
                        };
                        Ok(Span {
                            levels: one(s0 + sg * (far * 1.1 + 1.0))?,
                            clip: Some(Clip::Cylinder {
                                cyl,
                                dir: n * sg,
                                near: self.alternate,
                            }),
                        })
                    }
                }
            }
            Extent::ToNext => {
                // Inventor's Terminator: only faces of the chosen body.
                let allowed: Option<std::collections::HashSet<crate::kernel::FaceId>> =
                    self.terminator.as_ref().map(|name| {
                        body.solids()
                            .filter(|(_, s)| s.name.as_deref() == Some(name.as_str()))
                            .flat_map(|(_, s)| {
                                s.shells.iter().flat_map(|sh| sh.faces.iter().copied())
                            })
                            .collect()
                    });
                if let (Some(set), Some(name)) = (&allowed, &self.terminator)
                    && set.is_empty()
                {
                    return Err(format!(
                        "To Next: the terminator body {name} no longer exists"
                    ));
                }
                body.faces()
                    .filter(|(f, _)| allowed.as_ref().is_none_or(|s| s.contains(f)))
                    .filter_map(|(f, _)| {
                        let fp = body.face_plane(f)?;
                        if fp.normal.cross(&plane.normal).norm() > 1e-9 {
                            return None;
                        }
                        let t = (fp.origin - plane.origin).dot(&n);
                        let covers = probes
                            .iter()
                            .all(|p| body.face_contains(f, &(plane.point_at(p.x, p.y) + n * t)));
                        ((t - s0) * sign > 1e-9 && covers).then_some(t)
                    })
                    .min_by(|a, b| ((a - s0) * sign).total_cmp(&((b - s0) * sign)))
                    .ok_or_else(|| {
                        "To Next: no face in that direction covers the profile".to_owned()
                    })
                    .and_then(one)
                    .map(plain)
            }
            Extent::ThroughAll => {
                // The far corner of the body's bounding box in direction `sg`.
                let far = |sg: Real| {
                    body.bounding_box().map_or(0.0, |(lo, hi)| {
                        (0..8)
                            .map(|i| {
                                let c = crate::kernel::Point3::new(
                                    if i & 1 == 0 { lo.x } else { hi.x },
                                    if i & 2 == 0 { lo.y } else { hi.y },
                                    if i & 4 == 0 { lo.z } else { hi.z },
                                );
                                ((c - plane.origin).dot(&n) - s0) * sg
                            })
                            .fold(0.0, Real::max)
                    })
                };
                // Symmetric / Asymmetric: through all on both sides of the
                // sketch (Inventor), each side to its own far end.
                if matches!(
                    self.direction,
                    ExtentDirection::Symmetric | ExtentDirection::Asymmetric
                ) {
                    let (up, down) = (far(1.0), far(-1.0));
                    if up <= 1e-9 && down <= 1e-9 {
                        return Err("Through All: there is no body on either side".into());
                    }
                    // A side without material still gets a sliver of depth,
                    // so the levels stay ordered.
                    let tiny = 1e-6 * (up + down);
                    return Ok(plain(two(up.max(tiny), down.max(tiny), tb)));
                }
                let f = far(sign);
                if f <= 1e-9 {
                    return Err("Through All: there is no body in that direction".into());
                }
                one(s0 + sign * f).map(plain)
            }
        }
    }
}

/// The prefix of piece `n` of a feature (`Extrusion1`, `Extrusion1#2`).
fn piece_prefix(id: &str, n: usize) -> String {
    if n == 0 {
        id.to_owned()
    } else {
        format!("{id}#{}", n + 1)
    }
}

/// One profile of an extrusion, ready for a kernel.
pub(crate) struct Piece {
    /// Topology-name prefix.
    pub prefix: String,
    /// Outer loop (counter-clockwise).
    pub outer: Vec<ProfileSegment>,
    /// Holes.
    pub holes: Vec<Vec<ProfileSegment>>,
    /// Levels and clip.
    pub span: Span,
    /// Match Shape of an open profile in a Join: the tool is limited to
    /// this box (the body's bounding box).
    pub bounds: Option<(Point3, Point3)>,
}

impl Piece {
    /// The largest sketch coordinate of the profile (for clipping sizes).
    pub fn extent(&self) -> Real {
        segments_polygon(&self.outer)
            .iter()
            .map(|p| p.x.abs().max(p.y.abs()))
            .fold(0.0, Real::max)
    }
}

/// The extent of one profile: its cross-section levels and, when the
/// termination is not parallel to the sketch, what ends the tool (made
/// long enough to pass it).
pub(crate) struct Span {
    /// Offsets along the sketch normal with taper insets.
    pub levels: Vec<ExtrudeLevel>,
    /// The clip, if any.
    pub clip: Option<Clip>,
}

/// How a termination that is not parallel to the sketch ends an extrusion.
#[derive(Clone, Debug)]
pub(crate) enum Clip {
    /// Keep the side of `plane` that holds `keep` (a point of the profile).
    Plane {
        /// The termination plane.
        plane: Plane,
        /// A point on the kept side.
        keep: Point3,
    },
    /// Stop at a cylinder, travelling along `dir`: at its near side
    /// (`near`, Alternate Solution) or its far side.
    Cylinder {
        /// The cylinder (infinite).
        cyl: Cylinder,
        /// Direction of travel (unit).
        dir: Vector3,
        /// Near side.
        near: bool,
    },
}

/// Sample points of a region's outline (and one inside), sketch
/// coordinates: what must reach a termination.
fn probe_points(region: &Region) -> Vec<Point2> {
    region
        .outer
        .polygon(0.05)
        .into_iter()
        .chain(std::iter::once(region.interior_point()))
        .collect()
}

/// Diagonal of the probes' box.
fn probes_size(probes: &[Point2]) -> Real {
    let (mut lo, mut hi) = (
        Point2::new(Real::MAX, Real::MAX),
        Point2::new(Real::MIN, Real::MIN),
    );
    for p in probes {
        lo = lo.inf(p);
        hi = hi.sup(p);
    }
    if probes.is_empty() {
        0.0
    } else {
        (hi - lo).norm()
    }
}

/// Whether planar face `f` covers every probe, projected along the sketch
/// normal from offset `s0` onto the face's plane.
fn face_covers(
    body: &BRep,
    f: crate::kernel::FaceId,
    plane: &Plane,
    s0: Real,
    probes: &[Point2],
) -> bool {
    let Some(fp) = body.face_plane(f) else {
        return true; // curved faces are taken whole (infinite surface)
    };
    let n = plane.normal.into_inner();
    let m = fp.normal.into_inner();
    let nm = n.dot(&m);
    if nm.abs() < 1e-9 {
        return false;
    }
    probes.iter().all(|p| {
        let b = plane.point_at(p.x, p.y) + n * s0;
        let t = (fp.origin - b).dot(&m) / nm;
        body.face_contains(f, &(b + n * t))
    })
}

/// Where the ray `o + t·d` meets the infinite cylinder: `(t_in, t_out)`.
fn ray_cylinder(o: &Point3, d: &Vector3, cyl: &Cylinder) -> Option<(Real, Real)> {
    let a = cyl.axis.into_inner();
    let w = o - cyl.origin;
    let dp = d - a * d.dot(&a);
    let wp = w - a * w.dot(&a);
    let qa = dp.norm_squared();
    if qa < 1e-12 {
        return None;
    }
    let qb = 2.0 * wp.dot(&dp);
    let qc = wp.norm_squared() - cyl.radius * cyl.radius;
    let disc = qb * qb - 4.0 * qa * qc;
    if disc < 0.0 {
        return None;
    }
    let s = disc.sqrt();
    Some(((-qb - s) / (2.0 * qa), (-qb + s) / (2.0 * qa)))
}

/// Points along a loop of segments (arcs sampled), for probes.
pub(crate) fn segments_polygon(segs: &[ProfileSegment]) -> Vec<Point2> {
    let mut out = Vec::new();
    for s in segs {
        match &s.geometry {
            Segment2::Line { a, .. } => out.push(*a),
            Segment2::Arc { c, r, a0, sweep } => {
                let k = ((sweep.abs() / 0.1).ceil() as usize).max(2);
                for i in 0..k {
                    let t = a0 + sweep * i as Real / k as Real;
                    out.push(Point2::new(c.x + r * t.cos(), c.y + r * t.sin()));
                }
            }
            Segment2::Spline { points, .. } => out.extend(points.iter().copied()),
        }
    }
    out
}

/// The closed loop of an open profile: the chain, its ends extended along
/// their tangents to a frame around the body (in the sketch plane), and the
/// frame on the chosen side. Also the box a Match Shape Join is limited to.
#[allow(clippy::type_complexity)]
fn open_loop(
    op: &OpenProfile,
    body: &BRep,
    sketch: &Sketch,
    operation: super::Operation,
) -> Result<(Vec<ProfileSegment>, Option<(Point3, Point3)>), String> {
    let Some((lo, hi)) = body
        .bounding_box()
        .filter(|_| body.solids().any(|(_, s)| !s.open))
    else {
        return Err("an open profile needs a solid body (it cannot be the first feature)".into());
    };
    let (chain, first, last, start_dir, end_dir) = open_chain(op, sketch)?;
    // The frame: the body's box seen in the sketch plane (Join: exactly, so
    // nothing is added outside; else a little larger, to cut through).
    let plane = &sketch.plane;
    let mut flo = Point2::new(Real::MAX, Real::MAX);
    let mut fhi = Point2::new(Real::MIN, Real::MIN);
    for i in 0..8 {
        let c = Point3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        );
        let q = plane.project(&c);
        flo = flo.inf(&q);
        fhi = fhi.sup(&q);
    }
    for p in segments_polygon(&chain).into_iter().chain([last]) {
        flo = flo.inf(&p);
        fhi = fhi.sup(&p);
    }
    if operation != super::Operation::Join {
        let m = 1.0 + 0.05 * (fhi - flo).norm();
        flo -= Vector2::new(m, m);
        fhi += Vector2::new(m, m);
    }
    let exit = |p: Point2, d: Vector2| -> Point2 {
        let mut t = Real::MAX;
        for (pc, dc, lo_, hi_) in [(p.x, d.x, flo.x, fhi.x), (p.y, d.y, flo.y, fhi.y)] {
            if dc > 1e-12 {
                t = t.min((hi_ - pc) / dc);
            } else if dc < -1e-12 {
                t = t.min((lo_ - pc) / dc);
            }
        }
        if t == Real::MAX {
            p
        } else {
            p + d * t.max(0.0)
        }
    };
    let e_out = exit(last, end_dir);
    let s_out = exit(first, start_dir);
    // Perimeter parameter of a frame point: 0..4, counter-clockwise from
    // the low corner.
    let (w, h) = (fhi.x - flo.x, fhi.y - flo.y);
    let param = |p: Point2| -> Real {
        let eps = 1e-6 * (w + h);
        if (p.y - flo.y).abs() < eps {
            (p.x - flo.x) / w
        } else if (p.x - fhi.x).abs() < eps {
            1.0 + (p.y - flo.y) / h
        } else if (p.y - fhi.y).abs() < eps {
            2.0 + (fhi.x - p.x) / w
        } else {
            3.0 + (fhi.y - p.y) / h
        }
    };
    // Corner j (mod 4) sits at perimeter parameter j.
    let corner = |j: i64| match j.rem_euclid(4) {
        0 => Point2::new(flo.x, flo.y),
        1 => Point2::new(fhi.x, flo.y),
        2 => Point2::new(fhi.x, fhi.y),
        _ => Point2::new(flo.x, fhi.y),
    };
    let (ue, us) = (param(e_out), param(s_out));
    let mut walk = vec![e_out];
    if op.flip {
        // Clockwise from the end's exit back to the start's.
        let gap = (ue - us).rem_euclid(4.0);
        let mut j = ue.ceil() as i64 - 1;
        while (j as Real) > ue - gap + 1e-9 {
            walk.push(corner(j));
            j -= 1;
        }
    } else {
        // Counter-clockwise.
        let gap = (us - ue).rem_euclid(4.0);
        let mut j = ue.floor() as i64 + 1;
        while (j as Real) < ue + gap - 1e-9 {
            walk.push(corner(j));
            j += 1;
        }
    }
    walk.push(s_out);
    let mut lp = chain;
    let mut prev = last;
    for (k, q) in walk.into_iter().chain([first]).enumerate() {
        if (q - prev).norm() > 1e-9 {
            lp.push(ProfileSegment {
                geometry: Segment2::Line { a: prev, b: q },
                name: format!("Frame:{k}"),
            });
        }
        prev = q;
    }
    // Counter-clockwise outer loop.
    let poly = segments_polygon(&lp);
    let area: Real = (0..poly.len())
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    if area.abs() < 1e-9 {
        return Err("Match Shape: the open profile closes no area".into());
    }
    if area < 0.0 {
        lp.reverse();
        for s in &mut lp {
            s.geometry = reverse_segment(&s.geometry);
        }
    }
    let bounds = (operation == super::Operation::Join && op.match_shape).then_some((lo, hi));
    Ok((lp, bounds))
}

/// The open profile's curves ordered end to end: the chain, its first and
/// last points and the directions leaving it there (outward).
#[allow(clippy::type_complexity)]
pub(crate) fn open_chain(
    op: &OpenProfile,
    sketch: &Sketch,
) -> Result<(Vec<ProfileSegment>, Point2, Point2, Vector2, Vector2), String> {
    use crate::sketch::Entity;
    // The chain's curves as segments with their end points.
    struct Piece2 {
        seg: Segment2,
        name: String,
        a: Point2,
        b: Point2,
    }
    let mut pieces = Vec::new();
    for &e in &op.curves {
        let pos = |p| sketch.point(p).map(|q| q.position);
        let name = format!("{}:0", entity_key(e));
        match sketch.entity(e) {
            Some(Entity::Line { start, end }) => {
                let (Some(a), Some(b)) = (pos(*start), pos(*end)) else {
                    return Err("Match Shape: a curve of the open profile is gone".into());
                };
                pieces.push(Piece2 {
                    seg: Segment2::Line { a, b },
                    name,
                    a,
                    b,
                });
            }
            Some(Entity::Arc { center, start, end }) => {
                let (Some(c), Some(a), Some(b)) = (pos(*center), pos(*start), pos(*end)) else {
                    return Err("Match Shape: a curve of the open profile is gone".into());
                };
                let r = (a - c).norm();
                let a0 = (a.y - c.y).atan2(a.x - c.x);
                let a1 = (b.y - c.y).atan2(b.x - c.x);
                let sweep = (a1 - a0).rem_euclid(std::f64::consts::TAU);
                let b = Point2::new(c.x + r * a1.cos(), c.y + r * a1.sin());
                pieces.push(Piece2 {
                    seg: Segment2::Arc { c, r, a0, sweep },
                    name,
                    a,
                    b,
                });
            }
            Some(_) => {
                return Err("Match Shape: open profiles of lines and arcs only (yet)".into());
            }
            None => return Err("Match Shape: a curve of the open profile is gone".into()),
        }
    }
    if pieces.is_empty() {
        return Err("Match Shape: the open profile has no curves".into());
    }
    // Order them end to end.
    let close = |p: Point2, q: Point2| (p - q).norm() < 1e-6;
    let degree = |p: Point2| {
        pieces
            .iter()
            .map(|c| usize::from(close(c.a, p)) + usize::from(close(c.b, p)))
            .sum::<usize>()
    };
    let start_k = pieces
        .iter()
        .position(|c| degree(c.a) == 1 || degree(c.b) == 1)
        .ok_or("Match Shape: the curves form a closed loop (use it as a profile)")?;
    let mut used = vec![false; pieces.len()];
    let mut chain: Vec<ProfileSegment> = Vec::new();
    let mut at = if degree(pieces[start_k].a) == 1 {
        pieces[start_k].a
    } else {
        pieces[start_k].b
    };
    let first = at;
    let mut end_dir = Vector2::zeros();
    let mut start_dir = Vector2::zeros();
    for step in 0..pieces.len() {
        let Some(k) = (0..pieces.len())
            .find(|&k| !used[k] && (close(pieces[k].a, at) || close(pieces[k].b, at)))
        else {
            return Err("Match Shape: the open profile must be one connected chain".into());
        };
        used[k] = true;
        let c = &pieces[k];
        let forward = close(c.a, at);
        let seg = if forward {
            c.seg.clone()
        } else {
            reverse_segment(&c.seg)
        };
        let (t0, t1) = segment_tangents(&seg);
        if step == 0 {
            start_dir = -t0;
        }
        end_dir = t1;
        at = if forward { c.b } else { c.a };
        chain.push(ProfileSegment {
            geometry: seg,
            name: c.name.clone(),
        });
    }
    let last = at;
    Ok((chain, first, last, start_dir, end_dir))
}

/// A segment run the other way.
pub(super) fn reverse_segment(s: &Segment2) -> Segment2 {
    match s {
        Segment2::Line { a, b } => Segment2::Line { a: *b, b: *a },
        Segment2::Arc { c, r, a0, sweep } => Segment2::Arc {
            c: *c,
            r: *r,
            a0: a0 + sweep,
            sweep: -sweep,
        },
        Segment2::Spline {
            points,
            reversed,
            knots,
        } => Segment2::Spline {
            points: points.clone(),
            reversed: !reversed,
            knots: knots.clone(),
        },
    }
}

/// Unit tangents at the start and the end of a segment, in its direction.
fn segment_tangents(s: &Segment2) -> (Vector2, Vector2) {
    match s {
        Segment2::Line { a, b } => {
            let d = (b - a).try_normalize(1e-12).unwrap_or_else(Vector2::x);
            (d, d)
        }
        Segment2::Arc { a0, sweep, .. } => {
            let sg = sweep.signum();
            let t = |a: Real| Vector2::new(-a.sin(), a.cos()) * sg;
            (t(*a0), t(a0 + sweep))
        }
        Segment2::Spline {
            points, reversed, ..
        } => {
            let (p0, p1, q0, q1) = match points.as_slice() {
                [a, b, .., c, d] => (*a, *b, *c, *d),
                [a, b] => (*a, *b, *a, *b),
                _ => return (Vector2::x(), Vector2::x()),
            };
            let (s, e) = (
                (p1 - p0).try_normalize(1e-12).unwrap_or_else(Vector2::x),
                (q1 - q0).try_normalize(1e-12).unwrap_or_else(Vector2::x),
            );
            if *reversed { (-e, -s) } else { (s, e) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::{
        Feature, OriginPlane, PartDocument, PlaneRef, SketchOptions, SketchPlacement,
    };

    /// A box 80×60×20 and a sketch on the XY origin plane (z = 0) with a
    /// circle inside the box's footprint; returns the part, sketch index,
    /// the circle's profile and its topology key.
    fn box_and_circle() -> (PartDocument, usize, ProfileRef, String) {
        let mut part = PartDocument::with_box("Box1", [80.0, 60.0, 20.0]).unwrap();
        let placement = SketchPlacement {
            plane: PlaneRef::Origin(OriginPlane::XY),
            ..Default::default()
        };
        let opts = SketchOptions {
            auto_project_origin: false,
            ..Default::default()
        };
        let i = part.create_sketch(placement, &opts).unwrap();
        let sk = &mut part.sketches[i].sketch;
        let c = sk.add_circle(Point2::new(10.0, 10.0), 5.0).unwrap();
        let r = sk.regions();
        let pr = ProfileRef::of(&r[0], r[0].interior_point());
        (part, i, pr, entity_key(c))
    }

    /// z of the start (`S`) or end (`E`) vertex of the circle in `id`.
    fn z(part: &PartDocument, id: &str, level: &str, key: &str) -> Real {
        let v = part
            .body
            .find_vertex(&format!("{id}:V{level}:{key}:0"))
            .unwrap_or_else(|| panic!("{id}:V{level}"));
        part.body.vertex(v).unwrap().point.z
    }

    /// A region bounded by an interpolation spline (non-uniform knots) and
    /// a line extrudes into a valid solid.
    #[test]
    fn extrudes_a_region_with_an_interpolation_spline() {
        let mut part = PartDocument::default();
        let placement = SketchPlacement {
            plane: PlaneRef::Origin(OriginPlane::XY),
            ..Default::default()
        };
        let opts = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let i = part.create_sketch(placement, &opts).unwrap();
        let sk = &mut part.sketches[i].sketch;
        let pts: Vec<_> = [(0.0, 0.0), (8.0, 12.0), (30.0, 9.0), (40.0, 0.0)]
            .iter()
            .map(|&(x, y)| sk.add_point(Point2::new(x, y)))
            .collect();
        sk.add_interp_spline(pts.clone()).unwrap();
        sk.add_line_between(pts[3], pts[0]).unwrap();
        let r = sk.regions();
        assert_eq!(r.len(), 1);
        let pr = ProfileRef::of(&r[0], r[0].interior_point());
        part.add_extrude(i, ExtrudeFeature::new(vec![pr], 5.0))
            .unwrap();
        let (sid, _) = part.body.solids().next().unwrap();
        part.body.validate_solid(sid).unwrap();

        // A closed interpolation spline alone is a profile too.
        let i = part
            .create_sketch(
                SketchPlacement {
                    plane: PlaneRef::Origin(OriginPlane::XY),
                    ..Default::default()
                },
                &opts,
            )
            .unwrap();
        let sk = &mut part.sketches[i].sketch;
        let pts: Vec<_> = [(0.0, 30.0), (20.0, 32.0), (25.0, 50.0), (5.0, 45.0)]
            .iter()
            .map(|&(x, y)| sk.add_point(Point2::new(x, y)))
            .collect();
        let s = sk.add_interp_spline(pts).unwrap();
        sk.set_spline_closed(s, true).unwrap();
        let r = sk.regions();
        assert_eq!(r.len(), 1);
        let pr = ProfileRef::of(&r[0], r[0].interior_point());
        part.add_extrude(i, ExtrudeFeature::new(vec![pr], 5.0))
            .unwrap();
        for (sid, _) in part.body.solids() {
            part.body.validate_solid(sid).unwrap();
        }
    }

    /// Inventor's **Solids**: a Cut limited to one body leaves the other
    /// alone; the bodies keep their names (the creating feature's id).
    #[test]
    fn cut_only_the_chosen_body() {
        let mut part = PartDocument::with_box("Box1", [80.0, 60.0, 20.0]).unwrap();
        let opts = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let on_xy = || SketchPlacement {
            plane: PlaneRef::Origin(OriginPlane::XY),
            ..Default::default()
        };
        // A second body: a cylinder beside the box.
        let i = part.create_sketch(on_xy(), &opts).unwrap();
        let sk = &mut part.sketches[i].sketch;
        sk.add_circle(Point2::new(120.0, 30.0), 10.0).unwrap();
        let r = sk.regions();
        let pr = ProfileRef::of(&r[0], r[0].interior_point());
        let mut cyl = ExtrudeFeature::new(vec![pr], 20.0);
        cyl.operation = crate::part::Operation::NewSolid;
        let cyl_id = part.add_extrude(i, cyl).unwrap();
        // A slab over both, cut 5 mm deep into the cylinder only.
        let j = part.create_sketch(on_xy(), &opts).unwrap();
        let sk = &mut part.sketches[j].sketch;
        sk.add_rectangle_two_point(Point2::new(-10.0, -10.0), Point2::new(150.0, 70.0))
            .unwrap();
        let r = sk.regions();
        let pr = ProfileRef::of(&r[0], r[0].interior_point());
        let mut cut = ExtrudeFeature::new(vec![pr], 5.0);
        cut.operation = crate::part::Operation::Cut;
        cut.solids = vec![cyl_id.clone()];
        part.add_extrude(j, cut).unwrap();
        let vol = |name: &str| {
            let (sid, _) = part
                .body
                .solids()
                .find(|(_, s)| s.name.as_deref() == Some(name))
                .unwrap();
            let mut t = Vec::new();
            crate::kernel::csg::volume(&part.body.solid_polygons(sid, &mut t).unwrap())
        };
        assert!(
            (vol("Box1") - 80.0 * 60.0 * 20.0).abs() < 1e-6,
            "box untouched"
        );
        let exact = std::f64::consts::PI * 100.0 * 15.0;
        assert!((vol(&cyl_id) - exact).abs() / exact < 0.01, "cylinder cut");
        assert_eq!(
            part.bodies_before(part.features.len()),
            vec![
                ("Box1".to_owned(), "Solid1".to_owned()),
                (cyl_id.clone(), "Solid2".to_owned())
            ]
        );
        // A chosen body that no longer exists makes the feature sick.
        if let crate::part::Feature::Extrude(e) = &mut part.features[2] {
            e.solids = vec!["Gone".into()];
        }
        part.regenerate().unwrap();
        assert!(
            part.feature_health
                .values()
                .any(|m| m.contains("no longer exists"))
        );
    }

    #[test]
    fn directions_extents_from_and_taper() {
        type Case = (Box<dyn Fn(&mut ExtrudeFeature)>, Real, Real);
        let cases: Vec<Case> = vec![
            (Box::new(|_| {}), 0.0, 15.0),
            (
                Box::new(|f| f.direction = ExtentDirection::Flipped),
                0.0,
                -15.0,
            ),
            (
                Box::new(|f| f.direction = ExtentDirection::Symmetric),
                -7.5,
                7.5,
            ),
            (
                Box::new(|f| {
                    f.direction = ExtentDirection::Asymmetric;
                    f.distance_b = 4.0;
                }),
                -4.0,
                15.0,
            ),
            (
                Box::new(|f| f.extent = Extent::To(TargetRef::Face("Box1:Top".into()))),
                0.0,
                20.0,
            ),
            (
                Box::new(|f| f.extent = Extent::To(TargetRef::Vertex("Box1:V111".into()))),
                0.0,
                20.0,
            ),
            (Box::new(|f| f.extent = Extent::ToNext), 0.0, 20.0),
            (Box::new(|f| f.extent = Extent::ThroughAll), 0.0, 20.0),
            (
                // Between: from the top face to 5 mm above it.
                Box::new(|f| f.from = Some(TargetRef::Face("Box1:Top".into()))),
                20.0,
                35.0,
            ),
        ];
        for (k, (set, z0, z1)) in cases.into_iter().enumerate() {
            let (mut part, i, pr, key) = box_and_circle();
            let mut def = ExtrudeFeature::new(vec![pr], 15.0);
            set(&mut def);
            let id = part
                .add_extrude(i, def)
                .unwrap_or_else(|e| panic!("case {k}: {e}"));
            assert!(
                (z(&part, &id, "S", &key) - z0).abs() < 1e-9,
                "case {k} start"
            );
            assert!((z(&part, &id, "E", &key) - z1).abs() < 1e-9, "case {k} end");
        }
        // Taper: the circle grows by 15·tan(10°) at the end.
        let (mut part, i, pr, key) = box_and_circle();
        let mut def = ExtrudeFeature::new(vec![pr], 15.0);
        def.taper = 10f64.to_radians();
        let id = part.add_extrude(i, def).unwrap();
        let v = part.body.find_vertex(&format!("{id}:VE:{key}:0")).unwrap();
        let p = part.body.vertex(v).unwrap().point;
        let r = ((p.x - 10.0).powi(2) + (p.y - 10.0).powi(2)).sqrt();
        assert!((r - (5.0 + 15.0 * 10f64.to_radians().tan())).abs() < 1e-9);
        // Errors: To a face that is not parallel; nothing behind for To Next.
        let (mut part, i, pr, _) = box_and_circle();
        let mut def = ExtrudeFeature::new(vec![pr.clone()], 15.0);
        def.extent = Extent::To(TargetRef::Face("Box1:Front".into()));
        assert!(part.add_extrude(i, def).is_err());
        let mut def = ExtrudeFeature::new(vec![pr], 15.0);
        def.extent = Extent::ToNext;
        def.direction = ExtentDirection::Flipped;
        assert!(part.add_extrude(i, def).is_err());
    }

    #[test]
    fn consumed_and_shared_sketches() {
        let (mut part, i, pr, _) = box_and_circle();
        part.add_extrude(i, ExtrudeFeature::new(vec![pr.clone()], 5.0))
            .unwrap();
        let ps = &part.sketches[i];
        assert!(!ps.visible && !ps.shared, "consumed and hidden");
        assert_eq!(part.owner_of_sketch(i), Some(1));
        // A second feature from the same sketch shares it.
        part.add_extrude(i, ExtrudeFeature::new(vec![pr], 3.0))
            .unwrap();
        assert!(part.sketches[i].shared);
        assert_eq!(part.owner_of_sketch(i), None);
        assert!(part.unshare_sketch(i).is_err(), "two features use it");
        part.features.pop();
        part.unshare_sketch(i).unwrap();
        assert_eq!(part.owner_of_sketch(i), Some(1));
    }

    #[test]
    fn extrusion_follows_the_sketch_and_names_its_faces() {
        let mut part = PartDocument::with_box("Box1", [80.0, 60.0, 20.0]).unwrap();
        let placement = SketchPlacement {
            plane: PlaneRef::Face("Box1:Top".into()),
            ..Default::default()
        };
        let opts = SketchOptions {
            auto_reference_edges: false,
            ..Default::default()
        };
        let i = part.create_sketch(placement, &opts).unwrap();
        let sk = &mut part.sketches[i].sketch;
        let c = sk.add_circle(Point2::new(10.0, 10.0), 5.0).unwrap();
        let regions = sk.regions();
        assert_eq!(regions.len(), 1);
        let pr = ProfileRef::of(&regions[0], Point2::new(10.0, 10.0));
        let id = part
            .add_extrude(i, ExtrudeFeature::new(vec![pr], 15.0))
            .unwrap();
        assert_eq!(id, "Extrusion1");
        let end = part.body.find_face("Extrusion1:End").expect("named cap");
        let z = part
            .body
            .vertex(
                part.body
                    .find_vertex(&format!("Extrusion1:VE:{}:0", entity_key(c)))
                    .unwrap(),
            )
            .unwrap()
            .point
            .z;
        assert!((z - 35.0).abs() < 1e-9, "box top 20 + 15: {z}");
        let _ = end;
        // Taller box: the sketch moves with its face, the cylinder follows.
        if let Some(Feature::Box(b)) = part.features.first_mut() {
            b.size[2] = 30.0;
        }
        part.regenerate().unwrap();
        assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
        let v = part
            .body
            .find_vertex(&format!("Extrusion1:VE:{}:0", entity_key(c)))
            .unwrap();
        assert!((part.body.vertex(v).unwrap().point.z - 45.0).abs() < 1e-9);
        // A region that is gone makes the feature unhealthy, not the part broken.
        part.sketches[i].sketch.remove_entity(c).unwrap();
        part.regenerate().unwrap();
        assert!(part.feature_health.contains_key("Extrusion1"));
        // Adding an extrusion that cannot be built is refused.
        assert!(
            part.add_extrude(i, ExtrudeFeature::new(vec![], 5.0))
                .is_err()
        );
        // The feature survives saving and loading.
        let json = serde_json::to_string(&part).unwrap();
        let mut back: PartDocument = serde_json::from_str(&json).unwrap();
        back.regenerate().unwrap();
        assert_eq!(back.features, part.features);
    }
}

#[cfg(test)]
mod default_profile_tests {
    use super::default_profiles;
    use crate::part::{
        ExtrudeFeature, OriginPlane, PartDocument, ProfileRef, SketchOptions, SketchPlacement,
    };
    use crate::sketch::Point2;

    /// 40 rectangles drawn on a box's face (its edges projected): all 40
    /// are the starting profiles, not the face's outline region.
    #[test]
    fn many_rectangles_on_a_face_are_all_taken() {
        let mut part = PartDocument::default();
        let opts = SketchOptions {
            auto_project_origin: false,
            ..Default::default()
        };
        let i = part
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)
            .unwrap();
        part.sketches[i]
            .sketch
            .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(100.0, 50.0))
            .unwrap();
        let r = part.sketches[i].sketch.regions();
        part.add_extrude(
            i,
            ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], 5.0),
        )
        .unwrap();
        let with_edges = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: true,
            ..Default::default()
        };
        let j = part
            .create_sketch(SketchPlacement::on_face("Extrusion1:End"), &with_edges)
            .unwrap();
        let plane = part.sketches[j].sketch.plane.clone();
        for a in 0..10 {
            for b in 0..4 {
                let (x, y) = (2.0 + 10.0 * a as f64, 2.0 + 12.0 * b as f64);
                let p = plane.project(&crate::kernel::Point3::new(x, y, 5.0));
                let q = plane.project(&crate::kernel::Point3::new(x + 6.0, y + 8.0, 5.0));
                part.sketches[j]
                    .sketch
                    .add_rectangle_two_point(p, q)
                    .unwrap();
            }
        }
        let regions = part.sketches[j].sketch.regions();
        assert_eq!(
            regions.len(),
            41,
            "40 rectangles and the face outline with 40 holes"
        );
        // The shading of the outline region covers exactly its area.
        for r in &regions {
            let outer = r.outer.polygon(0.05);
            let holes: Vec<_> = r.holes.iter().map(|h| h.polygon(0.05)).collect();
            let (pts, tris) = crate::kernel::triangulate_with_holes(&outer, &holes);
            let tri: f64 = tris
                .iter()
                .map(|t| ((pts[t[1]] - pts[t[0]]).perp(&(pts[t[2]] - pts[t[0]])) / 2.0).abs())
                .sum();
            assert!((tri - r.area()).abs() < 1e-6, "{tri} vs {}", r.area());
        }
        let picked = default_profiles(&part.sketches[j], &regions);
        assert_eq!(picked.len(), 40);
    }

    /// A plate with a hole drawn by the user: the ring only (the circle
    /// inside fills its hole).
    #[test]
    fn a_plate_with_a_hole_keeps_the_hole() {
        let mut part = PartDocument::default();
        let opts = SketchOptions {
            auto_project_origin: false,
            ..Default::default()
        };
        let i = part
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)
            .unwrap();
        let sk = &mut part.sketches[i].sketch;
        sk.add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(40.0, 30.0))
            .unwrap();
        sk.add_circle(Point2::new(20.0, 15.0), 5.0).unwrap();
        let regions = sk.regions();
        assert_eq!(regions.len(), 2);
        let picked = default_profiles(&part.sketches[i], &regions);
        assert_eq!(picked.len(), 1);
        let ring = regions.iter().find(|r| !r.holes.is_empty()).unwrap();
        assert_eq!(
            picked[0].sources,
            ProfileRef::of(ring, ring.interior_point()).sources
        );
    }
}
