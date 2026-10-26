//! File types (`docs/spec/0.6a-file-types-projects.md` §3.1): every document
//! kind has its own extension, so a file is recognised by its name alone —
//! `.ccpart`, `.ccasm`, `.ccdrw`, `.ccpres` — beside the design project
//! (`.ccproj`) and the old multi-document package (`.3dcadcraft`).
//!
//! The model files share one container (the ZIP of [`format`](super::format));
//! loading reads the manifest and never trusts the extension.

use std::path::Path;

use super::DocumentKind;

/// What a file is, by its extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileType {
    /// One part (also sheet metal).
    Part,
    /// One assembly.
    Assembly,
    /// One drawing.
    Drawing,
    /// One presentation (document kind still to come).
    Presentation,
    /// Several documents in one file (Pack and Go, old files).
    Package,
    /// A design project: where a design's files live (not a model).
    Project,
}

impl FileType {
    /// Every type, model files first.
    pub const ALL: [FileType; 6] = [
        FileType::Part,
        FileType::Assembly,
        FileType::Drawing,
        FileType::Presentation,
        FileType::Package,
        FileType::Project,
    ];

    /// The types that hold documents (what Open lists).
    pub const MODELS: [FileType; 5] = [
        FileType::Part,
        FileType::Assembly,
        FileType::Drawing,
        FileType::Presentation,
        FileType::Package,
    ];

    /// The extension, without the dot.
    pub fn ext(self) -> &'static str {
        match self {
            FileType::Part => "ccpart",
            FileType::Assembly => "ccasm",
            FileType::Drawing => "ccdrw",
            FileType::Presentation => "ccpres",
            FileType::Package => super::FILE_EXTENSION,
            FileType::Project => "ccproj",
        }
    }

    /// The name shown in file filters and on Home.
    pub fn name(self) -> &'static str {
        match self {
            FileType::Part => "Part",
            FileType::Assembly => "Assembly",
            FileType::Drawing => "Drawing",
            FileType::Presentation => "Presentation",
            FileType::Package => "Package",
            FileType::Project => "Project",
        }
    }

    /// A filter line, e.g. `Part (*.ccpart)`.
    pub fn filter(self) -> String {
        format!("{} (*.{})", self.name(), self.ext())
    }

    /// The type of a path by its extension (case-insensitive).
    pub fn from_path(path: &Path) -> Option<FileType> {
        let ext = path.extension()?.to_str()?;
        FileType::ALL
            .into_iter()
            .find(|t| t.ext().eq_ignore_ascii_case(ext))
    }

    /// The single-document type a document of `kind` is saved as.
    pub fn for_kind(kind: DocumentKind) -> FileType {
        match kind {
            DocumentKind::Part => FileType::Part,
            DocumentKind::Assembly => FileType::Assembly,
            DocumentKind::Drawing => FileType::Drawing,
        }
    }

    /// The document kind a single-document file holds (`None` for the
    /// package, the project and kinds not built yet).
    pub fn kind(self) -> Option<DocumentKind> {
        match self {
            FileType::Part => Some(DocumentKind::Part),
            FileType::Assembly => Some(DocumentKind::Assembly),
            FileType::Drawing => Some(DocumentKind::Drawing),
            FileType::Presentation | FileType::Package | FileType::Project => None,
        }
    }

    /// Holds documents (opened as a model, not as a project).
    pub fn is_model(self) -> bool {
        self != FileType::Project
    }

    /// The extensions of every model file (Open's *All 3DCadCraft files*).
    pub fn model_exts() -> Vec<&'static str> {
        FileType::MODELS.iter().map(|t| t.ext()).collect()
    }
}

/// `path` with the extension of `ty` (replacing another model extension,
/// keeping an unrelated dot in the name: `bracket v2` → `bracket v2.ccpart`).
pub fn with_type(path: &Path, ty: FileType) -> std::path::PathBuf {
    if FileType::from_path(path).is_some() {
        path.with_extension(ty.ext())
    } else {
        let mut s = path.as_os_str().to_owned();
        s.push(".");
        s.push(ty.ext());
        s.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn extensions_are_distinct_and_round_trip() {
        let mut seen = std::collections::HashSet::new();
        for t in FileType::ALL {
            assert!(seen.insert(t.ext()), "{} repeats", t.ext());
            let p = PathBuf::from(format!("x.{}", t.ext().to_uppercase()));
            assert_eq!(FileType::from_path(&p), Some(t));
        }
        assert_eq!(FileType::from_path(Path::new("x.ipt")), None);
        assert_eq!(FileType::from_path(Path::new("noext")), None);
    }

    #[test]
    fn kinds_map_both_ways() {
        for k in [
            DocumentKind::Part,
            DocumentKind::Assembly,
            DocumentKind::Drawing,
        ] {
            assert_eq!(FileType::for_kind(k).kind(), Some(k));
        }
        assert!(!FileType::Project.is_model());
        assert!(FileType::model_exts().contains(&"3dcadcraft"));
        assert!(!FileType::model_exts().contains(&"ccproj"));
    }

    #[test]
    fn with_type_replaces_only_known_extensions() {
        assert_eq!(
            with_type(Path::new("a/b.3dcadcraft"), FileType::Part),
            PathBuf::from("a/b.ccpart")
        );
        assert_eq!(
            with_type(Path::new("bracket v2.1"), FileType::Assembly),
            PathBuf::from("bracket v2.1.ccasm")
        );
        assert_eq!(FileType::Drawing.filter(), "Drawing (*.ccdrw)");
    }
}
