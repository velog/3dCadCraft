//! # Projects, documents and the `.3dcadcraft` file format
//!
//! ## Model
//!
//! A **project** is the unit the user opens, saves and copies. It is stored
//! as **one file** (`name.3dcadcraft`) and holds any number of **documents**:
//!
//! | Document  | Contents                                   | Inventor analogue |
//! |-----------|--------------------------------------------|-------------------|
//! | Part      | sketches + solid body (later: feature tree) | `.ipt`            |
//! | Assembly  | placed occurrences of parts/sub-assemblies  | `.iam`            |
//! | Drawing   | sheets with views of parts                  | `.idw` / `.dwg`   |
//!
//! (Presentations will become a further document kind.)
//!
//! Copying a `.3dcadcraft` file copies the whole design — nothing can go
//! missing, unlike multi-file CAD datasets where an assembly silently loses
//! its parts when moved without "Pack and Go".
//!
//! ## Reuse across projects (Blender-style *Link* / *Append*)
//!
//! - **Link** — an assembly in project A references a part that lives in
//!   project B (a *library*, e.g. `fasteners.3dcadcraft`). The part stays in B;
//!   editing B updates every project that links it. Stored as a relative path
//!   plus B's [`ProjectId`] and the part's [`DocumentId`].
//! - **Append** — copies a document (and everything it depends on) into the
//!   current project, giving it fresh IDs. The copy is independent.
//! - **Pack** — appends every linked document so the project becomes fully
//!   self-contained again (the equivalent of Inventor's *Pack and Go*).
//!
//! All references are by UUID, never by name or position, so renaming a
//! document or reordering a project never breaks a link.
//!
//! See [`format`](mod@format) for the on-disk layout and [`library`] for link resolution.

pub mod design_project;
pub mod documents;
pub mod filetype;
pub mod format;
pub mod library;
pub mod storage;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::kernel::Transform3;
pub use crate::part::{PartDocument, PartSketch};

pub mod iprops;
pub use design_project::DesignProject;
pub use filetype::FileType;
pub use format::{FILE_EXTENSION, FORMAT_REVISION, FORMAT_VERSION, SaveOptions};
pub use library::Resolver;

macro_rules! uuid_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            /// A fresh, globally unique ID.
            pub fn new_random() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

uuid_id!(
    /// Identity of a project. Survives renaming and copying the file, which
    /// lets a link verify it found the right library.
    ProjectId
);
uuid_id!(
    /// Identity of a document, unique across all projects.
    DocumentId
);
uuid_id!(
    /// Identity of a library link inside one project.
    LibraryId
);

/// Everything that can go wrong with projects and files.
#[derive(Debug, Error)]
pub enum ProjectError {
    /// Operating-system I/O failure.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// File or directory involved.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The file is not a valid 3DCadCraft project.
    #[error("invalid project file {path}: {reason}")]
    Corrupt {
        /// File involved.
        path: PathBuf,
        /// What is wrong.
        reason: String,
    },
    /// Written by a newer 3DCadCraft with an incompatible format.
    #[error("{path} uses format v{found}; this build reads up to v{supported}")]
    UnsupportedVersion {
        /// File involved.
        path: PathBuf,
        /// Version in the file.
        found: u32,
        /// Newest version this build understands.
        supported: u32,
    },
    /// Written by a newer build using something this one cannot read
    /// (same format version, higher [`FORMAT_REVISION`]).
    #[error(
        "{path} was made by a newer 3DCadCraft ({generator}, revision {found}; this build reads revision {supported}): update the program ({reason})"
    )]
    NewerRevision {
        /// File involved.
        path: PathBuf,
        /// The build that wrote it.
        generator: String,
        /// Revision in the file.
        found: u32,
        /// Newest revision this build writes.
        supported: u32,
        /// The parse error.
        reason: String,
    },
    /// A document operation the file's state does not allow (a taken
    /// name, deleting the last document…); the text says why.
    #[error("{0}")]
    Refused(String),
    /// A [`DocumentId`] that is not in the project.
    #[error("document {0} not found")]
    UnknownDocument(DocumentId),
    /// A [`LibraryId`] that is not linked in the project.
    #[error("library {0} is not linked in this project")]
    UnknownLibrary(LibraryId),
    /// A linked library file could not be found.
    #[error("linked library not found: {path}")]
    MissingLibrary {
        /// Where it was expected.
        path: PathBuf,
    },
    /// The file at a library path is a different project than the one linked.
    #[error("{path} is not the linked library (expected project {expected}, found {found})")]
    LibraryMismatch {
        /// File involved.
        path: PathBuf,
        /// Linked project ID.
        expected: ProjectId,
        /// Project ID found in the file.
        found: ProjectId,
    },
    /// The document is still referenced by an assembly.
    #[error("document {document} is still used by assembly {user}")]
    InUse {
        /// Document that was to be removed.
        document: DocumentId,
        /// Assembly referencing it.
        user: DocumentId,
    },
    /// Assemblies reference each other in a loop.
    #[error("reference cycle through document {0}")]
    Cycle(DocumentId),
    /// Linking needs a file location to compute relative paths.
    #[error("the project must be saved before linking libraries")]
    NotSaved,
}

/// Convenience alias.
pub type ProjectResult<T> = Result<T, ProjectError>;

// ============================================================================
// Documents
// ============================================================================

/// Where a referenced document lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocRef {
    /// `None`: in the same project. `Some`: in a linked library.
    pub library: Option<LibraryId>,
    /// The referenced document.
    pub document: DocumentId,
}

impl DocRef {
    /// A reference to a document of the same project.
    pub fn local(document: DocumentId) -> Self {
        Self {
            library: None,
            document,
        }
    }
}

/// One placed instance of a part or sub-assembly.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Occurrence {
    /// Stable identity inside its assembly (occurrence paths, relations);
    /// never reused. 0 only in files written before ids existed:
    /// [`AssemblyDocument::fix_ids`] numbers those on load.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub id: u32,
    /// Display name, e.g. "Bolt:3".
    pub name: String,
    /// What is placed.
    pub component: DocRef,
    /// Placement in assembly coordinates.
    pub transform: Transform3,
    /// Grounded occurrences are never moved by the assembly solver.
    pub grounded: bool,
    /// Shown in the view (Inventor's per-occurrence Visibility).
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub visible: bool,
    /// BOM Structure of this occurrence only (Reference), over its
    /// document's (`docs/spec/3.6-bom.md`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bom: Option<iprops::BomStructure>,
    /// Suppress: left out of the view, picking, the solver (its
    /// relationships rest) and the BOM, but kept with its relationships.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
    /// Enabled: off = drawn faint and see-through and never picked in the
    /// view (Inventor's way of working past large components).
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub enabled: bool,
    /// Transparent: drawn see-through (still picked).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub transparent: bool,
    /// Appearance overrides of this occurrence (the assembly's, never
    /// written into the part — rule 8).
    #[serde(default, skip_serializing_if = "OccurrenceLooks::is_empty")]
    pub looks: OccurrenceLooks,
    /// Adaptive (spec 3.10 §3.4): the part's adaptive sketches and features
    /// take their free sizes from this occurrence's relationships
    /// (`assembly::adapt`); at most one occurrence of a part.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub adaptive: bool,
}

/// Appearance overrides an assembly gives one occurrence, by appearance
/// name (copies in [`AssemblyDocument::appearances`]). Precedence: face >
/// feature > the whole occurrence > the part's own looks.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OccurrenceLooks {
    /// The whole occurrence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<String>,
    /// Per feature of the part (by feature id): the faces it made.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub features: std::collections::BTreeMap<String, String>,
    /// Per face (by persistent face name).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub faces: std::collections::BTreeMap<String, String>,
}

impl OccurrenceLooks {
    /// No override.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The appearance a face named `face` gets from the assembly, if any.
    pub fn face_look(&self, face: Option<&str>) -> Option<&str> {
        if let Some(f) = face {
            if let Some(a) = self.faces.get(f) {
                return Some(a);
            }
            if let Some(a) = self.features.get(feature_of_face(f)) {
                return Some(a);
            }
        }
        self.appearance.as_deref()
    }

    /// Every appearance name in use.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.appearance
            .iter()
            .chain(self.features.values())
            .chain(self.faces.values())
            .map(String::as_str)
    }
}

/// The feature that made a face, from its persistent name: `Extrusion1:Top`
/// → `Extrusion1`; a pattern copy `Pattern1#3:Extrusion1:Top` → `Pattern1`;
/// a split piece `Box1:Top~Split1.2` → `Box1`.
pub fn feature_of_face(name: &str) -> &str {
    let end = name.find([':', '#', '~']).unwrap_or(name.len());
    &name[..end]
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

fn yes() -> bool {
    true
}

fn is_yes(v: &bool) -> bool {
    *v
}

impl Occurrence {
    /// A visible, not grounded occurrence of `component` (id 0 until an
    /// assembly takes it, see `AssemblyDocument::place`).
    pub fn new(name: impl Into<String>, component: DocRef, transform: Transform3) -> Self {
        Self {
            id: 0,
            name: name.into(),
            component,
            transform,
            grounded: false,
            visible: true,
            bom: None,
            suppressed: false,
            enabled: true,
            transparent: false,
            looks: OccurrenceLooks::default(),
            adaptive: false,
        }
    }

    /// Drawn in the view: visible and not suppressed.
    pub fn shown(&self) -> bool {
        self.visible && !self.suppressed
    }

    /// Picked in the view: shown and enabled.
    pub fn pickable(&self) -> bool {
        self.shown() && self.enabled
    }
}

/// An assembly: occurrences of other documents. The same part may be placed
/// many times; it is stored once. Editing operations are in
/// [`crate::assembly`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AssemblyDocument {
    /// Placed components.
    pub occurrences: Vec<Occurrence>,
    /// Constraints between them (`assembly::relation`). Older builds
    /// ignore them (the components keep their saved places).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<crate::assembly::Constraint>,
    /// Component patterns (`assembly::pattern`); their copies are among
    /// the occurrences, so older builds show them where they were saved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<crate::assembly::pattern::ComponentPattern>,
    /// Document copies of the appearances the occurrences' overrides use
    /// (`Occurrence::looks`), so the file shows the same without the
    /// user's library.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub appearances: Vec<crate::materials::Appearance>,
    /// Frames (`assembly::frame`, spec 6.1): what each frame member was
    /// made from, so it follows its skeleton. The members are ordinary
    /// occurrences, so older builds show them where they were saved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<crate::assembly::frame::Frame>,
}

/// The payload of a document.
// Projects hold few documents, so the size gap between variants is
// irrelevant; boxing would only make every match noisier.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum DocumentData {
    /// A part.
    Part(PartDocument),
    /// An assembly.
    Assembly(AssemblyDocument),
    /// A drawing: sheets with views of parts (`crate::drawing`).
    Drawing(crate::drawing::DrawingDocument),
}

/// Kind of a document, for indexes and UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentKind {
    /// [`DocumentData::Part`].
    Part,
    /// [`DocumentData::Assembly`].
    Assembly,
    /// [`DocumentData::Drawing`].
    Drawing,
}

impl DocumentData {
    /// The kind of this payload.
    pub fn kind(&self) -> DocumentKind {
        match self {
            DocumentData::Part(_) => DocumentKind::Part,
            DocumentData::Assembly(_) => DocumentKind::Assembly,
            DocumentData::Drawing(_) => DocumentKind::Drawing,
        }
    }

    /// Every document this one references.
    pub fn references(&self) -> Vec<DocRef> {
        match self {
            DocumentData::Part(_) => Vec::new(),
            DocumentData::Assembly(a) => a.occurrences.iter().map(|o| o.component).collect(),
            DocumentData::Drawing(d) => d.references(),
        }
    }

    /// Rewrites every reference through `f`.
    pub(crate) fn remap_references(&mut self, mut f: impl FnMut(DocRef) -> DocRef) {
        match self {
            DocumentData::Assembly(a) => {
                for o in &mut a.occurrences {
                    o.component = f(o.component);
                }
            }
            DocumentData::Drawing(d) => d.remap_references(&mut f),
            DocumentData::Part(_) => {}
        }
    }
}

/// A document of a project.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    /// Stable identity.
    pub id: DocumentId,
    /// Display name (need not be unique).
    pub name: String,
    /// Contents.
    pub data: DocumentData,
    /// iProperties (`iprops`): title, part number, custom ones…
    #[serde(default, skip_serializing_if = "iprops::IProperties::is_empty")]
    pub properties: iprops::IProperties,
}

/// A link to another `.3dcadcraft` project used as a library.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryLink {
    /// Identity of this link inside the project.
    pub id: LibraryId,
    /// Path of the library file relative to this project's directory,
    /// always with `/` separators so files move between OSes unchanged.
    pub path: String,
    /// The library's project ID, checked when resolving.
    pub project: ProjectId,
    /// The library's name when it was linked (for messages and UI).
    pub name: String,
}

// ============================================================================
// Project
// ============================================================================

/// A 3DCadCraft project: an ordered set of documents plus library links.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    /// Stable identity.
    pub id: ProjectId,
    /// Display name.
    pub name: String,
    documents: Vec<Document>,
    libraries: Vec<LibraryLink>,
    /// Set when the file was written by a newer build (higher format
    /// revision) and still opened: what may be built differently. Not saved.
    #[serde(skip)]
    pub newer_file: Option<String>,
    /// The preview picture (`thumbnail.png`, PNG bytes) written with the
    /// project, and the document it shows. Kept from the loaded file until
    /// replaced (the GUI draws a new one on every save).
    #[serde(skip)]
    pub thumbnail: Option<Vec<u8>>,
    /// The document the picture shows (the active one at save).
    #[serde(skip)]
    pub preview: Option<DocumentId>,
}

impl Project {
    /// An empty project.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: ProjectId::new_random(),
            name: name.into(),
            documents: Vec::new(),
            libraries: Vec::new(),
            newer_file: None,
            thumbnail: None,
            preview: None,
        }
    }

    /// Adds a document and returns its new ID.
    pub fn add_document(&mut self, name: impl Into<String>, data: DocumentData) -> DocumentId {
        let id = DocumentId::new_random();
        self.documents.push(Document {
            id,
            name: name.into(),
            data,
            properties: Default::default(),
        });
        id
    }

    /// All documents, in project order.
    pub fn documents(&self) -> &[Document] {
        &self.documents
    }

    /// Looks up a document.
    pub fn document(&self, id: DocumentId) -> Option<&Document> {
        self.documents.iter().find(|d| d.id == id)
    }

    /// Looks up a document mutably.
    pub fn document_mut(&mut self, id: DocumentId) -> Option<&mut Document> {
        self.documents.iter_mut().find(|d| d.id == id)
    }

    /// Removes a document, refusing if an assembly of this project still
    /// places it.
    pub fn remove_document(&mut self, id: DocumentId) -> ProjectResult<Document> {
        let pos = self
            .documents
            .iter()
            .position(|d| d.id == id)
            .ok_or(ProjectError::UnknownDocument(id))?;
        if let Some(user) = self
            .documents
            .iter()
            .find(|d| d.data.references().contains(&DocRef::local(id)))
        {
            return Err(ProjectError::InUse {
                document: id,
                user: user.id,
            });
        }
        Ok(self.documents.remove(pos))
    }

    /// Linked libraries.
    pub fn libraries(&self) -> &[LibraryLink] {
        &self.libraries
    }

    /// Looks up a library link.
    pub fn library(&self, id: LibraryId) -> Option<&LibraryLink> {
        self.libraries.iter().find(|l| l.id == id)
    }

    /// Adds a library link, reusing an existing link to the same project.
    pub(crate) fn add_library(
        &mut self,
        path: String,
        project: ProjectId,
        name: String,
    ) -> LibraryId {
        if let Some(l) = self.libraries.iter_mut().find(|l| l.project == project) {
            l.path = path; // the library may have moved; keep the newest location
            return l.id;
        }
        let id = LibraryId::new_random();
        self.libraries.push(LibraryLink {
            id,
            path,
            project,
            name,
        });
        id
    }

    /// Drops library links no document uses any more.
    pub fn prune_libraries(&mut self) {
        let used: std::collections::HashSet<LibraryId> = self
            .documents
            .iter()
            .flat_map(|d| d.data.references())
            .filter_map(|r| r.library)
            .collect();
        self.libraries.retain(|l| used.contains(&l.id));
    }

    /// Checks internal consistency: unique document IDs, every local
    /// reference resolves, every library reference has a link, and local
    /// assemblies do not form cycles. (Library contents are checked by
    /// [`Resolver`] when they are loaded.)
    pub fn validate(&self) -> ProjectResult<()> {
        let mut seen = std::collections::HashSet::new();
        for d in &self.documents {
            if !seen.insert(d.id) {
                return Err(ProjectError::Corrupt {
                    path: PathBuf::new(),
                    reason: format!("duplicate document id {}", d.id),
                });
            }
        }
        for d in &self.documents {
            for r in d.data.references() {
                match r.library {
                    None if self.document(r.document).is_none() => {
                        return Err(ProjectError::UnknownDocument(r.document));
                    }
                    Some(lib) if self.library(lib).is_none() => {
                        return Err(ProjectError::UnknownLibrary(lib));
                    }
                    _ => {}
                }
            }
        }
        // Depth-first search for local cycles.
        fn visit(p: &Project, id: DocumentId, stack: &mut Vec<DocumentId>) -> ProjectResult<()> {
            if stack.contains(&id) {
                return Err(ProjectError::Cycle(id));
            }
            stack.push(id);
            if let Some(d) = p.document(id) {
                for r in d
                    .data
                    .references()
                    .into_iter()
                    .filter(|r| r.library.is_none())
                {
                    visit(p, r.document, stack)?;
                }
            }
            stack.pop();
            Ok(())
        }
        for d in &self.documents {
            visit(self, d.id, &mut Vec::new())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn part() -> DocumentData {
        DocumentData::Part(PartDocument::default())
    }

    pub(crate) fn assembly_of(refs: &[DocRef]) -> DocumentData {
        DocumentData::Assembly(AssemblyDocument {
            occurrences: refs
                .iter()
                .enumerate()
                .map(|(i, &component)| Occurrence {
                    id: i as u32 + 1,
                    grounded: i == 0,
                    ..Occurrence::new(
                        format!("Component:{}", i + 1),
                        component,
                        Transform3::identity(),
                    )
                })
                .collect(),
            constraints: Vec::new(),
            patterns: Vec::new(),
            appearances: Vec::new(),
            frames: Vec::new(),
        })
    }

    #[test]
    fn referenced_document_cannot_be_removed() {
        let mut p = Project::new("p");
        let bolt = p.add_document("Bolt", part());
        let asm = p.add_document(
            "Asm",
            assembly_of(&[DocRef::local(bolt), DocRef::local(bolt)]),
        );
        assert!(
            matches!(p.remove_document(bolt), Err(ProjectError::InUse { user, .. }) if user == asm)
        );
        p.remove_document(asm).unwrap();
        p.remove_document(bolt).unwrap();
    }

    #[test]
    fn validate_detects_cycles_and_dangling_refs() {
        let mut p = Project::new("p");
        let a = p.add_document("A", part());
        let b = p.add_document("B", assembly_of(&[DocRef::local(a)]));
        p.validate().unwrap();

        // Make A an assembly that contains B: A → B → A.
        p.document_mut(a).unwrap().data = assembly_of(&[DocRef::local(b)]);
        assert!(matches!(p.validate(), Err(ProjectError::Cycle(_))));

        p.document_mut(a).unwrap().data = assembly_of(&[DocRef::local(DocumentId::new_random())]);
        assert!(matches!(
            p.validate(),
            Err(ProjectError::UnknownDocument(_))
        ));
    }
}
