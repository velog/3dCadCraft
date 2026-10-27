//! Physical Properties of an assembly (`docs/spec/3.0-assembly-commands.md`
//! §3.4, `docs/spec/0.10-physical.md`): each component's part properties
//! (computed once per part, in its own coordinates — rule 8) moved by the
//! occurrence's placement and summed with the parallel-axis theorem.
//! Suppressed components are left out (Inventor).

use std::collections::HashMap;

use crate::document::{AssemblyDocument, DocRef};
use crate::part::physical::{BodyPhysical, MassProps, Physical};

/// The assembly's properties as a [`Physical`] whose "bodies" are the
/// occurrences (name and label = the occurrence's name, material = its
/// part's, or "Mixed"). `part` computes a component's own properties.
pub fn assembly_physical(
    asm: &AssemblyDocument,
    part: &mut dyn FnMut(DocRef) -> Option<Result<Physical, String>>,
) -> Result<Physical, String> {
    let mut cache: HashMap<DocRef, Option<Result<Physical, String>>> = HashMap::new();
    let mut bodies = Vec::new();
    let mut warnings = Vec::new();
    for o in asm.occurrences.iter().filter(|o| !o.suppressed) {
        let r = cache
            .entry(o.component)
            .or_insert_with(|| part(o.component));
        let p = match r {
            Some(Ok(p)) => p,
            Some(Err(e)) => {
                warnings.push(format!("{}: {e}", o.name));
                continue;
            }
            None => {
                warnings.push(format!("{}: the component cannot be read", o.name));
                continue;
            }
        };
        let Some(own) = MassProps::combine(p.bodies.iter().map(|b| &b.props)) else {
            continue;
        };
        let mut mats: Vec<&str> = p.bodies.iter().map(|b| b.material.as_str()).collect();
        mats.dedup();
        let material = match mats.as_slice() {
            [one] => (*one).to_owned(),
            _ => "Mixed".to_owned(),
        };
        // Density as the part's mass over its volume (g/cm³).
        let density = if own.volume > 0.0 {
            own.mass / own.volume * 1e3
        } else {
            0.0
        };
        warnings.extend(
            p.warnings
                .iter()
                .filter(|w| !warnings.iter().any(|x| x.ends_with(w.as_str())))
                .map(|w| format!("{}: {w}", o.name))
                .collect::<Vec<_>>(),
        );
        bodies.push(BodyPhysical {
            name: o.name.clone(),
            label: o.name.clone(),
            material,
            density,
            material_known: p.bodies.iter().all(|b| b.material_known),
            props: own.moved(&o.transform),
        });
    }
    let total =
        MassProps::combine(bodies.iter().map(|b| &b.props)).ok_or("no component with a solid")?;
    Ok(Physical {
        bodies,
        total,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::DocumentId;
    use crate::kernel::{Point3, Transform3, Vector3};
    use crate::part::PartDocument;
    use crate::part::physical::{Accuracy, physical_properties};

    /// Two 20 × 10 × 5 blocks, one turned a quarter about Z and moved: the
    /// mass doubles, the centre lies between theirs, and the inertia is
    /// that of the two boxes about it.
    #[test]
    fn two_blocks_one_turned() {
        let block = PartDocument::with_box("Box1", [20.0, 10.0, 5.0]).unwrap();
        let one = physical_properties(&block, Accuracy::Medium).unwrap();
        let mut a = AssemblyDocument::default();
        let r = DocRef::local(DocumentId::new_random());
        a.place(r, "Block", Transform3::identity(), true);
        let turn = Transform3::translation(100.0, 0.0, 0.0)
            * Transform3::rotation(Vector3::z() * std::f64::consts::FRAC_PI_2);
        a.place(r, "Block", turn, false);
        let mut f = |_r: DocRef| Some(Ok(one.clone()));
        let p = assembly_physical(&a, &mut f).unwrap();
        assert_eq!(p.bodies.len(), 2);
        let m = one.total.mass;
        assert!((p.total.mass - 2.0 * m).abs() < 1e-9 * m);
        // Centres (10, 5, 2.5) and (100 − 5, 10, 2.5).
        let c = p.total.center;
        assert!((c - Point3::new(52.5, 7.5, 2.5)).norm() < 1e-9, "{c}");
        // The turned one's Ixx is the first one's Iyy.
        let i0 = one.total.inertia;
        let i1 = p.bodies[1].props.inertia;
        assert!((i1[(0, 0)] - i0[(1, 1)]).abs() < 1e-6 * i0[(1, 1)]);
        // Izz about the common centre: both own Izz + m·d² each (d = 42.5,
        // 2.5 in x, y).
        let d2 = 42.5_f64.powi(2) + 2.5_f64.powi(2);
        let want = 2.0 * i0[(2, 2)] + 2.0 * m * d2;
        assert!((p.total.inertia[(2, 2)] - want).abs() < 1e-6 * want);
        // A suppressed one is left out.
        a.occurrences[1].suppressed = true;
        let p = assembly_physical(&a, &mut f).unwrap();
        assert!((p.total.mass - m).abs() < 1e-9 * m);
    }
}
