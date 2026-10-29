//! A STEP file's assembly structure as an assembly (`docs/spec/7.7a-import.md` stage I2, Inventor's
//! *Convert Model → Assembly*): each product with children becomes an assembly document, each
//! product with bodies a part document holding one Import feature, every occurrence placed where the
//! file puts it. GUI-free.
//!
//! A part's Import embeds only its own bodies: the entities its solids reach, cut out of the
//! source text with the representation holding them (its units), so the geometry and the face
//! entities (the face names) are the file's own, in the product's coordinates, and a design file
//! never holds the whole source once per part. (Writing the bodies out again through the kernel
//! lost faces of supplier files.) Instances of one product (the same solid entities) share one part document; subassemblies of the same name
//! and components share one assembly document.

use std::collections::HashMap;

use crate::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Occurrence, Project};
use crate::kernel::exact::{StepBody, StepProduct, StepRead};
use crate::kernel::{Real, Transform3};
use crate::part::import::ImportOp;
use crate::part::{BodyOp, BodyOpFeature, PartDocument};

/// What [`step_project`] made.
#[derive(Clone, Debug, Default)]
pub struct AssemblyImport {
    /// The top assembly.
    pub top: Option<DocumentId>,
    /// Part documents made.
    pub parts: usize,
    /// Assembly documents made (the top one included).
    pub assemblies: usize,
    /// Occurrences placed, at every level.
    pub occurrences: usize,
    /// The reader's warnings and the products left out.
    pub warnings: Vec<String>,
}

/// Whether the file read as `r` is an assembly (a product with components).
pub fn is_assembly(r: &StepRead) -> bool {
    r.tree.len() > 1 || r.tree.iter().any(|n| !n.children.is_empty())
}

/// A rigid motion from a 4×4 placement (the rotation re-orthonormalised: files write 6-digit
/// directions).
fn isometry(m: &nalgebra::Matrix4<Real>) -> Transform3 {
    let r = m.fixed_view::<3, 3>(0, 0).into_owned();
    let rot = nalgebra::Rotation3::from_matrix(&r);
    let t = nalgebra::Translation3::new(m[(0, 3)], m[(1, 3)], m[(2, 3)]);
    Transform3::from_parts(t, rot.into())
}

struct Builder<'a> {
    text_name: String,
    bodies: &'a [StepBody],
    index: P21Index<'a>,
    project: &'a mut Project,
    /// Part documents by their bodies' solid entities (`None`: it could not be made; its other
    /// instances are not tried again).
    parts: HashMap<Vec<u64>, Option<DocumentId>>,
    /// Assembly documents by product name and component count.
    assemblies: HashMap<(String, usize), DocumentId>,
    /// Names given to documents (unique in the project).
    names: HashMap<String, usize>,
    out: AssemblyImport,
}

impl Builder<'_> {
    fn unique(&mut self, name: &str) -> String {
        let base = if name.trim().is_empty() {
            "Part"
        } else {
            name.trim()
        };
        let n = self.names.entry(base.to_owned()).or_insert(0);
        *n += 1;
        if *n == 1 {
            base.to_owned()
        } else {
            format!("{base}_{n}")
        }
    }

    /// The part of `bodies`: one document per set of solid entities.
    fn part(&mut self, name: &str, bodies: &[usize]) -> Option<DocumentId> {
        let mut key: Vec<u64> = bodies
            .iter()
            .filter_map(|&i| self.bodies.get(i).map(|b| b.item))
            .collect();
        key.sort_unstable();
        if let Some(id) = self.parts.get(&key) {
            return *id;
        }
        let made = self.make_part(name, &key);
        self.parts.insert(key, made);
        made
    }

    /// A new part document of the solids `key` (`None`, with a warning, when it cannot be made).
    fn make_part(&mut self, name: &str, key: &[u64]) -> Option<DocumentId> {
        if key.is_empty() {
            return None;
        }
        let doc_name = self.unique(name);
        let Some(text) = self.index.extract(key, &doc_name) else {
            self.out.warnings.push(format!(
                "{name}: its solids cannot be cut out of the file; left out"
            ));
            return None;
        };
        let mut part = PartDocument::default();
        let op = ImportOp::new(format!("{} › {doc_name}", self.text_name), text);
        if let Err(e) = part.add_body_op(BodyOpFeature::new(BodyOp::Import(op))) {
            self.out.warnings.push(format!("{name}: {e}; left out"));
            return None;
        }
        let id = self
            .project
            .add_document(doc_name, DocumentData::Part(part));
        self.out.parts += 1;
        Some(id)
    }

    /// The assembly of product `n` (at `world`), its components placed.
    fn assembly(&mut self, n: &StepProduct) -> Option<DocumentId> {
        let key = (n.name.clone(), n.children.len());
        if let Some(id) = self.assemblies.get(&key) {
            return Some(*id);
        }
        let mut asm = AssemblyDocument::default();
        // The product's own bodies: a part of its own, at its origin.
        if !n.bodies.is_empty()
            && let Some(p) = self.part(&format!("{} bodies", n.name), &n.bodies)
        {
            self.place(
                &mut asm,
                p,
                &format!("{} bodies", n.name),
                Transform3::identity(),
            );
        }
        for c in &n.children {
            let doc = if c.children.is_empty() {
                self.part(&c.name, &c.bodies)
            } else {
                self.assembly(c)
            };
            if let Some(d) = doc {
                self.place(&mut asm, d, &c.name, isometry(&c.transform));
            }
        }
        if asm.occurrences.is_empty() {
            self.out
                .warnings
                .push(format!("{}: no component could be read; left out", n.name));
            return None;
        }
        let name = self.unique(&n.name);
        let id = self.project.add_document(name, DocumentData::Assembly(asm));
        self.assemblies.insert(key, id);
        self.out.assemblies += 1;
        Some(id)
    }

    fn place(&mut self, asm: &mut AssemblyDocument, doc: DocumentId, name: &str, t: Transform3) {
        let base = if name.trim().is_empty() {
            "Part"
        } else {
            name.trim()
        };
        let mut o = Occurrence::new(asm.next_name(base), DocRef::local(doc), t);
        o.id = asm.next_id();
        // Inventor grounds the first component placed; the others keep their places (no
        // constraints move them).
        o.grounded = asm.occurrences.is_empty();
        asm.occurrences.push(o);
        self.out.occurrences += 1;
    }
}

/// The entities of a STEP exchange structure by id (the text of each `#id=…;`).
struct P21Index<'a> {
    header: &'a str,
    entities: HashMap<u64, &'a str>,
    /// Representations listing each solid among their items.
    reps_of: HashMap<u64, Vec<u64>>,
}

/// The `#id` references in one entity's text (strings skipped).
fn refs(e: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let b = e.as_bytes();
    let mut i = 0;
    let mut quoted = false;
    while i < b.len() {
        match b[i] {
            b'\'' => quoted = !quoted,
            b'#' if !quoted => {
                let j = i + 1;
                let mut k = j;
                while k < b.len() && b[k].is_ascii_digit() {
                    k += 1;
                }
                if let Ok(n) = e[j..k].parse() {
                    out.push(n);
                }
                i = k;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

impl<'a> P21Index<'a> {
    fn new(text: &'a str) -> Option<Self> {
        let data = text.find("DATA;")?;
        let header = &text[..data];
        let mut entities = HashMap::new();
        let mut reps_of: HashMap<u64, Vec<u64>> = HashMap::new();
        let body = &text[data + 5..];
        let b = body.as_bytes();
        let (mut start, mut quoted) = (None::<usize>, false);
        for (i, &c) in b.iter().enumerate() {
            match c {
                b'\'' => quoted = !quoted,
                b'#' if !quoted && start.is_none() => start = Some(i),
                b';' if !quoted => {
                    let Some(s) = start.take() else { continue };
                    let e = &body[s..=i];
                    let Some(eq) = e.find('=') else { continue };
                    let Ok(id) = e[1..eq].trim().parse::<u64>() else {
                        continue;
                    };
                    let rest = e[eq + 1..].trim_start();
                    // A representation's items: every reference before its context (the last).
                    if rest.contains("REPRESENTATION(") && !rest.contains("RELATIONSHIP") {
                        let mut items = refs(rest);
                        items.pop();
                        for r in items {
                            reps_of.entry(r).or_default().push(id);
                        }
                    }
                    entities.insert(id, e);
                }
                _ => {}
            }
        }
        Some(Self {
            header,
            entities,
            reps_of,
        })
    }

    /// A STEP text of just `solids`: each one's representation (the advanced B-rep one first,
    /// for its units) and everything they reach, as loose geometry.
    fn extract(&self, solids: &[u64], name: &str) -> Option<String> {
        let mut todo: Vec<u64> = Vec::new();
        for s in solids {
            let reps = self.reps_of.get(s)?;
            let rep = reps
                .iter()
                .find(|r| {
                    self.entities
                        .get(r)
                        .is_some_and(|e| e.contains("ADVANCED_BREP_SHAPE_REPRESENTATION"))
                })
                .or(reps.first())?;
            todo.push(*rep);
        }
        todo.sort_unstable();
        todo.dedup();
        let mut seen: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();
        while let Some(id) = todo.pop() {
            if !seen.insert(id) {
                continue;
            }
            let e = self.entities.get(&id)?;
            let eq = e.find('=')?;
            todo.extend(refs(&e[eq + 1..]).into_iter().filter(|r| !seen.contains(r)));
        }
        let mut out = String::with_capacity(seen.len() * 64);
        out += self.header.trim_end();
        out += &format!("\n/* 3DCadCraft: the solids of {name} */\nDATA;\n");
        for id in &seen {
            out += self.entities.get(id)?.trim();
            out.push('\n');
        }
        out += "ENDSEC;\nEND-ISO-10303-21;\n";
        Some(out)
    }
}

/// Every solid of STEP `text` cut out on its own (its entity and a STEP text of just it), for
/// finding which one a reader cannot build (`examples/step_info.rs --each`).
pub fn solid_texts(text: &str) -> Vec<(u64, String)> {
    let Some(index) = P21Index::new(text) else {
        return Vec::new();
    };
    let mut ids: Vec<u64> = index
        .entities
        .iter()
        .filter(|(_, e)| {
            let rest = e.split_once('=').map_or("", |x| x.1).trim_start();
            rest.starts_with("MANIFOLD_SOLID_BREP") || rest.starts_with("BREP_WITH_VOIDS")
        })
        .map(|(id, _)| *id)
        .collect();
    ids.sort_unstable();
    ids.into_iter()
        .filter_map(|id| Some((id, index.extract(&[id], &format!("#{id}"))?)))
        .collect()
}

/// The assembly of STEP `text` (read as `r`) added to `project` as documents: the top assembly is
/// the last one added. `file`: the source's name (shown in each part's Import). Document names
/// stay unique in the project. On an error some documents may have been added: callers work on a
/// copy (rule 5).
pub fn step_project(
    r: &StepRead,
    text: &str,
    file: &str,
    project: &mut Project,
) -> Result<AssemblyImport, String> {
    let index = P21Index::new(text).ok_or("not a STEP exchange structure (no DATA section)")?;
    let names = project
        .documents()
        .iter()
        .map(|d| (d.name.clone(), 1))
        .collect();
    let mut b = Builder {
        text_name: file.to_owned(),
        bodies: &r.bodies,
        index,
        project,
        parts: HashMap::new(),
        assemblies: HashMap::new(),
        names,
        out: AssemblyImport {
            warnings: r.warnings.clone(),
            ..Default::default()
        },
    };
    let top = match r.tree.as_slice() {
        [one] => b.assembly(one),
        roots => {
            // Several roots: one top assembly holding them.
            let n = StepProduct {
                name: file
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or("Assembly")
                    .to_owned(),
                transform: nalgebra::Matrix4::identity(),
                bodies: Vec::new(),
                children: roots.to_vec(),
            };
            b.assembly(&n)
        }
    };
    let Some(top) = top else {
        return Err("no component of the file could be read".into());
    };
    b.out.top = Some(top);
    Ok(b.out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::DocumentData;
    use crate::kernel::exact;

    #[test]
    fn a_plain_part_is_no_assembly() {
        let mut p = PartDocument::with_box("Box1", [10.0, 10.0, 10.0]).expect("box");
        p.regenerate().expect("regenerate");
        let text = exact::step_text(&p.exact_bodies.0, "Box").expect("export");
        let r = exact::step_bodies(&text, "Import1").expect("read");
        assert!(!is_assembly(&r), "{:?}", r.tree.len());
    }

    #[test]
    fn products_become_documents_and_instances_share_one() {
        // A tree by hand over one exported box: the same product placed at x = 0 and x = 50.
        let mut p = PartDocument::with_box("Box1", [10.0, 10.0, 10.0]).expect("box");
        p.regenerate().expect("regenerate");
        let text = exact::step_text(&p.exact_bodies.0, "Box").expect("export");
        let mut r = exact::step_bodies(&text, "Import1").expect("read");
        assert_eq!(r.bodies.len(), 1);
        let shift =
            |x: Real| nalgebra::Matrix4::new_translation(&nalgebra::Vector3::new(x, 0.0, 0.0));
        let leaf = |name: &str, body: usize, x: Real| StepProduct {
            name: name.into(),
            transform: shift(x),
            bodies: vec![body],
            children: Vec::new(),
        };
        r.tree = vec![StepProduct {
            name: "Top".into(),
            transform: nalgebra::Matrix4::identity(),
            bodies: Vec::new(),
            children: vec![leaf("Cube", 0, 0.0), leaf("Cube", 0, 50.0)],
        }];
        assert!(is_assembly(&r));
        let mut project = Project::new("X");
        let out = step_project(&r, &text, "x.step", &mut project).expect("import");
        assert_eq!(
            (out.parts, out.assemblies, out.occurrences),
            (1, 1, 2),
            "{out:?}"
        );
        let Some(DocumentData::Assembly(a)) =
            out.top.and_then(|t| project.document(t)).map(|d| &d.data)
        else {
            panic!("no top assembly");
        };
        assert_eq!(a.occurrences.len(), 2);
        assert_eq!(a.occurrences[0].component, a.occurrences[1].component);
        assert!((a.occurrences[1].transform.translation.vector.x - 50.0).abs() < 1e-9);
        assert!(a.occurrences[0].grounded && !a.occurrences[1].grounded);
        // The part holds the body in its own coordinates (at the origin), with the cube's volume.
        let Some(DocumentData::Part(part)) = project
            .document(a.occurrences[0].component.document)
            .map(|d| &d.data)
        else {
            panic!("no part");
        };
        let v: Real = part
            .exact_bodies
            .0
            .iter()
            .filter_map(|s| s.volume_at(1e-3))
            .sum();
        assert!((v - 1000.0).abs() < 1.0, "{v}");
        let (lo, hi) = part.body.bounding_box().expect("a box");
        assert!(
            lo.x.abs() < 1e-6 && (hi.x - 10.0).abs() < 1e-6,
            "{lo:?} {hi:?}"
        );
    }
}
