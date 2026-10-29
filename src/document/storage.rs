//! Inside ↔ separate files (`docs/spec/0.6b-multi-document-files.md` §3.5):
//! a document of a design file moves into a file of its own and is linked
//! back (**Save to Separate File**), a linked document is copied into the
//! design file (**Embed**), every linked one at once (**Embed All**), every
//! document but one out (**Separate All**).
//!
//! A document keeps its [`DocumentId`] wherever it is stored, so assembly
//! constraints, drawing views and BOM overrides that name it stay valid, and
//! a document taken out and embedded again is the same document. Library
//! links store paths relative to the file holding them; they are rewritten
//! for the new file. A link that would point a file at itself becomes a
//! local reference again.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::filetype::FileType;
use super::library::{canonical, relative_path};
use super::{
    DocRef, Document, DocumentId, LibraryId, Project, ProjectError, ProjectResult, Resolver,
};

/// The folder of a saved file (its canonical form), or `NotSaved`.
fn folder_of(path: &Path) -> ProjectResult<PathBuf> {
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !dir.is_dir() {
        return Err(ProjectError::NotSaved);
    }
    canonical(dir)
}

/// The file a document is saved to by Save to Separate File:
/// `<folder>/<name>.<kind ext>`, characters Windows refuses in names
/// replaced by `_`.
pub fn separate_file_name(folder: &Path, doc: &Document) -> PathBuf {
    let clean: String = doc
        .name
        .trim()
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let stem = if clean.is_empty() { "Document" } else { &clean };
    folder.join(format!(
        "{stem}.{}",
        FileType::for_kind(doc.data.kind()).ext()
    ))
}

/// The library link of `project` (saved in `from_dir`) to the file
/// `abs` holding project `id`: an existing link to the same project is
/// reused (`add_library`).
fn link_to(
    project: &mut Project,
    from_dir: &Path,
    abs: &Path,
    id: super::ProjectId,
    name: &str,
) -> LibraryId {
    project.add_library(relative_path(from_dir, abs), id, name.to_owned())
}

/// **Save to Separate File**: writes document `id` of `project` (saved at
/// `owner_path`) to `target` as a single-document file and links it back;
/// every reference in `project` follows. The document's own references to
/// documents still inside `project` become links to `owner_path`.
///
/// Refused for the file's last document and an unsaved `project`. An
/// existing `target` is replaced only with `replace` (the user confirmed),
/// when it is a file of the document's kind that `project` does not link.
/// Returns the new link.
pub fn extract(
    project: &mut Project,
    owner_path: &Path,
    id: DocumentId,
    target: &Path,
    replace: bool,
) -> ProjectResult<LibraryId> {
    if project.documents.len() <= 1 {
        return Err(ProjectError::Refused(
            "a file keeps at least one document".into(),
        ));
    }
    let owner_dir = folder_of(owner_path)?;
    let owner_abs = owner_dir.join(owner_path.file_name().unwrap_or_default());
    let doc = project
        .document(id)
        .cloned()
        .ok_or(ProjectError::UnknownDocument(id))?;
    if target.exists() {
        if !replace {
            return Err(ProjectError::Refused(format!(
                "{} already exists",
                target.display()
            )));
        }
        if FileType::from_path(target) != Some(FileType::for_kind(doc.data.kind())) {
            return Err(ProjectError::Refused(format!(
                "{} is another kind of file",
                target.display()
            )));
        }
        let t = canonical(target)?;
        let linked = project
            .libraries
            .iter()
            .any(|l| canonical(&owner_dir.join(&l.path)).is_ok_and(|p| p == t));
        if linked {
            return Err(ProjectError::Refused(format!(
                "{} is linked by this file",
                target.display()
            )));
        }
    }
    let target_dir = folder_of(target)?;

    // The new file: the document with its references made links.
    let stem = target
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| doc.name.clone());
    let mut file = Project::new(stem);
    let mut map: HashMap<DocRef, DocRef> = HashMap::new();
    for r in doc.data.references() {
        let lib = match r.library {
            None => link_to(
                &mut file,
                &target_dir,
                &owner_abs,
                project.id,
                &project.name,
            ),
            Some(lib) => {
                let l = project
                    .library(lib)
                    .ok_or(ProjectError::UnknownLibrary(lib))?
                    .clone();
                let abs = owner_dir.join(&l.path);
                link_to(&mut file, &target_dir, &abs, l.project, &l.name)
            }
        };
        map.insert(
            r,
            DocRef {
                library: Some(lib),
                document: r.document,
            },
        );
    }
    let mut moved = doc.clone();
    moved
        .data
        .remap_references(|r| map.get(&r).copied().unwrap_or(r));
    file.preview = Some(id);
    file.documents.push(moved);
    file.save(target)?;

    // This file: links instead of the document.
    let target_abs = target_dir.join(target.file_name().unwrap_or_default());
    let lib = link_to(project, &owner_dir, &target_abs, file.id, &file.name);
    let to = DocRef {
        library: Some(lib),
        document: id,
    };
    for d in &mut project.documents {
        d.data
            .remap_references(|r| if r == DocRef::local(id) { to } else { r });
    }
    project.documents.retain(|d| d.id != id);
    if project.preview == Some(id) {
        project.preview = None;
    }
    Ok(lib)
}

/// **Embed**: copies document `doc` of the file linked by `lib` into
/// `project` (saved at `owner_path`) and points every reference at the
/// copy; the link goes when nothing uses it. The copy keeps the id (a
/// fresh one only when `project` already holds that id). Its references
/// to its own file become links to that file; a link back to `project`
/// becomes local. The other file is not changed.
pub fn embed(
    project: &mut Project,
    owner_path: &Path,
    resolver: &mut Resolver,
    lib: LibraryId,
    doc: DocumentId,
) -> ProjectResult<DocumentId> {
    let owner_dir = folder_of(owner_path)?;
    let link = project
        .library(lib)
        .ok_or(ProjectError::UnknownLibrary(lib))?
        .clone();
    let (source_path, source) = resolver.load_library(owner_path, &link)?;
    let source_dir = source_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let original = source
        .document(doc)
        .cloned()
        .ok_or(ProjectError::UnknownDocument(doc))?;

    let mut map: HashMap<DocRef, DocRef> = HashMap::new();
    for r in original.data.references() {
        let to = match r.library {
            None => DocRef {
                library: Some(lib),
                document: r.document,
            },
            Some(l) => {
                let sl = source
                    .library(l)
                    .ok_or(ProjectError::UnknownLibrary(l))?
                    .clone();
                if sl.project == project.id {
                    DocRef::local(r.document)
                } else {
                    let abs = source_dir.join(&sl.path);
                    let new = link_to(project, &owner_dir, &abs, sl.project, &sl.name);
                    DocRef {
                        library: Some(new),
                        document: r.document,
                    }
                }
            }
        };
        map.insert(r, to);
    }
    let new_id = if project.document(doc).is_some() {
        DocumentId::new_random()
    } else {
        doc
    };
    let mut copy = original;
    copy.id = new_id;
    copy.data
        .remap_references(|r| map.get(&r).copied().unwrap_or(r));
    let from = DocRef {
        library: Some(lib),
        document: doc,
    };
    for d in &mut project.documents {
        d.data
            .remap_references(|r| if r == from { DocRef::local(new_id) } else { r });
    }
    project.documents.push(copy);
    // Only this link goes, and only when nothing uses it any more: a file
    // linked by Link File stays linked without users.
    let used = project
        .documents
        .iter()
        .flat_map(|d| d.data.references())
        .any(|r| r.library == Some(lib));
    if !used {
        project.libraries.retain(|l| l.id != lib);
    }
    Ok(new_id)
}

/// **Link File**: links the file at `path` from `project` (saved at
/// `owner_path`) without placing anything; its documents can then be
/// placed and shown (spec 0.6b §3.2). Refuses the file itself.
pub fn link_file(
    project: &mut Project,
    owner_path: &Path,
    resolver: &mut Resolver,
    path: &Path,
) -> ProjectResult<LibraryId> {
    let owner_dir = folder_of(owner_path)?;
    let other = resolver.load(path)?;
    if other.id == project.id {
        return Err(ProjectError::Refused("a file cannot link itself".into()));
    }
    let abs = canonical(path)?;
    Ok(link_to(project, &owner_dir, &abs, other.id, &other.name))
}

/// **Insert Document**: copies every document of the file at `path` into
/// `project` (saved at `owner_path`, or not saved yet). References between
/// them follow the copies; their links to other files are rewritten for
/// this file (a link back to `project` becomes local). Ids are kept when
/// free; a taken name gets ` (2)`, ` (3)`…. Returns the new documents.
pub fn insert_file(
    project: &mut Project,
    owner_path: Option<&Path>,
    resolver: &mut Resolver,
    path: &Path,
) -> ProjectResult<Vec<DocumentId>> {
    let source = resolver.load(path)?;
    if source.id == project.id {
        return Err(ProjectError::Refused("a file cannot insert itself".into()));
    }
    let source_dir = canonical(path)?
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let owner_dir = match owner_path {
        Some(p) => Some(folder_of(p)?),
        None => None,
    };
    // New ids first, so references between the copies can follow them.
    let ids: HashMap<DocumentId, DocumentId> = source
        .documents
        .iter()
        .map(|d| {
            let new = if project.document(d.id).is_some() {
                DocumentId::new_random()
            } else {
                d.id
            };
            (d.id, new)
        })
        .collect();
    let mut libs: HashMap<LibraryId, Option<LibraryId>> = HashMap::new();
    for l in &source.libraries {
        let to = if l.project == project.id {
            None
        } else {
            let abs = source_dir.join(&l.path);
            let path = match &owner_dir {
                Some(dir) => relative_path(dir, &abs),
                None => abs.to_string_lossy().replace('\\', "/"),
            };
            Some(project.add_library(path, l.project, l.name.clone()))
        };
        libs.insert(l.id, to);
    }
    let mut added = Vec::new();
    for d in &source.documents {
        let mut copy = d.clone();
        copy.id = ids.get(&d.id).copied().unwrap_or(d.id);
        if project.name_taken(&copy.name, None) {
            let base = copy.name.clone();
            copy.name = (2..)
                .map(|k| format!("{base} ({k})"))
                .find(|n| !project.name_taken(n, None))
                .unwrap_or(base);
        }
        copy.data.remap_references(|r| match r.library {
            None => DocRef::local(ids.get(&r.document).copied().unwrap_or(r.document)),
            Some(l) => match libs.get(&l) {
                Some(Some(to)) => DocRef {
                    library: Some(*to),
                    document: r.document,
                },
                Some(None) => DocRef::local(r.document),
                None => r,
            },
        });
        added.push(copy.id);
        project.documents.push(copy);
    }
    Ok(added)
}

/// **Embed All** (Pack): embeds every linked document, then what those
/// bring along, until `project` links nothing. Returns how many documents
/// came in.
pub fn embed_all(
    project: &mut Project,
    owner_path: &Path,
    resolver: &mut Resolver,
) -> ProjectResult<usize> {
    let mut done: HashSet<(LibraryId, DocumentId)> = HashSet::new();
    let mut n = 0;
    loop {
        let next = project
            .documents
            .iter()
            .flat_map(|d| d.data.references())
            .find_map(|r| r.library.map(|l| (l, r.document)))
            .filter(|k| !done.contains(k));
        let Some((lib, doc)) = next else {
            return Ok(n);
        };
        done.insert((lib, doc));
        embed(project, owner_path, resolver, lib, doc)?;
        n += 1;
    }
}

/// **Separate All** (Unpack): every document but `keep` to its own file in
/// the design file's folder (an existing name gets ` (2)`, ` (3)`…).
/// Documents whose references are all outside the file go first, so
/// assemblies and drawings link the new files rather than this one.
/// Returns the files written.
pub fn separate_all(
    project: &mut Project,
    owner_path: &Path,
    keep: DocumentId,
) -> ProjectResult<Vec<PathBuf>> {
    let keep = [keep];
    let folder = folder_of(owner_path)?;
    let mut written = Vec::new();
    loop {
        let left: Vec<DocumentId> = project
            .documents
            .iter()
            .map(|d| d.id)
            .filter(|d| !keep.contains(d))
            .collect();
        if left.is_empty() {
            return Ok(written);
        }
        let ready = |d: &DocumentId| {
            project.document(*d).is_some_and(|x| {
                x.data
                    .references()
                    .iter()
                    .all(|r| r.library.is_some() || keep.contains(&r.document))
            })
        };
        // A leaf first; with none (a loop) any of them.
        let Some(id) = left.iter().copied().find(ready).or(left.first().copied()) else {
            return Ok(written);
        };
        let doc = project
            .document(id)
            .cloned()
            .ok_or(ProjectError::UnknownDocument(id))?;
        let target = free_path(&separate_file_name(&folder, &doc));
        extract(project, owner_path, id, &target, false)?;
        written.push(target);
    }
}

/// **Save As** to a single-document file (`docs/spec/0.6a-file-types-projects.md`
/// §3.1, H2): `path`'s type must be `keep`'s kind (`.ccpart` for a part…);
/// every other document of `project` goes to a file of its own next to it
/// first (Separate All), so the saved file holds `keep` alone and links the
/// others — Inventor's one file per document (subassemblies too: their
/// parts are linked from their own files). A package path
/// (`.3dcadcraft`) saves everything as before. Returns the other files
/// written. On an error before anything was written, `project` is unchanged.
pub fn save_as_kind(
    project: &mut Project,
    path: &Path,
    keep: DocumentId,
) -> ProjectResult<Vec<PathBuf>> {
    let kind = project
        .document(keep)
        .map(|d| d.data.kind())
        .ok_or(ProjectError::UnknownDocument(keep))?;
    match FileType::from_path(path) {
        None | Some(FileType::Package) => {
            project.save(path)?;
            return Ok(Vec::new());
        }
        Some(t) if t.kind() != Some(kind) => {
            return Err(ProjectError::Refused(format!(
                "a {} file holds one {}; this document is a {}",
                t.filter(),
                t.name().to_lowercase(),
                FileType::for_kind(kind).name().to_lowercase()
            )));
        }
        Some(_) => {}
    }
    // The file must exist before the others can link back to it.
    project.save(path)?;
    let written = separate_all(project, path, keep)?;
    project.save(path)?;
    Ok(written)
}

/// `path`, or `name (2).ext`, `name (3).ext`… when it exists.
fn free_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = path.parent().unwrap_or(Path::new("."));
    (2..)
        .map(|k| dir.join(format!("{stem} ({k}).{ext}")))
        .find(|p| !p.exists())
        .unwrap_or_else(|| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{AssemblyDocument, DocumentData, Occurrence};
    use crate::kernel::Transform3;
    use crate::part::PartDocument;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cadcraft-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A saved design file: Part1 (a box), Assembly1 placing it twice.
    fn design(dir: &Path) -> (Project, PathBuf, DocumentId, DocumentId) {
        let mut p = Project::new("Design");
        let part = p.add_new_document(
            "Part",
            DocumentData::Part(PartDocument::with_box("Box1", [10.0, 20.0, 30.0]).unwrap()),
        );
        let mut asm = AssemblyDocument::default();
        for k in 1..=2 {
            asm.occurrences.push(Occurrence::new(
                format!("Part1:{k}"),
                DocRef::local(part),
                Transform3::identity(),
            ));
        }
        let a = p.add_new_document("Assembly", DocumentData::Assembly(asm));
        let path = dir.join("Design.3dcadcraft");
        p.save(&path).unwrap();
        (p, path, part, a)
    }

    fn refs(p: &Project, id: DocumentId) -> Vec<DocRef> {
        p.document(id).unwrap().data.references()
    }

    #[test]
    fn extract_writes_a_part_file_and_links_it() {
        let d = tmp("extract");
        let (mut p, path, part, a) = design(&d);
        let target = separate_file_name(&d, p.document(part).unwrap());
        assert_eq!(target.file_name().unwrap(), "Part1.ccpart");
        let lib = extract(&mut p, &path, part, &target, false).unwrap();
        assert!(p.document(part).is_none());
        assert!(
            refs(&p, a)
                .iter()
                .all(|r| r.library == Some(lib) && r.document == part)
        );
        p.validate().unwrap();
        // The file holds the same document (same id) and nothing else.
        let f = Project::load(&target).unwrap();
        assert_eq!(f.documents().len(), 1);
        assert_eq!(f.documents()[0].id, part);
        // Saved and reloaded, the assembly finds the part through the link.
        p.save(&path).unwrap();
        let back = Project::load(&path).unwrap();
        let mut res = Resolver::new();
        let doc = res.resolve(&path, &back, refs(&back, a)[0]).unwrap();
        assert_eq!(doc.id, part);
        // The last document stays; an existing file is not overwritten.
        assert!(extract(&mut p, &path, a, &target, true).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn replace_takes_an_unlinked_file_of_the_same_kind_only() {
        let d = tmp("replace");
        let (mut p, path, part, a) = design(&d);
        let old = d.join("Old.ccpart");
        std::fs::write(&old, b"x").unwrap();
        let asm_file = d.join("Old.ccasm");
        std::fs::write(&asm_file, b"x").unwrap();
        // Another kind is refused even when confirmed.
        assert!(extract(&mut p, &path, part, &asm_file, true).is_err());
        let lib = extract(&mut p, &path, part, &old, true).unwrap();
        assert_eq!(Project::load(&old).unwrap().documents()[0].id, part);
        p.save(&path).unwrap();
        // Now linked: replacing it with the assembly's copy is refused.
        let mut res = Resolver::new();
        embed(&mut p, &path, &mut res, lib, part).unwrap();
        let mut top = p.clone();
        let lib2 = link_file(&mut top, &path, &mut res, &old).unwrap();
        assert!(top.library(lib2).is_some());
        assert!(extract(&mut top, &path, part, &old, true).is_err());
        assert!(p.document(a).is_some());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_extracted_assembly_links_back_and_embed_brings_it_home() {
        let d = tmp("roundtrip");
        let (mut p, path, part, a) = design(&d);
        let target = d.join("Assembly1.ccasm");
        let lib = extract(&mut p, &path, a, &target, false).unwrap();
        p.save(&path).unwrap();
        // The assembly file links the part in the design file.
        let f = Project::load(&target).unwrap();
        let r = f.documents()[0].data.references();
        assert!(r.iter().all(|r| r.library.is_some() && r.document == part));
        assert_eq!(f.libraries()[0].path, "Design.3dcadcraft");
        // Nothing references the extracted assembly: keep a link by hand
        // through a second assembly placing it.
        let mut top = AssemblyDocument::default();
        top.occurrences.push(Occurrence::new(
            "Assembly1:1",
            DocRef {
                library: Some(lib),
                document: a,
            },
            Transform3::identity(),
        ));
        let t = p.add_new_document("Assembly", DocumentData::Assembly(top));
        let mut res = Resolver::new();
        let back = embed(&mut p, &path, &mut res, lib, a).unwrap();
        assert_eq!(back, a, "the id is kept");
        // Its link back to this file became local again; no links remain.
        assert!(refs(&p, a).iter().all(|r| *r == DocRef::local(part)));
        assert_eq!(refs(&p, t), vec![DocRef::local(a)]);
        assert!(p.libraries().is_empty());
        p.validate().unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn link_file_keeps_a_link_and_insert_copies_every_document() {
        let d = tmp("link");
        let (src, src_path, part, a) = design(&d);
        let mut p = Project::new("Other");
        p.add_new_document("Part", DocumentData::Part(PartDocument::default()));
        let path = d.join("Other.3dcadcraft");
        p.save(&path).unwrap();
        let mut res = Resolver::new();
        let lib = link_file(&mut p, &path, &mut res, &src_path).unwrap();
        assert_eq!(p.libraries().len(), 1);
        assert_eq!(p.libraries()[0].id, lib);
        assert!(link_file(&mut p, &path, &mut res, &path).is_err());
        // Insert copies both documents; Part1 is taken here: "Part1 (2)".
        let added = insert_file(&mut p, Some(&path), &mut res, &src_path).unwrap();
        assert_eq!(added, vec![part, a], "ids kept when free");
        assert_eq!(p.document(part).unwrap().name, "Part1 (2)");
        assert_eq!(refs(&p, a), vec![DocRef::local(part); 2]);
        p.validate().unwrap();
        // Again: fresh ids, the assembly follows its own copy.
        let again = insert_file(&mut p, Some(&path), &mut res, &src_path).unwrap();
        assert!(again.iter().all(|id| *id != part && *id != a));
        assert_eq!(refs(&p, again[1]), vec![DocRef::local(again[0]); 2]);
        assert_eq!(src.documents().len(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn save_as_an_assembly_file_moves_its_parts_out() {
        let d = tmp("save_as_kind");
        let (mut p, _, part, a) = design(&d);
        // The wrong kind is refused and nothing is written.
        let wrong = d.join("Top.ccpart");
        assert!(save_as_kind(&mut p, &wrong, a).is_err());
        assert!(!wrong.exists() && p.documents.len() == 2);
        let path = d.join("Top.ccasm");
        let written = save_as_kind(&mut p, &path, a).unwrap();
        assert_eq!(written, vec![d.join("Part1.ccpart")]);
        assert_eq!(p.documents.len(), 1);
        // The assembly links the part file; both open on their own.
        let again = Project::load(&path).unwrap();
        assert_eq!(again.documents.len(), 1);
        assert!(
            refs(&again, a)
                .iter()
                .all(|r| r.library.is_some() && r.document == part)
        );
        let file = Project::load(&written[0]).unwrap();
        assert!(matches!(file.documents[0].data, DocumentData::Part(_)));
        let mut res = Resolver::new();
        assert!(res.resolve(&path, &again, refs(&again, a)[0]).is_ok());
    }

    #[test]
    fn subassemblies_go_to_files_of_their_own_too() {
        let d = tmp("save_as_sub");
        let (mut p, _, part, a) = design(&d);
        let mut top = AssemblyDocument::default();
        top.occurrences.push(Occurrence::new(
            "Assembly1:1".to_owned(),
            DocRef::local(a),
            Transform3::identity(),
        ));
        let t = p.add_new_document("Top", DocumentData::Assembly(top));
        let path = d.join("Top1.ccasm");
        let written = save_as_kind(&mut p, &path, t).unwrap();
        assert_eq!(written.len(), 2, "{written:?}");
        let ids: Vec<DocumentId> = p.documents.iter().map(|x| x.id).collect();
        assert_eq!(ids, vec![t]);
        // Top links the subassembly's file, which links the part's file.
        let sub = refs(&p, t);
        assert!(sub.len() == 1 && sub[0].library.is_some() && sub[0].document == a);
        let mut res = crate::document::Resolver::new();
        let link = p.library(sub[0].library.unwrap()).unwrap().clone();
        let (sub_path, sub_file) = res.load_library(&path, &link).unwrap();
        assert_eq!(sub_file.documents().len(), 1);
        let inner = refs(&sub_file, a);
        assert!(!inner.is_empty() && inner.iter().all(|r| r.library.is_some()));
        let part_link = sub_file.library(inner[0].library.unwrap()).unwrap().clone();
        let (_, part_file) = res.load_library(&sub_path, &part_link).unwrap();
        assert!(part_file.document(part).is_some());
    }

    #[test]
    fn separate_all_then_embed_all_round_trips() {
        let d = tmp("unpack");
        let (mut p, path, part, a) = design(&d);
        let keep = p.add_new_document("Part", DocumentData::Part(PartDocument::default()));
        // A file in the way gets " (2)".
        std::fs::write(d.join("Part1.ccpart"), b"x").unwrap();
        let files = separate_all(&mut p, &path, keep).unwrap();
        let names: Vec<_> = files
            .iter()
            .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["Part1 (2).ccpart", "Assembly1.ccasm"]);
        assert_eq!(p.documents().len(), 1);
        p.save(&path).unwrap();
        // The assembly file links the part file, not the design file.
        let asm = Project::load(&d.join("Assembly1.ccasm")).unwrap();
        assert_eq!(asm.libraries()[0].path, "Part1 (2).ccpart");
        // Link the assembly from the kept file and pack everything back.
        let mut res = Resolver::new();
        let r = res
            .link(&mut p, &path, &d.join("Assembly1.ccasm"), a)
            .unwrap();
        let mut top = AssemblyDocument::default();
        top.occurrences
            .push(Occurrence::new("Assembly1:1", r, Transform3::identity()));
        p.add_new_document("Assembly", DocumentData::Assembly(top));
        let n = embed_all(&mut p, &path, &mut res).unwrap();
        assert_eq!(n, 2);
        assert!(p.libraries().is_empty());
        assert!(p.document(a).is_some() && p.document(part).is_some());
        p.validate().unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }
}
