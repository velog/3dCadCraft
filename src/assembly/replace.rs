//! Replace / Replace All (`docs/spec/3.0-assembly-commands.md` §3.1): an
//! occurrence shows another component and keeps its id, place, flags and
//! relationships. A relationship whose face or edge the new part does not
//! have becomes sick when the assembly is solved (rule 8: by persistent
//! name, never silently re-targeted).

use crate::document::{AssemblyDocument, DocRef};

impl AssemblyDocument {
    /// The occurrences Replace changes for the chosen `ids`: with `all`,
    /// every occurrence of their components (Replace All); a pattern's
    /// source brings its copies (they show the source's component).
    pub fn replace_targets(&self, ids: &[u32], all: bool) -> Vec<u32> {
        let mut out: Vec<u32> = if all {
            let comps: Vec<DocRef> = ids
                .iter()
                .filter_map(|&i| self.find(i).map(|o| o.component))
                .collect();
            self.occurrences
                .iter()
                .filter(|o| comps.contains(&o.component))
                .map(|o| o.id)
                .collect()
        } else {
            ids.iter()
                .copied()
                .filter(|&i| self.find(i).is_some())
                .collect()
        };
        for p in &self.patterns {
            for (k, s) in p.sources.iter().enumerate() {
                if !out.contains(s) {
                    continue;
                }
                for e in &p.elements {
                    if let Some(&c) = e.occurrences.get(k)
                        && c != 0
                        && !out.contains(&c)
                    {
                        out.push(c);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Makes occurrences `ids` show `new`, named `base:k`. Their feature and
    /// face appearance overrides go (the new part has other features); the
    /// whole occurrence's stays. Returns how many changed; nothing changes on
    /// an error (rule 5).
    pub fn replace(&mut self, ids: &[u32], new: DocRef, base: &str) -> Result<usize, String> {
        if ids.is_empty() {
            return Err("select the components to replace".into());
        }
        if let Some(bad) = ids.iter().find(|&&i| self.find(i).is_none()) {
            return Err(format!("no component {bad}"));
        }
        let todo: Vec<u32> = ids
            .iter()
            .copied()
            .filter(|&i| self.find(i).is_some_and(|o| o.component != new))
            .collect();
        if todo.is_empty() {
            return Err("the components already show that part".into());
        }
        for id in &todo {
            let name = self.next_name(base);
            if let Some(o) = self.find_mut(*id) {
                o.component = new;
                o.name = name;
                o.looks.features.clear();
                o.looks.faces.clear();
            }
        }
        self.purge_unused_appearances();
        Ok(todo.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::pattern::{ComponentPattern, Element, Layout};
    use crate::document::DocumentId;
    use crate::kernel::Transform3;

    fn r() -> DocRef {
        DocRef::local(DocumentId::new_random())
    }

    fn asm(bolt: DocRef, plate: DocRef) -> AssemblyDocument {
        let mut a = AssemblyDocument::default();
        a.place(plate, "Plate", Transform3::identity(), true);
        for k in 0..3 {
            let t = Transform3::translation(10.0 * k as f64, 0.0, 5.0);
            a.place(bolt, "Bolt", t, false);
        }
        a
    }

    #[test]
    fn replace_one_keeps_place_id_and_flags() {
        let (bolt, plate, screw) = (r(), r(), r());
        let mut a = asm(bolt, plate);
        a.find_mut(3).unwrap().transparent = true;
        let before = a.find(3).unwrap().transform;
        let ids = a.replace_targets(&[3], false);
        assert_eq!(ids, vec![3]);
        assert_eq!(a.replace(&ids, screw, "Screw").unwrap(), 1);
        let o = a.find(3).unwrap();
        assert_eq!(o.component, screw);
        assert_eq!(o.name, "Screw:1");
        assert!(o.transparent);
        assert_eq!(o.transform, before);
        // The other bolts stay.
        assert_eq!(a.find(2).unwrap().component, bolt);
        assert_eq!(a.find(4).unwrap().name, "Bolt:3");
    }

    #[test]
    fn replace_all_takes_every_occurrence_of_the_component() {
        let (bolt, plate, screw) = (r(), r(), r());
        let mut a = asm(bolt, plate);
        let ids = a.replace_targets(&[2], true);
        assert_eq!(ids, vec![2, 3, 4]);
        assert_eq!(a.replace(&ids, screw, "Screw").unwrap(), 3);
        let names: Vec<&str> = a.occurrences.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, ["Plate:1", "Screw:1", "Screw:2", "Screw:3"]);
    }

    #[test]
    fn a_pattern_source_brings_its_copies() {
        let (bolt, plate) = (r(), r());
        let mut a = asm(bolt, plate);
        a.patterns.push(ComponentPattern {
            id: 1,
            name: "Component Pattern 1".into(),
            sources: vec![2],
            layout: Layout::Rectangular {
                column: crate::assembly::relation::GeomRef::Origin {
                    sub: Vec::new(),
                    occurrence: crate::assembly::relation::ASSEMBLY,
                    item: crate::assembly::relation::OriginItem::XAxis,
                },
                column_flip: false,
                column_count: 3,
                column_spacing: 10.0,
                row: None,
                row_flip: false,
                row_count: 1,
                row_spacing: 0.0,
            },
            elements: vec![
                Element {
                    index: 2,
                    occurrences: vec![3],
                },
                Element {
                    index: 3,
                    occurrences: vec![0],
                },
            ],
        });
        assert_eq!(a.replace_targets(&[2], false), vec![2, 3]);
    }

    #[test]
    fn errors_change_nothing() {
        let (bolt, plate) = (r(), r());
        let mut a = asm(bolt, plate);
        let before = serde_json::to_string(&a).unwrap();
        assert!(a.replace(&[], bolt, "Bolt").is_err());
        assert!(a.replace(&[9], plate, "Plate").is_err());
        assert!(a.replace(&[2, 3], bolt, "Bolt").is_err());
        assert_eq!(serde_json::to_string(&a).unwrap(), before);
    }

    #[test]
    fn feature_and_face_looks_go_the_occurrence_look_stays() {
        let (bolt, plate, screw) = (r(), r(), r());
        let mut a = asm(bolt, plate);
        let lib = crate::materials::Library::default();
        let names: Vec<String> = crate::materials::builtin()
            .appearances
            .iter()
            .take(2)
            .map(|x| x.name.clone())
            .collect();
        use crate::assembly::looks::AsmLookTarget as T;
        a.set_look(&T::Occurrence(2), Some(&names[0]), &lib, &[])
            .unwrap();
        a.set_look(
            &T::Face {
                occurrence: 2,
                name: "Extrusion1:Top".into(),
            },
            Some(&names[1]),
            &lib,
            &[],
        )
        .unwrap();
        a.replace(&[2], screw, "Screw").unwrap();
        let l = &a.find(2).unwrap().looks;
        assert_eq!(l.appearance.as_deref(), Some(&*names[0]));
        assert!(l.faces.is_empty());
        assert_eq!(a.appearances.len(), 1);
    }
}
