//! Cross-project reuse: **Link**, **Append** and **Pack**.
//!
//! A [`Resolver`] loads linked library projects on demand (each file once)
//! and implements the three reuse operations described in [`super`].
//!
//! ## Finding a linked library
//!
//! A [`LibraryLink`] stores the library's path *relative to the linking
//! project's directory*. Resolution tries, in order:
//!
//! 1. `<project dir>/<stored relative path>` — the normal case; survives
//!    moving or copying the whole folder tree;
//! 2. `<project dir>/<library file name>` — the library was copied next to
//!    the project (e.g. both e-mailed to someone);
//!
//! and in both cases checks that the file found is the *same project*
//! (by [`super::ProjectId`]), so a different file that merely has the same name is
//! never silently used.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use super::{DocRef, Document, DocumentId, LibraryLink, Project, ProjectError, ProjectResult};

/// Loads and caches library projects, and performs link/append/pack.
#[derive(Default)]
pub struct Resolver {
    /// Keyed by canonical path, so one file is loaded once however it is
    /// reached.
    cache: HashMap<PathBuf, Rc<Project>>,
}

/// A document's origin while copying: (canonical file, project).
type Source = (PathBuf, Rc<Project>);

impl Resolver {
    /// An empty resolver.
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets cached projects (call after library files changed on disk).
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Loads a project file (or returns it from the cache).
    pub fn load(&mut self, path: &Path) -> ProjectResult<Rc<Project>> {
        let key = canonical(path)?;
        if let Some(p) = self.cache.get(&key) {
            return Ok(p.clone());
        }
        let p = Rc::new(Project::load(&key)?);
        self.cache.insert(key, p.clone());
        Ok(p)
    }

    /// Loads the library behind `link` as seen from the project at
    /// `owner_path`, using the search order in the module docs.
    pub fn load_library(&mut self, owner_path: &Path, link: &LibraryLink) -> ProjectResult<Source> {
        let dir = owner_path.parent().unwrap_or(Path::new("."));
        let stored = dir.join(&link.path);
        let flat = Path::new(&link.path).file_name().map(|n| dir.join(n));
        let path = std::iter::once(stored.clone())
            .chain(flat)
            .find(|p| p.exists())
            .ok_or(ProjectError::MissingLibrary { path: stored })?;

        let project = self.load(&path)?;
        if project.id != link.project {
            return Err(ProjectError::LibraryMismatch {
                path,
                expected: link.project,
                found: project.id,
            });
        }
        Ok((canonical(&path)?, project))
    }

    /// Returns (a copy of) the document `r` refers to, as seen from the
    /// project `owner` saved at `owner_path`.
    pub fn resolve(
        &mut self,
        owner_path: &Path,
        owner: &Project,
        r: DocRef,
    ) -> ProjectResult<Document> {
        match r.library {
            None => owner.document(r.document).cloned(),
            Some(lib) => {
                let link = owner
                    .library(lib)
                    .ok_or(ProjectError::UnknownLibrary(lib))?;
                let (_, project) = self.load_library(owner_path, link)?;
                project.document(r.document).cloned()
            }
        }
        .ok_or(ProjectError::UnknownDocument(r.document))
    }

    /// **Link**: references document `doc` of the library at `library_path`
    /// from `target`, which is (or will be) saved at `target_path`.
    ///
    /// Returns the [`DocRef`] to put into an assembly occurrence. The
    /// document is not copied; changes to the library show up in `target`.
    pub fn link(
        &mut self,
        target: &mut Project,
        target_path: &Path,
        library_path: &Path,
        doc: DocumentId,
    ) -> ProjectResult<DocRef> {
        let target_dir = target_path
            .parent()
            .filter(|d| d.as_os_str().is_empty() || d.is_dir())
            .ok_or(ProjectError::NotSaved)?;
        let target_dir = canonical(if target_dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            target_dir
        })?;

        let library = self.load(library_path)?;
        if library.id == target.id {
            return Err(ProjectError::Cycle(doc)); // a project cannot be its own library
        }
        library
            .document(doc)
            .ok_or(ProjectError::UnknownDocument(doc))?;

        let rel = relative_path(&target_dir, &canonical(library_path)?);
        let id = target.add_library(rel, library.id, library.name.clone());
        Ok(DocRef {
            library: Some(id),
            document: doc,
        })
    }

    /// **Append**: copies document `doc` of the project at `source_path`
    /// into `target`, together with everything it depends on (local or from
    /// the source's own libraries). All copies get fresh IDs and only local
    /// references, so the result is independent of the source.
    ///
    /// A component used several times is copied once.
    pub fn append(
        &mut self,
        target: &mut Project,
        source_path: &Path,
        doc: DocumentId,
    ) -> ProjectResult<DocumentId> {
        let source = (canonical(source_path)?, self.load(source_path)?);
        self.copy_into(target, &source, doc, &mut HashMap::new(), &mut Vec::new())
    }

    /// **Pack**: replaces every library reference in `target` (saved at
    /// `target_path`) by a local copy and removes the library links, making
    /// the project self-contained. Returns how many documents were copied in.
    pub fn pack(&mut self, target: &mut Project, target_path: &Path) -> ProjectResult<usize> {
        let before = target.documents().len();
        let mut memo = HashMap::new();
        let own_ids: Vec<DocumentId> = target.documents().iter().map(|d| d.id).collect();

        for id in own_ids {
            let refs: Vec<DocRef> = target
                .document(id)
                .map(|d| d.data.references())
                .unwrap_or_default()
                .into_iter()
                .filter(|r| r.library.is_some())
                .collect();
            let mut mapping = HashMap::new();
            for r in refs {
                let Some(lib) = r.library else { continue };
                let link = target
                    .library(lib)
                    .ok_or(ProjectError::UnknownLibrary(lib))?
                    .clone();
                let source = self.load_library(target_path, &link)?;
                let new_id =
                    self.copy_into(target, &source, r.document, &mut memo, &mut Vec::new())?;
                mapping.insert(r, new_id);
            }
            if let Some(d) = target.document_mut(id) {
                d.data
                    .remap_references(|r| mapping.get(&r).map_or(r, |&n| DocRef::local(n)));
            }
        }
        target.prune_libraries();
        Ok(target.documents().len() - before)
    }

    /// Deep-copies `doc` from `source` into `target`; dependencies first.
    /// `memo` maps (file, source id) → copied id so shared components are
    /// copied once; `stack` detects reference cycles.
    fn copy_into(
        &mut self,
        target: &mut Project,
        source: &Source,
        doc: DocumentId,
        memo: &mut HashMap<(PathBuf, DocumentId), DocumentId>,
        stack: &mut Vec<(PathBuf, DocumentId)>,
    ) -> ProjectResult<DocumentId> {
        let key = (source.0.clone(), doc);
        if let Some(&id) = memo.get(&key) {
            return Ok(id);
        }
        if stack.contains(&key) {
            return Err(ProjectError::Cycle(doc));
        }
        stack.push(key.clone());

        let original = source
            .1
            .document(doc)
            .ok_or(ProjectError::UnknownDocument(doc))?
            .clone();
        let mut mapping = HashMap::new();
        for r in original.data.references() {
            let dep_source = match r.library {
                None => source.clone(),
                Some(lib) => {
                    let link = source
                        .1
                        .library(lib)
                        .ok_or(ProjectError::UnknownLibrary(lib))?
                        .clone();
                    self.load_library(&source.0, &link)?
                }
            };
            let new_id = self.copy_into(target, &dep_source, r.document, memo, stack)?;
            mapping.insert(r, new_id);
        }

        let mut data = original.data;
        data.remap_references(|r| DocRef::local(mapping[&r]));
        let new_id = target.add_document(original.name, data);

        stack.pop();
        memo.insert(key, new_id);
        Ok(new_id)
    }
}

pub(super) fn canonical(path: &Path) -> ProjectResult<PathBuf> {
    path.canonicalize()
        .map(without_verbatim)
        .map_err(|source| match source.kind() {
            std::io::ErrorKind::NotFound => ProjectError::MissingLibrary {
                path: path.to_path_buf(),
            },
            _ => ProjectError::Io {
                path: path.to_path_buf(),
                source,
            },
        })
}

/// `path` without the Windows verbatim prefix that `canonicalize` adds
/// (`\\?\C:\x` → `C:\x`, `\\?\UNC\srv\share` → `\\srv\share`): the plain
/// form is what users read, and a verbatim path written with `/` (library
/// paths across drives) no longer parses as one.
pub(crate) fn without_verbatim(path: PathBuf) -> PathBuf {
    let Some(s) = path.to_str() else {
        return path;
    };
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match s.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
    }
}

/// Relative path from directory `from` to file `to` (both canonical), with
/// `/` separators. Falls back to the absolute path when no relative path
/// exists (different drives on Windows).
pub(super) fn relative_path(from: &Path, to: &Path) -> String {
    let (f, t): (Vec<Component>, Vec<Component>) =
        (from.components().collect(), to.components().collect());
    if f.first() != t.first() {
        return to.to_string_lossy().replace('\\', "/");
    }
    let common = f.iter().zip(&t).take_while(|(a, b)| a == b).count();
    let parts: Vec<String> = std::iter::repeat_n("..".to_owned(), f.len() - common)
        .chain(
            t[common..]
                .iter()
                .map(|c| c.as_os_str().to_string_lossy().into_owned()),
        )
        .collect();
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::tests::{assembly_of, part};
    use crate::document::{DocumentData, DocumentKind};
    use std::fs;

    /// `fasteners.3dcadcraft` with a Bolt and a Nut, saved under `dir/lib/`.
    fn make_library(dir: &Path) -> (PathBuf, DocumentId, DocumentId) {
        let mut lib = Project::new("Fasteners");
        let bolt = lib.add_document("Bolt M8", part());
        let nut = lib.add_document("Nut M8", part());
        fs::create_dir_all(dir.join("lib")).unwrap();
        let path = dir.join("lib/fasteners.3dcadcraft");
        lib.save(&path).unwrap();
        (path, bolt, nut)
    }

    #[test]
    fn verbatim_prefix_is_dropped() {
        assert_eq!(
            without_verbatim(PathBuf::from(r"\\?\C:\Users\a\x.3dcadcraft")),
            PathBuf::from(r"C:\Users\a\x.3dcadcraft")
        );
        assert_eq!(
            without_verbatim(PathBuf::from(r"\\?\UNC\srv\share\x")),
            PathBuf::from(r"\\srv\share\x")
        );
        // Other verbatim forms (devices, volume GUIDs) and plain paths stay.
        for p in [r"\\?\Volume{1}\x", "/home/a/x", r"D:\x"] {
            assert_eq!(without_verbatim(PathBuf::from(p)), PathBuf::from(p));
        }
    }

    /// Canonical paths on Windows carry no `\\?\`, so a library on another
    /// drive is stored as `D:/…`, which parses back to the same file.
    #[cfg(windows)]
    #[test]
    fn canonical_paths_are_plain_on_windows() {
        let dir = tempfile::tempdir().unwrap();
        let c = canonical(dir.path()).unwrap();
        assert!(!c.to_string_lossy().starts_with(r"\\?\"), "{}", c.display());
        assert!(c.is_dir());
        let other = relative_path(Path::new(r"C:\a"), Path::new(r"D:\lib\x.3dcadcraft"));
        assert_eq!(other, "D:/lib/x.3dcadcraft");
        assert_eq!(
            Path::new(r"C:\a").join(&other),
            PathBuf::from(r"D:\lib\x.3dcadcraft")
        );
    }

    #[test]
    fn relative_paths() {
        assert_eq!(
            relative_path(Path::new("/a/b/proj"), Path::new("/a/b/lib/x.3dcadcraft")),
            "../lib/x.3dcadcraft"
        );
        assert_eq!(
            relative_path(Path::new("/a/b"), Path::new("/a/b/x.3dcadcraft")),
            "x.3dcadcraft"
        );
    }

    #[test]
    fn linked_parts_resolve_and_survive_moving_the_tree() {
        let dir = tempfile::tempdir().unwrap();
        let (lib_path, bolt, _) = make_library(dir.path());
        fs::create_dir_all(dir.path().join("proj")).unwrap();
        let main_path = dir.path().join("proj/machine.3dcadcraft");

        let mut r = Resolver::new();
        let mut main = Project::new("Machine");
        let bolt_ref = r.link(&mut main, &main_path, &lib_path, bolt).unwrap();
        main.add_document("Machine", assembly_of(&[bolt_ref, bolt_ref]));
        assert_eq!(main.libraries()[0].path, "../lib/fasteners.3dcadcraft");
        main.save(&main_path).unwrap();

        // Move the whole tree: relative links still work.
        let moved = dir.path().join("moved");
        fs::rename(dir.path().join("proj"), {
            fs::create_dir_all(&moved).unwrap();
            moved.join("proj")
        })
        .unwrap();
        fs::rename(dir.path().join("lib"), moved.join("lib")).unwrap();
        let main_path = moved.join("proj/machine.3dcadcraft");
        let main = Project::load(&main_path).unwrap();
        let mut r = Resolver::new();
        assert_eq!(
            r.resolve(&main_path, &main, bolt_ref).unwrap().name,
            "Bolt M8"
        );

        // Library copied flat next to the project: found by file name.
        fs::rename(
            moved.join("lib/fasteners.3dcadcraft"),
            moved.join("proj/fasteners.3dcadcraft"),
        )
        .unwrap();
        let mut r = Resolver::new();
        assert_eq!(
            r.resolve(&main_path, &main, bolt_ref).unwrap().name,
            "Bolt M8"
        );

        // A different project with the same file name is refused.
        Project::new("Impostor")
            .save(&moved.join("proj/fasteners.3dcadcraft"))
            .unwrap();
        let mut r = Resolver::new();
        assert!(matches!(
            r.resolve(&main_path, &main, bolt_ref),
            Err(ProjectError::LibraryMismatch { .. })
        ));

        // Library gone entirely.
        fs::remove_file(moved.join("proj/fasteners.3dcadcraft")).unwrap();
        let mut r = Resolver::new();
        assert!(matches!(
            r.resolve(&main_path, &main, bolt_ref),
            Err(ProjectError::MissingLibrary { .. })
        ));
    }

    #[test]
    fn pack_makes_project_self_contained() {
        let dir = tempfile::tempdir().unwrap();
        let (lib_path, bolt, nut) = make_library(dir.path());
        let main_path = dir.path().join("machine.3dcadcraft");

        let mut r = Resolver::new();
        let mut main = Project::new("Machine");
        let b = r.link(&mut main, &main_path, &lib_path, bolt).unwrap();
        let n = r.link(&mut main, &main_path, &lib_path, nut).unwrap();
        assert_eq!(main.libraries().len(), 1, "one link per library file");
        let asm = main.add_document("Machine", assembly_of(&[b, b, n]));

        let copied = r.pack(&mut main, &main_path).unwrap();
        assert_eq!(copied, 2, "bolt copied once although placed twice");
        assert!(main.libraries().is_empty());
        let refs = main.document(asm).unwrap().data.references();
        assert!(refs.iter().all(|r| r.library.is_none()));
        assert_eq!(refs[0], refs[1]);

        // Delete the library: the packed project is complete on its own.
        main.save(&main_path).unwrap();
        fs::remove_file(&lib_path).unwrap();
        let reloaded = Project::load(&main_path).unwrap();
        assert_eq!(reloaded.documents().len(), 3);
    }

    #[test]
    fn append_copies_dependencies_with_fresh_ids() {
        let dir = tempfile::tempdir().unwrap();
        let (lib_path, bolt, _) = make_library(dir.path());

        // A sub-assembly project that links the bolt from the library.
        let sub_path = dir.path().join("clamp.3dcadcraft");
        let mut r = Resolver::new();
        let mut sub = Project::new("Clamp");
        let jaw = sub.add_document("Jaw", part());
        let b = r.link(&mut sub, &sub_path, &lib_path, bolt).unwrap();
        let clamp = sub.add_document("Clamp", assembly_of(&[DocRef::local(jaw), b, b]));
        sub.save(&sub_path).unwrap();

        let mut target = Project::new("Machine");
        let copy = r.append(&mut target, &sub_path, clamp).unwrap();
        assert_ne!(copy, clamp, "copies get fresh IDs");
        assert!(
            target.libraries().is_empty(),
            "append never leaves links behind"
        );
        let kinds: Vec<_> = target.documents().iter().map(|d| d.data.kind()).collect();
        assert_eq!(
            kinds,
            [
                DocumentKind::Part,
                DocumentKind::Part,
                DocumentKind::Assembly
            ]
        );
        target.validate().unwrap();
        assert!(matches!(
            target.document(copy).unwrap().data,
            DocumentData::Assembly(_)
        ));
    }
}
