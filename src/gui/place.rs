//! Place Component (`docs/spec/3.1-assemblies.md` §3.1), as Inventor: the
//! assembly is a file of its own and Place picks a part *file* in a
//! browser like Explorer, with a shaded preview of the part. The part is
//! **linked** (`document::library` Link: a path relative to the assembly,
//! the part stays its own file): saving the part file updates the
//! assembly. Occurrences are named after the file (`bracket.3dcadcraft` →
//! `bracket:1`).
//!
//! Also here: the cache of linked part files ([`LibCache`], reloaded when a
//! file changes on disk) and the see-through image of the part on the
//! cursor while placing.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense};

use super::CadCraftApp;
use super::camera::OrbitCamera;
use super::icons::Icon;
use crate::assembly::{self as asm, PickMesh};
use crate::document::{DocRef, DocumentData, DocumentId, LibraryId, Project};
use crate::kernel::{Point3, Real, Transform3, Vector3};
use crate::part::PartDocument;

/// One loaded part file.
struct LibEntry {
    project: Rc<Project>,
    modified: Option<SystemTime>,
    checked: Instant,
}

/// Linked part files, by canonical path: loaded (and regenerated) once,
/// reloaded when the file's time changes.
#[derive(Default)]
pub(super) struct LibCache {
    files: RefCell<HashMap<PathBuf, LibEntry>>,
    /// Where the library links of the loaded files lead (a linked
    /// assembly's own links, relative to its file), by id.
    links: RefCell<HashMap<LibraryId, PathBuf>>,
    /// The linked files the active file reaches at any depth, for the tree
    /// walk: (file path, its library ids, the projects). Built again when
    /// either changes or a file is (re)loaded.
    linked: RefCell<Option<LinkedKey>>,
}

/// The cached linked files: (file path, its library ids, the projects).
type LinkedKey = (String, Vec<LibraryId>, Rc<asm::tree::Linked>);

/// Where `link` of the file at `owner` leads (by its relative path, or by
/// its name in the owner's folder).
fn link_path(owner: &Path, link: &crate::document::LibraryLink) -> Option<PathBuf> {
    let dir = owner.parent()?;
    let stored = dir.join(&link.path);
    if stored.exists() {
        return Some(stored);
    }
    let flat = dir.join(Path::new(&link.path).file_name()?);
    flat.exists().then_some(flat)
}

/// The open Place Component window.
pub(super) struct PlaceDialog {
    dir: PathBuf,
    entries: Vec<(String, bool)>,
    error: Option<String>,
    /// The file clicked in the list.
    selected: Option<PathBuf>,
    /// Its parts (id, name).
    parts: Vec<(DocumentId, String)>,
    /// The part to place.
    part: Option<DocumentId>,
    preview: Option<Arc<PickMesh>>,
    /// The preview's turn (drag it).
    yaw: f32,
    /// The parts stored in this design file (spec 0.6b §3.3 *In This
    /// File*), listed above the file browser.
    local: Vec<(DocRef, String)>,
    /// The part of this file picked in that list.
    local_pick: Option<DocRef>,
    /// The assembly is saved, so other files can be linked (by a path
    /// relative to it).
    can_link: bool,
}

impl PlaceDialog {
    fn new(dir: PathBuf) -> Self {
        let mut d = Self {
            dir,
            entries: Vec::new(),
            error: None,
            selected: None,
            parts: Vec::new(),
            part: None,
            preview: None,
            yaw: 0.6,
            local: Vec::new(),
            local_pick: None,
            can_link: true,
        };
        d.refresh();
        d
    }

    fn refresh(&mut self) {
        let exts = [crate::document::FILE_EXTENSION, "ccpart", "ccasm"];
        match super::files::list_dir(&self.dir, &exts) {
            Ok(v) => {
                self.entries = v;
                self.error = None;
            }
            Err(e) => {
                self.entries.clear();
                self.error = Some(format!("{}: {e}", self.dir.display()));
            }
        }
    }

    fn go(&mut self, dir: PathBuf) {
        self.dir = dir;
        self.selected = None;
        self.parts.clear();
        self.part = None;
        self.preview = None;
        self.refresh();
    }
}

/// The parts of `project`: (id, name).
fn parts_of(project: &Project) -> Vec<(DocumentId, String)> {
    asm::placeable(project)
}

/// What a drawing's Base View can link from a file: its parts, then its
/// assemblies.
fn models_of(project: &Project) -> Vec<(DocumentId, String)> {
    let mut v = asm::placeable(project);
    v.extend(
        project
            .documents()
            .iter()
            .filter(|d| matches!(d.data, DocumentData::Assembly(_)))
            .map(|d| (d.id, format!("{} (assembly)", d.name))),
    );
    v
}

/// The occurrence name of part `doc` of the file `path`: the file's name
/// (its part's name too when the file holds several).
fn base_name(path: &Path, parts: &[(DocumentId, String)], doc: DocumentId) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Part")
        .to_owned();
    match parts.iter().find(|(d, _)| *d == doc) {
        Some((_, name)) if parts.len() > 1 => format!("{stem}-{name}"),
        _ => stem,
    }
}

impl LibCache {
    /// The project of the open tab of file `path` stands for the file (the
    /// assembly shows a part as edited in its tab, saved or not); the file's
    /// next save on disk replaces it as usual.
    pub(super) fn take_open(&self, path: &Path, project: Rc<Project>) {
        let Ok(key) = path.canonicalize() else {
            return;
        };
        let modified = std::fs::metadata(&key).and_then(|m| m.modified()).ok();
        self.files.borrow_mut().insert(
            key,
            LibEntry {
                project,
                modified,
                checked: Instant::now(),
            },
        );
        *self.linked.borrow_mut() = None;
    }

    /// Drops the cached linked projects (`CadCraftApp::linked`): this
    /// file's edits of linked files changed.
    pub(super) fn forget_linked(&self) {
        *self.linked.borrow_mut() = None;
    }

    /// Forgets file `path` (its tab closed: it is read from disk again).
    pub(super) fn forget(&self, path: &Path) {
        if let Ok(key) = path.canonicalize()
            && self.files.borrow_mut().remove(&key).is_some()
        {
            *self.linked.borrow_mut() = None;
        }
    }
}

impl CadCraftApp {
    /// Where the file of library link `lib` is (next to the assembly by its
    /// relative path, or by its name in the assembly's folder); a link of a
    /// loaded linked file (a subassembly's parts) is found next to that file.
    pub(super) fn library_path(&self, lib: LibraryId) -> Option<PathBuf> {
        match self.project.library(lib) {
            Some(link) => link_path(Path::new(self.file_path.trim()), link),
            None => self.asm.libs.links.borrow().get(&lib).cloned(),
        }
    }

    /// The linked files library links `ids` reach at any depth (loaded).
    fn linked_from(&self, ids: Vec<LibraryId>) -> asm::tree::Linked {
        let mut out = asm::tree::Linked::new();
        let mut queue = ids;
        while let Some(id) = queue.pop() {
            if out.contains(id) {
                continue;
            }
            // A linked file changed from this one: as edited.
            let edited = self
                .project
                .linked_edits
                .get(&id)
                .map(|p| Rc::new((**p).clone()));
            let Some(p) = edited.or_else(|| {
                self.library_path(id)
                    .and_then(|path| self.part_file(&path).ok())
            }) else {
                continue;
            };
            queue.extend(p.libraries().iter().map(|l| l.id));
            out.insert(id, p);
        }
        out
    }

    /// What Place offers of file `p`: its parts, and in an assembly its
    /// assemblies too (a subassembly of a linked file, spec 3.1a) unless one
    /// contains the active assembly; a drawing's Base View takes both.
    fn file_models(&self, p: &Project) -> Vec<(DocumentId, String)> {
        if self.is_drawing() {
            return models_of(p);
        }
        if self.asm.place_for_replace {
            return parts_of(p);
        }
        // Derive takes parts and assemblies of a file (spec 3.10 §3.5).
        if self.asm.place_for_derive {
            return models_of(p);
        }
        let linked = self.linked_from(p.libraries().iter().map(|l| l.id).collect());
        models_of(p)
            .into_iter()
            .filter(|(id, _)| {
                asm::tree::assembly_of(p, &linked, DocRef::local(*id)).is_none()
                    || !asm::tree::would_loop(p, &linked, self.part_id, DocRef::local(*id))
            })
            .collect()
    }

    /// A file assembly's parts as one picking mesh (the Place preview).
    fn file_assembly_mesh(&self, p: &Project, doc: DocumentId) -> Option<Arc<PickMesh>> {
        let linked = self.linked_from(p.libraries().iter().map(|l| l.id).collect());
        let a = asm::tree::assembly_of(p, &linked, DocRef::local(doc))?;
        let parts: Vec<(PickMesh, Transform3)> =
            asm::tree::leaves(p, &linked, DocRef::local(doc), a)
                .iter()
                .filter_map(|l| {
                    asm::tree::part_of(p, &linked, l.component).map(|x| (PickMesh::of(x), l.world))
                })
                .collect();
        Some(Arc::new(PickMesh::merged(
            parts.iter().map(|(m, t)| (m, *t)),
        )))
    }

    /// The projects of every linked file the active file reaches (linked
    /// subassemblies and their parts), for `assembly::tree` (cached).
    pub(super) fn linked(&self) -> Rc<asm::tree::Linked> {
        let ids: Vec<LibraryId> = self.project.libraries().iter().map(|l| l.id).collect();
        if let Some((f, i, l)) = &*self.asm.libs.linked.borrow()
            && *f == self.file_path
            && *i == ids
        {
            return l.clone();
        }
        let out = Rc::new(self.linked_from(ids.clone()));
        *self.asm.libs.linked.borrow_mut() = Some((self.file_path.clone(), ids, out.clone()));
        out
    }

    /// Part `doc` of the file linked as `lib` becomes `part` in this file's
    /// edits of linked files (`Project::linked_edits`: not saved until this
    /// file is, undone with it). No Undo step (the caller's).
    pub(super) fn edit_linked_part(&mut self, lib: LibraryId, doc: DocumentId, part: PartDocument) {
        let base = match self.project.linked_edits.get(&lib) {
            Some(p) => Some((**p).clone()),
            None => self
                .library_path(lib)
                .and_then(|path| self.part_file(&path).ok())
                .map(|p| (*p).clone()),
        };
        let Some(mut project) = base else {
            return;
        };
        if let Some(d) = project.document_mut(doc) {
            d.data = DocumentData::Part(part);
        }
        self.project
            .linked_edits
            .insert(lib, std::sync::Arc::new(project));
        *self.asm.libs.linked.borrow_mut() = None;
        self.scene_dirty = true;
    }

    /// Writes this file's edits of linked files into those files (after this
    /// file is saved): the message, or why one could not be written (it stays
    /// an edit then).
    pub(super) fn save_linked_edits(&mut self) -> Result<Vec<String>, String> {
        let mut saved = Vec::new();
        let edits = std::mem::take(&mut self.project.linked_edits);
        let mut failed = None;
        for (lib, p) in edits {
            let Some(path) = self.library_path(lib) else {
                failed = Some("a linked file was not found".to_owned());
                self.project.linked_edits.insert(lib, p);
                continue;
            };
            // The file open in a tab: that tab shows the saved file after
            // this — unless it has changes of its own, then nothing is
            // written (they would be lost either way).
            let key = path.canonicalize().ok();
            let same = |f: &str| {
                key.as_ref().is_some_and(|k| {
                    std::path::Path::new(f.trim())
                        .canonicalize()
                        .is_ok_and(|c| c == *k)
                })
            };
            if self
                .tabs
                .iter()
                .flatten()
                .any(|t| same(&t.file_path) && t.dirty)
            {
                failed = Some(format!(
                    "{} is open with changes of its own: save or close it there first",
                    path.display()
                ));
                self.project.linked_edits.insert(lib, p);
                continue;
            }
            match p.save(&path) {
                Ok(()) => {
                    for t in self.tabs.iter_mut().flatten() {
                        if same(&t.file_path) {
                            t.project = (*p).clone();
                            t.saved_sig = super::files::signature_of(&t.project);
                            t.history.clear();
                        }
                    }
                    if let Ok(key) = path.canonicalize() {
                        let modified = std::fs::metadata(&key).and_then(|m| m.modified()).ok();
                        self.asm.libs.files.borrow_mut().insert(
                            key,
                            LibEntry {
                                project: Rc::new((*p).clone()),
                                modified,
                                checked: Instant::now(),
                            },
                        );
                    }
                    saved.push(path.display().to_string());
                }
                Err(e) => {
                    failed = Some(format!("{}: {e}", path.display()));
                    self.project.linked_edits.insert(lib, p);
                }
            }
        }
        *self.asm.libs.linked.borrow_mut() = None;
        match failed {
            Some(e) => Err(e),
            None => Ok(saved),
        }
    }

    /// Writes the iProperties of document `doc` of the part file `path`:
    /// into its tab when it is open (unsaved there, as any edit), else into
    /// the file itself. The assembly's copy is updated at once. Returns the
    /// message and the project as it is now.
    pub(super) fn set_linked_props(
        &mut self,
        path: &Path,
        doc: DocumentId,
        props: crate::document::iprops::IProperties,
    ) -> Result<(String, Rc<Project>), String> {
        let key = path.canonicalize().map_err(|e| e.to_string())?;
        let same = |p: &str| {
            !p.trim().is_empty() && Path::new(p.trim()).canonicalize().is_ok_and(|c| c == key)
        };
        let mut done = None;
        for t in self.tabs.iter_mut().flatten() {
            if same(&t.file_path) {
                let d = t
                    .project
                    .document_mut(doc)
                    .ok_or("the part is not in its file")?;
                d.properties = props.clone();
                t.dirty = true;
                done = Some(Rc::new(t.project.clone()));
            }
        }
        let msg;
        let project = match done {
            Some(p) => {
                msg = format!(
                    "iProperties changed in the open {} (save it there)",
                    key.display()
                );
                p
            }
            None => {
                let mut p = Project::load(&key).map_err(|e| e.to_string())?;
                let d = p.document_mut(doc).ok_or("the part is not in its file")?;
                d.properties = props;
                p.save(&key).map_err(|e| e.to_string())?;
                msg = format!("iProperties saved into {}", key.display());
                Rc::new(load_regenerated(&key)?)
            }
        };
        let modified = std::fs::metadata(&key).and_then(|m| m.modified()).ok();
        self.asm.libs.files.borrow_mut().insert(
            key,
            LibEntry {
                project: project.clone(),
                modified,
                checked: Instant::now(),
            },
        );
        Ok((msg, project))
    }

    /// [`Self::part_file`] for the iProperties window.
    pub(super) fn part_file_pub(&self, path: &Path) -> Result<Rc<Project>, String> {
        self.part_file(path)
    }

    /// The project in the part file `path` (cached, its parts regenerated).
    fn part_file(&self, path: &Path) -> Result<Rc<Project>, String> {
        let key = path.canonicalize().map_err(|e| e.to_string())?;
        if let Some(e) = self.asm.libs.files.borrow().get(&key) {
            return Ok(e.project.clone());
        }
        let project = Rc::new(load_regenerated(&key)?);
        let modified = std::fs::metadata(&key).and_then(|m| m.modified()).ok();
        {
            // Shown and stored paths never carry canonicalize's verbatim prefix.
            let owner = crate::document::library::without_verbatim(key.clone());
            let mut links = self.asm.libs.links.borrow_mut();
            for l in project.libraries() {
                if let Some(p) = link_path(&owner, l) {
                    links.entry(l.id).or_insert(p);
                }
            }
        }
        *self.asm.libs.linked.borrow_mut() = None;
        self.asm.libs.files.borrow_mut().insert(
            key,
            LibEntry {
                project: project.clone(),
                modified,
                checked: Instant::now(),
            },
        );
        Ok(project)
    }

    /// Reloads linked part files saved since they were loaded (checked at
    /// most once a second); `true` when one changed.
    pub(super) fn poll_libraries(&self) -> bool {
        let mut changed = false;
        let mut files = self.asm.libs.files.borrow_mut();
        for (path, e) in files.iter_mut() {
            if e.checked.elapsed().as_secs_f32() < 1.0 {
                continue;
            }
            e.checked = Instant::now();
            let now = std::fs::metadata(path).and_then(|m| m.modified()).ok();
            if now != e.modified
                && let Ok(p) = load_regenerated(path)
            {
                e.project = Rc::new(p);
                e.modified = now;
                changed = true;
            }
        }
        if changed {
            *self.asm.libs.linked.borrow_mut() = None;
        }
        changed
    }

    /// Runs `f` on the part a component reference shows: a part of this
    /// file, or of a linked part file. `None` when it cannot be found.
    pub(super) fn with_part<R>(&self, r: DocRef, f: impl FnOnce(&PartDocument) -> R) -> Option<R> {
        match r.library {
            None => asm::part_of(&self.project, r).map(f),
            // Changed from this file and not saved yet.
            Some(lib) if self.project.linked_edits.contains_key(&lib) => {
                match self.project.linked_edits[&lib]
                    .document(r.document)
                    .map(|d| &d.data)
                {
                    Some(DocumentData::Part(p)) => Some(f(p)),
                    _ => None,
                }
            }
            Some(lib) => {
                let path = self.library_path(lib)?;
                let project = self.part_file(&path).ok()?;
                match project.document(r.document).map(|d| &d.data) {
                    Some(DocumentData::Part(p)) => Some(f(p)),
                    _ => None,
                }
            }
        }
    }

    /// Assemble › Place: the file browser (the assembly must be saved
    /// first: the link is a path relative to it).
    pub(super) fn open_place_dialog(&mut self) {
        // An assembly lists the parts of its own file first; they need no
        // saved file (spec 0.6b §3.3).
        let local = if self.is_drawing() {
            Vec::new()
        } else {
            // Derive: never the part itself.
            let me = DocRef::local(self.part_id);
            let derive = self.asm.place_for_derive;
            let mut v: Vec<(DocRef, String)> = self
                .placeable_here()
                .into_iter()
                .filter(|(r, _)| !(derive && *r == me))
                .collect();
            // Derive Assembly: the assemblies of this file not holding it.
            if derive {
                for d in self.project.documents() {
                    let r = DocRef::local(d.id);
                    if matches!(d.data, DocumentData::Assembly(_)) && !self.derive_loops(r, 0) {
                        v.push((r, format!("{} (assembly)", d.name)));
                    }
                }
            }
            v
        };
        let saved = !self.file_path.trim().is_empty();
        if !saved && !local.is_empty() {
            let dir = super::files::home_dir().unwrap_or_else(|| PathBuf::from("."));
            let mut d = PlaceDialog::new(dir);
            d.can_link = false;
            d.local_pick = local.first().map(|x| x.0);
            d.local = local;
            self.asm.place = Some(d);
            self.place_preview();
            return;
        }
        if !saved {
            let what = if self.is_drawing() {
                "Base View: save the drawing first"
            } else {
                "Place: save the assembly first"
            };
            self.file_status = Some(Ok(format!(
                "{what} (part files are linked by a path relative to it)"
            )));
            self.after_save = Some(super::files::Pending::Place);
            self.open_dialog(super::files::DialogPurpose::SaveAs);
            return;
        }
        let dir = Path::new(self.file_path.trim())
            .parent()
            .map(Path::to_path_buf)
            .or_else(super::files::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        let mut d = PlaceDialog::new(dir);
        d.local_pick = local.first().map(|x| x.0);
        d.local = local;
        self.asm.place = Some(d);
        self.place_preview();
    }

    /// What Place offers above the file browser: the parts of this file,
    /// then the parts of the files it links (Link File, earlier places).
    fn placeable_here(&self) -> Vec<(DocRef, String)> {
        let mut v: Vec<(DocRef, String)> = asm::placeable(&self.project)
            .into_iter()
            .map(|(id, name)| (DocRef::local(id), name))
            .collect();
        // Assemblies of this file as subassemblies (spec 3.1a), except the
        // active one and those that contain it.
        if self.is_assembly() {
            for d in self.project.documents() {
                if matches!(d.data, DocumentData::Assembly(_))
                    && !crate::assembly::tree::would_loop(
                        &self.project,
                        &self.linked(),
                        self.part_id,
                        DocRef::local(d.id),
                    )
                {
                    v.push((DocRef::local(d.id), format!("{} (assembly)", d.name)));
                }
            }
        }
        for l in self.project.libraries() {
            let Some(path) = self.library_path(l.id) else {
                continue;
            };
            let Ok(file) = self.part_file(&path) else {
                continue;
            };
            let f = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            for (id, name) in parts_of(&file) {
                let r = DocRef {
                    library: Some(l.id),
                    document: id,
                };
                v.push((r, format!("{name} ({f})")));
            }
        }
        v
    }

    /// The name of a referenced document: of this file, or of a linked file
    /// (`<name> (<file>)`); `(missing)` when it cannot be found.
    pub(super) fn ref_name(&self, r: DocRef) -> String {
        match r.library {
            None => self.project.document(r.document).map(|d| d.name.clone()),
            Some(lib) => self.library_path(lib).and_then(|path| {
                let file = self.part_file(&path).ok()?;
                let name = file.document(r.document)?.name.clone();
                let f = path.file_name()?.to_string_lossy().into_owned();
                Some(format!("{name} ({f})"))
            }),
        }
        .unwrap_or_else(|| "(missing)".to_owned())
    }

    /// Starts placing part `r` of this design file or of a file it links.
    fn place_local(&mut self, r: DocRef) {
        let name = match r.library {
            None => self.project.document(r.document).map(|d| d.name.clone()),
            Some(lib) => self.library_path(lib).and_then(|path| {
                let file = self.part_file(&path).ok()?;
                file.document(r.document).map(|d| d.name.clone())
            }),
        }
        .unwrap_or_else(|| "Part".into());
        let where_ = if r.library.is_some() {
            "linked"
        } else {
            "in this file"
        };
        self.asm.place = None;
        if self.asm.place_for_replace {
            self.replace_chosen(r, name);
            return;
        }
        if self.asm.place_for_derive {
            self.derive_chosen(r, name);
            return;
        }
        self.asm.place_name = name.clone();
        self.asm.tool = Some(super::assembly::AsmTool::Place(r));
        self.file_status = Some(Ok(format!(
            "Place: {name} ({where_}); click in the view to place it"
        )));
    }

    /// Selects file `path` in the dialog: loads it and previews its first
    /// part.
    fn place_select(&mut self, path: PathBuf) {
        let loaded = self.part_file(&path);
        let models = loaded
            .as_ref()
            .map(|p| self.file_models(p))
            .unwrap_or_default();
        let Some(d) = self.asm.place.as_mut() else {
            return;
        };
        d.selected = Some(path);
        d.local_pick = None;
        d.preview = None;
        match loaded {
            Ok(_) => {
                d.parts = models;
                d.part = d.parts.first().map(|x| x.0);
                d.error = d
                    .parts
                    .is_empty()
                    .then(|| "this file has no part or assembly to place".into());
            }
            Err(e) => {
                d.parts.clear();
                d.part = None;
                d.error = Some(e);
            }
        }
        self.place_preview();
    }

    fn place_preview(&mut self) {
        let Some(d) = self.asm.place.as_ref() else {
            return;
        };
        if let Some(r) = d.local_pick {
            let mesh = match r.library {
                None => match self.project.document(r.document).map(|x| &x.data) {
                    Some(DocumentData::Part(part)) => {
                        let mut part = part.clone();
                        let _ = part.regenerate();
                        Some(Arc::new(PickMesh::of(&part)))
                    }
                    // A subassembly: its parts as one mesh.
                    Some(DocumentData::Assembly(_)) => self.asm_mesh(r),
                    _ => None,
                },
                Some(_) => self.with_part(r, |p| Arc::new(PickMesh::of(p))),
            };
            if let Some(d) = self.asm.place.as_mut() {
                d.preview = mesh;
            }
            return;
        }
        let (Some(path), Some(doc)) = (d.selected.clone(), d.part) else {
            return;
        };
        let mesh =
            self.part_file(&path)
                .ok()
                .and_then(|p| match p.document(doc).map(|x| &x.data) {
                    Some(DocumentData::Part(part)) => Some(Arc::new(PickMesh::of(part))),
                    Some(DocumentData::Assembly(_)) => self.file_assembly_mesh(&p, doc),
                    _ => None,
                });
        if let Some(d) = self.asm.place.as_mut() {
            d.preview = mesh;
        }
    }

    /// Links part `doc` of `path` and starts placing it.
    fn place_chosen(&mut self, path: PathBuf, doc: DocumentId) {
        let parts = self
            .part_file(&path)
            .map(|p| self.file_models(&p))
            .unwrap_or_default();
        let owner = PathBuf::from(self.file_path.trim());
        self.checkpoint(if self.asm.place_for_replace {
            "Replace"
        } else if self.asm.place_for_derive {
            "Derive"
        } else {
            "Place"
        });
        let mut resolver = crate::document::Resolver::new();
        match resolver.link(&mut self.project, &owner, &path, doc) {
            Ok(r) if self.is_drawing() => {
                // Base View › Browse: the linked part is the view's file.
                self.asm.place = None;
                self.drw_linked(r);
                self.file_status = Some(Ok(format!(
                    "Base View: {} is linked; click on the sheet to place the view",
                    path.display()
                )));
            }
            Ok(r) if self.asm.place_for_replace => {
                self.asm.place = None;
                self.replace_chosen(r, base_name(&path, &parts, doc));
            }
            Ok(r) if self.asm.place_for_derive => {
                self.asm.place = None;
                self.derive_chosen(r, base_name(&path, &parts, doc));
            }
            Ok(r) => {
                self.asm.place = None;
                self.asm.place_name = base_name(&path, &parts, doc);
                self.asm.tool = Some(super::assembly::AsmTool::Place(r));
                self.file_status = Some(Ok(format!(
                    "Place: {} is linked; click in the view to place it",
                    path.display()
                )));
            }
            Err(e) => {
                self.history.cancel();
                if let Some(d) = self.asm.place.as_mut() {
                    d.error = Some(e.to_string());
                }
            }
        }
    }

    /// The Place Component window.
    pub(super) fn place_window(&mut self, ctx: &egui::Context) {
        let Some(d) = self.asm.place.as_mut() else {
            return;
        };
        let mut open = true;
        let mut go: Option<PathBuf> = None;
        let mut select: Option<PathBuf> = None;
        let mut place: Option<(PathBuf, DocumentId)> = None;
        let mut new_part: Option<DocumentId> = None;
        let mut local: Option<(DocRef, bool)> = None;
        let mut save_first = false;
        let mut cancel = false;
        let title = if self.asm.place_for_replace {
            "Replace Component: New Component"
        } else if self.asm.place_for_derive {
            "Derive: Source Part"
        } else {
            "Place Component"
        };
        let w = egui::Window::new(title)
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_size([860.0, 520.0])
            .min_size([600.0, 380.0]);
        let r = self
            .layout
            .place("place_component", ctx.content_rect(), true, w)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Up").on_hover_text("The folder above").clicked()
                        && let Some(p) = d.dir.parent()
                    {
                        go = Some(p.to_path_buf());
                    }
                    if let Some(h) = super::files::home_dir()
                        && ui.button("Home").clicked()
                    {
                        go = Some(h);
                    }
                    ui.label(RichText::new(d.dir.display().to_string()).monospace());
                });
                ui.separator();
                // The list and the preview share the window half and half,
                // down to the buttons, which are laid out first (an estimate
                // of their height made the window grow every frame).
                egui::Panel::bottom("place_buttons").show(ui, |ui| {
                    if let Some(e) = &d.error {
                        ui.colored_label(Color32::from_rgb(235, 80, 70), e);
                    }
                    ui.horizontal(|ui| {
                        let ready =
                            d.local_pick.is_some() || (d.selected.is_some() && d.part.is_some());
                        let label = if self.asm.place_for_derive || self.asm.place_for_replace {
                            "OK"
                        } else {
                            "Place"
                        };
                        if ui.add_enabled(ready, egui::Button::new(label)).clicked() {
                            if let Some(id) = d.local_pick {
                                local = Some((id, true));
                            } else if let (Some(p), Some(doc)) = (d.selected.clone(), d.part) {
                                place = Some((p, doc));
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
                let list_h = (ui.available_height() - 8.0).max(160.0);
                let half = (ui.available_width() - 24.0) / 2.0;
                ui.horizontal_top(|ui| {
                    // Folders and part files.
                    ui.vertical(|ui| {
                        ui.set_width(half);
                        // The parts stored in this design file. What is drawn
                        // above the file list is measured, never estimated
                        // (an estimate made the window grow every frame).
                        let top = ui.cursor().top();
                        if !d.local.is_empty() {
                            let linked = d.local.iter().any(|x| x.0.library.is_some());
                            ui.label(
                                RichText::new(if linked {
                                    "In This File and Linked Files"
                                } else {
                                    "In This File"
                                })
                                .strong(),
                            );
                            let h = (d.local.len() as f32 * 22.0).min(list_h * 0.4);
                            egui::ScrollArea::vertical()
                                .id_salt("place_local")
                                .max_height(h)
                                .show(ui, |ui| {
                                    for (id, name) in &d.local {
                                        ui.horizontal(|ui| {
                                            super::panels::tree_icon(ui, Icon::Part);
                                            let r = ui
                                                .selectable_label(d.local_pick == Some(*id), name);
                                            if r.double_clicked() {
                                                local = Some((*id, true));
                                            } else if r.clicked() {
                                                local = Some((*id, false));
                                            }
                                        });
                                    }
                                });
                            ui.separator();
                            ui.label(RichText::new("Files").strong());
                        }
                        if !d.can_link {
                            ui.weak(
                                "Save the assembly to link part files: they are linked by a path relative to it.",
                            );
                            if ui.button("Save As...").clicked() {
                                save_first = true;
                            }
                        }
                        let used = ui.cursor().top() - top;
                        egui::ScrollArea::vertical()
                            .id_salt("place_files")
                            .max_height((list_h - used).max(80.0))
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if !d.can_link {
                                    return;
                                }
                                for (name, is_dir) in &d.entries {
                                    let path = d.dir.join(name);
                                    ui.horizontal(|ui| {
                                        super::panels::tree_icon(
                                            ui,
                                            if *is_dir { Icon::Folder } else { Icon::Part },
                                        );
                                        let on = d.selected.as_ref() == Some(&path);
                                        let r = ui.selectable_label(on, name);
                                        if *is_dir {
                                            if r.double_clicked() || r.clicked() {
                                                go = Some(path.clone());
                                            }
                                        } else if r.double_clicked() {
                                            select = Some(path.clone());
                                            place = d.part.map(|p| (path.clone(), p));
                                        } else if r.clicked() {
                                            select = Some(path.clone());
                                        }
                                    });
                                }
                                if d.entries.is_empty() {
                                    ui.weak("No folders or .3dcadcraft files here");
                                }
                            });
                    });
                    ui.separator();
                    // Preview of the selected part.
                    ui.vertical(|ui| {
                        // Room for the three lines under it.
                        let w = ui.available_width();
                        let side = w.min(list_h - 70.0).max(120.0);
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(w, side), Sense::drag());
                        let painter = ui.painter_at(rect);
                        painter.rect_filled(rect, 4.0, Color32::from_rgb(38, 41, 46));
                        d.yaw += resp.drag_delta().x * 0.01;
                        match &d.preview {
                            Some(m) => draw_preview(&painter, rect, m, d.yaw),
                            None => {
                                painter.text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "Select a part or assembly file",
                                    egui::FontId::proportional(14.0),
                                    Color32::GRAY,
                                );
                            }
                        }
                        if let Some(id) = d.local_pick
                            && let Some((_, name)) = d.local.iter().find(|x| x.0 == id)
                        {
                            ui.label(RichText::new(name).strong());
                            // A linked part's list name carries its file: "Plate (Plate.ccpart)".
                            let base = match id.library {
                                Some(_) => name.rsplit_once(" (").map_or(name.as_str(), |x| x.0),
                                None => name.as_str(),
                            };
                            ui.weak(format!("Occurrences are named {base}:1, {base}:2…"));
                            ui.weak(if id.library.is_some() {
                                "In a linked file: save that file and the assembly shows the change."
                            } else {
                                "Stored in this file: edits to it show in the assembly at once."
                            });
                        } else if let Some(path) = &d.selected {
                            let stem = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                            ui.label(RichText::new(stem).strong());
                            if d.parts.len() > 1 {
                                ui.label("Parts in this file:");
                                for (id, name) in &d.parts {
                                    if ui.radio(d.part == Some(*id), name).clicked() {
                                        new_part = Some(*id);
                                    }
                                }
                            }
                            if let Some(doc) = d.part {
                                let base = base_name(path, &d.parts, doc);
                                ui.weak(format!("Occurrences are named {base}:1, {base}:2…"));
                            }
                        }
                        if d.local_pick.is_none() {
                            ui.weak(
                                "The component is linked: it stays its own file; save it and the \
                                 assembly shows the change.",
                            );
                        }
                    });
                });
            });
        let _ = r;
        if let Some(dir) = go
            && let Some(d) = self.asm.place.as_mut()
        {
            d.go(dir);
        }
        if let Some(p) = select {
            self.place_select(p);
        }
        if let Some((id, go)) = local {
            if go {
                self.place_local(id);
                return;
            }
            if let Some(d) = self.asm.place.as_mut() {
                d.local_pick = Some(id);
                d.selected = None;
            }
            self.place_preview();
        }
        if save_first {
            self.asm.place = None;
            self.file_status = Some(Ok(
                "Place: save the assembly first (part files are linked by a path relative to it)"
                    .into(),
            ));
            self.after_save = Some(super::files::Pending::Place);
            self.open_dialog(super::files::DialogPurpose::SaveAs);
            return;
        }
        if let Some(doc) = new_part {
            if let Some(d) = self.asm.place.as_mut() {
                d.part = Some(doc);
            }
            self.place_preview();
        }
        // A double-click selects first, then places that file's first part.
        if let Some((path, _)) = place {
            let doc = self.asm.place.as_ref().and_then(|d| d.part);
            if let Some(doc) = doc {
                self.place_chosen(path, doc);
            }
        }
        if cancel || !open {
            self.asm.place = None;
            self.asm.place_for_replace = false;
            self.asm.place_for_derive = false;
        }
    }
}

/// Loads `path` and regenerates its parts (bodies for display and
/// picking; display meshes are not saved in the file).
fn load_regenerated(path: &Path) -> Result<Project, String> {
    let mut p = Project::load(path).map_err(|e| e.to_string())?;
    let ids: Vec<DocumentId> = p.documents().iter().map(|d| d.id).collect();
    for id in ids {
        if let Some(DocumentData::Part(part)) = p.document_mut(id).map(|d| &mut d.data) {
            let _ = part.regenerate();
        }
    }
    Ok(p)
}

/// Flat shading of a triangle facing `n` (towards the eye `back`).
fn shade(n: Vector3, back: Vector3) -> f32 {
    let light = (back * 0.8 + Vector3::new(0.3, 0.2, 0.5)).normalize();
    (0.35 + 0.65 * n.dot(&light).abs()) as f32
}

/// A shaded picture of `mesh` in `rect`, seen from above at an angle and
/// turned by `yaw` (painter's algorithm: far triangles first).
fn draw_preview(painter: &egui::Painter, rect: Rect, mesh: &PickMesh, yaw: f32) {
    let Some((lo, hi)) = mesh.bbox else {
        return;
    };
    let c = Point3::from((lo.coords + hi.coords) / 2.0);
    let radius = ((hi - lo).norm() / 2.0).max(1e-6);
    let (y, pitch) = (yaw as Real, 0.6f64);
    let back = Vector3::new(pitch.cos() * y.cos(), pitch.cos() * y.sin(), pitch.sin());
    let right = Vector3::z().cross(&back).normalize();
    let up = back.cross(&right);
    let scale = (rect.width().min(rect.height()) as Real * 0.45) / radius;
    let to_screen = |p: &Point3| {
        let v = p - c;
        Pos2::new(
            rect.center().x + (v.dot(&right) * scale) as f32,
            rect.center().y - (v.dot(&up) * scale) as f32,
        )
    };
    let tris = mesh.triangles();
    let step = (tris.len() / 30000).max(1);
    let mut list: Vec<(Real, [Pos2; 3], f32)> = tris
        .iter()
        .step_by(step)
        .map(|(_, [a, b, cc])| {
            let n = (b - a).cross(&(cc - a));
            let n = n.try_normalize(1e-18).unwrap_or(back);
            let depth = ((a - c) + (b - c) + (cc - c)).dot(&back);
            (
                depth,
                [to_screen(a), to_screen(b), to_screen(cc)],
                shade(n, back),
            )
        })
        .collect();
    list.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut m = egui::Mesh::default();
    for (_, pts, s) in list {
        let g = (200.0 * s) as u8;
        let col = Color32::from_rgb(g, g, (g as f32 * 1.05).min(255.0) as u8);
        let i = m.vertices.len() as u32;
        for p in pts {
            m.colored_vertex(p, col);
        }
        m.add_triangle(i, i + 1, i + 2);
    }
    painter.add(egui::Shape::mesh(m));
}

/// The part `mesh` moved by `t`, drawn see-through over the view (where a
/// click would place it).
pub(super) fn draw_ghost(
    painter: &egui::Painter,
    camera: &OrbitCamera,
    rect: Rect,
    mesh: &PickMesh,
    t: &Transform3,
) {
    let (back, _) = camera.frame();
    let tris = mesh.triangles();
    let step = (tris.len() / 20000).max(1);
    let mut m = egui::Mesh::default();
    for (_, [a, b, c]) in tris.iter().step_by(step) {
        let (a, b, c) = (t * a, t * b, t * c);
        let (Some(pa), Some(pb), Some(pc)) = (
            camera.project(&a, rect),
            camera.project(&b, rect),
            camera.project(&c, rect),
        ) else {
            continue;
        };
        let n = (b - a).cross(&(c - a)).try_normalize(1e-18).unwrap_or(back);
        let s = shade(n, back);
        let col = Color32::from_rgba_unmultiplied(
            (255.0 * s) as u8,
            (170.0 * s) as u8,
            (70.0 * s) as u8,
            70,
        );
        let i = m.vertices.len() as u32;
        for p in [pa, pb, pc] {
            m.colored_vertex(p, col);
        }
        m.add_triangle(i, i + 1, i + 2);
    }
    painter.add(egui::Shape::mesh(m));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occurrences_are_named_after_the_file() {
        let (a, b) = (DocumentId::new_random(), DocumentId::new_random());
        let one = vec![(a, "Part1".to_owned())];
        let p = Path::new("/x/bracket.3dcadcraft");
        assert_eq!(base_name(p, &one, a), "bracket");
        let two = vec![(a, "Part1".to_owned()), (b, "Lid".to_owned())];
        assert_eq!(base_name(p, &two, b), "bracket-Lid");
    }
}
