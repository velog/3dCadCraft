//! **Rectangular Pattern, Circular Pattern and Mirror** of part features or
//! whole solids (Inventor: 3D Model → Pattern, `docs/spec/2.11-patterns.md`,
//! `docs/spec/2.12-mirror.md`).
//!
//! Feature mode copies the *tools* of the chosen features (the solids an
//! Extrude or Revolve adds, cuts or joins, kept by regeneration) and applies
//! the copies with the same operation; solid mode copies whole bodies. A
//! copy is a rigid motion of the original ([`PatternFeature::placements`]):
//! translations, rotations about an axis, or a reflection. The faces of copy
//! `k` (the original is number 1) are named `<pattern id>#<k>:<original
//! name>`, so a later fillet on a copy's edge stays with that copy.

use serde::{Deserialize, Serialize};

use crate::kernel::{BRep, Point3, Real, Vector3};

use super::PartSketch;
use super::boolean::Operation;
use super::draft::PullRef;
use super::params::FeatureParams;
use super::placement::{PlaneRef, PointRef, WorkGeom, plane_of, point_of};
use super::sweep::PathRef;
use super::work::LineRef;

/// Most instances (copies and original) one pattern may have (SolidCraft's
/// limit as well).
pub const MAX_INSTANCES: usize = 10000;

/// One direction of a rectangular pattern.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatternDirection {
    /// A straight edge / axis, or a plane whose normal is the direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<PullRef>,
    /// Reverses the direction.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    /// Instances along it, the original included (parameter `count1` /
    /// `count2`).
    pub count: u32,
    /// Incremental: the step between instances; Fitted: the whole span
    /// from the first to the last (mm; parameter `spacing1` / `spacing2`).
    pub spacing: Real,
    /// The spacing is the whole span (Inventor's Fitted / Distance).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fitted: bool,
    /// Instances on both sides of the original (an even count puts the
    /// extra one on the direction's side; Flip moves it).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub midplane: bool,
    /// A curve the instances follow (Inventor: a line, arc, spline or edge
    /// as the direction): the spacing is measured along it from its end
    /// nearer the original; it replaces `dir` when set (format revision 71).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathRef>,
    /// Orientation along a path: the copies turn with the curve's tangent
    /// (Inventor's Orientation *Direction*); off, they keep the original's
    /// orientation (*Identical*).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub follow: bool,
}

impl PatternDirection {
    /// Inventor's defaults: 2 instances, 10 mm apart, no direction yet.
    pub fn new() -> Self {
        Self {
            dir: None,
            flip: false,
            count: 2,
            spacing: 10.0,
            fitted: false,
            midplane: false,
            path: None,
            follow: false,
        }
    }

    /// Whether a direction (straight or a path) is chosen.
    pub fn is_set(&self) -> bool {
        self.dir.is_some() || self.path.as_ref().is_some_and(|p| !p.is_empty())
    }
}

impl Default for PatternDirection {
    fn default() -> Self {
        Self::new()
    }
}

/// What kind of pattern, with its placement inputs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PatternKind {
    /// Rows along one or two directions.
    Rectangular {
        /// Direction 1.
        first: PatternDirection,
        /// Direction 2 (columns), optional.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        second: Option<PatternDirection>,
    },
    /// Equal angles about an axis (Orientation Rotational).
    Circular {
        /// The rotation axis.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        axis: Option<LineRef>,
        /// Reverses the turning sense.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        flip: bool,
        /// Instances, the original included (parameter `count`).
        count: u32,
        /// Incremental: angle between instances; Fitted: the whole angle,
        /// 360° = a full circle (radians; parameter `angle`).
        angle: Real,
        /// The angle is the whole span (Inventor's default).
        #[serde(default = "yes")]
        fitted: bool,
        /// Instances on both sides of the original.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        midplane: bool,
    },
    /// One mirrored copy across a plane.
    Mirror {
        /// The mirror plane (origin / work plane or planar face).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plane: Option<PlaneRef>,
    },
    /// Copies at the centre points of a sketch (Inventor's Sketch Driven
    /// Pattern, Orientation Fixed; format revision 71): each copy is the
    /// original moved by (point − base) in the sketch plane.
    Sketch {
        /// `id` of the sketch whose centre points place the copies.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sketch: Option<String>,
        /// The original's place (a vertex or work point); none: the centre
        /// point nearest the original (its copy is the original itself).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base: Option<PointRef>,
    },
}

/// What placing the copies needs besides the body: the part's sketches
/// (paths, Sketch Driven) and where the original is (the middle of its
/// tools or solids).
#[derive(Clone, Copy, Default)]
pub struct PlaceCtx<'a> {
    /// The part's sketches.
    pub sketches: &'a [PartSketch],
    /// A point of the original (the middle of its box).
    pub near: Option<Point3>,
}

fn yes() -> bool {
    true
}

fn join() -> Operation {
    Operation::Join
}

/// A Rectangular / Circular Pattern or Mirror feature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatternFeature {
    /// Immutable identity (prefix of the copies' face names).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Kind and placement inputs.
    pub kind: PatternKind,
    /// Ids of the features copied (feature mode).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
    /// Copies whole solids instead of features.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub solid_mode: bool,
    /// The solids copied (solid mode, by persistent name; empty: all).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub solids: Vec<String>,
    /// Solid mode: Join (with the original) or New Solid.
    #[serde(default = "join")]
    pub operation: Operation,
    /// Solid mode: only the copy stays (Mirror's Remove Original); it keeps
    /// the original's body name.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub remove_original: bool,
    /// Equations of its values.
    #[serde(default, skip_serializing_if = "FeatureParams::is_empty")]
    pub params: FeatureParams,
    /// Copies turned off one by one (their numbers, 2 and up; Inventor's
    /// Suppress of an occurrence). The others keep their numbers, so their
    /// face names stay.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suppressed: Vec<usize>,
}

/// A copy's placement: its number (the original is 1) and the rigid motion.
pub type Placement = (usize, nalgebra::Matrix4<Real>);

impl PatternFeature {
    fn with_kind(kind: PatternKind) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            kind,
            features: Vec::new(),
            solid_mode: false,
            solids: Vec::new(),
            operation: Operation::Join,
            remove_original: false,
            params: FeatureParams::default(),
            suppressed: Vec::new(),
        }
    }

    /// A Sketch Driven Pattern without a sketch yet.
    pub fn sketch_driven() -> Self {
        Self::with_kind(PatternKind::Sketch {
            sketch: None,
            base: None,
        })
    }

    /// A rectangular pattern with Inventor's defaults (2 × 10 mm).
    pub fn rectangular() -> Self {
        Self::with_kind(PatternKind::Rectangular {
            first: PatternDirection::new(),
            second: None,
        })
    }

    /// A circular pattern with Inventor's defaults (6 over 360°, Fitted).
    pub fn circular() -> Self {
        Self::with_kind(PatternKind::Circular {
            axis: None,
            flip: false,
            count: 6,
            angle: std::f64::consts::TAU,
            fitted: true,
            midplane: false,
        })
    }

    /// A mirror without a plane yet.
    pub fn mirror() -> Self {
        Self::with_kind(PatternKind::Mirror { plane: None })
    }

    /// `id` of every sketch it reads: Sketch Driven's sketch first, then
    /// the sketches of path directions.
    pub fn sketch_ids(&self) -> Vec<&str> {
        let mut v = Vec::new();
        match &self.kind {
            PatternKind::Sketch {
                sketch: Some(s), ..
            } => v.push(s.as_str()),
            PatternKind::Rectangular { first, second } => {
                for d in std::iter::once(first).chain(second.iter()) {
                    if let Some(id) = d.path.as_ref().and_then(PathRef::sketch_id)
                        && !v.contains(&id)
                    {
                        v.push(id);
                    }
                }
            }
            _ => {}
        }
        v
    }

    /// The command (and id base): `RectangularPattern`, `CircularPattern`,
    /// `Mirror`.
    pub fn id_base(&self) -> &'static str {
        match self.kind {
            PatternKind::Rectangular { .. } => "RectangularPattern",
            PatternKind::Circular { .. } => "CircularPattern",
            PatternKind::Mirror { .. } => "Mirror",
            PatternKind::Sketch { .. } => "SketchPattern",
        }
    }

    /// The command's name as Inventor shows it.
    pub fn command(&self) -> &'static str {
        match self.kind {
            PatternKind::Rectangular { .. } => "Rectangular Pattern",
            PatternKind::Circular { .. } => "Circular Pattern",
            PatternKind::Mirror { .. } => "Mirror",
            PatternKind::Sketch { .. } => "Sketch Driven Pattern",
        }
    }

    /// Instances including the original.
    pub fn instances(&self) -> usize {
        match &self.kind {
            PatternKind::Rectangular { first, second } => {
                first.count as usize
                    * second
                        .as_ref()
                        .filter(|d| d.is_set())
                        .map_or(1, |d| d.count as usize)
            }
            PatternKind::Circular { count, .. } => *count as usize,
            PatternKind::Mirror { .. } => 2,
            // Counted when placed (the sketch's centre points).
            PatternKind::Sketch { .. } => 1,
        }
    }

    /// [`Self::placements_in`] without sketches (straight directions, axes
    /// and planes only).
    pub fn placements(&self, body: &BRep, works: &WorkGeom) -> Result<Vec<Placement>, String> {
        self.placements_in(body, works, &PlaceCtx::default())
    }

    /// The copies' placements (the original and suppressed copies left
    /// out), resolved against the body built so far. Errors are the user's
    /// (missing direction, count out of range…).
    pub fn placements_in(
        &self,
        body: &BRep,
        works: &WorkGeom,
        ctx: &PlaceCtx,
    ) -> Result<Vec<Placement>, String> {
        let mut out = self.all_placements(body, works, ctx)?;
        out.retain(|(k, _)| !self.suppressed.contains(k));
        Ok(out)
    }

    /// Every copy's placement, suppressed ones included.
    pub fn all_placements(
        &self,
        body: &BRep,
        works: &WorkGeom,
        ctx: &PlaceCtx,
    ) -> Result<Vec<Placement>, String> {
        use nalgebra::Matrix4;
        let n = self.instances();
        if n > MAX_INSTANCES {
            return Err(format!("too many instances ({n}; at most {MAX_INSTANCES})"));
        }
        let mut out = Vec::new();
        match &self.kind {
            PatternKind::Rectangular { first, second } => {
                let m1 = moves(first, body, works, ctx, "Direction 1")?;
                let two = second.as_ref().filter(|d| d.is_set());
                let m2 = match two {
                    Some(d) => moves(d, body, works, ctx, "Direction 2")?,
                    None => vec![Some(Matrix4::identity())],
                };
                if m2.len() > 1 && first.path.is_none() && two.is_some_and(|d| d.path.is_none()) {
                    let t = |m: &[Option<Matrix4<Real>>]| {
                        m.iter()
                            .flatten()
                            .map(|x| Vector3::new(x[(0, 3)], x[(1, 3)], x[(2, 3)]))
                            .find(|v| v.norm() > 1e-12)
                    };
                    if let (Some(a), Some(b)) = (t(&m1), t(&m2))
                        && a.cross(&b).norm() < 1e-9 * a.norm() * b.norm()
                    {
                        return Err("Direction 1 and Direction 2 are parallel".into());
                    }
                }
                // Instance `zero_of` of each direction is the original's
                // place; a place off a path (`None`) keeps its number but
                // makes no copy.
                let (z1, z2) = (zero_of(first), two.map_or(0, zero_of));
                let mut k = 1;
                for (j, a) in m2.iter().enumerate() {
                    for (i, b) in m1.iter().enumerate() {
                        if i == z1 && j == z2 {
                            continue;
                        }
                        k += 1;
                        if let (Some(a), Some(b)) = (a, b) {
                            out.push((k, b * a));
                        }
                    }
                }
                if out.is_empty() && self.instances() > 1 {
                    return Err("no instance lies on the path".into());
                }
            }
            PatternKind::Circular {
                axis,
                flip,
                count,
                angle,
                fitted,
                midplane,
            } => {
                let axis = axis
                    .as_ref()
                    .ok_or_else(|| "select the rotation axis".to_owned())?;
                let (o, a) = axis.resolve(body, works)?;
                let a = a
                    .try_normalize(1e-12)
                    .ok_or_else(|| "the axis has no direction".to_owned())?;
                let a = if *flip { -a } else { a };
                let n = *count as usize;
                if n < 1 {
                    return Err("the count must be at least 1".into());
                }
                if !angle.is_finite() {
                    return Err("the angle is not a number".into());
                }
                let full = (angle.abs() - std::f64::consts::TAU).abs() < 1e-9;
                let step = match (*fitted, n) {
                    (_, 1) => 0.0,
                    (true, _) if full => angle / n as Real,
                    (true, _) => angle / (n - 1) as Real,
                    (false, _) => *angle,
                };
                if n > 1 && step.abs() < 1e-9 {
                    return Err("the angle between instances must not be 0".into());
                }
                let shift = if *midplane { (n as i64 - 1) / 2 } else { 0 };
                let mut k = 1;
                for i in 0..n as i64 {
                    let at = i - shift;
                    if at == 0 {
                        continue;
                    }
                    k += 1;
                    let r = nalgebra::Rotation3::from_axis_angle(
                        &nalgebra::Unit::new_unchecked(a),
                        step * at as Real,
                    )
                    .to_homogeneous();
                    let m = Matrix4::new_translation(&o.coords)
                        * r
                        * Matrix4::new_translation(&-o.coords);
                    out.push((k, m));
                }
            }
            PatternKind::Mirror { plane } => {
                let plane = plane
                    .as_ref()
                    .ok_or_else(|| "select the mirror plane".to_owned())?;
                let pl = plane_of(plane, body, works).map_err(|e| e.to_string())?;
                out.push((2, mirror_matrix(pl.origin.coords, pl.normal.into_inner())));
            }
            PatternKind::Sketch { sketch, base } => {
                let id = sketch.as_ref().ok_or("select the sketch")?;
                let ps = ctx
                    .sketches
                    .iter()
                    .find(|s| &s.id == id)
                    .ok_or_else(|| format!("the sketch {id} no longer exists"))?;
                let pl = &ps.sketch.plane;
                let onto = |p: &Point3| {
                    let q = pl.project(p);
                    pl.point_at(q.x, q.y)
                };
                let points: Vec<Point3> = sketch_centres(&ps.sketch)
                    .into_iter()
                    .map(|p| pl.point_at(p.x, p.y))
                    .collect();
                if points.is_empty() {
                    return Err("the sketch has no centre points".into());
                }
                if points.len() > MAX_INSTANCES {
                    return Err(format!(
                        "too many instances ({}; at most {MAX_INSTANCES})",
                        points.len()
                    ));
                }
                // The original's place in the sketch plane.
                let b = match base {
                    Some(r) => onto(&point_of(r, body, works).map_err(|e| e.to_string())?),
                    None => {
                        let near = onto(&ctx.near.unwrap_or_else(Point3::origin));
                        points
                            .iter()
                            .copied()
                            .min_by(|a, b| (near - a).norm().total_cmp(&(near - b).norm()))
                            .unwrap_or(near)
                    }
                };
                let scale = 1.0 + b.coords.norm();
                let mut k = 1;
                for p in points {
                    if (p - b).norm() <= 1e-9 * scale {
                        continue; // the original
                    }
                    k += 1;
                    out.push((k, Matrix4::new_translation(&(p - b))));
                }
                if out.is_empty() {
                    return Err("the sketch has no centre point besides the original's".into());
                }
            }
        }
        Ok(out)
    }
}

/// The centre points of a sketch (Inventor's Sketch Driven Pattern takes
/// these), in the sketch's point order.
pub fn sketch_centres(sk: &crate::sketch::Sketch) -> Vec<crate::sketch::Point2> {
    sk.points()
        .filter(|(_, p)| p.style == crate::sketch::PointStyle::Center)
        .map(|(_, p)| p.position)
        .collect()
}

/// Index of the original's instance in a direction's list.
fn zero_of(d: &PatternDirection) -> usize {
    if d.midplane {
        (d.count.max(1) as usize - 1) / 2
    } else {
        0
    }
}

/// The motions of one direction's instances in order (the original's is
/// the identity at [`zero_of`]); `None` for an instance past the ends of a
/// path.
fn moves(
    d: &PatternDirection,
    body: &BRep,
    works: &WorkGeom,
    ctx: &PlaceCtx,
    what: &str,
) -> Result<Vec<Option<nalgebra::Matrix4<Real>>>, String> {
    use nalgebra::Matrix4;
    let Some(path) = d.path.as_ref().filter(|p| !p.is_empty()) else {
        let (dir, step, offs) = direction(d, body, works, what)?;
        return Ok(offs
            .into_iter()
            .map(|i| Some(Matrix4::new_translation(&(dir * (step * i as Real)))))
            .collect());
    };
    let near = ctx.near.unwrap_or_else(Point3::origin);
    let geom = path.resolve(ctx.sketches, body, near)?;
    let len = geom.length();
    if len < 1e-9 {
        return Err(format!("{what}: the path has no length"));
    }
    // Exact along lines and arcs (`PathGeom::eval`); `None` past the ends.
    let tol = 1e-7 * (1.0 + len);
    let walk = |s: Real| (s >= -tol && s <= len + tol).then(|| geom.eval(s.clamp(0.0, len)));
    let n = d.count as usize;
    if n < 1 {
        return Err(format!("{what}: the count must be at least 1"));
    }
    if !d.spacing.is_finite() {
        return Err(format!("{what}: the spacing is not a number"));
    }
    let step = match (d.fitted, n) {
        (_, 1) => 0.0,
        (true, _) => d.spacing / (n - 1) as Real,
        (false, _) => d.spacing,
    };
    if n > 1 && step.abs() < 1e-9 {
        return Err(format!("{what}: the spacing must not be 0"));
    }
    let step = if d.flip { -step } else { step };
    let z = zero_of(d) as i64;
    let (p0, t0) = geom.eval(0.0);
    Ok((0..n as i64)
        .map(|i| {
            let (p, t) = walk(step * (i - z) as Real)?;
            let mut m = Matrix4::new_translation(&(p - p0));
            if d.follow
                && let Some(r) = nalgebra::Rotation3::rotation_between(&t0, &t)
            {
                m = Matrix4::new_translation(&p.coords)
                    * r.to_homogeneous()
                    * Matrix4::new_translation(&-p.coords)
                    * m;
            }
            Some(m)
        })
        .collect())
}

/// Unit direction, spacing and instance offsets (in steps; 0 = the
/// original) of one rectangular direction.
fn direction(
    d: &PatternDirection,
    body: &BRep,
    works: &WorkGeom,
    what: &str,
) -> Result<(Vector3, Real, Vec<i64>), String> {
    let dir = match d.dir.as_ref() {
        Some(PullRef::Plane(p)) => plane_of(p, body, works)
            .map_err(|e| e.to_string())?
            .normal
            .into_inner(),
        Some(PullRef::Line(l)) => l.resolve(body, works)?.1,
        None => return Err(format!("select {what}")),
    };
    let dir = dir
        .try_normalize(1e-12)
        .ok_or_else(|| format!("{what} has no length"))?;
    let dir = if d.flip { -dir } else { dir };
    let n = d.count as usize;
    if n < 1 {
        return Err(format!("{what}: the count must be at least 1"));
    }
    if !d.spacing.is_finite() {
        return Err(format!("{what}: the spacing is not a number"));
    }
    let step = match (d.fitted, n) {
        (_, 1) => 0.0,
        (true, _) => d.spacing / (n - 1) as Real,
        (false, _) => d.spacing,
    };
    if n > 1 && step.abs() < 1e-9 {
        return Err(format!("{what}: the spacing must not be 0"));
    }
    let shift = if d.midplane { (n as i64 - 1) / 2 } else { 0 };
    Ok((dir, step, (0..n as i64).map(|i| i - shift).collect()))
}

/// Reflection in the plane through `o` with unit normal `n`:
/// `p' = p − 2((p − o)·n) n` (determinant −1; adapted from SolidCraft's
/// `mirror_matrix`, MIT/Apache-2.0).
pub fn mirror_matrix(o: Vector3, n: Vector3) -> nalgebra::Matrix4<Real> {
    let n = n.try_normalize(1e-12).unwrap_or_else(Vector3::z);
    let lin = nalgebra::Matrix3::identity() - 2.0 * n * n.transpose();
    let t = n * (2.0 * o.dot(&n));
    let mut m = lin.to_homogeneous();
    m[(0, 3)] = t.x;
    m[(1, 3)] = t.y;
    m[(2, 3)] = t.z;
    m
}

/// The tools a sketch feature applied (its solids before the operation),
/// kept by regeneration so a pattern can copy them.
#[derive(Clone, Debug)]
pub(super) struct ToolSet {
    /// The solids added, cut or joined.
    pub tools: Vec<crate::kernel::exact::ExactSolid>,
    /// Their operation.
    pub op: Operation,
    /// The bodies operated on (empty: all).
    pub solids: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::Point3;

    fn apply(m: &nalgebra::Matrix4<Real>, p: Point3) -> Point3 {
        m.transform_point(&p)
    }

    #[test]
    fn rectangular_offsets_incremental_fitted_midplane() {
        let body = BRep::new();
        let works = WorkGeom::new();
        let mut p = PatternFeature::rectangular();
        if let PatternKind::Rectangular { first, .. } = &mut p.kind {
            first.dir = Some(PullRef::Line(LineRef::Origin(0)));
            first.count = 3;
            first.spacing = 10.0;
        }
        let xs = |p: &PatternFeature| -> Vec<Real> {
            p.placements(&body, &works)
                .unwrap()
                .iter()
                .map(|(_, m)| apply(m, Point3::origin()).x)
                .collect()
        };
        assert_eq!(xs(&p), vec![10.0, 20.0]);
        let set = |p: &mut PatternFeature, f: &dyn Fn(&mut PatternDirection)| {
            if let PatternKind::Rectangular { first, .. } = &mut p.kind {
                f(first);
            }
        };
        set(&mut p, &|d| d.fitted = true);
        assert_eq!(xs(&p), vec![5.0, 10.0]);
        set(&mut p, &|d| {
            d.fitted = false;
            d.midplane = true;
        });
        assert_eq!(xs(&p), vec![-10.0, 10.0]);
        // Even count: the extra instance on the direction's side; Flip
        // moves it.
        set(&mut p, &|d| d.count = 4);
        assert_eq!(xs(&p), vec![-10.0, 10.0, 20.0]);
        set(&mut p, &|d| d.flip = true);
        assert_eq!(xs(&p), vec![10.0, -10.0, -20.0]);
    }

    #[test]
    fn two_directions_number_copies_row_by_row() {
        let body = BRep::new();
        let works = WorkGeom::new();
        let mut p = PatternFeature::rectangular();
        if let PatternKind::Rectangular { first, second } = &mut p.kind {
            first.dir = Some(PullRef::Line(LineRef::Origin(0)));
            first.count = 3;
            let mut d2 = PatternDirection::new();
            d2.dir = Some(PullRef::Plane(PlaneRef::Origin(
                super::super::OriginPlane::XZ,
            )));
            d2.flip = true; // the XZ normal is −Y
            d2.spacing = 5.0;
            *second = Some(d2);
        }
        let v = p.placements(&body, &works).unwrap();
        assert_eq!(v.len(), 5);
        assert_eq!(v.iter().map(|x| x.0).collect::<Vec<_>>(), [2, 3, 4, 5, 6]);
        let last = apply(&v[4].1, Point3::origin());
        assert!((last - Point3::new(20.0, 5.0, 0.0)).norm() < 1e-12);
        // Parallel directions are refused.
        if let PatternKind::Rectangular {
            second: Some(d2), ..
        } = &mut p.kind
        {
            d2.dir = Some(PullRef::Line(LineRef::Origin(0)));
        }
        assert!(
            p.placements(&body, &works)
                .unwrap_err()
                .contains("parallel")
        );
    }

    #[test]
    fn circular_full_circle_and_partial() {
        let body = BRep::new();
        let works = WorkGeom::new();
        let mut p = PatternFeature::circular();
        assert!(p.placements(&body, &works).is_err(), "no axis yet");
        if let PatternKind::Circular { axis, .. } = &mut p.kind {
            *axis = Some(LineRef::Origin(2));
        }
        let v = p.placements(&body, &works).unwrap();
        assert_eq!(v.len(), 5);
        let q = apply(&v[0].1, Point3::new(10.0, 0.0, 0.0));
        assert!(
            (q - Point3::new(5.0, 75f64.sqrt(), 0.0)).norm() < 1e-9,
            "{q}"
        );
        if let PatternKind::Circular {
            count,
            angle,
            midplane,
            fitted,
            ..
        } = &mut p.kind
        {
            *fitted = false;
            *count = 3;
            *angle = std::f64::consts::FRAC_PI_2;
            *midplane = true;
        }
        let v = p.placements(&body, &works).unwrap();
        let q = apply(&v[0].1, Point3::new(10.0, 0.0, 0.0));
        assert!((q - Point3::new(0.0, -10.0, 0.0)).norm() < 1e-9, "{q}");
    }

    #[test]
    fn mirror_is_a_reflection() {
        let m = mirror_matrix(Vector3::new(5.0, 0.0, 0.0), Vector3::x());
        let q = m.transform_point(&Point3::new(7.0, 1.0, 2.0));
        assert!((q - Point3::new(3.0, 1.0, 2.0)).norm() < 1e-12);
        assert!((m.fixed_view::<3, 3>(0, 0).determinant() + 1.0).abs() < 1e-12);
    }

    #[test]
    fn too_many_instances_are_refused() {
        let mut p = PatternFeature::circular();
        if let PatternKind::Circular { axis, count, .. } = &mut p.kind {
            *axis = Some(LineRef::Origin(2));
            *count = 10001;
        }
        let e = p.placements(&BRep::new(), &WorkGeom::new()).unwrap_err();
        assert!(e.contains("too many"), "{e}");
    }

    /// Box 60×40×10 with a through hole r 3 at (10, 10) (`Extrusion1`).
    fn holed_box() -> super::super::PartDocument {
        use super::super::{
            Extent, ExtentDirection, ExtrudeFeature, PartDocument, ProfileRef, SketchOptions,
            SketchPlacement,
        };
        let mut part = PartDocument::with_box("Box1", [60.0, 40.0, 10.0]).unwrap();
        let opts = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let j = part
            .create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)
            .unwrap();
        let c = part.sketches[j]
            .sketch
            .plane
            .project(&Point3::new(10.0, 10.0, 10.0));
        part.sketches[j].sketch.add_circle(c, 3.0).unwrap();
        let r = part.sketches[j].sketch.regions();
        let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], 1.0);
        ex.operation = Operation::Cut;
        ex.extent = Extent::ThroughAll;
        ex.direction = ExtentDirection::Flipped;
        part.add_extrude(j, ex).unwrap();
        part
    }

    fn volume(part: &super::super::PartDocument) -> Real {
        part.exact_bodies
            .0
            .iter()
            .map(|s| s.measure().unwrap().0)
            .sum()
    }

    #[test]
    fn feature_pattern_names_copies_and_survives_a_round_trip() {
        let mut part = holed_box();
        let hole = std::f64::consts::PI * 9.0 * 10.0;
        let mut p = PatternFeature::rectangular();
        p.features = vec!["Extrusion1".into()];
        if let PatternKind::Rectangular { first, .. } = &mut p.kind {
            first.dir = Some(PullRef::Line(LineRef::Origin(0)));
            first.count = 3;
            first.spacing = 20.0;
        }
        let id = part.add_pattern(p).unwrap();
        assert_eq!(id, "RectangularPattern1");
        assert_eq!(part.features[2].name(), "Rectangular Pattern1");
        assert!(part.exact);
        let v = volume(&part);
        assert!((v - (24_000.0 - 3.0 * hole)).abs() < 2e-3 * v, "{v}");
        let names: Vec<String> = part
            .body
            .faces()
            .filter_map(|(_, f)| f.name.clone())
            .collect();
        for k in [2, 3] {
            let pre = format!("RectangularPattern1#{k}:Extrusion1:");
            assert!(
                names.iter().any(|n| n.starts_with(&pre)),
                "{pre} in {names:?}"
            );
            // A click on a copy's face selects the pattern.
            let n = names.iter().find(|n| n.starts_with(&pre)).unwrap();
            assert_eq!(
                super::super::select::feature_id_of_face(n),
                Some("RectangularPattern1")
            );
        }
        // Count as a parameter: 3 → 2 copies.
        part.set_feature_field(2, "count1", "2").unwrap();
        part.regenerate().unwrap();
        let v = volume(&part);
        assert!((v - (24_000.0 - 2.0 * hole)).abs() < 2e-3 * v, "{v}");
        // Save and load (JSON, as in the project file).
        let text = serde_json::to_string(&part).unwrap();
        let mut back: super::super::PartDocument = serde_json::from_str(&text).unwrap();
        back.regenerate().unwrap();
        assert_eq!(back.features, part.features);
        assert!((volume(&back) - volume(&part)).abs() < 1e-6);
    }

    #[test]
    fn a_failing_pattern_leaves_the_part_unchanged() {
        let mut part = holed_box();
        let (features, v) = (part.features.clone(), volume(&part));
        let mut p = PatternFeature::mirror();
        p.features = vec!["Box1".into()];
        p.kind = PatternKind::Mirror {
            plane: Some(PlaneRef::Origin(super::super::OriginPlane::YZ)),
        };
        let e = part.add_pattern(p.clone()).unwrap_err().to_string();
        assert!(e.contains("cannot be patterned on its own"), "{e}");
        assert_eq!(part.features, features);
        assert!((volume(&part) - v).abs() < 1e-9);
        // The hole mirrored across YZ lands beside the box: no copy cuts.
        p.features = vec!["Extrusion1".into()];
        let e = part.add_pattern(p).unwrap_err().to_string();
        assert!(e.contains("none of the copies"), "{e}");
        assert_eq!(part.features, features);
        assert!(part.feature_health.is_empty());
    }

    /// The middle of copy `k`'s faces (the display triangles' points).
    fn copy_centre(part: &super::super::PartDocument, id: &str, k: usize) -> Option<Point3> {
        let pre = format!("{id}#{k}:");
        let pts: Vec<Point3> = part
            .body
            .faces()
            .filter(|(_, f)| f.name.as_deref().is_some_and(|n| n.starts_with(&pre)))
            .filter_map(|(_, f)| f.mesh.as_ref())
            .flat_map(|m| m.positions.iter().copied())
            .collect();
        (!pts.is_empty()).then(|| {
            pts.iter()
                .fold(Point3::origin(), |a, p| a + p.coords / pts.len() as Real)
        })
    }

    fn top_sketch(part: &mut super::super::PartDocument) -> usize {
        use super::super::{SketchOptions, SketchPlacement};
        let opts = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        part.create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)
            .unwrap()
    }

    /// Suppress of single copies (Inventor: Suppress of an occurrence):
    /// 4 holes along X with copy 3 suppressed cut 3 holes, the others keep
    /// their numbers (names) and a save and load keeps it.
    #[test]
    fn suppressed_copies_are_left_out() {
        let hole = std::f64::consts::PI * 9.0 * 10.0;
        let cases: [(u32, &[usize]); 5] = [
            (4, &[3]),
            (4, &[2, 4]),
            (5, &[5]),
            (3, &[]),
            (6, &[2, 3, 4, 5]),
        ];
        for (count, off) in cases {
            let mut part = holed_box();
            let mut p = PatternFeature::rectangular();
            p.features = vec!["Extrusion1".into()];
            if let PatternKind::Rectangular { first, .. } = &mut p.kind {
                first.dir = Some(PullRef::Line(LineRef::Origin(0)));
                first.count = count;
                first.spacing = 9.0;
            }
            p.suppressed = off.to_vec();
            part.add_pattern(p).unwrap();
            let holes = count as usize - off.len();
            let v = volume(&part);
            let want = 24_000.0 - holes as Real * hole;
            assert!(
                (v - want).abs() < 2e-3 * want,
                "{count} {off:?}: {v} vs {want}"
            );
            for k in 2..=count as usize {
                assert_eq!(
                    copy_centre(&part, "RectangularPattern1", k).is_some(),
                    !off.contains(&k),
                    "copy {k} of {count}, suppressed {off:?}"
                );
            }
            let text = serde_json::to_string(&part).unwrap();
            let mut back: super::super::PartDocument = serde_json::from_str(&text).unwrap();
            back.regenerate().unwrap();
            assert!((volume(&back) - v).abs() < 1e-6);
        }
    }

    /// Rectangular Pattern along a curve: the hole at (10, 10) copied along
    /// a half circle of r 20 about (30, 10) on the top face. Fitted over the
    /// arc length: copies at its top (30, 30) and its end (50, 10);
    /// Incremental 10 mm: at arc lengths 10 and 20. Orientation Direction
    /// turns them about the arc normal (a round hole looks the same: the
    /// centres must still match).
    #[test]
    fn pattern_along_a_curve() {
        use super::super::PathRef;
        let hole = std::f64::consts::PI * 9.0 * 10.0;
        for (fitted, follow) in [(true, false), (true, true), (false, false), (false, true)] {
            let mut part = holed_box();
            let j = top_sketch(&mut part);
            let pl = part.sketches[j].sketch.plane.clone();
            let at = |x: Real, y: Real| pl.project(&Point3::new(x, y, 10.0));
            // Counter-clockwise in the sketch: through y = 30 one way round
            // or the other, depending on the sketch axes.
            let q = at(10.0, 10.0) - at(30.0, 10.0);
            let turned = at(30.0, 10.0) + crate::sketch::Vector2::new(-q.y, q.x);
            let top = pl.point_at(turned.x, turned.y).y > 10.0;
            let sk = &mut part.sketches[j].sketch;
            let c = sk.add_point(at(30.0, 10.0));
            let a = sk.add_point(at(10.0, 10.0));
            let b = sk.add_point(at(50.0, 10.0));
            let arc = if top {
                sk.add_arc(c, a, b).unwrap()
            } else {
                sk.add_arc(c, b, a).unwrap()
            };
            let sid = part.sketches[j].id.clone();
            let mut p = PatternFeature::rectangular();
            p.features = vec!["Extrusion1".into()];
            let len = std::f64::consts::PI * 20.0;
            if let PatternKind::Rectangular { first, .. } = &mut p.kind {
                first.path = Some(PathRef::Sketch {
                    sketch: sid.clone(),
                    curves: vec![arc],
                });
                first.count = 3;
                first.fitted = fitted;
                first.spacing = if fitted { len } else { 10.0 };
                first.follow = follow;
            }
            part.add_pattern(p).unwrap();
            let v = volume(&part);
            let want = 24_000.0 - 3.0 * hole;
            assert!((v - want).abs() < 2e-3 * want, "{v} vs {want}");
            for k in [2usize, 3] {
                let s = if fitted { len / 2.0 } else { 10.0 } * (k - 1) as Real;
                let t = std::f64::consts::PI - s / 20.0;
                let w = Point3::new(30.0 + 20.0 * t.cos(), 10.0 + 20.0 * t.sin(), 5.0);
                let got = copy_centre(&part, "RectangularPattern1", k).expect("the copy");
                // The hole walls are round: their middle is the centre (a
                // polyline path: within the arc's sampling).
                assert!(
                    ((got.x - w.x).powi(2) + (got.y - w.y).powi(2)).sqrt() < 0.05,
                    "copy {k} (fitted {fitted}, follow {follow}): {got} vs {w}"
                );
            }
            // The path's sketch is the pattern's: a save and load builds
            // the same.
            assert_eq!(part.features[2].sketch_ids(), vec![sid.as_str()]);
            let text = serde_json::to_string(&part).unwrap();
            let mut back: super::super::PartDocument = serde_json::from_str(&text).unwrap();
            back.regenerate().unwrap();
            assert!((volume(&back) - v).abs() < 1e-6);
        }
    }

    /// A path too short for the copies: the ones past its end are left out;
    /// none on it is an error and the part is unchanged.
    #[test]
    fn copies_past_the_path_end_are_left_out() {
        use super::super::PathRef;
        let hole = std::f64::consts::PI * 9.0 * 10.0;
        let mut part = holed_box();
        let j = top_sketch(&mut part);
        let pl = part.sketches[j].sketch.plane.clone();
        let at = |x: Real, y: Real| pl.project(&Point3::new(x, y, 10.0));
        let line = part.sketches[j]
            .sketch
            .add_line(at(10.0, 10.0), at(35.0, 10.0));
        let sid = part.sketches[j].id.clone();
        let make = |count: u32, flip: bool| {
            let mut p = PatternFeature::rectangular();
            p.features = vec!["Extrusion1".into()];
            if let PatternKind::Rectangular { first, .. } = &mut p.kind {
                first.path = Some(PathRef::Sketch {
                    sketch: sid.clone(),
                    curves: vec![line],
                });
                first.count = count;
                first.spacing = 10.0;
                first.flip = flip;
            }
            p
        };
        // 5 asked, 10 mm apart on a 25 mm line: 0, 10, 20 fit.
        let mut a = part.clone();
        a.add_pattern(make(5, false)).unwrap();
        let v = volume(&a);
        assert!((v - (24_000.0 - 3.0 * hole)).abs() < 2e-3 * v, "{v}");
        assert!(copy_centre(&a, "RectangularPattern1", 4).is_none());
        // Flipped from the start: nothing lies on the path.
        let features = part.features.clone();
        let e = part.add_pattern(make(3, true)).unwrap_err().to_string();
        assert!(e.contains("path"), "{e}");
        assert_eq!(part.features, features);
    }

    /// Sketch Driven Pattern: the hole at (10, 10) copied to every centre
    /// point of a sketch on the top face; the point at the original makes no
    /// copy (default base point), a vertex as the base point moves them all
    /// by (point - vertex). A save and load builds the same.
    #[test]
    fn sketch_driven_pattern() {
        use super::super::PointRef;
        let hole = std::f64::consts::PI * 9.0 * 10.0;
        let cases: [(&[(Real, Real)], usize); 5] = [
            (&[(10.0, 10.0), (30.0, 10.0), (50.0, 30.0)], 2),
            (&[(10.0, 10.0), (20.0, 30.0)], 1),
            (
                &[
                    (10.0, 10.0),
                    (25.0, 20.0),
                    (40.0, 20.0),
                    (50.0, 10.0),
                    (50.0, 30.0),
                ],
                4,
            ),
            // The original off every point: the nearest stands for it.
            (&[(11.0, 10.0), (31.0, 10.0)], 1),
            (&[(10.0, 10.0)], 0),
        ];
        for (points, copies) in cases {
            let mut part = holed_box();
            let j = top_sketch(&mut part);
            let pl = part.sketches[j].sketch.plane.clone();
            for &(x, y) in points {
                let q = pl.project(&Point3::new(x, y, 10.0));
                part.sketches[j].sketch.add_center_point(q);
            }
            // A plain point is no centre point: no copy there.
            let q = pl.project(&Point3::new(40.0, 35.0, 10.0));
            part.sketches[j].sketch.add_point(q);
            let sid = part.sketches[j].id.clone();
            let mut p = PatternFeature::sketch_driven();
            p.features = vec!["Extrusion1".into()];
            p.kind = PatternKind::Sketch {
                sketch: Some(sid.clone()),
                base: None,
            };
            if copies == 0 {
                let before = part.features.clone();
                assert!(part.add_pattern(p).is_err(), "no copy: nothing to do");
                assert_eq!(part.features, before);
                continue;
            }
            let id = part.add_pattern(p).unwrap();
            assert_eq!(id, "SketchPattern1");
            assert_eq!(part.features[2].name(), "Sketch Driven Pattern1");
            let v = volume(&part);
            let want = 24_000.0 - (1 + copies) as Real * hole;
            assert!((v - want).abs() < 2e-3 * want, "{points:?}: {v} vs {want}");
            let dx = points[0].0 - 10.0;
            for (k, &(x, y)) in points.iter().skip(1).enumerate() {
                let c = copy_centre(&part, &id, k + 2).expect("a copy");
                assert!(
                    (c.x - (x - dx)).abs() < 0.05 && (c.y - y).abs() < 0.05,
                    "copy {} at {c}, want ({}, {y})",
                    k + 2,
                    x - dx
                );
            }
            let text = serde_json::to_string(&part).unwrap();
            let mut back: super::super::PartDocument = serde_json::from_str(&text).unwrap();
            back.regenerate().unwrap();
            assert!((volume(&back) - v).abs() < 1e-6);
        }
        // A vertex as the base point: the box corner (0, 0, 10); the centre
        // point (15, 5) moves the hole at (10, 10) to (25, 15).
        let mut part = holed_box();
        let j = top_sketch(&mut part);
        let pl = part.sketches[j].sketch.plane.clone();
        let q = pl.project(&Point3::new(15.0, 5.0, 10.0));
        part.sketches[j].sketch.add_center_point(q);
        let corner = part
            .body
            .vertices()
            .find(|(_, v)| (v.point - Point3::new(0.0, 0.0, 10.0)).norm() < 1e-9)
            .and_then(|(_, v)| v.name.clone())
            .expect("the corner name");
        let mut p = PatternFeature::sketch_driven();
        p.features = vec!["Extrusion1".into()];
        p.kind = PatternKind::Sketch {
            sketch: Some(part.sketches[j].id.clone()),
            base: Some(PointRef::Vertex(corner)),
        };
        let id = part.add_pattern(p).unwrap();
        let c = copy_centre(&part, &id, 2).expect("the copy");
        assert!(
            (c.x - 25.0).abs() < 0.05 && (c.y - 15.0).abs() < 0.05,
            "{c}"
        );
    }

    #[test]
    fn serde_round_trip() {
        let mut p = PatternFeature::circular();
        p.features = vec!["Extrusion2".into()];
        let s = serde_json::to_string(&p).unwrap();
        let back: PatternFeature = serde_json::from_str(&s).unwrap();
        assert_eq!(back, p);
        let mut m = PatternFeature::mirror();
        m.solid_mode = true;
        m.remove_original = true;
        let s = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<PatternFeature>(&s).unwrap(), m);
    }
}
