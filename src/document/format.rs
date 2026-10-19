//! On-disk format of a project.
//!
//! ## Layout
//!
//! The same layout is stored in two interchangeable forms:
//!
//! ```text
//! name.3dcadcraft            ← packed: a ZIP archive (default, one file)
//! name.3dcadcraft/           ← unpacked: a directory (git-friendly)
//! ├── mimetype             "application/vnd.3dcadcraft.project+zip" (stored, first)
//! ├── manifest.json        format version, project id/name, document index,
//! │                        library links
//! └── documents/
//!     ├── <uuid>.json      one file per document
//!     └── …
//! ```
//!
//! [`Project::load`] accepts either form, so a user can unpack a project into
//! a folder, keep it under version control (each document diffs on its own),
//! and pack it again at any time.
//!
//! ## Guarantees
//!
//! - **Atomic save** — data is written to a temporary sibling and then
//!   renamed over the target, so a crash or full disk never leaves a
//!   half-written project. The previous version is kept as `*.bak`
//!   (configurable via [`SaveOptions`]).
//! - **Deterministic** — saving the same project twice gives byte-identical
//!   files (fixed ZIP timestamps, stable ordering), so unchanged projects do
//!   not show up as modified in sync tools or VCS.
//! - **Exact numbers** — `f64` values round-trip bit-for-bit.
//! - **Versioned** — `manifest.json` carries `format_version`. Older versions
//!   are migrated on load; newer ones are rejected with a clear error instead
//!   of being misread. Unknown extra entries (e.g. thumbnails written by a
//!   newer build) are ignored.
//! - **Inspectable** — plain JSON inside a standard ZIP: any unzip tool or
//!   text editor can look inside, and the format can be documented and read
//!   by third parties (unlike proprietary binary CAD formats).

use std::fs;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use super::{
    Document, DocumentId, DocumentKind, LibraryLink, Project, ProjectError, ProjectId,
    ProjectResult,
};

/// File extension of 3DCadCraft projects (without the dot).
pub const FILE_EXTENSION: &str = "3dcadcraft";

/// Newest format version this build writes and reads.
pub const FORMAT_VERSION: u32 = 1;

/// Additive revision within [`FORMAT_VERSION`]: bumped whenever a build
/// writes something an older build cannot read (a new enum variant, a field
/// that changes the meaning of others). Older builds still open the file
/// when they can, and say "made by a newer 3DCadCraft" when they cannot,
/// instead of "invalid project file". New *optional* fields that old builds
/// may simply ignore need no bump.
///
/// - 1: fillet types (Variable, Chord, Full Round) and edge sets, sketch
///   pattern count equations (2026-10-19).
/// - 2: partial chamfer (`BlendFeature::partial`; an older build would make a
///   full chamfer of the edge) (2026-10-19).
/// - 3: Shell (`Feature::Shell`; an older build cannot read the file)
///   (2026-10-19).
/// - 4: Face Draft (`Feature::Draft`) (2026-10-19).
/// - 5: Rectangular / Circular Pattern and Mirror (`Feature::Pattern`)
///   (2026-10-19).
/// - 6: Hole and Thread (`Feature::Hole`, `Feature::Thread`) (2026-10-19).
/// - 8: Extrude / Revolve terminations at work / origin planes and points
///   (`TargetRef::Plane`, `TargetRef::Point`), Revolve To
///   (`RevolveExtent::To`); additive fields `alternate`, `extend_face`,
///   `terminator`, `open` (Match Shape), `minimum` (2026-10-19).
pub const FORMAT_REVISION: u32 = 8;

const FORMAT_MAGIC: &str = "3dcadcraft";
const MIMETYPE: &str = "application/vnd.3dcadcraft.project+zip";
const MIMETYPE_ENTRY: &str = "mimetype";
const MANIFEST_ENTRY: &str = "manifest.json";

/// Options for saving.
#[derive(Clone, Debug)]
pub struct SaveOptions {
    /// Keep the previously saved version as `<name>.3dcadcraft.bak`.
    pub backup: bool,
}

impl Default for SaveOptions {
    fn default() -> Self {
        Self { backup: true }
    }
}

/// `manifest.json`.
#[derive(Serialize, Deserialize)]
struct Manifest {
    /// Always `"3dcadcraft"`; identifies the file type.
    format: String,
    format_version: u32,
    /// [`FORMAT_REVISION`] of the build that wrote it (0 in older files).
    #[serde(default)]
    format_revision: u32,
    /// Which build wrote the file, for diagnostics.
    generator: String,
    project: ProjectHeader,
    documents: Vec<DocumentEntry>,
    libraries: Vec<LibraryLink>,
}

#[derive(Serialize, Deserialize)]
struct ProjectHeader {
    id: ProjectId,
    name: String,
}

/// Index entry: lets tools list a project's contents without parsing every
/// document.
#[derive(Serialize, Deserialize)]
struct DocumentEntry {
    id: DocumentId,
    name: String,
    kind: DocumentKind,
    file: String,
}

/// Read first, before the full manifest, so that a newer file is reported
/// as "unsupported version" rather than as a confusing parse error.
#[derive(Deserialize)]
struct VersionProbe {
    format: String,
    format_version: u32,
}

fn document_entry_name(id: DocumentId) -> String {
    format!("documents/{id}.json")
}

fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> ProjectError + '_ {
    move |source| ProjectError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn corrupt(path: &Path, reason: impl ToString) -> ProjectError {
    ProjectError::Corrupt {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    }
}

impl Project {
    /// The archive entries of this project, in write order.
    fn entries(&self, path: &Path) -> ProjectResult<Vec<(String, Vec<u8>)>> {
        let manifest = Manifest {
            format: FORMAT_MAGIC.into(),
            format_version: FORMAT_VERSION,
            format_revision: FORMAT_REVISION,
            generator: format!("3DCadCraft {}", env!("CARGO_PKG_VERSION")),
            project: ProjectHeader {
                id: self.id,
                name: self.name.clone(),
            },
            documents: self
                .documents
                .iter()
                .map(|d| DocumentEntry {
                    id: d.id,
                    name: d.name.clone(),
                    kind: d.data.kind(),
                    file: document_entry_name(d.id),
                })
                .collect(),
            libraries: self.libraries.clone(),
        };
        let mut entries = vec![
            (MIMETYPE_ENTRY.to_owned(), MIMETYPE.as_bytes().to_vec()),
            (MANIFEST_ENTRY.to_owned(), to_json(path, &manifest)?),
        ];
        for d in &self.documents {
            entries.push((document_entry_name(d.id), to_json(path, d)?));
        }
        Ok(entries)
    }

    /// Saves as a single packed `.3dcadcraft` file with default options.
    pub fn save(&self, path: &Path) -> ProjectResult<()> {
        self.save_with(path, &SaveOptions::default())
    }

    /// Saves as a single packed `.3dcadcraft` file (a ZIP archive).
    pub fn save_with(&self, path: &Path, opts: &SaveOptions) -> ProjectResult<()> {
        let started = std::time::Instant::now();
        let r = self.save_packed(path, opts);
        log_result("save", path, started, &r);
        r
    }

    fn save_packed(&self, path: &Path, opts: &SaveOptions) -> ProjectResult<()> {
        let entries = self.entries(path)?;
        let tmp = temp_sibling(path);
        let result = (|| {
            let file = fs::File::create(&tmp).map_err(io_err(&tmp))?;
            let mut zip = ZipWriter::new(BufWriter::new(file));
            // Fixed timestamp ⇒ deterministic bytes.
            let base = SimpleFileOptions::default().last_modified_time(DateTime::default());
            for (name, bytes) in &entries {
                // `mimetype` is stored uncompressed and first (as in ODF/EPUB) so
                // file-type sniffers can identify the archive from its header.
                let method = if name == MIMETYPE_ENTRY {
                    CompressionMethod::Stored
                } else {
                    CompressionMethod::Deflated
                };
                zip.start_file(name, base.compression_method(method))
                    .map_err(|e| corrupt(&tmp, e))?;
                zip.write_all(bytes).map_err(io_err(&tmp))?;
            }
            let writer = zip.finish().map_err(|e| corrupt(&tmp, e))?;
            let file = writer
                .into_inner()
                .map_err(|e| io_err(&tmp)(e.into_error()))?;
            file.sync_all().map_err(io_err(&tmp))
        })();
        if let Err(e) = result {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        commit(&tmp, path, opts.backup)
    }

    /// Saves in unpacked form: a directory with the same layout as the ZIP
    /// archive, convenient for version control.
    pub fn save_folder(&self, dir: &Path, opts: &SaveOptions) -> ProjectResult<()> {
        let entries = self.entries(dir)?;
        let tmp = temp_sibling(dir);
        let result = (|| {
            for (name, bytes) in &entries {
                let path = tmp.join(name);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(io_err(parent))?;
                }
                fs::write(&path, bytes).map_err(io_err(&path))?;
            }
            Ok(())
        })();
        if let Err(e) = result {
            let _ = fs::remove_dir_all(&tmp);
            return Err(e);
        }
        commit(&tmp, dir, opts.backup)
    }

    /// Loads a project from a packed `.3dcadcraft` file or an unpacked
    /// directory.
    pub fn load(path: &Path) -> ProjectResult<Project> {
        let started = std::time::Instant::now();
        let r = Self::load_any(path);
        log_result("open", path, started, &r);
        r
    }

    fn load_any(path: &Path) -> ProjectResult<Project> {
        let meta = fs::metadata(path).map_err(io_err(path))?;
        if meta.is_dir() {
            Self::load_with(path, |name| {
                let p = path.join(name);
                fs::read(&p).map_err(io_err(&p))
            })
        } else {
            let file = fs::File::open(path).map_err(io_err(path))?;
            let mut zip = ZipArchive::new(BufReader::new(file))
                .map_err(|e| corrupt(path, format!("not a 3DCadCraft archive ({e})")))?;
            Self::load_with(path, |name| {
                let mut entry = zip
                    .by_name(name)
                    .map_err(|e| corrupt(path, format!("{name}: {e}")))?;
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf).map_err(io_err(path))?;
                Ok(buf)
            })
        }
    }

    /// Shared loader: `read(entry_name)` returns the bytes of one entry.
    fn load_with(
        path: &Path,
        mut read: impl FnMut(&str) -> ProjectResult<Vec<u8>>,
    ) -> ProjectResult<Project> {
        let manifest_bytes = read(MANIFEST_ENTRY)?;

        let probe: VersionProbe =
            serde_json::from_slice(&manifest_bytes).map_err(|e| corrupt(path, e))?;
        if probe.format != FORMAT_MAGIC {
            return Err(corrupt(path, format!("unknown format '{}'", probe.format)));
        }
        if probe.format_version > FORMAT_VERSION {
            return Err(ProjectError::UnsupportedVersion {
                path: path.to_path_buf(),
                found: probe.format_version,
                supported: FORMAT_VERSION,
            });
        }
        // Migrations from older versions go here, keyed on `probe.format_version`.

        let manifest: Manifest =
            serde_json::from_slice(&manifest_bytes).map_err(|e| corrupt(path, e))?;
        let mut documents = Vec::with_capacity(manifest.documents.len());
        for entry in &manifest.documents {
            // Read only the canonical entry name derived from the UUID, never
            // a path taken from the file, so a crafted manifest cannot point
            // the reader outside the project ("zip slip").
            let bytes = read(&document_entry_name(entry.id))?;
            let doc: Document = serde_json::from_slice(&bytes).map_err(|e| {
                if manifest.format_revision > FORMAT_REVISION {
                    ProjectError::NewerRevision {
                        path: path.to_path_buf(),
                        generator: manifest.generator.clone(),
                        found: manifest.format_revision,
                        supported: FORMAT_REVISION,
                        reason: e.to_string(),
                    }
                } else {
                    corrupt(path, format!("document {}: {e}", entry.id))
                }
            })?;
            if doc.id != entry.id {
                return Err(corrupt(
                    path,
                    format!("document {} is stored under id {}", doc.id, entry.id),
                ));
            }
            documents.push(doc);
        }

        let project = Project {
            id: manifest.project.id,
            name: manifest.project.name,
            documents,
            libraries: manifest.libraries,
            newer_file: (manifest.format_revision > FORMAT_REVISION).then(|| {
                format!(
                    "made by a newer 3DCadCraft ({}, format revision {}; this build: {}): features added since may be missing or built differently, update the program",
                    manifest.generator, manifest.format_revision, FORMAT_REVISION
                )
            }),
        };
        project.validate().map_err(|e| corrupt(path, e))?;
        Ok(project)
    }
}

/// Logs the result of a save or open (`what`) of `path`.
fn log_result<T>(what: &str, path: &Path, started: std::time::Instant, r: &ProjectResult<T>) {
    let ms = started.elapsed().as_millis();
    match r {
        Ok(_) => log::info!("{what} {}: ok ({ms} ms)", path.display()),
        Err(e) => log::warn!("{what} {}: {e}", path.display()),
    }
}

/// A hidden temporary path next to `path` (same filesystem ⇒ atomic rename).
fn temp_sibling(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map_or_else(|| "project".into(), |n| n.to_string_lossy().into_owned());
    path.with_file_name(format!(".{name}.tmp-{}", uuid::Uuid::new_v4().simple()))
}

/// `<path>.bak`.
fn backup_path(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_owned();
    os.push(".bak");
    PathBuf::from(os)
}

fn remove_any(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Moves the freshly written `tmp` into place at `path`.
fn commit(tmp: &Path, path: &Path, backup: bool) -> ProjectResult<()> {
    let exists = fs::symlink_metadata(path).is_ok();
    let is_dir = fs::metadata(tmp).map(|m| m.is_dir()).unwrap_or(false);

    if exists && (backup || is_dir) {
        // Directories cannot be renamed over non-empty directories, so the
        // old version always moves aside first; for files only if backed up.
        let aside = if backup {
            backup_path(path)
        } else {
            temp_sibling(path)
        };
        remove_any(&aside).map_err(io_err(&aside))?;
        fs::rename(path, &aside).map_err(io_err(path))?;
        fs::rename(tmp, path).map_err(io_err(path))?;
        if !backup {
            remove_any(&aside).map_err(io_err(&aside))?;
        }
        Ok(())
    } else {
        // Atomic replace of a file (or first save).
        fs::rename(tmp, path).map_err(io_err(path))
    }
}

/// Pretty JSON: readable in unpacked projects and diffs line by line.
fn to_json<T: Serialize>(path: &Path, value: &T) -> ProjectResult<Vec<u8>> {
    serde_json::to_vec_pretty(value).map_err(|e| corrupt(path, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::tests::{assembly_of, part};
    use crate::document::{DocRef, DocumentData, PartDocument, PartSketch};
    use crate::kernel::{BRep, Point3};
    use crate::sketch::{Constraint, Point2, Sketch};

    fn sample_project() -> Project {
        let mut body = BRep::new();
        body.make_box(Point3::origin(), Point3::new(10.0, 20.0, 30.0))
            .unwrap();
        let mut sketch = Sketch::default();
        let line = sketch.add_line(Point2::new(0.1, 0.0), Point2::new(10.0, 0.3));
        sketch
            .add_constraint(Constraint::Horizontal { line })
            .unwrap();
        sketch.solve().unwrap();

        let mut p = Project::new("Gearbox");
        let housing = p.add_document(
            "Housing",
            DocumentData::Part(PartDocument {
                sketches: vec![PartSketch::new("Sketch1", sketch)],
                body,
                ..Default::default()
            }),
        );
        let bolt = p.add_document("Bolt M8", part());
        p.add_document(
            "Gearbox",
            assembly_of(&[
                DocRef::local(housing),
                DocRef::local(bolt),
                DocRef::local(bolt),
            ]),
        );
        p
    }

    fn as_json(p: &Project) -> serde_json::Value {
        serde_json::to_value(p).unwrap()
    }

    #[test]
    fn packed_and_unpacked_roundtrip_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let p = sample_project();

        let file = dir.path().join("gearbox.3dcadcraft");
        p.save(&file).unwrap();
        assert_eq!(as_json(&Project::load(&file).unwrap()), as_json(&p));

        let folder = dir.path().join("gearbox-src.3dcadcraft");
        p.save_folder(&folder, &SaveOptions::default()).unwrap();
        assert!(folder.join("manifest.json").is_file());
        assert_eq!(as_json(&Project::load(&folder).unwrap()), as_json(&p));
    }

    #[test]
    fn saving_is_deterministic_and_keeps_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("p.3dcadcraft");
        let mut p = sample_project();
        p.save(&file).unwrap();
        let first = fs::read(&file).unwrap();
        p.save(&file).unwrap();
        assert_eq!(fs::read(&file).unwrap(), first, "same project ⇒ same bytes");
        assert_eq!(fs::read(backup_path(&file)).unwrap(), first);

        p.name = "Renamed".into();
        p.save_with(&file, &SaveOptions { backup: false }).unwrap();
        assert_eq!(Project::load(&file).unwrap().name, "Renamed");
        // No temp files left behind.
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn folder_resave_replaces_contents() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("p.3dcadcraft");
        let mut p = sample_project();
        p.save_folder(&folder, &SaveOptions { backup: false })
            .unwrap();
        let removed = p.documents()[0].id;
        let asm = p.documents()[2].id;
        p.remove_document(asm).unwrap();
        p.remove_document(removed).unwrap();
        p.save_folder(&folder, &SaveOptions { backup: false })
            .unwrap();
        assert!(!folder.join(document_entry_name(removed)).exists());
        assert_eq!(Project::load(&folder).unwrap().documents().len(), 1);
    }

    #[test]
    fn newer_format_is_rejected_clearly() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("p.3dcadcraft");
        sample_project()
            .save_folder(&folder, &SaveOptions::default())
            .unwrap();
        let m = folder.join(MANIFEST_ENTRY);
        let text = fs::read_to_string(&m).unwrap().replace(
            &format!("\"format_version\": {FORMAT_VERSION}"),
            "\"format_version\": 999",
        );
        fs::write(&m, text).unwrap();
        assert!(matches!(
            Project::load(&folder),
            Err(ProjectError::UnsupportedVersion { found: 999, .. })
        ));
    }

    /// A file from a newer build (higher revision) whose document this
    /// build cannot read says so; an extra field it can ignore is fine.
    #[test]
    fn newer_revision_is_explained() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("p.3dcadcraft");
        sample_project()
            .save_folder(&folder, &SaveOptions::default())
            .unwrap();
        let m = folder.join(MANIFEST_ENTRY);
        let text = fs::read_to_string(&m).unwrap().replace(
            &format!("\"format_revision\": {FORMAT_REVISION}"),
            "\"format_revision\": 999",
        );
        fs::write(&m, text).unwrap();
        let docs: Vec<_> = fs::read_dir(folder.join("documents"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        // An unknown extra field: ignored, the project opens.
        let original = fs::read_to_string(&docs[0]).unwrap();
        let extra = original.replacen('{', "{\"from_the_future\": 1, ", 1);
        fs::write(&docs[0], &extra).unwrap();
        let p = Project::load(&folder).unwrap();
        assert!(
            p.newer_file
                .as_deref()
                .is_some_and(|m| m.contains("newer 3DCadCraft"))
        );
        // Something it cannot read: "made by a newer 3DCadCraft".
        fs::write(&docs[0], original.replacen("\"data\"", "\"data_v2\"", 1)).unwrap();
        let err = Project::load(&folder).unwrap_err();
        assert!(
            matches!(err, ProjectError::NewerRevision { found: 999, .. }),
            "{err}"
        );
        assert!(err.to_string().contains("newer 3DCadCraft"));
    }

    #[test]
    fn garbage_is_reported_as_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("bad.3dcadcraft");
        fs::write(&file, b"definitely not a zip").unwrap();
        assert!(matches!(
            Project::load(&file),
            Err(ProjectError::Corrupt { .. })
        ));
    }
}
