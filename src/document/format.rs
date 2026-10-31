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
//! │                        library links, the document of the preview picture
//! ├── thumbnail.png        preview picture (optional, `part::thumbnail`)
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
/// - 7: End of Part and suppressed features (`PartDocument::eop`,
///   `PartDocument::suppressed`; an older build would ignore both and build
///   every feature) (2026-10-19).
/// - 8: work feature methods on curved faces, curves and sketch geometry,
///   and UCS (`WorkDef::Ucs`, new `WorkPlaneDef` / `WorkAxisDef` /
///   `WorkPointDef` variants) (2026-10-19).
/// - 9: Face Draft Parting Line (`DraftType::PartingLine`), Symmetric /
///   Asymmetric extents, Move Parting Line; Shell Rounded Corners (an older
///   build would make a one-way or sharp result) (2026-10-19).
/// - 10: Hole drill points Flat W / Angle V / Angle Y, Symmetric, Extend To,
///   centres concentric to a cylindrical face (`HoleCenter::Axis`); sketch
///   Center Points (`PointStyle`, an old build reads them as plain points)
///   (2026-10-19).
/// - 11: Extrude / Revolve terminations at work / origin planes and points
///   (`TargetRef::Plane`, `TargetRef::Point`), Revolve To
///   (`RevolveExtent::To`); additive fields `alternate`, `extend_face`,
///   `terminator`, `open` (Match Shape), `minimum` (2026-10-19).
/// - 12: Sweep and Loft (`Feature::Sweep`, `Feature::Loft`) (2026-10-19).
/// - 13: Combine and Split (`Feature::Combine`, `Feature::Split`),
///   `PartDocument::hidden_bodies` (2026-10-20).
/// - 14: Coil and Rib / Web (`Feature::Coil`, `Feature::Rib`) (2026-10-20).
/// - 15: Revolve around a work axis (`AxisRef::Work`) (2026-10-23).
/// - 16: Project Geometry of work features (`LinkSource::Work`) (2026-10-23).
/// - 17: parameter units cm, m, in, ft, rad (`Unit::Cm` …) (2026-10-23).
/// - 18: true / false and text parameters (`Unit::Bool`, `Unit::Text`),
///   Suppress if conditions (`PartDocument::suppress_if`) (2026-10-23).
/// - 19: sheet metal: `Feature::SheetFace` / `SheetFlange`,
///   `PartDocument::sheet_metal` (2026-10-25).
/// - 20: Convert to Sheet Metal of a solid (`Feature::SheetConvert`), corner
///   relief shapes (`CornerRelief`), flange Width Extents (`FlangeWidth`)
///   (2026-10-25).
/// - 21: sheet metal Hem, Fold, Contour Flange, Unfold / Refold and Cut
///   (`Feature::SheetOp`) (2026-10-26).
/// - 22: Corner Seam (`SheetOp::CornerSeam`) (2026-10-26).
/// - 23: drawing documents (`DocumentData::Drawing`) (2026-10-26).
/// - 24: not used (was reserved for sheet metal; see 26).
/// - 25: parts lists and balloons in drawings (`AnnotationKind::PartsList`,
///   `AnnotationKind::Balloon`) (2026-10-26).
/// - 26: flat pattern views and bend notes (`DrawingView::sheet_metal`,
///   `AnnotationKind::BendNote`) (2026-10-26). Revision 24 stays unused: the
///   next new variant takes the next free number.
/// - 27: base views in any orientation (`ViewOrientation::Custom`: ViewCube,
///   Custom View) (2026-10-26).
/// - 28: bend tables of flat pattern views (`AnnotationKind::BendTable`)
///   (2026-10-26).
/// - 29: baseline / chain / ordinate dimension sets (`AnnotationKind::DimSet`)
///   (2026-10-27).
/// - 30: sheet metal Corner Round and Corner Chamfer
///   (`SheetOp::CornerRound` / `CornerChamfer`) (2026-10-27).
/// - 31: body operations (`Feature::BodyOp`: Thicken/Offset) (2026-10-27).
/// - 32: hole tables (`AnnotationKind::HoleTable`) (2026-10-27).
/// - 33: chamfer notes (`AnnotationKind::ChamferNote`) (2026-10-27).
/// - 34: Delete Face (`BodyOp::DeleteFace`) (2026-10-27).
/// - 35: Move Bodies (`BodyOp::MoveBodies`) (2026-10-27).
/// - 36: general tables, revision tables, tags and clouds in drawings
///   (`AnnotationKind::Table`, `RevisionTable`, `RevisionTag`,
///   `RevisionCloud`) (2026-10-28).
/// - 37: the surface commands (`BodyOp::Stitch`, `Patch`, `Trim`, `Extend`,
///   `Ruled`, `ReplaceFace`, `Sculpt`) (2026-10-28).
/// - 38: angular dimensions in drawings (`DimKind::Angle`) (2026-10-28).
/// - 39: section lines of several segments (`ViewKind::Section::mid`,
///   offset sections) (2026-10-28).
/// - 40: Copy Object (`BodyOp::CopyObject`) (2026-10-29).
/// - 41: loft sections of model edges (`LoftSection::Edges`) (2026-10-29).
/// - 42: Import of a STEP file (`BodyOp::Import`) (2026-10-29).
/// - 43: hem types (`SheetOp::Hem::hem_type`: Single / Teardrop / Rolled /
///   Double, radius, angle) (2026-10-29).
/// - 44: cross-part references (`PlaneRef::Context`,
///   `PartDocument::context`) (2026-10-29).
/// - 45: sheet metal Bend (`SheetOp::Bend`) (2026-10-29).
/// - 46: Convert to Sheet Metal finding an imported sheet's round bends
///   (`SheetConvertFeature::recognize`) (2026-10-29).
/// - 47: edges of other components projected into sketches
///   (`ContextGeom::Curve`, `LinkSource::Context`) (2026-10-29).
/// - 48: Unfold / Refold of chosen bends (`SheetOp::Unfold::bends`,
///   `stationary`, `SheetOp::Refold::bends`) (2026-10-29).
/// - 49: frame Notch (`assembly::frame::Treatment::Notch`, member shapes'
///   `NotchTool`s) (2026-10-30).
/// - 50: sheet metal Rip (`SheetOp::Rip`) (2026-10-30).
/// - 51: free-form (spline) edges of other components projected
///   (`ContextCurve::Spline`) (2026-10-30).
/// - 52: sheet metal Cut To / To Next and Direction (`CutExtent::To`,
///   `ToNext`, `SheetOp::Cut::direction`, `to`) (2026-10-30).
/// - 53: Flange Width Extents Between and Height Extents To
///   (`FlangeWidth::Between`, `SheetFlangeFeature::from` / `to` /
///   `height_to`) (2026-10-30).
/// - 54: Import of a mesh (STL, OBJ) as mesh bodies (`ImportOp` holding OBJ text)
///   (2026-10-31).
/// - 55: sheet metal Contour Roll (`SheetOp::ContourRoll`) (2026-10-31).
/// - 56: sheet metal Lofted Flange (`SheetOp::LoftedFlange`) (2026-10-31).
/// - 57: frame member bodies between two planes (`BodyOp::Prism`, mitred
///   members without a Trim Solid) (2026-10-31).
/// - 58: frame members between two points (`Skeleton::Between`: ends of
///   edges and sketch lines) (2026-10-31).
/// - 60: drawing Break Out To Sketch (`BreakOutDepth::ToSketch`) (2026-10-31).
pub const FORMAT_REVISION: u32 = 60;

const FORMAT_MAGIC: &str = "3dcadcraft";
const MIMETYPE: &str = "application/vnd.3dcadcraft.project+zip";
const MIMETYPE_ENTRY: &str = "mimetype";
const MANIFEST_ENTRY: &str = "manifest.json";
const THUMBNAIL_ENTRY: &str = "thumbnail.png";

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
    /// The document `thumbnail.png` shows (the active one at save). Older
    /// builds ignore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preview: Option<DocumentId>,
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
            preview: self
                .preview
                .filter(|id| self.thumbnail.is_some() && self.document(*id).is_some()),
        };
        let mut entries = vec![
            (MIMETYPE_ENTRY.to_owned(), MIMETYPE.as_bytes().to_vec()),
            (MANIFEST_ENTRY.to_owned(), to_json(path, &manifest)?),
        ];
        for d in &self.documents {
            entries.push((document_entry_name(d.id), to_json(path, d)?));
        }
        if let Some(png) = &self.thumbnail {
            entries.push((THUMBNAIL_ENTRY.to_owned(), png.clone()));
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
                // A PNG is compressed already.
                let method = if name == MIMETYPE_ENTRY || name == THUMBNAIL_ENTRY {
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
            let mut doc: Document = serde_json::from_slice(&bytes).map_err(|e| {
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
            // Occurrences of files before ids get theirs.
            match &mut doc.data {
                super::DocumentData::Assembly(a) => a.fix_ids(),
                super::DocumentData::Drawing(d) => d.fix(),
                super::DocumentData::Part(_) => {}
            }
            documents.push(doc);
        }

        let project = Project {
            id: manifest.project.id,
            name: manifest.project.name,
            documents,
            libraries: manifest.libraries,
            thumbnail: read(THUMBNAIL_ENTRY).ok(),
            preview: manifest.preview,
            linked_edits: Default::default(),
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

/// What the Open dialog shows of a project without loading it: the preview
/// picture, the previewed document's name, kind and iProperties, the
/// documents and the build that wrote it.
#[derive(Clone, Debug, Default)]
pub struct FilePreview {
    /// PNG bytes (`None` in files from older builds).
    pub thumbnail: Option<Vec<u8>>,
    /// The build that wrote the file.
    pub generator: String,
    /// Every document: (name, kind).
    pub documents: Vec<(String, DocumentKind)>,
    /// The previewed document (the active one at save, else the first).
    pub document: Option<(String, DocumentKind)>,
    /// Its iProperties.
    pub properties: super::iprops::IProperties,
}

/// Reads [`FilePreview`] of a packed or unpacked project: the manifest, the
/// picture and only the properties of one document.
pub fn read_preview(path: &Path) -> ProjectResult<FilePreview> {
    let meta = fs::metadata(path).map_err(io_err(path))?;
    if meta.is_dir() {
        preview_with(path, |name| fs::read(path.join(name)).ok())
    } else {
        let file = fs::File::open(path).map_err(io_err(path))?;
        let mut zip = ZipArchive::new(BufReader::new(file))
            .map_err(|e| corrupt(path, format!("not a 3DCadCraft archive ({e})")))?;
        preview_with(path, |name| {
            let mut entry = zip.by_name(name).ok()?;
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).ok()?;
            Some(buf)
        })
    }
}

fn preview_with(
    path: &Path,
    mut read: impl FnMut(&str) -> Option<Vec<u8>>,
) -> ProjectResult<FilePreview> {
    /// A document's name and properties; the model is skipped unparsed.
    #[derive(Deserialize)]
    struct Head {
        #[serde(default)]
        properties: super::iprops::IProperties,
    }
    let bytes = read(MANIFEST_ENTRY).ok_or_else(|| corrupt(path, "no manifest.json"))?;
    let probe: VersionProbe = serde_json::from_slice(&bytes).map_err(|e| corrupt(path, e))?;
    if probe.format != FORMAT_MAGIC {
        return Err(corrupt(path, format!("unknown format '{}'", probe.format)));
    }
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| corrupt(path, e))?;
    let entry = manifest
        .preview
        .and_then(|id| manifest.documents.iter().find(|d| d.id == id))
        .or_else(|| manifest.documents.first());
    let properties = entry
        // The canonical entry name, never a path from the file (as `load`).
        .and_then(|e| read(&document_entry_name(e.id)))
        .and_then(|b| serde_json::from_slice::<Head>(&b).ok())
        .map(|h| h.properties)
        .unwrap_or_default();
    Ok(FilePreview {
        thumbnail: read(THUMBNAIL_ENTRY),
        generator: manifest.generator,
        documents: manifest
            .documents
            .iter()
            .map(|d| (d.name.clone(), d.kind))
            .collect(),
        document: entry.map(|e| (e.name.clone(), e.kind)),
        properties,
    })
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
    fn preview_picture_and_properties_without_loading() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = sample_project();
        let bolt = p.documents()[1].id;
        p.document_mut(bolt)
            .unwrap()
            .properties
            .standard
            .insert("part_number".into(), "B-008".into());
        let png = vec![0x89, b'P', b'N', b'G', 1, 2, 3];
        p.thumbnail = Some(png.clone());
        p.preview = Some(bolt);
        let file = dir.path().join("t.3dcadcraft");
        let folder = dir.path().join("t-src.3dcadcraft");
        p.save(&file).unwrap();
        p.save_folder(&folder, &SaveOptions::default()).unwrap();
        assert!(folder.join("thumbnail.png").is_file());
        for path in [&file, &folder] {
            let v = read_preview(path).unwrap();
            assert_eq!(v.thumbnail.as_deref(), Some(png.as_slice()));
            assert_eq!(v.document, Some(("Bolt M8".into(), DocumentKind::Part)));
            assert_eq!(v.properties.get("part_number"), "B-008");
            assert_eq!(v.documents.len(), 3);
            // A load keeps the picture for the next save.
            let back = Project::load(path).unwrap();
            assert_eq!(back.thumbnail.as_deref(), Some(png.as_slice()));
            assert_eq!(back.preview, Some(bolt));
        }
        // Without a picture: no entry, no preview field, the first document.
        p.thumbnail = None;
        p.save(&file).unwrap();
        let v = read_preview(&file).unwrap();
        assert!(v.thumbnail.is_none());
        assert_eq!(v.document.map(|d| d.0), Some("Housing".into()));
        assert!(read_preview(&dir.path().join("missing.3dcadcraft")).is_err());
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
