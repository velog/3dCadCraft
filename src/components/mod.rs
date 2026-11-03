//! The Component Library (`docs/spec/6.3-content-center.md`), GUI-free.
//!
//! A library is a folder: `Samples/` holds one small family per kind of
//! component (stage CC-S), `User/` the families published from parts with a
//! table (`part::family`). A member is an ordinary part file, written the
//! first time it is used; assemblies link it like any part.

pub mod catalog;
pub mod fit;
pub mod samples;

use std::path::{Path, PathBuf};

use crate::document::iprops::IProperties;
use crate::document::{DocumentData, DocumentId, Project, ProjectId};
use crate::part::PartDocument;

/// The library's folder in the user's settings (`CADCRAFT3D_LIBRARY_DIR`
/// redirects it for scripted runs and tests).
pub fn root_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("CADCRAFT3D_LIBRARY_DIR") {
        return Some(PathBuf::from(d));
    }
    crate::materials::user_dir().and_then(|d| d.parent().map(|p| p.join("library")))
}

/// FNV-1a over `kind` and `name`: stable between builds and platforms.
fn derived_id(kind: &str, name: &str) -> uuid::Uuid {
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013B;
    let mut h: u128 = 0x6C62_272E_07BB_0142_62B8_2175_6295_C58D;
    for b in kind.bytes().chain([0]).chain(name.bytes()) {
        h = (h ^ u128::from(b)).wrapping_mul(PRIME);
    }
    uuid::Uuid::from_u128(h)
}

/// Saves `part` as a one-part file whose ids follow from `identity`: a file
/// written again (a library deleted, another computer) is the same file for
/// every assembly that links it (spec 6.3 §6.2).
fn write_part(
    path: &Path,
    identity: &str,
    part: PartDocument,
    properties: IProperties,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let shown = identity.rsplit('|').next().unwrap_or(identity);
    let mut project = Project::new(shown);
    project.id = ProjectId(derived_id("project", identity));
    let doc = project.add_document(shown, DocumentData::Part(part));
    if let Some(d) = project.document_mut(doc) {
        d.id = DocumentId(derived_id("document", identity));
        d.properties = properties;
    }
    project.save(path).map_err(|e| e.to_string())
}
