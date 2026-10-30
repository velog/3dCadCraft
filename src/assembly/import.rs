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

use serde::{Deserialize, Serialize};

use crate::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Occurrence, Project};
use crate::kernel::exact::{StepBody, StepProduct, StepRead};
use crate::kernel::{Real, Transform3};
use crate::part::import::{ImportOp, SourceLink};
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

/// What an assembly made from a STEP file remembers of it (spec 7.7a 3a).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AsmSource {
    /// The file the top assembly follows (Reference Model; `None`: a subassembly, or Convert).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<SourceLink>,
    /// Ids of the occurrences that came from the file (Update touches only these).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub occurrences: Vec<u32>,
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
        asm.import = Some(AsmSource {
            link: None,
            occurrences: asm.occurrences.iter().map(|o| o.id).collect(),
        });
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
    /// Representation `rep` rewritten to list, of its solid items, only `solids` (`None`: it holds
    /// no other solid, or its item list cannot be told).
    fn only_items(&self, rep: u64, solids: &[u64]) -> Option<String> {
        const SOLIDS: [&str; 4] = [
            "MANIFOLD_SOLID_BREP",
            "BREP_WITH_VOIDS",
            "SHELL_BASED_SURFACE_MODEL",
            "FACETED_BREP",
        ];
        let e = *self.entities.get(&rep)?;
        // The items: the first parenthesised list after the name.
        let open = e.find(",(")? + 1;
        let close = open + e[open..].find(')')?;
        let items = refs(&e[open..=close]);
        let is_solid = |id: &u64| {
            self.entities
                .get(id)
                .and_then(|t| t.split_once('='))
                .is_some_and(|(_, body)| SOLIDS.iter().any(|k| body.trim_start().starts_with(k)))
        };
        let keep: Vec<u64> = items
            .iter()
            .copied()
            .filter(|i| !is_solid(i) || solids.contains(i))
            .collect();
        if keep.len() == items.len() {
            return None;
        }
        let list = keep
            .iter()
            .map(|i| format!("#{i}"))
            .collect::<Vec<_>>()
            .join(",");
        Some(format!("{}({list}){}", &e[..open], &e[close + 1..]))
    }

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
        // A representation holding other solids too (several bodies of one product) lists only
        // these: the rest of it would come along.
        let own: HashMap<u64, String> = todo
            .iter()
            .filter_map(|&r| Some((r, self.only_items(r, solids)?)))
            .collect();
        let mut seen: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();
        while let Some(id) = todo.pop() {
            if !seen.insert(id) {
                continue;
            }
            let e = own
                .get(&id)
                .map(String::as_str)
                .or(self.entities.get(&id).copied())?;
            let eq = e.find('=')?;
            todo.extend(refs(&e[eq + 1..]).into_iter().filter(|r| !seen.contains(r)));
        }
        let mut out = String::with_capacity(seen.len() * 64);
        out += self.header.trim_end();
        out += &format!("\n/* 3DCadCraft: the solids of {name} */\nDATA;\n");
        for id in &seen {
            let e = own
                .get(id)
                .map(String::as_str)
                .or(self.entities.get(id).copied())?;
            out += e.trim();
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

/// What [`update_project`] changed.
#[derive(Clone, Debug, Default)]
pub struct AsmUpdate {
    /// Each part document updated: (its name, what its Import's update matched).
    pub parts: Vec<(String, crate::part::import::UpdateReport)>,
    /// Occurrences the file added.
    pub added: usize,
    /// Occurrences that took a new placement.
    pub moved: usize,
    /// Occurrences the file no longer has (removed with their constraints).
    pub removed: usize,
    /// Documents made for new products.
    pub new_documents: usize,
    /// Imported documents no longer placed anywhere (removed).
    pub dropped_documents: usize,
    /// The reader's warnings and what could not be updated.
    pub warnings: Vec<String>,
}

/// Whether two placements differ by more than a micrometre or a microradian.
fn moved(a: &Transform3, b: &Transform3) -> bool {
    (a.translation.vector - b.translation.vector).norm() > 1e-6
        || a.rotation.angle_to(&b.rotation) > 1e-6
}

/// **Update** of the assembly `top` of `project`, made by [`step_project`] from a file that has
/// changed, read again as `r` / `text` (spec 7.7a 3a). The new file is imported into a scratch
/// project; its tree and the old one are walked together, occurrences matching by name at each
/// level (only those that came from the file), and so do the documents behind them. A matched
/// part's Import is updated ([`ImportOp::updated`]: faces matched by geometry keep their names); a
/// matched occurrence keeps its id and takes the new placement; new products and occurrences are
/// added, those the file no longer has removed. On an error `project` may be half done: call it
/// on a copy.
pub fn update_project(
    project: &mut Project,
    top: DocumentId,
    r: &StepRead,
    text: &str,
    file: &str,
) -> Result<AsmUpdate, String> {
    let mut fresh = Project::new("update");
    let made = step_project(r, text, file, &mut fresh)?;
    let ftop = made.top.ok_or("no component of the file could be read")?;
    let mut out = AsmUpdate {
        warnings: made.warnings,
        ..Default::default()
    };
    let kind = |p: &Project, d: DocumentId| p.document(d).map(|d| d.data.kind());
    if kind(project, top) != kind(&fresh, ftop) {
        return Err("the file's top product is no longer an assembly".into());
    }
    // 1. The documents that correspond (new -> old), assembly by assembly from the top.
    let mut map: HashMap<DocumentId, DocumentId> = HashMap::from([(ftop, top)]);
    let mut pairs = vec![(top, ftop)];
    let mut i = 0;
    while let Some(&(od, nd)) = pairs.get(i) {
        i += 1;
        let (Some(DocumentData::Assembly(oa)), Some(DocumentData::Assembly(na))) = (
            project.document(od).map(|d| &d.data),
            fresh.document(nd).map(|d| &d.data),
        ) else {
            continue;
        };
        let from_file = oa
            .import
            .as_ref()
            .map(|s| s.occurrences.as_slice())
            .unwrap_or(&[]);
        for no in &na.occurrences {
            let Some(oo) = oa
                .occurrences
                .iter()
                .find(|o| o.name == no.name && from_file.contains(&o.id))
            else {
                continue;
            };
            let (o, n) = (oo.component.document, no.component.document);
            if oo.component.library.is_some()
                || map.contains_key(&n)
                || map.values().any(|v| *v == o)
                || kind(project, o) != kind(&fresh, n)
            {
                continue;
            }
            map.insert(n, o);
            if matches!(
                kind(&fresh, n),
                Some(crate::document::DocumentKind::Assembly)
            ) {
                pairs.push((o, n));
            }
        }
    }
    // 2. Matched parts: their Import reads the new text, faces matched by geometry.
    for (&n, &o) in &map {
        let Some(DocumentData::Part(np)) = fresh.document(n).map(|d| &d.data) else {
            continue;
        };
        let Some(new_text) = np.features.iter().find_map(|f| match f {
            crate::part::Feature::BodyOp(b) => match &b.op {
                BodyOp::Import(op) => Some(op.text.clone()),
                _ => None,
            },
            _ => None,
        }) else {
            continue;
        };
        let Some(doc) = project.document_mut(o) else {
            continue;
        };
        let name = doc.name.clone();
        let DocumentData::Part(op) = &mut doc.data else {
            continue;
        };
        let Some((k, b, imp)) = op.features.iter().enumerate().find_map(|(k, f)| match f {
            crate::part::Feature::BodyOp(b) => match &b.op {
                BodyOp::Import(imp) => Some((k, b.clone(), imp.clone())),
                _ => None,
            },
            _ => None,
        }) else {
            continue;
        };
        if *imp.text == *new_text {
            continue;
        }
        match imp.updated(&b.id, new_text) {
            Ok((next, report)) => {
                let mut nb = b;
                nb.op = BodyOp::Import(next);
                match op.replace_feature(k, crate::part::Feature::BodyOp(nb)) {
                    Ok(_) => out.parts.push((name, report)),
                    Err(e) => out.warnings.push(format!("{name}: {e}")),
                }
            }
            Err(e) => out.warnings.push(format!("{name}: {e}")),
        }
    }
    // 3. New products: their documents move into the project (names kept unique).
    let mut moved_in = Vec::new();
    for d in fresh.documents() {
        if map.contains_key(&d.id) {
            continue;
        }
        let taken = |s: &str| project.documents().iter().any(|x| x.name == s);
        let mut name = d.name.clone();
        let mut k = 1;
        while taken(&name) {
            k += 1;
            name = format!("{}_{k}", d.name);
        }
        let id = project.add_document(name, d.data.clone());
        map.insert(d.id, id);
        moved_in.push(id);
        out.new_documents += 1;
    }
    let local = |r: DocRef| match map.get(&r.document) {
        Some(&d) if r.library.is_none() => DocRef::local(d),
        _ => r,
    };
    for id in &moved_in {
        if let Some(DocumentData::Assembly(a)) = project.document_mut(*id).map(|d| &mut d.data) {
            for o in &mut a.occurrences {
                o.component = local(o.component);
            }
        }
    }
    // 4. Matched assemblies: occurrences moved, added, then removed (ids are never reused).
    let mut dropped: Vec<DocumentId> = Vec::new();
    for &(od, nd) in &pairs {
        let Some(DocumentData::Assembly(na)) = fresh.document(nd).map(|d| d.data.clone()) else {
            continue;
        };
        let Some(DocumentData::Assembly(oa)) = project.document_mut(od).map(|d| &mut d.data) else {
            continue;
        };
        let from_file = oa
            .import
            .as_ref()
            .map(|s| s.occurrences.clone())
            .unwrap_or_default();
        let mut kept = Vec::new();
        for no in &na.occurrences {
            let comp = local(no.component);
            if let Some(oo) = oa
                .occurrences
                .iter_mut()
                .find(|o| o.name == no.name && from_file.contains(&o.id))
            {
                if moved(&oo.transform, &no.transform) {
                    out.moved += 1;
                }
                oo.transform = no.transform;
                oo.component = comp;
                kept.push(oo.id);
            } else {
                let mut o = no.clone();
                o.id = oa.next_id();
                o.component = comp;
                o.grounded = false;
                if oa.occurrences.iter().any(|x| x.name == o.name) {
                    let base = o.name.rsplit_once(':').map_or(o.name.as_str(), |(b, _)| b);
                    o.name = oa.next_name(base);
                }
                kept.push(o.id);
                oa.occurrences.push(o);
                out.added += 1;
            }
        }
        for id in from_file.iter().filter(|id| !kept.contains(id)) {
            if let Some(o) = oa.remove(*id) {
                dropped.push(o.component.document);
                out.removed += 1;
            }
        }
        let src = oa.import.get_or_insert_with(Default::default);
        src.occurrences = kept;
        if let Some(l) = &mut src.link {
            *l = SourceLink::now(&l.path);
        }
    }
    // 5. Imported documents nothing places any more (their own components after them).
    while let Some(d) = dropped.pop() {
        if d == top || dropped.contains(&d) {
            continue;
        }
        let children: Vec<DocumentId> = match project.document(d).map(|x| &x.data) {
            Some(DocumentData::Assembly(a)) if a.import.is_some() => {
                a.occurrences.iter().map(|o| o.component.document).collect()
            }
            Some(DocumentData::Part(_)) | Some(DocumentData::Assembly(_)) => Vec::new(),
            _ => continue,
        };
        if project.remove_document(d).is_ok() {
            out.dropped_documents += 1;
            dropped.extend(children);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::DocumentData;
    use crate::kernel::exact;

    /// Two bodies (a cube of `h` and a small block) written as one STEP text and read back with a
    /// product tree by hand: `(name, body, x)` leaves under "Top".
    fn two_body_file(h: Real, leaves: &[(&str, usize, Real)]) -> (String, StepRead) {
        let a = exact::box_named(
            crate::kernel::Point3::origin(),
            crate::kernel::Point3::new(10.0, 10.0, h),
            "A",
        )
        .expect("box");
        let b = exact::box_named(
            crate::kernel::Point3::origin(),
            crate::kernel::Point3::new(4.0, 4.0, 4.0),
            "B",
        )
        .expect("box");
        let text = exact::step_text(&[a, b], "Two").expect("export");
        let mut r = exact::step_bodies(&text, "Import1").expect("read");
        assert_eq!(r.bodies.len(), 2);
        let shift =
            |x: Real| nalgebra::Matrix4::new_translation(&nalgebra::Vector3::new(x, 0.0, 0.0));
        r.tree = vec![StepProduct {
            name: "Top".into(),
            transform: nalgebra::Matrix4::identity(),
            bodies: Vec::new(),
            children: leaves
                .iter()
                .map(|&(name, body, x)| StepProduct {
                    name: name.into(),
                    transform: shift(x),
                    bodies: vec![body],
                    children: Vec::new(),
                })
                .collect(),
        }];
        (text, r)
    }

    fn asm(p: &Project, id: DocumentId) -> &AssemblyDocument {
        match p.document(id).map(|d| &d.data) {
            Some(DocumentData::Assembly(a)) => a,
            _ => panic!("no assembly"),
        }
    }

    /// Spec 7.7a 3a.3-3a.6: an updated file moves, adds and removes the occurrences that came from
    /// it, keeps their ids, updates the parts' faces by geometry and leaves an occurrence the user
    /// placed alone.
    #[test]
    fn an_assembly_follows_its_changed_file() {
        let (t0, r0) = two_body_file(
            10.0,
            &[("Cube", 0, 0.0), ("Cube", 0, 50.0), ("Gone", 1, 200.0)],
        );
        let mut p = Project::new("X");
        let made = step_project(&r0, &t0, "x.step", &mut p).expect("import");
        let top = made.top.expect("top");
        let before = asm(&p, top).clone();
        assert_eq!(before.occurrences.len(), 3);
        assert_eq!(before.import.as_ref().map(|s| s.occurrences.len()), Some(3));
        let cube = before.occurrences[0].component;
        let ids: Vec<u32> = before.occurrences.iter().map(|o| o.id).collect();
        // The user places another cube by hand.
        let Some(DocumentData::Assembly(a)) = p.document_mut(top).map(|d| &mut d.data) else {
            panic!("no assembly");
        };
        let mine = a.place(cube, "Mine", Transform3::translation(0.0, 99.0, 0.0), false);
        let docs = p.documents().len();
        // The new file: the cube 5 mm taller, the second one at x = 60, "Gone" gone, "Peg" new.
        let (t1, r1) = two_body_file(
            15.0,
            &[("Cube", 0, 0.0), ("Cube", 0, 60.0), ("Peg", 1, 100.0)],
        );
        let u = update_project(&mut p, top, &r1, &t1, "x.step").expect("update");
        assert_eq!((u.added, u.moved, u.removed), (1, 1, 1), "{u:?}");
        assert_eq!((u.new_documents, u.dropped_documents), (1, 1), "{u:?}");
        assert_eq!(p.documents().len(), docs, "Peg came, Gone went");
        // The cube part: the top face moved, the other five kept their names.
        assert_eq!(u.parts.len(), 1, "{u:?}");
        let rep = &u.parts[0].1;
        assert_eq!((rep.matched, rep.new, rep.lost), (5, 1, 1), "{rep:?}");
        let a = asm(&p, top);
        let by = |name: &str| a.occurrences.iter().find(|o| o.name == name);
        let c1 = by("Cube:1").expect("Cube:1");
        let c2 = by("Cube:2").expect("Cube:2");
        assert_eq!((c1.id, c2.id), (ids[0], ids[1]), "ids kept");
        assert!((c2.transform.translation.vector.x - 60.0).abs() < 1e-9);
        assert!(by("Gone:1").is_none());
        let peg = by("Peg:1").expect("Peg:1");
        assert!(!ids.contains(&peg.id), "a new id");
        // The user's occurrence is untouched and not counted as the file's.
        let m = a.occurrences.iter().find(|o| o.id == mine).expect("mine");
        assert!((m.transform.translation.vector.y - 99.0).abs() < 1e-9);
        let src = a.import.as_ref().expect("source");
        assert!(!src.occurrences.contains(&mine));
        assert_eq!(src.occurrences.len(), 3);
        // The cube part's body is 15 mm high now.
        let Some(DocumentData::Part(part)) = p.document(c1.component.document).map(|d| &d.data)
        else {
            panic!("no part");
        };
        let (lo, hi) = part.body.bounding_box().expect("box");
        assert!(((hi.z - lo.z) - 15.0).abs() < 1e-6, "{lo:?} {hi:?}");
        // Updating again from the same file changes nothing.
        let again = update_project(&mut p, top, &r1, &t1, "x.step").expect("again");
        assert_eq!(
            (again.added, again.moved, again.removed, again.parts.len()),
            (0, 0, 0, 0),
            "{again:?}"
        );
    }

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
