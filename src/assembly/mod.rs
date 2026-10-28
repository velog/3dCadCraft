//! Assemblies, step A1 (`docs/spec/3.1-assemblies.md` §3.1): placing
//! occurrences of parts, Ground, Free Move / Free Rotate, Visibility and
//! Delete, and the occurrences' geometry in assembly coordinates (box,
//! picking). GUI-free; the environment itself is `gui/assembly.rs`.
//!
//! A part is stored once and shared by all its occurrences (rule 8):
//! nothing here writes into a part. An occurrence's placement is its
//! `transform` (part-local → assembly coordinates).

use std::sync::Arc;

pub mod bom;
pub mod interference;
pub mod joint;
pub mod pattern;
pub mod physical;
pub mod relation;
pub mod solve;

pub use relation::{Constraint, ConstraintKind, Geom, GeomRef};
pub use solve::{Drag, Solved, solve};

use crate::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Occurrence, Project};
use crate::kernel::{FaceId, Point3, Real, TessellationOptions, Transform3, Vector3};
use crate::part::PartDocument;

impl AssemblyDocument {
    /// Gives every occurrence without an id (files before ids) or with a
    /// repeated one a new id, in list order.
    pub fn fix_ids(&mut self) {
        let mut next = self.next_id();
        let mut seen = std::collections::HashSet::new();
        for o in &mut self.occurrences {
            if o.id == 0 || !seen.insert(o.id) {
                o.id = next;
                seen.insert(next);
                next += 1;
            }
        }
    }

    /// The id the next occurrence gets (ids are never reused while the
    /// occurrence with the highest one exists).
    pub fn next_id(&self) -> u32 {
        self.occurrences.iter().map(|o| o.id).max().unwrap_or(0) + 1
    }

    /// The occurrence with id `id`.
    pub fn find(&self, id: u32) -> Option<&Occurrence> {
        self.occurrences.iter().find(|o| o.id == id)
    }

    /// The occurrence with id `id`, to change.
    pub fn find_mut(&mut self, id: u32) -> Option<&mut Occurrence> {
        self.occurrences.iter_mut().find(|o| o.id == id)
    }

    /// The next free name `base:k` (Inventor numbers the occurrences of a
    /// document: `Bracket:1`, `Bracket:2`; a deleted number is not
    /// filled in again while a higher one exists).
    pub fn next_name(&self, base: &str) -> String {
        let k = self
            .occurrences
            .iter()
            .filter_map(|o| {
                o.name
                    .strip_prefix(base)?
                    .strip_prefix(':')?
                    .parse::<u32>()
                    .ok()
            })
            .max()
            .unwrap_or(0);
        format!("{base}:{}", k + 1)
    }

    /// Places `component` as a new occurrence named after `base`; returns
    /// its id.
    pub fn place(
        &mut self,
        component: DocRef,
        base: &str,
        transform: Transform3,
        grounded: bool,
    ) -> u32 {
        let id = self.next_id();
        let mut o = Occurrence::new(self.next_name(base), component, transform);
        o.id = id;
        o.grounded = grounded;
        self.occurrences.push(o);
        id
    }

    /// Removes the occurrence `id` (Delete) and the constraints that use
    /// it (a pattern's source takes its pattern and copies along); `None`
    /// if there is none.
    pub fn remove(&mut self, id: u32) -> Option<Occurrence> {
        self.find(id)?;
        self.forget_in_patterns(id);
        let i = self.occurrences.iter().position(|o| o.id == id)?;
        self.constraints.retain(|c| !c.uses(id));
        Some(self.occurrences.remove(i))
    }
}

/// The part a reference shows: a part document of `project`. Library
/// documents and sub-assemblies are not shown yet (`None`).
pub fn part_of(project: &Project, r: DocRef) -> Option<&PartDocument> {
    if r.library.is_some() {
        return None;
    }
    match project.document(r.document).map(|d| &d.data) {
        Some(DocumentData::Part(p)) => Some(p),
        _ => None,
    }
}

/// The part documents of `project` that can be placed: (id, name).
pub fn placeable(project: &Project) -> Vec<(DocumentId, String)> {
    project
        .documents()
        .iter()
        .filter(|d| matches!(d.data, DocumentData::Part(_)))
        .map(|d| (d.id, d.name.clone()))
        .collect()
}

/// Moves `o` by `v` (assembly coordinates; Free Move).
pub fn move_by(o: &mut Occurrence, v: Vector3) {
    o.transform = Transform3::translation(v.x, v.y, v.z) * o.transform;
}

/// Turns `o` by `angle` (radians, right hand) about the line through
/// `center` along `axis` (Free Rotate).
pub fn rotate_about(o: &mut Occurrence, center: Point3, axis: Vector3, angle: Real) {
    let Some(axis) = nalgebra::Unit::try_new(axis, 1e-12) else {
        return;
    };
    let r = Transform3::rotation(axis.into_inner() * angle);
    let c = center.coords;
    o.transform = Transform3::translation(c.x, c.y, c.z)
        * r
        * Transform3::translation(-c.x, -c.y, -c.z)
        * o.transform;
}

/// The triangles of a part in its own coordinates, for picking its
/// occurrences (built once per part body; the GUI caches it by
/// [`PartDocument::body_signature`]).
#[derive(Clone, Debug, Default)]
pub struct PickMesh {
    tris: Vec<(FaceId, [Point3; 3])>,
    /// The box of the triangles (part coordinates).
    pub bbox: Option<(Point3, Point3)>,
}

impl PickMesh {
    /// The visible bodies' faces of `part`.
    pub fn of(part: &PartDocument) -> Self {
        let body = &part.body;
        let opts = TessellationOptions::default();
        let mut tris = Vec::new();
        let mut bbox: Option<(Point3, Point3)> = None;
        for (sid, solid) in body.solids() {
            if solid.name.as_deref().is_some_and(|n| !part.body_visible(n)) {
                continue;
            }
            let Ok(m) = body.tessellate_solid(sid, &opts) else {
                continue;
            };
            for (fid, range) in &m.face_triangles {
                for t in &m.faces.triangles[range.clone()] {
                    let p = t.map(|i| m.faces.positions[i as usize]);
                    for q in &p {
                        bbox = Some(bbox.map_or((*q, *q), |(lo, hi)| (lo.inf(q), hi.sup(q))));
                    }
                    tris.push((*fid, p));
                }
            }
        }
        Self { tris, bbox }
    }

    /// Every triangle (face, corners) in part coordinates.
    pub fn triangles(&self) -> &[(FaceId, [Point3; 3])] {
        &self.tris
    }

    /// The nearest face the ray `o + t·d` (t > 0, part coordinates) hits.
    pub fn hit(&self, o: &Point3, d: &Vector3) -> Option<(FaceId, Real)> {
        let mut best: Option<(FaceId, Real)> = None;
        for (f, [a, b, c]) in &self.tris {
            // Möller–Trumbore.
            let (e1, e2) = (b - a, c - a);
            let p = d.cross(&e2);
            let det = e1.dot(&p);
            if det.abs() < 1e-14 {
                continue;
            }
            let inv = 1.0 / det;
            let s = o - a;
            let u = s.dot(&p) * inv;
            if !(-1e-9..=1.0 + 1e-9).contains(&u) {
                continue;
            }
            let q = s.cross(&e1);
            let v = d.dot(&q) * inv;
            if v < -1e-9 || u + v > 1.0 + 1e-9 {
                continue;
            }
            let t = e2.dot(&q) * inv;
            if t > 0.0 && best.is_none_or(|b| t < b.1) {
                best = Some((*f, t));
            }
        }
        best
    }
}

/// What a ray hits in an assembly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// The occurrence's id.
    pub occurrence: u32,
    /// The face of its part.
    pub face: FaceId,
    /// Ray parameter (assembly units: the transforms are rigid).
    pub t: Real,
}

/// The nearest visible occurrence along `origin + t·dir` (assembly
/// coordinates); `mesh` gives each component's picking triangles. The ray
/// is taken into each occurrence's own frame (rule 8: parts are picked in
/// their local coordinates).
pub fn pick(
    asm: &AssemblyDocument,
    origin: &Point3,
    dir: &Vector3,
    mesh: &dyn Fn(DocRef) -> Option<Arc<PickMesh>>,
) -> Option<Hit> {
    pick_among(asm, origin, dir, mesh, &|_| true)
}

/// [`pick`] among the pickable occurrences that `keep` accepts (Isolate).
pub fn pick_among(
    asm: &AssemblyDocument,
    origin: &Point3,
    dir: &Vector3,
    mesh: &dyn Fn(DocRef) -> Option<Arc<PickMesh>>,
    keep: &dyn Fn(&Occurrence) -> bool,
) -> Option<Hit> {
    let mut best: Option<Hit> = None;
    for o in asm.occurrences.iter().filter(|o| o.pickable() && keep(o)) {
        let Some(m) = mesh(o.component) else {
            continue;
        };
        let inv = o.transform.inverse();
        let (lo, ld) = (inv * origin, inv * dir);
        if let Some((face, t)) = m.hit(&lo, &ld)
            && best.is_none_or(|b| t < b.t)
        {
            best = Some(Hit {
                occurrence: o.id,
                face,
                t,
            });
        }
    }
    best
}

/// The box of `o` in assembly coordinates (its part box's corners
/// transformed).
pub fn occurrence_box(o: &Occurrence, local: (Point3, Point3)) -> (Point3, Point3) {
    let (a, b) = local;
    let mut lo = Point3::new(Real::INFINITY, Real::INFINITY, Real::INFINITY);
    let mut hi = -lo;
    for k in 0..8 {
        let c = Point3::new(
            if k & 1 == 0 { a.x } else { b.x },
            if k & 2 == 0 { a.y } else { b.y },
            if k & 4 == 0 { a.z } else { b.z },
        );
        let w = o.transform * c;
        lo = lo.inf(&w);
        hi = hi.sup(&w);
    }
    (lo, hi)
}

/// The box of every visible occurrence of `asm` (assembly coordinates).
pub fn bounding_box(project: &Project, asm: &AssemblyDocument) -> Option<(Point3, Point3)> {
    asm.occurrences
        .iter()
        .filter(|o| o.shown())
        .filter_map(|o| {
            let b = part_of(project, o.component)?.body.bounding_box()?;
            Some(occurrence_box(o, b))
        })
        .reduce(|(a, b), (c, d)| (a.inf(&c), b.sup(&d)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (Project, DocumentId, DocumentId) {
        let mut p = Project::new("p");
        let part = PartDocument::with_box("Box1", [20.0, 10.0, 5.0]).unwrap();
        let bolt = p.add_document("Block", DocumentData::Part(part));
        let asm = p.add_document("Assembly1", DocumentData::Assembly(Default::default()));
        (p, bolt, asm)
    }

    fn asm_of(p: &mut Project, id: DocumentId) -> &mut AssemblyDocument {
        match p.document_mut(id).map(|d| &mut d.data) {
            Some(DocumentData::Assembly(a)) => a,
            _ => panic!("no assembly"),
        }
    }

    #[test]
    fn names_and_ids_count_per_document() {
        let (mut p, block, asm) = project();
        let a = asm_of(&mut p, asm);
        let r = DocRef::local(block);
        let i1 = a.place(r, "Block", Transform3::identity(), true);
        let i2 = a.place(r, "Block", Transform3::translation(30.0, 0.0, 0.0), false);
        let i3 = a.place(r, "Block", Transform3::translation(60.0, 0.0, 0.0), false);
        assert_eq!((i1, i2, i3), (1, 2, 3));
        assert_eq!(a.find(i2).map(|o| o.name.as_str()), Some("Block:2"));
        // Deleting the middle one: the next is Block:4, id 4.
        a.remove(i2).unwrap();
        assert_eq!(a.place(r, "Block", Transform3::identity(), false), 4);
        assert_eq!(
            a.occurrences.last().map(|o| o.name.as_str()),
            Some("Block:4")
        );
        assert_eq!(a.next_name("Other"), "Other:1");
        // Old files: ids 0 get numbers, repeated ids too.
        a.occurrences.iter_mut().for_each(|o| o.id = 0);
        a.occurrences[1].id = 7;
        a.fix_ids();
        let ids: Vec<u32> = a.occurrences.iter().map(|o| o.id).collect();
        assert_eq!(ids, vec![8, 7, 9]);
    }

    #[test]
    fn move_rotate_and_pick_in_the_occurrence_frame() {
        let (mut p, block, asm) = project();
        let r = DocRef::local(block);
        let mesh = Arc::new(PickMesh::of(part_of(&p, r).unwrap()));
        let a = asm_of(&mut p, asm);
        let id1 = a.place(r, "Block", Transform3::identity(), true);
        let id2 = a.place(r, "Block", Transform3::identity(), false);
        let o2 = a.find_mut(id2).unwrap();
        move_by(o2, Vector3::new(100.0, 0.0, 0.0));
        // A quarter turn about the vertical through its own corner (100, 0).
        rotate_about(
            o2,
            Point3::new(100.0, 0.0, 0.0),
            Vector3::z(),
            std::f64::consts::FRAC_PI_2,
        );
        let a = asm_of(&mut p, asm);
        let o2 = a.find(id2).unwrap().clone();
        // The far corner (20, 10, 5) now sits at (90, 20, 5).
        let c = o2.transform * Point3::new(20.0, 10.0, 5.0);
        assert!((c - Point3::new(90.0, 20.0, 5.0)).norm() < 1e-9, "{c}");
        let f = |_r: DocRef| Some(mesh.clone());
        // Straight down onto each: the top face at z = 5, t = 5.
        let down = Vector3::new(0.0, 0.0, -1.0);
        let h = pick(a, &Point3::new(5.0, 5.0, 10.0), &down, &f).unwrap();
        assert_eq!(h.occurrence, id1);
        assert!((h.t - 5.0).abs() < 1e-9);
        let h = pick(a, &Point3::new(95.0, 15.0, 10.0), &down, &f).unwrap();
        assert_eq!(h.occurrence, id2);
        assert!(pick(a, &Point3::new(50.0, 5.0, 10.0), &down, &f).is_none());
        // Disabled ones are drawn but not picked; suppressed ones neither.
        a.find_mut(id2).unwrap().enabled = false;
        assert!(pick(a, &Point3::new(95.0, 15.0, 10.0), &down, &f).is_none());
        a.find_mut(id2).unwrap().enabled = true;
        a.find_mut(id1).unwrap().suppressed = true;
        assert!(pick(a, &Point3::new(5.0, 5.0, 10.0), &down, &f).is_none());
        let keep_first = |o: &Occurrence| o.id == id1;
        a.find_mut(id1).unwrap().suppressed = false;
        // Isolate: only the kept ones are picked.
        assert!(pick_among(a, &Point3::new(95.0, 15.0, 10.0), &down, &f, &keep_first).is_none());
        // Hidden occurrences are not picked and leave the box.
        a.find_mut(id2).unwrap().visible = false;
        assert!(pick(a, &Point3::new(95.0, 15.0, 10.0), &down, &f).is_none());
        let a = a.clone();
        let (lo, hi) = bounding_box(&p, &a).unwrap();
        assert!(hi.x < 20.0 + 1e-6 && lo.x > -1e-6, "{lo} {hi}");
    }

    #[test]
    fn occurrences_round_trip_through_the_file() {
        let (mut p, block, asm) = project();
        let a = asm_of(&mut p, asm);
        let id = a.place(
            DocRef::local(block),
            "Block",
            Transform3::new(Vector3::new(1.0, 2.0, 3.0), Vector3::new(0.1, 0.2, 0.3)),
            false,
        );
        {
            let o = a.find_mut(id).unwrap();
            o.visible = false;
            o.suppressed = true;
            o.enabled = false;
            o.transparent = true;
        }
        let dir = std::env::temp_dir().join(format!("asm-rt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.3dcadcraft");
        p.save(&path).unwrap();
        let mut q = Project::load(&path).unwrap();
        let a0 = asm_of(&mut p, asm).clone();
        let a1 = asm_of(&mut q, asm).clone();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(a1.occurrences.len(), 1);
        let (o0, o1) = (&a0.occurrences[0], &a1.occurrences[0]);
        assert_eq!((o1.id, o1.visible, o1.grounded), (id, false, false));
        assert_eq!(o1.name, "Block:1");
        assert!(o1.suppressed && !o1.enabled && o1.transparent);
        assert_eq!(o0.transform, o1.transform);
    }
}
