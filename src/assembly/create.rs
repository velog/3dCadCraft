//! Create In-Place Component (`docs/spec/3.10-top-down.md` §3.1): a new,
//! empty part, sheet metal part or subassembly made in the design file and
//! placed in the active assembly — on a picked face or plane with its XY
//! plane Flush to it, or at the assembly's origin (grounded when it is the
//! first component).

use crate::assembly::relation::{Constraint, ConstraintKind, GeomRef, OriginItem};
use crate::document::iprops::BomStructure;
use crate::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Project};
use crate::kernel::{Point3, Transform3, Vector3};
use crate::part::PartDocument;
use nalgebra::UnitQuaternion;

/// What the new component is (the Template list).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Template {
    /// A standard part.
    Part,
    /// A sheet metal part (the default sheet rule).
    SheetMetal,
    /// An assembly (a subassembly, spec 3.1a).
    Assembly,
}

/// The options of the panel.
#[derive(Clone, Debug)]
pub struct CreateOptions {
    /// New Component Name (made unique in the design file).
    pub name: String,
    /// What the new component is.
    pub template: Template,
    /// Default BOM Structure of the new document.
    pub bom: BomStructure,
    /// The picked face or plane (in the assembly's references) and its
    /// plane in assembly coordinates: the new component's XY plane is placed
    /// on it and kept Flush (`None`: at the origin, no relationship).
    pub on: Option<(GeomRef, Point3, Vector3)>,
}

/// What was made.
#[derive(Clone, Debug)]
pub struct Created {
    /// The new document.
    pub document: DocumentId,
    /// Its occurrence in the assembly.
    pub occurrence: u32,
    /// The Flush relationship, when one was made.
    pub constraint: Option<u32>,
}

/// The placement that puts a component's XY plane on the plane through `p`
/// with normal `n` (its origin at `p`, its +Z along `n`).
pub fn frame_on(p: Point3, n: Vector3) -> Transform3 {
    let n = n.try_normalize(1e-12).unwrap_or_else(Vector3::z);
    let turn = UnitQuaternion::rotation_between(&Vector3::z(), &n).unwrap_or_else(|| {
        // Exactly opposite: half a turn about X.
        UnitQuaternion::from_axis_angle(&Vector3::x_axis(), std::f64::consts::PI)
    });
    Transform3::from_parts(p.coords.into(), turn)
}

/// Creates the component in `project` and places it in assembly `asm`. On
/// an error nothing is changed (rule 5).
pub fn create_in_place(
    project: &mut Project,
    asm: DocumentId,
    o: &CreateOptions,
) -> Result<Created, String> {
    let name = o.name.trim();
    if name.is_empty() {
        return Err("enter a name for the new component".into());
    }
    let Some(DocumentData::Assembly(a)) = project.document(asm).map(|d| &d.data) else {
        return Err("no assembly is open".into());
    };
    let first = a.occurrences.is_empty();
    if first && o.on.is_some() {
        // Inventor: the first component cannot be constrained.
        return Err("the first component is placed at the origin: pick no plane".into());
    }
    let data = match o.template {
        Template::Part => DocumentData::Part(PartDocument::default()),
        Template::SheetMetal => DocumentData::Part(PartDocument {
            sheet_metal: Some(Default::default()),
            ..Default::default()
        }),
        Template::Assembly => DocumentData::Assembly(AssemblyDocument::default()),
    };
    // The name as typed when it is free, else the next `<base><n>`
    // (`Part1` taken: `Part2`).
    let name = if project.name_taken(name, None) {
        project.free_name(name.trim_end_matches(|c: char| c.is_ascii_digit()))
    } else {
        name.to_owned()
    };
    let id = project.add_document(name.clone(), data);
    if let Some(d) = project.document_mut(id) {
        d.properties.bom_structure = o.bom;
    }
    let place =
        o.on.as_ref()
            .map_or(Transform3::identity(), |(_, p, n)| frame_on(*p, *n));
    let Some(DocumentData::Assembly(a)) = project.document_mut(asm).map(|d| &mut d.data) else {
        return Err("no assembly is open".into());
    };
    let occurrence = a.place(DocRef::local(id), &name, place, first);
    let constraint = o.on.as_ref().map(|(g, _, _)| {
        let xy = GeomRef::Origin {
            occurrence,
            sub: Vec::new(),
            item: OriginItem::XyPlane,
        };
        a.add_constraint(Constraint::new(ConstraintKind::Flush, xy, g.clone()))
    });
    Ok(Created {
        document: id,
        occurrence,
        constraint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (Project, DocumentId) {
        let mut p = Project::new("T");
        let plate = PartDocument::with_box("Box1", [40.0, 30.0, 10.0]).unwrap();
        let plate = p.add_document("Plate", DocumentData::Part(plate));
        let mut a = AssemblyDocument::default();
        a.place(DocRef::local(plate), "Plate", Transform3::identity(), true);
        let aid = p.add_document("Asm", DocumentData::Assembly(a));
        (p, aid)
    }

    fn asm(p: &Project, id: DocumentId) -> &AssemblyDocument {
        match p.document(id).map(|d| &d.data) {
            Some(DocumentData::Assembly(a)) => a,
            _ => panic!("no assembly"),
        }
    }

    #[test]
    fn a_part_on_a_face_is_placed_on_it_and_kept_flush() {
        let (mut p, aid) = project();
        let top = GeomRef::Face {
            occurrence: 1,
            sub: Vec::new(),
            name: "Box1:Top".into(),
            hint: [20.0, 15.0, 10.0],
        };
        let o = CreateOptions {
            name: "Plate".into(),
            template: Template::Part,
            bom: BomStructure::Purchased,
            on: Some((top, Point3::new(20.0, 15.0, 10.0), Vector3::z())),
        };
        let c = create_in_place(&mut p, aid, &o).unwrap();
        // The name was taken: a free one.
        let doc = p.document(c.document).unwrap();
        assert_eq!(doc.name, "Plate1");
        assert_eq!(doc.properties.bom_structure, BomStructure::Purchased);
        let a = asm(&p, aid);
        let occ = a.find(c.occurrence).unwrap();
        assert!(!occ.grounded);
        assert!((occ.transform.translation.vector - Vector3::new(20.0, 15.0, 10.0)).norm() < 1e-12);
        let k = a.constraint(c.constraint.unwrap()).unwrap();
        assert_eq!(k.kind, ConstraintKind::Flush);
        // The placement already satisfies it.
        let solved =
            crate::assembly::solve(a, &|r| crate::assembly::relation::resolve(&p, a, r), None);
        assert!(solved.converged && solved.sick.is_empty(), "{solved:?}");
        assert!(solved.residual < 1e-9);
    }

    #[test]
    fn the_first_component_is_grounded_at_the_origin() {
        let mut p = Project::new("T");
        let aid = p.add_document("Asm", DocumentData::Assembly(AssemblyDocument::default()));
        let mut o = CreateOptions {
            name: "Part1".into(),
            template: Template::Assembly,
            bom: BomStructure::Normal,
            on: None,
        };
        let c = create_in_place(&mut p, aid, &o).unwrap();
        assert_eq!(p.document(c.document).unwrap().name, "Part1");
        assert!(asm(&p, aid).find(c.occurrence).unwrap().grounded);
        assert!(matches!(
            p.document(c.document).map(|d| &d.data),
            Some(DocumentData::Assembly(_))
        ));
        // A sheet metal part carries the sheet rule.
        o.template = Template::SheetMetal;
        let c = create_in_place(&mut p, aid, &o).unwrap();
        assert_eq!(p.document(c.document).unwrap().name, "Part2");
        assert!(matches!(
            p.document(c.document).map(|d| &d.data),
            Some(DocumentData::Part(x)) if x.sheet_metal.is_some()
        ));
    }

    #[test]
    fn errors_change_nothing() {
        let (mut p, aid) = project();
        let before = serde_json::to_string(&p).unwrap();
        let o = CreateOptions {
            name: "  ".into(),
            template: Template::Part,
            bom: BomStructure::Normal,
            on: None,
        };
        assert!(create_in_place(&mut p, aid, &o).is_err());
        assert_eq!(serde_json::to_string(&p).unwrap(), before);
    }

    #[test]
    fn frames_turn_z_onto_the_normal() {
        for n in [
            Vector3::z(),
            -Vector3::z(),
            Vector3::x(),
            Vector3::new(1.0, 2.0, -3.0),
        ] {
            let t = frame_on(Point3::new(1.0, 2.0, 3.0), n);
            assert!((t * Vector3::z() - n.normalize()).norm() < 1e-12, "{n}");
            assert!((t * Point3::origin() - Point3::new(1.0, 2.0, 3.0)).norm() < 1e-12);
        }
    }
}
