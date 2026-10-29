//! Copy Components and Mirror Components (`docs/spec/3.0-assembly-commands.md`
//! §3.1): new part documents in the design file (Copy: a plain copy, Mirror:
//! a copy whose bodies are mirrored about its YZ plane) or the same parts
//! reused, placed as new occurrences, optionally with the relationships
//! among the chosen components copied onto the new ones.
//!
//! A mirrored occurrence is placed by `M · T · D`: `M` the reflection about
//! the assembly plane, `T` the source's placement, `D` the reflection about
//! the part's YZ plane (what the mirrored part already did). Two
//! reflections make a rigid motion, so placements stay `Transform3`.

use std::collections::HashMap;

use crate::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Project};
use crate::kernel::{Point3, Real, Transform3, Vector3};
use crate::part::{PatternFeature, PatternKind, PlaneRef};

/// What becomes of the chosen components.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyStatus {
    /// A new part document (Copy: a copy, Mirror: mirrored).
    New,
    /// The same part placed again (Mirror: turned, right only for parts
    /// symmetric about their YZ plane).
    Reuse,
}

/// The options both commands share.
#[derive(Clone, Debug)]
pub struct CopyOptions {
    /// New parts or the same ones reused.
    pub status: CopyStatus,
    /// New document names: `prefix + name + suffix` (made unique).
    pub prefix: String,
    /// See `prefix`.
    pub suffix: String,
    /// Copy / mirror the relationships between the chosen components.
    pub relationships: bool,
    /// Ground the new occurrences.
    pub ground: bool,
}

/// A mirror plane in assembly coordinates: a point and its normal.
#[derive(Clone, Copy, Debug)]
pub struct MirrorPlane {
    /// A point on the plane.
    pub point: Point3,
    /// Its normal (any length but zero).
    pub normal: Vector3,
}

/// What Copy / Mirror made: new occurrence per source, new documents.
#[derive(Clone, Debug, Default)]
pub struct CopyResult {
    /// (source, new occurrence).
    pub occurrences: Vec<(u32, u32)>,
    /// The part documents made.
    pub documents: Vec<DocumentId>,
}

/// Copies occurrences `ids` of assembly `asm` in `project`.
pub fn copy_components(
    project: &mut Project,
    asm: DocumentId,
    ids: &[u32],
    o: &CopyOptions,
) -> Result<CopyResult, String> {
    run(project, asm, ids, o, None)
}

/// Mirrors occurrences `ids` of assembly `asm` about `plane`.
pub fn mirror_components(
    project: &mut Project,
    asm: DocumentId,
    ids: &[u32],
    plane: MirrorPlane,
    o: &CopyOptions,
) -> Result<CopyResult, String> {
    run(project, asm, ids, o, Some(plane))
}

/// Both commands; nothing changes on an error (rule 5): the work is done on
/// a copy of the project.
fn run(
    project: &mut Project,
    asm: DocumentId,
    ids: &[u32],
    o: &CopyOptions,
    mirror: Option<MirrorPlane>,
) -> Result<CopyResult, String> {
    if ids.is_empty() {
        return Err("select the components".into());
    }
    let mut p = project.clone();
    let a = assembly(&p, asm)?.clone();
    let sources: Vec<_> = ids
        .iter()
        .map(|&i| a.find(i).cloned().ok_or(format!("no component {i}")))
        .collect::<Result<_, _>>()?;
    let reflect = match mirror {
        Some(m) => Some(reflection(m)?),
        None => None,
    };
    // One new document per component (several occurrences share it).
    let mut docs: HashMap<DocRef, (DocRef, String)> = HashMap::new();
    let mut out = CopyResult::default();
    for s in &sources {
        if docs.contains_key(&s.component) {
            continue;
        }
        let name = p
            .document(s.component.document)
            .filter(|_| s.component.library.is_none())
            .map(|d| d.name.clone())
            .ok_or_else(|| {
                format!(
                    "{}: only parts stored in this design file can be copied yet (Reuse places linked ones)",
                    s.name
                )
            })?;
        let target = match o.status {
            CopyStatus::Reuse => (s.component, name),
            CopyStatus::New => {
                let Some(DocumentData::Part(part)) =
                    p.document(s.component.document).map(|d| d.data.clone())
                else {
                    return Err(format!("{}: not a part", s.name));
                };
                let mut part = part;
                if mirror.is_some() {
                    mirror_part(&mut part).map_err(|e| format!("{}: {e}", s.name))?;
                }
                let base = format!("{}{}{}", o.prefix, name, o.suffix);
                let new_name = if p.name_taken(&base, None) {
                    p.free_name(&format!("{base}_"))
                } else {
                    base
                };
                let id = p.add_document(new_name.clone(), DocumentData::Part(part));
                out.documents.push(id);
                (DocRef::local(id), new_name)
            }
        };
        docs.insert(s.component, target);
    }
    let a = assembly_mut(&mut p, asm)?;
    let mut map: HashMap<u32, u32> = HashMap::new();
    for s in &sources {
        let (r, base) = docs[&s.component].clone();
        let t = match reflect {
            Some((sm, shift)) => mirrored_place(&s.transform, &sm, &shift),
            None => s.transform,
        };
        let id = a.place(r, &base, t, o.ground);
        if let Some(n) = a.find_mut(id) {
            n.looks.appearance = s.looks.appearance.clone();
            if o.status == CopyStatus::Reuse || mirror.is_none() {
                // Same features and faces: their overrides still apply.
                n.looks.features = s.looks.features.clone();
                n.looks.faces = s.looks.faces.clone();
            }
        }
        map.insert(s.id, id);
        out.occurrences.push((s.id, id));
    }
    if o.relationships {
        let copies: Vec<_> = a
            .constraints
            .iter()
            .filter(|c| map.contains_key(&c.a.occurrence()) && map.contains_key(&c.b.occurrence()))
            .cloned()
            .collect();
        for mut c in copies {
            for g in [&mut c.a, &mut c.b] {
                let o = g.occurrence_mut();
                *o = map[o];
            }
            if let Some(g) = c.plane.as_mut()
                && let Some(&n) = map.get(&g.occurrence())
            {
                *g.occurrence_mut() = n;
            }
            a.add_constraint(c);
        }
    }
    *project = p;
    Ok(out)
}

fn assembly(p: &Project, id: DocumentId) -> Result<&AssemblyDocument, String> {
    match p.document(id).map(|d| &d.data) {
        Some(DocumentData::Assembly(a)) => Ok(a),
        _ => Err("not an assembly".into()),
    }
}

fn assembly_mut(p: &mut Project, id: DocumentId) -> Result<&mut AssemblyDocument, String> {
    match p.document_mut(id).map(|d| &mut d.data) {
        Some(DocumentData::Assembly(a)) => Ok(a),
        _ => Err("not an assembly".into()),
    }
}

/// The part mirrored about its own YZ plane: a solid Mirror with Remove
/// Original after its features (the bodies keep their names).
pub fn mirror_part(part: &mut crate::part::PartDocument) -> Result<(), String> {
    let mut m = PatternFeature::mirror();
    m.kind = PatternKind::Mirror {
        plane: Some(PlaneRef::Origin(crate::part::OriginPlane::YZ)),
    };
    m.solid_mode = true;
    m.remove_original = true;
    part.add_pattern(m).map(|_| ()).map_err(|e| e.to_string())
}

/// The reflection about `m` as (S = I − 2nnᵀ, shift 2(p·n)n).
fn reflection(m: MirrorPlane) -> Result<(nalgebra::Matrix3<Real>, Vector3), String> {
    let n = m
        .normal
        .try_normalize(1e-12)
        .ok_or("the mirror plane has no direction")?;
    let s = nalgebra::Matrix3::identity() - 2.0 * n * n.transpose();
    Ok((s, n * (2.0 * m.point.coords.dot(&n))))
}

/// `M · T · D` (see the module text) as a rigid motion.
fn mirrored_place(t: &Transform3, s: &nalgebra::Matrix3<Real>, shift: &Vector3) -> Transform3 {
    let d = nalgebra::Matrix3::from_diagonal(&Vector3::new(-1.0, 1.0, 1.0));
    let r = s * t.rotation.to_rotation_matrix().into_inner() * d;
    let rot = nalgebra::UnitQuaternion::from_rotation_matrix(
        &nalgebra::Rotation3::from_matrix_unchecked(r),
    );
    let tr = s * t.translation.vector + shift;
    Transform3::from_parts(nalgebra::Translation3::from(tr), rot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::PartDocument;

    /// A design file with a 10 × 20 × 30 box part placed twice (off the
    /// origin, turned), the second constrained to the first.
    fn project() -> (Project, DocumentId, DocumentId) {
        let mut p = Project::new("T");
        let part = PartDocument::with_box("Box1", [10.0, 20.0, 30.0]).unwrap();
        let pid = p.add_document("Block", DocumentData::Part(part));
        let mut a = AssemblyDocument::default();
        a.place(DocRef::local(pid), "Block", Transform3::identity(), true);
        let t = Transform3::new(Vector3::new(40.0, 5.0, 0.0), Vector3::new(0.0, 0.0, 0.4));
        a.place(DocRef::local(pid), "Block", t, false);
        let aid = p.add_document("Assembly", DocumentData::Assembly(a));
        (p, pid, aid)
    }

    fn opts(status: CopyStatus) -> CopyOptions {
        CopyOptions {
            status,
            prefix: String::new(),
            suffix: "_CPY".into(),
            relationships: true,
            ground: false,
        }
    }

    #[test]
    fn copy_makes_one_new_part_for_both_occurrences() {
        let (mut p, _, aid) = project();
        let r = copy_components(&mut p, aid, &[1, 2], &opts(CopyStatus::New)).unwrap();
        assert_eq!(r.documents.len(), 1);
        assert_eq!(p.document(r.documents[0]).unwrap().name, "Block_CPY");
        let a = assembly(&p, aid).unwrap();
        assert_eq!(a.occurrences.len(), 4);
        let (n1, n2) = (a.find(3).unwrap(), a.find(4).unwrap());
        assert_eq!(n1.component, n2.component);
        assert_eq!(n1.name, "Block_CPY:1");
        assert_eq!(n2.transform, a.find(2).unwrap().transform);
        // A second copy gets its own name.
        let r2 = copy_components(&mut p, aid, &[1], &opts(CopyStatus::New)).unwrap();
        assert_eq!(p.document(r2.documents[0]).unwrap().name, "Block_CPY_1");
    }

    #[test]
    fn reuse_places_the_same_part() {
        let (mut p, pid, aid) = project();
        let r = copy_components(&mut p, aid, &[2], &opts(CopyStatus::Reuse)).unwrap();
        assert!(r.documents.is_empty());
        let a = assembly(&p, aid).unwrap();
        assert_eq!(a.find(3).unwrap().component, DocRef::local(pid));
        assert_eq!(a.find(3).unwrap().name, "Block:3");
    }

    #[test]
    fn relationships_among_the_chosen_ones_are_copied() {
        use crate::assembly::relation::{ConstraintKind, GeomRef};
        let (mut p, _, aid) = project();
        let face = |o, n: &str| GeomRef::Face {
            sub: Vec::new(),
            occurrence: o,
            name: format!("Box1:{n}"),
            hint: [0.0; 3],
        };
        let c = crate::assembly::relation::Constraint::new(
            ConstraintKind::Mate,
            face(1, "Top"),
            face(2, "Bottom"),
        );
        assembly_mut(&mut p, aid).unwrap().add_constraint(c);
        copy_components(&mut p, aid, &[1, 2], &opts(CopyStatus::New)).unwrap();
        let a = assembly(&p, aid).unwrap();
        assert_eq!(a.constraints.len(), 2);
        let c2 = &a.constraints[1];
        assert_eq!((c2.a.occurrence(), c2.b.occurrence()), (3, 4));
        // Only one of the pair: nothing to copy.
        copy_components(&mut p, aid, &[2], &opts(CopyStatus::New)).unwrap();
        assert_eq!(assembly(&p, aid).unwrap().constraints.len(), 2);
    }

    #[test]
    fn mirror_places_the_mirrored_part_where_the_reflection_is() {
        let (mut p, _, aid) = project();
        // Mirror about the plane x = 100.
        let plane = MirrorPlane {
            point: Point3::new(100.0, 0.0, 0.0),
            normal: Vector3::x(),
        };
        let mut o = opts(CopyStatus::New);
        o.suffix = "_MIR".into();
        let r = mirror_components(&mut p, aid, &[2], plane, &o).unwrap();
        let mir = match &p.document(r.documents[0]).unwrap().data {
            DocumentData::Part(m) => m.clone(),
            _ => panic!("not a part"),
        };
        assert!(mir.feature_health.is_empty(), "{:?}", mir.feature_health);
        let a = assembly(&p, aid).unwrap();
        let (src, new) = (a.find(2).unwrap(), a.find(3).unwrap());
        assert_eq!(new.name, "Block_MIR:1");
        // Every corner of the source box, reflected, is a corner of the new
        // occurrence's mirrored box (in its part: x → −x).
        let size = [10.0, 20.0, 30.0];
        for k in 0..8 {
            let c = Point3::new(
                if k & 1 == 0 { 0.0 } else { size[0] },
                if k & 2 == 0 { 0.0 } else { size[1] },
                if k & 4 == 0 { 0.0 } else { size[2] },
            );
            let w = src.transform * c;
            let reflected = Point3::new(200.0 - w.x, w.y, w.z);
            let local_mirrored = Point3::new(-c.x, c.y, c.z);
            let got = new.transform * local_mirrored;
            assert!((got - reflected).norm() < 1e-9, "{k}: {got} vs {reflected}");
        }
        // The placement is rigid (a rotation, not a reflection).
        let m = new.transform.rotation.to_rotation_matrix().into_inner();
        assert!((m.determinant() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn errors_change_nothing() {
        let (mut p, _, aid) = project();
        let before = serde_json::to_string(&p).unwrap();
        assert!(copy_components(&mut p, aid, &[], &opts(CopyStatus::New)).is_err());
        assert!(copy_components(&mut p, aid, &[9], &opts(CopyStatus::New)).is_err());
        let flat = MirrorPlane {
            point: Point3::origin(),
            normal: Vector3::zeros(),
        };
        assert!(mirror_components(&mut p, aid, &[1], flat, &opts(CopyStatus::New)).is_err());
        assert_eq!(serde_json::to_string(&p).unwrap(), before);
    }
}
