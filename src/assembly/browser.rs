//! The assembly browser's component rows, GUI-free
//! (`docs/spec/3.0-assembly-commands.md` §3.9a): which first-level
//! components get a row of their own, what each row shows, and the
//! relationships nested under it. Built once per assembly state and kept
//! by [`key`], so an idle frame neither walks the occurrences per row nor
//! filters every relationship for every component (a 625-component
//! assembly asked `pattern_of` and `uses` 625 times each per frame).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use super::AssemblyDocument;
use crate::document::DocRef;

/// One first-level component row.
#[derive(Clone, Debug, PartialEq)]
pub struct OccRow {
    /// Occurrence id.
    pub id: u32,
    /// Its name.
    pub name: String,
    /// What it places.
    pub component: DocRef,
    /// Grounded.
    pub grounded: bool,
    /// Visible.
    pub visible: bool,
    /// Suppressed.
    pub suppressed: bool,
    /// Enabled.
    pub enabled: bool,
    /// A flexible subassembly.
    pub flexible: bool,
    /// A subassembly (the caller's answer: it depends on loaded files).
    pub sub: bool,
    /// A pattern's copy: placed by its pattern, so no degrees of freedom.
    pub element: bool,
    /// The relationships that use it, in the assembly's order.
    pub uses: Vec<u32>,
}

/// The rows of an assembly's components.
#[derive(Clone, Debug, Default)]
pub struct Rows {
    /// First-level components with a row of their own, in the assembly's order.
    pub occs: Vec<OccRow>,
    /// Row index by occurrence id.
    pub index: HashMap<u32, usize>,
}

impl Rows {
    /// The row of occurrence `id`.
    pub fn get(&self, id: u32) -> Option<&OccRow> {
        self.index.get(&id).map(|&k| &self.occs[k])
    }
}

/// A cheap fingerprint of what the rows show (names, flags, components,
/// which relationships use whom, patterns, frames, the tools part), so the
/// rows are rebuilt only when it changes. `hidden`: an occurrence listed
/// elsewhere (the frame assembly's own row).
pub fn key(a: &AssemblyDocument, hidden: Option<u32>) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    hidden.hash(&mut h);
    for o in &a.occurrences {
        (o.id, &o.name, o.component, o.grounded, o.visible).hash(&mut h);
        (o.suppressed, o.enabled, o.flexible).hash(&mut h);
    }
    for c in &a.constraints {
        (c.id, c.a.occurrence(), c.b.occurrence()).hash(&mut h);
        c.plane.as_ref().map(|p| p.occurrence()).hash(&mut h);
    }
    for p in &a.patterns {
        (p.id, &p.sources).hash(&mut h);
        for e in &p.elements {
            e.occurrences.hash(&mut h);
        }
    }
    a.features.as_ref().map(|f| f.occurrence).hash(&mut h);
    for f in &a.frames {
        for m in &f.members {
            m.occurrence.hash(&mut h);
        }
        for c in &f.caps {
            c.occurrence.hash(&mut h);
        }
    }
    h.finish()
}

/// The component rows of `a`. Left out (listed elsewhere): pattern sources
/// and copies (under their pattern), the hidden tools part, frame members
/// and end caps (under their frame) and `hidden`. `is_sub` tells whether a
/// component is a subassembly.
pub fn rows(a: &AssemblyDocument, hidden: Option<u32>, is_sub: impl Fn(DocRef) -> bool) -> Rows {
    use std::collections::HashSet;
    let mut elsewhere: HashSet<u32> = HashSet::new();
    let mut elements: HashSet<u32> = HashSet::new();
    for p in &a.patterns {
        elsewhere.extend(&p.sources);
        for e in &p.elements {
            elsewhere.extend(&e.occurrences);
            elements.extend(&e.occurrences);
        }
    }
    if let Some(f) = &a.features {
        elsewhere.insert(f.occurrence);
    }
    elsewhere.extend(hidden);
    for f in &a.frames {
        elsewhere.extend(f.members.iter().map(|m| m.occurrence));
        elsewhere.extend(f.caps.iter().map(|c| c.occurrence));
    }
    let mut uses: HashMap<u32, Vec<u32>> = HashMap::new();
    for c in &a.constraints {
        let mut on = vec![c.a.occurrence(), c.b.occurrence()];
        on.extend(c.plane.as_ref().map(|p| p.occurrence()));
        on.sort_unstable();
        on.dedup();
        for o in on {
            uses.entry(o).or_default().push(c.id);
        }
    }
    let mut subs: HashMap<DocRef, bool> = HashMap::new();
    let mut out = Rows::default();
    for o in a.occurrences.iter().filter(|o| !elsewhere.contains(&o.id)) {
        let sub = *subs
            .entry(o.component)
            .or_insert_with(|| is_sub(o.component));
        out.index.insert(o.id, out.occs.len());
        out.occs.push(OccRow {
            id: o.id,
            name: o.name.clone(),
            component: o.component,
            grounded: o.grounded,
            visible: o.visible,
            suppressed: o.suppressed,
            enabled: o.enabled,
            flexible: o.flexible,
            sub,
            element: elements.contains(&o.id),
            uses: uses.remove(&o.id).unwrap_or_default(),
        });
    }
    out
}

/// The names along an Edit in place path (spec 3.10 §3.2: the browser's
/// *Top › Sub › Part*): the top assembly's name, then the occurrence names
/// from `top` down `sub`, read through `name_at(path)`; a name that cannot
/// be found reads `?`.
pub fn crumbs(
    top_name: &str,
    path: &[u32],
    name_at: impl Fn(&[u32]) -> Option<String>,
) -> Vec<String> {
    let mut v = vec![top_name.to_owned()];
    for k in 1..=path.len() {
        v.push(name_at(&path[..k]).unwrap_or_else(|| "?".into()));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::pattern::{ComponentPattern, Element};
    use crate::assembly::relation::{Constraint, ConstraintKind, GeomRef, OriginItem};
    use crate::document::{DocumentId, Occurrence};
    use crate::kernel::Transform3;

    fn plane(occurrence: u32) -> GeomRef {
        GeomRef::Origin {
            occurrence,
            sub: Vec::new(),
            item: OriginItem::XyPlane,
        }
    }

    /// `n` components of one part, every one but the first Flush with the
    /// first.
    fn board(n: u32) -> AssemblyDocument {
        let doc = DocRef::local(DocumentId::new_random());
        let mut a = AssemblyDocument::default();
        for k in 1..=n {
            let mut o = Occurrence::new(format!("Part:{k}"), doc, Transform3::identity());
            o.id = k;
            a.occurrences.push(o);
        }
        for k in 2..=n {
            a.add_constraint(Constraint::new(ConstraintKind::Flush, plane(k), plane(1)));
        }
        a
    }

    #[test]
    fn relationships_are_listed_under_both_components() {
        let a = board(4);
        let r = rows(&a, None, |_| false);
        assert_eq!(r.occs.len(), 4);
        assert_eq!(r.get(1).map(|o| o.uses.len()), Some(3));
        for k in 2..=4 {
            assert_eq!(r.get(k).map(|o| o.uses.len()), Some(1));
        }
    }

    #[test]
    fn patterns_tools_and_hidden_rows_are_listed_elsewhere() {
        let mut a = board(6);
        a.patterns.push(ComponentPattern {
            id: 1,
            name: "Pattern:1".into(),
            sources: vec![2],
            layout: crate::assembly::pattern::Layout::Rectangular {
                column: plane(1),
                column_flip: false,
                column_count: 2,
                column_spacing: 10.0,
                row: None,
                row_flip: false,
                row_count: 1,
                row_spacing: 0.0,
            },
            elements: vec![Element {
                index: 2,
                occurrences: vec![3],
            }],
        });
        a.features = Some(crate::assembly::features::AsmFeatures {
            occurrence: 4,
            ..Default::default()
        });
        let r = rows(&a, Some(5), |_| false);
        let ids: Vec<u32> = r.occs.iter().map(|o| o.id).collect();
        assert_eq!(ids, vec![1, 6]);
    }

    #[test]
    fn the_key_follows_what_the_rows_show() {
        let mut a = board(3);
        let k0 = key(&a, None);
        assert_eq!(k0, key(&a.clone(), None));
        a.occurrences[1].visible = false;
        let k1 = key(&a, None);
        assert_ne!(k0, k1);
        a.occurrences[1].name = "Renamed".into();
        assert_ne!(k1, key(&a, None));
        // A moved component shows the same row.
        let k2 = key(&a, None);
        a.occurrences[2].transform = Transform3::translation(1.0, 2.0, 3.0);
        assert_eq!(k2, key(&a, None));
    }

    /// Rows of 625 components and 624 relationships are built in a few
    /// milliseconds and their key in well under one (debug build: the key
    /// is taken every frame, the rows only when it changes).
    #[test]
    fn rows_of_625_components_within_budget() {
        let a = board(625);
        let t = std::time::Instant::now();
        let r = rows(&a, None, |_| false);
        let build = t.elapsed().as_secs_f64() * 1e3;
        let t = std::time::Instant::now();
        let mut k = 0;
        for _ in 0..10 {
            k ^= key(&a, None);
        }
        let hash = t.elapsed().as_secs_f64() * 1e3 / 10.0;
        assert_ne!(k, 1);
        assert_eq!(r.occs.len(), 625);
        assert!(build < 50.0, "rows {build:.2} ms");
        assert!(hash < 4.0, "key {hash:.3} ms");
        eprintln!("625 rows: build {build:.2} ms, key {hash:.3} ms");
    }

    #[test]
    fn crumbs_name_every_level() {
        let names = |p: &[u32]| match p {
            [2] => Some("Bracket:2".to_owned()),
            [2, 7] => Some("Post:1".to_owned()),
            _ => None,
        };
        assert_eq!(
            crumbs("Top", &[2, 7], names),
            vec!["Top", "Bracket:2", "Post:1"]
        );
        assert_eq!(crumbs("Top", &[3], names), vec!["Top", "?"]);
    }
}
