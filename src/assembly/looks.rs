//! Appearance overrides of occurrences (`docs/spec/3.0-assembly-commands.md`
//! §3.7): an assembly colours a component, a feature or a face of it without
//! touching the part (rule 8). The overrides live in `Occurrence::looks`, the
//! appearances they name are copied into `AssemblyDocument::appearances`.

use crate::document::AssemblyDocument;
use crate::materials::{self, Appearance, Library};

/// What an assembly appearance override goes onto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AsmLookTarget {
    /// The whole occurrence.
    Occurrence(u32),
    /// The faces a feature of its part made (feature id).
    Feature {
        /// The occurrence's id.
        occurrence: u32,
        /// The feature's id.
        id: String,
    },
    /// One face of its part (persistent name).
    Face {
        /// The occurrence's id.
        occurrence: u32,
        /// The face's persistent name.
        name: String,
    },
}

impl AsmLookTarget {
    /// The occurrence it belongs to.
    pub fn occurrence(&self) -> u32 {
        match self {
            AsmLookTarget::Occurrence(o)
            | AsmLookTarget::Feature { occurrence: o, .. }
            | AsmLookTarget::Face { occurrence: o, .. } => *o,
        }
    }
}

impl AssemblyDocument {
    /// The appearance `name`: the document's copy first, then the library,
    /// then the built-in one.
    pub fn appearance(&self, name: &str, lib: &Library) -> Option<Appearance> {
        self.appearances
            .iter()
            .find(|a| a.name == name)
            .or_else(|| lib.appearance(name))
            .or_else(|| materials::builtin().appearance(name))
            .cloned()
    }

    /// Gives `target` appearance `name` (`None` removes the override).
    /// `extra` holds appearances found nowhere else (a part's own custom
    /// ones). Returns whether anything changed; an unknown appearance or
    /// occurrence is an error and changes nothing (rule 5).
    pub fn set_look(
        &mut self,
        target: &AsmLookTarget,
        name: Option<&str>,
        lib: &Library,
        extra: &[Appearance],
    ) -> Result<bool, String> {
        let oid = target.occurrence();
        if self.find(oid).is_none() {
            return Err(format!("no component {oid}"));
        }
        if let Some(n) = name
            && !self.appearances.iter().any(|a| a.name == n)
        {
            let a = self
                .appearance(n, lib)
                .or_else(|| extra.iter().find(|a| a.name == n).cloned())
                .ok_or_else(|| format!("unknown appearance {n}"))?;
            self.appearances.push(a);
        }
        let own = name.map(str::to_owned);
        let Some(o) = self.occurrences.iter_mut().find(|o| o.id == oid) else {
            return Err(format!("no component {oid}"));
        };
        let looks = &mut o.looks;
        let before = looks.clone();
        match target {
            AsmLookTarget::Occurrence(_) => looks.appearance = own,
            AsmLookTarget::Feature { id, .. } => set_entry(&mut looks.features, id, own),
            AsmLookTarget::Face { name, .. } => set_entry(&mut looks.faces, name, own),
        }
        let changed = *looks != before;
        self.purge_unused_appearances();
        Ok(changed)
    }

    /// Removes every override of occurrences `ids` (`None`: of all);
    /// returns how many overrides went.
    pub fn clear_looks(&mut self, ids: Option<&[u32]>) -> usize {
        let mut n = 0;
        for o in &mut self.occurrences {
            if ids.is_some_and(|ids| !ids.contains(&o.id)) {
                continue;
            }
            n += o.looks.names().count();
            o.looks = Default::default();
        }
        self.purge_unused_appearances();
        n
    }

    /// Drops the document copies no override uses.
    pub(crate) fn purge_unused_appearances(&mut self) {
        let used: std::collections::BTreeSet<String> = self
            .occurrences
            .iter()
            .flat_map(|o| o.looks.names().map(str::to_owned))
            .collect();
        self.appearances.retain(|a| used.contains(&a.name));
    }
}

fn set_entry(map: &mut std::collections::BTreeMap<String, String>, key: &str, v: Option<String>) {
    match v {
        Some(a) => {
            map.insert(key.to_owned(), a);
        }
        None => {
            map.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{DocRef, DocumentId, Occurrence, feature_of_face};
    use crate::kernel::Transform3;

    fn asm() -> AssemblyDocument {
        let mut a = AssemblyDocument::default();
        for k in 0..2 {
            let mut o = Occurrence::new(
                format!("Plate:{}", k + 1),
                DocRef::local(DocumentId::new_random()),
                Transform3::identity(),
            );
            o.id = k + 1;
            a.occurrences.push(o);
        }
        a
    }

    fn some_appearance() -> String {
        materials::builtin().appearances[0].name.clone()
    }

    #[test]
    fn feature_of_a_face_name() {
        assert_eq!(feature_of_face("Extrusion1:Top"), "Extrusion1");
        assert_eq!(feature_of_face("Pattern1#3:Extrusion1:Top"), "Pattern1");
        assert_eq!(feature_of_face("Box1:Top~Split1.2"), "Box1");
        assert_eq!(feature_of_face("Box1"), "Box1");
    }

    #[test]
    fn occurrence_override_stays_in_the_assembly() {
        let mut a = asm();
        let lib = Library::default();
        let red = some_appearance();
        assert!(
            a.set_look(&AsmLookTarget::Occurrence(2), Some(&red), &lib, &[])
                .unwrap()
        );
        assert_eq!(a.find(2).unwrap().looks.appearance.as_deref(), Some(&*red));
        assert!(a.find(1).unwrap().looks.is_empty());
        assert_eq!(a.appearances.len(), 1);
        // The same again changes nothing.
        assert!(
            !a.set_look(&AsmLookTarget::Occurrence(2), Some(&red), &lib, &[])
                .unwrap()
        );
        // Removing it purges the copy.
        assert!(
            a.set_look(&AsmLookTarget::Occurrence(2), None, &lib, &[])
                .unwrap()
        );
        assert!(a.appearances.is_empty());
    }

    #[test]
    fn precedence_face_feature_occurrence() {
        let mut a = asm();
        let lib = Library::default();
        let names: Vec<String> = materials::builtin()
            .appearances
            .iter()
            .take(3)
            .map(|x| x.name.clone())
            .collect();
        let t = |x: AsmLookTarget, n: &str, a: &mut AssemblyDocument| {
            a.set_look(&x, Some(n), &lib, &[]).unwrap();
        };
        t(AsmLookTarget::Occurrence(1), &names[0], &mut a);
        t(
            AsmLookTarget::Feature {
                occurrence: 1,
                id: "Extrusion1".into(),
            },
            &names[1],
            &mut a,
        );
        t(
            AsmLookTarget::Face {
                occurrence: 1,
                name: "Extrusion1:Top".into(),
            },
            &names[2],
            &mut a,
        );
        let l = &a.find(1).unwrap().looks;
        assert_eq!(l.face_look(Some("Extrusion1:Top")), Some(&*names[2]));
        assert_eq!(l.face_look(Some("Extrusion1:Side")), Some(&*names[1]));
        assert_eq!(l.face_look(Some("Hole1:Wall:1")), Some(&*names[0]));
        assert_eq!(l.face_look(None), Some(&*names[0]));
        assert_eq!(a.appearances.len(), 3);
        assert_eq!(a.clear_looks(Some(&[2])), 0);
        assert_eq!(a.clear_looks(None), 3);
        assert!(a.appearances.is_empty());
    }

    #[test]
    fn unknown_appearance_changes_nothing() {
        let mut a = asm();
        let lib = Library::default();
        assert!(
            a.set_look(
                &AsmLookTarget::Occurrence(1),
                Some("No such look"),
                &lib,
                &[]
            )
            .is_err()
        );
        assert!(
            a.set_look(&AsmLookTarget::Occurrence(9), None, &lib, &[])
                .is_err()
        );
        assert!(a.find(1).unwrap().looks.is_empty());
        assert!(a.appearances.is_empty());
    }

    #[test]
    fn looks_round_trip_and_stay_out_of_old_files() {
        let mut a = asm();
        let plain = serde_json::to_string(&a).unwrap();
        assert!(!plain.contains("looks") && !plain.contains("appearances"));
        let lib = Library::default();
        a.set_look(
            &AsmLookTarget::Face {
                occurrence: 2,
                name: "Box1:Top".into(),
            },
            Some(&some_appearance()),
            &lib,
            &[],
        )
        .unwrap();
        let text = serde_json::to_string(&a).unwrap();
        let b: AssemblyDocument = serde_json::from_str(&text).unwrap();
        assert_eq!(b.find(2).unwrap().looks, a.find(2).unwrap().looks);
        assert_eq!(b.appearances, a.appearances);
    }
}
