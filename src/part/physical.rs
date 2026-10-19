//! Physical Properties (`docs/spec/0.10-physical.md`, Inventor's
//! iProperties → Physical): mass, volume, area, centre of gravity and the
//! inertia tensor of every solid body and of the whole part.
//!
//! Integrals come from the body's display triangles with the divergence
//! theorem: each outward triangle `(a, b, c)` and the origin span a signed
//! tetrahedron, whose volume, first moment and second moment
//! `∫ x xᵀ dV = det/120 · (Σ pᵢpᵢᵀ + (Σ pᵢ)(Σ pᵢ)ᵀ)` are exact. Planar faces
//! are therefore exact; curved ones as fine as their tessellation (the
//! accuracy setting; faces from the exact kernel carry its own mesh).
//!
//! Units: mm, g/cm³, g (1 mm³ at 1 g/cm³ = 0.001 g); moments in g·mm².

use nalgebra::{Matrix3, SymmetricEigen};

use super::PartDocument;
use super::appearance::Resolver;
use crate::kernel::{BRep, Point3, Real, SolidId, TessellationOptions, Vector3};

/// Requested Accuracy (Inventor's levels): chord tolerance of the
/// tessellation of analytic faces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Accuracy {
    /// 0.1 mm (Inventor's default level).
    #[default]
    Low,
    /// 0.02 mm.
    Medium,
    /// 0.005 mm.
    High,
    /// 0.001 mm.
    VeryHigh,
}

impl Accuracy {
    /// Every level, for the drop-down.
    pub const ALL: [Accuracy; 4] = [
        Accuracy::Low,
        Accuracy::Medium,
        Accuracy::High,
        Accuracy::VeryHigh,
    ];

    /// Name as shown.
    pub fn label(self) -> &'static str {
        match self {
            Accuracy::Low => "Low",
            Accuracy::Medium => "Medium",
            Accuracy::High => "High",
            Accuracy::VeryHigh => "Very High",
        }
    }

    /// Tessellation for this level.
    pub fn tessellation(self) -> TessellationOptions {
        let (chord_tolerance, max_segments) = match self {
            Accuracy::Low => (0.1, 256),
            Accuracy::Medium => (0.02, 512),
            Accuracy::High => (0.005, 1024),
            Accuracy::VeryHigh => (0.001, 2048),
        };
        TessellationOptions {
            chord_tolerance,
            max_segments,
        }
    }
}

/// Mass properties of one or more bodies.
#[derive(Clone, Debug, PartialEq)]
pub struct MassProps {
    /// Volume (mm³).
    pub volume: Real,
    /// Surface area (mm²).
    pub area: Real,
    /// Mass (g).
    pub mass: Real,
    /// Centre of gravity (mm).
    pub center: Point3,
    /// Inertia tensor about the centre of gravity, model axes (g·mm²):
    /// diagonal Ixx, Iyy, Izz; off-diagonal the products with the sign of
    /// the tensor (−∫xy dm).
    pub inertia: Matrix3<Real>,
}

impl MassProps {
    /// Principal moments (ascending, g·mm²) and their axes (unit vectors).
    pub fn principal(&self) -> ([Real; 3], [Vector3; 3]) {
        let e = SymmetricEigen::new(self.inertia);
        let mut idx = [0usize, 1, 2];
        idx.sort_by(|&a, &b| e.eigenvalues[a].total_cmp(&e.eigenvalues[b]));
        let vals = idx.map(|k| e.eigenvalues[k]);
        let axes = idx.map(|k| {
            let v: Vector3 = e.eigenvectors.column(k).into_owned();
            v.try_normalize(1e-300).unwrap_or_else(Vector3::x)
        });
        (vals, axes)
    }

    /// The bodies together (parallel-axis theorem for the inertia).
    pub fn combine<'a>(parts: impl IntoIterator<Item = &'a MassProps>) -> Option<MassProps> {
        let parts: Vec<&MassProps> = parts.into_iter().collect();
        if parts.is_empty() {
            return None;
        }
        let mass: Real = parts.iter().map(|p| p.mass).sum();
        let volume = parts.iter().map(|p| p.volume).sum();
        let area = parts.iter().map(|p| p.area).sum();
        // Weighted by mass; zero-mass sets fall back to volume weights.
        let (w, total): (Vec<Real>, Real) = if mass > 0.0 {
            (parts.iter().map(|p| p.mass).collect(), mass)
        } else {
            (parts.iter().map(|p| p.volume).collect(), volume)
        };
        let center = if total.abs() > 0.0 {
            Point3::from(
                parts
                    .iter()
                    .zip(&w)
                    .map(|(p, w)| p.center.coords * *w)
                    .sum::<Vector3>()
                    / total,
            )
        } else {
            parts[0].center
        };
        let mut inertia = Matrix3::zeros();
        for p in &parts {
            let d = p.center - center;
            inertia += p.inertia + (Matrix3::identity() * d.dot(&d) - d * d.transpose()) * p.mass;
        }
        Some(MassProps {
            volume,
            area,
            mass,
            center,
            inertia,
        })
    }
}

/// One solid body's properties.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyPhysical {
    /// Persistent body name (the creating feature's id).
    pub name: String,
    /// Display name (`Solid1`).
    pub label: String,
    /// Material name.
    pub material: String,
    /// Density (g/cm³).
    pub density: Real,
    /// Whether the material was found (else Generic's density was used).
    pub material_known: bool,
    /// The numbers.
    pub props: MassProps,
}

/// The part's properties.
#[derive(Clone, Debug, PartialEq)]
pub struct Physical {
    /// Every solid body.
    pub bodies: Vec<BodyPhysical>,
    /// All of them together.
    pub total: MassProps,
    /// Warnings (unknown materials).
    pub warnings: Vec<String>,
}

/// Volume, area, first and second moments of the closed triangle set
/// (unit density).
fn integrate(body: &BRep, sid: SolidId, opts: &TessellationOptions) -> Result<MassProps, String> {
    let mesh = body
        .tessellate_solid(sid, opts)
        .map_err(|e| e.to_string())?
        .faces;
    let (mut vol, mut area) = (0.0, 0.0);
    let mut first = Vector3::zeros();
    let mut second = Matrix3::<Real>::zeros();
    for t in &mesh.triangles {
        let [a, b, c] = t.map(|i| mesh.positions[i as usize].coords);
        area += (b - a).cross(&(c - a)).norm() / 2.0;
        let det = a.dot(&b.cross(&c));
        vol += det / 6.0;
        let s = a + b + c;
        first += s * (det / 24.0);
        second += (a * a.transpose() + b * b.transpose() + c * c.transpose() + s * s.transpose())
            * (det / 120.0);
    }
    if vol.abs() < 1e-12 {
        return Err("the body has no volume".into());
    }
    let center = Point3::from(first / vol);
    // ∫ (x−c)(x−c)ᵀ dV, then the inertia tensor tr(C)·E − C.
    let c = center.coords;
    let cov = second - c * c.transpose() * vol;
    let inertia = Matrix3::identity() * cov.trace() - cov;
    Ok(MassProps {
        volume: vol,
        area,
        mass: 0.0,
        center,
        inertia,
    })
}

/// Physical properties of every solid body of `part` (surface bodies have
/// no volume and are left out).
pub fn physical_properties(part: &PartDocument, accuracy: Accuracy) -> Result<Physical, String> {
    let opts = accuracy.tessellation();
    let r = Resolver::new(part);
    let labels = part.bodies_before(usize::MAX);
    let mut bodies = Vec::new();
    let mut warnings = Vec::new();
    for (sid, s) in part.body.solids() {
        if s.open {
            continue;
        }
        let name = s.name.clone().unwrap_or_default();
        let (mat, known) = r.body_material(s.name.as_deref());
        if !known {
            warnings.push(format!(
                "{name}: unknown material {}, Generic's density is used",
                mat.name
            ));
        }
        let mut props = integrate(&part.body, sid, &opts).map_err(|e| format!("{name}: {e}"))?;
        // g/cm³ → g/mm³.
        let rho = mat.density * 1e-3;
        props.mass = props.volume * rho;
        props.inertia *= rho;
        let label = labels
            .iter()
            .find(|(n, _)| *n == name)
            .map_or_else(|| format!("Solid{}", bodies.len() + 1), |(_, l)| l.clone());
        bodies.push(BodyPhysical {
            name,
            label,
            material: mat.name,
            density: mat.density,
            material_known: known,
            props,
        });
    }
    bodies.sort_by(|a, b| a.label.cmp(&b.label));
    let total =
        MassProps::combine(bodies.iter().map(|b| &b.props)).ok_or("the part has no solid body")?;
    Ok(Physical {
        bodies,
        total,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::materials;
    use crate::part::appearance::LookTarget;
    use crate::part::{
        ExtrudeFeature, Operation, PlaneRef, ProfileRef, ShellFeature, SketchOptions,
        SketchPlacement,
    };
    use approx::assert_relative_eq;

    fn plate(material: &str) -> PartDocument {
        let mut part = PartDocument::with_box("Box1", [100.0, 50.0, 10.0]).unwrap();
        part.set_material(Some(material), materials::builtin())
            .unwrap();
        part
    }

    /// Box1 plus a rectangle `lo..hi` (model x/y on the top face) extruded
    /// up by `h` with `op`.
    fn with_block(
        mut part: PartDocument,
        lo: [Real; 2],
        hi: [Real; 2],
        h: Real,
        op: Operation,
    ) -> PartDocument {
        let opts = SketchOptions {
            auto_reference_edges: false,
            auto_project_origin: false,
            ..Default::default()
        };
        let i = part
            .create_sketch(
                SketchPlacement {
                    plane: PlaneRef::Face("Box1:Top".into()),
                    ..Default::default()
                },
                &opts,
            )
            .unwrap();
        let plane = part.sketches[i].sketch.plane.clone();
        let z = part.body.bounding_box().unwrap().1.z;
        let a = plane.project(&Point3::new(lo[0], lo[1], z));
        let b = plane.project(&Point3::new(hi[0], hi[1], z));
        let sk = &mut part.sketches[i].sketch;
        sk.add_rectangle_two_point(a, b).unwrap();
        let r = sk.regions();
        assert_eq!(r.len(), 1);
        let mut e = ExtrudeFeature::new(vec![ProfileRef::of(&r[0], r[0].interior_point())], h);
        e.operation = op;
        part.add_extrude(i, e).unwrap();
        part
    }

    /// Steel 100×50×10: 50 cm³ · 7.85 g/cm³ = 392.5 g; centre in the
    /// middle; the box's analytic inertia m(b²+c²)/12…
    #[test]
    fn steel_plate_mass_and_inertia() {
        let part = plate("Steel S235");
        let p = physical_properties(&part, Accuracy::Low).unwrap();
        assert_eq!(p.bodies.len(), 1);
        let t = &p.total;
        assert_relative_eq!(t.volume, 50_000.0, max_relative = 1e-9);
        assert_relative_eq!(t.mass, 392.5, max_relative = 1e-9);
        assert_relative_eq!(t.area, 2.0 * (5000.0 + 1000.0 + 500.0), max_relative = 1e-9);
        assert_relative_eq!(t.center, Point3::new(50.0, 25.0, 5.0), epsilon = 1e-9);
        let m = 392.5;
        let ixx = m * (50.0f64.powi(2) + 10.0f64.powi(2)) / 12.0;
        let iyy = m * (100.0f64.powi(2) + 10.0f64.powi(2)) / 12.0;
        let izz = m * (100.0f64.powi(2) + 50.0f64.powi(2)) / 12.0;
        assert_relative_eq!(t.inertia[(0, 0)], ixx, max_relative = 1e-9);
        assert_relative_eq!(t.inertia[(1, 1)], iyy, max_relative = 1e-9);
        assert_relative_eq!(t.inertia[(2, 2)], izz, max_relative = 1e-9);
        assert!(t.inertia[(0, 1)].abs() < 1e-6 * izz);
        let (vals, axes) = t.principal();
        assert_relative_eq!(vals[0], ixx, max_relative = 1e-9);
        assert_relative_eq!(vals[2], izz, max_relative = 1e-9);
        assert!(axes[2].z.abs() > 1.0 - 1e-9);
        // The exact kernel agrees on volume, area and centre.
        let m = part.exact_bodies.0[0].measure_full().unwrap();
        assert_relative_eq!(m.volume, t.volume, max_relative = 1e-6);
        assert_relative_eq!(m.centroid, t.center, epsilon = 1e-6);
    }

    #[test]
    fn aluminium_and_generic() {
        let p = physical_properties(&plate("Aluminium 6061-T6"), Accuracy::Low).unwrap();
        assert_relative_eq!(p.total.mass, 135.0, max_relative = 1e-9);
        let g = PartDocument::with_box("Box1", [100.0, 50.0, 10.0]).unwrap();
        let p = physical_properties(&g, Accuracy::Low).unwrap();
        assert_eq!(p.bodies[0].material, "Generic");
        assert_relative_eq!(p.total.mass, 50.0, max_relative = 1e-9);
        assert!(p.warnings.is_empty());
    }

    /// A box shelled 2 mm with the top removed: outer minus the cavity
    /// 96×46×8 (closed bottom).
    #[test]
    fn shelled_box_mass() {
        let mut part = plate("Steel S235");
        let mut sh = ShellFeature::new(2.0);
        let top = part.body.find_face("Box1:Top").unwrap();
        let body = part.body.clone();
        sh.toggle(&body, top, None);
        part.add_shell(sh).unwrap();
        let p = physical_properties(&part, Accuracy::Low).unwrap();
        let v = 100.0 * 50.0 * 10.0 - 96.0 * 46.0 * 8.0;
        assert_relative_eq!(p.total.volume, v, max_relative = 1e-6);
        assert_relative_eq!(p.total.mass, v * 7.85e-3, max_relative = 1e-6);
        // The open top moves the centre down: below the box's mid-height.
        assert!(p.total.center.z < 5.0, "{:?}", p.total.center);
        assert_relative_eq!(p.total.center.x, 50.0, epsilon = 1e-6);
    }

    /// A closed cavity (no face removed) counts as a void too.
    #[test]
    fn closed_cavity_is_hollow() {
        let mut part = plate("Steel S235");
        part.add_shell(ShellFeature::new(2.0)).unwrap();
        let p = physical_properties(&part, Accuracy::Low).unwrap();
        let v = 100.0 * 50.0 * 10.0 - 96.0 * 46.0 * 6.0;
        assert_relative_eq!(p.total.volume, v, max_relative = 1e-6);
    }

    /// L-shape: 100×50×10 plate and a 10×50×40 wall joined on one end.
    /// Centre of gravity from the two boxes.
    #[test]
    fn l_shape_center_of_gravity() {
        let part = with_block(
            plate("Steel S235"),
            [0.0, 0.0],
            [10.0, 50.0],
            40.0,
            Operation::Join,
        );
        let p = physical_properties(&part, Accuracy::Low).unwrap();
        assert_eq!(p.bodies.len(), 1);
        let (v1, v2) = (50_000.0, 20_000.0);
        let x = (50.0 * v1 + 5.0 * v2) / (v1 + v2);
        let z = (5.0 * v1 + 30.0 * v2) / (v1 + v2);
        assert_relative_eq!(p.total.volume, v1 + v2, max_relative = 1e-9);
        assert_relative_eq!(p.total.center, Point3::new(x, 25.0, z), epsilon = 1e-6);
        // Iyy about the COG by the parallel-axis theorem on the two boxes.
        let rho = 7.85e-3;
        let (m1, m2) = (v1 * rho, v2 * rho);
        let iyy = m1 * (100.0f64.powi(2) + 10.0f64.powi(2)) / 12.0
            + m1 * ((50.0 - x).powi(2) + (5.0 - z).powi(2))
            + m2 * (10.0f64.powi(2) + 40.0f64.powi(2)) / 12.0
            + m2 * ((5.0 - x).powi(2) + (30.0 - z).powi(2));
        assert_relative_eq!(p.total.inertia[(1, 1)], iyy, max_relative = 1e-9);
        // The product Ixz is not zero for an L.
        assert!(p.total.inertia[(0, 2)].abs() > 1.0);
    }

    /// Two bodies with different materials: per-body masses and a total
    /// that adds up; the total centre is mass-weighted.
    #[test]
    fn several_bodies_with_their_materials() {
        let mut part = with_block(
            plate("Steel S235"),
            [20.0, 10.0],
            [40.0, 30.0],
            10.0,
            Operation::NewSolid,
        );
        let lib = materials::builtin();
        let names: Vec<String> = part
            .body
            .solids()
            .filter_map(|(_, s)| s.name.clone())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
        let other = names.iter().find(|n| *n != "Box1").unwrap().clone();
        part.set_body_material(&other, Some("Aluminium 6061-T6"), lib)
            .unwrap();
        // Appearance overrides do not change masses.
        part.set_appearance(&LookTarget::Body(other.clone()), Some("ABS - Red"), lib)
            .unwrap();
        let p = physical_properties(&part, Accuracy::Low).unwrap();
        assert_eq!(p.bodies.len(), 2);
        let steel = p.bodies.iter().find(|b| b.name == "Box1").unwrap();
        let alu = p.bodies.iter().find(|b| b.name == other).unwrap();
        assert_relative_eq!(steel.props.mass, 392.5, max_relative = 1e-9);
        assert_relative_eq!(alu.props.mass, 4000.0 * 2.7e-3, max_relative = 1e-9);
        assert_eq!(alu.material, "Aluminium 6061-T6");
        assert_relative_eq!(p.total.mass, 392.5 + 10.8, max_relative = 1e-9);
        let zc = (392.5 * 5.0 + 10.8 * 15.0) / (392.5 + 10.8);
        assert_relative_eq!(p.total.center.z, zc, epsilon = 1e-9);
    }

    /// An unknown material name (a file from elsewhere) weighs as Generic
    /// and says so.
    #[test]
    fn unknown_material_warns() {
        let mut part = plate("Steel S235");
        part.material = Some("Mithril".into());
        let p = physical_properties(&part, Accuracy::Low).unwrap();
        assert_eq!(p.warnings.len(), 1);
        assert_relative_eq!(p.total.mass, 50.0, max_relative = 1e-9);
    }
}
