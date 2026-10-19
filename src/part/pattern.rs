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

use crate::kernel::{BRep, Real, Vector3};

use super::boolean::Operation;
use super::draft::PullRef;
use super::params::FeatureParams;
use super::placement::{PlaneRef, WorkGeom, plane_of};
use super::work::LineRef;

/// Most instances (copies and original) one pattern may have (SolidCraft's
/// limit as well).
pub const MAX_INSTANCES: usize = 1000;

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
        }
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
        }
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

    /// The command (and id base): `RectangularPattern`, `CircularPattern`,
    /// `Mirror`.
    pub fn id_base(&self) -> &'static str {
        match self.kind {
            PatternKind::Rectangular { .. } => "RectangularPattern",
            PatternKind::Circular { .. } => "CircularPattern",
            PatternKind::Mirror { .. } => "Mirror",
        }
    }

    /// The command's name as Inventor shows it.
    pub fn command(&self) -> &'static str {
        match self.kind {
            PatternKind::Rectangular { .. } => "Rectangular Pattern",
            PatternKind::Circular { .. } => "Circular Pattern",
            PatternKind::Mirror { .. } => "Mirror",
        }
    }

    /// Instances including the original.
    pub fn instances(&self) -> usize {
        match &self.kind {
            PatternKind::Rectangular { first, second } => {
                first.count as usize
                    * second
                        .as_ref()
                        .filter(|d| d.dir.is_some())
                        .map_or(1, |d| d.count as usize)
            }
            PatternKind::Circular { count, .. } => *count as usize,
            PatternKind::Mirror { .. } => 2,
        }
    }

    /// The copies' placements (the original left out), resolved against
    /// the body built so far. Errors are the user's (missing direction,
    /// count out of range…).
    pub fn placements(&self, body: &BRep, works: &WorkGeom) -> Result<Vec<Placement>, String> {
        use nalgebra::Matrix4;
        let n = self.instances();
        if n > MAX_INSTANCES {
            return Err(format!("too many instances ({n}; at most {MAX_INSTANCES})"));
        }
        let mut out = Vec::new();
        match &self.kind {
            PatternKind::Rectangular { first, second } => {
                let (d1, s1, o1) = direction(first, body, works, "Direction 1")?;
                let (d2, s2, o2) = match second.as_ref().filter(|d| d.dir.is_some()) {
                    Some(d) => direction(d, body, works, "Direction 2")?,
                    None => (Vector3::zeros(), 0.0, vec![0]),
                };
                if (d1.cross(&d2).norm() < 1e-9) && o2.len() > 1 {
                    return Err("Direction 1 and Direction 2 are parallel".into());
                }
                let mut k = 1;
                for &j in &o2 {
                    for &i in &o1 {
                        if i == 0 && j == 0 {
                            continue;
                        }
                        k += 1;
                        let t = d1 * (s1 * i as Real) + d2 * (s2 * j as Real);
                        out.push((k, Matrix4::new_translation(&t)));
                    }
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
        }
        Ok(out)
    }
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
            *count = 1001;
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
