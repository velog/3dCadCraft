//! Many documents in one design file (`docs/spec/0.6b-multi-document-files.md`):
//! the operations of Manage › Documents on a [`Project`] — unique names per
//! kind, who uses a document, duplicate, rename, delete. GUI-free; the GUI
//! records an Undo step before each of them.

use super::{
    DocRef, Document, DocumentData, DocumentId, DocumentKind, Project, ProjectError, ProjectResult,
};

/// The base of new names of a kind (`Part1`, `Assembly1`, `Drawing1`).
pub fn base_name(kind: DocumentKind) -> &'static str {
    match kind {
        DocumentKind::Part => "Part",
        DocumentKind::Assembly => "Assembly",
        DocumentKind::Drawing => "Drawing",
    }
}

impl Project {
    /// Is `name` taken by a document other than `except` (case-insensitive,
    /// as file names on Windows)?
    pub fn name_taken(&self, name: &str, except: Option<DocumentId>) -> bool {
        self.documents
            .iter()
            .any(|d| Some(d.id) != except && d.name.trim().eq_ignore_ascii_case(name.trim()))
    }

    /// The next free `<base><n>` (`Part3` when `Part1` and `Part2` exist).
    pub fn free_name(&self, base: &str) -> String {
        (1..)
            .map(|n| format!("{base}{n}"))
            .find(|n| !self.name_taken(n, None))
            .unwrap_or_else(|| base.to_owned())
    }

    /// Adds a document of a new name `<base><n>` with fresh iProperties and
    /// returns its id (New Part / Assembly / Drawing in this file).
    pub fn add_new_document(&mut self, base: &str, data: DocumentData) -> DocumentId {
        let name = self.free_name(base);
        let id = self.add_document(name, data);
        if let Some(d) = self.document_mut(id) {
            d.properties = super::iprops::IProperties::new_document();
        }
        id
    }

    /// The documents of this file that reference `id` (Show Uses, Delete).
    pub fn users_of(&self, id: DocumentId) -> Vec<DocumentId> {
        self.documents
            .iter()
            .filter(|d| d.data.references().contains(&DocRef::local(id)))
            .map(|d| d.id)
            .collect()
    }

    /// Duplicate Document: a copy of `id` named `<name> Copy` (then
    /// `<name> Copy2`…); references to the original stay where they are.
    pub fn duplicate_document(&mut self, id: DocumentId) -> ProjectResult<DocumentId> {
        let src = self
            .document(id)
            .cloned()
            .ok_or(ProjectError::UnknownDocument(id))?;
        let base = format!("{} Copy", src.name);
        let name = if self.name_taken(&base, None) {
            (2..)
                .map(|n| format!("{base}{n}"))
                .find(|n| !self.name_taken(n, None))
                .unwrap_or(base)
        } else {
            base
        };
        let new = DocumentId::new_random();
        self.documents.push(Document {
            id: new,
            name,
            ..src
        });
        Ok(new)
    }

    /// Rename Document: the id stays (links from other files still
    /// resolve); an empty or taken name is refused.
    pub fn rename_document(&mut self, id: DocumentId, name: &str) -> ProjectResult<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ProjectError::Refused("a name cannot be empty".into()));
        }
        if self.name_taken(name, Some(id)) {
            return Err(ProjectError::Refused(format!(
                "\"{name}\" already exists in this file"
            )));
        }
        let d = self
            .document_mut(id)
            .ok_or(ProjectError::UnknownDocument(id))?;
        d.name = name.to_owned();
        Ok(())
    }

    /// Delete Document: refuses the last document of the file and one that
    /// another document uses (`InUse`, see [`Project::remove_document`]).
    pub fn delete_document(&mut self, id: DocumentId) -> ProjectResult<Document> {
        if self.documents.len() <= 1 && self.document(id).is_some() {
            return Err(ProjectError::Refused(
                "a file keeps at least one document".into(),
            ));
        }
        self.remove_document(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{AssemblyDocument, Occurrence};
    use crate::part::PartDocument;

    fn part() -> DocumentData {
        DocumentData::Part(PartDocument::default())
    }

    #[test]
    fn new_documents_get_the_next_free_name_of_their_kind() {
        let mut p = Project::new("P");
        let a = p.add_new_document("Part", part());
        let b = p.add_new_document("Part", part());
        let c = p.add_new_document(
            base_name(DocumentKind::Assembly),
            DocumentData::Assembly(AssemblyDocument::default()),
        );
        let names: Vec<_> = [a, b, c]
            .iter()
            .map(|&id| p.document(id).unwrap().name.clone())
            .collect();
        assert_eq!(names, ["Part1", "Part2", "Assembly1"]);
        p.rename_document(a, "Bracket").unwrap();
        assert_eq!(p.free_name("Part"), "Part1");
        assert!(!p.document(b).unwrap().properties.is_empty());
    }

    #[test]
    fn rename_refuses_empty_and_taken_names() {
        let mut p = Project::new("P");
        let a = p.add_new_document("Part", part());
        let b = p.add_new_document("Part", part());
        assert!(p.rename_document(b, " part1 ").is_err());
        assert!(p.rename_document(b, "  ").is_err());
        assert_eq!(p.document(b).unwrap().name, "Part2");
        // Its own name (other case) is fine.
        p.rename_document(a, "PART1").unwrap();
        assert_eq!(p.document(a).unwrap().name, "PART1");
    }

    #[test]
    fn duplicate_keeps_the_original_and_its_users() {
        let mut p = Project::new("P");
        let a = p.add_new_document("Part", part());
        let mut asm = AssemblyDocument::default();
        asm.occurrences.push(Occurrence::new(
            "Part1:1",
            DocRef::local(a),
            crate::kernel::Transform3::identity(),
        ));
        let s = p.add_new_document("Assembly", DocumentData::Assembly(asm));
        let c = p.duplicate_document(a).unwrap();
        let c2 = p.duplicate_document(a).unwrap();
        assert_ne!(c, a);
        assert_eq!(p.document(c).unwrap().name, "Part1 Copy");
        assert_eq!(p.document(c2).unwrap().name, "Part1 Copy2");
        assert_eq!(p.users_of(a), vec![s]);
        assert!(p.users_of(c).is_empty());
        p.validate().unwrap();
    }

    #[test]
    fn delete_refuses_used_and_last_documents() {
        let mut p = Project::new("P");
        let a = p.add_new_document("Part", part());
        let mut asm = AssemblyDocument::default();
        asm.occurrences.push(Occurrence::new(
            "Part1:1",
            DocRef::local(a),
            crate::kernel::Transform3::identity(),
        ));
        let s = p.add_new_document("Assembly", DocumentData::Assembly(asm));
        assert!(matches!(
            p.delete_document(a),
            Err(ProjectError::InUse { .. })
        ));
        p.delete_document(s).unwrap();
        assert!(p.delete_document(a).is_err(), "the last document stays");
        assert_eq!(p.documents().len(), 1);
    }
}
