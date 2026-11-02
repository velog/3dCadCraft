//! The browser's Files tab (spec 0.6b §3.3): the documents of the open design
//! file, by kind, and the folder it is in, instead of a row of tabs above the
//! view that a file with many parts overflows. A row hovered or selected shows its picture in
//! a pane below the lists; a row dragged into an assembly places it there.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText};

use super::documents::{DocCmd, doc_menu, kind_icon};
use super::icons::Icon;
use super::panels::tree_icon;
use crate::document::{DocumentData, DocumentId, DocumentKind};
use crate::kernel::Point3;
use crate::part::thumbnail;

/// Extensions the Folder list shows.
const OPENED: &[&str] = &[
    "3dcadcraft",
    "ccpart",
    "ccasm",
    "ccdrw",
    "ccpres",
    "ccproj",
    "step",
    "stp",
    "stl",
    "obj",
];

/// How often a shown folder is read again.
const REFRESH: Duration = Duration::from_secs(3);
/// The picture's side in the preview pane.
const PANE_SIDE: f32 = 190.0;

/// Which side of the browser panel shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BrowserTab {
    /// The active document's tree.
    #[default]
    Model,
    /// This file's documents and the folder.
    Files,
}

/// One entry of a listed folder.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    path: PathBuf,
    name: String,
    folder: bool,
}

/// What a row of the Files tab stands for (also the payload of its drag).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum FileItem {
    /// A document of the open file.
    Doc(DocumentId),
    /// A file in the folder.
    File(PathBuf),
}

/// What the preview pane shows of an item, made on a worker thread.
#[derive(Clone, Debug, Default)]
struct Shown {
    png: Option<Vec<u8>>,
    lines: Vec<String>,
}

/// The picture of one item (by its state), kept while the Files tab browses.
#[derive(Default)]
struct Preview {
    slot: Arc<Mutex<Option<Shown>>>,
    texture: Option<egui::TextureHandle>,
}

impl std::fmt::Debug for Preview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Preview")
            .field("texture", &self.texture.is_some())
            .finish()
    }
}

/// How many pictures stay in memory.
const PREVIEWS_KEPT: usize = 128;
/// The ghost's source while a file row is dragged.
type DragMesh = (PathBuf, Arc<crate::assembly::PickMesh>);

/// The Files tab's state.
#[derive(Debug, Default)]
pub struct FilesUi {
    /// Which tab shows.
    pub tab: BrowserTab,
    /// The folder shown (`None`: the open file's own).
    folder: Option<PathBuf>,
    /// What it holds, read at most every [`REFRESH`] (rule 20: never per frame).
    listed: Option<(PathBuf, Instant, Vec<Entry>)>,
    /// The row clicked.
    selected: Option<FileItem>,
    /// The pictures, by item and state.
    previews: HashMap<String, Preview>,
    /// The file row being dragged, as a mesh for the ghost.
    drag_mesh: Option<DragMesh>,
    /// A file dropped in the view while it is still loading: placed when it is there.
    pending: Option<(PathBuf, Point3)>,
}

/// The kinds of file a drag can place.
fn placeable_file(path: &Path) -> bool {
    path.extension()
        .and_then(|x| x.to_str())
        .map(str::to_ascii_lowercase)
        .is_some_and(|x| matches!(x.as_str(), "ccpart" | "ccasm" | "3dcadcraft"))
}

/// What the pane says of a file, read on a worker thread: the picture saved with it. A design
/// file saved without one (an older build) draws its parts now.
fn file_shown(path: &Path) -> Shown {
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let design = placeable_file(path)
        || path
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| x.eq_ignore_ascii_case("ccdrw") || x.eq_ignore_ascii_case("ccproj"));
    if !design {
        return Shown {
            png: None,
            lines: vec![name, "Imported with File, Open or Import".into()],
        };
    }
    match crate::document::format::read_preview(path) {
        Ok(v) => {
            let count = |k| v.documents.iter().filter(|d| d.1 == k).count();
            let mut lines = vec![name];
            if let Some((doc, _)) = &v.document {
                lines.push(doc.clone());
            }
            let (p, a) = (count(DocumentKind::Part), count(DocumentKind::Assembly));
            lines.push(match a {
                0 => format!("{p} part(s)"),
                _ => format!("{p} part(s), {a} assembly(ies)"),
            });
            let png = v.thumbnail.clone().or_else(|| file_scene_png(path));
            Shown { png, lines }
        }
        Err(e) => Shown {
            png: None,
            lines: vec![name, e.to_string()],
        },
    }
}

/// The picture of a file's first part (it was saved without a picture).
fn file_scene_png(path: &Path) -> Option<Vec<u8>> {
    let mut p = crate::document::Project::load(path).ok()?;
    p.rebuild_unmeshed_parts();
    p.documents().iter().find_map(|d| match &d.data {
        DocumentData::Part(x) => thumbnail::part_png(x, thumbnail::iso_view()),
        _ => None,
    })
}

/// The folder's subfolders and openable files, folders first, by name.
fn read_folder(dir: &Path) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<Entry> = rd
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let folder = e.file_type().ok()?.is_dir();
            let known = path
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| OPENED.contains(&x.to_ascii_lowercase().as_str()));
            (folder || known).then_some(Entry { path, name, folder })
        })
        .collect();
    v.sort_by(|a, b| {
        b.folder
            .cmp(&a.folder)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    v
}

/// The icon of a file by its extension.
fn file_icon(path: &Path) -> Icon {
    match path
        .extension()
        .and_then(|x| x.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("ccasm") => Icon::Assembly,
        Some("ccdrw") => Icon::Drawing,
        Some("3dcadcraft" | "ccproj") => Icon::Project,
        _ => Icon::Part,
    }
}

impl super::CadCraftApp {
    /// The tab row at the top of the browser panel.
    pub(super) fn browser_tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (tab, label, tip) in [
                (BrowserTab::Model, "Model", "The active document's tree"),
                (
                    BrowserTab::Files,
                    "Files",
                    "The documents of this file and the folder it is in",
                ),
            ] {
                let on = self.files.tab == tab;
                let text = if on {
                    RichText::new(label).strong()
                } else {
                    RichText::new(label)
                };
                if ui.selectable_label(on, text).on_hover_text(tip).clicked() {
                    self.files.tab = tab;
                }
            }
        });
    }

    /// The Files tab: the lists, and below them the picture of the row hovered or selected.
    pub(super) fn files_tab(&mut self, ui: &mut egui::Ui) {
        let mut open = None;
        let mut ask: Option<(DocCmd, DocumentId)> = None;
        let mut open_file: Option<PathBuf> = None;
        let mut hovered: Option<FileItem> = None;
        let mut order: Vec<FileItem> = Vec::new();
        let ctx = ui.ctx().clone();
        // Bottom first, so the lists get what is left (a list sized from an estimate grows the
        // panel every frame).
        let width = ui.available_width();
        let side = (width - 8.0).clamp(64.0, PANE_SIDE);
        let total = ui.available_rect_before_wrap();
        ui.allocate_rect(total, egui::Sense::hover());
        let pane = egui::Rect::from_min_max(
            egui::pos2(total.left(), total.bottom() - side - 88.0),
            total.right_bottom(),
        );
        let lists = egui::Rect::from_min_max(
            total.left_top(),
            egui::pos2(total.right(), pane.top() - 4.0),
        );
        let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(lists));
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(&mut list_ui, |ui| {
                ui.horizontal(|ui| {
                    tree_icon(ui, Icon::Project);
                    ui.label(RichText::new(self.project.name.clone()).strong());
                });
                let docs: Vec<(DocumentId, String, DocumentKind)> = self
                    .project
                    .documents()
                    .iter()
                    .map(|d| (d.id, d.name.clone(), d.data.kind()))
                    .collect();
                for (kind, title) in [
                    (DocumentKind::Assembly, "Assemblies"),
                    (DocumentKind::Part, "Parts"),
                    (DocumentKind::Drawing, "Drawings"),
                ] {
                    let of: Vec<_> = docs.iter().filter(|d| d.2 == kind).collect();
                    if of.is_empty() {
                        continue;
                    }
                    egui::CollapsingHeader::new(format!("{title} ({})", of.len()))
                        .id_salt(("files_kind", title))
                        .default_open(true)
                        .show(ui, |ui| {
                            for (id, name, kind) in of {
                                let item = FileItem::Doc(*id);
                                order.push(item.clone());
                                ui.horizontal(|ui| {
                                    tree_icon(ui, kind_icon(*kind));
                                    let on = self.files.selected.as_ref() == Some(&item)
                                        || (self.files.selected.is_none() && *id == self.part_id);
                                    // No tooltip: it would cover the next rows while
                                    // browsing; the pane says what the row does.
                                    let r = ui
                                        .selectable_label(on, name)
                                        .interact(egui::Sense::click_and_drag());
                                    if r.hovered() {
                                        hovered = Some(item.clone());
                                    }
                                    if r.clicked() {
                                        self.files.selected = Some(item.clone());
                                    }
                                    if *kind != DocumentKind::Drawing {
                                        r.dnd_set_drag_payload(item.clone());
                                    }
                                    if r.double_clicked() && *id != self.part_id {
                                        open = Some(*id);
                                    }
                                    r.context_menu(|ui| doc_menu(ui, *id, &mut ask, &mut open));
                                });
                            }
                        });
                }
                ui.separator();
                self.folder_list(ui, &mut open_file, &mut hovered, &mut order);
            });
        if let Some(step) = ctx.input(|i| {
            if i.key_pressed(egui::Key::ArrowDown) {
                Some(1isize)
            } else if i.key_pressed(egui::Key::ArrowUp) {
                Some(-1)
            } else {
                None
            }
        }) && !ctx.egui_wants_keyboard_input()
            && let Some(k) = order
                .iter()
                .position(|x| Some(x) == self.files.selected.as_ref())
            && let Some(next) = order.get((k as isize + step).max(0) as usize)
        {
            self.files.selected = Some(next.clone());
        }
        let shown = hovered.or_else(|| self.files.selected.clone());
        self.files_preview_pane(ui, pane, side, shown.as_ref());
        self.files_drag_label(&ctx);
        if let Some(id) = open {
            self.asm.came_from = None;
            self.activate_document(id);
        }
        if let Some((c, id)) = ask {
            self.docs_apply_to(c, id);
        }
        if let Some(p) = open_file {
            self.open_path(&p);
        }
    }

    /// The name of what is being dragged, next to the cursor.
    fn files_drag_label(&mut self, ctx: &egui::Context) {
        let Some(item) = egui::DragAndDrop::payload::<FileItem>(ctx) else {
            self.files.drag_mesh = None;
            return;
        };
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            egui::DragAndDrop::clear_payload(ctx);
            return;
        }
        if let FileItem::File(p) = &*item {
            // Loaded meanwhile: it is there when the row is dropped.
            self.asm.libs.prefetch(p);
        }
        let Some(at) = ctx.pointer_interact_pos() else {
            return;
        };
        let name = match &*item {
            FileItem::Doc(id) => self
                .project
                .document(*id)
                .map_or_else(String::new, |d| d.name.clone()),
            FileItem::File(p) => p
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        };
        let hint = if self.is_assembly() {
            "Drop it in the view to place it"
        } else {
            "Open an assembly to place it"
        };
        egui::Area::new(egui::Id::new("files_drag_label"))
            .order(egui::Order::Tooltip)
            .fixed_pos(at + egui::vec2(14.0, 14.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(RichText::new(name).strong());
                    ui.label(RichText::new(hint).weak().small());
                });
            });
    }

    /// The key of an item's picture: changes with what it shows.
    fn preview_key(&self, item: &FileItem) -> Option<String> {
        match item {
            FileItem::File(p) => {
                let time = std::fs::metadata(p).and_then(|m| m.modified()).ok();
                Some(format!("f:{}:{time:?}", p.display()))
            }
            FileItem::Doc(id) => match &self.project.document(*id)?.data {
                DocumentData::Part(x) => Some(format!("d:{id:?}:{}", x.body_signature())),
                DocumentData::Assembly(a) => {
                    let places: Vec<_> = a
                        .occurrences
                        .iter()
                        .filter(|o| o.shown())
                        .map(|o| (o.component, o.transform))
                        .collect();
                    Some(format!("a:{id:?}:{places:?}"))
                }
                _ => Some(format!("o:{id:?}")),
            },
        }
    }

    /// The assembly a save's picture shows: the active one, or (a part active) the file's top
    /// assembly. `None`: a part alone, or a drawing.
    pub(super) fn picture_assembly(&self) -> Option<DocumentId> {
        if self.is_assembly() {
            return Some(self.part_id);
        }
        if self.is_drawing() {
            return None;
        }
        let used: Vec<DocumentId> = self
            .project
            .documents()
            .iter()
            .flat_map(|d| d.data.references())
            .map(|r| r.document)
            .collect();
        self.project
            .documents()
            .iter()
            .find(|d| matches!(d.data, DocumentData::Assembly(_)) && !used.contains(&d.id))
            .map(|d| d.id)
    }

    /// What a document of this file looks like, to draw on a worker thread.
    pub(super) fn doc_scene(&self, id: DocumentId) -> Option<(thumbnail::Scene, Vec<String>)> {
        let d = self.project.document(id)?;
        let mut scene = thumbnail::Scene::default();
        let mut lines = vec![d.name.clone()];
        match &d.data {
            DocumentData::Part(x) => {
                scene.add_part(x, None);
                lines.push("Part in this file".into());
            }
            DocumentData::Assembly(a) => {
                let linked = self.linked();
                let r = crate::document::DocRef::local(id);
                let leaves = crate::assembly::tree::leaves(&self.project, &linked, r, a);
                for l in &leaves {
                    self.with_part(l.component, |p| scene.add_part(p, Some(&l.world)));
                }
                lines.push(format!("Assembly in this file, {} part(s)", leaves.len()));
            }
            _ => lines.push("Drawing in this file".into()),
        }
        Some((scene, lines))
    }

    /// The pane below the lists.
    fn files_preview_pane(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        side: f32,
        item: Option<&FileItem>,
    ) {
        let ctx = ui.ctx().clone();
        let pic = egui::Rect::from_min_size(
            rect.center_top() + egui::vec2(-side / 2.0, 4.0),
            egui::vec2(side, side),
        );
        let painter = ui.painter_at(rect);
        painter.rect_filled(pic, 4.0, ui.visuals().extreme_bg_color);
        let weak = ui.visuals().weak_text_color();
        let font = egui::FontId::proportional(12.0);
        let Some(key) = item.and_then(|i| self.preview_key(i)) else {
            painter.text(
                pic.center(),
                egui::Align2::CENTER_CENTER,
                "Point at a part to see it",
                font,
                weak,
            );
            return;
        };
        if !self.files.previews.contains_key(&key) {
            if self.files.previews.len() >= PREVIEWS_KEPT {
                self.files.previews.clear();
            }
            let preview = Preview::default();
            let slot = preview.slot.clone();
            let job: Box<dyn FnOnce() -> Shown + Send> = match item {
                Some(FileItem::File(p)) => {
                    let p = p.clone();
                    Box::new(move || file_shown(&p))
                }
                Some(FileItem::Doc(id)) => {
                    let (scene, lines) = self.doc_scene(*id).unwrap_or_default();
                    Box::new(move || Shown {
                        png: thumbnail::png(&scene, thumbnail::iso_view(), 256),
                        lines,
                    })
                }
                None => Box::new(Shown::default),
            };
            let wake = ctx.clone();
            let spawned = std::thread::Builder::new()
                .name("files-preview".into())
                .stack_size(64 << 20)
                .spawn(move || {
                    let shown = job();
                    *crate::diagnostics::lock(&slot) = Some(shown);
                    wake.request_repaint();
                });
            if let Err(e) = spawned {
                log::warn!("files preview: {e}");
            }
            self.files.previews.insert(key.clone(), preview);
        }
        let Some(preview) = self.files.previews.get_mut(&key) else {
            return;
        };
        let shown = crate::diagnostics::lock(&preview.slot).clone();
        let Some(shown) = shown else {
            painter.text(
                pic.center(),
                egui::Align2::CENTER_CENTER,
                "Reading...",
                font,
                weak,
            );
            return;
        };
        if preview.texture.is_none()
            && let Some((w, h, rgba)) = shown.png.as_deref().and_then(thumbnail::decode_png)
        {
            let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
            preview.texture = Some(ctx.load_texture(
                format!("files-preview:{key}"),
                img,
                egui::TextureOptions::LINEAR,
            ));
        }
        match &preview.texture {
            Some(t) => {
                painter.image(
                    t.id(),
                    pic.shrink(2.0),
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            None => {
                painter.text(
                    pic.center(),
                    egui::Align2::CENTER_CENTER,
                    "No preview",
                    font.clone(),
                    weak,
                );
            }
        }
        let mut y = pic.bottom() + 4.0;
        for (k, line) in shown.lines.iter().take(3).enumerate() {
            let color = if k == 0 {
                ui.visuals().text_color()
            } else {
                weak
            };
            let g = painter.layout(line.clone(), font.clone(), color, rect.width() - 8.0);
            let h = g.size().y;
            painter.galley(egui::pos2(rect.left() + 4.0, y), g, color);
            y += h + 1.0;
        }
        let tip = painter.layout(
            "Double-click opens it; drag it into an assembly to place it".into(),
            egui::FontId::proportional(10.0),
            weak,
            rect.width() - 8.0,
        );
        painter.galley(egui::pos2(rect.left() + 4.0, y + 1.0), tip, weak);
    }

    /// A row of this tab dropped on the view at `pos`.
    pub(super) fn file_item_dropped(&mut self, item: &FileItem, pos: egui::Pos2, rect: egui::Rect) {
        if !self.is_assembly() {
            self.file_status = Some(Ok(
                "Files: drop it into an assembly to place it; a double-click opens it".into(),
            ));
            return;
        }
        let Some(at) = self.place_point(pos, rect) else {
            return;
        };
        match item {
            FileItem::Doc(id) => self.place_doc_at(*id, at),
            FileItem::File(path) => {
                if !placeable_file(path) {
                    self.file_status = Some(Err(
                        "Place: only parts and assemblies can be dropped here".into(),
                    ));
                } else if self.asm.libs.ready(path) {
                    self.place_file_at(path, at);
                } else {
                    self.asm.libs.prefetch(path);
                    self.files.pending = Some((path.clone(), at));
                    self.file_status = Some(Ok(format!("Place: loading {}", path.display())));
                }
            }
        }
    }

    /// The file dropped while it was loading, once it is there; the ghost of a dragged row.
    pub(super) fn files_drag_view(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        rect: egui::Rect,
    ) {
        if let Some((path, at)) = self.files.pending.clone() {
            if self.asm.libs.ready(&path) {
                self.files.pending = None;
                self.place_file_at(&path, at);
            } else {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }
        let Some(item) = egui::DragAndDrop::payload::<FileItem>(ctx) else {
            return;
        };
        let Some(pos) = ctx.pointer_interact_pos().filter(|p| rect.contains(*p)) else {
            return;
        };
        let Some(at) = self.place_point(pos, rect) else {
            return;
        };
        let mesh = match &*item {
            FileItem::Doc(id) => self.asm_mesh(crate::document::DocRef::local(*id)),
            FileItem::File(p) => match &self.files.drag_mesh {
                Some((q, m)) if q == p => Some(m.clone()),
                _ => {
                    ctx.request_repaint_after(Duration::from_millis(50));
                    let m = self.asm.libs.ready(p).then(|| self.file_mesh(p)).flatten();
                    if let Some(m) = &m {
                        self.files.drag_mesh = Some((p.clone(), m.clone()));
                    }
                    m
                }
            },
        };
        if let Some(mesh) = mesh {
            let t = crate::kernel::Transform3::translation(at.x, at.y, at.z);
            super::place::draw_ghost(painter, &self.camera, rect, &mesh, &t);
        }
    }

    /// The folder's list: up, subfolders, files.
    fn folder_list(
        &mut self,
        ui: &mut egui::Ui,
        open_file: &mut Option<PathBuf>,
        hovered: &mut Option<FileItem>,
        order: &mut Vec<FileItem>,
    ) {
        let own = Path::new(&self.file_path).parent().map(Path::to_path_buf);
        let Some(dir) = self
            .files
            .folder
            .clone()
            .or(own.filter(|p| !p.as_os_str().is_empty()))
            .or_else(super::files::home_dir)
        else {
            return;
        };
        let now = Instant::now();
        let stale = !matches!(&self.files.listed,
            Some((d, at, _)) if *d == dir && now.duration_since(*at) < REFRESH);
        if stale {
            self.files.listed = Some((dir.clone(), now, read_folder(&dir)));
        }
        let name = dir.file_name().map_or_else(
            || dir.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        ui.horizontal(|ui| {
            if ui
                .small_button("Up")
                .on_hover_text("The parent folder")
                .clicked()
                && let Some(p) = dir.parent()
            {
                self.files.folder = Some(p.to_path_buf());
            }
            ui.label(RichText::new(name).strong())
                .on_hover_text(dir.display().to_string());
        });
        let entries = self
            .files
            .listed
            .as_ref()
            .map(|(_, _, v)| v.clone())
            .unwrap_or_default();
        if entries.is_empty() {
            ui.label(RichText::new("No design files here").weak());
        }
        let current = Path::new(&self.file_path).to_path_buf();
        for e in entries {
            ui.horizontal(|ui| {
                tree_icon(
                    ui,
                    if e.folder {
                        Icon::Project
                    } else {
                        file_icon(&e.path)
                    },
                );
                let item = FileItem::File(e.path.clone());
                let on = self.files.selected.as_ref() == Some(&item)
                    || (self.files.selected.is_none() && e.path == current);
                let r = ui
                    .selectable_label(on, &e.name)
                    .interact(egui::Sense::click_and_drag());
                if !e.folder {
                    order.push(item.clone());
                    if r.hovered() {
                        *hovered = Some(item.clone());
                    }
                    if r.clicked() {
                        self.files.selected = Some(item.clone());
                    }
                    if placeable_file(&e.path) && e.path != current {
                        r.dnd_set_drag_payload(item);
                    }
                }
                if r.double_clicked() {
                    if e.folder {
                        self.files.folder = Some(e.path.clone());
                    } else {
                        *open_file = Some(e.path.clone());
                    }
                }
            });
        }
    }

    /// Where the document chips were: one button naming the active document,
    /// listing the file's documents.
    pub(super) fn active_document_menu(&mut self, ui: &mut egui::Ui) {
        if self.project.documents().len() < 2 {
            return;
        }
        let docs: Vec<(DocumentId, String, DocumentKind)> = self
            .project
            .documents()
            .iter()
            .map(|d| (d.id, d.name.clone(), d.data.kind()))
            .collect();
        let name = docs
            .iter()
            .find(|d| d.0 == self.part_id)
            .map_or_else(String::new, |d| d.1.clone());
        let mut open = None;
        ui.menu_button(format!("{name} ({})", docs.len()), |ui| {
            egui::ScrollArea::vertical()
                .max_height(420.0)
                .show(ui, |ui| {
                    for (id, n, kind) in &docs {
                        ui.horizontal(|ui| {
                            tree_icon(ui, kind_icon(*kind));
                            if ui.selectable_label(*id == self.part_id, n).clicked() {
                                open = Some(*id);
                                ui.close();
                            }
                        });
                    }
                });
        })
        .response
        .on_hover_text("The documents of this file (Files tab of the browser)");
        if let Some(id) = open
            && id != self.part_id
        {
            self.asm.came_from = None;
            self.activate_document(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_lists_folders_first_and_only_design_files() {
        let dir = std::env::temp_dir().join(format!("cc_files_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Sub")).unwrap();
        for f in ["b.ccpart", "a.ccasm", "notes.txt", ".hidden.ccpart"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        let names: Vec<String> = read_folder(&dir).into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["Sub", "a.ccasm", "b.ccpart"]);
        assert!(placeable_file(&dir.join("a.ccasm")));
        assert!(!placeable_file(&dir.join("a.ccdrw")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_folder_lists_nothing() {
        assert!(read_folder(Path::new("Z:/no/such/folder/here")).is_empty());
    }
}
