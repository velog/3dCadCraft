//! Materials, appearances and Physical Properties in the GUI
//! (`docs/spec/2.15-materials.md`, `docs/spec/0.10-physical.md`): the
//! Quick Access Material / Appearance lists, the Material and Appearance
//! browsers, Adjust, Clear, a minimal Styles Editor, the Physical
//! Properties window, the browser/view context menus and the coloured
//! scene. The data lives in `part::appearance`; every change records an
//! Undo checkpoint first.

use eframe::egui::{self, Color32, RichText};

use super::CadCraftApp;
use super::renderer::SceneGeometry;
use super::select::SelItem;
use crate::kernel::TessellationOptions;
use crate::materials::{self, Appearance, GENERIC, Library};
use crate::part::PartDocument;
use crate::part::appearance::{ClearWhat, LookSource, LookTarget, Resolver, face_hint};
use crate::part::physical::{Accuracy, MassProps, Physical, physical_properties};

/// Ribbon commands of this module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatCmd {
    /// Tools → Material Browser.
    MaterialBrowser,
    /// Tools → Appearance Browser.
    AppearanceBrowser,
    /// Tools → Adjust.
    Adjust,
    /// Tools → Clear.
    Clear,
    /// Manage → Styles Editor.
    StylesEditor,
    /// Manage → Update (document styles from the library).
    UpdateStyles,
    /// Manage → Purge (unused document styles).
    Purge,
    /// Inspect → Physical Properties.
    Physical,
}

/// Window state and the loaded libraries.
pub(super) struct MaterialUi {
    /// Built-in + user library.
    pub lib: Library,
    /// Problems found loading the user library (shown once).
    problems: Vec<String>,
    show_materials: bool,
    show_appearances: bool,
    show_adjust: bool,
    show_clear: bool,
    show_styles: bool,
    show_physical: bool,
    search: String,
    adjust_color: [u8; 3],
    adjust_opacity: f64,
    clear: ClearWhat,
    clear_selected_only: bool,
    accuracy: Accuracy,
    kg: bool,
    /// The last computation and its key (stale when the key differs).
    physical: Option<(u64, Result<Physical, String>)>,
    /// Bodies left out of the totals (by persistent name).
    excluded: Vec<String>,
}

impl MaterialUi {
    /// Closes floating window `key` (Escape over it; keys of `layout.rs`).
    pub(super) fn close_window(&mut self, key: &str) {
        match key {
            "materials" => self.show_materials = false,
            "appearances" => self.show_appearances = false,
            "adjust" => self.show_adjust = false,
            "clear_looks" => self.show_clear = false,
            "styles" => self.show_styles = false,
            "physical" => self.show_physical = false,
            _ => {}
        }
    }

    /// Loads the built-in library and the user's (`materials::user_dir`).
    pub(super) fn load() -> Self {
        let (lib, problems) = Library::with_user(materials::user_dir().as_deref());
        for p in &problems {
            log::warn!("{p}");
        }
        Self {
            lib,
            problems,
            show_materials: false,
            show_appearances: false,
            show_adjust: false,
            show_clear: false,
            show_styles: false,
            show_physical: false,
            search: String::new(),
            adjust_color: [200, 60, 60],
            adjust_opacity: 1.0,
            clear: ClearWhat::ALL,
            clear_selected_only: true,
            accuracy: Accuracy::default(),
            kg: false,
            physical: None,
            excluded: Vec::new(),
        }
    }

    /// The spec F1 opens while one of the windows is open.
    pub(super) fn help_file(&self) -> Option<&'static str> {
        if self.show_physical {
            Some("0.10-physical.md")
        } else if self.show_materials
            || self.show_appearances
            || self.show_adjust
            || self.show_clear
            || self.show_styles
        {
            Some("2.15-materials.md")
        } else {
            None
        }
    }

    /// Whether Adjust is open (its status-bar prompt).
    pub(super) fn adjusting(&self) -> bool {
        self.show_adjust
    }
}

/// Status-bar prompt of Adjust.
pub(super) const ADJUST_PROMPT: &str = "Adjust: select faces, features or bodies (the Select list sets which), pick a look or a colour, then Apply";

// ---- scene ------------------------------------------------------------------

/// How the viewport paints one face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct FacePaint {
    /// Shader colour (see `Appearance::shader_rgb`).
    pub rgb: [f32; 3],
    /// Opacity (1 = opaque pass, else the transparent pass).
    pub alpha: f32,
    /// Drawn from both sides (surface bodies).
    pub two_sided: bool,
}

/// Amber of surface bodies without a look of their own.
const SURFACE_AMBER: [f32; 3] = [0.86, 0.62, 0.32];

/// The paint of a face with appearance `look` from `source`, on a surface
/// body (`open`) or a solid. Surface bodies stay amber unless something
/// was assigned to them.
pub(super) fn face_paint(look: &Appearance, source: LookSource, open: bool) -> FacePaint {
    let rgb = if open && source == LookSource::Material {
        SURFACE_AMBER
    } else {
        look.shader_rgb()
    };
    FacePaint {
        rgb,
        alpha: look.opacity.clamp(0.0, 1.0) as f32,
        two_sided: open,
    }
}

/// Adds every body of `part` to the scene, each face in its resolved
/// appearance (opaque faces in the shaded pass with the red back-face
/// check; see-through ones in the transparent pass), plus the edges.
pub(super) fn add_part_meshes(g: &mut SceneGeometry, part: &PartDocument) {
    add_part_meshes_at(g, part, None, None);
}

/// [`add_part_meshes`] moved by `place` (an assembly occurrence) and
/// blended half way to `tint` (its selection colour).
pub(super) fn add_part_meshes_at(
    g: &mut SceneGeometry,
    part: &PartDocument,
    place: Option<&crate::kernel::Transform3>,
    tint: Option<[f32; 3]>,
) {
    let body = &part.body;
    let looks = Resolver::new(part).face_looks();
    let fallback = (Appearance::fallback(), LookSource::Material);
    for (id, solid) in body.solids() {
        // Hidden bodies (Combine's kept toolbodies, Visibility off).
        if solid.name.as_deref().is_some_and(|n| !part.body_visible(n)) {
            continue;
        }
        let mesh = match body.tessellate_solid(id, &TessellationOptions::default()) {
            Ok(m) => match place {
                Some(t) => moved(m, t),
                None => m,
            },
            Err(e) => {
                log::error!("tessellation failed: {e}");
                continue;
            }
        };
        let f = &mesh.faces;
        let mut colors = vec![SURFACE_AMBER; f.positions.len()];
        let mut opaque: Vec<[u32; 3]> = Vec::new();
        // Transparent triangles by colour.
        let mut clear: Vec<([f32; 4], Vec<[u32; 3]>)> = Vec::new();
        for (fid, range) in &mesh.face_triangles {
            let (look, src) = looks.get(fid).unwrap_or(&fallback);
            let mut paint = face_paint(look, *src, solid.open);
            if let Some(c) = tint {
                for (x, t) in paint.rgb.iter_mut().zip(c) {
                    *x = 0.5 * (*x + t);
                }
            }
            let tris = &f.triangles[range.clone()];
            if paint.alpha < 0.999 {
                let c = [paint.rgb[0], paint.rgb[1], paint.rgb[2], paint.alpha];
                match clear.iter_mut().find(|(k, _)| *k == c) {
                    Some((_, v)) => v.extend_from_slice(tris),
                    None => clear.push((c, tris.to_vec())),
                }
                continue;
            }
            for t in tris {
                for &i in t {
                    if let Some(c) = colors.get_mut(i as usize) {
                        *c = paint.rgb;
                    }
                }
            }
            opaque.extend_from_slice(tris);
        }
        g.add_mesh_colored(&f.positions, &f.normals, &opaque, &colors);
        if solid.open {
            // Surface bodies: their back is visible and is not an
            // orientation error.
            let back: Vec<_> = f.normals.iter().map(|n| -n).collect();
            let tris: Vec<_> = opaque.iter().map(|t| [t[0], t[2], t[1]]).collect();
            g.add_mesh_colored(&f.positions, &back, &tris, &colors);
        }
        for (c, tris) in &clear {
            g.add_mesh_transparent(&f.positions, &f.normals, tris, *c);
        }
        for edge in &mesh.edges {
            g.add_edge_polyline(edge, [0.02, 0.02, 0.04]);
        }
    }
}

/// `m` moved by the rigid transform `t`.
fn moved(
    mut m: crate::kernel::SolidMesh,
    t: &crate::kernel::Transform3,
) -> crate::kernel::SolidMesh {
    for p in &mut m.faces.positions {
        *p = t * *p;
    }
    for n in &mut m.faces.normals {
        *n = t * *n;
    }
    for e in &mut m.edges {
        for p in e {
            *p = t * *p;
        }
    }
    m
}

// ---- small widgets ------------------------------------------------------------

/// A colour swatch before a label.
fn swatch(ui: &mut egui::Ui, a: &Appearance) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
    let [cr, cg, cb] = a.color;
    let alpha = (a.opacity.clamp(0.0, 1.0) * 255.0) as u8;
    ui.painter().rect_filled(
        r,
        2.0,
        Color32::from_rgba_unmultiplied(cr, cg, cb, alpha.max(40)),
    );
    ui.painter().rect_stroke(
        r,
        2.0,
        egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
        egui::StrokeKind::Inside,
    );
}

fn matches(search: &str, name: &str, category: &str) -> bool {
    let s = search.trim().to_lowercase();
    s.is_empty() || name.to_lowercase().contains(&s) || category.to_lowercase().contains(&s)
}

/// Categories in first-use order.
fn categories<'a>(names: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut out: Vec<&str> = Vec::new();
    for c in names {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

fn mass_text(g: f64, kg: bool) -> String {
    if kg {
        format!("{:.6} kg", g / 1000.0)
    } else {
        format!("{g:.3} g")
    }
}

fn moment_text(gmm2: f64, kg: bool) -> String {
    if kg {
        // g·mm² → kg·m².
        format!("{:.6e} kg·m²", gmm2 * 1e-9)
    } else {
        format!("{gmm2:.3} g·mm²")
    }
}

/// A plain-text report (Clipboard).
fn report(p: &Physical, total: &MassProps, kg: bool) -> String {
    let mut s = String::from("Physical Properties\n");
    for b in &p.bodies {
        s += &format!(
            "{} ({}): {}, {:.3} g/cm³, mass {}, volume {:.3} mm³, area {:.3} mm²\n",
            b.label,
            b.name,
            b.material,
            b.density,
            mass_text(b.props.mass, kg),
            b.props.volume,
            b.props.area
        );
    }
    let c = total.center;
    s += &format!(
        "Total: mass {}, volume {:.3} mm³, area {:.3} mm²\nCenter of Gravity: {:.4}, {:.4}, {:.4} mm\n",
        mass_text(total.mass, kg),
        total.volume,
        total.area,
        c.x,
        c.y,
        c.z
    );
    let (vals, axes) = total.principal();
    for (k, (v, a)) in vals.iter().zip(&axes).enumerate() {
        s += &format!(
            "I{}: {}  axis ({:.4}, {:.4}, {:.4})\n",
            k + 1,
            moment_text(*v, kg),
            a.x,
            a.y,
            a.z
        );
    }
    let i = &total.inertia;
    s += &format!(
        "About the centre of gravity: Ixx {}, Iyy {}, Izz {}, Ixy {}, Iyz {}, Ixz {}\n",
        moment_text(i[(0, 0)], kg),
        moment_text(i[(1, 1)], kg),
        moment_text(i[(2, 2)], kg),
        moment_text(i[(0, 1)], kg),
        moment_text(i[(1, 2)], kg),
        moment_text(i[(0, 2)], kg)
    );
    s
}

/// What a click in a list asks for (applied after drawing).
enum Do {
    PartMaterial(Option<String>),
    Appearance(Option<String>),
    Custom([u8; 3], f64),
    Clear,
    Update,
    Purge,
    ComputePhysical,
}

impl CadCraftApp {
    /// A ribbon command.
    pub(super) fn material_cmd(&mut self, cmd: MatCmd) {
        // In a drawing the editor shows the drawing's styles.
        if matches!(cmd, MatCmd::StylesEditor) && self.is_drawing() {
            self.open_drawing_styles();
            return;
        }
        let m = &mut self.mat;
        match cmd {
            MatCmd::MaterialBrowser => m.show_materials = true,
            MatCmd::AppearanceBrowser => m.show_appearances = true,
            MatCmd::Adjust => m.show_adjust = true,
            MatCmd::Clear => {
                m.clear_selected_only = !self.sel.is_empty();
                m.show_clear = true;
            }
            MatCmd::StylesEditor => m.show_styles = true,
            MatCmd::UpdateStyles => self.run(Do::Update),
            MatCmd::Purge => self.run(Do::Purge),
            MatCmd::Physical => {
                m.show_physical = true;
                self.run(Do::ComputePhysical);
            }
        }
        if let Some(p) = self.mat.problems.first().cloned() {
            self.file_status = Some(Err(p));
            self.mat.problems.clear();
        }
    }

    /// The look targets of the idle-view selection (faces, features,
    /// bodies); empty means the part.
    fn look_targets(&self) -> Vec<LookTarget> {
        let body = &self.part().body;
        self.sel
            .iter()
            .filter_map(|x| match x {
                SelItem::Face(name) => {
                    let f = body.find_face(name)?;
                    Some(LookTarget::Face {
                        face: name.clone(),
                        hint: face_hint(body, f)?,
                    })
                }
                SelItem::Feature(id) => Some(LookTarget::Feature(id.clone())),
                SelItem::Body(n) => Some(LookTarget::Body(n.clone())),
                SelItem::Edge(_) | SelItem::Sketch(_) | SelItem::Vertex(_) => None,
            })
            .collect()
    }

    /// Runs `f` on a copy of the part; on success the copy replaces the
    /// part as one Undo step named `label` (nothing recorded when `f`
    /// reports no change); on failure the part is unchanged (rule 5).
    fn edit_part(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut PartDocument) -> Result<bool, String>,
    ) {
        let mut trial = self.part().clone();
        match f(&mut trial) {
            Ok(changed) => {
                if changed {
                    self.history.break_coalescing();
                    self.checkpoint(label);
                    *self.part_mut() = trial;
                    self.history.break_coalescing();
                    self.scene_dirty = true;
                }
                self.file_status = None;
            }
            Err(e) => self.file_status = Some(Err(format!("{label}: {e}"))),
        }
    }

    /// Gives `targets` (none: the part) appearance `name` (`None`: remove
    /// the override), as one Undo step.
    pub(super) fn assign_appearance(&mut self, targets: &[LookTarget], name: Option<&str>) {
        let targets: Vec<LookTarget> = if targets.is_empty() {
            vec![LookTarget::Part]
        } else {
            targets.to_vec()
        };
        let lib = self.mat.lib.clone();
        self.edit_part("Appearance", |p| {
            for t in &targets {
                p.set_appearance(t, name, &lib)?;
            }
            Ok(true)
        });
    }

    /// Gives `bodies` (none: the part) material `name`.
    pub(super) fn assign_material(&mut self, bodies: &[String], name: Option<&str>) {
        let lib = self.mat.lib.clone();
        let bodies = bodies.to_vec();
        self.edit_part("Material", |p| {
            if bodies.is_empty() {
                p.set_material(name, &lib)?;
            } else {
                for b in &bodies {
                    p.set_body_material(b, name, &lib)?;
                }
            }
            Ok(true)
        });
    }

    fn selected_bodies(&self) -> Vec<String> {
        self.sel
            .iter()
            .filter_map(|x| match x {
                SelItem::Body(n) => Some(n.clone()),
                _ => None,
            })
            .collect()
    }

    fn run(&mut self, d: Do) {
        match d {
            Do::PartMaterial(name) => {
                let bodies = self.selected_bodies();
                self.assign_material(&bodies, name.as_deref());
            }
            Do::Appearance(name) => {
                let t = self.look_targets();
                self.assign_appearance(&t, name.as_deref());
            }
            Do::Custom(color, opacity) => {
                let t = self.look_targets();
                let targets = if t.is_empty() {
                    vec![LookTarget::Part]
                } else {
                    t
                };
                let lib = self.mat.lib.clone();
                self.edit_part("Adjust", |p| {
                    let name = p.add_custom_appearance(color, opacity);
                    for x in &targets {
                        p.set_appearance(x, Some(&name), &lib)?;
                    }
                    Ok(true)
                });
            }
            Do::Clear => {
                let only = (self.mat.clear_selected_only && !self.sel.is_empty())
                    .then(|| self.look_targets());
                let what = self.mat.clear;
                let mut n = 0;
                self.edit_part("Clear Appearance", |p| {
                    n = p.clear_appearances(what, only.as_deref());
                    Ok(n > 0)
                });
                self.file_status = Some(Ok(format!("Clear: {n} override(s) removed")));
            }
            Do::Update => {
                let lib = self.mat.lib.clone();
                let mut n = 0;
                self.edit_part("Update Styles", |p| {
                    n = p.update_styles(&lib);
                    Ok(n > 0)
                });
                self.file_status =
                    Some(Ok(format!("Update: {n} style(s) updated from the library")));
            }
            Do::Purge => {
                let mut n = 0;
                self.edit_part("Purge Styles", |p| {
                    n = p.purge_styles();
                    Ok(n > 0)
                });
                self.file_status = Some(Ok(format!("Purge: {n} unused style(s) removed")));
            }
            Do::ComputePhysical => {
                let key = self.physical_key();
                let r = physical_properties(self.part(), self.mat.accuracy)
                    .map_err(|e| format!("Physical Properties: {e}"));
                self.mat.physical = Some((key, r));
            }
        }
    }

    /// What the physical properties depend on (stale when it changes).
    fn physical_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let p = self.part();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.body_key().hash(&mut h);
        serde_json::to_vec(&p.features)
            .unwrap_or_default()
            .hash(&mut h);
        p.material.hash(&mut h);
        serde_json::to_vec(&p.looks.bodies)
            .unwrap_or_default()
            .hash(&mut h);
        serde_json::to_vec(&p.looks.materials)
            .unwrap_or_default()
            .hash(&mut h);
        self.mat.accuracy.hash(&mut h);
        h.finish()
    }

    /// The label of the part's current appearance (`*` when something
    /// below the part has its own).
    fn appearance_label(&self) -> String {
        let p = self.part();
        let name = p.appearance.as_deref().unwrap_or("As Material");
        let star = if p.override_count() > usize::from(p.appearance.is_some()) {
            "*"
        } else {
            ""
        };
        format!("{star}{name}")
    }

    /// Material and Appearance lists of the Quick Access Toolbar.
    pub(super) fn material_qat(&mut self, ui: &mut egui::Ui) {
        let mut todo = None;
        let current = self
            .part()
            .material
            .clone()
            .unwrap_or_else(|| GENERIC.to_owned());
        ui.menu_button(format!("Material: {current}"), |ui| {
            ui.set_max_height(480.0);
            if let Some(n) = self.material_items(ui) {
                todo = Some(Do::PartMaterial(n));
            }
            ui.separator();
            if ui.button("Material Browser...").clicked() {
                self.mat.show_materials = true;
                ui.close();
            }
        })
        .response
        .on_hover_text("Material of the part, or of the selected bodies");
        ui.menu_button(format!("Appearance: {}", self.appearance_label()), |ui| {
            ui.set_max_height(480.0);
            if let Some(n) = self.appearance_items(ui) {
                todo = Some(Do::Appearance(n));
            }
            ui.separator();
            if ui.button("Appearance Browser...").clicked() {
                self.mat.show_appearances = true;
                ui.close();
            }
            if ui.button("Adjust...").clicked() {
                self.mat.show_adjust = true;
                ui.close();
            }
            if ui.button("Clear...").clicked() {
                self.mat.clear_selected_only = !self.sel.is_empty();
                self.mat.show_clear = true;
                ui.close();
            }
        })
        .response
        .on_hover_text(
            "Appearance of the selected faces, features or bodies (nothing selected: the part). \
             * = some faces, features or bodies have their own",
        );
        if let Some(d) = todo {
            self.run(d);
        }
    }

    /// Material entries by category; `Some(None)` = Generic.
    fn material_items(&self, ui: &mut egui::Ui) -> Option<Option<String>> {
        let lib = &self.mat.lib;
        let mut picked = None;
        let cats = categories(lib.materials.iter().map(|m| m.category.as_str()));
        for c in cats {
            ui.menu_button(c, |ui| {
                for m in lib.materials.iter().filter(|m| m.category == c) {
                    let look = Resolver::new(self.part()).appearance(&m.appearance);
                    ui.horizontal(|ui| {
                        swatch(ui, &look);
                        if ui
                            .button(&m.name)
                            .on_hover_text(format!("{:.3} g/cm³", m.density))
                            .clicked()
                        {
                            picked = Some((m.name != GENERIC).then(|| m.name.clone()));
                            ui.close();
                        }
                    });
                }
            });
        }
        picked
    }

    /// Appearance entries by category; `Some(None)` = As Material.
    fn appearance_items(&self, ui: &mut egui::Ui) -> Option<Option<String>> {
        let mut picked = None;
        if ui
            .button("As Material")
            .on_hover_text("Remove the override")
            .clicked()
        {
            picked = Some(None);
            ui.close();
        }
        let doc = &self.part().looks.appearances;
        if !doc.is_empty() {
            ui.label(RichText::new("Document Appearances").weak());
            for a in doc {
                ui.horizontal(|ui| {
                    swatch(ui, a);
                    if ui.button(&a.name).clicked() {
                        picked = Some(Some(a.name.clone()));
                        ui.close();
                    }
                });
            }
            ui.separator();
        }
        let lib = &self.mat.lib;
        for c in categories(lib.appearances.iter().map(|a| a.category.as_str())) {
            ui.menu_button(c, |ui| {
                for a in lib.appearances.iter().filter(|a| a.category == c) {
                    ui.horizontal(|ui| {
                        swatch(ui, a);
                        if ui.button(&a.name).clicked() {
                            picked = Some(Some(a.name.clone()));
                            ui.close();
                        }
                    });
                }
            });
        }
        picked
    }

    /// The Appearance → … submenu (browser rows, the view); returns the
    /// pick (`Some(None)`: As Material / remove the override).
    pub(super) fn appearance_submenu(&self, ui: &mut egui::Ui) -> Option<Option<String>> {
        let mut out = None;
        ui.menu_button("Appearance", |ui| {
            ui.set_max_height(480.0);
            out = self.appearance_items(ui);
        });
        out
    }

    /// The Material → … submenu (body rows).
    pub(super) fn material_submenu(&self, ui: &mut egui::Ui) -> Option<Option<String>> {
        let mut out = None;
        ui.menu_button("Material", |ui| {
            ui.set_max_height(480.0);
            out = self.material_items(ui);
        });
        out
    }

    /// The view's context menu with a selection: Appearance → ….
    pub(super) fn appearance_context_menu(&mut self, ui: &mut egui::Ui) {
        if let Some(pick) = self.appearance_submenu(ui) {
            self.run(Do::Appearance(pick));
            ui.close();
        }
    }

    /// Every window of this module.
    pub(super) fn material_windows(&mut self, ctx: &egui::Context) {
        let mut todo: Vec<Do> = Vec::new();
        self.material_browser(ctx, &mut todo);
        self.appearance_browser(ctx, &mut todo);
        self.adjust_window(ctx, &mut todo);
        self.clear_window(ctx, &mut todo);
        self.styles_window(ctx, &mut todo);
        self.physical_window(ctx, &mut todo);
        for d in todo {
            self.run(d);
        }
    }

    fn target_summary(&self) -> String {
        match self.selection_summary() {
            Some(s) if !self.sel.is_empty() => s,
            _ => "Nothing selected: the part".to_owned(),
        }
    }

    fn material_browser(&mut self, ctx: &egui::Context, todo: &mut Vec<Do>) {
        if !self.mat.show_materials {
            return;
        }
        let mut open = true;
        let mut search = std::mem::take(&mut self.mat.search);
        self.layout
            .place(
                "materials",
                ctx.content_rect(),
                true,
                egui::Window::new("Material Browser")
                    .open(&mut open)
                    .default_width(420.0)
                    .resizable(true),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Search");
                    ui.text_edit_singleline(&mut search);
                });
                ui.label(RichText::new(format!("Assign to: {}", self.target_summary())).weak());
                ui.separator();
                let part = self.part();
                let r = Resolver::new(part);
                let (used, _) = part.styles_in_use();
                ui.label(RichText::new("Document Materials").strong());
                let mut doc: Vec<&materials::Material> = part.looks.materials.iter().collect();
                if doc.is_empty() {
                    ui.label(RichText::new("Generic").weak());
                }
                doc.retain(|m| matches(&search, &m.name, &m.category));
                for m in doc {
                    ui.horizontal(|ui| {
                        swatch(ui, &r.appearance(&m.appearance));
                        let star = if used.contains(&m.name) {
                            ""
                        } else {
                            "  (unused)"
                        };
                        if ui
                            .button(format!("{}{star}", m.name))
                            .on_hover_text(
                                "Assign to the selected bodies (nothing selected: the part)",
                            )
                            .clicked()
                        {
                            todo.push(Do::PartMaterial(Some(m.name.clone())));
                        }
                        ui.label(RichText::new(format!("{:.3} g/cm³", m.density)).weak());
                    });
                }
                ui.separator();
                ui.label(RichText::new("Library").strong());
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        let lib = &self.mat.lib;
                        for c in categories(lib.materials.iter().map(|m| m.category.as_str())) {
                            let rows: Vec<_> = lib
                                .materials
                                .iter()
                                .filter(|m| {
                                    m.category == c && matches(&search, &m.name, &m.category)
                                })
                                .collect();
                            if rows.is_empty() {
                                continue;
                            }
                            egui::CollapsingHeader::new(c)
                                .default_open(!search.is_empty())
                                .show(ui, |ui| {
                                    egui::Grid::new(("mat_lib", c))
                                        .striped(true)
                                        .show(ui, |ui| {
                                            for m in rows {
                                                ui.horizontal(|ui| {
                                                    swatch(ui, &r.appearance(&m.appearance));
                                                    if ui.button(&m.name).clicked() {
                                                        todo.push(Do::PartMaterial(
                                                            (m.name != GENERIC)
                                                                .then(|| m.name.clone()),
                                                        ));
                                                    }
                                                });
                                                ui.label(format!("{:.3} g/cm³", m.density));
                                                ui.label(
                                                    m.yield_strength
                                                        .map_or_else(String::new, |v| {
                                                            format!("Re {v} MPa")
                                                        }),
                                                );
                                                ui.label(
                                                    m.youngs_modulus
                                                        .map_or_else(String::new, |v| {
                                                            format!("E {v} GPa")
                                                        }),
                                                );
                                                ui.end_row();
                                            }
                                        });
                                });
                        }
                    });
                ui.separator();
                let dir = materials::user_dir()
                    .map_or_else(|| "(none)".to_owned(), |d| d.display().to_string());
                ui.label(
                    RichText::new(format!(
                        "User library: {dir}\\{} (same columns as data/materials/materials.csv)",
                        materials::MATERIALS_FILE
                    ))
                    .small()
                    .weak(),
                );
                if ui.button("Reload Library").clicked() {
                    let fresh = MaterialUi::load();
                    self.mat.lib = fresh.lib;
                    self.mat.problems = fresh.problems;
                }
            });
        self.mat.search = search;
        self.mat.show_materials = open;
    }

    fn appearance_browser(&mut self, ctx: &egui::Context, todo: &mut Vec<Do>) {
        if !self.mat.show_appearances {
            return;
        }
        let mut open = true;
        let mut search = std::mem::take(&mut self.mat.search);
        self.layout
            .place(
                "appearances",
                ctx.content_rect(),
                true,
                egui::Window::new("Appearance Browser")
                    .open(&mut open)
                    .default_width(360.0)
                    .resizable(true),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Search");
                    ui.text_edit_singleline(&mut search);
                });
                ui.label(RichText::new(format!("Assign to: {}", self.target_summary())).weak());
                if ui
                    .button("As Material")
                    .on_hover_text("Remove the override")
                    .clicked()
                {
                    todo.push(Do::Appearance(None));
                }
                ui.separator();
                ui.label(RichText::new("Document Appearances").strong());
                let (_, used) = self.part().styles_in_use();
                for a in &self.part().looks.appearances {
                    if !matches(&search, &a.name, &a.category) {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        swatch(ui, a);
                        let tail = if used.contains(&a.name) {
                            ""
                        } else {
                            "  (unused)"
                        };
                        if ui.button(format!("{}{tail}", a.name)).clicked() {
                            todo.push(Do::Appearance(Some(a.name.clone())));
                        }
                    });
                }
                ui.separator();
                ui.label(RichText::new("Library").strong());
                egui::ScrollArea::vertical()
                    .max_height(380.0)
                    .show(ui, |ui| {
                        let lib = &self.mat.lib;
                        for c in categories(lib.appearances.iter().map(|a| a.category.as_str())) {
                            let rows: Vec<_> = lib
                                .appearances
                                .iter()
                                .filter(|a| {
                                    a.category == c && matches(&search, &a.name, &a.category)
                                })
                                .collect();
                            if rows.is_empty() {
                                continue;
                            }
                            egui::CollapsingHeader::new(c)
                                .default_open(true)
                                .show(ui, |ui| {
                                    for a in rows {
                                        ui.horizontal(|ui| {
                                            swatch(ui, a);
                                            let hover = format!(
                                                "sRGB {:?}, opacity {:.2}",
                                                a.color, a.opacity
                                            );
                                            if ui.button(&a.name).on_hover_text(hover).clicked() {
                                                todo.push(Do::Appearance(Some(a.name.clone())));
                                            }
                                        });
                                    }
                                });
                        }
                    });
            });
        self.mat.search = search;
        self.mat.show_appearances = open;
    }

    fn adjust_window(&mut self, ctx: &egui::Context, todo: &mut Vec<Do>) {
        if !self.mat.show_adjust {
            return;
        }
        let mut open = true;
        let summary = self.target_summary();
        self.layout
            .place(
                "adjust",
                ctx.content_rect(),
                false,
                egui::Window::new("Adjust")
                    .open(&mut open)
                    .default_width(300.0),
            )
            .show(ctx, |ui| {
                ui.label(RichText::new(super::i18n::tr(ADJUST_PROMPT)).weak());
                ui.label(format!("Targets: {summary}"));
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Colour");
                    ui.color_edit_button_srgb(&mut self.mat.adjust_color);
                });
                ui.add(egui::Slider::new(&mut self.mat.adjust_opacity, 0.05..=1.0).text("Opacity"));
                ui.horizontal(|ui| {
                    if ui
                        .button("Apply")
                        .on_hover_text(
                            "A new Custom appearance in this document, given to the targets",
                        )
                        .clicked()
                    {
                        todo.push(Do::Custom(self.mat.adjust_color, self.mat.adjust_opacity));
                    }
                    if ui.button("Appearance Browser...").clicked() {
                        self.mat.show_appearances = true;
                    }
                });
            });
        self.mat.show_adjust = open;
    }

    fn clear_window(&mut self, ctx: &egui::Context, todo: &mut Vec<Do>) {
        if !self.mat.show_clear {
            return;
        }
        let mut open = true;
        let mut ok = false;
        let has_sel = !self.sel.is_empty();
        let summary = self.target_summary();
        self.layout
            .place(
                "clear_looks",
                ctx.content_rect(),
                false,
                egui::Window::new("Clear Appearance Overrides")
                    .open(&mut open)
                    .default_width(300.0),
            )
            .show(ctx, |ui| {
                let c = &mut self.mat.clear;
                ui.checkbox(&mut c.faces, "Faces");
                ui.checkbox(&mut c.features, "Features");
                ui.checkbox(&mut c.bodies, "Bodies");
                ui.checkbox(&mut c.part, "Part (back to As Material)");
                if ui.button("Select All").clicked() {
                    *c = ClearWhat::ALL;
                }
                ui.separator();
                ui.add_enabled(
                    has_sel,
                    egui::Checkbox::new(&mut self.mat.clear_selected_only, "Only the selection"),
                )
                .on_hover_text(summary);
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        ok = true;
                    }
                    if ui.button("Cancel").clicked() {
                        self.mat.show_clear = false;
                    }
                });
            });
        if ok {
            todo.push(Do::Clear);
            open = false;
        }
        self.mat.show_clear = open && self.mat.show_clear;
    }

    fn styles_window(&mut self, ctx: &egui::Context, todo: &mut Vec<Do>) {
        if !self.mat.show_styles {
            return;
        }
        let mut open = true;
        self.layout
            .place(
                "styles",
                ctx.content_rect(),
                true,
                egui::Window::new("Style and Standard Editor")
                    .open(&mut open)
                    .default_width(460.0)
                    .resizable(true),
            )
            .show(ctx, |ui| {
                let part = self.part();
                let lib = &self.mat.lib;
                let (mats, looks) = part.styles_in_use();
                let location = |in_lib: bool| if in_lib { "Both" } else { "Document" };
                ui.add_enabled(false, egui::Label::new("Lighting"))
                    .on_disabled_hover_text(
                        "Lighting styles — planned (docs/FEATURES.md, section 2.15)",
                    );
                egui::CollapsingHeader::new("Material")
                    .default_open(true)
                    .show(ui, |ui| {
                        egui::Grid::new("styles_mat").striped(true).show(ui, |ui| {
                            ui.label(RichText::new("Name").strong());
                            ui.label(RichText::new("Location").strong());
                            ui.label(RichText::new("In Use").strong());
                            ui.end_row();
                            for m in &part.looks.materials {
                                ui.label(&m.name);
                                ui.label(location(lib.material(&m.name).is_some()));
                                ui.label(if mats.contains(&m.name) { "Yes" } else { "No" });
                                ui.end_row();
                            }
                        });
                    });
                egui::CollapsingHeader::new("Appearance")
                    .default_open(true)
                    .show(ui, |ui| {
                        egui::Grid::new("styles_look").striped(true).show(ui, |ui| {
                            ui.label(RichText::new("Name").strong());
                            ui.label(RichText::new("Location").strong());
                            ui.label(RichText::new("In Use").strong());
                            ui.end_row();
                            for a in &part.looks.appearances {
                                ui.horizontal(|ui| {
                                    swatch(ui, a);
                                    ui.label(&a.name);
                                });
                                ui.label(location(lib.appearance(&a.name).is_some()));
                                ui.label(if looks.contains(&a.name) { "Yes" } else { "No" });
                                ui.end_row();
                            }
                        });
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .button("Update")
                        .on_hover_text("Replace document copies by the library's versions")
                        .clicked()
                    {
                        todo.push(Do::Update);
                    }
                    if ui
                        .button("Purge")
                        .on_hover_text("Remove document styles nothing uses")
                        .clicked()
                    {
                        todo.push(Do::Purge);
                    }
                    ui.add_enabled(false, egui::Button::new("Save to Library"))
                        .on_disabled_hover_text("Planned — docs/FEATURES.md, section 2.15");
                });
            });
        self.mat.show_styles = open;
    }

    fn physical_window(&mut self, ctx: &egui::Context, todo: &mut Vec<Do>) {
        if !self.mat.show_physical {
            return;
        }
        let mut open = true;
        let key = self.physical_key();
        let kg = self.mat.kg;
        self.layout
            .place(
                "physical",
                ctx.content_rect(),
                true,
                egui::Window::new("Physical Properties")
                    .open(&mut open)
                    .default_width(520.0)
                    .resizable(true),
            )
            .show(ctx, |ui| {
                let stale = self.mat.physical.as_ref().is_none_or(|(k, _)| *k != key);
                ui.horizontal(|ui| {
                    ui.label("Requested Accuracy");
                    egui::ComboBox::from_id_salt("phys_accuracy")
                        .selected_text(self.mat.accuracy.label())
                        .show_ui(ui, |ui| {
                            for a in Accuracy::ALL {
                                ui.selectable_value(&mut self.mat.accuracy, a, a.label());
                            }
                        });
                    if ui
                        .add_enabled(stale, egui::Button::new("Update"))
                        .on_hover_text("Recalculate (enabled when the values are out of date)")
                        .clicked()
                    {
                        todo.push(Do::ComputePhysical);
                    }
                    ui.toggle_value(&mut self.mat.kg, "kg")
                        .on_hover_text("Units: g (off) or kg");
                });
                if stale {
                    ui.colored_label(Color32::from_rgb(230, 140, 0), "Out of date — Update");
                }
                let Some((_, result)) = &self.mat.physical else {
                    return;
                };
                let p = match result {
                    Ok(p) => p,
                    Err(e) => {
                        ui.colored_label(Color32::RED, e);
                        return;
                    }
                };
                for w in &p.warnings {
                    ui.colored_label(Color32::from_rgb(230, 140, 0), format!("Warning: {w}"));
                }
                ui.separator();
                let excluded = &mut self.mat.excluded;
                egui::Grid::new("phys_bodies").striped(true).show(ui, |ui| {
                    for h in ["Include", "Solid", "Material", "Density", "Mass", "Volume"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for b in &p.bodies {
                        let mut on = !excluded.contains(&b.name);
                        if ui.checkbox(&mut on, "").changed() {
                            if on {
                                excluded.retain(|n| *n != b.name);
                            } else {
                                excluded.push(b.name.clone());
                            }
                        }
                        ui.label(&b.label)
                            .on_hover_text(format!("Created by {}", b.name));
                        ui.label(&b.material);
                        ui.label(format!("{:.3} g/cm³", b.density));
                        ui.label(mass_text(b.props.mass, kg));
                        ui.label(format!("{:.3} mm³", b.props.volume));
                        ui.end_row();
                    }
                });
                let chosen: Vec<&MassProps> = p
                    .bodies
                    .iter()
                    .filter(|b| !excluded.contains(&b.name))
                    .map(|b| &b.props)
                    .collect();
                let Some(t) = MassProps::combine(chosen) else {
                    ui.label("No solid included");
                    return;
                };
                ui.separator();
                egui::Grid::new("phys_total").num_columns(2).show(ui, |ui| {
                    ui.label("Mass");
                    ui.label(RichText::new(mass_text(t.mass, kg)).strong());
                    ui.end_row();
                    ui.label("Area");
                    ui.label(format!("{:.3} mm²", t.area));
                    ui.end_row();
                    ui.label("Volume");
                    ui.label(format!("{:.3} mm³", t.volume));
                    ui.end_row();
                    ui.label("Center of Gravity");
                    ui.label(format!(
                        "X {:.4} mm, Y {:.4} mm, Z {:.4} mm",
                        t.center.x, t.center.y, t.center.z
                    ));
                    ui.end_row();
                });
                ui.separator();
                ui.label(RichText::new("Principal Moments (about the centre of gravity)").strong());
                let (vals, axes) = t.principal();
                egui::Grid::new("phys_principal")
                    .num_columns(2)
                    .show(ui, |ui| {
                        for (k, (v, a)) in vals.iter().zip(&axes).enumerate() {
                            ui.label(format!("I{}", k + 1));
                            ui.label(format!(
                                "{}   axis ({:.4}, {:.4}, {:.4})",
                                moment_text(*v, kg),
                                a.x,
                                a.y,
                                a.z
                            ));
                            ui.end_row();
                        }
                    });
                ui.label(RichText::new("Mass Moments of Inertia (COG, model axes)").strong());
                let i = &t.inertia;
                egui::Grid::new("phys_inertia")
                    .num_columns(4)
                    .show(ui, |ui| {
                        for (l1, a, l2, b) in [
                            ("Ixx", i[(0, 0)], "Ixy", i[(0, 1)]),
                            ("Iyy", i[(1, 1)], "Iyz", i[(1, 2)]),
                            ("Izz", i[(2, 2)], "Ixz", i[(0, 2)]),
                        ] {
                            ui.label(l1);
                            ui.label(moment_text(a, kg));
                            ui.label(l2);
                            ui.label(moment_text(b, kg));
                            ui.end_row();
                        }
                    });
                ui.separator();
                if ui
                    .button("Clipboard")
                    .on_hover_text("Copy the report as text")
                    .clicked()
                {
                    ui.ctx().copy_text(report(p, &t, kg));
                }
            });
        self.mat.show_physical = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The viewport colour of the default look is the old body blue, a
    /// material's look shows, surface bodies stay amber unless assigned,
    /// opacity goes to the transparent pass.
    #[test]
    fn face_colour_selection() {
        let def = Appearance::fallback();
        let p = face_paint(&def, LookSource::Material, false);
        for (got, want) in p.rgb.iter().zip([0.42, 0.56, 0.75]) {
            assert!((got - want).abs() < 0.01, "{:?}", p.rgb);
        }
        assert_eq!((p.alpha, p.two_sided), (1.0, false));
        let lib = materials::builtin();
        let red = lib.appearance("ABS - Red").unwrap();
        assert!(face_paint(red, LookSource::Face, false).rgb[0] > 0.5);
        assert_eq!(
            face_paint(red, LookSource::Material, true).rgb,
            SURFACE_AMBER
        );
        assert_ne!(face_paint(red, LookSource::Body, true).rgb, SURFACE_AMBER);
        let glass = lib.appearance("Glass - Clear").unwrap();
        assert!(face_paint(glass, LookSource::Part, false).alpha < 0.3);
    }

    /// The scene of a part with a see-through face puts that face in the
    /// transparent pass and the rest in the opaque one.
    #[test]
    fn scene_splits_opaque_and_transparent() {
        let lib = materials::builtin();
        let mut part = PartDocument::with_box("Box1", [10.0, 10.0, 10.0]).unwrap();
        let mut plain = SceneGeometry::default();
        add_part_meshes(&mut plain, &part);
        assert_eq!(plain.counts().1, 0);
        let f = part.body.find_face("Box1:Top").unwrap();
        let hint = face_hint(&part.body, f).unwrap();
        part.set_appearance(
            &LookTarget::Face {
                face: "Box1:Top".into(),
                hint,
            },
            Some("Glass - Clear"),
            lib,
        )
        .unwrap();
        let mut g = SceneGeometry::default();
        add_part_meshes(&mut g, &part);
        let (opaque, clear) = g.counts();
        assert!(clear > 0 && clear < opaque, "{opaque} {clear}");
        assert_eq!(opaque + clear, plain.counts().0);
    }
}
