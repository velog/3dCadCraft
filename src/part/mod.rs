//! # Part modeling environment
//!
//! A [`PartDocument`] holds the features that build the body, the body
//! itself (a regenerable cache), and the part's sketches.
//!
//! This module implements Inventor's **Start 2D Sketch** workflow:
//!
//! 1. pick an origin plane or a planar face → [`SketchPlacement`];
//! 2. the sketch gets its coordinate system (origin, x/y axes), see
//!    [`placement`];
//! 3. per the application options ([`SketchOptions`], Inventor's
//!    *Tools → Options → Sketch*), the **part origin** and the **edges of the
//!    selected face** are projected into the sketch as fixed reference
//!    geometry;
//! 4. the sketch is **associative**: after every regeneration
//!    ([`PartDocument::regenerate`]) it re-finds its face by persistent name,
//!    moves with it, and updates its projected geometry — user geometry
//!    constrained to projected points follows. If a source disappears the
//!    sketch is flagged (`health`) and the projections freeze in place,
//!    as Inventor does;
//! 5. later edits: **Redefine** (move to another plane/face, keeping the 2D
//!    geometry) and **Edit Coordinate System** (new origin/axis; geometry
//!    stays where it is in 3D).

pub mod appearance;
pub mod blend;
pub mod boolean;
pub mod coil;
pub mod combine;
pub mod draft;
mod exact;
pub mod export;
pub mod expr;
pub mod extrude;
pub mod history;
pub mod hole;
pub mod link;
#[cfg(test)]
mod link_tests;
pub mod loft;
pub mod measure;
#[cfg(test)]
mod option_tests;
pub mod params;
pub mod pattern;
pub mod physical;
pub mod placement;
pub mod primitive;
pub mod revolve;
pub mod rib;
pub mod select;
pub mod sheet_metal;
pub mod shell;
pub mod split;
pub mod standards;
pub mod sweep;
pub mod units;
pub mod work;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use blend::{
    BlendFeature, BlendKind, ChamferMethod, EdgeRef, FilletSet, FilletType, PartialChamfer,
    VariableRadius,
};
pub use boolean::{Operation, Output};
pub use coil::{CoilAxis, CoilEnd, CoilFeature, CoilMethod};
pub use combine::CombineFeature;
pub use draft::{DraftExtent, DraftFeature, DraftType, PartingSide, PartingTool, PullRef};
pub use extrude::{Extent, ExtentDirection, ExtrudeFeature, OpenProfile, ProfileRef, TargetRef};
pub use hole::{
    ClearanceSpec, CosmeticThread, DrillPoint, FastenerFit, HoleCenter, HoleFeature, HoleGroup,
    HoleInstance, HoleSpec, HoleType, SeatType, Termination, ThreadBands, ThreadFeature,
    ThreadSpec,
};
pub use link::{GeometryLink, LinkSource, SketchGeo};
pub use loft::{
    ConditionKind, LoftCondition, LoftFeature, LoftGuide, LoftSection, MapPoint, PlacedSection,
};
pub use pattern::{PatternDirection, PatternFeature, PatternKind};
pub use placement::{OriginPlane, PlacementError, PlaneRef, PointRef, SketchPlacement};
pub use primitive::{PrimitiveFeature, PrimitiveKind};
pub use revolve::{AxisRef, RevolveExtent, RevolveFeature};
pub use rib::{DraftHold, RibBoss, RibDirection, RibExtent, RibFeature, RibSide};
pub use shell::{FaceRef, ShellCorners, ShellDirection, ShellFeature};
pub use split::{SplitFeature, SplitMethod, SplitToolRef};
pub use sweep::{PathRef, ProfileScaling, SweepFeature, SweepOrientation, SweepType};
pub use work::{
    CurveRef, LineRef, UcsDef, WorkAxisDef, WorkDef, WorkFeature, WorkPlaneDef, WorkPointDef,
};

use crate::kernel::{BRep, KernelError, Plane, Point3, Real};
use crate::sketch::{Entity, EntityId, Point2, PointId, Sketch, SketchError};

/// Sketch-related application options (Inventor: Tools → Application
/// Options → Sketch tab; API `SketchOptions`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SketchOptions {
    /// Project the part origin as a fixed point on sketch creation
    /// (`AutoProjectPartOrigin`).
    pub auto_project_origin: bool,
    /// Project the edges of the selected face as reference geometry
    /// (`AutomaticReferenceEdges`).
    pub auto_reference_edges: bool,
    /// Turn the view to look straight at a new or edited sketch
    /// (`ParallelViewOnSketchCreationInPart`).
    pub look_at_on_create: bool,
    /// Show the sketch's X/Y axes (`DisplayAxes`).
    pub show_axes: bool,
    /// Show the coordinate-system indicator (`DisplayCoordinateSystemIndicator`).
    pub show_triad: bool,
    /// Show grid lines on the sketch plane (`DisplayGridLines`).
    pub show_grid: bool,
}

impl Default for SketchOptions {
    fn default() -> Self {
        Self {
            auto_project_origin: true,
            auto_reference_edges: true,
            look_at_on_create: true,
            show_axes: true,
            show_triad: true,
            show_grid: true,
        }
    }
}

/// A box primitive at the part origin (Inventor: 3D Model → Primitives →
/// Box). Its faces, edges and vertices are named `"<id>:…"`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoxFeature {
    /// Immutable identity, the prefix of its topology names. Never changes
    /// after creation, so renaming the feature cannot break references to
    /// its faces — from sketches here, or from assembly constraints and
    /// drawings that reference this part.
    pub id: String,
    /// Display name shown in the browser; may be renamed freely.
    pub name: String,
    /// Size along X, Y, Z (mm).
    pub size: [Real; 3],
    /// Equations of the sizes (`length`, `width`, `height`).
    #[serde(default, skip_serializing_if = "params::FeatureParams::is_empty")]
    pub params: params::FeatureParams,
}

/// A feature in the part's history. More kinds (revolve, hole…) arrive
/// with the feature tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Feature {
    /// Box primitive.
    Box(BoxFeature),
    /// Extrusion of sketch regions.
    Extrude(ExtrudeFeature),
    /// Revolve of sketch regions around an axis.
    Revolve(RevolveFeature),
    /// A work plane, axis or point (no body).
    Work(WorkFeature),
    /// Fillet or Chamfer of model edges.
    Blend(BlendFeature),
    /// Shell: hollows a solid (format revision 3).
    Shell(ShellFeature),
    /// Face Draft: tilts faces against a pull direction (format revision 4).
    Draft(DraftFeature),
    /// Rectangular / Circular Pattern or Mirror of features or solids
    /// (format revision 5).
    Pattern(PatternFeature),
    /// Hole: drilled, clearance or tapped holes with seats (format revision 6).
    Hole(HoleFeature),
    /// Thread: a cosmetic or modeled thread on a cylindrical face (format
    /// revision 6).
    Thread(ThreadFeature),
    /// Combine: a base body with toolbodies (format revision 13).
    Combine(CombineFeature),
    /// Split: Split Face, Trim Solid, Split Solid (format revision 13).
    Split(SplitFeature),
    /// Sweep: profiles along a path (format revision 7).
    Sweep(SweepFeature),
    /// Loft: a body through sections (format revision 7).
    Loft(LoftFeature),
    /// Coil: a profile screwed about an axis (format revision 14).
    Coil(CoilFeature),
    /// Rib / Web: thin walls from open profiles (format revision 14).
    Rib(RibFeature),
    /// Sheet metal Face: a plate from a sketch profile (format revision 19).
    SheetFace(sheet_metal::SheetFaceFeature),
    /// Sheet metal Flange: bends on sheet edges (format revision 19).
    SheetFlange(sheet_metal::SheetFlangeFeature),
    /// Convert to Sheet Metal of a solid (`sheet_metal.rs`).
    SheetConvert(sheet_metal::SheetConvertFeature),
    /// Hem, Fold, Contour Flange, Unfold / Refold, Cut (`sheet_metal.rs`).
    SheetOp(sheet_metal::SheetOpFeature),
}

/// A sketch-based solid feature (Extrude or Revolve), as regeneration
/// treats them alike.
#[derive(Clone, Copy)]
enum Sweep<'a> {
    Extrude(&'a ExtrudeFeature),
    Revolve(&'a RevolveFeature),
}

/// Results of exact build steps by key (the step's feature, its sketch as
/// resolved, and every step before it): regeneration rebuilds only from the
/// first changed feature on, like Inventor. Shared by clones of the part
/// (previews computed on worker threads fill it too). Runtime only.
#[derive(Clone, Default)]
pub struct BuildCache(std::sync::Arc<std::sync::Mutex<Vec<CachedStep>>>);

#[derive(Clone)]
struct CachedStep {
    key: u64,
    solids: Vec<crate::kernel::exact::ExactSolid>,
    body: BRep,
    health: Option<String>,
    /// The tools the step applied (Extrude, Revolve, feature patterns), so
    /// a later pattern still finds them when this step is a cache hit.
    tools: Vec<pattern::ToolSet>,
    /// Holes and threads the step made (Hole / Thread features).
    extra: exact::HoleOutput,
    /// Health of other features (the modeled threads' finishing step).
    healths: Vec<(String, String)>,
    /// The sheet metal models after the step (by sheet body).
    metal: HashMap<String, sheet_metal::SheetState>,
}

impl std::fmt::Debug for BuildCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BuildCache")
    }
}

/// The exact solids of a part (runtime only).
#[derive(Clone, Default)]
pub struct ExactBodies(pub Vec<crate::kernel::exact::ExactSolid>);

impl std::fmt::Debug for ExactBodies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ExactBodies({})", self.0.len())
    }
}

impl BuildCache {
    /// Steps kept (most recent last).
    const SIZE: usize = 64;

    fn get(&self, key: u64) -> Option<CachedStep> {
        let v = self.0.lock().ok()?;
        v.iter().rev().find(|c| c.key == key).cloned()
    }

    fn put(&self, step: CachedStep) {
        if let Ok(mut v) = self.0.lock() {
            v.retain(|c| c.key != step.key);
            v.push(step);
            let n = v.len();
            if n > Self::SIZE {
                v.drain(..n - Self::SIZE);
            }
        }
    }
}

/// The key of a build step: the previous step's key, the feature and the
/// sketch it uses (geometry and resolved plane — not its visibility or
/// other display state, which changes without changing the body).
fn step_key(prev: u64, feature: &Feature, sketches: &[&crate::sketch::Sketch]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    prev.hash(&mut h);
    serde_json::to_vec(feature).unwrap_or_default().hash(&mut h);
    for ps in sketches {
        serde_json::to_vec(ps).unwrap_or_default().hash(&mut h);
    }
    h.finish()
}

/// How regeneration builds a feature (the sweeps share one path).
#[derive(Clone, Copy)]
enum BuildStep<'a> {
    Box(&'a BoxFeature),
    Sweep(Sweep<'a>),
    Work(&'a WorkFeature),
    Blend(&'a BlendFeature),
    Shell(&'a ShellFeature),
    Draft(&'a DraftFeature),
    Pattern(&'a PatternFeature),
    Hole(&'a HoleFeature),
    Thread(&'a ThreadFeature),
    Combine(&'a CombineFeature),
    Split(&'a SplitFeature),
    Swept(&'a SweepFeature),
    Loft(&'a LoftFeature),
    Coil(&'a CoilFeature),
    Rib(&'a RibFeature),
    SheetFace(&'a sheet_metal::SheetFaceFeature),
    SheetFlange(&'a sheet_metal::SheetFlangeFeature),
    SheetConvert(&'a sheet_metal::SheetConvertFeature),
    SheetOp(&'a sheet_metal::SheetOpFeature),
}

impl<'a> BuildStep<'a> {
    fn of(f: &'a Feature) -> Self {
        match f {
            Feature::Box(b) => Self::Box(b),
            Feature::Extrude(e) => Self::Sweep(Sweep::Extrude(e)),
            Feature::Revolve(r) => Self::Sweep(Sweep::Revolve(r)),
            Feature::Work(w) => Self::Work(w),
            Feature::Blend(bl) => Self::Blend(bl),
            Feature::Shell(sh) => Self::Shell(sh),
            Feature::Draft(d) => Self::Draft(d),
            Feature::Pattern(p) => Self::Pattern(p),
            Feature::Hole(h) => Self::Hole(h),
            Feature::Thread(t) => Self::Thread(t),
            Feature::Combine(c) => Self::Combine(c),
            Feature::Split(sp) => Self::Split(sp),
            Feature::Sweep(s) => Self::Swept(s),
            Feature::Loft(l) => Self::Loft(l),
            Feature::Coil(c) => Self::Coil(c),
            Feature::Rib(r) => Self::Rib(r),
            Feature::SheetFace(f) => Self::SheetFace(f),
            Feature::SheetFlange(f) => Self::SheetFlange(f),
            Feature::SheetConvert(f) => Self::SheetConvert(f),
            Feature::SheetOp(f) => Self::SheetOp(f),
        }
    }
}

impl Feature {
    /// Immutable identity (topology-name prefix).
    pub fn id(&self) -> &str {
        match self {
            Feature::Box(b) => &b.id,
            Feature::Extrude(e) => &e.id,
            Feature::Revolve(r) => &r.id,
            Feature::Work(w) => &w.id,
            Feature::Blend(b) => &b.id,
            Feature::Shell(s) => &s.id,
            Feature::Draft(d) => &d.id,
            Feature::Pattern(p) => &p.id,
            Feature::Hole(h) => &h.id,
            Feature::Thread(t) => &t.id,
            Feature::Combine(c) => &c.id,
            Feature::Split(sp) => &sp.id,
            Feature::Sweep(s) => &s.id,
            Feature::Loft(l) => &l.id,
            Feature::Coil(c) => &c.id,
            Feature::Rib(r) => &r.id,
            Feature::SheetFace(f) => &f.id,
            Feature::SheetFlange(f) => &f.id,
            Feature::SheetConvert(f) => &f.id,
            Feature::SheetOp(f) => &f.id,
        }
    }

    /// Display name.
    pub fn name(&self) -> &str {
        match self {
            Feature::Box(b) => &b.name,
            Feature::Extrude(e) => &e.name,
            Feature::Revolve(r) => &r.name,
            Feature::Work(w) => &w.name,
            Feature::Blend(b) => &b.name,
            Feature::Shell(s) => &s.name,
            Feature::Draft(d) => &d.name,
            Feature::Pattern(p) => &p.name,
            Feature::Hole(h) => &h.name,
            Feature::Thread(t) => &t.name,
            Feature::Combine(c) => &c.name,
            Feature::Split(sp) => &sp.name,
            Feature::Sweep(s) => &s.name,
            Feature::Loft(l) => &l.name,
            Feature::Coil(c) => &c.name,
            Feature::Rib(r) => &r.name,
            Feature::SheetFace(f) => &f.name,
            Feature::SheetFlange(f) => &f.name,
            Feature::SheetConvert(f) => &f.name,
            Feature::SheetOp(f) => &f.name,
        }
    }

    /// Renames it (the id, and so every persistent name, stays).
    pub fn set_name(&mut self, name: &str) {
        let n = match self {
            Feature::Box(b) => &mut b.name,
            Feature::Extrude(e) => &mut e.name,
            Feature::Revolve(r) => &mut r.name,
            Feature::Work(w) => &mut w.name,
            Feature::Blend(b) => &mut b.name,
            Feature::Shell(s) => &mut s.name,
            Feature::Draft(d) => &mut d.name,
            Feature::Pattern(p) => &mut p.name,
            Feature::Hole(h) => &mut h.name,
            Feature::Thread(t) => &mut t.name,
            Feature::Combine(c) => &mut c.name,
            Feature::Split(sp) => &mut sp.name,
            Feature::Sweep(s) => &mut s.name,
            Feature::Loft(l) => &mut l.name,
            Feature::Coil(c) => &mut c.name,
            Feature::Rib(r) => &mut r.name,
            Feature::SheetFace(f) => &mut f.name,
            Feature::SheetFlange(f) => &mut f.name,
            Feature::SheetConvert(f) => &mut f.name,
            Feature::SheetOp(f) => &mut f.name,
        };
        *n = name.to_owned();
    }

    /// Whether it creates a body of its own (a box, or New Solid).
    pub fn creates_body(&self) -> bool {
        match self {
            Feature::Box(_) => true,
            Feature::Extrude(e) => e.operation == Operation::NewSolid,
            Feature::Revolve(r) => r.operation == Operation::NewSolid,
            Feature::Sweep(s) => s.operation == Operation::NewSolid,
            Feature::Loft(l) => l.operation == Operation::NewSolid,
            Feature::Coil(c) => c.operation == Operation::NewSolid,
            Feature::Rib(_) => false,
            Feature::SheetFace(_) => true,
            Feature::SheetFlange(_) | Feature::SheetConvert(_) => false,
            Feature::SheetOp(f) => f.creates_body(),
            Feature::Work(_)
            | Feature::Blend(_)
            | Feature::Shell(_)
            | Feature::Draft(_)
            | Feature::Pattern(_) => false,
            Feature::Hole(_) | Feature::Thread(_) | Feature::Combine(_) => false,
            Feature::Split(sp) => sp.method == SplitMethod::SplitSolid,
        }
    }

    /// `id` of the sketch it consumes, if any.
    pub fn sketch(&self) -> Option<&str> {
        match self {
            Feature::Box(_)
            | Feature::Work(_)
            | Feature::Blend(_)
            | Feature::Shell(_)
            | Feature::Pattern(_) => None,
            Feature::Draft(d) => d.parting_sketch(),
            Feature::Thread(_) | Feature::Combine(_) => None,
            Feature::Split(sp) => sp.sketch(),
            Feature::Hole(h) => (!h.sketch.is_empty()).then_some(h.sketch.as_str()),
            Feature::Extrude(e) => Some(&e.sketch),
            Feature::Revolve(r) => Some(&r.sketch),
            Feature::Sweep(s) => Some(&s.sketch),
            Feature::Loft(l) => l.sketch_ids().first().copied(),
            Feature::Coil(c) => Some(&c.sketch),
            Feature::Rib(r) => Some(&r.sketch),
            Feature::SheetFace(f) => Some(&f.sketch),
            Feature::SheetFlange(_) | Feature::SheetConvert(_) => None,
            Feature::SheetOp(f) => f.sketch(),
        }
    }

    /// `id` of every sketch it uses (a sweep's profile and path sketches, a
    /// loft's sections); the first is [`Feature::sketch`].
    pub fn sketch_ids(&self) -> Vec<&str> {
        match self {
            Feature::Sweep(s) => s.sketch_ids(),
            Feature::Loft(l) => l.sketch_ids(),
            f => f.sketch().into_iter().collect(),
        }
    }
}

/// A model point projected into a sketch as a fixed point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Projection {
    /// The fixed sketch point.
    pub point: PointId,
    /// Where it comes from.
    pub source: PointRef,
}

fn yes() -> bool {
    true
}

/// A named sketch of a part, with its placement and projected geometry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartSketch {
    /// Immutable identity, referenced by features (e.g. an extrusion's
    /// profile sketch); `name` may be renamed freely. Older files have none:
    /// [`PartDocument::regenerate`] fills it from the name.
    #[serde(default)]
    pub id: String,
    /// Display name, e.g. "Sketch1".
    pub name: String,
    /// Plane and coordinate-system definition.
    #[serde(default)]
    pub placement: SketchPlacement,
    /// The sketch geometry and constraints (in sketch coordinates).
    pub sketch: Sketch,
    /// Projected model points (fixed, updated on regeneration).
    #[serde(default)]
    pub projections: Vec<Projection>,
    /// Place in the history: the number of features that existed when the
    /// sketch was created (the browser lists it after them). `None` (older
    /// files): after all features.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_features: Option<usize>,
    /// Consumed by a feature but shared: listed on its own and available to
    /// further features (Inventor's Share Sketch).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shared: bool,
    /// Lines that are projected face edges (reference geometry).
    #[serde(default)]
    pub reference_lines: Vec<EntityId>,
    /// Projected geometry of other sketches and curved model edges
    /// (associative, see [`link`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<GeometryLink>,
    /// Visibility in the graphics window.
    #[serde(default = "yes")]
    pub visible: bool,
    /// Problem found at the last regeneration (e.g. its face vanished).
    #[serde(skip)]
    pub health: Option<String>,
}

impl PartSketch {
    /// A visible sketch without model references.
    pub fn new(name: impl Into<String>, sketch: Sketch) -> Self {
        let name = name.into();
        Self {
            id: name.clone(),
            name,
            placement: SketchPlacement::default(),
            sketch,
            projections: Vec::new(),
            after_features: None,
            shared: false,
            reference_lines: Vec::new(),
            links: Vec::new(),
            visible: true,
            health: None,
        }
    }

    /// Is `p` a projected (reference) point?
    pub fn is_projected(&self, p: PointId) -> bool {
        self.projections.iter().any(|pr| pr.point == p)
            || self.links.iter().any(|l| l.points.contains(&p))
    }

    /// Is curve `e` projected (reference) geometry: a projected straight
    /// edge or a linked curve? It cannot be trimmed or edited.
    pub fn is_reference(&self, e: EntityId) -> bool {
        self.reference_lines.contains(&e) || self.links.iter().any(|l| l.curve == Some(e))
    }

    /// The link whose geometry `target` is (a linked point or curve).
    pub fn link_of(&self, target: RefTarget) -> Option<usize> {
        self.links.iter().position(|l| match target {
            RefTarget::Point(p) => l.curve.is_none() && l.points.first() == Some(&p),
            RefTarget::Curve(c) | RefTarget::Line(c) => l.curve == Some(c),
        })
    }

    /// The reference row target of linked point `p` or linked curve `e`:
    /// the link that owns it.
    pub fn link_target_of_point(&self, p: PointId) -> Option<RefTarget> {
        self.links
            .iter()
            .find(|l| l.points.contains(&p))
            .map(|l| match l.curve {
                Some(c) => RefTarget::Curve(c),
                None => RefTarget::Point(p),
            })
    }

    fn source_of(&self, p: PointId) -> Option<&PointRef> {
        self.projections
            .iter()
            .find(|pr| pr.point == p)
            .map(|pr| &pr.source)
    }

    /// The sketch's references (Properties → References): one row per
    /// projected edge, then per projected point that is not an edge end.
    pub fn references(&self, body: &BRep, works: &placement::WorkGeom) -> Vec<Reference> {
        let alive = |src: Option<&PointRef>| {
            src.is_some_and(|s| placement::point_of(s, body, works).is_ok())
        };
        let mut rows = Vec::new();
        let mut ends = std::collections::HashSet::new();
        for &line in &self.reference_lines {
            let Ok((a, b)) = self.sketch.line_points(line) else {
                continue;
            };
            ends.insert(a);
            ends.insert(b);
            let (sa, sb) = (self.source_of(a), self.source_of(b));
            rows.push(Reference {
                target: RefTarget::Line(line),
                source: format!(
                    "Edge {} - {}",
                    sa.map_or("?".into(), describe),
                    sb.map_or("?".into(), describe)
                ),
                ok: alive(sa) && alive(sb),
                points: vec![a, b],
                label: None,
            });
        }
        for pr in &self.projections {
            if ends.contains(&pr.point) {
                continue;
            }
            rows.push(Reference {
                target: RefTarget::Point(pr.point),
                source: match &pr.source {
                    PointRef::PartOrigin => "Origin: Center Point".into(),
                    other => other.describe(),
                },
                ok: alive(Some(&pr.source)),
                points: vec![pr.point],
                label: None,
            });
        }
        for l in &self.links {
            let kind = match l.curve.and_then(|c| self.sketch.entity(c)) {
                Some(Entity::Line { .. }) => "Line",
                Some(Entity::Circle { .. }) => "Circle",
                Some(Entity::Arc { .. }) => "Arc",
                Some(Entity::Spline { .. }) => "Spline",
                None => "Point",
            };
            let from = match &l.source {
                LinkSource::Sketch { sketch, .. } => sketch.clone(),
                LinkSource::Edge { edge } => edge.faces[0]
                    .split(':')
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
                LinkSource::Work { id } => id.clone(),
            };
            let what = match &l.source {
                LinkSource::Sketch { .. } => kind.to_owned(),
                LinkSource::Edge { .. } => format!("edge ({})", kind.to_lowercase()),
                LinkSource::Work { .. } => format!("work feature ({})", kind.to_lowercase()),
            };
            rows.push(Reference {
                target: match l.curve {
                    Some(c) => RefTarget::Curve(c),
                    None => RefTarget::Point(l.points[0]),
                },
                source: match &l.source {
                    LinkSource::Sketch { sketch, .. } => format!("{sketch}: {kind}"),
                    LinkSource::Edge { edge } => {
                        format!("Edge {} / {} ({kind})", edge.faces[0], edge.faces[1])
                    }
                    LinkSource::Work { id } => format!("{id} ({kind})"),
                },
                ok: l.status.is_none(),
                points: l.points.clone(),
                label: Some(format!("{from} · {what}")),
            });
        }
        rows
    }

    /// Drops the link of a projected point (it stays where it is, free).
    fn unlink_point(&mut self, p: PointId) -> Result<(), SketchError> {
        self.projections.retain(|pr| pr.point != p);
        self.sketch.set_fixed(p, false)
    }

    /// Points of reference lines other than `line`.
    fn other_line_ends(&self, line: EntityId) -> Vec<PointId> {
        self.reference_lines
            .iter()
            .filter(|&&l| l != line)
            .filter_map(|&l| self.sketch.line_points(l).ok())
            .flat_map(|(a, b)| [a, b])
            .collect()
    }

    /// **Break Link**: the referenced geometry becomes ordinary sketch
    /// geometry, no longer updated from the model. An edge's end points
    /// stay linked while another reference edge uses them.
    pub fn break_link(&mut self, target: RefTarget) -> Result<(), SketchError> {
        if let Some(k) = self.link_of(target) {
            let l = self.links.remove(k);
            for p in l.points {
                self.sketch.set_fixed(p, false)?;
            }
            if let Some(c) = l.curve
                && self.sketch.is_radius_fixed(c)
            {
                self.sketch.set_radius_fixed(c, false)?;
            }
            return Ok(());
        }
        match target {
            RefTarget::Curve(_) => Err(SketchError::UnknownEntity),
            RefTarget::Point(p) => self.unlink_point(p),
            RefTarget::Line(line) => {
                let (a, b) = self.sketch.line_points(line)?;
                let keep = self.other_line_ends(line);
                self.reference_lines.retain(|&l| l != line);
                for p in [a, b] {
                    if !keep.contains(&p) {
                        self.unlink_point(p)?;
                    }
                }
                Ok(())
            }
        }
    }

    /// **Delete** a reference: the projected geometry goes, with its
    /// constraints; end points shared with other curves stay.
    pub fn delete_reference(&mut self, target: RefTarget) -> Result<(), SketchError> {
        if let Some(k) = self.link_of(target) {
            let l = self.links.remove(k);
            if let Some(c) = l.curve {
                self.sketch.remove_entity(c)?;
            }
            for p in l.points {
                if self.sketch.point(p).is_none() || self.is_projected(p) {
                    continue;
                }
                let used = self.sketch.entities().any(|(_, e)| e.points().contains(&p));
                if used {
                    // User curves built on it keep the point (free now);
                    // the constraints that used the projection go.
                    let doomed: Vec<_> = self
                        .sketch
                        .constraints()
                        .filter(|(_, c)| c.references_point(p))
                        .map(|(id, _)| id)
                        .collect();
                    for c in doomed {
                        self.sketch.remove_constraint(c)?;
                    }
                    self.sketch.set_fixed(p, false)?;
                } else {
                    self.sketch.remove_point(p)?;
                }
            }
            return Ok(());
        }
        match target {
            RefTarget::Curve(_) => Err(SketchError::UnknownEntity),
            RefTarget::Point(p) => {
                self.projections.retain(|pr| pr.point != p);
                self.sketch.remove_point(p)
            }
            RefTarget::Line(line) => {
                let (a, b) = self.sketch.line_points(line)?;
                let keep = self.other_line_ends(line);
                self.reference_lines.retain(|&l| l != line);
                self.sketch.remove_entity(line)?;
                for p in [a, b] {
                    let used = self.sketch.entities().any(|(_, e)| e.points().contains(&p));
                    if !keep.contains(&p) && !used {
                        self.projections.retain(|pr| pr.point != p);
                        self.sketch.remove_point(p)?;
                    }
                }
                Ok(())
            }
        }
    }
}

/// What a reference row stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefTarget {
    /// A projected point.
    Point(PointId),
    /// A projected edge (reference line).
    Line(EntityId),
    /// A linked curve (from another sketch or a curved model edge).
    Curve(EntityId),
}

/// One projected reference of a sketch, for the References list.
#[derive(Clone, Debug, PartialEq)]
pub struct Reference {
    /// The projected geometry.
    pub target: RefTarget,
    /// Where it comes from, e.g. `Edge Extrusion1:V... - ...`.
    pub source: String,
    /// The source exists in the model (else the geometry is frozen).
    pub ok: bool,
    /// Its sketch points (for highlighting).
    pub points: Vec<PointId>,
    /// Short label for the list (`Sketch1 · Circle`); `None`: the GUI
    /// derives one from `source`.
    pub label: Option<String>,
}

/// Errors of part-modeling operations.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum PartError {
    /// The placement cannot be resolved.
    #[error(transparent)]
    Placement(#[from] PlacementError),
    /// The kernel rejected an operation.
    #[error(transparent)]
    Kernel(#[from] KernelError),
    /// No sketch with that index.
    #[error("no sketch #{0}")]
    NoSketch(usize),
    /// A feature could not be built.
    #[error("{0}")]
    Feature(String),
}

/// A single part: features, the body they produce, and sketches.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PartDocument {
    /// History of features that build `body`. Empty for parts whose body
    /// was stored directly (older files); then `body` is kept as is.
    #[serde(default)]
    pub features: Vec<Feature>,
    /// Sketches in creation order.
    pub sketches: Vec<PartSketch>,
    /// The B-Rep body (a cache of `features` when there are any).
    pub body: BRep,
    /// Features that failed at the last regeneration (id → problem); they
    /// are skipped, the rest of the part still builds (Inventor's sick
    /// features).
    #[serde(skip)]
    pub feature_health: HashMap<String, String>,
    /// User parameters (Parameters → Add Numeric).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub user_params: Vec<params::UserParam>,
    /// Next model-parameter number (`d<N>`).
    #[serde(default)]
    pub next_param: usize,
    /// Key flag and display precision per parameter name (the Parameters
    /// table; renames carry them).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub param_meta: std::collections::BTreeMap<String, params::ParamMeta>,
    /// The part's material (`None`: Generic), see [`appearance`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    /// Sheet Metal Defaults: set for a sheet metal part (5.6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet_metal: Option<sheet_metal::SheetMetalDefaults>,
    /// The sheet metal models of the last regeneration, by sheet body
    /// (runtime only).
    #[serde(skip)]
    pub sheet_models: HashMap<String, sheet_metal::SheetState>,
    /// The part's appearance (`None`: As Material).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<String>,
    /// Appearance/material overrides by persistent name and the document
    /// copies of the definitions in use.
    #[serde(default, skip_serializing_if = "appearance::Looks::is_empty")]
    pub looks: appearance::Looks,
    /// Bodies hidden in the graphics window, by persistent name (Combine's
    /// kept toolbodies; a body's Visibility in the browser).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_bodies: Vec<String>,
    /// Parameters whose equation failed at the last evaluation.
    #[serde(skip)]
    pub param_errors: HashMap<String, String>,
    /// Mass (kg), volume, area, centre of gravity of the last regeneration
    /// that needed them (`params::PHYSICAL`; runtime only).
    #[serde(skip)]
    pub physical_values: Option<[Real; 6]>,
    /// Work planes, axes and points as computed at the last regeneration
    /// (by feature id); sick ones are missing.
    #[serde(skip)]
    pub work: placement::WorkGeom,
    /// Whether the last regeneration used the exact kernel (`false`: the
    /// faceted one, see [`PartDocument::regenerate`]).
    #[serde(skip)]
    pub exact: bool,
    /// Why the last regeneration left the exact kernel: the feature id and
    /// the kernel's message (runtime only). The faceted rebuild shows it as
    /// that feature's health instead of "needs the exact geometry kernel".
    #[serde(skip)]
    pub exact_failure: Option<(String, String)>,
    /// Edit Feature rolls the part back: only the features before this
    /// index are built (Inventor shows the model as it was before the
    /// edited feature). Runtime only.
    #[serde(skip)]
    pub rollback: Option<usize>,
    /// **End of Part** (Inventor's EOP marker): only the features before
    /// this index are built; the rest are rolled back (greyed in the
    /// browser) and new features are inserted here. `None`: after the last
    /// feature. Composes with [`PartDocument::rollback`] (the smaller one
    /// wins). See [`history`] (format revision 7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eop: Option<usize>,
    /// Suppressed features by id: kept in the history but skipped by
    /// regeneration (Inventor's Suppress Features). See [`history`]
    /// (format revision 7).
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub suppressed: std::collections::BTreeSet<String>,
    /// Suppress if: per feature id, a true / false equation; while it is
    /// true the feature (and what depends on it) is suppressed (format
    /// revision 18).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub suppress_if: std::collections::BTreeMap<String, String>,
    /// The features [`Self::suppress_if`] suppressed at the last evaluation
    /// (with their dependents; runtime).
    #[serde(skip)]
    pub cond_suppressed: std::collections::BTreeSet<String>,
    /// Exact build steps already made (see [`BuildCache`]).
    #[serde(skip)]
    pub build_cache: BuildCache,
    /// The exact solids of the last exact regeneration (empty when the
    /// faceted kernel built the part): STEP export reads them.
    #[serde(skip)]
    pub exact_bodies: ExactBodies,
    /// The holes of the last regeneration, in feature order (see
    /// [`PartDocument::holes`]). Runtime only.
    #[serde(skip)]
    pub hole_instances: Vec<HoleInstance>,
    /// Cosmetic threads of the last regeneration (display hints). Runtime
    /// only.
    #[serde(skip)]
    pub cosmetic_threads: Vec<CosmeticThread>,
}

impl PartDocument {
    /// A cheap signature of the body (no walk over face meshes): the
    /// vertices' box, the element counts, the mesh sizes and the hidden
    /// bodies. It changes on every rebuild; caches of display or picking
    /// data are keyed by it.
    pub fn body_signature(&self) -> String {
        let body = &self.body;
        let bbox = body
            .vertices()
            .fold(None, |b: Option<(Point3, Point3)>, (_, v)| {
                Some(b.map_or((v.point, v.point), |(lo, hi)| {
                    (lo.inf(&v.point), hi.sup(&v.point))
                }))
            });
        let mesh: usize = body
            .faces()
            .filter_map(|(_, f)| f.mesh.as_ref())
            .map(|m| m.positions.len())
            .sum();
        format!(
            "{}|{}|{}|{mesh}|{bbox:?}|{:?}",
            body.edges().count(),
            body.faces().count(),
            body.vertices().count(),
            self.hidden_bodies
        )
    }

    /// A part consisting of one box primitive.
    pub fn with_box(name: impl Into<String>, size: [Real; 3]) -> Result<Self, PartError> {
        let name = name.into();
        let mut part = Self {
            features: vec![Feature::Box(BoxFeature {
                id: name.clone(),
                name,
                size,
                params: Default::default(),
            })],
            ..Default::default()
        };
        part.regenerate()?;
        Ok(part)
    }

    /// The first box feature, for editing its size.
    pub fn box_feature_mut(&mut self) -> Option<&mut BoxFeature> {
        self.features.iter_mut().find_map(|f| match f {
            Feature::Box(b) => Some(b),
            _ => None,
        })
    }

    /// Rebuilds the body from the features, then updates every sketch
    /// (placement and projections) — the associativity step.
    ///
    /// The exact kernel (`kernel::exact`) builds the part when it covers
    /// every feature; otherwise (an option it lacks, or a kernel failure)
    /// the whole part is rebuilt with the faceted kernel.
    pub fn regenerate(&mut self) -> Result<(), PartError> {
        for ps in &mut self.sketches {
            if ps.id.is_empty() {
                ps.id = ps.name.clone();
            }
        }
        // Parameters first: equations set dimensions and feature values.
        self.sync_parameters();
        self.evaluate_parameters();
        self.exact_failure = None;
        self.exact = exact::enabled() && self.build_features(true)?;
        if !self.exact {
            self.build_features(false)?;
            // A feature only the exact kernel builds, which made it give up:
            // its health says why (a kernel failure), not that it needs the
            // exact kernel.
            if let Some((id, msg)) = &self.exact_failure
                && let Some(h) = self.feature_health.get_mut(id)
                && h.contains("needs the exact geometry kernel")
            {
                *h = format!("the geometry kernel failed: {msg}");
            }
        }
        // Equations on the physical properties: measured on the new body,
        // then those user parameters evaluated again (no size depends on
        // them, `evaluate_parameters` refuses that).
        if self.uses_physical() {
            self.update_physical();
            self.evaluate_parameters();
        }
        Ok(())
    }

    /// Builds the body with the exact kernel (`use_exact`) or the faceted
    /// one, then updates the sketches. Returns `false` (and leaves the part
    /// for a faceted rebuild) when the exact kernel had to give up.
    fn build_features(&mut self, use_exact: bool) -> Result<bool, PartError> {
        use crate::kernel::exact::{ExactSolid, to_brep};
        self.feature_health.clear();
        let mut works = placement::WorkGeom::new();
        let mut body = BRep::new();
        // The exact solids so far (exact build only).
        let mut solids: Vec<ExactSolid> = Vec::new();
        // Edit Feature's rollback and the End of Part: the smaller wins.
        let limit = self.build_limit();
        let mut prev_key = 0u64;
        // The tools of the features built so far (by id), for patterns.
        let mut tools: HashMap<String, Vec<pattern::ToolSet>> = HashMap::new();
        // Holes, cosmetic and modeled threads of the features so far.
        let mut hole_out = exact::HoleOutput::default();
        // Sheet metal models by sheet body.
        let mut metal: HashMap<String, sheet_metal::SheetState> = HashMap::new();
        let rule = sheet_metal::rule_of(self);
        for (j, f) in self.features.iter().enumerate().take(limit) {
            // Sketches made at this point of the history follow the body as
            // it was then (Inventor), not the final one.
            for k in 0..self.sketches.len() {
                if self.sketches[k].after_features == Some(j) {
                    update_sketch(&body, &works, &mut self.sketches, k);
                }
            }
            // The feature's own sketches follow the body built so far.
            for id in f.sketch_ids() {
                if let Some(k) = self.sketches.iter().position(|s| s.id == id) {
                    update_sketch(&body, &works, &mut self.sketches, k);
                }
            }
            // Suppressed: as if it were not in the history (its key is not
            // chained either, so the following steps match a history
            // without it).
            if self.suppressed.contains(f.id()) || self.cond_suppressed.contains(f.id()) {
                continue;
            }
            let used: Vec<&crate::sketch::Sketch> = f
                .sketch_ids()
                .into_iter()
                .filter_map(|id| self.sketches.iter().find(|s| s.id == id))
                .map(|ps| &ps.sketch)
                .collect();
            let mut key = step_key(prev_key, f, &used);
            // Sheet metal steps also follow the rule (a new thickness
            // rebuilds them).
            if matches!(
                f,
                Feature::SheetFace(_)
                    | Feature::SheetFlange(_)
                    | Feature::SheetConvert(_)
                    | Feature::SheetOp(_)
            ) {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                key.hash(&mut h);
                serde_json::to_vec(&rule).unwrap_or_default().hash(&mut h);
                key = h.finish();
            }
            prev_key = key;
            let cacheable = use_exact && !matches!(f, Feature::Work(_));
            if cacheable && let Some(hit) = self.build_cache.get(key) {
                solids = hit.solids;
                body = hit.body;
                if let Some(h) = hit.health {
                    self.feature_health.insert(f.id().to_owned(), h);
                }
                if !hit.tools.is_empty() {
                    tools.insert(f.id().to_owned(), hit.tools);
                }
                hole_out.append(hit.extra);
                metal = hit.metal;
                continue;
            }
            let started = std::time::Instant::now();
            // The tools this step applies (kept for later patterns).
            let mut made: Vec<pattern::ToolSet> = Vec::new();
            let mut extra = exact::HoleOutput::default();
            // Surface bodies (exact sheets) never take part in another
            // feature's Booleans or blends: only Split sees them (its tool).
            let sheets: Vec<ExactSolid> = if use_exact && !matches!(f, Feature::Split(_)) {
                let (open, closed): (Vec<ExactSolid>, Vec<ExactSolid>) =
                    std::mem::take(&mut solids)
                        .into_iter()
                        .partition(ExactSolid::is_open);
                solids = closed;
                open
            } else {
                Vec::new()
            };
            // `Ok(Some(solids))`: the new exact solids; `Ok(None)`: done by
            // the faceted kernel (`body` updated); `Err`: the feature fails.
            let step: Result<Option<Vec<ExactSolid>>, exact::Fail> = match BuildStep::of(f) {
                BuildStep::Work(w) => {
                    let sketches = &self.sketches;
                    let features = &self.features;
                    let revolve_axis = |id: &str| {
                        features.iter().find_map(|f| match f {
                            Feature::Revolve(r) if r.id == id => {
                                let ps = sketches.iter().find(|s| s.id == r.sketch)?;
                                r.world_axis(&ps.sketch).ok()
                            }
                            _ => None,
                        })
                    };
                    let ctx = work::Ctx {
                        body: &body,
                        works: &works,
                        revolve_axis: &revolve_axis,
                        sketches,
                    };
                    match w.build(&ctx) {
                        Ok(r) => {
                            r.store(&mut works);
                            Ok(None)
                        }
                        Err(msg) => Err(exact::Fail::User(msg)),
                    }
                }
                BuildStep::Blend(bl) if use_exact => exact::blend(bl, &body, &solids).map(Some),
                BuildStep::Combine(c) if use_exact => combine::build_exact(c, &solids).map(Some),
                BuildStep::Combine(c) => match combine::build_faceted(c, &mut body) {
                    Ok(()) => Ok(None),
                    Err(msg) => Err(exact::Fail::User(msg)),
                },
                BuildStep::Split(sp) if use_exact => {
                    split::build_exact(sp, &body, &works, &self.sketches, &solids).map(Some)
                }
                BuildStep::Split(_) => Err(exact::Fail::User(
                    "Split needs the exact geometry kernel (an earlier feature uses an option it does not cover yet)".into(),
                )),
                BuildStep::Shell(sh) if use_exact => exact::shell(sh, &solids).map(Some),
                BuildStep::Draft(dr) if use_exact => {
                    let lines = dr
                        .parting_sketch()
                        .and_then(|id| self.sketches.iter().find(|s| s.id == id))
                        .map(|ps| draft::sketch_lines(&ps.sketch));
                    exact::draft(dr, &body, &works, &solids, lines.as_deref()).map(Some)
                }
                BuildStep::Pattern(p) if use_exact => {
                    let sources = if p.solid_mode {
                        Ok(Vec::new())
                    } else {
                        pattern_sources(&self.features[..j], p, &tools, &self.feature_health)
                    };
                    match sources {
                        Ok(sources) => {
                            exact::pattern(p, &body, &works, &solids, &sources).map(|(next, t)| {
                                made = t;
                                // Copies of holes are holes too (Hole Table,
                                // `4x` notes): each source hole moved by
                                // each copy's placement.
                                if !p.solid_mode
                                    && let Ok(places) = p.placements(&body, &works)
                                {
                                    let src: Vec<HoleInstance> = hole_out
                                        .holes
                                        .iter()
                                        .filter(|h| p.features.contains(&h.feature))
                                        .cloned()
                                        .collect();
                                    // Their cosmetic threads too (the
                                    // thread texture of every copy).
                                    let threads: Vec<CosmeticThread> = hole_out
                                        .cosmetic
                                        .iter()
                                        .filter(|c| p.features.contains(&c.feature))
                                        .cloned()
                                        .collect();
                                    for (k, m) in &places {
                                        for h in &src {
                                            // The copy starts on the face
                                            // its tool meets (the body's,
                                            // e.g. the same top face).
                                            let mut c = h.transformed(m);
                                            c.copy.push(*k);
                                            extra.holes.push(c);
                                        }
                                        for c in &threads {
                                            extra.cosmetic.push(c.transformed(m));
                                        }
                                    }
                                }
                                Some(next)
                            })
                        }
                        Err(msg) => Err(exact::Fail::User(msg)),
                    }
                }
                BuildStep::Pattern(p) => Err(exact::Fail::User(format!(
                    "{} needs the exact geometry kernel (an earlier feature uses an option it does not cover yet)",
                    p.command()
                ))),
                BuildStep::Hole(h) if use_exact => {
                    let ps = self.sketches.iter().find(|s| s.id == h.sketch);
                    exact::hole(h, ps, &body, &solids).map(|(next, out, sets)| {
                        extra = out;
                        made = sets;
                        Some(next)
                    })
                }
                BuildStep::Hole(_) => Err(exact::Fail::User(
                    "Hole needs the exact geometry kernel (an earlier feature uses an option it does not cover yet)".into(),
                )),
                BuildStep::Thread(th) if use_exact || !th.spec.modeled => {
                    match exact::thread(th, &body) {
                        Ok((out, ring)) => {
                            extra = out;
                            match ring {
                                Some(r) => exact::combine(solids.clone(), vec![r], Operation::Cut, &[], false)
                                    .map(Some),
                                None => Ok(None),
                            }
                        }
                        Err(e) => Err(e),
                    }
                }
                BuildStep::Thread(_) => Err(exact::Fail::User(
                    "a modeled thread needs the exact geometry kernel (an earlier feature uses an option it does not cover yet)".into(),
                )),
                BuildStep::Swept(sw) if use_exact => match sw.build(&self.sketches, &body) {
                    Ok(added) => {
                        made.push(pattern::ToolSet {
                            tools: added.clone(),
                            op: sw.operation,
                            solids: sw.solids.clone(),
                        });
                        exact::combine(solids.clone(), added, sw.operation, &sw.solids, false)
                            .map(Some)
                            .map_err(exact::as_user)
                    }
                    Err(e) => Err(e),
                },
                BuildStep::Loft(lo) if use_exact => match lo.build(&self.sketches, &body) {
                    Ok(added) => {
                        made.push(pattern::ToolSet {
                            tools: added.clone(),
                            op: lo.operation,
                            solids: lo.solids.clone(),
                        });
                        exact::combine(solids.clone(), added, lo.operation, &lo.solids, false)
                            .map(Some)
                            .map_err(exact::as_user)
                    }
                    Err(e) => Err(e),
                },
                BuildStep::Coil(co) if use_exact => match self.sketches.iter().find(|s| s.id == co.sketch) {
                    None => Err(exact::Fail::User(format!("sketch {} is missing", co.sketch))),
                    Some(ps) => match co.build_exact(&ps.sketch, &body, &works, &solids) {
                        Ok(Some((next, tools))) => {
                            made.push(pattern::ToolSet {
                                tools,
                                op: co.operation,
                                solids: co.solids.clone(),
                            });
                            Ok(Some(next))
                        }
                        // Surface output: the faceted kernel builds the part.
                        Ok(None) => return Ok(false),
                        Err(e) => Err(e),
                    },
                },
                BuildStep::Coil(co) => match self.sketches.iter().find(|s| s.id == co.sketch) {
                    None => Err(exact::Fail::User(format!("sketch {} is missing", co.sketch))),
                    Some(ps) => {
                        let mut trial = body.clone();
                        let before: std::collections::HashSet<_> =
                            trial.solids().map(|(id, _)| id).collect();
                        match co
                            .build_faceted(&mut trial, &ps.sketch, &works)
                            .and_then(|()| boolean::apply(&mut trial, &before, co.operation, &co.solids))
                        {
                            Ok(()) => {
                                body = trial;
                                Ok(None)
                            }
                            Err(msg) => Err(exact::Fail::User(msg)),
                        }
                    }
                },
                BuildStep::Rib(rb) if use_exact => match self.sketches.iter().find(|s| s.id == rb.sketch) {
                    None => Err(exact::Fail::User(format!("sketch {} is missing", rb.sketch))),
                    Some(ps) => match rb.build_exact(&ps.sketch, &body) {
                        Ok(added) => {
                            made.push(pattern::ToolSet {
                                tools: added.clone(),
                                op: Operation::Join,
                                solids: rb.solids.clone(),
                            });
                            exact::combine(solids.clone(), added, Operation::Join, &rb.solids, false)
                                .map(Some)
                                .map_err(exact::as_user)
                        }
                        Err(e) => Err(e),
                    },
                },
                BuildStep::Rib(_) => Err(exact::Fail::User(
                    "Rib needs the exact geometry kernel (an earlier feature uses an option it does not cover yet)".into(),
                )),
                BuildStep::SheetFace(sf) if use_exact => {
                    match self.sketches.iter().find(|s| s.id == sf.sketch) {
                        None => Err(exact::Fail::User(format!("sketch {} is missing", sf.sketch))),
                        Some(ps) => match exact::sheet_face(sf, &ps.sketch, &rule) {
                            Ok((solid, state)) => {
                                metal.insert(sf.id.clone(), state);
                                let mut next = solids.clone();
                                next.push(solid);
                                Ok(Some(next))
                            }
                            Err(e) => Err(e),
                        },
                    }
                }
                BuildStep::SheetFlange(fl) if use_exact => {
                    match exact::sheet_flange(fl, &solids, &metal, &rule) {
                        Ok((next, state)) => {
                            metal.insert(fl.body.clone(), state);
                            Ok(Some(next))
                        }
                        Err(e) => Err(e),
                    }
                }
                BuildStep::SheetConvert(sc) if use_exact => {
                    match exact::sheet_convert(sc, &solids, &rule) {
                        Ok((next, state)) => {
                            metal.insert(sc.body.clone(), state);
                            Ok(Some(next))
                        }
                        Err(e) => Err(e),
                    }
                }
                BuildStep::SheetOp(op) if use_exact => {
                    match exact::sheet_op(op, &solids, &metal, &rule, &self.sketches) {
                        Ok((next, body, state)) => {
                            metal.insert(body, state);
                            Ok(Some(next))
                        }
                        Err(e) => Err(e),
                    }
                }
                BuildStep::SheetFace(_)
                | BuildStep::SheetFlange(_)
                | BuildStep::SheetConvert(_)
                | BuildStep::SheetOp(_) => Err(exact::Fail::User(
                    "sheet metal needs the exact geometry kernel (an earlier feature uses an option it does not cover yet)".into(),
                )),
                BuildStep::Swept(_) => Err(exact::Fail::User(
                    "Sweep needs the exact geometry kernel (an earlier feature uses an option it does not cover yet)".into(),
                )),
                BuildStep::Loft(_) => Err(exact::Fail::User(
                    "Loft needs the exact geometry kernel (an earlier feature uses an option it does not cover yet)".into(),
                )),
                BuildStep::Draft(_) => Err(exact::Fail::User(
                    "Face Draft needs the exact geometry kernel (an earlier feature uses an option it does not cover yet)".into(),
                )),
                // The faceted kernel has no shell (and never gets one).
                BuildStep::Shell(_) => Err(exact::Fail::User(
                    "Shell needs the exact geometry kernel (an earlier feature uses an option it does not cover yet)".into(),
                )),
                BuildStep::Blend(bl) => {
                    // On a copy: a failure leaves the body as it was.
                    let mut trial = body.clone();
                    match bl.build(&mut trial) {
                        Ok(()) => {
                            body = trial;
                            Ok(None)
                        }
                        Err(msg) => Err(exact::Fail::User(msg)),
                    }
                }
                BuildStep::Box(b) => {
                    let [x, y, z] = b.size;
                    let (lo, hi) = (Point3::origin(), Point3::new(x, y, z));
                    if use_exact {
                        crate::kernel::exact::box_named(lo, hi, &b.id)
                            .map(|s| {
                                let mut next = solids.clone();
                                next.push(s);
                                Some(next)
                            })
                            .map_err(exact::Fail::Kernel)
                    } else {
                        let s = body.make_box_named(lo, hi, &b.id)?;
                        body.name_solid(s, &b.id);
                        Ok(None)
                    }
                }
                BuildStep::Sweep(sweep) => {
                    let sketch_id = f.sketch().unwrap_or_default();
                    let (op, chosen) = match sweep {
                        Sweep::Extrude(e) => (e.operation, &e.solids),
                        Sweep::Revolve(r) => (r.operation, &r.solids),
                    };
                    match self.sketches.iter().find(|s| s.id == sketch_id) {
                        None => Err(exact::Fail::User(format!("sketch {sketch_id} is missing"))),
                        Some(ps) if use_exact => {
                            let built = match sweep {
                                Sweep::Extrude(e) => exact::extrude(e, &body, &works, &ps.sketch),
                                Sweep::Revolve(r) => exact::revolve(r, &body, &works, &ps.sketch),
                            };
                            let surface = match sweep {
                                Sweep::Extrude(e) => e.output == Output::Surface,
                                Sweep::Revolve(r) => r.output == Output::Surface,
                            };
                            match built {
                                // Surface output: new surface bodies, no
                                // operation (and nothing to pattern).
                                Ok(Some(added)) if surface => {
                                    let mut next = solids.clone();
                                    next.extend(added);
                                    Ok(Some(next))
                                }
                                Ok(Some(added)) => {
                                    made.push(pattern::ToolSet {
                                        tools: added.clone(),
                                        op,
                                        solids: chosen.clone(),
                                    });
                                    exact::combine(solids.clone(), added, op, chosen, false)
                                        .map(Some)
                                }
                                // An option the exact kernel lacks.
                                Ok(None) => return Ok(false),
                                Err(e) => Err(e),
                            }
                        }
                        Some(ps) => {
                            // Build on a copy: a failure leaves the body as it was.
                            let mut trial = body.clone();
                            let before: std::collections::HashSet<_> =
                                trial.solids().map(|(id, _)| id).collect();
                            let built = match sweep {
                                Sweep::Extrude(e) => e.build_in(&mut trial, &works, &ps.sketch),
                                Sweep::Revolve(r) => r.build_in(&mut trial, &works, &ps.sketch),
                            };
                            // New bodies are named after this feature.
                            let added: Vec<_> = trial
                                .solids()
                                .map(|(id, _)| id)
                                .filter(|id| !before.contains(id))
                                .collect();
                            for id in added {
                                trial.name_solid(id, f.id());
                            }
                            match built
                                .and_then(|()| boolean::apply(&mut trial, &before, op, chosen))
                            {
                                Ok(()) => {
                                    body = trial;
                                    Ok(None)
                                }
                                Err(msg) => Err(exact::Fail::User(msg)),
                            }
                        }
                    }
                }
            };
            // The surface bodies go back (after the solids).
            let step = step.map(|o| {
                o.map(|mut next| {
                    next.extend(sheets.iter().cloned());
                    next
                })
            });
            solids.extend(sheets);
            let took = started.elapsed();
            if took.as_millis() > 200 {
                log::info!(
                    "regenerate {} ({} kernel): {} ms",
                    f.id(),
                    if use_exact { "exact" } else { "faceted" },
                    took.as_millis()
                );
            }
            let mut failed = match &step {
                Err(exact::Fail::User(msg)) => Some(msg.clone()),
                _ => None,
            };
            if step.is_err() {
                made.clear();
            }
            match step {
                Ok(Some(next)) => match crate::kernel::exact::to_brep_with(
                    &next,
                    matches!(
                        f,
                        Feature::Sweep(_) | Feature::Loft(_) | Feature::Coil(_) | Feature::Rib(_)
                    ),
                ) {
                    // A result the kernel made but that is not a valid solid
                    // (a non-manifold edge: a hemisphere shell, rounds wider
                    // than the wall; FreeCAD #19184, #27918) is never kept.
                    Ok(b) => match invalid_solid(&b) {
                        None => {
                            solids = next;
                            body = b;
                        }
                        Some(why)
                            if matches!(
                                f,
                                Feature::Extrude(_) | Feature::Revolve(_) | Feature::Box(_)
                            ) =>
                        {
                            log::warn!("exact kernel: {}: invalid result: {why}", f.id());
                            self.exact_failure =
                                Some((f.id().to_owned(), format!("invalid result ({why})")));
                            return Ok(false);
                        }
                        Some(why) => {
                            let msg = format!("the geometry kernel made an invalid solid ({why})");
                            self.feature_health.insert(f.id().to_owned(), msg.clone());
                            failed = Some(msg);
                        }
                    },
                    // Sweep / Loft have no faceted fallback: their own health.
                    Err(msg)
                        if matches!(
                            f,
                            Feature::Sweep(_)
                                | Feature::Loft(_)
                                | Feature::Coil(_)
                                | Feature::Rib(_)
                        ) =>
                    {
                        let msg =
                            format!("the geometry kernel made a result it cannot show ({msg})");
                        self.feature_health.insert(f.id().to_owned(), msg.clone());
                        failed = Some(msg);
                    }
                    Err(msg) => {
                        log::warn!("exact kernel: {}: {msg}", f.id());
                        self.exact_failure = Some((f.id().to_owned(), msg));
                        return Ok(false);
                    }
                },
                Ok(None) => {}
                Err(exact::Fail::User(msg)) => {
                    self.feature_health.insert(f.id().to_owned(), msg);
                }
                Err(exact::Fail::Kernel(msg)) => {
                    log::warn!("exact kernel: {}: {msg}", f.id());
                    self.exact_failure = Some((f.id().to_owned(), msg));
                    return Ok(false);
                }
            }
            if failed.is_some() {
                extra = exact::HoleOutput::default();
            }
            if cacheable {
                self.build_cache.put(CachedStep {
                    key,
                    solids: solids.clone(),
                    body: body.clone(),
                    health: failed,
                    tools: made.clone(),
                    extra: extra.clone(),
                    healths: Vec::new(),
                    metal: metal.clone(),
                });
            }
            if !made.is_empty() {
                tools.insert(f.id().to_owned(), made);
            }
            hole_out.append(extra);
        }
        // Modeled threads: cut after every feature (their zones are faceted,
        // and no later Boolean may meet them).
        if use_exact && !hole_out.grooves.is_empty() {
            let key = {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                prev_key.hash(&mut h);
                format!("{:?}", hole_out.grooves).hash(&mut h);
                h.finish()
            };
            let (next, failed) = match self.build_cache.get(key) {
                Some(hit) => (Some((hit.solids, hit.body)), hit.healths),
                None => {
                    let (next, failed) = exact::finish_threads(solids.clone(), &hole_out.grooves);
                    match to_brep(&next) {
                        Ok(b) => {
                            self.build_cache.put(CachedStep {
                                key,
                                solids: next.clone(),
                                body: b.clone(),
                                health: None,
                                tools: Vec::new(),
                                extra: exact::HoleOutput::default(),
                                healths: failed.clone(),
                                metal: metal.clone(),
                            });
                            (Some((next, b)), failed)
                        }
                        Err(msg) => (
                            None,
                            hole_out
                                .grooves
                                .iter()
                                .map(|g| {
                                    (
                                        g.prefix.clone(),
                                        format!("the modeled thread cannot be shown ({msg})"),
                                    )
                                })
                                .collect(),
                        ),
                    }
                }
            };
            if let Some((s, b)) = next {
                solids = s;
                body = b;
            }
            for (id, msg) in failed {
                self.feature_health.entry(id).or_insert(msg);
            }
        }
        self.hole_instances = hole_out.holes;
        self.cosmetic_threads = hole_out.cosmetic;
        self.sheet_models = metal;
        if !self.features.is_empty() {
            self.body = body;
        }
        self.exact_bodies = ExactBodies(if use_exact { solids } else { Vec::new() });
        // The rest (made after every feature, or older files) follow the
        // final body.
        let n = self.features.len();
        // Rolled back (Edit Feature or End of Part): the sketches made at
        // the stop follow the body there, and so does the edited feature's
        // own sketch; the later sketches keep their state.
        let edited_sketch = self
            .rollback
            .filter(|&k| k <= limit)
            .and_then(|k| self.features.get(k))
            .and_then(|f| f.sketch().map(str::to_owned));
        for i in 0..self.sketches.len() {
            let ps = &self.sketches[i];
            let follows = match ps.after_features {
                None => limit >= n,
                Some(k) => k == limit || (limit >= n && k >= n),
            } || edited_sketch.as_deref() == Some(ps.id.as_str());
            if follows {
                update_sketch(&self.body, &works, &mut self.sketches, i);
            }
        }
        self.work = works;
        Ok(true)
    }

    /// The solid bodies made by the features before index `k`, in order:
    /// (persistent name, display name `SolidN`). Surface bodies are left out,
    /// and so are toolbodies a Combine before `k` consumed (the numbers of
    /// the others stay). Split Solid makes `<id>#2`.
    pub fn bodies_before(&self, k: usize) -> Vec<(String, String)> {
        let upto = &self.features[..k.min(self.features.len())];
        let consumed: Vec<&String> = upto
            .iter()
            .filter_map(|f| match f {
                Feature::Combine(c) => Some(c.consumed()),
                _ => None,
            })
            .flatten()
            .collect();
        upto.iter()
            .filter(|f| f.creates_body())
            .filter_map(|f| match f {
                Feature::Extrude(e) => (e.output == Output::Solid).then(|| e.id.clone()),
                Feature::Revolve(r) => (r.output == Output::Solid).then(|| r.id.clone()),
                Feature::Box(b) => Some(b.id.clone()),
                Feature::Sweep(s) => (s.output == Output::Solid).then(|| s.id.clone()),
                Feature::Loft(l) => (l.output == Output::Solid).then(|| l.id.clone()),
                Feature::Split(sp) => Some(sp.new_body()),
                Feature::Coil(c) => (c.output == Output::Solid).then(|| c.id.clone()),
                Feature::SheetFace(f) => Some(f.id.clone()),
                Feature::SheetFlange(_) | Feature::SheetConvert(_) => None,
                Feature::SheetOp(f) => f.creates_body().then(|| f.id.clone()),
                Feature::Work(_)
                | Feature::Blend(_)
                | Feature::Shell(_)
                | Feature::Draft(_)
                | Feature::Pattern(_)
                | Feature::Combine(_)
                | Feature::Rib(_) => None,
                Feature::Hole(_) | Feature::Thread(_) => None,
            })
            .enumerate()
            .filter(|(_, id)| !consumed.contains(&id))
            .map(|(n, id)| (id, format!("Solid{}", n + 1)))
            .collect()
    }

    /// `"Extrusion<N>"`, N one more than the highest existing number.
    pub fn next_feature_id(&self, base: &str) -> String {
        let n = self
            .features
            .iter()
            .filter_map(|f| f.id().strip_prefix(base)?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("{base}{}", n + 1)
    }

    /// **Extrude** regions of sketch `index` (a new solid, see
    /// [`extrude`]) with the options of `def` (its id, name and sketch are
    /// assigned here). The sketch is consumed; a sketch already consumed by
    /// another feature becomes shared (Inventor). Returns the feature id; on
    /// failure nothing changes.
    pub fn add_extrude(
        &mut self,
        index: usize,
        mut def: ExtrudeFeature,
    ) -> Result<String, PartError> {
        let sketch_id = self
            .sketches
            .get(index)
            .ok_or(PartError::NoSketch(index))?
            .id
            .clone();
        if def.profiles.is_empty() && def.open.is_none() {
            return Err(PartError::Feature("select at least one profile".into()));
        }
        let id = self.next_feature_id("Extrusion");
        // A name given in the command (breadcrumb rename) is kept.
        let name = if def.name.is_empty() {
            id.clone()
        } else {
            def.name.clone()
        };
        (def.id, def.name, def.sketch) = (id.clone(), name, sketch_id.clone());
        let reused = !self.consumers(&sketch_id).is_empty();
        self.insert_feature_checked(Feature::Extrude(def))?;
        let ps = &mut self.sketches[index];
        if reused {
            ps.shared = true;
        } else {
            ps.visible = false; // consumed sketches are hidden
        }
        Ok(id)
    }

    /// **Revolve** regions of sketch `index` (see [`revolve`]) with the options of `def` (its id, name and sketch are
    /// assigned here). The sketch is consumed; a sketch already consumed by
    /// another feature becomes shared (Inventor). Returns the feature id; on
    /// failure nothing changes.
    pub fn add_revolve(
        &mut self,
        index: usize,
        mut def: RevolveFeature,
    ) -> Result<String, PartError> {
        let sketch_id = self
            .sketches
            .get(index)
            .ok_or(PartError::NoSketch(index))?
            .id
            .clone();
        if def.profiles.is_empty() {
            return Err(PartError::Feature("select at least one profile".into()));
        }
        let id = self.next_feature_id("Revolution");
        let name = if def.name.is_empty() {
            id.clone()
        } else {
            def.name.clone()
        };
        (def.id, def.name, def.sketch) = (id.clone(), name, sketch_id.clone());
        let reused = !self.consumers(&sketch_id).is_empty();
        self.insert_feature_checked(Feature::Revolve(def))?;
        let ps = &mut self.sketches[index];
        if reused {
            ps.shared = true;
        } else {
            ps.visible = false; // consumed sketches are hidden
        }
        Ok(id)
    }

    /// **Sweep** regions of sketch `index` along `def.path` (see [`sweep`];
    /// id, name and profile sketch are assigned here). The profile and path
    /// sketches are consumed (hidden), or shared when another feature uses
    /// them already. Returns the feature id; on failure nothing changes.
    pub fn add_sweep(&mut self, index: usize, mut def: SweepFeature) -> Result<String, PartError> {
        let sketch_id = self
            .sketches
            .get(index)
            .ok_or(PartError::NoSketch(index))?
            .id
            .clone();
        if def.profiles.is_empty() {
            return Err(PartError::Feature("select at least one profile".into()));
        }
        if def.path.is_empty() {
            return Err(PartError::Feature("select the path".into()));
        }
        let id = self.next_feature_id("Sweep");
        (def.id, def.name, def.sketch) = (id.clone(), id.clone(), sketch_id);
        let f = Feature::Sweep(def);
        let used: Vec<String> = f.sketch_ids().into_iter().map(str::to_owned).collect();
        self.add_consuming(f, &used)?;
        Ok(id)
    }

    /// **Loft** through `def.sections` (see [`loft`]; id and name are
    /// assigned here). Its sketches are consumed like a sweep's. Returns the
    /// feature id; on failure nothing changes.
    pub fn add_loft(&mut self, mut def: LoftFeature) -> Result<String, PartError> {
        if def.sections.len() < 2 {
            return Err(PartError::Feature("select at least two sections".into()));
        }
        let id = self.next_feature_id("Loft");
        (def.id, def.name) = (id.clone(), id.clone());
        let f = Feature::Loft(def);
        let used: Vec<String> = f.sketch_ids().into_iter().map(str::to_owned).collect();
        self.add_consuming(f, &used)?;
        Ok(id)
    }

    /// Adds `f` that consumes the sketches `used`: rebuilt, rolled
    /// back on failure; each sketch is hidden, or shared when another
    /// feature used it already.
    fn add_consuming(&mut self, f: Feature, used: &[String]) -> Result<(), PartError> {
        let reused: Vec<bool> = used.iter().map(|s| !self.consumers(s).is_empty()).collect();
        self.insert_feature_checked(f)?;
        for (sid, reused) in used.iter().zip(reused) {
            if let Some(ps) = self.sketches.iter_mut().find(|s| &s.id == sid) {
                if reused {
                    ps.shared = true;
                } else {
                    ps.visible = false;
                }
            }
        }
        Ok(())
    }

    /// **Edit Feature**: replaces feature `k` by `def` (keeping its id,
    /// name, sketch and place in the history) and rebuilds. If the new
    /// definition fails, the part is left as it was and the reason
    /// returned. Clears a rollback.
    pub fn replace_feature(&mut self, k: usize, mut def: Feature) -> Result<String, PartError> {
        let old = self
            .features
            .get(k)
            .cloned()
            .ok_or_else(|| PartError::Feature("the feature no longer exists".into()))?;
        let id = old.id().to_owned();
        match (&old, &mut def) {
            // Extrude / Revolve keep a new name (breadcrumb rename).
            (Feature::Extrude(o), Feature::Extrude(n)) => {
                if n.name.is_empty() {
                    n.name = o.name.clone();
                }
                (n.id, n.sketch) = (o.id.clone(), o.sketch.clone());
            }
            (Feature::Revolve(o), Feature::Revolve(n)) => {
                if n.name.is_empty() {
                    n.name = o.name.clone();
                }
                (n.id, n.sketch) = (o.id.clone(), o.sketch.clone());
            }
            (Feature::Blend(o), Feature::Blend(n)) => {
                (n.id, n.name) = (o.id.clone(), o.name.clone());
            }
            (Feature::Shell(o), Feature::Shell(n)) => {
                (n.id, n.name) = (o.id.clone(), o.name.clone());
            }
            (Feature::Draft(o), Feature::Draft(n)) => {
                (n.id, n.name) = (o.id.clone(), o.name.clone());
            }
            (Feature::Pattern(o), Feature::Pattern(n)) if o.id_base() == n.id_base() => {
                (n.id, n.name) = (o.id.clone(), o.name.clone());
            }
            (Feature::Hole(o), Feature::Hole(n)) => {
                (n.id, n.name, n.sketch) = (o.id.clone(), o.name.clone(), o.sketch.clone());
            }
            (Feature::Thread(o), Feature::Thread(n)) => {
                (n.id, n.name) = (o.id.clone(), o.name.clone());
            }
            (Feature::Combine(o), Feature::Combine(n)) => {
                (n.id, n.name) = (o.id.clone(), o.name.clone());
            }
            (Feature::Split(o), Feature::Split(n)) => {
                (n.id, n.name) = (o.id.clone(), o.name.clone());
            }
            (Feature::Sweep(o), Feature::Sweep(n)) => {
                (n.id, n.name, n.sketch) = (o.id.clone(), o.name.clone(), o.sketch.clone());
            }
            (Feature::Coil(o), Feature::Coil(n)) => {
                (n.id, n.name, n.sketch) = (o.id.clone(), o.name.clone(), o.sketch.clone());
            }
            (Feature::Rib(o), Feature::Rib(n)) => {
                (n.id, n.name, n.sketch) = (o.id.clone(), o.name.clone(), o.sketch.clone());
            }
            (Feature::Loft(o), Feature::Loft(n)) => {
                (n.id, n.name) = (o.id.clone(), o.name.clone());
            }
            // Sheet metal: the same command (a Hem stays a Hem); the sketch
            // of a Face may change (its profile is picked again).
            (Feature::SheetFace(o), Feature::SheetFace(n)) => {
                (n.id, n.name) = (o.id.clone(), o.name.clone());
            }
            (Feature::SheetFlange(o), Feature::SheetFlange(n)) => {
                (n.id, n.name) = (o.id.clone(), o.name.clone());
            }
            (Feature::SheetConvert(o), Feature::SheetConvert(n)) => {
                (n.id, n.name) = (o.id.clone(), o.name.clone());
            }
            (Feature::SheetOp(o), Feature::SheetOp(n)) if o.kind_name() == n.kind_name() => {
                (n.id, n.name) = (o.id.clone(), o.name.clone());
            }
            // Redefine of a work feature: any method of the same kind (a
            // plane stays a plane, so what refers to it still resolves).
            (Feature::Work(o), Feature::Work(n)) if o.def.prefix() == n.def.prefix() => {
                (n.id, n.name) = (o.id.clone(), o.name.clone());
            }
            _ => {
                return Err(PartError::Feature(
                    "the edit changes the kind of feature".into(),
                ));
            }
        }
        self.rollback = None;
        self.features[k] = def;
        self.regenerate()?;
        if let Some(msg) = self.feature_health.get(&id).cloned() {
            self.features[k] = old;
            self.regenerate()?;
            return Err(PartError::Feature(msg));
        }
        Ok(id)
    }

    /// Removes feature `k`, keeping the sketches' places in the history.
    pub fn remove_feature(&mut self, k: usize) -> Option<Feature> {
        if k >= self.features.len() {
            return None;
        }
        for ps in &mut self.sketches {
            if let Some(n) = ps.after_features.as_mut()
                && *n > k
            {
                *n -= 1;
            }
        }
        if let Some(e) = self.eop.as_mut()
            && *e > k
        {
            *e -= 1;
        }
        let removed = self.features.remove(k);
        self.suppressed.remove(removed.id());
        self.suppress_if.remove(removed.id());
        if self.features.is_empty() {
            // `regenerate` keeps a body without features (old files store
            // the body directly), so the last feature's body goes here.
            self.body = BRep::new();
        }
        Some(removed)
    }

    /// What breaks if feature `k` is deleted: the sketches and features
    /// that become sick (found by rebuilding a copy without it), with the
    /// reason. Its own consumed sketch is not listed.
    pub fn breaks_if_removed(&self, k: usize) -> Vec<(String, String)> {
        let mut trial = self.clone();
        if trial.remove_feature(k).is_none() || trial.regenerate().is_err() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for ps in &trial.sketches {
            let was_ok = self
                .sketches
                .iter()
                .find(|s| s.id == ps.id)
                .is_some_and(|s| s.health.is_none());
            if let (true, Some(h)) = (was_ok, &ps.health) {
                out.push((ps.name.clone(), h.clone()));
            }
        }
        for f in &trial.features {
            if !self.feature_health.contains_key(f.id())
                && let Some(h) = trial.feature_health.get(f.id())
            {
                out.push((f.name().to_owned(), h.clone()));
            }
        }
        out
    }

    /// Indices of the features that use the sketch with `id`.
    pub fn consumers(&self, sketch_id: &str) -> Vec<usize> {
        self.features
            .iter()
            .enumerate()
            .filter(|(_, f)| f.sketch_ids().contains(&sketch_id))
            .map(|(k, _)| k)
            .collect()
    }

    /// The feature that owns sketch `index` in the browser: its first
    /// consumer, unless the sketch is shared (then it stands on its own).
    pub fn owner_of_sketch(&self, index: usize) -> Option<usize> {
        let ps = self.sketches.get(index)?;
        if ps.shared {
            return None;
        }
        self.consumers(&ps.id).first().copied()
    }

    /// **Share Sketch**: a consumed sketch becomes available to other
    /// features and is listed on its own (still hidden until shown).
    pub fn share_sketch(&mut self, index: usize) -> Result<(), PartError> {
        let ps = self
            .sketches
            .get_mut(index)
            .ok_or(PartError::NoSketch(index))?;
        ps.shared = true;
        Ok(())
    }

    /// **Unshare**: only when at most one feature uses the sketch.
    pub fn unshare_sketch(&mut self, index: usize) -> Result<(), PartError> {
        let id = self
            .sketches
            .get(index)
            .ok_or(PartError::NoSketch(index))?
            .id
            .clone();
        if self.consumers(&id).len() > 1 {
            return Err(PartError::Feature(
                "the sketch is used by several features and cannot be unshared".into(),
            ));
        }
        let ps = &mut self.sketches[index];
        ps.shared = false;
        ps.visible = false;
        Ok(())
    }

    /// Re-resolves every sketch against the current body.
    pub fn update_sketches(&mut self) {
        for i in 0..self.sketches.len() {
            update_sketch(&self.body, &self.work, &mut self.sketches, i);
        }
    }

    /// **Fillet / Chamfer**: adds the feature (id `Fillet<N>` / `Chamfer<N>`).
    /// On failure nothing changes.
    pub fn add_blend(&mut self, mut def: BlendFeature) -> Result<String, PartError> {
        let base = def.command();
        let id = self.next_feature_id(base);
        (def.id, def.name) = (id.clone(), id.clone());
        self.insert_feature_checked(Feature::Blend(def))?;
        Ok(id)
    }

    /// **Shell**: adds the feature (id `Shell<N>`). On failure nothing
    /// changes.
    pub fn add_shell(&mut self, mut def: ShellFeature) -> Result<String, PartError> {
        let id = self.next_feature_id("Shell");
        (def.id, def.name) = (id.clone(), id.clone());
        self.insert_feature_checked(Feature::Shell(def))?;
        Ok(id)
    }

    /// **Face Draft**: adds the feature (id `FaceDraft<N>`, shown as
    /// `Face Draft<N>`). On failure nothing changes.
    pub fn add_draft(&mut self, mut def: DraftFeature) -> Result<String, PartError> {
        let id = self.next_feature_id("FaceDraft");
        def.name = format!("Face Draft{}", &id["FaceDraft".len()..]);
        def.id = id.clone();
        self.insert_feature_checked(Feature::Draft(def))?;
        Ok(id)
    }

    /// **Combine** (id `Combine<N>`): the base body joined with, cut by or
    /// intersected with the toolbodies. Kept toolbodies are hidden
    /// (Inventor). On failure nothing changes.
    pub fn add_combine(&mut self, mut def: CombineFeature) -> Result<String, PartError> {
        def.validate().map_err(PartError::Feature)?;
        let id = self.next_feature_id("Combine");
        (def.id, def.name) = (id.clone(), id.clone());
        let keep = if def.keep_tools {
            def.tools.clone()
        } else {
            Vec::new()
        };
        self.insert_feature_checked(Feature::Combine(def))?;
        for t in keep {
            if !self.hidden_bodies.contains(&t) {
                self.hidden_bodies.push(t);
            }
        }
        Ok(id)
    }

    /// **Split** (id `Split<N>`): Split Face, Trim Solid or Split Solid. A
    /// sketch tool is consumed (hidden unless Keep sketch visible; shared
    /// when another feature uses it). On failure nothing changes.
    pub fn add_split(&mut self, mut def: SplitFeature) -> Result<String, PartError> {
        let id = self.next_feature_id("Split");
        (def.id, def.name) = (id.clone(), id.clone());
        let sketch = def.sketch().map(str::to_owned);
        let visible = def.keep_sketch_visible;
        let reused = sketch
            .as_deref()
            .is_some_and(|s| !self.consumers(s).is_empty());
        self.insert_feature_checked(Feature::Split(def))?;
        if let Some(ps) = sketch.and_then(|s| self.sketches.iter_mut().find(|p| p.id == s)) {
            if reused {
                ps.shared = true;
            } else if !visible {
                ps.visible = false;
            }
        }
        Ok(id)
    }

    /// Whether the body with persistent name `name` is shown.
    pub fn body_visible(&self, name: &str) -> bool {
        !self.hidden_bodies.iter().any(|h| h == name)
    }

    /// **Rectangular Pattern / Circular Pattern / Mirror**: adds the
    /// feature (id `RectangularPattern<N>` shown as `Rectangular
    /// Pattern<N>`, likewise `CircularPattern<N>`, `Mirror<N>`). On failure
    /// nothing changes.
    pub fn add_pattern(&mut self, mut def: PatternFeature) -> Result<String, PartError> {
        let base = def.id_base();
        let id = self.next_feature_id(base);
        def.name = format!("{}{}", def.command(), &id[base.len()..]);
        def.id = id.clone();
        self.insert_feature_checked(Feature::Pattern(def))?;
        Ok(id)
    }

    /// **Hole**: adds the feature (id `Hole<N>`). Its sketch (if any) is
    /// consumed like an extrusion's: hidden, or shared when another feature
    /// uses it already. On failure nothing changes.
    pub fn add_hole(&mut self, mut def: HoleFeature) -> Result<String, PartError> {
        let id = self.next_feature_id("Hole");
        (def.id, def.name) = (id.clone(), id.clone());
        let sketch = def.sketch.clone();
        let reused = !sketch.is_empty() && !self.consumers(&sketch).is_empty();
        self.insert_feature_checked(Feature::Hole(def))?;
        if let Some(ps) = self.sketches.iter_mut().find(|s| s.id == sketch) {
            if reused {
                ps.shared = true;
            } else {
                ps.visible = false;
            }
        }
        Ok(id)
    }

    /// The sketch Hole makes on a planar face (Inventor's Allow Center
    /// Point Creation): a sketch on `face` without projections, with a
    /// point at each of `centers` (model coordinates, projected onto the
    /// face). Returns the sketch index and the points.
    pub fn create_hole_sketch(
        &mut self,
        face: &str,
        centers: &[Point3],
    ) -> Result<(usize, Vec<PointId>), PartError> {
        let opts = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..SketchOptions::default()
        };
        let i = self.create_sketch(SketchPlacement::on_face(face), &opts)?;
        let ps = &mut self.sketches[i];
        let plane = ps.sketch.plane.clone();
        let pts = centers
            .iter()
            .map(|c| ps.sketch.add_center_point(plane.project(c)))
            .collect();
        Ok((i, pts))
    }

    /// **Project Geometry** of a straight model edge into sketch `index`:
    /// its two vertices become fixed projected points (associative: they
    /// follow the model at every regeneration, by persistent name) joined
    /// by a reference line, which is returned. An edge already projected
    /// returns its line.
    pub fn project_edge(
        &mut self,
        index: usize,
        edge: crate::kernel::EdgeId,
    ) -> Result<EntityId, PartError> {
        let body = &self.body;
        let e = body
            .edge(edge)
            .ok_or_else(|| PartError::Feature("the edge no longer exists".into()))?;
        if body.edge_direction(edge).is_none() {
            return Err(PartError::Feature(
                "Project Geometry: a curved edge is projected with project_curved_edge".into(),
            ));
        }
        let name = |v| body.vertex(v).and_then(|x| x.name.clone());
        let (Some(a), Some(b)) = (name(e.start), name(e.end)) else {
            return Err(PartError::Feature(
                "Project Geometry: the edge's ends have no names".into(),
            ));
        };
        self.project_segment(index, &a, &b)
    }

    /// A reference line in sketch `index` between the projections of the
    /// model vertices named `a` and `b` (made when missing).
    pub fn project_segment(
        &mut self,
        index: usize,
        a: &str,
        b: &str,
    ) -> Result<EntityId, PartError> {
        let body = self.body.clone();
        let works = self.work.clone();
        let ps = self
            .sketches
            .get_mut(index)
            .ok_or(PartError::NoSketch(index))?;
        let plane = ps.sketch.plane.clone();
        let mut point_of = |name: &str| -> Result<PointId, PartError> {
            let src = PointRef::Vertex(name.to_owned());
            if let Some(p) = ps.projections.iter().find(|p| p.source == src) {
                return Ok(p.point);
            }
            let w = placement::point_of(&src, &body, &works)?;
            let point = ps.sketch.add_fixed_point(plane.project(&w));
            ps.projections.push(Projection { point, source: src });
            Ok(point)
        };
        let (pa, pb) = (point_of(a)?, point_of(b)?);
        if pa == pb {
            return Err(PartError::Feature("the edge has no length".into()));
        }
        if let Some(&l) = ps.reference_lines.iter().find(|&&l| {
            ps.sketch
                .line_points(l)
                .is_ok_and(|(x, y)| (x, y) == (pa, pb) || (x, y) == (pb, pa))
        }) {
            return Ok(l);
        }
        let line = ps
            .sketch
            .add_line_between(pa, pb)
            .map_err(|e| PartError::Feature(e.to_string()))?;
        ps.reference_lines.push(line);
        Ok(line)
    }

    /// **Project Geometry** of a point or curve of another sketch into
    /// sketch `index` (Inventor: Project Geometry on sketch geometry). The
    /// source must come before the target in the history. The projection is
    /// fixed reference geometry that follows its source at every
    /// regeneration ([`link`]); `construction` makes the curve construction
    /// geometry (Shift+click). Projecting the same source again returns the
    /// existing link. Returns the link's index in `PartSketch::links`.
    pub fn project_sketch_geometry(
        &mut self,
        index: usize,
        sketch: &str,
        geo: SketchGeo,
        construction: bool,
    ) -> Result<usize, PartError> {
        let source = LinkSource::Sketch {
            sketch: sketch.to_owned(),
            geo,
        };
        self.add_link(index, source, construction)
    }

    /// **Project Geometry** of a circular model edge (circle or arc) into
    /// sketch `index`: a fixed reference circle/arc that follows the edge
    /// by the names of its faces. The edge must be parallel to the sketch.
    pub fn project_curved_edge(
        &mut self,
        index: usize,
        edge: crate::kernel::EdgeId,
    ) -> Result<usize, PartError> {
        let r = blend::edge_ref(&self.body, edge).ok_or_else(|| {
            PartError::Feature("Project Geometry: the edge's faces have no names".into())
        })?;
        self.add_link(index, LinkSource::Edge { edge: r }, false)
    }

    /// **Project Geometry** of a work point / axis / plane into sketch
    /// `index`: a fixed reference point or line that follows it.
    pub fn project_work(&mut self, index: usize, id: &str) -> Result<usize, PartError> {
        if !self.features.iter().any(|f| f.id() == id) {
            return Err(PartError::Feature(format!(
                "Project Geometry: work feature {id} does not exist"
            )));
        }
        self.add_link(index, LinkSource::Work { id: id.to_owned() }, false)
    }

    fn add_link(
        &mut self,
        index: usize,
        source: LinkSource,
        construction: bool,
    ) -> Result<usize, PartError> {
        let fail = |m: String| PartError::Feature(format!("Project Geometry: {m}"));
        if index >= self.sketches.len() {
            return Err(PartError::NoSketch(index));
        }
        if let Some(k) = self.sketches[index]
            .links
            .iter()
            .position(|l| l.source == source)
        {
            return Ok(k);
        }
        let shape =
            link::resolve(&source, &self.sketches, index, &self.body, &self.work).map_err(fail)?;
        let ps = &mut self.sketches[index];
        let proj = link::project(&shape, &ps.sketch.plane).map_err(fail)?;
        let (points, curve) =
            link::create(&mut ps.sketch, &shape, &proj, construction).map_err(fail)?;
        ps.links.push(GeometryLink {
            source,
            points,
            curve,
            status: None,
        });
        Ok(ps.links.len() - 1)
    }

    /// **Replace Reference** (Inventor 2026.1): link `k` of sketch `index`
    /// takes `source` instead (same kind of geometry); its target geometry
    /// and the constraints on it stay, and it moves to the new source. On
    /// failure nothing changes.
    pub fn relink(&mut self, index: usize, k: usize, source: LinkSource) -> Result<(), PartError> {
        let fail = |m: String| PartError::Feature(format!("Replace Reference: {m}"));
        let ps = self.sketches.get(index).ok_or(PartError::NoSketch(index))?;
        let link = ps
            .links
            .get(k)
            .cloned()
            .ok_or_else(|| fail("no such reference".into()))?;
        let shape =
            link::resolve(&source, &self.sketches, index, &self.body, &self.work).map_err(fail)?;
        let proj = link::project(&shape, &ps.sketch.plane).map_err(fail)?;
        let kind_now = match link.curve.and_then(|c| ps.sketch.entity(c)) {
            Some(Entity::Line { .. }) => "Line",
            Some(Entity::Circle { .. }) => "Circle",
            Some(Entity::Arc { .. }) => "Arc",
            Some(Entity::Spline { .. }) => "Spline",
            None => "Point",
        };
        if shape.kind() != kind_now {
            return Err(fail(format!(
                "pick a {} (the reference is a {})",
                kind_now.to_lowercase(),
                kind_now.to_lowercase()
            )));
        }
        let mut trial = ps.sketch.clone();
        link::place(&mut trial, &link, &proj).map_err(fail)?;
        let _ = trial.solve();
        let ps = &mut self.sketches[index];
        ps.sketch = trial;
        ps.links[k].source = source;
        ps.links[k].status = None;
        Ok(())
    }

    /// Indices of the sketches that project geometry of sketch `id`.
    pub fn link_dependents(&self, id: &str) -> Vec<usize> {
        (0..self.sketches.len())
            .filter(|&i| {
                self.sketches[i]
                    .links
                    .iter()
                    .any(|l| l.source.sketch() == Some(id))
            })
            .collect()
    }

    /// Whether sketch `source` may project into sketch `target` (it comes
    /// first in the history).
    pub fn can_project_from(&self, target: usize, source: usize) -> bool {
        source != target
            && source < self.sketches.len()
            && link::order_key(&self.sketches, source) < link::order_key(&self.sketches, target)
    }

    /// The centres of `def` on the current body: (point on the start
    /// plane, unit drilling direction, start face).
    pub fn hole_placements(
        &self,
        def: &HoleFeature,
    ) -> Result<Vec<(Point3, crate::kernel::Vector3, Option<String>)>, String> {
        let ps = self.sketches.iter().find(|s| s.id == def.sketch);
        exact::hole_placements(def, ps, &self.body)
    }

    /// **Thread**: adds the feature (id `Thread<N>`). On failure nothing
    /// changes.
    pub fn add_thread(&mut self, mut def: ThreadFeature) -> Result<String, PartError> {
        let id = self.next_feature_id("Thread");
        (def.id, def.name) = (id.clone(), id.clone());
        self.insert_feature_checked(Feature::Thread(def))?;
        Ok(id)
    }

    /// Every hole of the part as of the last regeneration, in feature and
    /// centre order: spec, centre, direction, start face, feature id and
    /// occurrence (drawing data: Hole Table, hole notes).
    ///
    /// Pattern / Mirror hook: a feature that copies a Hole feature adds
    /// `h.transformed(&m)` for each copy (with its element index pushed onto
    /// [`HoleInstance::copy`]) to [`PartDocument::hole_instances`] at its
    /// place in the regeneration loop.
    pub fn holes(&self) -> &[HoleInstance] {
        &self.hole_instances
    }

    /// The holes grouped as a drawing counts them: identical holes on the
    /// same face, from any Hole feature (`4x Ø6.6 THRU`).
    pub fn hole_groups(&self) -> Vec<HoleGroup> {
        hole::group_holes(&self.hole_instances)
    }

    /// **Work Plane / Axis / Point**: adds a work feature (id `WorkPlane<N>`,
    /// name `Work Plane<N>`, likewise for axes and points). On failure
    /// nothing changes.
    pub fn add_work(&mut self, mut def: WorkFeature) -> Result<String, PartError> {
        let (idp, namep) = def.def.prefix();
        let id = self.next_feature_id(idp);
        def.name = id.replace(idp, namep);
        def.id = id.clone();
        self.insert_feature_checked(Feature::Work(def))?;
        Ok(id)
    }
}

/// The tools a feature pattern copies, in history order (`before`: the
/// features before the pattern). A feature without tools of its own is
/// refused with the reason.
fn pattern_sources<'t>(
    before: &[Feature],
    p: &PatternFeature,
    tools: &'t HashMap<String, Vec<pattern::ToolSet>>,
    health: &HashMap<String, String>,
) -> Result<Vec<&'t pattern::ToolSet>, String> {
    if p.features.is_empty() {
        return Err("select the features to copy".into());
    }
    let mut at: Vec<usize> = Vec::new();
    for id in &p.features {
        let k = before
            .iter()
            .position(|f| f.id() == id)
            .ok_or_else(|| format!("the feature {id} no longer exists before it"))?;
        if !at.contains(&k) {
            at.push(k);
        }
    }
    at.sort_unstable();
    let mut out = Vec::new();
    for k in at {
        let f = &before[k];
        // Any feature that applied tools (Extrude, Revolve, a feature
        // pattern; a Hole later) can be patterned.
        if let Some(sets) = tools.get(f.id()) {
            out.extend(sets.iter());
            continue;
        }
        return Err(match f {
            _ if health.contains_key(f.id()) => {
                format!("{} failed, so it cannot be copied", f.name())
            }
            Feature::Extrude(_)
            | Feature::Revolve(_)
            | Feature::Sweep(_)
            | Feature::Loft(_)
            | Feature::Coil(_)
            | Feature::Rib(_) => {
                format!("{} cannot be copied (a surface)", f.name())
            }
            Feature::Hole(_) => format!("{} made no hole to copy", f.name()),
            Feature::Box(_)
            | Feature::Work(_)
            | Feature::Blend(_)
            | Feature::Shell(_)
            | Feature::Draft(_)
            | Feature::Thread(_)
            | Feature::Combine(_)
            | Feature::Split(_)
            | Feature::SheetFace(_)
            | Feature::SheetFlange(_)
            | Feature::SheetConvert(_)
            | Feature::SheetOp(_)
            | Feature::Pattern(_) => format!(
                "{} cannot be patterned on its own (pattern the solid)",
                f.name()
            ),
        });
    }
    Ok(out)
}

/// Re-resolves one sketch against `body`: placement, projected points,
/// solve; problems go to its `health`.
fn update_sketch(body: &BRep, works: &placement::WorkGeom, sketches: &mut [PartSketch], i: usize) {
    let Some(ps) = sketches.get_mut(i) else {
        return;
    };
    match ps.placement.resolve_with(body, works, &ps.sketch.plane) {
        Ok(plane) => {
            ps.sketch.plane = plane;
            ps.health = None;
            // Geometry projected from other sketches / curved edges.
            let mut missing = link::apply(sketches, i, body, works);
            let ps = &mut sketches[i];
            for pr in &ps.projections {
                match project(body, works, &ps.sketch.plane, &pr.source) {
                    Some(uv) => {
                        let _ = ps.sketch.move_point(pr.point, uv);
                    }
                    None => missing.push(describe(&pr.source)),
                }
            }
            if !missing.is_empty() {
                ps.health = Some(format!(
                    "projected source(s) missing, frozen: {}",
                    missing.join(", ")
                ));
            }
            let _ = ps.sketch.solve();
        }
        Err(e) => ps.health = Some(e.to_string()),
    }
}

impl PartDocument {
    /// `"Sketch<N>"` with N one more than the highest existing number.
    pub fn next_sketch_name(&self) -> String {
        // Ids and names both count ("Sketch1 (Slot)" uses 1): a new
        // sketch's id must never repeat an existing one.
        let n = self
            .sketches
            .iter()
            .flat_map(|s| [&s.name, &s.id])
            .filter_map(|n| {
                let digits: String = n
                    .strip_prefix("Sketch")?
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect();
                digits.parse::<u32>().ok()
            })
            .max()
            .unwrap_or(0);
        format!("Sketch{}", n + 1)
    }

    /// **Start 2D Sketch**: creates a sketch with `placement`, projects
    /// origin and face edges per `opts`, and returns its index.
    pub fn create_sketch(
        &mut self,
        placement: SketchPlacement,
        opts: &SketchOptions,
    ) -> Result<usize, PartError> {
        let plane = placement.resolve_with(&self.body, &self.work, &Plane::xy())?;
        let mut ps = PartSketch::new(self.next_sketch_name(), Sketch::new(plane));
        ps.placement = placement;
        ps.after_features = Some(self.insert_index());
        add_projections(&self.body, &mut ps, opts);
        let _ = ps.sketch.solve();
        self.sketches.push(ps);
        Ok(self.sketches.len() - 1)
    }

    /// **Redefine**: moves sketch `index` to another plane or face. The 2D
    /// geometry keeps its sketch coordinates (it moves with the plane); the
    /// coordinate system resets to the default; old projections are
    /// replaced by projections from the new face.
    pub fn redefine_sketch(
        &mut self,
        index: usize,
        plane: PlaneRef,
        opts: &SketchOptions,
    ) -> Result<(), PartError> {
        let ps = self
            .sketches
            .get_mut(index)
            .ok_or(PartError::NoSketch(index))?;
        let placement = SketchPlacement {
            plane,
            ..Default::default()
        };
        let resolved = placement.resolve(&self.body, &ps.sketch.plane)?;
        clear_projections(ps);
        ps.placement = placement;
        ps.sketch.plane = resolved;
        ps.health = None;
        add_projections(&self.body, ps, opts);
        let _ = ps.sketch.solve();
        Ok(())
    }

    /// **Edit Coordinate System**: changes origin and axis of sketch
    /// `index`. Geometry keeps its 3D position (its 2D coordinates are
    /// re-expressed in the new system).
    pub fn set_coordinate_system(
        &mut self,
        index: usize,
        origin: PointRef,
        axis_edge: Option<String>,
        axis_is_x: bool,
        natural_direction: bool,
    ) -> Result<(), PartError> {
        let ps = self
            .sketches
            .get_mut(index)
            .ok_or(PartError::NoSketch(index))?;
        let placement = SketchPlacement {
            plane: ps.placement.plane.clone(),
            origin,
            axis_edge,
            axis_is_x,
            natural_direction,
        };
        let new_plane = placement.resolve(&self.body, &ps.sketch.plane)?;
        let old_plane = ps.sketch.plane.clone();
        ps.sketch
            .map_points(|uv| new_plane.project(&old_plane.point_at(uv.x, uv.y)));
        ps.sketch.plane = new_plane;
        ps.placement = placement;
        let _ = ps.sketch.solve();
        Ok(())
    }

    /// Deletes sketch `index`.
    pub fn delete_sketch(&mut self, index: usize) -> Result<PartSketch, PartError> {
        if index >= self.sketches.len() {
            return Err(PartError::NoSketch(index));
        }
        Ok(self.sketches.remove(index))
    }
}

/// Sketch coordinates of a model point reference.
fn project(
    body: &BRep,
    works: &placement::WorkGeom,
    plane: &Plane,
    source: &PointRef,
) -> Option<Point2> {
    placement::point_of(source, body, works)
        .ok()
        .map(|p| plane.project(&p))
}

fn describe(source: &PointRef) -> String {
    match source {
        PointRef::PartOrigin => "part origin".into(),
        PointRef::Vertex(n) => n.clone(),
        other => other.describe(),
    }
}

/// Why `body` is not a valid set of solids (closed, oriented, manifold),
/// if it is not; surface bodies are not checked.
fn invalid_solid(body: &BRep) -> Option<String> {
    body.solids()
        .filter(|(_, s)| !s.open)
        .find_map(|(id, _)| body.validate_solid(id).err().map(|e| format!("{e:?}")))
}

/// Adds the automatic projections (part origin, face edges) per `opts`.
fn add_projections(body: &BRep, ps: &mut PartSketch, opts: &SketchOptions) {
    let plane = ps.sketch.plane.clone();
    if opts.auto_project_origin {
        let point = ps.sketch.add_fixed_point(plane.project(&Point3::origin()));
        ps.projections.push(Projection {
            point,
            source: PointRef::PartOrigin,
        });
    }
    let PlaneRef::Face(face_name) = &ps.placement.plane else {
        return;
    };
    if !opts.auto_reference_edges {
        return;
    }
    let Some(face) = body.find_face(face_name) else {
        return;
    };

    let mut point_of = HashMap::new();
    for v in body.face_vertices(face) {
        let Some(vertex) = body.vertex(v) else {
            continue;
        };
        let Some(name) = vertex.name.clone() else {
            log::warn!("unnamed vertex on {face_name}: cannot project associatively");
            continue;
        };
        let point = ps.sketch.add_fixed_point(plane.project(&vertex.point));
        ps.projections.push(Projection {
            point,
            source: PointRef::Vertex(name),
        });
        point_of.insert(v, point);
    }
    for e in body.face_edges(face) {
        let Some(edge) = body.edge(e) else { continue };
        if body.edge_direction(e).is_none() {
            log::warn!("projection of curved edges is not implemented yet");
            continue;
        }
        if let (Some(&a), Some(&b)) = (point_of.get(&edge.start), point_of.get(&edge.end))
            && let Ok(line) = ps.sketch.add_line_between(a, b)
        {
            ps.reference_lines.push(line);
        }
    }
}

/// Removes all projected points (and, by cascade, reference lines and
/// constraints attached to them).
fn clear_projections(ps: &mut PartSketch) {
    for pr in std::mem::take(&mut ps.projections) {
        let _ = ps.sketch.remove_point(pr.point);
    }
    ps.reference_lines.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deleting a box warns about the sketch on its face (and the extrusion
    /// of that sketch); deleting the extrusion breaks nothing.
    #[test]
    fn breaks_if_removed_lists_dependents() {
        use crate::part::{ExtrudeFeature, ProfileRef};
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
        sk.add_circle(Point2::new(10.0, 10.0), 5.0).unwrap();
        let r = sk.regions();
        let pr = ProfileRef::of(&r[0], r[0].interior_point());
        part.add_extrude(i, ExtrudeFeature::new(vec![pr], 5.0))
            .unwrap();
        let hit = part.breaks_if_removed(0);
        assert!(
            hit.iter().any(|(n, _)| n == &part.sketches[i].name),
            "{hit:?}"
        );
        assert!(part.breaks_if_removed(1).is_empty());
        // Nothing changed in the part itself.
        assert_eq!(part.features.len(), 2);
    }

    /// A sketch on a face that a later Join swallows stays healthy: it is
    /// resolved against the body at its place in the history.
    #[test]
    fn sketch_follows_the_body_of_its_history_position() {
        use crate::part::{ExtrudeFeature, ProfileRef};
        let mut part = PartDocument::with_box("Box1", [80.0, 60.0, 20.0]).unwrap();
        let placement = SketchPlacement {
            plane: PlaneRef::Face("Box1:Top".into()),
            ..Default::default()
        };
        let i = part
            .create_sketch(placement, &SketchOptions::default())
            .unwrap();
        let r = part.sketches[i].sketch.regions();
        let big = r
            .iter()
            .max_by(|a, b| a.area().total_cmp(&b.area()))
            .unwrap();
        let pr = ProfileRef::of(big, big.interior_point());
        let mut e = ExtrudeFeature::new(vec![pr], 5.0);
        e.operation = crate::part::Operation::Join;
        part.add_extrude(i, e).unwrap();
        part.regenerate().unwrap();
        assert!(part.body.find_face("Box1:Top").is_none(), "joined away");
        assert_eq!(part.sketches[i].health, None);
    }

    /// References list, Break Link and Delete on a sketch on a box face.
    #[test]
    fn references_break_link_and_delete() {
        let mut part = PartDocument::with_box("Box1", [80.0, 60.0, 20.0]).unwrap();
        let placement = SketchPlacement {
            plane: PlaneRef::Face("Box1:Top".into()),
            ..Default::default()
        };
        let i = part
            .create_sketch(placement, &SketchOptions::default())
            .unwrap();
        let ps = &part.sketches[i];
        let rows = ps.references(&part.body, &part.work);
        // Four edges of the face and the projected part origin.
        assert_eq!(
            rows.iter()
                .filter(|r| matches!(r.target, RefTarget::Line(_)))
                .count(),
            4
        );
        assert!(rows.iter().any(|r| r.source == "Origin: Center Point"));
        assert!(rows.iter().all(|r| r.ok));
        // Break one edge: it becomes normal geometry; its ends stay linked
        // (the neighbouring edges use them).
        let RefTarget::Line(line) = rows[0].target else {
            panic!()
        };
        let (a, _) = part.sketches[i].sketch.line_points(line).unwrap();
        let ps = &mut part.sketches[i];
        ps.break_link(RefTarget::Line(line)).unwrap();
        assert!(!ps.reference_lines.contains(&line));
        assert!(ps.sketch.entity(line).is_some());
        assert!(ps.is_projected(a));
        // Delete another edge: the line goes, shared ends stay.
        let other = ps.reference_lines[0];
        ps.delete_reference(RefTarget::Line(other)).unwrap();
        assert!(ps.sketch.entity(other).is_none());
        // Two edges left; the corner between the broken and the deleted
        // edge is still linked, now listed as a point.
        let now = ps.references(&part.body, &part.work);
        assert_eq!(
            now.iter()
                .filter(|r| matches!(r.target, RefTarget::Line(_)))
                .count(),
            2
        );
        assert_eq!(now.len(), rows.len() - 1);
        // A missing source shows as not ok.
        let ps = &mut part.sketches[i];
        ps.projections[0].source = PointRef::Vertex("Gone:V".into());
        assert!(ps.references(&part.body, &part.work).iter().any(|r| !r.ok));
    }
    use crate::sketch::{Constraint, SolveStatus};
    use approx::assert_relative_eq;

    fn part() -> PartDocument {
        PartDocument::with_box("Box1", [80.0, 60.0, 20.0]).unwrap()
    }

    fn world(ps: &PartSketch, p: PointId) -> Point3 {
        let uv = ps.sketch.point(p).unwrap().position;
        ps.sketch.to_world(&uv)
    }

    #[test]
    fn sketch_on_face_projects_origin_and_edges() {
        let mut part = part();
        let i = part
            .create_sketch(
                SketchPlacement::on_face("Box1:Top"),
                &SketchOptions::default(),
            )
            .unwrap();
        let ps = &part.sketches[i];
        assert_eq!(ps.name, "Sketch1");
        assert_eq!(ps.projections.len(), 1 + 4, "origin + 4 corners");
        assert_eq!(ps.reference_lines.len(), 4);
        assert!(
            ps.projections
                .iter()
                .all(|p| ps.sketch.point(p.point).unwrap().fixed)
        );
        assert_relative_eq!(
            ps.sketch.plane.origin,
            Point3::new(0.0, 0.0, 20.0),
            epsilon = 1e-12
        );
        // The projected corner V111 sits on the real corner.
        let v111 = ps
            .projections
            .iter()
            .find(|p| p.source == PointRef::Vertex("Box1:V111".into()))
            .unwrap();
        assert_relative_eq!(
            world(ps, v111.point),
            Point3::new(80.0, 60.0, 20.0),
            epsilon = 1e-9
        );

        // Options off: nothing projected.
        let opts = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let j = part
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)
            .unwrap();
        assert_eq!(part.sketches[j].name, "Sketch2");
        assert!(part.sketches[j].projections.is_empty());
    }

    #[test]
    fn sketch_follows_its_face_when_the_body_changes() {
        let mut part = part();
        let i = part
            .create_sketch(
                SketchPlacement::on_face("Box1:Top"),
                &SketchOptions::default(),
            )
            .unwrap();
        // A user point constrained to a projected corner.
        let corner = part.sketches[i]
            .projections
            .iter()
            .find(|p| p.source == PointRef::Vertex("Box1:V111".into()))
            .unwrap()
            .point;
        let ps = &mut part.sketches[i];
        let mine = ps.sketch.add_point(Point2::new(70.0, 50.0));
        ps.sketch
            .add_constraint(Constraint::Coincident { a: mine, b: corner })
            .unwrap();
        assert_eq!(ps.sketch.solve().unwrap().status, SolveStatus::Solved);

        // Make the box taller and longer.
        part.box_feature_mut().unwrap().size = [100.0, 60.0, 35.0];
        part.regenerate().unwrap();
        let ps = &part.sketches[i];
        assert!(ps.health.is_none());
        assert_relative_eq!(ps.sketch.plane.origin.z, 35.0, epsilon = 1e-12);
        assert_relative_eq!(
            world(ps, mine),
            Point3::new(100.0, 60.0, 35.0),
            epsilon = 1e-9
        );
    }

    #[test]
    fn renaming_a_feature_keeps_references() {
        let mut part = part();
        part.create_sketch(
            SketchPlacement::on_face("Box1:Top"),
            &SketchOptions::default(),
        )
        .unwrap();
        part.box_feature_mut().unwrap().name = "Base plate".into();
        part.regenerate().unwrap();
        assert!(
            part.sketches[0].health.is_none(),
            "display name is not identity"
        );
    }

    #[test]
    fn lost_face_is_reported() {
        let mut part = part();
        part.create_sketch(
            SketchPlacement::on_face("Box1:Top"),
            &SketchOptions::default(),
        )
        .unwrap();
        // Simulate the feature being replaced by a different one.
        part.box_feature_mut().unwrap().id = "Box9".into();
        part.regenerate().unwrap();
        assert!(
            part.sketches[0]
                .health
                .as_deref()
                .unwrap()
                .contains("no longer exists")
        );
    }

    #[test]
    fn redefine_moves_geometry_with_the_plane() {
        let mut part = part();
        let opts = SketchOptions::default();
        let i = part
            .create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)
            .unwrap();
        let p = part.sketches[i].sketch.add_point(Point2::new(10.0, 5.0));
        part.redefine_sketch(i, PlaneRef::Face("Box1:Front".into()), &opts)
            .unwrap();
        let ps = &part.sketches[i];
        assert_eq!(
            ps.sketch.point(p).unwrap().position,
            Point2::new(10.0, 5.0),
            "2D coords kept"
        );
        // Front face: x = +X, y = +Z, plane y = 0 → (10, 0, 5).
        assert_relative_eq!(world(ps, p), Point3::new(10.0, 0.0, 5.0), epsilon = 1e-9);
        assert_eq!(ps.projections.len(), 5, "re-projected from the new face");
    }

    #[test]
    fn edit_coordinate_system_keeps_geometry_in_place() {
        let mut part = part();
        let i = part
            .create_sketch(
                SketchPlacement::on_face("Box1:Top"),
                &SketchOptions::default(),
            )
            .unwrap();
        let p = part.sketches[i].sketch.add_point(Point2::new(10.0, 5.0));
        let before = world(&part.sketches[i], p);
        part.set_coordinate_system(
            i,
            PointRef::Vertex("Box1:V111".into()),
            Some("Box1:E011-111".into()),
            true,
            false,
        )
        .unwrap();
        let ps = &part.sketches[i];
        assert_relative_eq!(world(ps, p), before, epsilon = 1e-9);
        // New origin at the far corner, x = −X: the point is at (70, …).
        assert_relative_eq!(ps.sketch.point(p).unwrap().position.x, 70.0, epsilon = 1e-9);
    }

    #[test]
    fn part_with_sketches_roundtrips_through_a_project_file() {
        let mut part = part();
        part.create_sketch(
            SketchPlacement::on_face("Box1:Right"),
            &SketchOptions::default(),
        )
        .unwrap();
        let mut project = crate::document::Project::new("p");
        project.add_document("Part1", crate::document::DocumentData::Part(part));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.3dcadcraft");
        project.save(&path).unwrap();
        let loaded = crate::document::Project::load(&path).unwrap();
        let crate::document::DocumentData::Part(mut p) = loaded.documents()[0].data.clone() else {
            panic!("part expected")
        };
        assert_eq!(
            p.sketches[0].placement,
            SketchPlacement::on_face("Box1:Right")
        );
        p.regenerate().unwrap();
        assert!(p.sketches[0].health.is_none());
    }
}
