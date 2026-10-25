//! Sheet metal parts (`docs/spec/5.6-sheet-metal.md`, stage S1): the
//! document's Sheet Metal Defaults (one rule: thickness, bend radius,
//! K-factor, reliefs, gap), the **Face** (a plate from a sketch profile)
//! and **Flange** (bends on sheet edges) features, and the flat pattern.
//! The sheet model itself is SolidCraft's, behind
//! `kernel::exact::sheet_metal`; regeneration keeps one model per sheet body
//! (`PartDocument::sheet_models`, runtime) next to the exact solids.

use serde::{Deserialize, Serialize};

use super::extrude::ProfileRef;
use crate::kernel::Real;
use crate::kernel::exact::sheet_metal as ksm;

/// Sheet Metal Defaults: the part's rule. `None` values follow the
/// thickness as Inventor's default rule does (`BendRadius = Thickness`…).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetMetalDefaults {
    /// Thickness (mm).
    pub thickness: Real,
    /// Inner bend radius (mm); `None`: the thickness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bend_radius: Option<Real>,
    /// K-factor of the linear unfold rule (0..1).
    #[serde(default = "k_default")]
    pub k_factor: Real,
    /// Bend relief width; `None`: the thickness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relief_width: Option<Real>,
    /// Bend relief depth; `None`: half the thickness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relief_depth: Option<Real>,
    /// Corner relief size; `None`: four times the thickness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner_relief: Option<Real>,
    /// Gap between flanges meeting at a corner; `None`: the thickness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<Real>,
}

fn k_default() -> Real {
    0.44
}

impl Default for SheetMetalDefaults {
    fn default() -> Self {
        Self {
            thickness: 2.0,
            bend_radius: None,
            k_factor: k_default(),
            relief_width: None,
            relief_depth: None,
            corner_relief: None,
            gap: None,
        }
    }
}

impl SheetMetalDefaults {
    /// The values with their defaults filled in.
    pub fn values(&self) -> ksm::RuleValues {
        let t = self.thickness;
        ksm::RuleValues {
            thickness: t,
            k_factor: self.k_factor,
            bend_radius: self.bend_radius.unwrap_or(t),
            relief_width: self.relief_width.unwrap_or(t),
            relief_depth: self.relief_depth.unwrap_or(t * 0.5),
            corner_relief: self.corner_relief.unwrap_or(t * 4.0),
            gap: self.gap.unwrap_or(t),
        }
    }

    /// Checks the rule (an error names what is wrong).
    pub fn validate(&self) -> Result<(), String> {
        let v = self.values();
        if !(v.thickness.is_finite() && v.thickness > 0.0) {
            return Err("the thickness must be positive".into());
        }
        if !(v.bend_radius.is_finite() && v.bend_radius > 0.0) {
            return Err("the bend radius must be positive".into());
        }
        if !(0.0..=1.0).contains(&v.k_factor) {
            return Err("the K-factor must be between 0 and 1".into());
        }
        Ok(())
    }
}

/// Face: a plate from closed profiles of a sketch, the rule's thickness
/// thick.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetFaceFeature {
    /// Immutable id (`Face1`): the sheet body's name and its faces' prefix.
    pub id: String,
    /// Display name.
    pub name: String,
    /// The sketch.
    pub sketch: String,
    /// Its profile(s) (the first makes the sheet).
    pub profiles: Vec<ProfileRef>,
    /// The thickness on the other side of the sketch plane.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
}

/// Where a flange's bend lies against its edge (Inventor's Bend Position).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlangePosition {
    /// Inside of the base face extents: the base keeps its size.
    #[default]
    Inside,
    /// Outside of the base face extents: the bend starts at the edge.
    Outside,
    /// The bend centred on the edge.
    Middle,
}

impl FlangePosition {
    /// Label (panel).
    pub fn label(self) -> &'static str {
        match self {
            FlangePosition::Inside => "Inside of base face extents",
            FlangePosition::Outside => "Outside of base face extents",
            FlangePosition::Middle => "Bend centred on the edge",
        }
    }

    /// The kernel's value.
    pub(super) fn kernel_pub(self) -> ksm::BendPosition {
        match self {
            FlangePosition::Inside => ksm::BendPosition::Inside,
            FlangePosition::Outside => ksm::BendPosition::Outside,
            FlangePosition::Middle => ksm::BendPosition::Middle,
        }
    }
}

/// A sheet edge by the sheet's persistent edge name and a point near the
/// pick (part coordinates).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetEdge {
    /// The sheet's name of the edge.
    pub name: String,
    /// Where it was picked.
    pub hint: [Real; 3],
}

/// Flange: bends on sheet edges, all with one height and angle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetFlangeFeature {
    /// Immutable id (`Flange1`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// The sheet body (its Face's id).
    pub body: String,
    /// The edges.
    pub edges: Vec<SheetEdge>,
    /// Height to the outer face, from the virtual sharp (mm).
    pub height: Real,
    /// Bend angle (radians, 0..π).
    pub angle: Real,
    /// Bend radius; `None`: the rule's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<Real>,
    /// Bend Position.
    #[serde(default)]
    pub position: FlangePosition,
    /// The flange to the other side.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    /// Equations of the fields (`height`, `angle`).
    #[serde(
        default,
        skip_serializing_if = "super::params::FeatureParams::is_empty"
    )]
    pub params: super::params::FeatureParams,
}

impl SheetFlangeFeature {
    /// Its numeric fields (parameters).
    pub fn fields(&self) -> Vec<(&'static str, super::expr::Unit)> {
        use super::expr::Unit;
        vec![("height", Unit::Mm), ("angle", Unit::Deg)]
    }

    /// A field's value.
    pub fn value(&self, field: &str) -> Option<Real> {
        match field {
            "height" => Some(self.height),
            "angle" => Some(self.angle),
            _ => None,
        }
    }

    /// Sets a field.
    pub fn set(&mut self, field: &str, v: Real) {
        match field {
            "height" => self.height = v,
            "angle" => self.angle = v,
            _ => {}
        }
    }

    /// A flange of `height` at 90° on `edges` of `body`.
    pub fn new(body: &str, edges: Vec<SheetEdge>, height: Real) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            body: body.to_owned(),
            edges,
            height,
            angle: std::f64::consts::FRAC_PI_2,
            radius: None,
            position: FlangePosition::Inside,
            flip: false,
            params: Default::default(),
        }
    }
}

/// A sheet body's model with the face count the sheet steps left it with:
/// a Hole or Cut on it later is not in the model yet (stage S2), so a
/// further flange would drop it — that is refused.
#[derive(Clone, Debug)]
pub struct SheetState {
    /// The model.
    pub model: ksm::SheetModel,
    /// Faces of the solid when the last sheet step made it.
    pub faces: usize,
}

/// The flat pattern of a sheet body.
#[derive(Clone, Debug)]
pub struct FlatPattern {
    /// The sheet body's name.
    pub body: String,
    /// Length and width of the flat (mm).
    pub size: (Real, Real),
    /// Its bend lines.
    pub bends: Vec<ksm::BendLine>,
    /// The flat solid (for the view).
    pub solid: crate::kernel::exact::ExactSolid,
}

/// The rule of a part: its defaults, or Inventor's default rule.
pub fn rule_of(part: &super::PartDocument) -> SheetMetalDefaults {
    part.sheet_metal.clone().unwrap_or_default()
}

impl super::PartDocument {
    /// Makes the part a sheet metal part (Convert to Sheet Metal / a new
    /// Sheet Metal part): its rule is `rule` (Inventor's default if `None`).
    pub fn make_sheet_metal(&mut self, rule: Option<SheetMetalDefaults>) {
        self.sheet_metal = Some(rule.unwrap_or_default());
    }

    /// **Face**: a plate from the first default profile of sketch
    /// `sketch` (id `Face<N>`). On failure nothing changes.
    pub fn add_sheet_face(
        &mut self,
        sketch: usize,
        profiles: Vec<ProfileRef>,
        flip: bool,
    ) -> Result<String, super::PartError> {
        if self.sheet_metal.is_none() {
            self.make_sheet_metal(None);
        }
        let ps = self
            .sketches
            .get(sketch)
            .ok_or_else(|| super::PartError::Feature("no such sketch".into()))?;
        let id = self.next_feature_id("Face");
        let def = SheetFaceFeature {
            id: id.clone(),
            name: id.clone(),
            sketch: ps.id.clone(),
            profiles,
            flip,
        };
        self.insert_feature_checked(super::Feature::SheetFace(def))?;
        Ok(id)
    }

    /// **Flange** (id `Flange<N>`). On failure nothing changes.
    pub fn add_sheet_flange(
        &mut self,
        mut def: SheetFlangeFeature,
    ) -> Result<String, super::PartError> {
        let id = self.next_feature_id("Flange");
        (def.id, def.name) = (id.clone(), id.clone());
        self.insert_feature_checked(super::Feature::SheetFlange(def))?;
        Ok(id)
    }

    /// The sheet edge nearest `at` (part coordinates): (sheet body, edge
    /// name) — what a Flange stores.
    pub fn sheet_edge_at(&self, at: crate::kernel::Point3) -> Option<(String, SheetEdge)> {
        self.sheet_models
            .iter()
            .filter_map(|(body, st)| {
                let name = ksm::edge_name(&st.model, at)?;
                let p = ksm::edge_point(&st.model, &name, at)?;
                Some(((p - at).norm(), body.clone(), name))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, body, name)| {
                (
                    body,
                    SheetEdge {
                        name,
                        hint: [at.x, at.y, at.z],
                    },
                )
            })
    }

    /// The flat patterns of the sheet bodies (Create Flat Pattern).
    pub fn flat_patterns(&self) -> Vec<FlatPattern> {
        let mut v: Vec<FlatPattern> = self
            .sheet_models
            .iter()
            .filter_map(|(body, st)| {
                Some(FlatPattern {
                    body: body.clone(),
                    size: ksm::flat_size(&st.model).ok()?,
                    bends: ksm::bends(&st.model),
                    solid: ksm::flat_solid(&st.model, body).ok()?,
                })
            })
            .collect();
        v.sort_by(|a, b| a.body.cmp(&b.body));
        v
    }
}

#[cfg(test)]
mod tests {
    use crate::kernel::{Point3, Real};
    use crate::part::{OriginPlane, PartDocument, SketchOptions, SketchPlacement};
    use crate::sketch::Point2;

    fn volume(p: &PartDocument) -> Real {
        p.exact_bodies
            .0
            .iter()
            .filter_map(|s| s.volume_at(1e-3))
            .sum()
    }

    /// A 100 × 60 plate 2 thick (Face), a 25 mm flange on its front edge;
    /// the flat pattern is longer by the developed flange; save / load
    /// keeps both; a missing edge makes the flange sick.
    #[test]
    fn face_flange_and_flat_pattern_in_a_part() {
        let mut p = PartDocument::default();
        p.make_sheet_metal(None);
        let o = SketchOptions {
            auto_project_origin: false,
            ..Default::default()
        };
        let i = p
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
            .unwrap();
        p.sketches[i]
            .sketch
            .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(100.0, 60.0))
            .unwrap();
        let r = p.sketches[i].sketch.regions();
        let prof = vec![crate::part::ProfileRef::of(&r[0], r[0].interior_point())];
        let face = p.add_sheet_face(i, prof, false).unwrap();
        assert!(p.exact);
        assert!((volume(&p) - 12_000.0).abs() < 1.0, "{}", volume(&p));
        let (body, edge) = p
            .sheet_edge_at(Point3::new(50.0, 0.0, 2.0))
            .expect("an edge");
        assert_eq!(body, face);
        let fl = super::SheetFlangeFeature::new(&body, vec![edge], 25.0);
        p.add_sheet_flange(fl).unwrap();
        assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
        let flats = p.flat_patterns();
        assert_eq!(flats.len(), 1);
        let (a, b) = flats[0].size;
        let ba = std::f64::consts::FRAC_PI_2 * (2.0 + 0.44 * 2.0);
        let want = 60.0 - 4.0 + ba + (25.0 - 4.0);
        assert!((a.min(b) - want).abs() < 0.05, "{a} × {b}, want {want}");
        assert_eq!(flats[0].bends.len(), 1);
        // The sheet's faces are named.
        assert!(p.body.faces().all(|(_, f)| f.name.is_some()));
        // Round trip through JSON (the file's part document).
        let json = serde_json::to_string(&p).unwrap();
        let mut q: PartDocument = serde_json::from_str(&json).unwrap();
        q.regenerate().unwrap();
        assert!((volume(&q) - volume(&p)).abs() < 1e-6 * volume(&p));
        assert_eq!(q.flat_patterns()[0].size, flats[0].size);
        // A thicker rule: everything follows.
        q.sheet_metal.as_mut().unwrap().thickness = 3.0;
        q.regenerate().unwrap();
        assert!(q.feature_health.is_empty(), "{:?}", q.feature_health);
        assert!(volume(&q) > volume(&p) * 1.4);
    }
}
