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
        };
        d.refresh();
        d
    }

    fn refresh(&mut self) {
        match super::files::list_dir(&self.dir, &[crate::document::FILE_EXTENSION]) {
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

impl CadCraftApp {
    /// Where the part file of library link `lib` is (next to the assembly
    /// by its relative path, or by its name in the assembly's folder).
    pub(super) fn library_path(&self, lib: LibraryId) -> Option<PathBuf> {
        let link = self.project.library(lib)?;
        let owner = Path::new(self.file_path.trim());
        let dir = owner.parent()?;
        let stored = dir.join(&link.path);
        if stored.exists() {
            return Some(stored);
        }
        let flat = dir.join(Path::new(&link.path).file_name()?);
        flat.exists().then_some(flat)
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
        changed
    }

    /// Runs `f` on the part a component reference shows: a part of this
    /// file, or of a linked part file. `None` when it cannot be found.
    pub(super) fn with_part<R>(&self, r: DocRef, f: impl FnOnce(&PartDocument) -> R) -> Option<R> {
        match r.library {
            None => asm::part_of(&self.project, r).map(f),
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
        if self.file_path.trim().is_empty() {
            self.file_status = Some(Ok(
                "Place: save the assembly first (part files are linked by a path relative to it)"
                    .into(),
            ));
            self.after_save = Some(super::files::Pending::Place);
            self.open_dialog(super::files::DialogPurpose::SaveAs);
            return;
        }
        let dir = Path::new(self.file_path.trim())
            .parent()
            .map(Path::to_path_buf)
            .or_else(super::files::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        self.asm.place = Some(PlaceDialog::new(dir));
    }

    /// Selects file `path` in the dialog: loads it and previews its first
    /// part.
    fn place_select(&mut self, path: PathBuf) {
        let loaded = self.part_file(&path);
        let Some(d) = self.asm.place.as_mut() else {
            return;
        };
        d.selected = Some(path);
        d.preview = None;
        match loaded {
            Ok(p) => {
                d.parts = parts_of(&p);
                d.part = d.parts.first().map(|x| x.0);
                d.error = d.parts.is_empty().then(|| "this file has no part".into());
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
        let (Some(path), Some(doc)) = (d.selected.clone(), d.part) else {
            return;
        };
        let mesh =
            self.part_file(&path)
                .ok()
                .and_then(|p| match p.document(doc).map(|x| &x.data) {
                    Some(DocumentData::Part(part)) => Some(Arc::new(PickMesh::of(part))),
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
            .map(|p| parts_of(&p))
            .unwrap_or_default();
        let owner = PathBuf::from(self.file_path.trim());
        self.checkpoint("Place");
        let mut resolver = crate::document::Resolver::new();
        match resolver.link(&mut self.project, &owner, &path, doc) {
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
        let mut cancel = false;
        let w = egui::Window::new("Place Component")
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_width(760.0);
        let r = self
            .layout
            .place("place_component", ctx.content_rect(), false, w)
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
                ui.horizontal_top(|ui| {
                    // Folders and part files.
                    ui.vertical(|ui| {
                        ui.set_width(330.0);
                        egui::ScrollArea::vertical()
                            .id_salt("place_files")
                            .max_height(330.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
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
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(360.0, 260.0), Sense::drag());
                        let painter = ui.painter_at(rect);
                        painter.rect_filled(rect, 4.0, Color32::from_rgb(38, 41, 46));
                        d.yaw += resp.drag_delta().x * 0.01;
                        match &d.preview {
                            Some(m) => draw_preview(&painter, rect, m, d.yaw),
                            None => {
                                painter.text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "Select a part file",
                                    egui::FontId::proportional(14.0),
                                    Color32::GRAY,
                                );
                            }
                        }
                        if let Some(path) = &d.selected {
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
                        ui.weak(
                            "The part is linked: it stays its own file; save it and the \
                             assembly shows the change.",
                        );
                    });
                });
                if let Some(e) = &d.error {
                    ui.colored_label(Color32::from_rgb(235, 80, 70), e);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let ready = d.selected.is_some() && d.part.is_some();
                    if ui.add_enabled(ready, egui::Button::new("Place")).clicked()
                        && let (Some(p), Some(doc)) = (d.selected.clone(), d.part)
                    {
                        place = Some((p, doc));
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
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
