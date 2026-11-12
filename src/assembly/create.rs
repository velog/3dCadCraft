//! Create In-Place Component (`docs/spec/3.10-top-down.md` §3.1): a new,
//! empty part, sheet metal part or subassembly made in the design file and
//! placed in the active assembly — on a picked face or plane with its XY
//! plane Flush to it, or at the assembly's origin (grounded when it is the
//! first component).

use crate::assembly::relation::{Constraint, ConstraintKind, GeomRef, OriginItem};
use crate::document::iprops::BomStructure;
use crate::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, DocumentKind, Project};
use crate::kernel::{Point3, Transform3, Vector3};
use crate::part::PartDocument;
use nalgebra::UnitQuaternion;
use std::path::{Path, PathBuf};

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

/// What [`create_as_file`] made.
#[derive(Clone, Debug)]
pub struct CreatedFile {
    /// The new document (its id in the new file) and its occurrence.
    pub created: Created,
    /// The new file.
    pub path: PathBuf,
    /// The assembly's link to it.
    pub library: crate::document::LibraryId,
}

/// The file Create stores a new component in: `<folder>/<name>.<kind ext>`
/// (`folder` `None` = the assembly's folder). Refused when it exists
/// (Inventor asks for another name).
pub fn new_file_path(
    owner_path: &Path,
    folder: Option<&Path>,
    name: &str,
    template: Template,
) -> Result<PathBuf, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("enter a name for the new component".into());
    }
    let dir = match folder {
        Some(f) => f.to_path_buf(),
        None => owner_path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .ok_or("the assembly has no folder")?,
    };
    if !dir.is_dir() {
        return Err(format!("no folder {}", dir.display()));
    }
    let kind = match template {
        Template::Assembly => DocumentKind::Assembly,
        Template::Part | Template::SheetMetal => DocumentKind::Part,
    };
    let path = crate::document::storage::file_name_for(&dir, name, kind);
    if path.exists() {
        return Err(format!(
            "{} already exists: enter another name",
            path.display()
        ));
    }
    Ok(path)
}

/// Create In-Place Component as Inventor makes it: the new part (or
/// subassembly) is a file of its own at `path` (from [`new_file_path`]),
/// linked by assembly `asm` of `project` saved at `owner_path`, and placed
/// as [`create_in_place`] places it. The new file is written; on an error
/// `project` is unchanged (rule 5).
pub fn create_as_file(
    project: &mut Project,
    asm: DocumentId,
    owner_path: &Path,
    path: &Path,
    o: &CreateOptions,
) -> Result<CreatedFile, String> {
    if !owner_path.is_file() {
        return Err(
            "save the assembly first: the new file is linked by a path relative to it".into(),
        );
    }
    if path.exists() {
        return Err(format!(
            "{} already exists: enter another name",
            path.display()
        ));
    }
    let mut trial = project.clone();
    let c = create_in_place(&mut trial, asm, o)?;
    // The document is named after its file.
    if let (Some(stem), Some(d)) = (
        path.file_stem().and_then(|s| s.to_str()),
        trial.document_mut(c.document),
    ) {
        d.name = stem.to_owned();
    }
    let library =
        crate::document::storage::extract(&mut trial, owner_path, c.document, path, false)
            .map_err(|e| e.to_string())?;
    *project = trial;
    Ok(CreatedFile {
        created: c,
        path: path.to_path_buf(),
        library,
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

    /// Create stores the new part as a file next to the saved assembly (the
    /// assembly links it); the part edited and saved in its own file is what
    /// the assembly finds after a reload, and Undo of the assembly (its old
    /// project) leaves the written file alone.
    #[test]
    fn create_writes_a_part_file_next_to_the_assembly() {
        let dir = std::env::temp_dir().join(format!("cc_create_file_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (mut p, aid) = project();
        let owner = dir.join("Asm.3dcadcraft");
        p.save(&owner).unwrap();
        let before = p.clone();
        let o = CreateOptions {
            name: "Bracket".into(),
            template: Template::Part,
            bom: BomStructure::Normal,
            on: None,
        };
        let path = new_file_path(&owner, None, &o.name, o.template).unwrap();
        assert_eq!(path, dir.join("Bracket.ccpart"));
        let made = create_as_file(&mut p, aid, &owner, &path, &o).unwrap();
        assert!(path.is_file());
        // The part left the design file; the occurrence links the new file.
        assert!(p.document(made.created.document).is_none());
        let occ = asm(&p, aid).find(made.created.occurrence).unwrap().clone();
        assert_eq!(occ.component.library, Some(made.library));
        assert_eq!(occ.component.document, made.created.document);
        let link = p.library(made.library).unwrap().clone();
        let found = crate::document::library::locate(&owner, &link, None).unwrap();
        assert_eq!(found.canonicalize().unwrap(), path.canonicalize().unwrap());
        // A second component of that name is refused before anything is made.
        assert!(new_file_path(&owner, None, "Bracket", Template::Part).is_err());
        let again = p.clone();
        assert!(create_as_file(&mut p, aid, &owner, &path, &o).is_err());
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            serde_json::to_string(&again).unwrap()
        );
        // Edited in its own file.
        let mut part_file = Project::load(&path).unwrap();
        assert_eq!(part_file.documents().len(), 1);
        assert_eq!(part_file.documents()[0].name, "Bracket");
        let edited = PartDocument::with_box("Box1", [5.0, 6.0, 7.0]).unwrap();
        part_file.document_mut(occ.component.document).unwrap().data = DocumentData::Part(edited);
        part_file.save(&path).unwrap();
        p.save(&owner).unwrap();
        // Reopened: the assembly links the edited part.
        let reopened = Project::load(&owner).unwrap();
        let link = reopened.library(made.library).unwrap().clone();
        let at = crate::document::library::locate(&owner, &link, None).unwrap();
        let file = Project::load(&at).unwrap();
        match file.document(occ.component.document).map(|d| &d.data) {
            Some(DocumentData::Part(x)) => assert_eq!(x.features.len(), 1),
            _ => panic!("the linked part is missing"),
        }
        // Undo = the project before: the file stays on disk.
        p = before;
        assert!(asm(&p, aid).find(made.created.occurrence).is_none());
        assert!(path.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_as_a_file_needs_a_saved_assembly() {
        let (mut p, aid) = project();
        let before = serde_json::to_string(&p).unwrap();
        let dir = std::env::temp_dir().join(format!("cc_create_unsaved_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let o = CreateOptions {
            name: "Part9".into(),
            template: Template::Part,
            bom: BomStructure::Normal,
            on: None,
        };
        let owner = dir.join("NotThere.3dcadcraft");
        let path = dir.join("Part9.ccpart");
        assert!(create_as_file(&mut p, aid, &owner, &path, &o).is_err());
        assert_eq!(serde_json::to_string(&p).unwrap(), before);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
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
