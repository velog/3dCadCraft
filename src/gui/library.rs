//! Assemble › Place from Library (`docs/spec/6.3-content-center.md` §3.2,
//! §3.3): category tree, families, sizes, preview. Choosing Place writes the
//! member's file if it is missing, links it and hands over to the Place tool.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, Color32, RichText, Sense};

use super::CadCraftApp;
use crate::assembly::PickMesh;
use crate::components::catalog::Catalog;
use crate::components::samples::Detail;
use crate::document::{DocumentData, DocumentId};
use crate::part::CosmeticThread;

/// The window's state.
pub(super) struct LibraryUi {
    catalog: Catalog,
    /// The category picked in the tree (`""` = all).
    category: String,
    family: Option<usize>,
    member: Option<usize>,
    search: String,
    /// The level of detail asked for.
    detail: Detail,
    /// Only families of this standard (`None` = all).
    standard: Option<String>,
    /// Place starts Smart Drop where the family supports it.
    smart: bool,

    error: Option<String>,
    /// Family pictures that have arrived, and the worker drawing the others.
    thumbnails: HashMap<usize, egui::TextureHandle>,
    thumbnail_rx: Option<std::sync::mpsc::Receiver<(usize, Vec<u8>)>>,
    /// Preview per member file (a preview is one member's regeneration): its
    /// mesh and cosmetic threads.
    meshes: HashMap<PathBuf, (Arc<PickMesh>, Vec<CosmeticThread>)>,
    shown: Option<PathBuf>,
    yaw: f32,
}

impl CadCraftApp {
    /// Assemble › Place from Library. A member is linked by a path relative
    /// to the assembly, so the assembly must be saved first.
    pub(super) fn place_from_library(&mut self) {
        let Some(root) = crate::components::root_dir() else {
            self.file_status = Some(Err(
                "Place from Library: no user folder for the library".into()
            ));
            return;
        };
        if self.file_path.trim().is_empty() {
            self.file_status = Some(Ok(
                "Place from Library: save the assembly first (components are linked by a path relative to it)".into(),
            ));
            self.after_save = Some(super::files::Pending::Library);
            self.open_dialog(super::files::DialogPurpose::SaveAs);
            return;
        }
        let catalog = Catalog::load(&root);
        let error = catalog.warnings.first().cloned();
        // A remembered filter whose families are gone must not hide everything.
        let standard = self
            .library_standard
            .clone()
            .filter(|s| catalog.standards().contains(s));
        let thumbnail_rx = spawn_thumbnails(catalog.clone());
        self.asm.library = Some(LibraryUi {
            catalog,
            category: String::new(),
            family: None,
            member: None,
            search: String::new(),
            detail: Detail::from_label(&self.library_detail),
            standard,
            smart: !self.library_manual,
            error,
            thumbnails: HashMap::new(),
            thumbnail_rx,
            meshes: HashMap::new(),
            shown: None,
            yaw: 0.6,
        });
    }

    /// The member picked in the window: its file and the mesh of its part.
    fn library_select(&mut self, ui_state: &mut LibraryUi) {
        let (Some(f), Some(m)) = (ui_state.family, ui_state.member) else {
            ui_state.shown = None;
            return;
        };
        match ui_state.catalog.member_file_at(f, m, ui_state.detail) {
            Ok(path) => {
                if !ui_state.meshes.contains_key(&path) {
                    let mesh = self.part_file_pub(&path).ok().and_then(|p| {
                        p.documents().iter().find_map(|d| match &d.data {
                            DocumentData::Part(part) => {
                                Some((Arc::new(PickMesh::of(part)), part.cosmetic_threads.clone()))
                            }
                            _ => None,
                        })
                    });
                    if let Some(mesh) = mesh {
                        ui_state.meshes.insert(path.clone(), mesh);
                    }
                }
                ui_state.shown = Some(path);
                ui_state.error = None;
            }
            Err(e) => {
                ui_state.shown = None;
                ui_state.error = Some(e);
            }
        }
    }

    /// The window.
    pub(super) fn library_window(&mut self, ctx: &egui::Context) {
        let Some(mut lib) = self.asm.library.take() else {
            return;
        };
        let mut open = true;
        let mut place = false;
        let mut cancel = false;
        let mut picked: Option<(usize, usize)> = None;
        let mut redo = false;
        lib.take_thumbnails(ctx);
        let w = egui::Window::new("Place from Library")
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_size([920.0, 540.0])
            .min_size([700.0, 400.0]);
        self.layout
            .place("place_library", ctx.content_rect(), true, w)
            .show(ctx, |ui| {
                egui::Panel::bottom("library_buttons").show(ui, |ui| {
                    if let Some(e) = &lib.error {
                        ui.colored_label(Color32::from_rgb(235, 80, 70), e);
                    }
                ui.horizontal(|ui| {
                    ui.label("Detail");
                    let offered = match (lib.family, lib.member) {
                        (Some(f), Some(m)) => lib.catalog.levels(f, m),
                        _ => Detail::ALL.to_vec(),
                    };
                    for d in Detail::ALL {
                        let tip = match d {
                            Detail::Simple => "The envelope only: no thread, no recess. For large assemblies.",
                            Detail::Standard => "The usual shape with a cosmetic thread.",
                            Detail::Full => "The richest shape: chamfered hexagons, a bearing's rings. Takes longer to build.",
                        };
                        // The level asked for stays; this size may only have another.
                        let on = d == if offered.contains(&lib.detail) {
                            lib.detail
                        } else {
                            Detail::Standard
                        };
                        if ui
                            .add_enabled(offered.contains(&d), egui::RadioButton::new(on, d.label()))
                            .on_hover_text(tip)
                            .on_disabled_hover_text("This size has no such level")
                            .clicked()
                        {
                            lib.detail = d;
                            redo = true;
                        }
                    }
                });
                    ui.horizontal(|ui| {
                        let droppable = lib
                            .family
                            .is_some_and(|f| lib.catalog.fits(f).iter().any(|x| x.droppable()));
                        ui.add_enabled(
                            droppable || lib.family.is_none(),
                            egui::Checkbox::new(&mut lib.smart, "Smart Drop"),
                        )
                        .on_hover_text("The size follows the hole or shaft under the cursor; a click places it with its relationships.")
                        .on_disabled_hover_text("This family is placed by hand");
                        ui.add_space(12.0);
                        if ui
                            .add_enabled(lib.shown.is_some(), egui::Button::new("Place"))
                            .clicked()
                        {
                            place = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("Search");
                    ui.add(egui::TextEdit::singleline(&mut lib.search).desired_width(260.0));
                // Always there, so the Standard row does not move while typing.
                if ui
                    .add_enabled(!lib.search.is_empty(), egui::Button::new("x").small())
                    .on_hover_text("Clear the search")
                    .clicked()
                {
                    lib.search.clear();
                }
                ui.add_space(16.0);
                ui.label("Standard");
                if ui.selectable_label(lib.standard.is_none(), "All").clicked() {
                    lib.standard = None;
                }
                for s in lib.catalog.standards() {
                    let on = lib.standard.as_deref() == Some(s.as_str());
                    let tip = match s.as_str() {
                        "ISO" => "International: ISO",
                        "DIN" => "German: DIN",
                        "ANSI" => "American: ANSI / ASME, inch sizes",
                        "EN" => "European: EN (structural shapes)",
                        "Own" => "Families you published",
                        _ => "Manufacturers' series and other parts",
                    };
                    if ui.selectable_label(on, s.as_str()).on_hover_text(tip).clicked() {
                        lib.standard = Some(s);
                        lib.category.clear();
                        lib.family = None;
                    }
                }

                });
                ui.separator();
                let height = (ui.available_height() - 8.0).max(160.0);
                let width = ui.available_width();
                let (tree_w, list_w) = (width * 0.26, width * 0.34);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(tree_w);
                        ui.set_height(height);
                        egui::ScrollArea::vertical()
                            .id_salt("library_tree")
                            .show(ui, |ui| {
                                if ui
                                    .selectable_label(lib.category.is_empty(), "All Categories")
                                    .clicked()
                                {
                                    lib.category.clear();
                                    lib.family = None;
                                }
                                                        for c in lib.catalog.categories_of(lib.standard.as_deref()) {

                                    let depth = c.matches('/').count();
                                    let name = c.rsplit('/').next().unwrap_or(&c).to_owned();
                                    ui.horizontal(|ui| {
                                                                        ui.add_space(12.0 * (depth as f32 + 1.0));
                                let (r, _) =
                                    ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::hover());
                                super::icons::paint_category(ui.painter(), r, &c);

                                        if ui.selectable_label(lib.category == c, name).clicked() {
                                            lib.category = c.clone();
                                            lib.family = None;
                                        }
                                    });
                                }
                            });
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_width(list_w);
                        ui.set_height(height);
                        egui::ScrollArea::vertical()
                            .id_salt("library_list")
                            .show(ui, |ui| {
                                if lib.search.trim().is_empty() {
                                    members_by_family(ui, &mut lib, &mut picked);
                                } else {
                                                                let standard = lib.standard.clone();
                            let hits: Vec<(usize, usize)> = lib
                                .catalog
                                .search(&lib.search)
                                .into_iter()
                                .filter(|(f, _)| lib.catalog.in_standard(*f, standard.as_deref()))
                                .collect();

                                    if hits.is_empty() {
                                        ui.weak("Nothing found");
                                    }
                                    for (f, m) in hits {
                                        let fam = &lib.catalog.families[f];
                                        let text = format!("{} {}", fam.name, fam.members[m].size);
                                        let on = lib.family == Some(f) && lib.member == Some(m);
                                        if ui.selectable_label(on, text).clicked() {
                                            picked = Some((f, m));
                                        }
                                    }
                                }
                            });
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_height(height);
                        let side = ui.available_width().min(height - 110.0).max(120.0);
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(side, side), Sense::drag());
                        ui.painter()
                            .rect_filled(rect, 4.0, Color32::from_rgb(36, 40, 46));
                        lib.yaw += resp.drag_delta().x * 0.01;
                        let mesh = lib.shown.as_ref().and_then(|p| lib.meshes.get(p));
                        match mesh {
                            Some((mesh, threads)) => {
                                super::place::draw_preview_threads(
                                    ui.painter(),
                                    rect,
                                    mesh,
                                    lib.yaw,
                                    threads,
                                );
                            }
                            None => {
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "Select a size",
                                    egui::FontId::proportional(14.0),
                                    Color32::GRAY,
                                );
                            }
                        }
                        if let (Some(f), Some(m)) = (lib.family, lib.member)
                            && let Some(fam) = lib.catalog.families.get(f)
                            && let Some(e) = fam.members.get(m)
                        {
                            ui.label(RichText::new(format!("{} {}", fam.name, e.size)).strong());
                            ui.weak(fam.category.replace('/', " › "));
                            if fam.file.is_some() {
                                ui.weak("Your published family: its member is made once and linked.");
                            } else if fam.checked.is_empty() {
                                ui.weak("Nominal values, not yet compared with a source.");
                            } else {
                                ui.weak(format!("Compared with {}.", fam.checked));
                                ui.weak("Check against the standard before production.");
                            }
                        }
                    });
                });
            });
        if let Some((f, m)) = picked {
            lib.family = Some(f);
            lib.member = Some(m);
            redo = true;
        }
        if redo {
            self.library_select(&mut lib);
        }
        self.library_standard = lib.standard.clone();
        self.library_detail = lib.detail.label().to_owned();
        self.library_manual = !lib.smart;
        if place
            && lib.smart
            && let Some(f) = lib.family
            && lib.catalog.fits(f).iter().any(|x| x.droppable())
        {
            let LibraryUi {
                catalog, detail, ..
            } = lib;
            self.smart_drop_start(catalog, f, detail);
            return;
        }
        if place && let Some(path) = lib.shown.clone() {
            let doc = first_document(self, &path);
            if let Some(doc) = doc {
                self.place_chosen(path, doc);
                return;
            }
            lib.error = Some("the member's file holds no part".into());
        }
        if !(cancel || !open) {
            self.asm.library = Some(lib);
        }
    }
}

/// The families of the picked category, each with its sizes below it.
fn members_by_family(ui: &mut egui::Ui, lib: &mut LibraryUi, picked: &mut Option<(usize, usize)>) {
    let standard = lib.standard.clone();
    let list: Vec<usize> = lib
        .catalog
        .in_category(&lib.category)
        .into_iter()
        .filter(|f| lib.catalog.in_standard(*f, standard.as_deref()))
        .collect();

    if list.is_empty() {
        ui.weak("No families here");
    }
    // The family of a category holding only one is open from the start.
    let only = list.len() == 1 && !lib.category.is_empty();
    let side = crate::components::catalog::THUMBNAIL as f32 / 2.0;
    for f in list {
        let fam = &lib.catalog.families[f];
        let id = ui.make_persistent_id(("library_family", f));
        let ctx = ui.ctx().clone();
        let mut state =
            egui::collapsing_header::CollapsingState::load_with_default_open(&ctx, id, only);
        if only {
            state.set_open(true);
        }
        let mut toggle = false;
        let header = state.show_header(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), Sense::hover());
            if let Some(t) = lib.thumbnails.get(&f) {
                ui.painter().image(
                    t.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            toggle = ui.selectable_label(false, &fam.name).clicked();
        });
        header.body(|ui| {
            for (m, e) in fam.members.iter().enumerate() {
                let on = lib.family == Some(f) && lib.member == Some(m);
                if ui.selectable_label(on, &e.size).clicked() {
                    *picked = Some((f, m));
                }
            }
        });
        if toggle {
            let mut s =
                egui::collapsing_header::CollapsingState::load_with_default_open(&ctx, id, only);
            s.toggle(ui);
            s.store(&ctx);
        }
    }
}

/// Draws the family pictures one by one off the interface's thread. Dropping
/// the receiver (the window closes) ends the worker at its next picture.
fn spawn_thumbnails(catalog: Catalog) -> Option<std::sync::mpsc::Receiver<(usize, Vec<u8>)>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("library-thumbnails".into())
        .spawn(move || {
            for f in 0..catalog.families.len() {
                let picture = crate::diagnostics::crash::guard(|| catalog.thumbnail(f));
                if let Ok(Ok(rgba)) = picture
                    && tx.send((f, rgba)).is_err()
                {
                    return;
                }
            }
        })
        .ok()?;
    Some(rx)
}

impl LibraryUi {
    /// Moves the pictures that have arrived into textures, and asks for
    /// another frame while more are coming.
    fn take_thumbnails(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.thumbnail_rx else {
            return;
        };
        let side = crate::components::catalog::THUMBNAIL as usize;
        loop {
            match rx.try_recv() {
                Ok((f, rgba)) => {
                    let image = egui::ColorImage::from_rgba_unmultiplied([side, side], &rgba);
                    self.thumbnails.insert(
                        f,
                        ctx.load_texture(
                            format!("library_thumbnail_{f}"),
                            image,
                            egui::TextureOptions::LINEAR,
                        ),
                    );
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.thumbnail_rx = None;
                    return;
                }
            }
        }
    }
}

fn first_document(app: &CadCraftApp, path: &std::path::Path) -> Option<DocumentId> {
    let project = app.part_file_pub(path).ok()?;
    project
        .documents()
        .iter()
        .find(|d| matches!(d.data, DocumentData::Part(_)))
        .map(|d| d.id)
}
