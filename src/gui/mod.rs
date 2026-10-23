//! Desktop GUI: egui panels + a wgpu-rendered 3D viewport.
//!
//! Only compiled with the `gui` feature. The window follows Inventor's layout
//! so users can migrate without relearning (see `docs/UI.md`): Quick Access
//! bar and File menu, ribbon with tabs, Model Browser (left), Properties
//! (right), ViewCube, status bar, Inventor mouse profile and function keys.
//!
//! ## State
//!
//! The single source of truth is [`Project`]; the GUI edits its active part
//! ([`PartDocument`]) through `part::` operations. Every user action records
//! a snapshot in [`History`] first, which gives Undo/Redo (Ctrl+Z / Ctrl+Y).
//!
//! ## Sketch workflow (Inventor's Start 2D Sketch)
//!
//! *Start 2D Sketch* (S) → origin planes appear, faces highlight under the
//! mouse → click one → the sketch is created with projected origin and face
//! edges, the view turns to look at it, the Sketch tab opens → edit →
//! *Finish Sketch*. Sketches are listed in the browser (double-click = Edit,
//! right-click = Edit / Redefine / Visibility / Delete), and follow their
//! face when the box is resized.
//!
//! Set `CADCRAFT3D_DEMO=rectangle|slot|tangents|lines` to start with a demo
//! sketch on the box's top face.

mod alias_input;
mod blend;
mod breadcrumb;
mod browser_edit;
mod camera;
mod cmd_search;
mod coil_rib;
mod combine_split;
mod command_ui;
mod commands;
mod demos;
mod diag;
mod doc_tabs;
mod draft;
mod feature_cmd;
mod files;
mod hole;
mod hole_dims;
mod hole_panel;
mod i18n;
mod icons;
mod keys;
mod layout;
mod loft;
mod manip;
mod material;
mod measure;
mod measure_pick;
mod overlay;
mod panels;
mod pattern_dialog;
mod pattern_feature;
mod presets;
mod project;
mod renderer;
mod ribbon;
mod select;
mod shell;
mod show_dims;
mod sketch_tools;
mod sweep;
mod valuefield;
mod viewcube;
mod work;
mod work_panel;

use eframe::egui::{self, Color32, Key, PointerButton, RichText, Sense};

use crate::command::History;
use crate::document::{DocumentData, DocumentId, Project, SaveOptions};
use crate::kernel::{Point3, Real, TessellationOptions};
use crate::part::PartSketch;
use crate::part::{OriginPlane, PartDocument, PlaneRef, SketchOptions, SketchPlacement};
use crate::sketch::{Constraint, ConstraintId, EntityId, Style, Vector2};
use crate::sketch::{Point2, PointId, SolveReport, SolveStatus};
use camera::OrbitCamera;
use demos::Demo;
use renderer::{SceneGeometry, ViewportRenderer};
use ribbon::{Action, Tab};
use sketch_tools::{Align, ClickResult, Pick, SketchTools, Snap, Tool};
use viewcube::StandardView;

/// Launches the desktop application, optionally opening a project
/// (`.3dcadcraft` file or unpacked folder).
pub fn run(open: Option<std::path::PathBuf>) -> eframe::Result {
    if let Some(p) = &open {
        log::info!("open at start: {}", p.display());
    }
    let (size, maximized) = layout::startup_window();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("3DCadCraft")
            .with_inner_size(size)
            .with_maximized(maximized),
        ..Default::default()
    };
    eframe::run_native(
        "3DCadCraft",
        options,
        Box::new(move |cc| {
            let mut app = CadCraftApp::new(cc);
            app.mark_saved();
            if let Some(path) = open {
                app.open_path(&path);
            }
            Ok(Box::new(app))
        }),
    )
}

/// Mouse navigation profiles (Tools → Application Options).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum MousePreset {
    /// Inventor defaults: Shift+middle orbit, middle pan, F2/F3/F4 + left.
    Inventor,
    /// Left orbit, right/middle pan (3DCadCraft's original scheme).
    Classic,
}

impl MousePreset {
    fn label(self) -> &'static str {
        match self {
            MousePreset::Inventor => "Inventor",
            MousePreset::Classic => "Classic (left = orbit)",
        }
    }
}

/// What the Model Browser has selected; drives the Properties panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Selection {
    Project,
    Body,
    /// A feature (by index) other than the box, which is `Body`.
    Feature(usize),
    Sketch(usize),
    Document(DocumentId),
}

/// Why the user is picking a plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickPurpose {
    /// Start 2D Sketch.
    NewSketch,
    /// Redefine sketch #i.
    Redefine(usize),
    /// The sketch plane of a primitive (Box, Cylinder, Sphere, Torus).
    Primitive(crate::part::PrimitiveKind),
}

/// The point of segment `a`–`b` nearest to the line through `o` along `d`.
fn closest_on_segment(a: &Point3, b: &Point3, o: &Point3, d: &crate::kernel::Vector3) -> Point3 {
    let u = b - a;
    let w = a - o;
    let (uu, ud, dd) = (u.dot(&u), u.dot(d), d.dot(d));
    let (uw, dw) = (u.dot(&w), d.dot(&w));
    let den = uu * dd - ud * ud;
    let s = if den.abs() < 1e-12 * uu.max(1e-12) * dd.max(1e-12) {
        0.0
    } else {
        ((ud * dw - dd * uw) / den).clamp(0.0, 1.0)
    };
    a + u * s
}

/// Interaction mode of the viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Normal,
    PickPlane(PickPurpose),
}

/// The options of a feature command in progress.
#[derive(Clone, Debug)]
enum DraftDef {
    Extrude(crate::part::ExtrudeFeature),
    Revolve(crate::part::RevolveFeature),
}

impl DraftDef {
    fn profiles(&self) -> &Vec<crate::part::ProfileRef> {
        match self {
            DraftDef::Extrude(e) => &e.profiles,
            DraftDef::Revolve(r) => &r.profiles,
        }
    }

    fn profiles_mut(&mut self) -> &mut Vec<crate::part::ProfileRef> {
        match self {
            DraftDef::Extrude(e) => &mut e.profiles,
            DraftDef::Revolve(r) => &mut r.profiles,
        }
    }

    /// Inventor's command name.
    fn command(&self) -> &'static str {
        match self {
            DraftDef::Extrude(_) => "Extrude",
            DraftDef::Revolve(_) => "Revolve",
        }
    }
}

/// The Extrude / Revolve command in progress: its profile sketch, chosen
/// regions and options (Inventor's feature dialog).
#[derive(Clone, Debug)]
struct ExtrudeDraft {
    sketch: usize,
    /// All options; id, name and sketch are assigned on OK.
    def: DraftDef,
    /// Revolve: clicks pick the axis (Inventor's Axis selector) instead of
    /// profiles; also set while no axis has been chosen.
    axis_pick: bool,
    /// Revolve: an axis has been chosen.
    axis_set: bool,
    /// The sketch was made for this command (Press/Pull on a face): Cancel
    /// removes it again.
    temp_sketch: bool,
    /// Edit Feature of this feature index (OK replaces it in place).
    edit: Option<usize>,
    /// The operation seen last frame: a switch to Cut / Intersect turns the
    /// direction into the material (Inventor), see `sync_extrude_direction`.
    last_op: Option<crate::part::Operation>,
    /// The direction was turned by that, not by the user.
    auto_flipped: bool,
    /// The operation follows Predict Boolean Operation (until the user
    /// picks one).
    op_auto: bool,
    /// The inputs the prediction was made for.
    predicted_for: Option<String>,
    /// The panel's breadcrumb (`Sketch1 > Extrusion1`).
    crumbs: breadcrumb::BreadcrumbState,
    /// Made by a primitive command (Box, Cylinder, Sphere, Torus).
    primitive: Option<crate::part::PrimitiveKind>,
}

impl ExtrudeDraft {
    /// A new command on sketch `sketch` with `def` (no edit, axis unset).
    fn new(sketch: usize, def: DraftDef) -> Self {
        Self {
            sketch,
            def,
            axis_pick: false,
            axis_set: false,
            temp_sketch: false,
            edit: None,
            last_op: None,
            auto_flipped: false,
            op_auto: true,
            predicted_for: None,
            crumbs: breadcrumb::BreadcrumbState::at(1),
            primitive: None,
        }
    }
}

/// Something in the model that can be picked in the view.
/// Select Other (Inventor): with the cursor resting where several things
/// overlap, a small control cycles through them.
#[derive(Clone, Debug)]
struct SelectOther {
    /// Where the cursor rested.
    at: egui::Pos2,
    /// Candidates (every category that passes the filters), the default
    /// pick first.
    items: Vec<select::SelItem>,
    /// The current one.
    index: usize,
}

/// Picking data of the body, built once per body (a part after Booleans
/// has many thousands of facet faces: testing them one by one each frame,
/// with a fresh tessellation, made the view crawl).
pub(super) struct PickCache {
    /// Every triangle of every face.
    tris: Vec<(crate::kernel::FaceId, [Point3; 3])>,
    /// Faces using each edge.
    edge_faces: std::collections::HashMap<crate::kernel::EdgeId, Vec<crate::kernel::FaceId>>,
}

impl PickCache {
    fn build(
        body: &crate::kernel::BRep,
        hidden: &std::collections::HashSet<crate::kernel::FaceId>,
    ) -> Self {
        let opts = TessellationOptions::default();
        let mut tris = Vec::new();
        let mut edge_faces: std::collections::HashMap<_, Vec<_>> = Default::default();
        for (fid, face) in body.faces() {
            if hidden.contains(&fid) {
                continue;
            }
            if let Ok(mesh) = body.face_mesh(fid, &opts) {
                for t in &mesh.triangles {
                    tris.push((
                        fid,
                        [
                            mesh.positions[t[0] as usize],
                            mesh.positions[t[1] as usize],
                            mesh.positions[t[2] as usize],
                        ],
                    ));
                }
            }
            for lp in std::iter::once(&face.outer).chain(&face.inner) {
                for ce in &lp.coedges {
                    let v = edge_faces.entry(ce.edge).or_default();
                    if !v.contains(&fid) {
                        v.push(fid);
                    }
                }
            }
        }
        Self { tris, edge_faces }
    }

    /// Every face the ray `o + t·d` (t > 0) hits, nearest first, with t.
    pub(super) fn hits(
        &self,
        o: &Point3,
        d: &crate::kernel::Vector3,
    ) -> Vec<(crate::kernel::FaceId, f64)> {
        let mut out: Vec<(crate::kernel::FaceId, f64)> = Vec::new();
        for (f, [a, b, c]) in &self.tris {
            // Möller–Trumbore.
            let (e1, e2) = (b - a, c - a);
            let p = d.cross(&e2);
            let det = e1.dot(&p);
            if det.abs() < 1e-14 {
                continue;
            }
            let inv = 1.0 / det;
            let s = o - a;
            let u = s.dot(&p) * inv;
            if !(-1e-9..=1.0 + 1e-9).contains(&u) {
                continue;
            }
            let q = s.cross(&e1);
            let v = d.dot(&q) * inv;
            if v < -1e-9 || u + v > 1.0 + 1e-9 {
                continue;
            }
            let t = e2.dot(&q) * inv;
            if t <= 0.0 {
                continue;
            }
            match out.iter_mut().find(|(g, _)| g == f) {
                Some(x) => x.1 = x.1.min(t),
                None => out.push((*f, t)),
            }
        }
        out.sort_by(|x, y| x.1.total_cmp(&y.1));
        out
    }

    /// The nearest face hit by the ray.
    pub(super) fn pick(
        &self,
        o: &Point3,
        d: &crate::kernel::Vector3,
    ) -> Option<(crate::kernel::FaceId, f64)> {
        let mut best: Option<(crate::kernel::FaceId, f64)> = None;
        for (f, t) in self.hits(o, d) {
            if best.is_none_or(|(_, bt)| t < bt) {
                best = Some((f, t));
            }
        }
        best
    }

    /// Faces using edge `e`.
    pub(super) fn faces_of(&self, e: crate::kernel::EdgeId) -> Vec<crate::kernel::FaceId> {
        self.edge_faces.get(&e).cloned().unwrap_or_default()
    }
}

/// Visible model edges with their sampled points.
type EdgePolylines = std::rc::Rc<Vec<(crate::kernel::EdgeId, Vec<Point3>)>>;

/// A drawn handle of an interpolation spline.
struct SplineGrip {
    spline: EntityId,
    /// Index of the fit point in the spline's point list.
    k: usize,
    base: Point2,
    grip: Point2,
    set: bool,
}

/// Dynamic Input (Inventor's heads-up display): value fields next to the
/// cursor while drawing. Typed values lock the geometry and become
/// dimensions.
#[derive(Clone, Debug, Default)]
struct Hud {
    /// (tool, clicks) the fields belong to; a change resets them.
    key: Option<(Tool, usize)>,
    /// Locked values (mm, or degrees for angles).
    vals: [Option<f64>; 2],
    /// The field typing goes to.
    focus: usize,
    /// Text being typed into the focused field.
    text: String,
    /// Tab (+1) / Shift+Tab (−1) presses taken from the raw input.
    tab: i32,
}

/// Whose values a manipulator edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ManipOwner {
    /// The open Extrude / Revolve dialog.
    Draft,
    /// Feature `k` selected in the browser.
    Feature(usize),
    /// The open Work Plane / Axis / Point dialog.
    Work,
    /// The open Shell dialog (thickness arrow).
    Shell,
    /// The open Hole dialog (arrows of the centres, `hole_dims`).
    Hole,
    /// The open Fillet / Chamfer dialog (size arrows, `blend_handles`).
    Blend,
}

impl ManipOwner {
    /// A dialog's handle: its value follows the drag live.
    fn live(self) -> bool {
        matches!(
            self,
            ManipOwner::Draft | ManipOwner::Shell | ManipOwner::Hole | ManipOwner::Blend
        )
    }
}

/// Edge polylines of an Extrude preview, or why it cannot be built.
type PreviewResult = Result<Vec<Vec<Point3>>, String>;

/// The preview's "error" while it is computed on its worker thread.
pub(super) const PREVIEW_PENDING: &str = "Computing the preview...";

/// Which Extrude input a click in the graphics window sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TargetSlot {
    /// The start face (From / Between).
    From,
    /// The termination (To).
    To,
}

/// Whose target is being picked: the open Extrude dialog or feature `k`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TargetOwner {
    Draft,
    Feature(usize),
}

/// Top-level application state.
struct CadCraftApp {
    project: Project,
    /// The part being edited (always a Part document of `project`).
    part_id: DocumentId,
    /// Empty stand-in returned by `part`/`part_mut` should `part_id` ever
    /// not name a part (a programming error), so the GUI never panics.
    fallback_part: PartDocument,
    history: History<Project>,
    /// Open documents (`doc_tabs`): the active one is `None` here (it lives
    /// in the fields above), the others wait in their slot.
    tabs: Vec<Option<doc_tabs::DocTab>>,
    /// Index of the active document in `tabs`.
    active_tab: usize,
    /// Tabs whose changes the quit was told to drop (Don't Save).
    quit_dropped: Vec<usize>,
    /// Ctrl+Tab steps taken from the raw input this frame.
    tab_step: i32,

    /// Index of the sketch in edit mode.
    editing: Option<usize>,
    mode: Mode,
    selection: Selection,
    /// Solver report of the sketch in edit mode.
    report: Option<Result<SolveReport, String>>,
    sketch_opts: SketchOptions,
    show_labels: bool,
    /// Application Options → Display → Thread texture (cosmetic threads).
    thread_texture: bool,
    /// Application Options → Show command alias input dialog (G6).
    alias_input: bool,
    /// The open alias field (`alias_input`).
    alias: alias_input::AliasInput,
    /// View → Object Visibility (persisted with the Application Options).
    obj_vis: layout::ObjectVisibility,

    camera: OrbitCamera,
    renderer: Option<ViewportRenderer>,
    scene_dirty: bool,
    /// Regions of the edited sketch, keyed by the sketch's serialised form
    /// (recomputed only when the sketch changes).
    regions_cache: std::cell::RefCell<Option<(String, Vec<crate::sketch::Region>)>>,
    dragging: Option<PointId>,
    /// The 3D view's rectangle at the last frame (to put in-place editors
    /// over dimension texts).
    viewport_rect: egui::Rect,
    /// The Parameters window is open.
    show_params: bool,
    /// Text being typed in Parameters cells (key: column + name).
    param_buf: std::collections::HashMap<String, String>,
    /// Parameters table filter: All, User, Model, Used, Unused.
    param_filter: u8,
    /// The body the table's physical properties were measured on.
    param_phys_key: String,
    /// Parameters table: text size, column widths (persisted), search text.
    param_font: f32,
    param_cols: [f32; 3],
    param_search: String,
    /// Multi-Value list being edited: user parameter index, text.
    param_values_edit: Option<(usize, String)>,
    /// The Suppress if window: feature index, condition text.
    suppress_if_edit: Option<(usize, String)>,
    /// The in-place dimension editor shows its tolerance part.
    dim_tol_open: bool,
    /// How dimension texts are shown (status bar: Value / Name / …).
    dim_display: crate::sketch::DimDisplay,
    /// The last command started, for Repeat.
    last_command: Option<Action>,
    /// Registry ids of the recently run commands, newest first
    /// (`commands::remember`; saved in `settings.json` → `options`).
    recent_commands: Vec<String>,
    /// The command search field (Alt+S, `cmd_search`).
    cmd_search: cmd_search::CmdSearch,
    /// The Extrude command, while its dialog is open.
    extrude: Option<ExtrudeDraft>,
    /// An Extrude / Revolve waiting while its sketch is edited (breadcrumb).
    suspended_draft: Option<ExtrudeDraft>,
    /// A primitive being placed (clicks in its sketch).
    prim_draft: Option<feature_cmd::PrimDraft>,
    /// Advanced Settings of the feature panels.
    feature_settings: feature_cmd::FeatureSettings,
    /// Picking a face or vertex for an extrusion's From / To.
    target_pick: Option<(TargetOwner, TargetSlot)>,
    /// Preview of the Extrude dialog: key, edge polylines or the error.
    extrude_preview: std::cell::RefCell<Option<(String, PreviewResult)>>,
    /// The preview being computed on a worker thread (the exact kernel can
    /// take seconds; computing it in the frame froze the UI).
    preview_job: std::cell::RefCell<Option<(String, std::sync::mpsc::Receiver<PreviewResult>)>>,
    /// The part a preview built (key, signature of the part it started
    /// from, result): OK takes it instead of building the feature again on
    /// the UI thread (a pattern took seconds and looked like it did nothing).
    preview_part: std::sync::Arc<std::sync::Mutex<Option<(String, u64, PartDocument)>>>,
    /// Plane under the cursor when the viewport context menu opened.
    context_target: Option<PlaneRef>,

    file_path: String,
    /// The open file dialog (Open / Save As / Export).
    file_dialog: Option<files::FileDialog>,
    /// Signature of the project when last saved or opened.
    saved_sig: u64,
    /// (time checked, dirty) — "unsaved changes", recomputed once a second.
    dirty_cache: (f64, bool),
    /// "Save changes?" is open for this.
    unsaved_prompt: Option<files::Pending>,
    /// What runs once the save the prompt asked for succeeds.
    after_save: Option<files::Pending>,
    /// The window may close now (saved, or changes discarded).
    close_ok: bool,
    /// A project dropped on the window, waiting for "save changes?".
    dropped: Option<std::path::PathBuf>,
    /// The window title last set.
    window_title: String,
    /// Result of the last file operation, shown in the status bar.
    file_status: Option<Result<String, String>>,

    tab: Tab,
    mouse: MousePreset,
    invert_wheel: bool,
    show_options: bool,

    /// Sketch tools of the sketch in edit mode.
    tools: SketchTools,
    /// Line tool: dragging from a curve end (point, start position in
    /// sketch coordinates, initial drag direction once known).
    arc_drag: Option<(PointId, Point2, Option<Vector2>)>,
    /// Select tool: dragging a whole curve (entity, last cursor position).
    dragging_entity: Option<(EntityId, Point2)>,
    /// Box selection in a sketch: where the drag started (screen).
    box_select: Option<egui::Pos2>,
    /// A dimension whose text is being dragged.
    dragging_dim: Option<ConstraintId>,
    /// Degrees of freedom of the edited sketch (recomputed after each solve).
    dof: Option<crate::sketch::DofAnalysis>,
    /// Show All Degrees of Freedom (status-bar toggle, as in Inventor).
    show_dof: bool,
    /// Relax mode: dragging changes dimensions instead of fighting them.
    relax: bool,
    /// Slice Graphics (F7): the model in front of the edited sketch's plane
    /// is cut away.
    slice: bool,
    /// ViewCube → Set Current View as Home: (yaw, pitch) in degrees.
    home_view: Option<(f64, f64)>,
    /// The last right-click was on the ViewCube.
    cube_context: bool,
    /// ViewCube transitions and cube/compass drags.
    view_cube: viewcube::ViewCube,
    /// Offset Element window: sketch, pattern, element, along/across (mm).
    offset_edit: Option<(usize, crate::sketch::PatternId, usize, [f64; 2])>,
    /// A sketch-pattern handle being dragged, and the value shown.
    pattern_drag: Option<(sketch_tools::PatGrip, String)>,
    /// A manipulator being dragged: whose, the handle, its current value.
    manip_drag: Option<(ManipOwner, manip::Handle, f64)>,
    /// What the Hole arrow being dragged moves (`hole_dims`).
    hole_drag: Option<hole_dims::HoleHandle>,
    /// Measure into a field (the `>` menu, Edit Dimension), `measure_pick`.
    measure_pick: Option<measure_pick::MeasurePick>,
    /// Show Dimensions for a field (the `>` menu), `show_dims`.
    show_dims: Option<show_dims::ShowDims>,
    /// What a right-click inside the edited sketch hit.
    sketch_context: Option<SketchCtx>,
    /// Where the last right-click hit the sketch plane (sketch coordinates).
    context_point: Option<Point2>,
    /// Splines showing their curvature comb.
    combs: std::collections::HashSet<EntityId>,
    /// The model face selected in the view (persistent name) and where the
    /// click was (for the contextual toolbar).
    face_sel: Option<(String, egui::Pos2)>,
    /// Select Other control, and where/since when the cursor rests.
    select_other: Option<SelectOther>,
    rest: Option<(egui::Pos2, f64)>,
    /// The model edge selected in the view and where the click was.
    edge_sel: Option<(crate::kernel::EdgeId, egui::Pos2)>,
    /// Select Priority, the user filter, box mode and prehighlight (Quick
    /// Access Toolbar, Shift + right-click; persisted in `AppOptions`).
    select: crate::command::selection::SelectSettings,
    /// What may be picked this frame: the active selector's kinds ∩ the
    /// user filter (`update_pick_filter`, start of `viewport`).
    pick_filter: std::cell::Cell<crate::command::selection::Effective>,
    /// The open Shift + right-click selection menu: where, the view and its
    /// content (fixed when it opened).
    sel_menu: Option<(
        egui::Pos2,
        egui::Rect,
        crate::command::selection::MenuContext,
    )>,
    /// It opened this frame (the opening press must not close it).
    sel_menu_fresh: bool,
    /// The right button went down with Shift (the selection menu, never a
    /// pan or the context menu).
    shift_rmb: bool,
    /// The selected set (several faces, edges, features, bodies, sketches).
    sel: Vec<select::SelItem>,
    /// The set before the last change (Previous Selection).
    prev_sel: Vec<select::SelItem>,
    /// Body signature the set was made on (stale items are dropped).
    sel_key: String,
    /// The selection and the prehighlighted item the scene was built with
    /// (selected faces are drawn in the 3D scene, with depth).
    scene_sel: (Vec<select::SelItem>, Option<select::SelItem>),
    /// Sampled visible edges of the body, keyed by a body signature.
    edge_cache: std::cell::RefCell<Option<(String, EdgePolylines)>>,
    /// Picking data of the body (see [`PickCache`]).
    pick_cache: std::cell::RefCell<Option<(String, std::rc::Rc<PickCache>)>>,
    /// `body_key` of this frame: (caching on, key). `ui` turns it on and
    /// clears the key when a frame starts (a body rebuilt during the frame
    /// is seen from the next one); off outside frames (tests).
    body_key_frame: std::cell::RefCell<(bool, Option<String>)>,
    /// The open Fillet / Chamfer command.
    blend_draft: Option<blend::BlendDraft>,
    /// The open Shell command.
    shell_draft: Option<shell::ShellDraft>,
    /// The open Sweep command.
    sweep_draft: Option<sweep::SweepDraft>,
    /// The open Loft command.
    loft_draft: Option<sweep::LoftDraft>,
    /// The open Coil command.
    coil_draft: Option<coil_rib::CoilDraft>,
    /// The open Rib command.
    rib_draft: Option<coil_rib::RibDraft>,
    /// Presets of the feature commands (the Preset row).
    presets: presets::PresetUi,
    /// The open Face Draft command.
    draft_draft: Option<draft::DraftDraft>,
    /// The open Combine command.
    combine_draft: Option<combine_split::CombineDraft>,
    /// The open Split command.
    split_draft: Option<combine_split::SplitDraft>,
    /// The open Rectangular / Circular Pattern or Mirror command.
    pattern_draft: Option<pattern_feature::PatternDraft>,
    /// The open Hole command.
    hole_draft: Option<hole::HoleDraft>,
    /// The Hole whose sketch was opened from its panel: Finish Sketch
    /// reopens the panel (feature id).
    hole_return: Option<String>,
    /// Project Geometry is active in the open sketch.
    project_geometry: bool,
    /// Replace Reference armed: (sketch, link index); the next Project
    /// Geometry pick replaces that link's source.
    relink: Option<(usize, usize)>,
    /// The open Thread command.
    thread_draft: Option<hole::ThreadDraft>,
    /// The open Work Plane / Axis / Point command.
    work_draft: Option<work::WorkDraft>,
    /// A feature deletion waiting for confirmation, with what it breaks.
    confirm_delete: Option<(usize, Vec<(String, String)>)>,
    /// The options of the Delete Features dialog.
    delete_opts: crate::part::history::DeleteOptions,
    /// The browser row being renamed (F2 / slow double-click).
    rename: Option<browser_edit::RenameTarget>,
    /// The feature under the pointer in the browser (highlighted in the
    /// view), by id.
    browser_hover: Option<String>,
    /// The References row under the pointer: (sketch, its points).
    ref_hover: Option<(usize, Vec<PointId>)>,
    /// Dynamic Input fields.
    hud: Hud,
    /// The last value typed into each HUD field (Up recalls it).
    hud_last: std::collections::HashMap<&'static str, f64>,
    /// The last cursor pick in the edited sketch (for Enter in the HUD).
    last_pick: Option<Pick>,
    /// A spline end handle being dragged: (spline, at the end?).
    handle_drag: Option<(EntityId, usize)>,
    /// Constraint row hovered in the Properties list (highlighted in the window).
    list_hover: Option<ConstraintId>,
    /// Materials, appearances, Physical Properties (`material.rs`).
    mat: material::MaterialUi,
    /// Inspect → Measure window.
    measure: measure::MeasureUi,
    /// Journal, autosave and Restore (`diag.rs`).
    diag: diag::DiagUi,
    /// Panels, floating windows, User Interface toggles (`layout.rs`).
    layout: layout::LayoutUi,
    /// Full Screen to send to the window this frame.
    full_screen_cmd: Option<bool>,
}

/// Target of a right-click inside the edited sketch.
#[derive(Clone, Copy, Debug, PartialEq)]
enum SketchCtx {
    Constraint(ConstraintId),
    Entity(EntityId),
    /// A projected (reference) point.
    Point(PointId),
    /// A point of the sketch itself (Center Point / normal point).
    OwnPoint(PointId),
}

impl CadCraftApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        i18n::load();
        let mut project = Project::new("Untitled");
        // A new part starts empty, as in Inventor (sketch on an origin plane).
        let part = PartDocument::default();
        let part_id = project.add_document("Part1", DocumentData::Part(part));
        let mut app = Self {
            project,
            part_id,
            fallback_part: PartDocument::default(),
            history: History::default(),
            tabs: vec![None],
            active_tab: 0,
            quit_dropped: Vec::new(),
            tab_step: 0,
            editing: None,
            mode: Mode::Normal,
            selection: Selection::Body,
            report: None,
            sketch_opts: SketchOptions::default(),
            show_labels: true,
            thread_texture: true,
            alias_input: false,
            alias: Default::default(),
            obj_vis: Default::default(),
            camera: OrbitCamera::default(),
            renderer: cc.wgpu_render_state.as_ref().map(ViewportRenderer::new),
            scene_dirty: true,
            regions_cache: Default::default(),
            dragging: None,
            extrude: None,
            suspended_draft: None,
            prim_draft: None,
            feature_settings: Default::default(),
            last_command: None,
            recent_commands: Vec::new(),
            cmd_search: Default::default(),
            dim_display: Default::default(),
            viewport_rect: egui::Rect::NOTHING,
            dim_tol_open: false,
            show_params: false,
            param_buf: Default::default(),
            param_filter: 0,
            param_phys_key: String::new(),
            param_font: 1.0,
            param_cols: [80.0, 150.0, 120.0],
            param_search: String::new(),
            param_values_edit: None,
            suppress_if_edit: None,
            target_pick: None,
            extrude_preview: Default::default(),
            preview_job: Default::default(),
            preview_part: Default::default(),
            context_target: None,
            file_path: String::new(),
            file_dialog: None,
            saved_sig: 0,
            dirty_cache: (f64::NEG_INFINITY, false),
            unsaved_prompt: None,
            after_save: None,
            close_ok: false,
            dropped: None,
            window_title: String::new(),
            file_status: None,
            tab: Tab::Model,
            mouse: MousePreset::Inventor,
            invert_wheel: false,
            show_options: false,
            tools: SketchTools::default(),
            arc_drag: None,
            dragging_entity: None,
            dragging_dim: None,
            dof: None,
            show_dof: false,
            relax: false,
            slice: false,
            home_view: None,
            cube_context: false,
            view_cube: Default::default(),
            manip_drag: None,
            hole_drag: None,
            measure_pick: None,
            show_dims: None,
            pattern_drag: None,
            context_point: None,
            combs: Default::default(),
            handle_drag: None,
            hud: Hud::default(),
            hud_last: Default::default(),
            face_sel: None,
            edge_sel: None,
            select: Default::default(),
            pick_filter: std::cell::Cell::new(crate::command::selection::effective(
                None,
                crate::command::selection::Filter::all(),
            )),
            sel_menu: None,
            sel_menu_fresh: false,
            shift_rmb: false,
            sel: Vec::new(),
            prev_sel: Vec::new(),
            sel_key: String::new(),
            scene_sel: (Vec::new(), None),
            select_other: None,
            rest: None,
            edge_cache: std::cell::RefCell::new(None),
            pick_cache: std::cell::RefCell::new(None),
            body_key_frame: std::cell::RefCell::new((false, None)),
            ref_hover: None,
            confirm_delete: None,
            delete_opts: Default::default(),
            rename: None,
            browser_hover: None,
            work_draft: None,
            blend_draft: None,
            shell_draft: None,
            sweep_draft: None,
            loft_draft: None,
            coil_draft: None,
            rib_draft: None,
            draft_draft: None,
            combine_draft: None,
            split_draft: None,
            pattern_draft: None,
            presets: Default::default(),
            hole_draft: None,
            hole_return: None,
            project_geometry: false,
            relink: None,
            thread_draft: None,
            last_pick: None,
            box_select: None,
            offset_edit: None,
            sketch_context: None,
            list_hover: None,
            mat: material::MaterialUi::load(),
            measure: Default::default(),
            diag: diag::DiagUi::new(),
            layout: layout::LayoutUi::default(),
            full_screen_cmd: None,
        };
        let (lay, options) = layout::LayoutUi::load(files::settings_path());
        app.layout = lay;
        app.apply_options(options);
        // Ctrl+0 is Clean Screen (Inventor), not egui's zoom reset.
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        // Developer aid: start with a demo sketch, e.g. CADCRAFT3D_DEMO=slot.
        if let Some(demo) = std::env::var("CADCRAFT3D_DEMO")
            .ok()
            .and_then(|n| Demo::from_name(&n))
        {
            app.load_demo(demo);
            app.history.clear();
        }
        app.fit_view();
        app
    }

    // ---- model access ------------------------------------------------------

    fn part(&self) -> &PartDocument {
        match self.project.document(self.part_id).map(|d| &d.data) {
            Some(DocumentData::Part(p)) => p,
            _ => &self.fallback_part,
        }
    }

    fn part_mut(&mut self) -> &mut PartDocument {
        match self.project.document_mut(self.part_id).map(|d| &mut d.data) {
            Some(DocumentData::Part(p)) => p,
            _ => &mut self.fallback_part,
        }
    }

    /// Records the state before an action named `label` (for Undo).
    fn checkpoint(&mut self, label: &str) {
        self.history.record(label, self.project.clone());
        self.diag.checkpoint(label);
    }

    /// Re-validates UI state after the project was replaced (undo, open).
    fn after_restore(&mut self) {
        let n = self.part().sketches.len();
        if self.editing.is_some_and(|i| i >= n) {
            self.editing = None;
        }
        if self.editing.is_none() && self.tab == Tab::Sketch {
            self.tab = Tab::Model;
        }
        if matches!(self.selection, Selection::Sketch(i) if i >= n) {
            self.selection = Selection::Body;
        }
        self.mode = Mode::Normal;
        self.dragging = None;
        self.scene_dirty = true;
        self.solve();
    }

    fn undo(&mut self) {
        let label = self.history.undo_label().map(str::to_owned);
        if let Some(p) = self.history.undo(self.project.clone()) {
            self.diag.undo_redo("undo", label);
            self.project = p;
            self.after_restore();
        }
    }

    fn redo(&mut self) {
        let label = self.history.redo_label().map(str::to_owned);
        if let Some(p) = self.history.redo(self.project.clone()) {
            self.diag.undo_redo("redo", label);
            self.project = p;
            self.after_restore();
        }
    }

    // ---- box ---------------------------------------------------------------

    fn box_size(&self) -> [Real; 3] {
        self.part()
            .features
            .iter()
            .find_map(|f| match f {
                crate::part::Feature::Box(b) => Some(b.size),
                _ => None,
            })
            .or_else(|| {
                self.part()
                    .body
                    .bounding_box()
                    .map(|(lo, hi)| (hi - lo).into())
            })
            .unwrap_or([100.0; 3])
    }

    // ---- sketches ----------------------------------------------------------

    /// Applies a picked plane for the current pick purpose.
    fn plane_picked(&mut self, plane: PlaneRef) {
        let purpose = match self.mode {
            Mode::PickPlane(p) => p,
            Mode::Normal => return,
        };
        self.mode = Mode::Normal;
        let opts = self.sketch_opts.clone();
        match purpose {
            PickPurpose::Primitive(kind) => self.primitive_plane(kind, plane),
            PickPurpose::NewSketch => {
                let name = self.part().next_sketch_name();
                self.checkpoint(&format!("Create {name}"));
                let placement = SketchPlacement {
                    plane,
                    ..Default::default()
                };
                match self.part_mut().create_sketch(placement, &opts) {
                    Ok(i) => self.edit_sketch(i),
                    Err(e) => self.file_status = Some(Err(e.to_string())),
                }
            }
            PickPurpose::Redefine(i) => {
                self.checkpoint("Redefine Sketch");
                if let Err(e) = self.part_mut().redefine_sketch(i, plane, &opts) {
                    self.file_status = Some(Err(e.to_string()));
                }
                self.solve();
            }
        }
    }

    /// Enters edit mode for sketch `i` (Edit Sketch / after creation).
    fn edit_sketch(&mut self, i: usize) {
        if i >= self.part().sketches.len() {
            return;
        }
        self.editing = Some(i);
        self.selection = Selection::Sketch(i);
        self.tab = Tab::Sketch;
        self.tools.set_tool(Tool::Select);
        self.part_mut().sketches[i].visible = true;
        if self.sketch_opts.look_at_on_create {
            self.look_at_sketch(i);
        }
        self.solve();
    }

    /// Finish Sketch.
    fn finish_sketch(&mut self) {
        // Sketches that project from this one follow it now (Inventor
        // updates projections when the source sketch is finished).
        if let Some(i) = self.editing
            && let Some(id) = self.part().sketches.get(i).map(|s| s.id.clone())
            && !self.part().link_dependents(&id).is_empty()
        {
            let _ = self.part_mut().regenerate();
            self.scene_dirty = true;
        }
        self.project_geometry = false;
        self.relink = None;
        self.slice = false;
        // The features using the sketch follow it now (Inventor updates the
        // model on Finish Sketch). Without this the body stayed as it was
        // until Rebuild All, and the session journal recorded that stale
        // body after every edit in the sketch (the replay then "differed").
        if self.editing.take().is_some() {
            if let Err(e) = self.part_mut().regenerate() {
                self.file_status = Some(Err(e.to_string()));
            }
            self.scene_dirty = true;
        }
        self.dragging = None;
        self.arc_drag = None;
        self.tools.set_tool(Tool::Select);
        self.tools.dim_edit = None;
        self.tools.over_prompt = None;
        self.report = None;
        self.tab = Tab::Model;
        // Breadcrumb: back to the command that opened the sketch.
        self.resume_suspended_draft();
        self.resume_hole();
    }

    fn look_at_sketch(&mut self, i: usize) {
        let Some(ps) = self.part().sketches.get(i) else {
            return;
        };
        let plane = ps.sketch.plane.clone();
        let (center, radius) = match self.part().body.bounding_box() {
            Some((lo, hi)) => {
                let c = nalgebra::center(&lo, &hi);
                let uv = plane.project(&c);
                (
                    plane.point_at(uv.x, uv.y),
                    nalgebra::distance(&lo, &hi) / 2.0,
                )
            }
            None => (plane.origin, 50.0),
        };
        self.camera.look_at(&plane, center, radius);
    }

    /// Edit Pattern dialog OK: rebuilds the pattern with the new values.
    fn commit_edit_pattern(&mut self) {
        let (Some(i), Some((id, base))) = (self.editing, self.tools.edit_pattern) else {
            return;
        };
        let kind = self.tools.pattern_kind_from_fields(&base);
        self.checkpoint("Edit Pattern");
        let counts = std::mem::take(&mut self.tools.pattern_count_eq);
        let sk = &mut self.part_mut().sketches[i].sketch;
        match sk
            .edit_pattern(id, kind)
            .and_then(|()| sk.set_pattern_count_exprs(id, counts))
        {
            Ok(()) => {
                self.tools.edit_pattern = None;
                self.file_status = None;
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("Edit Pattern: {e}")));
            }
        }
        self.solve();
    }

    /// The last command, if it can run now (sketch tools need a sketch).
    fn repeatable(&self) -> Option<Action> {
        self.last_command
            .filter(|a| !matches!(a, Action::Tool(_)) || self.editing.is_some())
    }

    /// F1: opens the specification of what the user is doing
    /// (`docs/spec/…`), as Inventor's F1 opens the help of the command.
    fn open_help(&mut self) {
        let file = if self.measure.open {
            "0.10-physical.md"
        } else if let Some(f) = self.mat.help_file() {
            f
        } else if self.work_draft.is_some()
            || matches!(self.selection, Selection::Feature(k)
                if matches!(self.part().features.get(k), Some(crate::part::Feature::Work(_))))
        {
            "2.10-work-features.md"
        } else if self.sweep_draft.is_some()
            || matches!(self.selection, Selection::Feature(k)
                if matches!(self.part().features.get(k), Some(crate::part::Feature::Sweep(_))))
        {
            "5.1-sweep.md"
        } else if self.loft_draft.is_some()
            || matches!(self.selection, Selection::Feature(k)
                if matches!(self.part().features.get(k), Some(crate::part::Feature::Loft(_))))
        {
            "5.2-loft.md"
        } else if self.coil_draft.is_some()
            || matches!(self.selection, Selection::Feature(k)
                if matches!(self.part().features.get(k), Some(crate::part::Feature::Coil(_))))
        {
            "5.3-coil.md"
        } else if self.rib_draft.is_some()
            || matches!(self.selection, Selection::Feature(k)
                if matches!(self.part().features.get(k), Some(crate::part::Feature::Rib(_))))
        {
            "2.7-rib.md"
        } else if self.shell_draft.is_some() {
            "2.6-shell.md"
        } else if self.draft_draft.is_some() {
            "2.8-draft.md"
        } else if self.combine_draft.is_some()
            || self.split_draft.is_some()
            || matches!(self.selection, Selection::Feature(k)
                if matches!(self.part().features.get(k),
                    Some(crate::part::Feature::Combine(_) | crate::part::Feature::Split(_))))
        {
            "2.13-combine-split.md"
        } else if let Some(d) = &self.pattern_draft {
            match d.def.kind {
                crate::part::PatternKind::Mirror { .. } => "2.12-mirror.md",
                _ => "2.11-patterns.md",
            }
        } else if self.hole_draft.is_some()
            || matches!(self.selection, Selection::Feature(k)
                if matches!(self.part().features.get(k), Some(crate::part::Feature::Hole(_))))
        {
            "2.3-hole.md"
        } else if self.thread_draft.is_some()
            || matches!(self.selection, Selection::Feature(k)
                if matches!(self.part().features.get(k), Some(crate::part::Feature::Thread(_))))
        {
            "2.9-thread.md"
        } else if let Some(d) = &self.blend_draft {
            if d.def.command() == "Chamfer" {
                "2.5-chamfer.md"
            } else {
                "2.4-fillet.md"
            }
        } else if let Some(d) = &self.extrude {
            match d.def {
                DraftDef::Extrude(_) => "2.1-extrude.md",
                DraftDef::Revolve(_) => "2.2-revolve.md",
            }
        } else if self.editing.is_some() && self.project_geometry {
            "1.8-project-geometry.md"
        } else if self.editing.is_some() {
            // The tool's spec, from the command registry.
            commands::by_action(Action::Tool(self.tools.tool))
                .and_then(|c| c.spec)
                .unwrap_or("1.2-sketch-geometry.md")
        } else if matches!(self.mode, Mode::PickPlane(_)) {
            "1.1-sketch.md"
        } else {
            "0-ui.md"
        };
        // Native separators: Explorer does not open `C:\x/docs/spec/a.md`.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("docs")
            .join("spec")
            .join(file);
        let opener = if cfg!(target_os = "windows") {
            "explorer"
        } else if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        if let Err(e) = std::process::Command::new(opener).arg(&path).spawn() {
            self.file_status = Some(Err(format!("Help: cannot open {}: {e}", path.display())));
        }
    }

    // ---- features ----------------------------------------------------------

    /// **Extrude**: opens the dialog on the edited / selected / last sketch
    /// with closed regions that is free or visible (a consumed sketch is
    /// offered only when shown, as in Inventor); a single region is
    /// pre-selected.
    fn start_extrude(&mut self) {
        self.start_feature(false);
    }

    /// **Revolve**: like Extrude; the axis is the sketch's only centerline,
    /// else it is picked next.
    fn start_revolve(&mut self) {
        self.start_feature(true);
    }

    fn start_feature(&mut self, revolve: bool) {
        let cmd = if revolve { "Revolve" } else { "Extrude" };
        let preferred = self.editing.or(match self.selection {
            Selection::Sketch(i) => Some(i),
            _ => None,
        });
        self.finish_sketch();
        self.mode = Mode::Normal;
        let n = self.part().sketches.len();
        let usable = |app: &Self, i: usize| {
            let part = app.part();
            let ps = &part.sketches[i];
            (ps.visible || part.consumers(&ps.id).is_empty())
                && !app.sketch_regions(&ps.sketch).is_empty()
        };
        let Some(sketch) = preferred
            .filter(|&i| i < n && usable(self, i))
            .or_else(|| (0..n).rev().find(|&i| usable(self, i)))
        else {
            self.file_status = Some(Err(format!(
                "{cmd}: no free or visible sketch has a closed profile"
            )));
            return;
        };
        let regions = self.sketch_regions(&self.part().sketches[sketch].sketch);
        let profiles =
            crate::part::extrude::default_profiles(&self.part().sketches[sketch], &regions);
        let operation = crate::part::Operation::default_for(&self.part().body);
        let mut draft = if revolve {
            let auto = crate::part::revolve::auto_axis(&self.part().sketches[sketch].sketch);
            let axis_set = auto.is_some();
            let mut def = crate::part::RevolveFeature::new(
                profiles,
                auto.unwrap_or(crate::part::AxisRef::Origin(0)),
            );
            def.operation = operation;
            let mut d = ExtrudeDraft::new(sketch, DraftDef::Revolve(def));
            d.axis_pick = !axis_set;
            d.axis_set = axis_set;
            d
        } else {
            let mut def = crate::part::ExtrudeFeature::new(profiles, 10.0);
            def.extend_face = false; // Inventor: off, on by itself when needed
            def.operation = operation;
            ExtrudeDraft::new(sketch, DraftDef::Extrude(def))
        };
        self.draft_preset_start(&mut draft.def);
        self.extrude = Some(draft);
    }

    /// **Extrude from a face** (Press/Pull): a sketch on the face with its
    /// edges projected, the face's region (under `at`) as the profile, Join.
    fn press_pull(&mut self, face: String, at: egui::Pos2) {
        self.checkpoint("Extrude Face");
        let opts = SketchOptions {
            auto_reference_edges: true,
            ..self.sketch_opts.clone()
        };
        let placement = SketchPlacement {
            plane: PlaneRef::Face(face),
            ..Default::default()
        };
        let i = match self.part_mut().create_sketch(placement, &opts) {
            Ok(i) => i,
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("Extrude: {e}")));
                return;
            }
        };
        let sk = &self.part().sketches[i].sketch;
        let regions = self.sketch_regions(sk);
        let rect = self.viewport_rect;
        let under = self
            .camera
            .ray_plane(at, rect, &sk.plane)
            .map(|w| sk.plane.project(&w));
        let region = under
            .and_then(|q| regions.iter().find(|r| r.contains(q)))
            .or_else(|| regions.iter().max_by(|a, b| a.area().total_cmp(&b.area())));
        let Some(r) = region else {
            let _ = self.part_mut().delete_sketch(i);
            self.history.cancel();
            self.file_status = Some(Err("Extrude: the face has no closed boundary".into()));
            return;
        };
        let mut def = crate::part::ExtrudeFeature::new(
            vec![crate::part::ProfileRef::of(r, r.interior_point())],
            10.0,
        );
        def.operation = crate::part::Operation::default_for(&self.part().body);
        let mut def = DraftDef::Extrude(def);
        self.draft_preset_start(&mut def);
        self.part_mut().sketches[i].visible = false;
        self.face_sel = None;
        let mut d = ExtrudeDraft::new(i, def);
        d.temp_sketch = true;
        self.extrude = Some(d);
        self.scene_dirty = true;
    }

    /// Closes the Extrude/Revolve dialog without a feature; a sketch made
    /// for it (Press/Pull) goes too.
    fn cancel_extrude(&mut self) {
        self.cancel_primitive();
        self.shell_draft = None;
        self.sweep_draft = None;
        self.loft_draft = None;
        self.coil_draft = None;
        self.rib_draft = None;
        self.draft_draft = None;
        self.close_body_cmds();
        self.pattern_draft = None;
        self.hole_draft = None;
        self.thread_draft = None;
        if let Some(d) = self.extrude.take()
            && d.temp_sketch
        {
            let _ = self.part_mut().delete_sketch(d.sketch);
            self.history.cancel();
            self.scene_dirty = true;
        }
        self.target_pick = None;
        self.end_feature_edit();
    }

    /// **Edit Feature** (browser, context menus): opens feature `k`'s
    /// dialog with its inputs; the part is rolled back to it (shows the
    /// body before it, so its edges / profiles can be picked again).
    pub(super) fn edit_feature_dialog(&mut self, k: usize) {
        use crate::part::Feature;
        let Some(f) = self.part().features.get(k).cloned() else {
            return;
        };
        self.finish_sketch();
        self.mode = Mode::Normal;
        self.cancel_extrude();
        self.blend_draft = None;
        self.work_draft = None;
        self.face_sel = None;
        self.edge_sel = None;
        self.selection = Selection::Feature(k);
        let sketch_of = |app: &Self, id: &str| app.part().sketches.iter().position(|s| s.id == id);
        match f {
            Feature::Extrude(e) => {
                let Some(sketch) = sketch_of(self, &e.sketch) else {
                    self.file_status =
                        Some(Err("Edit Feature: the sketch no longer exists".into()));
                    return;
                };
                self.roll_back_to(k);
                let mut d = ExtrudeDraft::new(sketch, DraftDef::Extrude(e));
                (d.edit, d.op_auto) = (Some(k), false);
                self.extrude = Some(d);
            }
            Feature::Revolve(r) => {
                let Some(sketch) = sketch_of(self, &r.sketch) else {
                    self.file_status =
                        Some(Err("Edit Feature: the sketch no longer exists".into()));
                    return;
                };
                self.roll_back_to(k);
                let mut d = ExtrudeDraft::new(sketch, DraftDef::Revolve(r));
                (d.edit, d.op_auto, d.axis_set) = (Some(k), false, true);
                self.extrude = Some(d);
            }
            Feature::Blend(b) => {
                self.roll_back_to(k);
                self.preset_edit(b.command(), &b);
                self.blend_draft = Some(blend::BlendDraft::editing(b, k));
            }
            Feature::Shell(sh) => {
                self.roll_back_to(k);
                self.preset_edit("Shell", &sh);
                self.shell_draft = Some(shell::ShellDraft::editing(sh, k));
            }
            Feature::Draft(dr) => {
                self.roll_back_to(k);
                self.preset_edit("Face Draft", &dr);
                self.draft_draft = Some(draft::DraftDraft::editing(dr, k));
            }
            Feature::Pattern(p) => {
                self.roll_back_to(k);
                self.pattern_draft = Some(pattern_feature::PatternDraft::editing(p, k));
            }
            Feature::Hole(h) => {
                self.roll_back_to(k);
                self.hole_draft = Some(hole::HoleDraft::editing(h, k));
                self.hole_load_sketch();
            }
            Feature::Thread(t) => {
                self.roll_back_to(k);
                self.thread_draft = Some(hole::ThreadDraft::editing(t, k));
            }
            Feature::Combine(c) => {
                self.roll_back_to(k);
                self.combine_draft = Some(combine_split::CombineDraft::editing(c, k));
            }
            Feature::Split(sp) => {
                self.roll_back_to(k);
                self.split_draft = Some(combine_split::SplitDraft::editing(sp, k));
            }
            Feature::Sweep(s) => self.edit_sweep(s, k),
            Feature::Loft(l) => self.edit_loft(l, k),
            Feature::Coil(c) => self.edit_coil(c, k),
            Feature::Rib(r) => self.edit_rib(r, k),
            Feature::Work(_) => self.redefine_work(k),
            // The base box is edited in Properties, which shows it as the
            // body (its sizes); `Selection::Feature` of a box showed nothing.
            Feature::Box(_) => {
                self.selection = Selection::Body;
                self.file_status = Some(Ok("Edit Feature: the Box sizes are in Properties".into()));
            }
        }
        if let Some(def) = self.extrude.as_ref().map(|d| d.def.clone()) {
            self.draft_preset_edit(&def);
        }
    }

    /// Extrude dialog: when the operation becomes Cut or Intersect and the
    /// direction is Default although the material is on the other side of
    /// the profile, the direction turns into it (Inventor points a cut into
    /// the part); back to Join / New Solid it turns back.
    pub(super) fn sync_extrude_direction(&mut self) {
        use crate::part::{ExtentDirection, Operation};
        let Some(d) = self.extrude.as_ref() else {
            return;
        };
        let DraftDef::Extrude(e) = &d.def else { return };
        let (op, last, dir, auto) = (e.operation, d.last_op, e.direction, d.auto_flipped);
        if last == Some(op) {
            return;
        }
        let removing = matches!(op, Operation::Cut | Operation::Intersect);
        let flip_to = match (last, removing) {
            // First frame: keep what the feature (or an edit) has.
            (None, _) => None,
            (Some(_), true) if dir == ExtentDirection::Default => {
                let part = self.part();
                part.sketches
                    .get(d.sketch)
                    .and_then(|ps| e.material_below(&part.body, &ps.sketch))
                    .filter(|&below| below)
                    .map(|_| ExtentDirection::Flipped)
            }
            (Some(_), false) if auto && dir == ExtentDirection::Flipped => {
                Some(ExtentDirection::Default)
            }
            _ => None,
        };
        if let Some(d) = self.extrude.as_mut() {
            d.last_op = Some(op);
            if let Some(nd) = flip_to {
                if let DraftDef::Extrude(e) = &mut d.def {
                    e.direction = nd;
                }
                d.auto_flipped = nd == ExtentDirection::Flipped;
            }
        }
    }

    /// Rebuild All (Inventor: Manage > Update): every feature of the part is
    /// recomputed from scratch, without the build cache; the status bar
    /// tells how long it took and how many features failed.
    pub(super) fn rebuild_all(&mut self) {
        let t = std::time::Instant::now();
        let part = self.part_mut();
        part.build_cache = Default::default();
        let result = part.regenerate();
        let failed = part.feature_health.len();
        let n = part.features.len();
        self.file_status = Some(match result {
            Err(e) => Err(format!("Rebuild All: {e}")),
            Ok(_) if failed > 0 => Err(format!(
                "Rebuild All: {failed} of {n} features failed ({:.2} s)",
                t.elapsed().as_secs_f64()
            )),
            Ok(_) => Ok(format!(
                "Rebuild All: {n} features in {:.2} s",
                t.elapsed().as_secs_f64()
            )),
        });
        self.edge_cache.replace(None);
        self.scene_dirty = true;
    }

    /// Builds only the features before `k` (Edit Feature).
    fn roll_back_to(&mut self, k: usize) {
        self.part_mut().rollback = Some(k);
        if let Err(e) = self.part_mut().regenerate() {
            self.file_status = Some(Err(e.to_string()));
        }
        self.scene_dirty = true;
    }

    /// Ends a rollback (the edit dialog closed): the whole part again.
    pub(super) fn end_feature_edit(&mut self) {
        if self.part().rollback.is_some() {
            self.part_mut().rollback = None;
            if let Err(e) = self.part_mut().regenerate() {
                self.file_status = Some(Err(e.to_string()));
            }
            self.scene_dirty = true;
        }
    }

    /// Revolve: picks the sketch line under the cursor as the axis.
    fn axis_pick(&mut self, pos: egui::Pos2, rect: egui::Rect) {
        let Some(d) = self.extrude.as_ref() else {
            return;
        };
        let ps = &self.part().sketches[d.sketch];
        let sk = &ps.sketch;
        let screen = |id| {
            sk.point(id).and_then(|p| {
                self.camera
                    .project(&sk.plane.point_at(p.position.x, p.position.y), rect)
            })
        };
        let best = sk
            .entities()
            .filter_map(|(id, e)| match e {
                crate::sketch::Entity::Line { start, end } => {
                    let (a, b) = (screen(*start)?, screen(*end)?);
                    let ab = b - a;
                    let t = ((pos - a).dot(ab) / ab.length_sq().max(1e-9)).clamp(0.0, 1.0);
                    Some(((a + ab * t).distance(pos), id))
                }
                _ => None,
            })
            .filter(|(dist, _)| *dist <= overlay::CURVE_SNAP_PX)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let Some((_, line)) = best else {
            self.file_status = Some(Err("Revolve: pick a line of the sketch as the axis".into()));
            return;
        };
        if let Some(d) = self.extrude.as_mut()
            && let DraftDef::Revolve(r) = &mut d.def
        {
            r.axis = crate::part::AxisRef::SketchLine(line);
            d.axis_set = true;
            d.axis_pick = false;
            self.file_status = None;
        }
    }

    /// Extrude: a click in the graphics window toggles the region under it
    /// (in the current profile sketch, else in another visible sketch).
    fn extrude_pick(&mut self, pos: egui::Pos2, rect: egui::Rect) {
        if self.open_curve_pick(pos, rect) {
            return; // an open curve (Match Shape)
        }
        let Some(d) = self.extrude.as_ref() else {
            return;
        };
        let current = d.sketch;
        let sketches = &self.part().sketches;
        let order: Vec<usize> = std::iter::once(current)
            .chain((0..sketches.len()).filter(|&i| i != current && sketches[i].visible))
            .collect();
        for i in order {
            let sk = &self.part().sketches[i].sketch;
            let Some(world) = self.camera.ray_plane(pos, rect, &sk.plane) else {
                continue;
            };
            let uv = sk.plane.project(&world);
            let regions = self.sketch_regions(sk);
            let Some(r) = regions
                .iter()
                .filter(|r| r.contains(uv))
                .min_by(|a, b| a.area().total_cmp(&b.area()))
            else {
                continue;
            };
            let pr = crate::part::ProfileRef::of(r, r.interior_point());
            let auto = crate::part::revolve::auto_axis(sk);
            let Some(draft) = self.extrude.as_mut() else {
                return;
            };
            let switched = i != draft.sketch;
            let profiles = draft.def.profiles_mut();
            if switched {
                *profiles = vec![pr];
            } else if let Some(k) = profiles.iter().position(|p| p.sources == pr.sources) {
                profiles.remove(k);
            } else {
                profiles.push(pr);
            }
            if switched {
                draft.sketch = i;
                if let DraftDef::Revolve(r) = &mut draft.def {
                    draft.axis_set = auto.is_some();
                    draft.axis_pick = auto.is_none();
                    if let Some(a) = auto {
                        r.axis = a;
                    }
                }
            }
            return;
        }
    }

    /// Extrude / Revolve: the profiles of the dialog's sketch taken by a
    /// selection box from `a` to `b` (screen): window (left → right) takes
    /// the regions fully inside, crossing (right → left) also those it
    /// touches. No modifier replaces the profiles, Shift adds, Ctrl toggles.
    fn extrude_box_pick(
        &mut self,
        rect: egui::Rect,
        a: egui::Pos2,
        b: egui::Pos2,
        add: bool,
        toggle: bool,
    ) {
        let Some(d) = self.extrude.as_ref() else {
            return;
        };
        let Some(ps) = self.part().sketches.get(d.sketch) else {
            return;
        };
        let sk = &ps.sketch;
        let r = egui::Rect::from_two_pos(a, b);
        if r.width() < 3.0 && r.height() < 3.0 {
            return;
        }
        let window = self.select.box_mode.is_window(a.x, b.x);
        let regions = self.sketch_regions(sk);
        let screen =
            |q: crate::sketch::Point2| self.camera.project(&sk.plane.point_at(q.x, q.y), rect);
        let hits: Vec<crate::part::ProfileRef> = regions
            .iter()
            .filter(|reg| {
                let pts: Vec<egui::Pos2> = reg
                    .outer
                    .polygon(0.05)
                    .into_iter()
                    .filter_map(screen)
                    .collect();
                if pts.is_empty() {
                    return false;
                }
                if window {
                    pts.iter().all(|p| r.contains(*p))
                } else {
                    pts.iter().any(|p| r.contains(*p))
                        || screen(reg.interior_point()).is_some_and(|p| r.contains(p))
                        || {
                            // The box inside the region (no corner of the
                            // outline in the box): its centre is in the region.
                            let c = r.center();
                            self.camera
                                .ray_plane(c, rect, &sk.plane)
                                .is_some_and(|w| reg.contains(sk.plane.project(&w)))
                        }
                }
            })
            .map(|reg| crate::part::ProfileRef::of(reg, reg.interior_point()))
            .collect();
        let Some(draft) = self.extrude.as_mut() else {
            return;
        };
        let profiles = draft.def.profiles_mut();
        if !add && !toggle {
            profiles.clear();
        }
        for pr in hits {
            match profiles.iter().position(|p| p.sources == pr.sources) {
                Some(k) if toggle => {
                    profiles.remove(k);
                }
                Some(_) => {}
                None => profiles.push(pr),
            }
        }
    }

    /// A face or vertex under the cursor as an extent target: a vertex
    /// within the snap radius wins, else the face hit by the ray.
    fn target_under(&self, pos: egui::Pos2, rect: egui::Rect) -> Option<crate::part::TargetRef> {
        let body = &self.part().body;
        let vertex = body
            .solids()
            .flat_map(|(s, _)| {
                body.solid(s)
                    .into_iter()
                    .flat_map(|sol| sol.shells.iter())
                    .flat_map(|sh| sh.faces.iter())
                    .flat_map(|&f| body.face_vertices(f))
            })
            .filter_map(|v| {
                let vx = body.vertex(v)?;
                let d = self.camera.project(&vx.point, rect)?.distance(pos);
                Some((d, vx.name.clone()?))
            })
            .filter(|(d, _)| *d <= overlay::POINT_SNAP_PX)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, name)) = vertex {
            return Some(crate::part::TargetRef::Vertex(name));
        }
        // A work point (To a point: the plane through it).
        if let Some(work::WpInput::Point(p @ crate::part::PointRef::Work(_))) =
            self.work_axis_or_point_under(pos, rect)
        {
            return Some(crate::part::TargetRef::Point(p));
        }
        let (origin, dir) = self.camera.ray(pos, rect)?;
        if let Some((f, _)) = self.face_hit(&origin, &dir) {
            return Some(crate::part::TargetRef::Face(body.face(f)?.name.clone()?));
        }
        // Work and origin planes.
        match self.plane_under(pos, rect, true)? {
            PlaneRef::Face(n) => Some(crate::part::TargetRef::Face(n)),
            p => Some(crate::part::TargetRef::Plane(p)),
        }
    }

    /// Applies a picked target to the dialog or to an existing feature.
    fn target_picked(&mut self, pos: egui::Pos2, rect: egui::Rect) {
        let Some((owner, slot)) = self.target_pick.take() else {
            return;
        };
        let Some(t) = self.target_under(pos, rect) else {
            self.file_status = Some(Err("Pick a face or a vertex of the model".into()));
            self.target_pick = Some((owner, slot));
            return;
        };
        // Sets the target; Inventor: Extend face to end feature turns on by
        // itself when the face does not cover the profile.
        let set = |def: &mut crate::part::ExtrudeFeature, part: &PartDocument, sketch: usize| {
            match slot {
                TargetSlot::From => def.from = Some(t.clone()),
                TargetSlot::To => def.extent = crate::part::Extent::To(t.clone()),
            }
            if let Some(ps) = part.sketches.get(sketch)
                && def.needs_extended_face(&part.body, &part.work, &ps.sketch)
            {
                def.extend_face = true;
            }
        };
        match owner {
            TargetOwner::Draft => {
                let Some(mut d) = self.extrude.take() else {
                    return;
                };
                match &mut d.def {
                    DraftDef::Extrude(e) => set(e, self.part(), d.sketch),
                    DraftDef::Revolve(r) => {
                        r.extent = crate::part::RevolveExtent::To;
                        r.to = Some(t.clone());
                    }
                }
                self.extrude = Some(d);
            }
            TargetOwner::Feature(k) => {
                if let Some(crate::part::Feature::Extrude(e)) = self.part().features.get(k).cloned()
                {
                    let mut e = e;
                    let sketch = self.part().sketches.iter().position(|s| s.id == e.sketch);
                    set(&mut e, self.part(), sketch.unwrap_or(usize::MAX));
                    self.edit_extrude(k, e);
                } else if let Some(crate::part::Feature::Revolve(mut r)) =
                    self.part().features.get(k).cloned()
                {
                    r.extent = crate::part::RevolveExtent::To;
                    r.to = Some(t.clone());
                    self.edit_feature(k, crate::part::Feature::Revolve(r));
                }
            }
        }
    }

    /// A preview by `key`: cached, or computed by `work` on a worker
    /// thread ([`PREVIEW_PENDING`] until it is done). A newer key replaces
    /// the job; the old thread's result is dropped.
    /// The preview for `key` if it needs no new work: the cached result, or
    /// "computing" while a job runs — for this key, or for an older one
    /// (one job at a time: changing a value quickly must not start a full
    /// regeneration per step; the newest key starts when the old job ends).
    /// `None`: start a job. Callers check this before copying the part, so
    /// an idle frame copies nothing.
    pub(super) fn preview_ready(&self, key: &str) -> Option<PreviewResult> {
        if let Some((k, r)) = &*self.extrude_preview.borrow()
            && k == key
        {
            return Some(r.clone());
        }
        let mut job = self.preview_job.borrow_mut();
        let (k, rx) = job.as_ref()?;
        let same = k == key;
        let done = match rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return Some(Err(PREVIEW_PENDING.into())),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("the preview could not be computed".into())
            }
        };
        let k = k.clone();
        *job = None;
        *self.extrude_preview.borrow_mut() = Some((k, done.clone()));
        // A finished job for an older key: start the current one.
        same.then_some(done)
    }

    /// A cheap signature of what the part is built from (features,
    /// sketches, End of Part, suppressions): equal means a preview's result
    /// is still the part OK would build.
    pub(super) fn part_signature(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let p = self.part();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_vec(&p.features)
            .unwrap_or_default()
            .hash(&mut h);
        serde_json::to_vec(&p.sketches)
            .unwrap_or_default()
            .hash(&mut h);
        serde_json::to_vec(&p.user_params)
            .unwrap_or_default()
            .hash(&mut h);
        p.eop.hash(&mut h);
        let mut sup: Vec<&String> = p.suppressed.iter().collect();
        sup.sort();
        sup.hash(&mut h);
        h.finish()
    }

    /// The part the preview for `key` built from the part as it is now, if
    /// there is one (taken: it is used once).
    pub(super) fn take_preview_part(&self, key: &str) -> Option<PartDocument> {
        let sig = self.part_signature();
        let mut slot = self.preview_part.lock().ok()?;
        match slot.take() {
            Some((k, s, p)) if k == key && s == sig => Some(p),
            _ => None,
        }
    }

    pub(super) fn preview_async(
        &self,
        key: String,
        work: impl FnOnce() -> PreviewResult + Send + 'static,
    ) -> PreviewResult {
        if let Some(r) = self.preview_ready(&key) {
            return r;
        }
        let mut job = self.preview_job.borrow_mut();
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("preview".into())
            // The kernel recurses deeply on large bodies.
            .stack_size(64 << 20)
            .spawn(move || {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                    .unwrap_or_else(|_| Err("internal error while computing the preview".into()));
                let _ = tx.send(r);
            });
        if spawned.is_err() {
            return Err("the preview could not be started".into());
        }
        *job = Some((key, rx));
        Err(PREVIEW_PENDING.into())
    }

    /// The edge polylines of feature `id` of `part` (a preview).
    pub(super) fn feature_polylines(part: &PartDocument, id: &str) -> Vec<Vec<Point3>> {
        let body = &part.body;
        let opts = TessellationOptions::default();
        body.feature_edges(id)
            .into_iter()
            .filter_map(|e| body.sample_edge(e, &opts).ok())
            .collect()
    }

    /// The Extrude dialog's result as edge polylines (cached by its
    /// options, computed off the frame), or why it cannot be built.
    fn extrude_preview(&self) -> Result<Vec<Vec<Point3>>, String> {
        let Some(d) = &self.extrude else {
            return Ok(Vec::new());
        };
        let open = matches!(&d.def, DraftDef::Extrude(e) if e.open.is_some());
        if d.def.profiles().is_empty() && !open {
            return Err("Select a profile".to_owned());
        }
        if matches!(d.def, DraftDef::Revolve(_)) && !d.axis_set {
            return Err("Pick the axis".to_owned());
        }
        // Inventor: in a part with several bodies the preview waits for the
        // Solids choice (or New Solid).
        let (op, output, chosen) = match &d.def {
            DraftDef::Extrude(e) => (e.operation, e.output, e.solids.is_empty()),
            DraftDef::Revolve(r) => (r.operation, r.output, r.solids.is_empty()),
        };
        if chosen
            && output == crate::part::Output::Solid
            && op != crate::part::Operation::NewSolid
            && self
                .part()
                .bodies_before(d.edit.unwrap_or(usize::MAX))
                .len()
                > 1
        {
            return Err(format!(
                "{}: choose the Solids to {} (or New Solid)",
                d.def.command(),
                op.label()
            ));
        }
        // The sketch's geometry too: its dimensions can be edited meanwhile.
        let sig = self
            .part()
            .sketches
            .get(d.sketch)
            .map_or(0, |ps| feature_cmd::sketch_sig(&ps.sketch));
        let key = format!("{}|{sig}|{:?}|{:?}", d.sketch, d.edit, d.def);
        // Cached or still computing: no copy of the part this frame.
        if let Some(r) = self.preview_ready(&key) {
            return r;
        }
        let mut trial = self.part().clone();
        let (sketch, def, edit) = (d.sketch, d.def.clone(), d.edit);
        self.preview_async(key, move || {
            use crate::part::Feature;
            let added = match (edit, &def) {
                (Some(k), DraftDef::Extrude(e)) => {
                    trial.replace_feature(k, Feature::Extrude(e.clone()))
                }
                (Some(k), DraftDef::Revolve(r)) => {
                    trial.replace_feature(k, Feature::Revolve(r.clone()))
                }
                (None, DraftDef::Extrude(e)) => trial.add_extrude(sketch, e.clone()),
                (None, DraftDef::Revolve(r)) => trial.add_revolve(sketch, r.clone()),
            };
            added
                .map(|id| Self::feature_polylines(&trial, &id))
                .map_err(|e| e.to_string())
        })
    }

    /// Extrude / Revolve dialog OK: adds the feature (or reports why it
    /// cannot).
    fn commit_extrude(&mut self) {
        let Some(mut d) = self.extrude.clone() else {
            return;
        };
        let cmd = if d.edit.is_some() {
            "Edit Feature"
        } else {
            d.def.command()
        };
        self.draft_preset_stamp(&mut d.def);
        let used = d.def.clone();
        self.checkpoint(cmd);
        let result = match (d.edit, d.def) {
            (Some(k), DraftDef::Extrude(e)) => self
                .part_mut()
                .replace_feature(k, crate::part::Feature::Extrude(e)),
            (Some(k), DraftDef::Revolve(r)) => self
                .part_mut()
                .replace_feature(k, crate::part::Feature::Revolve(r)),
            (None, DraftDef::Extrude(e)) => self.part_mut().add_extrude(d.sketch, e),
            (None, DraftDef::Revolve(r)) => self.part_mut().add_revolve(d.sketch, r),
        };
        if result.is_err()
            && let Some(k) = d.edit
        {
            // Stay in the edit: rolled back again.
            self.roll_back_to(k);
        }
        match result {
            Ok(_) => {
                self.selection = Selection::Feature(d.edit.unwrap_or(self.part().last_inserted()));
                self.extrude = None;
                self.target_pick = None;
                self.file_status = None;
                self.draft_preset_used(&used, d.edit.is_none());
                self.scene_dirty = true;
            }
            Err(e) => {
                self.history.cancel(); // nothing changed
                self.file_status = Some(Err(format!("{cmd}: {e}")));
            }
        }
    }

    /// The manipulators shown now: the open dialog's, else the selected
    /// feature's (not while a sketch is edited).
    fn handles(&self) -> Option<(ManipOwner, Vec<manip::Handle>)> {
        use crate::part::Feature;
        let part = self.part();
        // Offset work planes: the distance arrow from the base plane.
        let offset = |base: &PlaneRef, d: f64| {
            let p = crate::part::placement::plane_of(base, &part.body, &part.work).ok()?;
            let (c, _) = self.work_plane_frame(&p);
            Some(vec![manip::plane_offset_handle(
                c,
                p.normal.into_inner(),
                d,
            )])
        };
        use crate::part::{WorkDef, WorkPlaneDef};
        if self.hole_draft.is_some() {
            let hs: Vec<_> = self.hole_handles().into_iter().map(|(_, h)| h).collect();
            return (!hs.is_empty()).then_some((ManipOwner::Hole, hs));
        }
        if self.blend_draft.is_some() {
            let hs = self.blend_handles();
            return (!hs.is_empty()).then_some((ManipOwner::Blend, hs));
        }
        if let Some(d) = &self.work_draft {
            return match d.def() {
                Ok(WorkDef::Plane(WorkPlaneDef::Offset { base, distance })) => {
                    Some((ManipOwner::Work, offset(&base, distance)?))
                }
                Ok(def) => Some((ManipOwner::Work, self.work_angle_handle(&def)?)),
                _ => None,
            };
        }
        if let Selection::Feature(k) = self.selection
            && self.editing.is_none()
            && let Some(Feature::Work(_)) = part.features.get(k)
        {
            // While its handle is dragged the arrow follows the pending value
            // (the feature itself changes on release).
            let Some(Feature::Work(w)) = self.pending_work(k) else {
                return None;
            };
            return match &w.def {
                WorkDef::Plane(WorkPlaneDef::Offset { base, distance }) => {
                    Some((ManipOwner::Feature(k), offset(base, *distance)?))
                }
                def => Some((ManipOwner::Feature(k), self.work_angle_handle(def)?)),
            };
        }
        if let Some(d) = &self.extrude {
            let sk = &part.sketches.get(d.sketch)?.sketch;
            let h = match &d.def {
                DraftDef::Extrude(e) => manip::extrude_handles(e, sk),
                DraftDef::Revolve(_) if !d.axis_set => Vec::new(),
                DraftDef::Revolve(r) => manip::revolve_handles(r, sk, &part.work),
            };
            return Some((ManipOwner::Draft, h));
        }
        if self.shell_draft.is_some() {
            return Some((ManipOwner::Shell, self.shell_handles()));
        }
        let Selection::Feature(k) = self.selection else {
            return None;
        };
        if self.editing.is_some() || part.feature_health.contains_key(part.features.get(k)?.id()) {
            return None;
        }
        let f = part.features.get(k)?;
        let sk = &part
            .sketches
            .iter()
            .find(|s| Some(s.id.as_str()) == f.sketch())?
            .sketch;
        let h = match f {
            Feature::Extrude(e) => manip::extrude_handles(e, sk),
            Feature::Revolve(r) => manip::revolve_handles(r, sk, &part.work),
            _ => Vec::new(),
        };
        Some((ManipOwner::Feature(k), h))
    }

    /// Work feature `k` as it would be with the value of its dragged handle
    /// (unchanged when nothing is dragged).
    pub(super) fn pending_work(&self, k: usize) -> Option<crate::part::Feature> {
        let mut f = self.part().features.get(k).cloned()?;
        if let Some((ManipOwner::Feature(kk), h, v)) = &self.manip_drag
            && *kk == k
            && v.is_finite()
        {
            crate::part::params::set_feature_field_value(&mut f, h.field, *v);
        }
        Some(f)
    }

    /// Handles of the interpolation splines of sketch `i`: the set ones,
    /// and on a selected spline one per fit point (grey = free: dragging it
    /// activates the handle).
    fn spline_grips(&self, i: usize) -> Vec<SplineGrip> {
        let sk = &self.part().sketches[i].sketch;
        let mut out = Vec::new();
        for (e, ent) in sk.entities() {
            let crate::sketch::Entity::Spline { points } = ent else {
                continue;
            };
            let Some(fit) = sk.spline_fit(e) else {
                continue;
            };
            let selected = self.tools.selected.contains(&e);
            let n = points.len();
            // A closed curve's last point is its first.
            let count = if fit.closed { n - 1 } else { n };
            for k in 0..count {
                let set = fit.handle(k, n).is_some();
                if !set && !selected {
                    continue;
                }
                let Some((base, grip)) = sk.spline_handle_grip_at(e, k) else {
                    continue;
                };
                out.push(SplineGrip {
                    spline: e,
                    k,
                    base,
                    grip,
                    set,
                });
            }
        }
        out
    }

    /// The spline handle grip under `pos` (screen).
    fn spline_grip_at(
        &self,
        i: usize,
        rect: egui::Rect,
        pos: egui::Pos2,
    ) -> Option<(EntityId, usize)> {
        let sk = &self.part().sketches[i].sketch;
        self.spline_grips(i)
            .into_iter()
            .filter_map(|g| {
                let s = self
                    .camera
                    .project(&sk.plane.point_at(g.grip.x, g.grip.y), rect)?;
                Some(((g.spline, g.k), s.distance(pos)))
            })
            .filter(|(_, d)| *d <= 8.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(g, _)| g)
    }

    /// Curves of sketch `i` taken by a selection box from `a` to `b`
    /// (screen): window = fully inside, crossing (b left of a) = touching.
    fn box_hits(&self, i: usize, rect: egui::Rect, a: egui::Pos2, b: egui::Pos2) -> Vec<EntityId> {
        let sk = &self.part().sketches[i].sketch;
        let r = egui::Rect::from_two_pos(a, b);
        let window = self.select.box_mode.is_window(a.x, b.x);
        let seg_hits = |p: egui::Pos2, q: egui::Pos2| {
            if r.contains(p) || r.contains(q) {
                return true;
            }
            // Segment against the 4 box edges.
            let cross = |a: egui::Pos2, b: egui::Pos2, c: egui::Pos2, d: egui::Pos2| {
                let o = |p: egui::Pos2, q: egui::Pos2, r: egui::Pos2| {
                    (q - p).x * (r - p).y - (q - p).y * (r - p).x
                };
                o(a, b, c) * o(a, b, d) < 0.0 && o(c, d, a) * o(c, d, b) < 0.0
            };
            let c = [
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
            ];
            (0..4).any(|k| cross(p, q, c[k], c[(k + 1) % 4]))
        };
        sk.entities()
            .filter_map(|(id, _)| {
                let pts: Vec<egui::Pos2> = overlay::polyline(sk, id)
                    .iter()
                    .filter_map(|q| self.camera.project(&sk.plane.point_at(q.x, q.y), rect))
                    .collect();
                if pts.is_empty() {
                    return None;
                }
                let taken = if window {
                    pts.iter().all(|p| r.contains(*p))
                } else {
                    pts.windows(2).any(|w| seg_hits(w[0], w[1]))
                        || pts.iter().any(|p| r.contains(*p))
                };
                taken.then_some(id)
            })
            .collect()
    }

    /// The visible edges of the body as polylines (cached per body).
    /// A cheap signature of the body: changes when it is rebuilt.
    pub(super) fn body_key(&self) -> String {
        if let (true, Some(k)) = &*self.body_key_frame.borrow() {
            return k.clone();
        }
        let k = self.compute_body_key();
        let mut c = self.body_key_frame.borrow_mut();
        if c.0 {
            c.1 = Some(k.clone());
        }
        k
    }

    fn compute_body_key(&self) -> String {
        // Called many times a frame (every `picker()`, every hidden-line
        // test): no walk over the face meshes. `bounding_box` reads every
        // mesh position — on a 2500-hole pattern a selected face made frames
        // take 80 ms. The vertices' box plus the mesh sizes catch a rebuild.
        let body = &self.part().body;
        let bbox = body
            .vertices()
            .fold(None, |b: Option<(Point3, Point3)>, (_, v)| {
                Some(b.map_or((v.point, v.point), |(lo, hi)| {
                    (lo.inf(&v.point), hi.sup(&v.point))
                }))
            });
        let mesh: usize = body
            .faces()
            .filter_map(|(_, f)| f.mesh.as_ref())
            .map(|m| m.positions.len())
            .sum();
        format!(
            "{}|{}|{}|{mesh}|{bbox:?}|{:?}",
            body.edges().count(),
            body.faces().count(),
            body.vertices().count(),
            self.part().hidden_bodies
        )
    }

    /// The picking data of the current body.
    pub(super) fn picker(&self) -> std::rc::Rc<PickCache> {
        let key = self.body_key();
        if let Some((k, v)) = &*self.pick_cache.borrow()
            && *k == key
        {
            return v.clone();
        }
        let v = std::rc::Rc::new(PickCache::build(&self.part().body, &self.hidden_faces()));
        *self.pick_cache.borrow_mut() = Some((key, v.clone()));
        v
    }

    fn edge_polylines(&self) -> EdgePolylines {
        let body = &self.part().body;
        let key = self.body_key();
        if let Some((k, v)) = &*self.edge_cache.borrow()
            && *k == key
        {
            return v.clone();
        }
        let opts = TessellationOptions::default();
        let shown = self.shown_edges();
        let v: EdgePolylines = std::rc::Rc::new(
            body.visible_edges()
                .into_iter()
                .filter(|e| shown.as_ref().is_none_or(|s| s.contains(e)))
                .filter(|&e| body.solids().any(|(_, s)| !s.open) || body.edge(e).is_some())
                .filter_map(|e| Some((e, body.sample_edge(e, &opts).ok()?)))
                .collect(),
        );
        *self.edge_cache.borrow_mut() = Some((key, v.clone()));
        v
    }

    /// The visible model edge under `pos` (within `POINT_PICK_PX`); edges
    /// hidden behind a face do not count.
    fn edge_under(&self, pos: egui::Pos2, rect: egui::Rect) -> Option<crate::kernel::EdgeId> {
        if !self.cat_on(crate::command::selection::Category::Edge) {
            return None;
        }
        let (origin, dir) = self.camera.ray(pos, rect)?;
        // Unit direction: ray parameters are then mm, which the occlusion
        // tolerance below assumes (an unscaled ray let hidden edges through).
        let dir = dir.try_normalize(1e-12)?;
        let face_t = self.picker().pick(&origin, &dir).map(|(_, t)| t);
        let mut best: Option<(crate::kernel::EdgeId, f32)> = None;
        for (e, pts) in self.edge_polylines().iter() {
            for w in pts.windows(2) {
                let (Some(a), Some(b)) = (
                    self.camera.project(&w[0], rect),
                    self.camera.project(&w[1], rect),
                ) else {
                    continue;
                };
                let ab = b - a;
                let t = ((pos - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                let d = (a + ab * t).distance(pos);
                if d > overlay::EDGE_PICK_PX || best.is_some_and(|(_, bd)| bd <= d) {
                    continue;
                }
                // Hidden behind a face? The segment's point nearest the eye
                // ray (the screen parameter is not the 3D one under
                // perspective: on a long straight edge that put the point
                // millimetres off and hid the edge).
                let q = closest_on_segment(&w[0], &w[1], &origin, &dir);
                let te = (q - origin).dot(&dir) / dir.norm_squared();
                if face_t.is_some_and(|ft| ft < te - 0.01 * te.abs().max(1.0)) {
                    continue;
                }
                best = Some((*e, d));
            }
        }
        best.map(|(e, _)| e)
    }

    /// Draws the Select Other control and handles its arrows; returns
    /// whether the current candidate was accepted (click on its name).
    fn select_other_ui(&mut self, ui: &mut egui::Ui, rect: egui::Rect) -> bool {
        let Some(so) = self.select_other.clone() else {
            return false;
        };
        let n = so.items.len();
        let label = self.item_label(&so.items[so.index]);
        let pos = egui::pos2(
            (so.at.x + 14.0).min(rect.right() - 260.0),
            (so.at.y - 40.0).max(rect.top() + 4.0),
        );
        let mut step = 0i32;
        let mut accept = false;
        egui::Area::new(egui::Id::new("select_other"))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(3.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui
                                .small_button("<")
                                .on_hover_text("Previous (Left)")
                                .clicked()
                            {
                                step = -1;
                            }
                            if ui.small_button(">").on_hover_text("Next (Right)").clicked() {
                                step = 1;
                            }
                            if ui
                                .selectable_label(true, format!("{} / {n}  {label}", so.index + 1))
                                .on_hover_text("Click to select it")
                                .clicked()
                            {
                                accept = true;
                            }
                        });
                    });
            });
        let (left, right) = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::ArrowDown),
            )
        });
        if left {
            step = -1;
        }
        if right {
            step = 1;
        }
        // Shift + wheel cycles too (egui turns it into a sideways scroll).
        let (shift, wheel) = ui.input(|i| {
            let d: egui::Vec2 = i
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::MouseWheel { delta, .. } => Some(*delta),
                    _ => None,
                })
                .fold(egui::Vec2::ZERO, |a, b| a + b);
            (i.modifiers.shift, if d.y != 0.0 { d.y } else { d.x })
        });
        if shift && wheel != 0.0 {
            step = if wheel > 0.0 { -1 } else { 1 };
        }
        if step != 0
            && let Some(so) = &mut self.select_other
        {
            so.index = (so.index as i32 + step).rem_euclid(n as i32) as usize;
        }
        accept
    }

    /// "Edge Extrusion1:… · L 40.00 mm" (unnamed edges: by their faces).
    fn edge_description(&self, e: crate::kernel::EdgeId) -> String {
        let body = &self.part().body;
        let len: f64 = self
            .edge_polylines()
            .iter()
            .find(|(id, _)| *id == e)
            .map_or(0.0, |(_, p)| {
                p.windows(2).map(|w| (w[1] - w[0]).norm()).sum()
            });
        let name = body
            .edge(e)
            .and_then(|x| x.name.clone())
            .unwrap_or_else(|| {
                self.picker()
                    .faces_of(e)
                    .into_iter()
                    .filter_map(|f| body.face(f).and_then(|f| f.name.clone()))
                    .collect::<Vec<_>>()
                    .join(" | ")
            });
        format!("Edge {name}  ·  L {len:.2} mm")
    }

    /// The feature behind an edge: that of its first named face.
    fn edge_feature(&self, e: crate::kernel::EdgeId) -> Option<usize> {
        let body = &self.part().body;
        self.picker()
            .faces_of(e)
            .into_iter()
            .filter_map(|f| body.face(f).and_then(|f| f.name.clone()))
            .find_map(|n: String| self.face_feature(&n))
    }

    /// Suggestions next to a selected edge: Fillet, Chamfer and Edit Feature.
    fn edge_toolbar(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        use icons::Icon;
        let Some((e, at)) = self.edge_sel else {
            return;
        };
        let feature = self.edge_feature(e);
        let pos = egui::pos2(
            (at.x + 14.0).min(rect.right() - 100.0),
            (at.y - 44.0).max(rect.top() + 4.0),
        );
        let mut edit = false;
        let mut start = None;
        egui::Area::new(egui::Id::new("edge_toolbar"))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(3.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let b = |ui: &mut egui::Ui, icon, tip: &str, on: bool| {
                                ribbon::icon_button(
                                    ui,
                                    icon,
                                    "",
                                    ribbon::Size::Mini,
                                    on,
                                    false,
                                    false,
                                )
                                .on_hover_text(tip)
                                .clicked()
                            };
                            if b(ui, Icon::FilletEdge, "Fillet this edge", true) {
                                start = Some(false);
                            }
                            if b(ui, Icon::ChamferEdge, "Chamfer this edge", true) {
                                start = Some(true);
                            }
                            if b(ui, Icon::Extrude, "Edit Feature", feature.is_some()) {
                                edit = true;
                            }
                        });
                    });
            });
        if edit && let Some(k) = feature {
            self.edge_sel = None;
            self.edit_feature_dialog(k);
        }
        if let Some(chamfer) = start {
            self.start_blend(chamfer, Some(e));
        }
    }

    /// The feature that made the face `name` (its topology-name prefix).
    fn face_feature(&self, name: &str) -> Option<usize> {
        let prefix = name.split(':').next()?;
        let id = prefix.split('#').next()?;
        self.part().features.iter().position(|f| f.id() == id)
    }

    /// "Face Extrusion1:End · Extrusion1 · Solid2" for the tooltip.
    fn face_description(&self, name: &str) -> String {
        let part = self.part();
        let mut text = format!("Face {name}");
        if let Some(k) = self.face_feature(name) {
            text.push_str(&format!("  ·  {}", part.features[k].name()));
        }
        let body = &part.body;
        let solid = body.find_face(name).and_then(|f| {
            body.solids()
                .find(|(_, s)| s.shells.iter().any(|sh| sh.faces.contains(&f)))
                .and_then(|(_, s)| s.name.clone())
        });
        if let Some(s) = solid
            && let Some((_, label)) = part
                .bodies_before(usize::MAX)
                .into_iter()
                .find(|(n, _)| *n == s)
        {
            text.push_str(&format!("  ·  {label}"));
        }
        text
    }

    /// Suggestions next to a selected face (0.9 §6): New Sketch, Edit
    /// Feature, Edit Sketch, Look At.
    fn face_toolbar(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        use icons::Icon;
        let Some((name, at)) = self.face_sel.clone() else {
            return;
        };
        let feature = self.face_feature(&name);
        let sketch = feature.and_then(|k| {
            let id = self.part().features[k].sketch()?.to_owned();
            self.part().sketches.iter().position(|s| s.id == id)
        });
        let pos = egui::pos2(
            (at.x + 14.0).min(rect.right() - 130.0),
            (at.y - 44.0).max(rect.top() + 4.0),
        );
        let mut act: Option<u8> = None;
        egui::Area::new(egui::Id::new("face_toolbar"))
            .fixed_pos(pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(3.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let b = |ui: &mut egui::Ui, icon, tip: &str, on: bool| {
                                ribbon::icon_button(
                                    ui,
                                    icon,
                                    "",
                                    ribbon::Size::Mini,
                                    on,
                                    false,
                                    false,
                                )
                                .on_hover_text(tip)
                                .clicked()
                            };
                            if b(ui, Icon::StartSketch, "New Sketch on this face", true) {
                                act = Some(0);
                            }
                            if b(
                                ui,
                                Icon::Extrude,
                                "Extrude this face (Press/Pull: Join outwards; Cut + Flipped inwards)",
                                true,
                            ) {
                                act = Some(4);
                            }
                            if b(ui, Icon::Part, "Edit Feature", feature.is_some()) {
                                act = Some(1);
                            }
                            if b(ui, Icon::Line, "Edit Sketch", sketch.is_some()) {
                                act = Some(2);
                            }
                            if b(ui, Icon::LookAt, "Look At this face", true) {
                                act = Some(3);
                            }
                        });
                    });
            });
        match act {
            Some(0) => {
                self.face_sel = None;
                self.mode = Mode::PickPlane(PickPurpose::NewSketch);
                self.plane_picked(PlaneRef::Face(name));
            }
            Some(4) => self.press_pull(name, at),
            Some(1) => {
                self.face_sel = None;
                if let Some(k) = feature {
                    self.edit_feature_dialog(k);
                }
            }
            Some(2) => {
                self.face_sel = None;
                if let Some(i) = sketch {
                    self.edit_sketch(i);
                }
            }
            Some(3) => {
                let placement = crate::part::SketchPlacement {
                    plane: PlaneRef::Face(name),
                    ..Default::default()
                };
                let body = &self.part().body;
                if let Ok(plane) = placement.resolve(body, &crate::kernel::Plane::xy()) {
                    let (center, radius) = match body.bounding_box() {
                        Some((lo, hi)) => {
                            let uv = plane.project(&nalgebra::center(&lo, &hi));
                            (
                                plane.point_at(uv.x, uv.y),
                                nalgebra::distance(&lo, &hi) / 2.0,
                            )
                        }
                        None => (plane.origin, 50.0),
                    };
                    self.camera.look_at(&plane, center, radius);
                }
            }
            _ => {}
        }
    }

    /// Dynamic Input fields of the current tool and step.
    pub(super) fn hud_fields(&self) -> &'static [&'static str] {
        if self.editing.is_none() {
            return &[];
        }
        match (self.tools.tool, self.tools.pick_count()) {
            // The first point by coordinates (sketch X/Y).
            (
                Tool::Line
                | Tool::CircleCenter
                | Tool::RectTwoPoint
                | Tool::RectCenter
                | Tool::Point
                | Tool::Move
                | Tool::Copy
                | Tool::Rotate,
                0,
            ) => &["X", "Y"],
            (Tool::Line, 1) => &["Length", "Angle"],
            (Tool::CircleCenter, 1) => &["Diameter"],
            (Tool::RectTwoPoint | Tool::RectCenter, 1) => &["Width", "Height"],
            (Tool::Move | Tool::Copy, 1) => &["Delta X", "Delta Y"],
            (Tool::Rotate, 2) => &["Angle"],
            _ => &[],
        }
    }

    /// The values the cursor gives now (mm; angles in degrees).
    pub(super) fn hud_live(&self, pick: &Pick) -> [f64; 2] {
        let Some(a) = self.tools.first_pick() else {
            return [pick.pos.x, pick.pos.y];
        };
        let d = pick.pos - a;
        match self.tools.tool {
            Tool::Move | Tool::Copy => [d.x, d.y],
            Tool::Rotate => {
                let r = self.tools.pick_pos(1).map_or(d, |p| p - a);
                [(r.x * d.y - r.y * d.x).atan2(r.dot(&d)).to_degrees(), 0.0]
            }
            Tool::Line => [d.norm(), d.y.atan2(d.x).to_degrees()],
            Tool::CircleCenter => [2.0 * d.norm(), 0.0],
            Tool::RectTwoPoint => [d.x.abs(), d.y.abs()],
            Tool::RectCenter => [2.0 * d.x.abs(), 2.0 * d.y.abs()],
            _ => [0.0, 0.0],
        }
    }

    /// The cursor pick moved to honour the locked values.
    fn hud_apply(&self, mut pick: Pick) -> Pick {
        let v = self.hud.vals;
        if self.hud_fields().is_empty() || (v[0].is_none() && v[1].is_none()) {
            return pick;
        }
        let Some(a) = self.tools.first_pick() else {
            // First point: absolute coordinates.
            pick.pos = Point2::new(v[0].unwrap_or(pick.pos.x), v[1].unwrap_or(pick.pos.y));
            pick.snap = Snap::None;
            pick.infer = None;
            pick.align = Align::default();
            return pick;
        };
        let d = pick.pos - a;
        let sign = |x: f64| if x < 0.0 { -1.0 } else { 1.0 };
        pick.pos = match self.tools.tool {
            Tool::Move | Tool::Copy => {
                a + crate::sketch::Vector2::new(v[0].unwrap_or(d.x), v[1].unwrap_or(d.y))
            }
            Tool::Rotate => {
                let r = self.tools.pick_pos(1).map_or(d, |p| p - a);
                let Some(ang) = v[0] else { return pick };
                let (s, c) = ang.to_radians().sin_cos();
                a + crate::sketch::Vector2::new(c * r.x - s * r.y, s * r.x + c * r.y)
            }
            Tool::Line => {
                let ang = v[1].map_or(d.y.atan2(d.x), f64::to_radians);
                let dir = crate::sketch::Vector2::new(ang.cos(), ang.sin());
                let len = v[0].unwrap_or_else(|| d.dot(&dir).max(0.0));
                a + dir * len
            }
            Tool::CircleCenter => {
                let dir = d
                    .try_normalize(1e-12)
                    .unwrap_or(crate::sketch::Vector2::x());
                a + dir * (v[0].unwrap_or(2.0 * d.norm()) / 2.0)
            }
            Tool::RectTwoPoint => {
                let w = v[0].map_or(d.x, |w| w * sign(d.x));
                let h = v[1].map_or(d.y, |h| h * sign(d.y));
                a + crate::sketch::Vector2::new(w, h)
            }
            Tool::RectCenter => {
                let w = v[0].map_or(d.x, |w| w / 2.0 * sign(d.x));
                let h = v[1].map_or(d.y, |h| h / 2.0 * sign(d.y));
                a + crate::sketch::Vector2::new(w, h)
            }
            _ => pick.pos,
        };
        pick.snap = Snap::None;
        pick.infer = None;
        pick.align = Align::default();
        pick
    }

    /// Keys for the HUD: digits / . / - / Backspace type into the focused
    /// field, Tab moves on (locking the typed value), Enter places the point,
    /// Esc clears. Returns true when the keys were taken.
    fn hud_keys(&mut self, ctx: &egui::Context) -> bool {
        let n = self.hud_fields().len();
        if n == 0 {
            return false;
        }
        let mut typed = String::new();
        // A value starts with a digit, a sign, a point or a bracket; after
        // that the field takes an expression too (`10+5`, `d0*2`, `1 in`):
        // letters then belong to the field, not to the shortcuts.
        let started = !self.hud.text.is_empty();
        ctx.input_mut(|i| {
            i.events.retain(|e| match e {
                egui::Event::Text(t)
                    if t.chars().all(|c| {
                        c.is_ascii_digit()
                            || ".,-(".contains(c)
                            || (started || !typed.is_empty())
                                && (c.is_ascii_alphanumeric() || "_+*/^() ".contains(c))
                    }) =>
                {
                    typed.push_str(t);
                    false
                }
                _ => true,
            })
        });
        let mut took = !typed.is_empty();
        self.hud.text.push_str(&typed.replace(',', "."));
        let tab = std::mem::take(&mut self.hud.tab);
        // Up: the value typed last time into this field.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowUp))
            && let Some(name) = self.hud_fields().get(self.hud.focus)
            && let Some(v) = self.hud_last.get(name)
        {
            self.hud.text = format!("{v}");
            took = true;
        }
        let (bs, enter, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::Backspace),
                !self.hud.text.is_empty() || self.hud.vals.iter().any(Option::is_some),
                i.key_pressed(Key::Escape),
            )
        });
        if bs {
            self.hud.text.pop();
            took = true;
        }
        // The field's value: an expression with the part's parameters (the
        // field's unit for bare numbers: degrees for angles, mm otherwise).
        let params: std::collections::HashMap<String, crate::part::expr::Value> = self
            .part()
            .parameter_table()
            .into_iter()
            .map(|r| (r.name.clone(), r.as_value()))
            .collect();
        let angle = |hud: &Hud, names: &[&str]| {
            names.get(hud.focus).is_some_and(|n| n.starts_with("Angle"))
        };
        let names = self.hud_fields();
        let lock = |hud: &mut Hud| {
            let unit = if angle(hud, names) {
                crate::part::expr::Unit::Deg
            } else {
                crate::part::expr::Unit::Mm
            };
            if let Ok((v, _)) =
                crate::part::expr::eval(hud.text.trim(), unit, &|n| params.get(n).cloned())
            {
                hud.vals[hud.focus] = Some(unit.to_display(v));
            }
            hud.text.clear();
        };
        if tab != 0 {
            lock(&mut self.hud);
            self.hud.focus = (self.hud.focus as i32 + tab).rem_euclid(n as i32) as usize;
            took = true;
        }
        if enter && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter)) {
            lock(&mut self.hud);
            if let Some(p) = self.last_pick {
                let p = self.hud_apply(p);
                self.hud_click(p);
            }
            took = true;
        }
        if esc && (!self.hud.text.is_empty() || self.hud.vals.iter().any(Option::is_some)) {
            self.hud.vals = [None, None];
            self.hud.text.clear();
            took = true;
        }
        took
    }

    /// A click of a drawing tool; values locked in the HUD become
    /// dimensions of the new geometry.
    fn hud_click(&mut self, pick: Pick) {
        let Some(i) = self.editing else { return };
        // Remember typed values per field (Up recalls them).
        for (k, name) in self.hud_fields().iter().enumerate() {
            if let Some(v) = self.hud.vals[k] {
                self.hud_last.insert(name, v);
            }
        }
        // Coordinates of a first point only place it.
        let vals = if self.hud_fields().is_empty() || self.tools.pick_count() == 0 {
            [None, None]
        } else {
            self.hud.vals
        };
        let tool = self.tools.tool;
        let sk = &self.part().sketches[i].sketch;
        let before: std::collections::HashSet<EntityId> = sk.entities().map(|(e, _)| e).collect();
        // The line the chain continues from (for an angle dimension).
        let prev_line = match self.tools.first_snap() {
            Some(Snap::Point(p)) => sk
                .entities()
                .find(|(_, e)| e.is_line() && e.endpoints().contains(&p))
                .map(|(e, _)| e),
            _ => None,
        };
        self.tool_op(|t, ps| t.click(ps, pick));
        if vals.iter().all(Option::is_none) {
            return;
        }
        let sk = &mut self.part_mut().sketches[i].sketch;
        let new: Vec<EntityId> = sk
            .entities()
            .map(|(e, _)| e)
            .filter(|e| !before.contains(e))
            .collect();
        let mut extra: Vec<crate::sketch::Constraint> = Vec::new();
        use crate::sketch::Constraint as C;
        match tool {
            Tool::Line => {
                if let Some(&l) = new
                    .iter()
                    .find(|e| sk.entity(**e).is_some_and(|x| x.is_line()))
                {
                    let (a, b) = sk.line_points(l).unwrap_or_default();
                    if let Some(len) = vals[0] {
                        extra.push(C::Distance { a, b, value: len });
                    }
                    if let Some(ang) = vals[1] {
                        let q = (ang / 90.0).round();
                        if (ang - q * 90.0).abs() < 1e-9 {
                            extra.push(if (q as i64).rem_euclid(2) == 0 {
                                C::Horizontal { line: l }
                            } else {
                                C::Vertical { line: l }
                            });
                        } else if let Some(pl) = prev_line
                            && let (Some((p0, p1)), Some((q0, q1))) =
                                (sk.line_geometry(pl), sk.line_geometry(l))
                        {
                            // Angle to the previous line of the chain, as now drawn.
                            let (u, w) = (p1 - p0, q1 - q0);
                            let v = (u.x * w.y - u.y * w.x).atan2(u.dot(&w));
                            extra.push(C::Angle {
                                a: pl,
                                b: l,
                                value: v,
                                supplement: false,
                            });
                        } else {
                            // First line: angle to the sketch X axis via a
                            // horizontal construction reference is not
                            // created; the typed angle only places the point.
                        }
                    }
                }
            }
            Tool::CircleCenter => {
                if let (Some(&c), Some(d)) = (new.first(), vals[0]) {
                    extra.push(C::Diameter {
                        circle: c,
                        value: d,
                    });
                }
            }
            Tool::RectTwoPoint | Tool::RectCenter => {
                let horiz = |sk: &crate::sketch::Sketch, e: EntityId| {
                    sk.line_geometry(e)
                        .map(|(a, b)| (b - a).x.abs() > (b - a).y.abs())
                };
                let h = new.iter().copied().find(|&e| horiz(sk, e) == Some(true));
                let v = new.iter().copied().find(|&e| horiz(sk, e) == Some(false));
                for (line, val) in [(h, vals[0]), (v, vals[1])] {
                    if let (Some(l), Some(val)) = (line, val)
                        && let Ok((a, b)) = sk.line_points(l)
                    {
                        extra.push(C::Distance { a, b, value: val });
                    }
                }
            }
            _ => {}
        }
        for c in extra {
            let _ = sk.add_constraint(c);
        }
        self.solve();
    }

    /// The pattern handle whose grip is under `pos`.
    fn pattern_handle_at(
        &self,
        hs: &[sketch_tools::PatHandle],
        sk: &crate::sketch::Sketch,
        rect: egui::Rect,
        pos: egui::Pos2,
    ) -> Option<sketch_tools::PatGrip> {
        hs.iter()
            .filter_map(|h| {
                let s = self
                    .camera
                    .project(&sk.plane.point_at(h.pos.x, h.pos.y), rect)?;
                Some((h.grip, s.distance(pos)))
            })
            .filter(|(_, d)| *d <= 11.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(g, _)| g)
    }

    /// The current value of the field a handle edits.
    fn handle_value(&self, h: &manip::Handle) -> Option<f64> {
        use crate::part::params::feature_value;
        if let Some(d) = &self.work_draft {
            return match h.field {
                "distance" => Some(d.distance),
                "angle" => Some(d.angle),
                _ => None,
            };
        }
        let f = match (&self.extrude, self.selection) {
            (Some(d), _) => match d.def.clone() {
                DraftDef::Extrude(e) => crate::part::Feature::Extrude(e),
                DraftDef::Revolve(r) => crate::part::Feature::Revolve(r),
            },
            (None, Selection::Feature(k)) => self.part().features.get(k)?.clone(),
            _ => return None,
        };
        feature_value(&f, h.field)
    }

    /// Applies a manipulator value: live to the dialog, on release to a
    /// feature (one Undo step).
    fn manip_apply(&mut self, owner: ManipOwner, field: &str, v: f64) {
        use crate::part::params::set_feature_field_value;
        // A one-sided Extrude dragged through its profile flips direction
        // and keeps a positive distance (Inventor); zero is not a distance.
        let flip_extrude = |f: &mut crate::part::Feature, v: f64| -> Option<f64> {
            if let crate::part::Feature::Extrude(e) = f
                && field == "distance"
            {
                if v.abs() < 1e-9 {
                    return None;
                }
                if v < 0.0 {
                    e.direction = match e.direction {
                        crate::part::ExtentDirection::Default => {
                            crate::part::ExtentDirection::Flipped
                        }
                        crate::part::ExtentDirection::Flipped => {
                            crate::part::ExtentDirection::Default
                        }
                        other => other,
                    };
                    return Some(-v);
                }
            }
            Some(v)
        };
        match owner {
            ManipOwner::Draft => {
                if let Some(d) = self.extrude.as_mut() {
                    let mut f = match d.def.clone() {
                        DraftDef::Extrude(e) => crate::part::Feature::Extrude(e),
                        DraftDef::Revolve(r) => crate::part::Feature::Revolve(r),
                    };
                    let Some(v) = flip_extrude(&mut f, v) else {
                        return;
                    };
                    set_feature_field_value(&mut f, field, v);
                    d.def = match f {
                        crate::part::Feature::Extrude(e) => DraftDef::Extrude(e),
                        crate::part::Feature::Revolve(r) => DraftDef::Revolve(r),
                        _ => return,
                    };
                }
            }
            ManipOwner::Feature(k) => {
                if let Some(mut f) = self.part().features.get(k).cloned() {
                    let Some(v) = flip_extrude(&mut f, v) else {
                        return;
                    };
                    set_feature_field_value(&mut f, field, v);
                    self.edit_feature(k, f);
                }
            }
            ManipOwner::Work => {
                if let Some(d) = self.work_draft.as_mut() {
                    match field {
                        "distance" => d.distance = v,
                        "angle" => d.angle = v,
                        _ => {}
                    }
                }
            }
            ManipOwner::Hole => {}
            ManipOwner::Blend => {
                if let Some(d) = self.blend_draft.as_mut()
                    && v > 0.0
                {
                    let mut f = crate::part::Feature::Blend(d.def.clone());
                    set_feature_field_value(&mut f, field, v);
                    if let crate::part::Feature::Blend(mut b) = f {
                        // A dragged value replaces a typed equation.
                        if let Some(p) = b.params.0.get_mut(field) {
                            p.expr = None;
                        }
                        d.def = b;
                    }
                }
            }
            ManipOwner::Shell => {
                if let Some(d) = self.shell_draft.as_mut()
                    && v > 0.0
                {
                    let mut f = crate::part::Feature::Shell(d.def.clone());
                    set_feature_field_value(&mut f, field, v);
                    if let crate::part::Feature::Shell(s) = f {
                        d.def = s;
                    }
                }
            }
        }
    }

    /// **Offset Element**: an extra offset of one rectangular-pattern
    /// element (SolidWorks' "Instances to Vary").
    fn offset_window(&mut self, ctx: &egui::Context) {
        let Some((i, pid, k, mut v)) = self.offset_edit else {
            return;
        };
        let mut open = true;
        let mut apply = None;
        self.layout
            .place(
                "offset",
                ctx.content_rect(),
                false,
                egui::Window::new("Offset Element")
                    .open(&mut open)
                    .collapsible(false)
                    .resizable(false),
            )
            .show(ctx, |ui| {
                ui.label(format!("Element {} of the pattern", k + 2));
                egui::Grid::new("offset_el").num_columns(2).show(ui, |ui| {
                    ui.label("Along direction 1");
                    ui.add(valuefield::ExprValue::new(&mut v[0]).suffix(" mm"));
                    ui.end_row();
                    ui.label("Along direction 2 / across");
                    ui.add(valuefield::ExprValue::new(&mut v[1]).suffix(" mm"));
                    ui.end_row();
                });
                ui.label(
                    RichText::new("Without direction 2, the second value is across direction 1.")
                        .small()
                        .weak(),
                );
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        apply = Some(v);
                    }
                    if ui.button("Reset").clicked() {
                        apply = Some([0.0, 0.0]);
                    }
                    if ui.button("Cancel").clicked() {
                        self.offset_edit = None;
                    }
                });
            });
        if let Some(e) = self.offset_edit.as_mut() {
            e.3 = v;
        }
        if !open {
            self.offset_edit = None;
        }
        if let Some(v) = apply {
            self.offset_edit = None;
            self.checkpoint("Offset Element");
            let r = self.part_mut().sketches[i]
                .sketch
                .set_pattern_offset(pid, k, v);
            match r {
                Ok(()) => self.solve(),
                Err(e) => {
                    self.history.cancel();
                    self.file_status = Some(Err(format!("Offset Element: {e}")));
                }
            }
        }
    }

    /// Slice Graphics: the cut plane (normal towards the viewer, offset),
    /// when on while a sketch is edited.
    fn slice_plane(&self) -> Option<(crate::kernel::Vector3, f64)> {
        let i = self.editing.filter(|_| self.slice)?;
        let plane = &self.part().sketches.get(i)?.sketch.plane;
        let mut n = plane.normal.into_inner();
        // Cut the side the viewer is on (along the view direction in ortho).
        let towards_eye = self.camera.eye() - self.camera.target;
        if n.dot(&towards_eye) < 0.0 {
            n = -n;
        }
        // A hair in front of the plane, so faces lying in it stay.
        let w = n.dot(&plane.origin.coords) + 1e-5 * self.camera.distance.max(1.0);
        Some((n, w))
    }

    /// Text typed into a field of the open Extrude / Revolve dialog.
    fn draft_field(&mut self, field: &str, text: &str) {
        let Some(d) = self.extrude.clone() else {
            return;
        };
        let cmd = d.def.command();
        let mut f = match d.def {
            DraftDef::Extrude(e) => crate::part::Feature::Extrude(e),
            DraftDef::Revolve(r) => crate::part::Feature::Revolve(r),
        };
        match self.part().apply_draft_field(&mut f, field, text) {
            Ok(()) => {
                if let Some(d) = self.extrude.as_mut() {
                    d.def = match f {
                        crate::part::Feature::Extrude(e) => DraftDef::Extrude(e),
                        crate::part::Feature::Revolve(r) => DraftDef::Revolve(r),
                        _ => return,
                    };
                }
                self.file_status = None;
            }
            Err(e) => self.file_status = Some(Err(format!("{cmd}: {e}"))),
        }
    }

    /// Replaces feature `k` (Extrude or Revolve) and rebuilds the part.
    fn edit_feature(&mut self, k: usize, def: crate::part::Feature) {
        self.checkpoint("Edit Feature");
        if let Some(f) = self.part_mut().features.get_mut(k) {
            *f = def;
        }
        if let Err(e) = self.part_mut().regenerate() {
            self.file_status = Some(Err(e.to_string()));
        }
        self.scene_dirty = true;
    }

    /// Replaces extrusion `k`'s options and rebuilds the part.
    fn edit_extrude(&mut self, k: usize, def: crate::part::ExtrudeFeature) {
        self.edit_feature(k, crate::part::Feature::Extrude(def));
    }

    fn delete_sketch(&mut self, i: usize) {
        self.checkpoint("Delete Sketch");
        let _ = self.part_mut().delete_sketch(i);
        self.editing = match self.editing {
            Some(e) if e == i => None,
            Some(e) if e > i => Some(e - 1),
            other => other,
        };
        self.selection = Selection::Body;
        self.solve();
    }

    /// Creates a demo sketch on the top face of the box and edits it.
    fn load_demo(&mut self, demo: Demo) {
        self.checkpoint(&format!("Demo {}", demo.short_label()));
        let opts = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..self.sketch_opts.clone()
        };
        match self.part_mut().create_sketch(
            SketchPlacement {
                plane: PlaneRef::Origin(OriginPlane::XY),
                ..Default::default()
            },
            &opts,
        ) {
            Ok(i) => {
                let ps = &mut self.part_mut().sketches[i];
                ps.sketch = demo.build(ps.sketch.plane.clone());
                ps.name = format!("{} ({})", ps.name, demo.short_label());
                self.edit_sketch(i);
            }
            Err(e) => self.file_status = Some(Err(e.to_string())),
        }
    }

    /// The closed regions of `sketch`, cached until the sketch changes.
    fn sketch_regions(&self, sketch: &crate::sketch::Sketch) -> Vec<crate::sketch::Region> {
        let key = serde_json::to_string(sketch).unwrap_or_default();
        let mut cache = self.regions_cache.borrow_mut();
        match &*cache {
            Some((k, r)) if *k == key => r.clone(),
            _ => {
                let r = sketch.regions();
                *cache = Some((key, r.clone()));
                r
            }
        }
    }

    /// Re-solves the sketch in edit mode and stores its report.
    fn solve(&mut self) {
        // Parameters first: new dimensions get names, equations their values.
        let part = self.part_mut();
        part.sync_parameters();
        part.evaluate_parameters();
        self.report = self.editing.map(|i| {
            self.part_mut().sketches[i]
                .sketch
                .solve()
                .map_err(|e| e.to_string())
        });
        // Per-geometry freedom: colours fully constrained curves and feeds
        // the Show All Degrees of Freedom glyphs.
        self.dof = self
            .editing
            .map(|i| self.part().sketches[i].sketch.dof_analysis());
    }

    /// Moves a point of the edited sketch to `target` and re-solves, like
    /// dragging in Inventor: first keep the point exactly under the cursor
    /// (temporarily fixed) and let the rest follow; if that is impossible let
    /// the solver pull it to the nearest valid spot; if even that fails,
    /// undo the move.
    fn drag_point(&mut self, id: PointId, target: Point2) {
        let Some(i) = self.editing else { return };
        let Some(pos) = self.part().sketches[i].sketch.point(id).map(|p| p.position) else {
            return;
        };
        self.drag_points(&[id], target - pos);
    }

    /// Moves points by `delta` and re-solves with them held where moved, so
    /// the rest of the sketch follows (Inventor dragging). In Relax mode the
    /// dimensions give way: they are released for the solve and re-measured
    /// afterwards, instead of pulling the geometry back.
    fn drag_points(&mut self, ids: &[PointId], delta: Vector2) {
        let Some(i) = self.editing else { return };
        let relax = self.relax;
        self.checkpoint("Drag");
        let sketch = &mut self.part_mut().sketches[i].sketch;
        let movable: Vec<PointId> = ids
            .iter()
            .copied()
            .filter(|&p| sketch.point(p).is_some_and(|q| !q.fixed))
            .collect();
        if movable.is_empty() {
            return;
        }
        let before = sketch.clone();
        let relaxed = if relax {
            sketch.relax_dimensions()
        } else {
            Vec::new()
        };
        for &p in &movable {
            if let Some(q) = sketch.point(p).map(|q| q.position) {
                let _ = sketch.move_point(p, q + delta);
            }
            let _ = sketch.set_fixed(p, true);
        }
        let held = sketch.solve();
        for &p in &movable {
            let _ = sketch.set_fixed(p, false);
        }
        let ok = |r: &Result<SolveReport, _>| matches!(r, Ok(r) if r.status == SolveStatus::Solved);
        let fine = ok(&held) || ok(&sketch.solve());
        // Inventor: dimensions the drag had to change become driven.
        sketch.settle_relaxed_dimensions(&relaxed);
        if !fine {
            *sketch = before;
        }
        self.solve();
    }

    /// Inventor: a point dropped onto another point becomes Coincident with
    /// it; dropped onto a curve, it is constrained onto that curve. Part of
    /// the same Undo step as the drag; if the sketch cannot be solved with
    /// the new constraint, nothing is added.
    fn attach_dropped_point(&mut self, id: PointId, cursor: egui::Pos2, rect: egui::Rect) {
        let Some(i) = self.editing else { return };
        let sk = &self.part().sketches[i].sketch;
        // The point's own curves (and their other points) are no targets:
        // attaching to them would collapse the curve.
        let own: Vec<EntityId> = sk
            .entities()
            .filter(|(_, e)| e.points().contains(&id))
            .map(|(e, _)| e)
            .collect();
        let own_points = sk.points_of(&own);
        let c = if let Some(q) = overlay::pick_other_point(
            &self.camera,
            rect,
            sk,
            cursor,
            overlay::POINT_SNAP_PX,
            |q| q == id || own_points.contains(&q),
        ) {
            Constraint::Coincident { a: id, b: q }
        } else if let Some((e, _)) = overlay::pick_other_entity(
            &self.camera,
            rect,
            sk,
            cursor,
            overlay::CURVE_SNAP_PX,
            |e| own.contains(&e),
        ) {
            let Some(c) = sk.on_curve(id, e) else { return };
            c
        } else {
            return;
        };
        if sk.constraints().any(|(_, k)| *k == c) {
            return;
        }
        self.checkpoint("Drag");
        let sketch = &mut self.part_mut().sketches[i].sketch;
        let before = sketch.clone();
        let solved = sketch.add_constraint(c).is_ok()
            && matches!(sketch.solve(), Ok(r) if r.status == SolveStatus::Solved);
        if !solved {
            *sketch = before;
        }
        self.solve();
    }

    // ---- files -------------------------------------------------------------

    fn save_project(&mut self, as_folder: bool) {
        let path = std::path::PathBuf::from(self.file_path.trim());
        let result = if as_folder {
            self.project.save_folder(&path, &SaveOptions::default())
        } else {
            self.project.save(&path)
        };
        let ok = result.is_ok();
        self.file_status = Some(
            result
                .map(|()| format!("Saved {}", path.display()))
                .map_err(|e| format!("Save: {e}")),
        );
        if ok {
            self.saved_ok();
        } else {
            self.after_save = None;
            self.close_ok = false;
        }
    }

    fn open_project(&mut self) {
        let path = std::path::PathBuf::from(self.file_path.trim());
        self.file_status = Some(match Project::load(&path) {
            Ok(mut project) => {
                let part = project
                    .documents()
                    .iter()
                    .find(|d| matches!(d.data, DocumentData::Part(_)))
                    .map(|d| d.id);
                match part {
                    Some(id) => {
                        if let Some(DocumentData::Part(p)) =
                            project.document_mut(id).map(|d| &mut d.data)
                        {
                            let _ = p.regenerate(); // resolve sketch placements
                        }
                        let mut msg = format!(
                            "Opened '{}' ({} documents)",
                            project.name,
                            project.documents().len()
                        );
                        if let Some(w) = &project.newer_file {
                            msg.push_str(&format!(". Warning: {w}"));
                        }
                        self.project = project;
                        self.part_id = id;
                        self.history.clear();
                        self.editing = None;
                        self.selection = Selection::Body;
                        self.after_restore();
                        self.fit_view();
                        Ok(msg)
                    }
                    None => Err("project contains no part".into()),
                }
            }
            Err(e) => Err(e.to_string()),
        });
    }

    // ---- view --------------------------------------------------------------

    /// Fits the body and the visible sketches (an empty part shows the
    /// origin at a sensible scale).
    fn fit_view(&mut self) {
        let part = self.part();
        let mut bb = part.body.bounding_box();
        for (i, ps) in part.sketches.iter().enumerate() {
            if !ps.visible && self.editing != Some(i) {
                continue;
            }
            for (_, p) in ps.sketch.points() {
                let w = ps.sketch.plane.point_at(p.position.x, p.position.y);
                bb = Some(match bb {
                    Some((lo, hi)) => (lo.inf(&w), hi.sup(&w)),
                    None => (w, w),
                });
            }
        }
        match bb {
            Some((lo, hi)) if nalgebra::distance(&lo, &hi) > 1.0 => self.camera.fit(lo, hi),
            _ => self.camera.fit(
                Point3::new(-50.0, -50.0, -50.0),
                Point3::new(50.0, 50.0, 50.0),
            ),
        }
    }

    /// Snaps the camera to a standard view (ViewCube, View tab, F6).
    fn set_view(&mut self, view: StandardView) {
        let before = self.camera.clone();
        let (yaw, pitch) = match (view, self.home_view) {
            (StandardView::Home, Some(h)) => h,
            _ => view.angles(),
        };
        self.camera.yaw = yaw.to_radians();
        self.camera.pitch = pitch.to_radians();
        self.camera.roll = 0.0;
        if view == StandardView::Home {
            self.fit_view();
        }
        self.view_cube.animate(before, &mut self.camera);
    }

    /// Right-click on the ViewCube (Inventor's ViewCube menu).
    fn cube_menu(&mut self, ui: &mut egui::Ui) {
        if ui.button("Home").clicked() {
            self.set_view(StandardView::Home);
            ui.close();
        }
        for p in [
            camera::Projection::Orthographic,
            camera::Projection::Perspective,
            camera::Projection::PerspectiveOrthoFaces,
        ] {
            if ui.radio(self.camera.projection == p, p.label()).clicked() {
                self.apply(Action::Projection(p));
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Set Current View as Home").clicked() {
            self.home_view = Some((self.camera.yaw.to_degrees(), self.camera.pitch.to_degrees()));
            ui.close();
        }
        if self.home_view.is_some() && ui.button("Reset Home").clicked() {
            self.home_view = None;
            ui.close();
        }
        if ui.button("Fit (F6 zooms Home)").clicked() {
            self.fit_view();
            ui.close();
        }
    }

    /// A click on the ViewCube: Home, a face/edge/corner, an arrow to the
    /// neighbouring face or a 90° roll.
    fn cube_clicked(&mut self, hit: viewcube::CubeHit) {
        use viewcube::CubeHit;
        let before = self.camera.clone();
        match hit {
            CubeHit::Home => self.set_view(StandardView::Home),
            CubeHit::Dir(d) => self.camera.look_from(viewcube::direction(d)),
            CubeHit::Compass(Some(d)) => {
                let dir = viewcube::compass_direction(&self.camera, d);
                self.camera.look_from(dir);
            }
            CubeHit::Compass(None) => {}
            CubeHit::Arrow(x, y) => {
                if let Some(d) = viewcube::arrow_direction(&self.camera, x, y) {
                    self.camera.look_from(d);
                }
            }
            CubeHit::Roll(sign) => {
                self.camera.roll += sign as f64 * std::f64::consts::FRAC_PI_2;
            }
        }
        // Smooth transition (Home already animated itself).
        self.view_cube.animate(before, &mut self.camera);
    }

    /// Half-size of the origin-plane squares shown while picking.
    fn origin_plane_half(&self) -> Real {
        let [x, y, z] = self.box_size();
        x.max(y).max(z) * 0.6
    }

    /// Colour encoding the solver state of the edited sketch (Inventor's
    /// meaning of colours).
    fn sketch_color(&self) -> Color32 {
        match &self.report {
            Some(Ok(r)) if r.status == SolveStatus::Failed => Color32::from_rgb(220, 30, 30),
            Some(Ok(r)) if r.redundant > 0 => Color32::from_rgb(230, 130, 0),
            Some(Ok(r)) if r.dof == 0 => Color32::from_rgb(20, 150, 50),
            Some(Ok(_)) => Color32::from_rgb(30, 90, 220),
            Some(Err(_)) => Color32::from_rgb(220, 30, 30),
            None => Color32::from_gray(120),
        }
    }

    fn apply(&mut self, action: Action) {
        // The recent-command list (alias input, command search).
        commands::remember(&mut self.recent_commands, action);
        // Remember commands for Repeat (Inventor: Enter / right-click).
        match action {
            Action::Tool(t) if t != Tool::Select => self.last_command = Some(action),
            Action::Extrude
            | Action::Revolve
            | Action::StartSketch
            | Action::WorkPlane
            | Action::WorkAxis
            | Action::WorkPoint
            | Action::Ucs
            | Action::Fillet
            | Action::Chamfer
            | Action::Shell
            | Action::Sweep
            | Action::Loft
            | Action::Coil
            | Action::Rib
            | Action::Draft
            | Action::Combine
            | Action::Split
            | Action::RectPattern
            | Action::CircPattern
            | Action::MirrorFeature => self.last_command = Some(action),
            Action::Hole | Action::Thread => self.last_command = Some(action),
            _ => {}
        }
        match action {
            Action::StartSketch => {
                self.finish_sketch();
                self.mode = Mode::PickPlane(PickPurpose::NewSketch);
            }
            Action::FinishSketch => {
                self.project_geometry = false;
                self.relink = None;
                self.finish_sketch()
            }
            Action::ProjectGeometry => {
                self.project_geometry = !self.project_geometry;
                self.relink = None;
            }
            Action::CenterPoint => {
                self.tools.center_point = !self.tools.center_point;
                if self.tools.center_point {
                    self.tools.set_tool(Tool::Point);
                }
            }
            Action::EditCoordinateSystem => {
                if let Some(i) = self.editing {
                    self.selection = Selection::Sketch(i);
                }
            }
            Action::LookAt => {
                if let Some(i) = self.editing.or(match self.selection {
                    Selection::Sketch(i) => Some(i),
                    _ => None,
                }) {
                    self.look_at_sketch(i);
                }
            }
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::Demo(d) => self.load_demo(d),
            Action::ShowConstraints(on) => self.show_labels = on,
            Action::SelectBody => self.selection = Selection::Body,
            Action::View(v) => self.set_view(v),
            Action::Options => self.show_options = true,
            Action::Ui(c) => self.ui_command(c),
            Action::Tool(t) => {
                if self.editing.is_some() {
                    self.tools.set_tool(t);
                }
            }
            Action::Format(style) => self.format(style),
            Action::ToggleDof => self.show_dof = !self.show_dof,
            Action::ToggleRelax => self.relax = !self.relax,
            Action::Extrude => self.start_extrude(),
            Action::Revolve => self.start_revolve(),
            Action::Primitive(k) => self.start_primitive(k),
            Action::Fillet => self.start_blend(false, None),
            Action::Chamfer => self.start_blend(true, None),
            Action::Shell => self.start_shell(),
            Action::Sweep => self.start_sweep(),
            Action::Loft => self.start_loft(),
            Action::Coil => self.start_coil(),
            Action::Rib => self.start_rib(),
            Action::Draft => self.start_draft(),
            Action::Combine => self.start_combine(),
            Action::Split => self.start_split(),
            Action::RectPattern => self.start_pattern(crate::part::PatternFeature::rectangular()),
            Action::CircPattern => self.start_pattern(crate::part::PatternFeature::circular()),
            Action::MirrorFeature => self.start_pattern(crate::part::PatternFeature::mirror()),
            Action::Hole => self.start_hole(),
            Action::Thread => self.start_thread(),
            Action::WorkPlane | Action::WorkAxis | Action::WorkPoint | Action::Ucs => {
                self.blend_draft = None;
                self.finish_sketch();
                self.mode = Mode::Normal;
                self.cancel_extrude();
                let kind = match action {
                    Action::WorkAxis => work::WorkKind::Axis,
                    Action::WorkPoint => work::WorkKind::Point,
                    Action::Ucs => work::WorkKind::Ucs,
                    _ => work::WorkKind::Plane,
                };
                self.work_draft = Some(work::WorkDraft::new(kind));
                self.face_sel = None;
                self.edge_sel = None;
            }
            Action::Projection(p) => {
                self.camera.projection = p;
                self.file_status = Some(Ok(format!("View: {}", p.label())));
            }
            Action::Parameters => self.show_params = true,
            Action::RebuildAll => self.rebuild_all(),
            Action::Material(c) => self.material_cmd(c),
            Action::App(c) => self.app_command(c),
        }
    }

    /// Menu and key commands without a ribbon button (the same calls as the
    /// File / Help menus and `shortcuts`).
    fn app_command(&mut self, c: commands::AppCmd) {
        use commands::AppCmd;
        // `request` re-checks the dirty state (forced), so the cached time
        // is as good as the frame's.
        let now = self.dirty_cache.0;
        match c {
            AppCmd::New => self.request(files::Pending::New, now),
            AppCmd::Open => self.request(files::Pending::Open, now),
            AppCmd::Save => self.save(),
            AppCmd::SaveAs => self.open_dialog(files::DialogPurpose::SaveAs),
            AppCmd::SaveAsFolder => self.open_dialog(files::DialogPurpose::SaveFolder),
            AppCmd::Export(f) => self.open_dialog(files::DialogPurpose::Export(f)),
            AppCmd::Help => self.open_help(),
            AppCmd::SaveDiagnostics => self.open_dialog(files::DialogPurpose::Diagnostics),
            AppCmd::OpenLogsFolder => self.open_logs_folder(),
            AppCmd::SliceGraphics => {
                if self.editing.is_some() {
                    self.slice = !self.slice;
                }
            }
            AppCmd::SelectAll => match self.editing {
                Some(i) => {
                    self.tools.selected = self.part().sketches[i]
                        .sketch
                        .entities()
                        .map(|(id, _)| id)
                        .collect();
                }
                None if self.extrude.is_none() => self.select_all(),
                None => {}
            },
            AppCmd::Delete => {
                if self.editing.is_some() {
                    self.delete_selection();
                }
            }
            AppCmd::ZoomAll => self.fit_view(),
            AppCmd::Measure => self.toggle_measure(),
        }
    }

    /// Runs a sketch-tool operation on the edited sketch, recording Undo
    /// when it changed the model and reporting errors in the status bar.
    fn tool_op(&mut self, f: impl FnOnce(&mut SketchTools, &mut PartSketch) -> ClickResult) {
        let Some(i) = self.editing else { return };
        let before = self.project.clone();
        let mut tools = std::mem::take(&mut self.tools);
        let result = f(&mut tools, &mut self.part_mut().sketches[i]);
        self.tools = tools;
        match result {
            Ok(Some(label)) => {
                self.history.break_coalescing();
                self.history.record(&label, before);
                self.file_status = None;
                self.solve();
            }
            Ok(None) => {}
            Err(msg) => self.file_status = Some(Err(msg)),
        }
    }

    /// Format panel: style of the selected entities, or construction mode.
    fn format(&mut self, style: Style) {
        let Some(i) = self.editing else { return };
        let selected = self.tools.selected.clone();
        if selected.is_empty() {
            if style == Style::Construction {
                self.tools.construction = !self.tools.construction;
            }
            return;
        }
        self.checkpoint("Format");
        let ps = &mut self.part_mut().sketches[i];
        for e in selected {
            if !ps.reference_lines.contains(&e) {
                let _ = ps.sketch.set_style(e, style);
            }
        }
    }

    /// Opens the Edit Dimension box for a dimension (double-click on its
    /// value, or Edit from its context menu).
    fn open_dimension_edit(&mut self, c: ConstraintId) {
        let Some(i) = self.dim_sketch() else { return };
        let sk = &self.part().sketches[i].sketch;
        let Some(con) = sk.constraint(c) else { return };
        let Some(value) = con.value() else { return };
        if sk.is_driven(c) {
            self.file_status = Some(Err(
                "Driven dimensions show measured values; make it driving to edit".into(),
            ));
            return;
        }
        let is_angle = matches!(con, crate::sketch::Constraint::Angle { .. });
        self.tools.dim_edit = Some(sketch_tools::DimEdit {
            constraint: c,
            text: sketch_tools::format_value(value, is_angle),
            is_angle,
            focused: false,
        });
    }

    /// Context menu for a constraint or a curve of the edited sketch.
    fn sketch_context_menu(&mut self, ui: &mut egui::Ui, ctx: SketchCtx) {
        let Some(i) = self.editing else { return };
        match ctx {
            SketchCtx::Constraint(c) => {
                let sk = &self.part().sketches[i].sketch;
                let Some(con) = sk.constraint(c).cloned() else {
                    return;
                };
                ui.label(RichText::new(con.name()).strong());
                if con.is_dimension() {
                    let driven = sk.is_driven(c);
                    if !driven && ui.button("Edit Dimension...").clicked() {
                        self.open_dimension_edit(c);
                        ui.close();
                    }
                    if ui
                        .button(if driven {
                            "Make Driving"
                        } else {
                            "Driven Dimension"
                        })
                        .clicked()
                    {
                        self.checkpoint("Driven Dimension");
                        let ps = &mut self.part_mut().sketches[i];
                        if ps.sketch.set_driven(c, !driven).is_ok()
                            && !ps
                                .sketch
                                .solve()
                                .is_ok_and(|r| r.status == SolveStatus::Solved)
                        {
                            let _ = ps.sketch.set_driven(c, driven);
                            self.file_status = Some(Err(
                                "Making it driving would over-constrain the sketch".into(),
                            ));
                        }
                        self.solve();
                        ui.close();
                    }
                    let flippable = matches!(
                        con,
                        crate::sketch::Constraint::HorizontalDistance { .. }
                            | crate::sketch::Constraint::VerticalDistance { .. }
                            | crate::sketch::Constraint::PointLineDistance { .. }
                            | crate::sketch::Constraint::Angle { .. }
                    );
                    if flippable
                        && !driven
                        && ui
                            .button("Flip Direction")
                            .on_hover_text("Move the geometry to the other side")
                            .clicked()
                    {
                        self.checkpoint("Flip Direction");
                        if let Err(e) = self.part_mut().sketches[i].sketch.flip_dimension(c) {
                            self.history.cancel();
                            self.file_status = Some(Err(format!("Flip Direction: {e}")));
                        }
                        self.solve();
                        ui.close();
                    }
                    if sk_has_label(self, i, c) && ui.button("Reset Position").clicked() {
                        self.checkpoint("Move Dimension");
                        self.part_mut().sketches[i]
                            .sketch
                            .set_dimension_label(c, None);
                        ui.close();
                    }
                }
                if ui.button("Delete Constraint").clicked() {
                    self.checkpoint("Delete Constraint");
                    let _ = self.part_mut().sketches[i].sketch.remove_constraint(c);
                    self.tools.selected_constraints.retain(|&x| x != c);
                    self.solve();
                    ui.close();
                }
            }
            SketchCtx::Point(p) => {
                let ps = &self.part().sketches[i];
                let target = ps
                    .link_target_of_point(p)
                    .unwrap_or(crate::part::RefTarget::Point(p));
                self.reference_menu(ui, i, target);
            }
            SketchCtx::OwnPoint(p) => {
                // Format → Center Point on an existing point (Inventor converts
                // selected points with the Center Point button).
                let center = self.part().sketches[i].sketch.center_points().contains(&p);
                let label = if center {
                    "Convert to Normal Point"
                } else {
                    "Convert to Center Point"
                };
                if ui
                    .button(label)
                    .on_hover_text("Center Points are hole centres (a cross)")
                    .clicked()
                {
                    let style = if center {
                        crate::sketch::PointStyle::Normal
                    } else {
                        crate::sketch::PointStyle::Center
                    };
                    self.checkpoint("Center Point");
                    if let Err(e) = self.part_mut().sketches[i].sketch.set_point_style(p, style) {
                        self.history.cancel();
                        self.file_status = Some(Err(format!("Center Point: {e}")));
                    }
                    ui.close();
                }
            }
            SketchCtx::Entity(e)
                if self.part().sketches[i]
                    .link_of(crate::part::RefTarget::Curve(e))
                    .is_some() =>
            {
                self.reference_menu(ui, i, crate::part::RefTarget::Curve(e));
            }
            SketchCtx::Entity(e) => {
                let ps = &self.part().sketches[i];
                let reference = ps.reference_lines.contains(&e);
                let Some(ent) = ps.sketch.entity(e) else {
                    return;
                };
                ui.label(
                    RichText::new(match ent {
                        crate::sketch::Entity::Line { .. } => "Line",
                        crate::sketch::Entity::Circle { .. } => "Circle",
                        crate::sketch::Entity::Arc { .. } => "Arc",
                        crate::sketch::Entity::Spline { .. } => "Spline",
                    })
                    .strong(),
                );
                let points = ent.points();
                if matches!(ent, crate::sketch::Entity::Spline { .. }) {
                    let interp = ps.sketch.is_interp_spline(e);
                    ui.label(
                        RichText::new(if interp {
                            "Interpolation"
                        } else {
                            "Control vertex"
                        })
                        .weak(),
                    );
                    let at = self.context_point;
                    // The fit point under the right-click, if any.
                    let near_point = at.and_then(|q| {
                        points
                            .iter()
                            .copied()
                            .filter_map(|p| {
                                ps.sketch.point(p).map(|x| (p, (x.position - q).norm()))
                            })
                            .min_by(|a, b| a.1.total_cmp(&b.1))
                            .filter(|(_, d)| *d < 1.5)
                            .map(|(p, _)| p)
                    });
                    let shown = self.combs.contains(&e);
                    if ui
                        .checkbox(&mut shown.clone(), "Display Curvature")
                        .clicked()
                    {
                        if shown {
                            self.combs.remove(&e);
                        } else {
                            self.combs.insert(e);
                        }
                        ui.close();
                    }
                    if interp {
                        if let Some(q) = at
                            && ui.button("Insert Point").clicked()
                        {
                            self.checkpoint("Insert Point");
                            let _ = self.part_mut().sketches[i].sketch.insert_spline_point(e, q);
                            self.solve();
                            ui.close();
                        }
                        if let Some(p) = near_point
                            && ui.button("Delete Point").clicked()
                        {
                            self.checkpoint("Delete Point");
                            match self.part_mut().sketches[i].sketch.remove_spline_point(e, p) {
                                Ok(()) => self.solve(),
                                Err(err) => {
                                    self.history.cancel();
                                    self.file_status = Some(Err(format!("Delete Point: {err}")));
                                }
                            }
                            ui.close();
                        }
                        // Handle of the fit point under the right-click.
                        let sk = &self.part().sketches[i].sketch;
                        let fit = sk.spline_fit(e).cloned().unwrap_or_default();
                        let n = points.len();
                        if let Some(k) =
                            near_point.and_then(|p| points.iter().position(|&q| q == p))
                        {
                            if fit.handle(k, n).is_some() {
                                if ui.button("Deactivate Handle").clicked() {
                                    self.checkpoint("Deactivate Handle");
                                    let _ = self.part_mut().sketches[i]
                                        .sketch
                                        .set_spline_handle_at(e, k, None);
                                    self.solve();
                                    ui.close();
                                }
                            } else if ui.button("Activate Handle").clicked() {
                                self.checkpoint("Activate Handle");
                                let _ = self.part_mut().sketches[i]
                                    .sketch
                                    .activate_spline_handle(e, k);
                                self.solve();
                                ui.close();
                            }
                        }
                        if ui
                            .add_enabled(fit.has_handles(), egui::Button::new("Reset All Handles"))
                            .clicked()
                        {
                            self.checkpoint("Reset Handles");
                            let _ = self.part_mut().sketches[i].sketch.reset_spline_handles(e);
                            self.solve();
                            ui.close();
                        }
                        let mut closed = fit.closed;
                        if ui.checkbox(&mut closed, "Close Curve").clicked() {
                            self.checkpoint("Close Curve");
                            match self.part_mut().sketches[i]
                                .sketch
                                .set_spline_closed(e, closed)
                            {
                                Ok(()) => self.solve(),
                                Err(err) => {
                                    self.history.cancel();
                                    self.file_status = Some(Err(format!("Close Curve: {err}")));
                                }
                            }
                            ui.close();
                        }
                    }
                    let label = if interp {
                        "Convert to CV"
                    } else {
                        "Convert to Interpolation"
                    };
                    if ui.button(label).clicked() {
                        self.checkpoint(label);
                        match self.part_mut().sketches[i].sketch.convert_spline(e) {
                            Ok(()) => self.solve(),
                            Err(err) => {
                                self.history.cancel();
                                self.file_status = Some(Err(format!("{label}: {err}")));
                            }
                        }
                        ui.close();
                    }
                    ui.separator();
                }
                let ps = &self.part().sketches[i];
                if let Some((pid, inst)) = ps.sketch.pattern_containing(e) {
                    ui.label(RichText::new("Pattern").weak());
                    let sick = ps.sketch.pattern_health(pid);
                    if let Some(h) = &sick {
                        ui.colored_label(Color32::from_rgb(230, 140, 0), format!("Warning: {h}"));
                    }
                    let kind = ps.sketch.pattern_current_kind(pid);
                    let suppressed = inst.is_some_and(|k| {
                        ps.sketch
                            .pattern(pid)
                            .is_some_and(|p| p.suppressed.contains(&k))
                    });
                    if ui
                        .add_enabled(sick.is_none(), egui::Button::new("Edit Pattern..."))
                        .on_disabled_hover_text("Delete Pattern or Break Pattern to resolve it")
                        .clicked()
                    {
                        if let Some(kind) = kind {
                            self.tools.start_edit_pattern(pid, kind);
                            if let Some(i) = self.editing
                                && let Some(p) = self.part().sketches[i].sketch.pattern(pid)
                            {
                                self.tools.pattern_count_eq = p.count_exprs.clone();
                            }
                        }
                        ui.close();
                    }
                    if let Some(k) = inst {
                        let on = suppressed;
                        let label = if on {
                            "Unsuppress Element"
                        } else {
                            "Suppress Element"
                        };
                        if ui.button(label).clicked() {
                            self.checkpoint(label);
                            let _ = self.part_mut().sketches[i]
                                .sketch
                                .set_pattern_suppressed(pid, k, !on);
                            self.solve();
                            ui.close();
                        }
                        let rect_kind =
                            matches!(kind, Some(crate::sketch::PatternKind::Rectangular { .. }));
                        if ui
                            .add_enabled(
                                rect_kind && sick.is_none(),
                                egui::Button::new("Offset Element..."),
                            )
                            .on_hover_text(
                                "Move this element by an extra offset; the others stay on the grid",
                            )
                            .on_disabled_hover_text("Only elements of rectangular patterns")
                            .clicked()
                        {
                            let cur = self.part().sketches[i].sketch.pattern_offset(pid, k);
                            self.offset_edit = Some((i, pid, k, cur));
                            ui.close();
                        }
                    }
                    if ui.button("Delete Pattern").clicked() {
                        self.checkpoint("Delete Pattern");
                        let _ = self.part_mut().sketches[i].sketch.delete_pattern(pid);
                        self.tools.selected.clear();
                        self.solve();
                        ui.close();
                    }
                    if ui
                        .button("Break Pattern")
                        .on_hover_text("Associative off: the copies become independent geometry")
                        .clicked()
                    {
                        self.checkpoint("Break Pattern");
                        let _ = self.part_mut().sketches[i].sketch.break_pattern(pid);
                        self.solve();
                        ui.close();
                    }
                    ui.separator();
                    return;
                }
                if !reference {
                    for (label, style) in [
                        ("Construction", Style::Construction),
                        ("Centerline", Style::Centerline),
                        ("Normal", Style::Normal),
                    ] {
                        if ui.button(label).clicked() {
                            self.checkpoint("Format");
                            let _ = self.part_mut().sketches[i].sketch.set_style(e, style);
                            ui.close();
                        }
                    }
                    if ui
                        .button("Fix")
                        .on_hover_text("Fix its points where they are")
                        .clicked()
                    {
                        self.checkpoint("Fix");
                        let sk = &mut self.part_mut().sketches[i].sketch;
                        for p in &points {
                            if let Some(at) = sk.point(*p).filter(|q| !q.fixed).map(|q| q.position)
                            {
                                let _ = sk.add_constraint(crate::sketch::Constraint::Fix {
                                    point: *p,
                                    at,
                                });
                            }
                        }
                        self.solve();
                        ui.close();
                    }
                }
                if ui
                    .button("Delete Constraints")
                    .on_hover_text("Remove every constraint on this curve and its points")
                    .clicked()
                {
                    self.checkpoint("Delete Constraints");
                    let sk = &mut self.part_mut().sketches[i].sketch;
                    let doomed: Vec<ConstraintId> = sk
                        .constraints()
                        .filter(|(_, c)| {
                            c.references_entity(e) || points.iter().any(|&p| c.references_point(p))
                        })
                        .map(|(id, _)| id)
                        .collect();
                    for c in doomed {
                        let _ = sk.remove_constraint(c);
                    }
                    self.solve();
                    ui.close();
                }
                if !reference && ui.button("Delete").clicked() {
                    self.tools.selected = vec![e];
                    self.delete_selection();
                    ui.close();
                }
                if ui
                    .button(if self.show_dof {
                        "Hide Degrees of Freedom"
                    } else {
                        "Display Degrees of Freedom"
                    })
                    .clicked()
                {
                    self.show_dof = !self.show_dof;
                    ui.close();
                }
            }
        }
    }

    /// Deletes the selected sketch entities (projected reference geometry
    /// cannot be deleted, as in Inventor).
    fn delete_selection(&mut self) {
        let Some(i) = self.editing else { return };
        let selected = std::mem::take(&mut self.tools.selected);
        let constraints = std::mem::take(&mut self.tools.selected_constraints);
        if selected.is_empty() && constraints.is_empty() {
            return;
        }
        // Inventor: copies of a pattern are not deleted one by one.
        let sk = &self.part().sketches[i].sketch;
        if selected
            .iter()
            .any(|&e| matches!(sk.pattern_containing(e), Some((_, Some(_)))))
        {
            self.tools.selected = selected;
            self.file_status = Some(Err(
                "Delete: pattern elements cannot be deleted one by one; use Suppress Element or Delete Pattern".into(),
            ));
            return;
        }
        // Deleting a pattern's original deletes the pattern.
        let doomed: Vec<_> = selected
            .iter()
            .filter_map(|&e| sk.pattern_containing(e))
            .map(|(id, _)| id)
            .collect();
        self.checkpoint("Delete");
        let ps = &mut self.part_mut().sketches[i];
        for id in doomed {
            let _ = ps.sketch.delete_pattern(id);
        }
        for c in constraints {
            let _ = ps.sketch.remove_constraint(c);
        }
        for e in selected {
            if ps.link_of(crate::part::RefTarget::Curve(e)).is_some() {
                // Projected geometry from Project Geometry can be deleted
                // (with the constraints that used it).
                let _ = ps.delete_reference(crate::part::RefTarget::Curve(e));
                continue;
            }
            if !ps.reference_lines.contains(&e) {
                let points = ps.sketch.entity(e).map(|x| x.points()).unwrap_or_default();
                let _ = ps.sketch.remove_entity(e);
                // Drop points nothing else uses (Inventor deletes them too).
                for p in points {
                    let used = ps.sketch.entities().any(|(_, x)| x.points().contains(&p));
                    if !used && !ps.is_projected(p) {
                        let _ = ps.sketch.remove_point(p);
                    }
                }
            }
        }
        self.solve();
    }

    /// The cursor as a sketch pick: snapped to a point (any, including
    /// projected) or onto a curve, then with H/V inference.
    fn sketch_pick(
        &self,
        i: usize,
        pointer: egui::Pos2,
        rect: egui::Rect,
        ctrl: bool,
    ) -> Option<Pick> {
        let ps = &self.part().sketches[i];
        let plane = &ps.sketch.plane;
        let world = self.camera.ray_plane(pointer, rect, plane)?;
        let raw = plane.project(&world);
        // Drawing tools snap per the inference settings; tools that pick
        // geometry always see points and curves.
        let k = self.tools.inference;
        let drawing = self.tools.drawing();
        // Tools that pick geometry use a small point radius, so a small
        // circle can still be picked by its curve, not its centre.
        let point_px = if drawing {
            overlay::POINT_SNAP_PX
        } else {
            overlay::POINT_PICK_PX
        };
        let point = (!drawing || k.coincident)
            .then(|| overlay::pick_any_point(&self.camera, rect, &ps.sketch, pointer, point_px))
            .flatten();
        let midpoint = ((drawing && k.infer && k.midpoint) || self.tools.takes_midpoints())
            .then(|| {
                overlay::pick_midpoint(
                    &self.camera,
                    rect,
                    &ps.sketch,
                    pointer,
                    overlay::POINT_SNAP_PX,
                )
            })
            .flatten();
        let curve = (!drawing || (k.infer && k.on_curve))
            .then(|| {
                overlay::pick_entity(
                    &self.camera,
                    rect,
                    &ps.sketch,
                    pointer,
                    overlay::CURVE_SNAP_PX,
                )
            })
            .flatten();
        let pick = if let Some(p) = point {
            Pick {
                pos: ps.sketch.point(p)?.position,
                snap: Snap::Point(p),
                infer: None,
                align: Align::default(),
            }
        } else if let Some((line, m)) = midpoint {
            Pick {
                pos: m,
                snap: Snap::Midpoint(line),
                infer: None,
                align: Align::default(),
            }
        } else if let Some((e, on)) = curve {
            Pick {
                pos: on,
                snap: Snap::OnEntity(e),
                infer: None,
                align: Align::default(),
            }
        } else {
            Pick {
                pos: raw,
                snap: Snap::None,
                infer: None,
                align: Align::default(),
            }
        };
        // Sketch millimetres per screen pixel at the cursor.
        let mm_per_px = self
            .camera
            .ray_plane(pointer + egui::vec2(1.0, 0.0), rect, plane)
            .map_or(0.0, |w| (plane.project(&w) - raw).norm());
        Some(self.tools.infer(&ps.sketch, pick, ctrl, mm_per_px))
    }

    /// Global function keys and shortcuts (Inventor defaults, docs/UI.md).
    fn shortcuts(&mut self, ctx: &egui::Context) {
        // ---- A0 (spec 0.15 §4.4): the panel key of this frame, see `keys`.
        keys::set_panel_key(ctx, None);
        self.tab_keys(ctx);
        // Esc ends a measure into a field, and only that.
        if self.measure_pick.is_some()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape))
        {
            self.measure_pick = None;
            return;
        }
        // Alt+S: the command search, also from a text field or a command.
        self.search_shortcut(ctx);
        if ctx.egui_wants_keyboard_input() || self.file_window_open() {
            return; // typing into a text field
        }
        if self.hud_keys(ctx) {
            return;
        }
        // A letter with the alias field on opens it instead of starting.
        if keys::letter_alias(ctx.input(|i| i.modifiers)) && self.alias_start(ctx) {
            return;
        }
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        let (cmd, shift) = ctx.input(|i| (i.modifiers.command, i.modifiers.shift));
        // Single letters: alone or with Shift, never with Ctrl / Alt.
        let letter = keys::letter_alias(ctx.input(|i| i.modifiers));
        if pressed(Key::F1) {
            self.open_help();
        }
        let open = self.open_state();
        // Enter / Ctrl+Enter in a feature panel: its OK reads the key.
        keys::set_panel_key(ctx, keys::panel_key(&open, pressed(Key::Enter), cmd));
        // Enter in a command with selectors: Continue / OK (`command_ui`).
        if open.flow && pressed(Key::Enter) {
            self.command_enter(cmd);
        }
        // Enter (or Space) with nothing open repeats the last command.
        if (pressed(Key::Enter) || pressed(Key::Space))
            && !cmd
            && open.repeat_on_key()
            && let Some(a) = self.repeatable()
        {
            self.apply(a);
        }
        // ---- end A0
        if pressed(Key::F7) && self.editing.is_some() {
            self.slice = !self.slice;
        }
        if pressed(Key::F11) && self.editing.is_some() {
            self.relax = !self.relax;
        }
        if pressed(Key::F6) {
            self.set_view(StandardView::Home);
        }
        if pressed(Key::F8) {
            self.show_labels = true;
        }
        if pressed(Key::F9) {
            self.show_labels = false;
        }
        if cmd && shift && pressed(Key::S) {
            self.open_dialog(files::DialogPurpose::SaveAs);
        } else if cmd && pressed(Key::S) {
            self.save();
        } else if letter && pressed(Key::S) && self.editing.is_none() {
            self.apply(Action::StartSketch);
        }
        if letter && pressed(Key::M) && self.editing.is_none() {
            self.apply(Action::App(commands::AppCmd::Measure));
        }
        if letter && pressed(Key::E) {
            self.apply(Action::Extrude);
        }
        if letter && pressed(Key::R) && self.editing.is_none() {
            self.apply(Action::Revolve);
        }
        if letter && pressed(Key::H) && self.editing.is_none() {
            self.apply(Action::Hole);
        }
        // Ctrl+A outside a sketch: everything of the selection mode.
        if cmd && pressed(Key::A) && self.editing.is_none() && self.extrude.is_none() {
            self.select_all();
        }
        // Ctrl+A in a sketch: select all its curves.
        if cmd
            && pressed(Key::A)
            && let Some(i) = self.editing
        {
            self.tools.selected = self.part().sketches[i]
                .sketch
                .entities()
                .map(|(id, _)| id)
                .collect();
        }
        if cmd && pressed(Key::O) {
            let now = ctx.input(|i| i.time);
            self.request(files::Pending::Open, now);
        }
        if cmd && pressed(Key::N) {
            let now = ctx.input(|i| i.time);
            self.request(files::Pending::New, now);
        }
        if cmd && (pressed(Key::Y) || (shift && pressed(Key::Z))) {
            self.redo();
        } else if cmd && pressed(Key::Z) {
            self.undo();
        }
        if self.editing.is_some() && !cmd {
            let tool = [
                (Key::L, Tool::Line),
                (Key::C, Tool::CircleCenter),
                (Key::R, Tool::RectTwoPoint),
                (Key::D, Tool::Dimension),
                (Key::X, Tool::Trim),
                (Key::O, Tool::Offset),
            ]
            .into_iter()
            .find(|(k, _)| letter && pressed(*k));
            if let Some((_, t)) = tool {
                self.tools.set_tool(t);
            }
            if pressed(Key::Enter) {
                self.tool_op(|t, ps| t.finish(ps));
            }
            if pressed(Key::Delete) {
                self.delete_selection();
            }
        }
        // Esc first closes the selection menu (Shift + right-click).
        if pressed(Key::Escape) && self.sel_menu.take().is_none() {
            self.select_other = None;
            self.project_geometry = false;
            self.relink = None;
            self.work_draft = None;
            self.blend_draft = None;
            self.shell_draft = None;
            self.sweep_draft = None;
            self.loft_draft = None;
            self.coil_draft = None;
            self.rib_draft = None;
            self.draft_draft = None;
            self.close_body_cmds();
            self.pattern_draft = None;
            self.cancel_hole();
            self.thread_draft = None;
            self.end_feature_edit();
            self.face_sel = None;
            self.edge_sel = None;
            if !self.sel.is_empty() {
                self.prev_sel = std::mem::take(&mut self.sel);
            }
            if self.editing.is_some() && self.tools.tool != Tool::Select {
                self.tools.escape();
            } else {
                self.tools.selected.clear();
            }
            self.mode = Mode::Normal;
            self.cancel_extrude();
            self.tools.edit_pattern = None;
            self.target_pick = None;
            self.dragging = None;
            self.arc_drag = None;
            self.file_status = None;
        }
    }

    /// Quick Access bar with the File menu.
    fn quick_access(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.menu_button(RichText::new("File").strong(), |ui| {
                ui.set_min_width(260.0);
                self.file_menu(ui);
            });
            ui.menu_button("View", |ui| {
                ui.set_min_width(220.0);
                self.view_menu(ui);
            })
            .response
            .on_hover_text("User Interface: panels, Clean Screen (Ctrl+0), Full Screen (F11)");
            ui.separator();
            let now = ui.input(|i| i.time);
            if ui.button("Open").on_hover_text("Ctrl+O").clicked() {
                self.request(files::Pending::Open, now);
            }
            if ui.button("Save").on_hover_text("Ctrl+S").clicked() {
                self.save();
            }
            ui.separator();
            let undo = self
                .history
                .undo_label()
                .map(|l| format!("Undo {l} (Ctrl+Z)"));
            if ui
                .add_enabled(undo.is_some(), egui::Button::new("Undo"))
                .on_hover_text(undo.unwrap_or_default())
                .clicked()
            {
                self.undo();
            }
            let redo = self
                .history
                .redo_label()
                .map(|l| format!("Redo {l} (Ctrl+Y)"));
            if ui
                .add_enabled(redo.is_some(), egui::Button::new("Redo"))
                .on_hover_text(redo.unwrap_or_default())
                .clicked()
            {
                self.redo();
            }
            if ui
                .button("Rebuild")
                .on_hover_text("Rebuild All: recompute every feature from scratch")
                .clicked()
            {
                self.rebuild_all();
            }
            ui.separator();
            self.select_menu(ui);
            ui.separator();
            self.material_qat(ui);
            ui.separator();
            let dirty = self.dirty_cache.1;
            ui.label(
                RichText::new(format!(
                    "{}{}",
                    self.document_title(),
                    if dirty { " *" } else { "" }
                ))
                .strong(),
            )
            .on_hover_text(if dirty { "Unsaved changes" } else { "Saved" });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("Help", |ui| {
                    ui.set_min_width(200.0);
                    self.help_menu(ui);
                });
                self.command_search(ui);
                ui.label(RichText::new("3DCadCraft").weak());
            });
        });
    }

    fn build_scene(&self) -> SceneGeometry {
        let mut g = SceneGeometry::default();

        // Ground grid on XY, 10 mm spacing.
        let grid = [0.55, 0.56, 0.6];
        for i in -10..=10 {
            let t = i as f64 * 10.0;
            g.add_line(
                &Point3::new(t, -100.0, 0.0),
                &Point3::new(t, 100.0, 0.0),
                grid,
            );
            g.add_line(
                &Point3::new(-100.0, t, 0.0),
                &Point3::new(100.0, t, 0.0),
                grid,
            );
        }
        // World axes.
        let o = Point3::origin();
        g.add_line(&o, &Point3::new(30.0, 0.0, 0.0), [0.9, 0.1, 0.1]);
        g.add_line(&o, &Point3::new(0.0, 30.0, 0.0), [0.1, 0.7, 0.1]);
        g.add_line(&o, &Point3::new(0.0, 0.0, 30.0), [0.1, 0.2, 0.9]);

        // Faces in their appearance (face > feature > body > part > material).
        material::add_part_meshes(&mut g, self.part());
        // Cosmetic threads: a banded texture on their faces (one mesh).
        if self.thread_texture {
            g.add_thread_bands(&crate::part::ThreadBands::of(&self.part().cosmetic_threads));
        }
        let body = &self.part().body;
        // Selected (blue) and prehighlighted (green) faces: drawn again just
        // outside the body, so the depth test hides them behind other faces.
        let eps = body
            .bounding_box()
            .map_or(1e-3, |(a, b)| ((b - a).norm() * 2e-4).max(1e-4));
        let opts = TessellationOptions::default();
        let mut paint = |faces: Vec<crate::kernel::FaceId>, color: [f32; 3]| {
            for f in faces {
                if let Ok(m) = body.face_mesh(f, &opts) {
                    let pos: Vec<Point3> = m
                        .positions
                        .iter()
                        .zip(&m.normals)
                        .map(|(p, n)| p + n * eps)
                        .collect();
                    g.add_mesh(&pos, &m.normals, &m.triangles, color);
                }
            }
        };
        let (sel, hover) = &self.scene_sel;
        for x in sel {
            paint(self.item_faces(x), [0.25, 0.5, 0.95]);
        }
        if let Some(h) = hover
            && !sel.contains(h)
        {
            paint(self.item_faces(h), [0.4, 0.82, 0.45]);
        }
        g
    }

    /// The plane under the cursor. A model face wins over the origin planes
    /// (which are large, translucent and usually *in front of* the faces
    /// the user is aiming at); origin planes are picked where no face is.
    fn plane_under(
        &self,
        pos: egui::Pos2,
        rect: egui::Rect,
        with_origin_planes: bool,
    ) -> Option<PlaneRef> {
        use crate::command::selection::Category;
        let (origin, dir) = self.camera.ray(pos, rect)?;
        let body = &self.part().body;
        if let Some((f, _)) = self.face_hit(&origin, &dir)
            && let Some(name) = body.face(f).and_then(|f| f.name.clone())
        {
            return Some(PlaneRef::Face(name));
        }
        let work = self
            .work_plane_under(pos, rect)
            .map(|(id, t)| (t, PlaneRef::Work(id)));
        if !with_origin_planes || !self.cat_on(Category::Origin) {
            return work.map(|(_, p)| p);
        }
        let half = self.origin_plane_half();
        OriginPlane::ALL
            .into_iter()
            .filter_map(|p| {
                Some((
                    overlay::hit_origin_plane(p, half, &origin, &dir)?,
                    PlaneRef::Origin(p),
                ))
            })
            .chain(work)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, p)| p)
    }

    fn viewport(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let (rect, mut response) =
            ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.viewport_rect = rect;
        // What may be picked this frame: selector kinds ∩ user filter.
        self.update_pick_filter();
        let pointer = response.hover_pos();
        // --- ViewCube: running transition, cube/compass drags (they own
        // the pointer for the whole drag, before anything else picks) ---
        if self.view_cube.tick(&mut self.camera, ui.input(|i| i.time)) {
            ui.ctx().request_repaint();
        }
        let scrolled = response.hovered() && ui.input(|i| i.smooth_scroll_delta.y != 0.0);
        let press = ui.input(|i| i.pointer.press_origin());
        let cube_on = self.layout.visible(layout::Element::ViewCube);
        let cube_drag = cube_on
            && self
                .view_cube
                .input(&mut self.camera, rect, &response, press, scrolled);
        let on_cube = if cube_on {
            self.view_cube
                .drag_hit()
                .or_else(|| pointer.and_then(|p| viewcube::pick(&self.camera, rect, p)))
        } else {
            None
        };

        // Measure into a field takes the click first (`measure_pick`).
        if on_cube.is_none() {
            self.measure_pick_click(&mut response, rect);
        }
        // --- picking a sketch plane ---
        let picking = matches!(self.mode, Mode::PickPlane(_));
        let plane_hover = match (picking, pointer, on_cube) {
            (true, Some(p), None) => self.plane_under(p, rect, true),
            _ => None,
        };
        if picking
            && response.clicked_by(PointerButton::Primary)
            && on_cube.is_none()
            && let Some(plane) = plane_hover.clone()
        {
            self.plane_picked(plane);
        }
        // --- model selection: faces light up under the cursor, a click
        // selects one (and its feature) and shows the suggestions ---
        // --- Fillet / Chamfer command: picks edges ---
        let faces_mode = self.blend_draft.as_ref().is_some_and(|d| d.picks_faces());
        let blend_hover = match (&self.blend_draft, pointer, on_cube) {
            (Some(_), Some(p), None) if !faces_mode => self.edge_under(p, rect),
            _ => None,
        };
        // Loop / Feature / Full Round pick faces.
        let blend_face_hover = match (&self.blend_draft, pointer, on_cube) {
            (Some(_), Some(p), None) if faces_mode => match self.plane_under(p, rect, false) {
                Some(PlaneRef::Face(name)) => Some((name, p)),
                _ => None,
            },
            _ => None,
        };
        if self.blend_draft.is_some()
            && response.clicked_by(PointerButton::Primary)
            && let Some(e) = blend_hover
        {
            let body = self.part().body.clone();
            if let Some(d) = self.blend_draft.as_mut() {
                d.pick(&body, e);
            }
        }
        if self.blend_draft.is_some()
            && response.clicked_by(PointerButton::Primary)
            && let Some((name, p)) = blend_face_hover.clone()
        {
            self.blend_face_pick(&name, p, rect);
        }
        // --- Shell / Face Draft / Hole / Thread: pick faces (with the model
        // point) ---
        let face_cmd = self.shell_draft.is_some()
            || self.hole_draft.is_some()
            || self.thread_draft.is_some()
            || self.draft_draft.as_ref().is_some_and(|d| d.picks_faces());
        let shell_hover = match (face_cmd, pointer, on_cube) {
            (true, Some(p), None) => self.camera.ray(p, rect).and_then(|(o, dir)| {
                let dir = dir.try_normalize(1e-12)?;
                let (f, t) = self.face_hit(&o, &dir)?;
                Some((f, o + dir * t))
            }),
            _ => None,
        };
        if self.shell_draft.is_some()
            && response.clicked_by(PointerButton::Primary)
            && let Some((f, at)) = shell_hover
        {
            self.shell_pick(f, at);
        }
        if self.thread_draft.is_some()
            && response.clicked_by(PointerButton::Primary)
            && let Some((f, at)) = shell_hover
        {
            self.thread_pick(f, at);
        }
        // Hole: a sketch point, a circular edge or a planar face.
        let hole_hover = match (&self.hole_draft, pointer, on_cube) {
            (Some(_), Some(p), None) => {
                let e = self.edge_under(p, rect);
                self.hole_pick_under(p, rect, e, shell_hover)
            }
            _ => None,
        };
        // A placement dimension's value: edit in place / its menu.
        let on_hole_dim = self.hole_draft.is_some()
            && on_cube.is_none()
            && pointer.is_some_and(|p| self.hole_dim_at(p, rect).is_some());
        let dim_took = on_hole_dim && self.hole_dim_input(&response, pointer, rect);
        if self.hole_draft.is_some()
            && !dim_took
            && response.clicked_by(PointerButton::Primary)
            && let Some(pick) = hole_hover.clone()
        {
            self.hole_pick(pick);
        }
        // Combine / Split: bodies, faces, planes, sketches (`gui/combine_split.rs`).
        let body_cmd = self.combine_draft.is_some() || self.split_draft.is_some();
        let body_hover = match (body_cmd, pointer, on_cube) {
            (true, Some(p), None) => self.body_cmd_hover(p, rect),
            _ => None,
        };
        if body_cmd
            && response.clicked_by(PointerButton::Primary)
            && let Some(h) = body_hover.clone()
        {
            self.body_cmd_pick(h);
        }
        // Face Draft: what its active selector takes (`gui/draft.rs`).
        let draft_hover = match (self.draft_draft.is_some(), pointer, on_cube) {
            (true, Some(p), None) => self.draft_hover(p, rect, shell_hover),
            _ => None,
        };
        if self.draft_draft.is_some()
            && response.clicked_by(PointerButton::Primary)
            && let Some(h) = draft_hover.clone()
        {
            self.draft_pick(h);
        }
        let draft_edge = match &draft_hover {
            Some(draft::DraftPick::Edge(e)) => Some(*e),
            _ => None,
        };
        let draft_plane = match &draft_hover {
            Some(draft::DraftPick::Plane(p)) => Some(p.clone()),
            _ => None,
        };
        // --- Pattern / Mirror: features (faces), solids, directions,
        // axes and planes ---
        let pat_geom = self
            .pattern_draft
            .as_ref()
            .is_some_and(|d| d.picks_geometry());
        let pat_face = match (pat_geom, pointer, on_cube) {
            (true, Some(p), None) => self.camera.ray(p, rect).and_then(|(o, dir)| {
                let dir = dir.try_normalize(1e-12)?;
                self.face_hit(&o, &dir).map(|(f, _)| f)
            }),
            _ => None,
        };
        let (pat_edge, pat_plane) = match (&self.pattern_draft, pointer, on_cube) {
            (Some(d), Some(p), None) if !d.picks_geometry() => {
                let e = if d.picks_lines() {
                    self.edge_under(p, rect)
                        .filter(|&e| self.part().body.edge_direction(e).is_some())
                } else {
                    None
                };
                let pl = if e.is_none() && d.picks_planes() {
                    self.plane_under(p, rect, true)
                } else {
                    None
                };
                (e, pl)
            }
            _ => (None, None),
        };
        if self.pattern_draft.is_some() && response.clicked_by(PointerButton::Primary) {
            if let Some(f) = pat_face {
                self.pattern_pick_face(f);
            } else if pat_edge.is_some() || pat_plane.is_some() {
                self.pattern_pick_ref(pat_edge, pat_plane.clone());
            }
        }
        // --- Work Plane command: picks planes, edges and points ---
        let wp_hover = match (&self.work_draft, pointer, on_cube) {
            (Some(d), Some(p), None) => self.wp_input_under(p, rect, d.kind),
            _ => None,
        };
        if self.work_draft.is_some()
            && response.clicked_by(PointerButton::Primary)
            && let Some(input) = wp_hover.clone()
            && let Some(d) = self.work_draft.as_mut()
        {
            d.pick(input);
        }
        let idle = !picking
            && self.work_draft.is_none()
            && self.blend_draft.is_none()
            && self.shell_draft.is_none()
            && !self.sweep_loft_open()
            && !self.coil_rib_open()
            && self.draft_draft.is_none()
            && !body_cmd
            && self.pattern_draft.is_none()
            && self.hole_draft.is_none()
            && self.thread_draft.is_none()
            && self.editing.is_none()
            && self.extrude.is_none()
            && self.prim_draft.is_none()
            && self.target_pick.is_none()
            && on_cube.is_none();
        // Edges first (vertex > edge > face, 0-ui §3.2); the other
        // categories come from `sel_item_at` (one pick order, one filter).
        let edge_hover = match (idle, pointer) {
            (true, Some(p)) if self.dragging.is_none() => self.edge_under(p, rect),
            _ => None,
        };
        let face_hover = match (idle, pointer, edge_hover) {
            (true, Some(p), None) => match self.plane_under(p, rect, false) {
                Some(PlaneRef::Face(name)) => Some(name),
                _ => None,
            },
            _ => None,
        };
        // Select Other: resting ~0.7 s over several candidates (every
        // category that passes the filters, at any priority).
        let now = ui.input(|i| i.time);
        let mut edge_hover = edge_hover;
        let mut face_hover = face_hover;
        let mut other_hover: Option<select::SelItem> = None;
        if idle {
            match (pointer, self.rest) {
                (Some(p), Some((at, _))) if p.distance(at) <= 3.0 => {}
                (Some(p), _) => {
                    self.rest = Some((p, now));
                    // Leaving the control's neighbourhood closes it.
                    if self
                        .select_other
                        .as_ref()
                        .is_some_and(|s| s.at.distance(p) > 60.0)
                    {
                        self.select_other = None;
                    }
                }
                (None, _) => {}
            }
            if self.select_other.is_none()
                && self.sel_menu.is_none()
                && let (Some(p), Some((at, since))) = (pointer, self.rest)
                && p.distance(at) <= 3.0
                && now - since > 0.7
                && self.dragging.is_none()
            {
                let items = self.sel_candidates(p, rect);
                if items.len() > 1 {
                    self.select_other = Some(SelectOther {
                        at: p,
                        items,
                        index: 0,
                    });
                }
            }
            if self.select_other.is_some() {
                ui.ctx().request_repaint();
                let accept = self.select_other_ui(ui, rect);
                if let Some(so) = &self.select_other {
                    let cur = so.items[so.index].clone();
                    let at = so.at;
                    (edge_hover, face_hover) = match &cur {
                        select::SelItem::Edge(e) => (Some(*e), None),
                        select::SelItem::Face(f) => (None, Some(f.clone())),
                        _ => (None, None),
                    };
                    other_hover = Some(cur.clone());
                    if accept {
                        self.select_other = None;
                        self.idle_select(Some(cur), at, false, false);
                    }
                }
            } else if pointer.is_some() && self.rest.is_some_and(|(_, s)| now - s < 0.8) {
                // Wake up once the rest time has passed.
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(750));
            }
        } else {
            self.select_other = None;
        }
        if response.clicked_by(PointerButton::Primary)
            && on_cube.is_none()
            && let Some(p) = pointer
        {
            self.select_other = None;
            if self.prim_draft.is_some() {
                self.primitive_click(p, rect);
            } else if self.target_pick.is_some() {
                self.target_picked(p, rect);
            } else if self.extrude.as_ref().is_some_and(|d| d.axis_pick) {
                self.axis_pick(p, rect);
            } else if self.extrude.is_some() {
                if self.cat_on(crate::command::selection::Category::SketchEntity) {
                    self.extrude_pick(p, rect);
                }
            } else if idle {
                let (add, toggle) = ui.input(|k| (k.modifiers.shift, k.modifiers.command));
                let item = self.sel_item_at(p, rect, edge_hover, face_hover.as_deref());
                self.idle_select(item, p, add, toggle);
            }
        }
        // Double click: Select Tangencies (faces and edges).
        if idle
            && response.double_clicked_by(PointerButton::Primary)
            && let Some(p) = pointer
            && let Some(item) = self.sel_item_at(p, rect, edge_hover, face_hover.as_deref())
            && matches!(item, select::SelItem::Edge(_) | select::SelItem::Face(_))
        {
            self.face_sel = None;
            self.edge_sel = None;
            if !self.sel.contains(&item) {
                self.sel.push(item.clone());
            }
            self.select_tangencies(&item);
        }
        if !idle {
            self.face_sel = None;
            self.edge_sel = None;
        }
        // The body was rebuilt: drop what no longer exists.
        let key = self.body_key();
        if key != self.sel_key {
            self.sel_key = key;
            let part = self.part();
            let alive = |x: &select::SelItem| match x {
                select::SelItem::Face(n) => part.body.find_face(n).is_some(),
                select::SelItem::Edge(_) => false,
                select::SelItem::Feature(id) => part.features.iter().any(|f| f.id() == id),
                select::SelItem::Body(n) => part
                    .body
                    .solids()
                    .any(|(_, s)| s.name.as_deref() == Some(n.as_str())),
                select::SelItem::Sketch(i) => *i < part.sketches.len(),
                select::SelItem::Vertex(n) => part
                    .body
                    .vertices()
                    .any(|(_, v)| v.name.as_deref() == Some(n.as_str())),
            };
            let sel: Vec<_> = self.sel.iter().filter(|x| alive(x)).cloned().collect();
            let prev: Vec<_> = self.prev_sel.iter().filter(|x| alive(x)).cloned().collect();
            self.sel = sel;
            self.prev_sel = prev;
        }
        // A selected edge that vanished (regeneration, Undo).
        if let Some((e, _)) = self.edge_sel
            && self.part().body.edge(e).is_none()
        {
            self.edge_sel = None;
        }

        // --- right-click: New Sketch on the face under the cursor; inside
        // the edited sketch: the constraint label or curve under it ---
        let label_hit = match (self.dim_sketch(), pointer) {
            (Some(i), Some(p)) if self.show_labels && !picking && on_cube.is_none() => {
                overlay::label_layout(
                    ui.painter(),
                    &self.camera,
                    rect,
                    &self.part().sketches[i].sketch,
                    self.dim_display,
                )
                .into_iter()
                .rev()
                .find(|l| l.rect.contains(p))
                .map(|l| l.constraint)
            }
            _ => None,
        };
        // A click on another dimension while one is edited (in the box or in
        // a value field just used) puts its *name* there: `d3 = d1`
        // follows d1, not only its value (Inventor).
        // Show Dimensions: a dimension's name goes into its field; a face
        // picks the feature whose dimensions show.
        if self.show_dims.is_some() && response.clicked_by(PointerButton::Primary) {
            let named = label_hit.and_then(|c| self.dimension_name_for_link(c));
            let took = match (named, pointer) {
                (Some(n), _) => {
                    self.show_dims_pick(ui.ctx(), n);
                    true
                }
                (None, Some(p)) => self.show_dims_face(p, rect),
                _ => false,
            };
            if took {
                response.flags.remove(
                    egui::response::Flags::CLICKED | egui::response::Flags::FAKE_PRIMARY_CLICKED,
                );
            }
        }
        if response.clicked_by(PointerButton::Primary)
            && let Some(c) = label_hit
            && let Some(name) = self.dimension_name_for_link(c)
        {
            let field = valuefield::recent_field(ui.ctx(), ui.input(|i| i.time));
            let into_box = self
                .tools
                .dim_edit
                .as_ref()
                .is_some_and(|e| e.constraint != c);
            if into_box && let Some(e) = self.tools.dim_edit.as_mut() {
                // A plain value is replaced, an equation gets the name added.
                if crate::part::expr::is_plain_number(&e.text) || e.text.trim().is_empty() {
                    e.text = name;
                } else {
                    e.text.push_str(&name);
                }
                e.focused = false;
                response.flags.remove(
                    egui::response::Flags::CLICKED | egui::response::Flags::FAKE_PRIMARY_CLICKED,
                );
            } else if self.tools.dim_edit.is_none()
                && let Some(id) = field
            {
                ui.ctx()
                    .data_mut(|d| d.insert_temp(measure_pick::result_id(id), name));
                response.flags.remove(
                    egui::response::Flags::CLICKED | egui::response::Flags::FAKE_PRIMARY_CLICKED,
                );
            }
        }
        // Extrude / Revolve open: a double click on a dimension of the
        // profile sketch edits it in place (the preview follows).
        if self.editing.is_none()
            && self.extrude.is_some()
            && response.double_clicked_by(PointerButton::Primary)
            && let Some(c) = label_hit
        {
            self.open_dimension_edit(c);
        }
        if response.secondary_clicked() {
            self.cube_context = on_cube.is_some();
        }
        // Shift + right-click: the selection menu at the cursor (Inventor).
        if response.hovered() && ui.input(|i| i.pointer.button_pressed(PointerButton::Secondary)) {
            self.shift_rmb = ui.input(|i| i.modifiers.shift);
        }
        if response.secondary_clicked()
            && self.shift_rmb
            && on_cube.is_none()
            && let Some(p) = pointer
        {
            self.select_other = None;
            let menu = self.shift_menu_context(p, rect);
            self.sel_menu = Some((p, rect, menu));
            self.sel_menu_fresh = true;
        }
        if response.secondary_clicked()
            && let Some(p) = pointer
        {
            self.context_target = self.plane_under(p, rect, false);
            self.context_point = self.editing.and_then(|i| {
                let plane = &self.part().sketches[i].sketch.plane;
                self.camera
                    .ray_plane(p, rect, plane)
                    .map(|w| plane.project(&w))
            });
            let point_under = self.editing.and_then(|i| {
                let ps = &self.part().sketches[i];
                overlay::pick_any_point(&self.camera, rect, &ps.sketch, p, overlay::POINT_PICK_PX)
                    .map(|q| (q, ps.is_projected(q)))
                    // A curve's end or centre keeps the curve's menu; only
                    // standalone points get the point menu.
                    .filter(|&(q, projected)| {
                        projected || !ps.sketch.entities().any(|(_, e)| e.points().contains(&q))
                    })
            });
            self.sketch_context = match (label_hit, self.editing) {
                (Some(c), _) => Some(SketchCtx::Constraint(c)),
                (None, Some(_)) if point_under.is_some() => point_under.map(|(q, projected)| {
                    if projected {
                        SketchCtx::Point(q)
                    } else {
                        SketchCtx::OwnPoint(q)
                    }
                }),
                (None, Some(i)) => overlay::pick_entity(
                    &self.camera,
                    rect,
                    &self.part().sketches[i].sketch,
                    p,
                    6.0,
                )
                .map(|(e, _)| SketchCtx::Entity(e)),
                _ => None,
            };
        }
        let sketch_context = self.sketch_context;
        let context_target = self.context_target.clone();
        // Shift + right button: the selection menu instead (spec 0.9a §6).
        if !self.shift_rmb && !on_hole_dim {
            response.context_menu(|ui| {
                if self.cube_context {
                    self.cube_menu(ui);
                    return;
                }
                // An open command: its menu (OK, Continue, Back, options…).
                if self.command_context_menu(ui) {
                    return;
                }
                // Extrude / Revolve: Continue (to the axis), OK, Cancel.
                if self.editing.is_none()
                    && let Some(d) = self.extrude.as_ref()
                {
                    let revolve_profiles = matches!(d.def, DraftDef::Revolve(_)) && !d.axis_pick;
                    if revolve_profiles
                        && ui
                            .button("Continue")
                            .on_hover_text("Done with the profiles: pick the axis")
                            .clicked()
                    {
                        if let Some(d) = self.extrude.as_mut() {
                            d.axis_pick = true;
                        }
                        ui.close();
                    }
                    if ui.button("OK (Ctrl+Enter)").clicked() {
                        self.commit_extrude();
                        ui.close();
                    }
                    if ui.button("Cancel (Esc)").clicked() {
                        self.cancel_extrude();
                        ui.close();
                    }
                    ui.separator();
                }
                // No Repeat while any command is open (spec 0.15 §3.1.1, A0).
                if self.open_state().repeat_in_menu()
                    && let Some(a) = self.repeatable()
                {
                    if ui.button(format!("Repeat {}", command_label(a))).clicked() {
                        self.apply(a);
                        ui.close();
                    }
                    ui.separator();
                }
                if self.editing.is_some() && self.project_geometry {
                    if ui
                        .button("Done (Esc)")
                        .on_hover_text("End Project Geometry")
                        .clicked()
                    {
                        self.project_geometry = false;
                        self.relink = None;
                        ui.close();
                    }
                    ui.separator();
                }
                if self.editing.is_some()
                    && let Some(ctx) = sketch_context
                {
                    self.sketch_context_menu(ui, ctx);
                    ui.separator();
                }
                if self.editing.is_none() && !self.sel.is_empty() {
                    self.appearance_context_menu(ui);
                    self.measure_context(ui);
                }
                ui.menu_button("Projection", |ui| {
                    for p in [
                        camera::Projection::Orthographic,
                        camera::Projection::Perspective,
                        camera::Projection::PerspectiveOrthoFaces,
                    ] {
                        if ui.radio(self.camera.projection == p, p.label()).clicked() {
                            self.apply(Action::Projection(p));
                            ui.close();
                        }
                    }
                });
                match &context_target {
                    Some(plane) => {
                        if ui.button("New Sketch").clicked() {
                            self.mode = Mode::PickPlane(PickPurpose::NewSketch);
                            self.plane_picked(plane.clone());
                            ui.close();
                        }
                    }
                    None => {
                        ui.label(RichText::new("No face under the cursor").weak());
                    }
                }
                if self.editing.is_some() {
                    if self.tools.tool != Tool::Select {
                        if ui
                            .button("Done")
                            .on_hover_text("End the line chain / create the spline")
                            .clicked()
                        {
                            self.tool_op(|t, ps| t.finish(ps));
                            ui.close();
                        }
                        if ui.button("Cancel (Esc)").clicked() {
                            self.tools.escape();
                            ui.close();
                        }
                    }
                    if ui.button("Finish 2D Sketch").clicked() {
                        self.finish_sketch();
                        ui.close();
                    }
                }
            });
        }

        // --- sketch tools ---
        let ctrl = ui.input(|i| i.modifiers.ctrl);
        let shift_held = ui.input(|i| i.modifiers.shift);
        let selecting = self.tools.tool == Tool::Select;
        let in_sketch = self.editing.filter(|_| !picking && on_cube.is_none());
        let cursor_pick = match (in_sketch, pointer) {
            (Some(i), Some(p)) => self.sketch_pick(i, p, rect, ctrl),
            _ => None,
        };
        // Dynamic Input: reset when the step changes, then apply the locks.
        let hud_key = Some((self.tools.tool, self.tools.pick_count()));
        if self.hud.key != hud_key {
            self.hud = Hud {
                key: hud_key,
                ..Default::default()
            };
        }
        let cursor_pick = cursor_pick.map(|p| self.hud_apply(p));
        if cursor_pick.is_some() {
            self.last_pick = cursor_pick;
        }
        self.tools.shift = shift_held;
        let hovered = match (in_sketch, pointer) {
            (Some(i), Some(p)) if self.dragging.is_none() && selecting && label_hit.is_none() => {
                overlay::pick_point(
                    &self.camera,
                    rect,
                    &self.part().sketches[i].sketch,
                    p,
                    overlay::POINT_SNAP_PX,
                )
            }
            _ => None,
        };
        let hovered_entity = match (in_sketch, pointer) {
            (Some(i), Some(p))
                if hovered.is_none()
                    && label_hit.is_none()
                    && (selecting || self.tools.picks_geometry()) =>
            {
                overlay::pick_entity(
                    &self.camera,
                    rect,
                    &self.part().sketches[i].sketch,
                    p,
                    overlay::CURVE_SNAP_PX,
                )
                .map(|(e, _)| e)
            }
            _ => None,
        };
        if in_sketch.is_some()
            && response.double_clicked_by(PointerButton::Primary)
            && let Some(c) = label_hit
        {
            self.open_dimension_edit(c);
        }
        let dim_label = label_hit.filter(|&c| {
            self.tools.tool == Tool::Dimension
                && in_sketch.is_some_and(|i| {
                    self.part().sketches[i]
                        .sketch
                        .constraint(c)
                        .is_some_and(crate::sketch::Constraint::is_dimension)
                })
        });
        let projected = match (in_sketch, pointer) {
            (Some(i), Some(p))
                if self.project_geometry && response.clicked_by(PointerButton::Primary) =>
            {
                self.project_click(i, p, rect, shift_held)
            }
            _ => false,
        };
        if projected {
        } else if let Some(c) = dim_label
            && response.clicked_by(PointerButton::Primary)
        {
            // Inventor: the Dimension command on an existing dimension edits it.
            self.open_dimension_edit(c);
        } else if in_sketch.is_some() && response.clicked_by(PointerButton::Primary) {
            if selecting && let Some(c) = label_hit {
                // Constraint glyph / dimension: select it (Shift adds).
                if !shift_held {
                    self.tools.selected.clear();
                    self.tools.selected_constraints.clear();
                }
                let sel = &mut self.tools.selected_constraints;
                if let Some(k) = sel.iter().position(|&x| x == c) {
                    sel.remove(k);
                } else {
                    sel.push(c);
                }
            } else if selecting
                && self.tools.skip_pick
                && let (Some((pid, _)), Some(e), Some(i)) =
                    (self.tools.edit_pattern, hovered_entity, in_sketch)
                && let Some((p2, Some(k))) = self.part().sketches[i].sketch.pattern_containing(e)
                && p2 == pid
            {
                // Instances to Skip: toggle the clicked copy.
                let on = self.part().sketches[i]
                    .sketch
                    .pattern(pid)
                    .is_some_and(|p| p.suppressed.contains(&k));
                self.checkpoint("Suppress Element");
                let _ = self.part_mut().sketches[i]
                    .sketch
                    .set_pattern_suppressed(pid, k, !on);
                self.solve();
            } else if selecting {
                if !shift_held {
                    self.tools.selected_constraints.clear();
                }
                match hovered_entity {
                    Some(e) => {
                        if !shift_held {
                            self.tools.selected.clear();
                        }
                        if let Some(k) = self.tools.selected.iter().position(|&x| x == e) {
                            self.tools.selected.remove(k);
                        } else {
                            self.tools.selected.push(e);
                        }
                    }
                    None => self.tools.selected.clear(),
                }
            } else if let Some(mut pick) = cursor_pick {
                // Automatic projection (Inventor): Dimension / a constraint
                // on a model edge projects it and takes the projected line.
                let wants_edge = self.tools.picks_geometry()
                    && pick.snap == Snap::None
                    && (self.tools.tool != Tool::Dimension || self.tools.pick_count() < 2);
                if wants_edge
                    && let (Some(i), Some(p)) = (in_sketch, pointer)
                    && let Some(e) = self.edge_under(p, rect)
                    && self.part().body.edge_direction(e).is_some()
                {
                    self.checkpoint("Project Geometry");
                    match self.part_mut().project_edge(i, e) {
                        Ok(line) => {
                            self.solve();
                            pick.snap = Snap::OnEntity(line);
                        }
                        Err(err) => {
                            self.history.cancel();
                            self.file_status = Some(Err(format!("Project Geometry: {err}")));
                        }
                    }
                }
                self.hud_click(pick);
            }
        }
        // Double-click on a curve: select its chain (Fusion; SolidWorks'
        // Select Chain).
        if selecting
            && response.double_clicked_by(PointerButton::Primary)
            && let (Some(i), Some(e)) = (in_sketch, hovered_entity)
        {
            let (chain, _) = self.part().sketches[i].sketch.chain(e);
            if !shift_held {
                self.tools.selected.clear();
            }
            for (c, _) in chain {
                if !self.tools.selected.contains(&c) {
                    self.tools.selected.push(c);
                }
            }
        }
        if in_sketch.is_some()
            && matches!(self.tools.tool, Tool::SplineCv | Tool::SplineInterp)
            && response.double_clicked_by(PointerButton::Primary)
        {
            self.tool_op(|t, ps| t.finish(ps));
        }
        if response.clicked()
            && let Some(hit) = on_cube
        {
            self.cube_clicked(hit);
        }
        // --- sketch-pattern handles (Edit Pattern) ---
        if response.drag_started_by(PointerButton::Primary)
            && let (Some(i), Some(p)) = (in_sketch, ui.input(|i| i.pointer.press_origin()))
            && self.tools.pattern_active()
        {
            let sk = &self.part().sketches[i].sketch;
            let hs = self.tools.pattern_handles(sk);
            if let Some(g) = self.pattern_handle_at(&hs, sk, rect, p) {
                self.pattern_drag = Some((g, String::new()));
            }
        }
        if let (Some((g, _)), Some(i)) = (self.pattern_drag.clone(), in_sketch)
            && response.dragged_by(PointerButton::Primary)
            && let Some(cursor) = response.interact_pointer_pos()
        {
            let plane = self.part().sketches[i].sketch.plane.clone();
            if let Some(w) = self.camera.ray_plane(cursor, rect, &plane) {
                let at = plane.project(&w);
                let sk = self.part().sketches[i].sketch.clone();
                if let Some(t) = self.tools.drag_pattern_handle(&sk, g, at) {
                    self.pattern_drag = Some((g, t));
                }
            }
        }
        if response.drag_stopped() {
            self.pattern_drag = None;
        }

        // --- manipulators (before sketch dragging and orbiting) ---
        let handles = self.handles();
        let hot_handle = match (&handles, pointer) {
            (Some((_, hs)), Some(p)) => manip::hit(hs, &self.camera, rect, p),
            _ => None,
        };
        if response.drag_started_by(PointerButton::Primary)
            && on_cube.is_none()
            && let (Some((owner, hs)), Some(p)) = (&handles, ui.input(|i| i.pointer.press_origin()))
            && let Some(i) = manip::hit(hs, &self.camera, rect, p)
        {
            self.manip_drag = Some((*owner, hs[i].clone(), f64::NAN));
        }
        if let Some((owner, h, _)) = self.manip_drag.clone()
            && response.dragged_by(PointerButton::Primary)
            && let Some(cursor) = response.interact_pointer_pos()
            && let Some((o, d)) = self.camera.ray(cursor, rect)
            && let Some(raw) = manip::drag_value(&h.geom, &o, &d)
        {
            let fine = ui.input(|i| i.modifiers.ctrl);
            let v = manip::snap(&h, raw, fine);
            if let Some(m) = self.manip_drag.as_mut()
                && m.2 != v
            {
                m.2 = v;
                if owner == ManipOwner::Hole {
                    self.hole_manip(&h, v);
                } else if owner.live() {
                    self.manip_apply(owner, h.field, v);
                }
            }
        }
        if response.drag_stopped()
            && let Some((owner, h, v)) = self.manip_drag.take()
        {
            if !owner.live() && v.is_finite() {
                self.manip_apply(owner, h.field, v);
            }
            self.hole_drag = None;
            self.history.break_coalescing();
        }
        // Idle view: a box from empty space selects (window / crossing).
        if response.drag_started_by(PointerButton::Primary)
            && self.manip_drag.is_none()
            && idle
            && self.mouse == MousePreset::Inventor
            && !ui.input(|i| i.key_down(Key::F2) || i.key_down(Key::F3) || i.key_down(Key::F4))
            && let Some(press) = ui.input(|i| i.pointer.press_origin())
            && self
                .sel_item_at(press, rect, self.edge_under(press, rect), None)
                .is_none()
            && self.plane_under(press, rect, false).is_none()
        {
            self.box_select = Some(press);
        }
        if response.drag_stopped()
            && idle
            && let (Some(a), Some(b)) = (self.box_select.take(), pointer)
        {
            let (add, toggle) = ui.input(|k| (k.modifiers.shift, k.modifiers.command));
            if a.distance(b) > 3.0 {
                let items = self.box_items(rect, a, b);
                self.face_sel = None;
                self.edge_sel = None;
                self.sel_box(items, add, toggle);
            }
        }
        // Extrude / Revolve: a box from empty space picks profiles (Inventor:
        // window select of several closed profiles).
        if response.drag_started_by(PointerButton::Primary)
            && self.manip_drag.is_none()
            && in_sketch.is_none()
            && on_cube.is_none()
            && self.mouse == MousePreset::Inventor
            && self.target_pick.is_none()
            && self.extrude.as_ref().is_some_and(|d| !d.axis_pick)
            && !ui.input(|i| i.key_down(Key::F2) || i.key_down(Key::F3) || i.key_down(Key::F4))
        {
            self.box_select = ui.input(|i| i.pointer.press_origin());
        }
        if response.drag_stopped()
            && in_sketch.is_none()
            && self.extrude.is_some()
            && let (Some(a), Some(b)) = (self.box_select.take(), pointer)
        {
            let (add, toggle) = ui.input(|k| (k.modifiers.shift, k.modifiers.command));
            self.extrude_box_pick(rect, a, b, add, toggle);
        }
        if response.drag_started_by(PointerButton::Primary) && self.manip_drag.is_none() {
            self.history.break_coalescing();
            // egui reports the drag only after the pointer has travelled a
            // few pixels, so pick the point where the button went *down*.
            let press = ui.input(|i| i.pointer.press_origin());
            self.dragging = None;
            self.arc_drag = None;
            self.dragging_dim = None;
            // Text of a dimension under the press: drag the text, not geometry.
            if let (Some(i), Some(p), true) = (in_sketch, press, self.show_labels) {
                let sk = &self.part().sketches[i].sketch;
                self.dragging_dim =
                    overlay::label_layout(ui.painter(), &self.camera, rect, sk, self.dim_display)
                        .into_iter()
                        .rev()
                        .find(|l| {
                            l.rect.contains(p)
                                && sk
                                    .constraint(l.constraint)
                                    .is_some_and(crate::sketch::Constraint::is_dimension)
                        })
                        .map(|l| l.constraint);
            }
            if let (Some(i), Some(p), None, None) =
                (in_sketch, press, self.dragging_dim, &self.pattern_drag)
            {
                let sk = &self.part().sketches[i].sketch;
                match self.tools.tool {
                    Tool::Select if self.spline_grip_at(i, rect, p).is_some() => {
                        self.handle_drag = self.spline_grip_at(i, rect, p);
                    }
                    Tool::Select => {
                        let point =
                            overlay::pick_point(&self.camera, rect, sk, p, overlay::POINT_SNAP_PX);
                        // Not on a point: drag the whole curve under the press.
                        let curve = match point {
                            None => overlay::pick_entity(
                                &self.camera,
                                rect,
                                sk,
                                p,
                                overlay::CURVE_SNAP_PX,
                            )
                            .zip(self.camera.ray_plane(p, rect, &sk.plane))
                            .map(|((e, _), w)| (e, sk.plane.project(&w))),
                            Some(_) => None,
                        };
                        // Empty space: box selection (Inventor mouse).
                        if point.is_none() && curve.is_none() && self.mouse == MousePreset::Inventor
                        {
                            self.box_select = Some(p);
                        }
                        self.dragging = point;
                        self.dragging_entity = curve;
                    }
                    // Line tool: drag from a curve end → tangent/perpendicular arc.
                    Tool::Line => {
                        if let Some(q) = overlay::pick_any_point(
                            &self.camera,
                            rect,
                            sk,
                            p,
                            overlay::POINT_SNAP_PX,
                        ) && sk.entities().any(|(_, e)| {
                            (e.is_line() || e.is_circular()) && e.endpoints().contains(&q)
                        }) {
                            let start = sk.point(q).map_or(Point2::origin(), |x| x.position);
                            self.tools.set_tool(Tool::Line); // the drag replaces the click chain
                            self.arc_drag = Some((q, start, None));
                        }
                    }
                    _ => {}
                }
            }
        }
        if let (Some((q, start, dir)), Some(pick)) = (self.arc_drag, cursor_pick)
            && dir.is_none()
        {
            let d = pick.pos - start;
            if d.norm() > 1e-6 && response.drag_delta() != egui::Vec2::ZERO {
                let travelled = ui
                    .input(|i| i.pointer.press_origin())
                    .zip(pointer)
                    .map_or(0.0, |(a, b)| a.distance(b));
                if travelled > 10.0 {
                    self.arc_drag = Some((q, start, Some(d)));
                }
            }
        }
        if response.drag_stopped()
            && let (Some(a), Some(b), Some(i)) = (self.box_select.take(), pointer, in_sketch)
        {
            let (add, toggle) = ui.input(|k| (k.modifiers.shift, k.modifiers.command));
            let hit = self.box_hits(i, rect, a, b);
            let sel = &mut self.tools.selected;
            if !add && !toggle {
                sel.clear();
                self.tools.selected_constraints.clear();
            }
            for e in hit {
                match sel.iter().position(|&x| x == e) {
                    Some(k) if toggle => {
                        sel.remove(k);
                    }
                    Some(_) => {}
                    None => sel.push(e),
                }
            }
        }
        if response.drag_stopped() {
            self.handle_drag = None;
            self.box_select = None;
            if let (Some((q, _, Some(dir))), Some(pick)) = (self.arc_drag.take(), cursor_pick) {
                let to = pick.pos;
                self.tool_op(|t, ps| t.arc_drag(ps, q, dir, to));
            }
            if let (Some(id), Some(p)) = (self.dragging, pointer) {
                self.attach_dropped_point(id, p, rect);
            }
            self.dragging = None;
            self.dragging_entity = None;
            self.dragging_dim = None;
            self.arc_drag = None;
            self.history.break_coalescing();
        }

        // --- spline handle drag: H = 3 (cursor − end point) ---
        if let (Some((e, k)), Some(i)) = (self.handle_drag, in_sketch)
            && response.dragged_by(PointerButton::Primary)
            && let Some(cursor) = response.interact_pointer_pos()
        {
            let plane = self.part().sketches[i].sketch.plane.clone();
            if let Some(w) = self.camera.ray_plane(cursor, rect, &plane) {
                self.checkpoint("Spline Handle");
                let _ = self.part_mut().sketches[i]
                    .sketch
                    .set_spline_handle_grip_at(e, k, plane.project(&w));
                self.solve();
            }
        }

        // --- navigation ---
        let delta = response.drag_delta();
        let (shift, f2, f3, f4) = ui.input(|i| {
            (
                i.modifiers.shift,
                i.key_down(Key::F2),
                i.key_down(Key::F3),
                i.key_down(Key::F4),
            )
        });
        let left = response.dragged_by(PointerButton::Primary);
        let middle = response.dragged_by(PointerButton::Middle);
        let right = response.dragged_by(PointerButton::Secondary);
        enum Nav {
            None,
            Orbit,
            Pan,
            Zoom,
        }
        let nav = match self.mouse {
            // A drag on the ViewCube orbits by itself.
            _ if cube_drag => Nav::None,
            // Function keys take priority over point dragging, as in Inventor.
            MousePreset::Inventor if left && f4 => Nav::Orbit,
            MousePreset::Inventor if left && f2 => Nav::Pan,
            MousePreset::Inventor if left && f3 => Nav::Zoom,
            MousePreset::Inventor if middle && shift => Nav::Orbit,
            MousePreset::Inventor if middle => Nav::Pan,
            MousePreset::Classic
                if left
                    && self.dragging.is_none()
                    && self.manip_drag.is_none()
                    && self.pattern_drag.is_none() =>
            {
                Nav::Orbit
            }
            // Shift + right button opens the selection menu, never a pan.
            MousePreset::Classic if (right && !self.shift_rmb) || middle => Nav::Pan,
            _ => Nav::None,
        };
        match nav {
            Nav::Orbit => self.camera.orbit(delta.x, delta.y),
            Nav::Pan => self.camera.pan(delta.x, delta.y, rect.height()),
            Nav::Zoom => self.camera.zoom(-delta.y * 3.0),
            Nav::None if self.dragging_dim.is_some() && left => {
                if let (Some(c), Some(i), Some(cursor)) = (
                    self.dragging_dim,
                    self.editing,
                    response.interact_pointer_pos(),
                ) {
                    let plane = self.part().sketches[i].sketch.plane.clone();
                    if let Some(w) = self.camera.ray_plane(cursor, rect, &plane) {
                        let at = plane.project(&w);
                        self.checkpoint("Move Dimension");
                        self.part_mut().sketches[i]
                            .sketch
                            .set_dimension_label(c, Some(at));
                    }
                }
            }
            Nav::None if self.dragging_entity.is_some() && left => {
                if let (Some((e, last)), Some(i), Some(cursor)) = (
                    self.dragging_entity,
                    self.editing,
                    response.interact_pointer_pos(),
                ) {
                    let sk = &self.part().sketches[i].sketch;
                    let plane = sk.plane.clone();
                    let pts = sk.points_of(&[e]);
                    if let Some(w) = self.camera.ray_plane(cursor, rect, &plane) {
                        let now = plane.project(&w);
                        self.drag_points(&pts, now - last);
                        self.dragging_entity = Some((e, now));
                    }
                }
            }
            Nav::None => {
                if let (Some(id), Some(i)) = (self.dragging, self.editing)
                    && left
                    && let Some(cursor) = response.interact_pointer_pos()
                {
                    let plane = self.part().sketches[i].sketch.plane.clone();
                    if let Some(world) = self.camera.ray_plane(cursor, rect, &plane) {
                        self.drag_point(id, plane.project(&world));
                    }
                }
            }
        }
        if response.hovered() && !(self.select_other.is_some() && ui.input(|i| i.modifiers.shift)) {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.camera
                    .zoom(if self.invert_wheel { -scroll } else { scroll });
            }
        }
        if response.double_clicked_by(PointerButton::Middle)
            || (self.mouse == MousePreset::Classic && response.double_clicked())
        {
            self.fit_view();
        }

        // --- 3D ---
        let painter = ui.painter_at(rect);
        let scene = std::mem::take(&mut self.scene_dirty).then(|| self.build_scene());
        let slice = self.slice_plane();
        match (frame.wgpu_render_state(), self.renderer.as_mut()) {
            (Some(rs), Some(renderer)) => {
                if let Some(scene) = scene {
                    renderer.upload(rs, &scene);
                }
                let ppp = ui.ctx().pixels_per_point();
                let size = [
                    (rect.width() * ppp).round() as u32,
                    (rect.height() * ppp).round() as u32,
                ];
                let aspect = (rect.width() / rect.height().max(1.0)) as f64;
                let tex = renderer.render(
                    rs,
                    size,
                    &self.camera.view_projection(aspect),
                    &self.camera.eye(),
                    slice,
                );
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                painter.image(tex, rect, uv, Color32::WHITE);
            }
            _ => {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "wgpu is not available",
                    egui::FontId::proportional(18.0),
                    Color32::RED,
                );
            }
        }

        // --- overlays ---
        let hovered_work = match (&plane_hover, &body_hover) {
            (Some(PlaneRef::Work(id)), _)
            | (_, Some(combine_split::BodyPick::Plane(PlaneRef::Work(id)))) => Some(id.as_str()),
            _ => None,
        };
        let hovered_work = hovered_work
            .map(str::to_owned)
            .or_else(|| match (idle, pointer) {
                (true, Some(p)) => self.work_feature_under(p, rect),
                _ => None,
            });
        // Inputs of an open command first, the work features (and the
        // preview) on top of them.
        if let Some(d) = &self.work_draft {
            overlay::draw_origin_planes(
                &painter,
                &self.camera,
                rect,
                self.origin_plane_half(),
                None,
            );
            for input in &d.picks {
                self.draw_wp_input(&painter, rect, input, Color32::from_rgb(40, 110, 230));
            }
            if let Some(h) = &wp_hover {
                self.draw_wp_input(&painter, rect, h, Color32::from_rgb(60, 190, 90));
            }
        }
        self.draw_work_features(&painter, rect, hovered_work.as_deref());
        self.draw_blend_edges(
            &painter,
            rect,
            blend_hover,
            blend_face_hover.as_ref().map(|(n, _)| n.as_str()),
        );
        if picking {
            let hovered_origin = match &plane_hover {
                Some(PlaneRef::Origin(p)) => Some(*p),
                _ => None,
            };
            overlay::draw_origin_planes(
                &painter,
                &self.camera,
                rect,
                self.origin_plane_half(),
                hovered_origin,
            );
            if let Some(PlaneRef::Face(name)) = &plane_hover {
                let body = &self.part().body;
                if let Some(mesh) = body
                    .find_face(name)
                    .and_then(|f| body.face_mesh(f, &TessellationOptions::default()).ok())
                {
                    overlay::draw_face_highlight(&painter, &self.camera, rect, &mesh);
                }
            }
        }
        // Split's tool selector: the origin planes are shown while it takes
        // planes (Inventor shows them for the pick), the hovered one lit;
        // also always while View → Object Visibility shows them.
        if !picking
            && self.work_draft.is_none()
            && (self.obj_vis.origin_planes
                || self
                    .body_cmd_flow()
                    .is_some_and(|f| f.accepts(crate::command::flow::PickKind::Plane)))
        {
            let hovered_origin = match &body_hover {
                Some(combine_split::BodyPick::Plane(PlaneRef::Origin(p))) => Some(*p),
                _ => None,
            };
            overlay::draw_origin_planes(
                &painter,
                &self.camera,
                rect,
                self.origin_plane_half(),
                hovered_origin,
            );
        }
        // Selected and prehighlighted faces, the name next to the cursor.
        {
            let hover_item = match (idle, pointer) {
                (true, Some(p)) if self.select.prehighlight => other_hover
                    .clone()
                    .or_else(|| self.sel_item_at(p, rect, edge_hover, face_hover.as_deref())),
                _ => None,
            };
            self.draw_selection(&painter, rect, hover_item.as_ref());
            // Shell: its faces to remove, and the face under the cursor.
            let cmd_faces = if self.shell_draft.is_some() {
                Some(self.shell_faces())
            } else if self.draft_draft.is_some() {
                Some(self.draft_faces())
            } else if body_cmd {
                Some(self.body_cmd_faces())
            } else if self.pattern_draft.is_some() {
                Some(self.pattern_faces())
            } else if let Some(d) = &self.thread_draft {
                let body = &self.part().body;
                Some(
                    d.face
                        .iter()
                        .filter_map(|r| {
                            crate::part::shell::face_near(body, &r.face, Point3::from(r.hint))
                        })
                        .collect(),
                )
            } else if let Some(d) = &self.hole_draft {
                let body = &self.part().body;
                Some(d.face.iter().filter_map(|n| body.find_face(n)).collect())
            } else {
                None
            };
            let (want_sel, want_hover) = match cmd_faces {
                Some(faces) => {
                    let body = &self.part().body;
                    let name = |f| body.face(f).and_then(|x| x.name.clone());
                    let plane_face = match draft_plane.as_ref().or(pat_plane.as_ref()) {
                        Some(PlaneRef::Face(n)) => Some(n.clone()),
                        _ => None,
                    };
                    (
                        faces
                            .into_iter()
                            .filter_map(|f| name(f).map(select::SelItem::Face))
                            .collect(),
                        shell_hover
                            .map(|(f, _)| f)
                            .or(pat_face)
                            .and_then(name)
                            .or(plane_face)
                            .map(select::SelItem::Face)
                            .or_else(|| self.body_cmd_hover_item(body_hover.as_ref())),
                    )
                }
                None => (
                    self.sel.clone(),
                    hover_item.or_else(|| self.browser_hover.clone().map(select::SelItem::Feature)),
                ),
            };
            if let Some(e) = pat_edge.filter(|_| draft_edge.is_none()) {
                let opts = TessellationOptions::default();
                if let Ok(pts) = self.part().body.sample_edge(e, &opts) {
                    self.draw_visible_polyline(
                        &painter,
                        rect,
                        &pts,
                        egui::Stroke::new(4.0, egui::Color32::from_rgb(60, 190, 90)),
                    );
                }
            }
            self.draw_draft_overlay(&painter, rect, draft_hover.as_ref());
            self.draw_command_hint(&painter, rect, pointer);
            self.draw_split_arrow(&painter, rect);
            self.draw_pattern_refs(&painter, rect);
            if self.scene_sel.0 != want_sel || self.scene_sel.1 != want_hover {
                self.scene_sel = (want_sel, want_hover);
                self.scene_dirty = true;
            }
            self.draw_holes_and_threads(&painter, rect);
            self.hole_dim_windows(ui.ctx(), rect);
            self.draw_measure(&painter, rect);
            self.draw_measure_pick(&painter, rect);
            self.sweep_loft_view(
                &painter,
                rect,
                pointer.filter(|_| on_cube.is_none()),
                response.clicked_by(PointerButton::Primary),
            );
            self.coil_rib_view(
                &painter,
                rect,
                pointer.filter(|_| on_cube.is_none()),
                response.clicked_by(PointerButton::Primary),
            );
            if let Some(hole::HolePick::Circle(r)) = &hole_hover
                && let Some(e) = self.part().body.edges().map(|(e, _)| e).find(|&e| {
                    crate::part::blend::edge_ref(&self.part().body, e).as_ref() == Some(r)
                })
                && let Ok(pts) = self
                    .part()
                    .body
                    .sample_edge(e, &TessellationOptions::default())
            {
                self.draw_visible_polyline(
                    &painter,
                    rect,
                    &pts,
                    egui::Stroke::new(4.0, egui::Color32::from_rgb(60, 190, 90)),
                );
            }
            let preview = if self.shell_draft.is_some() {
                Some(self.shell_preview())
            } else if self.draft_draft.is_some() {
                Some(self.draft_preview())
            } else if body_cmd {
                Some(self.body_cmd_preview())
            } else if self.pattern_draft.is_some() {
                Some(self.pattern_preview())
            } else if self.hole_draft.is_some() {
                Some(self.hole_preview())
            } else if self.thread_draft.is_some() {
                Some(self.thread_preview())
            } else {
                None
            };
            if let Some(Ok(edges)) = preview {
                let stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(230, 120, 20));
                for e in edges {
                    let pts: Vec<_> = e
                        .iter()
                        .filter_map(|p| self.camera.project(p, rect))
                        .collect();
                    painter.add(egui::Shape::line(pts, stroke));
                }
            }
            let tip = match (&face_hover, edge_hover) {
                (_, Some(e)) => Some(self.edge_description(e)),
                (Some(name), None) => Some(self.face_description(name)),
                _ => None,
            };
            if let (Some(text), Some(p)) = (tip, pointer)
                && self.dragging.is_none()
                && self.select_other.is_none()
            {
                let font = egui::FontId::proportional(12.0);
                let galley = painter.layout_no_wrap(text, font, Color32::from_gray(30));
                let r = egui::Rect::from_min_size(
                    p + egui::vec2(16.0, 18.0),
                    galley.size() + egui::vec2(10.0, 4.0),
                );
                painter.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(255, 255, 230, 235));
                painter.rect_stroke(
                    r,
                    3.0,
                    egui::Stroke::new(1.0, Color32::from_gray(150)),
                    egui::StrokeKind::Inside,
                );
                painter.galley(r.min + egui::vec2(5.0, 2.0), galley, Color32::from_gray(30));
            }
        }
        if self.face_sel.is_some() {
            self.face_toolbar(ui, rect);
        }
        if self.edge_sel.is_some() {
            self.edge_toolbar(ui, rect);
        }
        self.draw_project_hover(&painter, rect, pointer);
        // The References row under the pointer: its geometry lights up.
        if let Some((si, pts)) = self.ref_hover.take()
            && let Some(ps) = self.part().sketches.get(si)
        {
            let col = Color32::from_rgb(230, 120, 20);
            let screen: Vec<_> = pts
                .iter()
                .filter_map(|&p| ps.sketch.point(p))
                .filter_map(|q| {
                    self.camera
                        .project(&ps.sketch.plane.point_at(q.position.x, q.position.y), rect)
                })
                .collect();
            if let [a, b] = screen[..] {
                painter.line_segment([a, b], egui::Stroke::new(4.0, col));
            }
            for s in screen {
                painter.circle_stroke(s, 7.0, egui::Stroke::new(2.0, col));
            }
        }
        if let Some(i) = self.editing {
            let plane = self.part().sketches[i].sketch.plane.clone();
            let [x, y, z] = self.box_size();
            let o = &self.sketch_opts;
            overlay::draw_sketch_frame(
                &painter,
                &self.camera,
                rect,
                &plane,
                x.max(y).max(z) * 0.75,
                (o.show_grid, o.show_axes, o.show_triad),
            );
        }
        for (i, ps) in self.part().sketches.iter().enumerate() {
            let profile_sketch = self.extrude.as_ref().is_some_and(|d| d.sketch == i)
                || self.show_dims.as_ref().is_some_and(|s| s.sketch == Some(i));
            // Sketches below the End of Part are rolled back: not drawn.
            let shown = ps.visible && !self.part().sketch_rolled_back(i);
            // Project Geometry: earlier sketches are pickable, hidden ones too.
            let source = self.project_geometry
                && self
                    .editing
                    .is_some_and(|t| self.project_sources(t).contains(&i));
            if !shown && self.editing != Some(i) && !profile_sketch && !source {
                continue;
            }
            let active = self.editing == Some(i);
            let style = overlay::SketchStyle {
                dim_display: self.dim_display,
                fully_color: Color32::from_rgb(20, 120, 40),
                fully: if active {
                    self.dof
                        .as_ref()
                        .map(|d| d.fully_constrained.clone())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                },
                dof: if active && self.show_dof {
                    self.dof.clone()
                } else {
                    None
                },
                hovered_constraint: if active {
                    label_hit.or(self.list_hover)
                } else {
                    None
                },
                selected_constraints: if active {
                    self.tools.selected_constraints.clone()
                } else {
                    Vec::new()
                },
                color: if active {
                    self.sketch_color()
                } else if source && !shown {
                    Self::project_source_color()
                } else {
                    Color32::from_gray(110)
                },
                reference: if active {
                    overlay::REFERENCE
                } else {
                    Color32::from_gray(140)
                },
                show_labels: (active || (profile_sketch && self.editing.is_none()))
                    && self.show_labels,
                highlighted: if active {
                    self.dragging.or(hovered)
                } else {
                    None
                },
                selected: if active {
                    // Hovering a pattern element lights up the whole pattern.
                    let pattern: Vec<EntityId> = hovered_entity
                        .and_then(|e| ps.sketch.pattern_containing(e))
                        .and_then(|(id, _)| ps.sketch.pattern(id))
                        .map(|p| p.all_entities().collect())
                        .unwrap_or_default();
                    self.tools
                        .selected
                        .iter()
                        .copied()
                        .chain(hovered_entity)
                        .chain(pattern)
                        .collect()
                } else {
                    Vec::new()
                },
            };
            if active {
                let regions = self.sketch_regions(&ps.sketch);
                overlay::draw_regions(
                    &painter,
                    &self.camera,
                    rect,
                    &ps.sketch,
                    &regions,
                    overlay::REGION_FILL,
                );
            }
            if profile_sketch && let Some(d) = &self.extrude {
                let regions = self.sketch_regions(&ps.sketch);
                for r in &regions {
                    let chosen = d
                        .def
                        .profiles()
                        .iter()
                        .any(|p| p.resolve(&regions).is_some_and(|x| x == r));
                    let fill = if chosen {
                        overlay::PROFILE_CHOSEN
                    } else {
                        overlay::PROFILE_FILL
                    };
                    overlay::draw_regions(
                        &painter,
                        &self.camera,
                        rect,
                        &ps.sketch,
                        std::slice::from_ref(r),
                        fill,
                    );
                }
            }
            overlay::draw_sketch(&painter, &self.camera, rect, ps, &style);
        }
        self.draw_primitive_preview(&painter, rect, ui.input(|i| i.pointer.hover_pos()));
        // Revolve: the chosen axis, extended past the profile.
        if let Some(d) = &self.extrude
            && d.axis_set
            && let DraftDef::Revolve(r) = &d.def
            && let Some(ps) = self.part().sketches.get(d.sketch)
            && let Ok(ax) =
                crate::part::revolve::resolve_axis_in(&r.axis, &ps.sketch, &self.part().work)
        {
            let dir = ax.dir.normalize();
            let reach = self.camera.distance * 0.6;
            let ends = [ax.origin - dir * reach, ax.origin + dir * reach].map(|p| {
                self.camera
                    .project(&ps.sketch.plane.point_at(p.x, p.y), rect)
            });
            if let [Some(a), Some(b)] = ends {
                let stroke = egui::Stroke::new(2.5, Color32::from_rgb(40, 160, 60));
                painter.extend(egui::Shape::dashed_line(&[a, b], stroke, 12.0, 6.0));
            }
        }
        // Manipulators: while a feature's handle is dragged, show it at the
        // pending value (the feature changes on release).
        if let Some((owner, hs)) = self.handles() {
            let hs = match (&self.manip_drag, owner) {
                (Some((ManipOwner::Feature(k), h, v)), ManipOwner::Feature(_)) if v.is_finite() => {
                    let part = self.part();
                    part.features
                        .get(*k)
                        .cloned()
                        .and_then(|mut f| {
                            crate::part::params::set_feature_field_value(&mut f, h.field, *v);
                            let sk = &part
                                .sketches
                                .iter()
                                .find(|s| Some(s.id.as_str()) == f.sketch())?
                                .sketch;
                            Some(match &f {
                                crate::part::Feature::Extrude(e) => manip::extrude_handles(e, sk),
                                crate::part::Feature::Revolve(r) => {
                                    manip::revolve_handles(r, sk, &part.work)
                                }
                                _ => Vec::new(),
                            })
                        })
                        .unwrap_or(hs)
                }
                _ => hs,
            };
            let (hot, label) = match &self.manip_drag {
                Some((_, h, v)) => {
                    let i = hs.iter().position(|x| x.field == h.field);
                    let v = if v.is_finite() {
                        *v
                    } else {
                        i.and_then(|i| self.handle_value(&hs[i])).unwrap_or(0.0)
                    };
                    (i, Some(manip::text(h, v)))
                }
                None => (
                    hot_handle,
                    hot_handle
                        .and_then(|i| self.handle_value(&hs[i]).map(|v| manip::text(&hs[i], v))),
                ),
            };
            manip::draw(&painter, &self.camera, rect, &hs, hot, label);
        }
        // Extrude preview: the edges of the solid-to-be.
        if self.extrude.is_some()
            && let Ok(edges) = self.extrude_preview()
        {
            let stroke = egui::Stroke::new(2.0, Color32::from_rgb(230, 120, 20));
            for e in edges {
                let pts: Vec<egui::Pos2> = e
                    .iter()
                    .filter_map(|p| self.camera.project(p, rect))
                    .collect();
                painter.add(egui::Shape::line(pts, stroke));
            }
        }
        // Box selection: window (left → right, solid blue) selects what is
        // fully inside, crossing (right → left, dashed green) also what it
        // crosses (Inventor, SolidWorks, Fusion).
        if let (Some(a), Some(b)) = (self.box_select, pointer) {
            let r = egui::Rect::from_two_pos(a, b);
            let window = self.select.box_mode.is_window(a.x, b.x);
            let (fill, col) = if window {
                (
                    Color32::from_rgba_unmultiplied(60, 120, 230, 30),
                    Color32::from_rgb(40, 100, 220),
                )
            } else {
                (
                    Color32::from_rgba_unmultiplied(60, 200, 90, 30),
                    Color32::from_rgb(30, 150, 60),
                )
            };
            painter.rect_filled(r, 0.0, fill);
            let c = [
                r.left_top(),
                r.right_top(),
                r.right_bottom(),
                r.left_bottom(),
                r.left_top(),
            ];
            if window {
                painter.add(egui::Shape::line(c.to_vec(), egui::Stroke::new(1.2, col)));
            } else {
                painter.extend(egui::Shape::dashed_line(
                    &c,
                    egui::Stroke::new(1.2, col),
                    6.0,
                    4.0,
                ));
            }
        }
        // Interpolation-spline end handles and curvature combs.
        if let Some(i) = in_sketch {
            let sk = &self.part().sketches[i].sketch;
            let to_s = |q: Point2| self.camera.project(&sk.plane.point_at(q.x, q.y), rect);
            for g in self.spline_grips(i) {
                let (Some(a), Some(b), Some(c)) =
                    (to_s(g.base - (g.grip - g.base)), to_s(g.base), to_s(g.grip))
                else {
                    continue;
                };
                let col = if g.set {
                    Color32::from_rgb(230, 120, 20)
                } else {
                    Color32::from_rgb(140, 150, 170)
                };
                painter.line_segment([a, c], egui::Stroke::new(1.5, col));
                let _ = b;
                painter.rect_filled(
                    egui::Rect::from_center_size(c, egui::vec2(8.0, 8.0)),
                    1.0,
                    col,
                );
            }
            // Comb teeth about 40 px at the most.
            let mm_per_px = 2.0 * self.camera.distance * (self.camera.fov_y / 2.0).tan()
                / rect.height().max(1.0) as f64;
            for &e in &self.combs {
                if sk.entity(e).is_none() {
                    continue;
                }
                let raw = sk.curvature_comb(e, 80, 1.0);
                let longest = raw
                    .iter()
                    .map(|(a, b)| (b - a).norm())
                    .fold(1e-12, f64::max);
                let s = 40.0 * mm_per_px / longest;
                let col = Color32::from_rgb(200, 60, 160);
                let mut tips = Vec::new();
                for (a, b) in raw {
                    let tip = a + (b - a) * s;
                    if let (Some(pa), Some(pt)) = (to_s(a), to_s(tip)) {
                        painter.line_segment([pa, pt], egui::Stroke::new(1.0, col));
                        tips.push(pt);
                    }
                }
                painter.add(egui::Shape::line(tips, egui::Stroke::new(1.2, col)));
            }
        }
        // Dynamic Input fields next to the cursor (Tab moves between them).
        let fields = self.hud_fields();
        if let (Some(i), Some(pick), false) = (in_sketch, self.last_pick, fields.is_empty()) {
            let sk = &self.part().sketches[i].sketch;
            if let Some(at) = self
                .camera
                .project(&sk.plane.point_at(pick.pos.x, pick.pos.y), rect)
            {
                let live = self.hud_live(&pick);
                let font = egui::FontId::proportional(12.0);
                let mut y = at.y + 18.0;
                for (k, name) in fields.iter().enumerate() {
                    let focus = k == self.hud.focus;
                    let unit = if name.starts_with("Angle") {
                        "°"
                    } else {
                        " mm"
                    };
                    let (value, locked) = if focus && !self.hud.text.is_empty() {
                        (format!("{}|", self.hud.text), false)
                    } else if let Some(v) = self.hud.vals[k] {
                        (format!("{v:.2}{unit}"), true)
                    } else {
                        (format!("{:.2}{unit}", live[k]), false)
                    };
                    let text = format!("{name}: {value}");
                    let galley = painter.layout_no_wrap(text, font.clone(), Color32::BLACK);
                    let r = egui::Rect::from_min_size(
                        egui::pos2(at.x + 18.0, y),
                        galley.size() + egui::vec2(10.0, 4.0),
                    );
                    let fill = if locked {
                        Color32::from_rgb(255, 236, 200)
                    } else {
                        Color32::from_rgb(255, 255, 255)
                    };
                    painter.rect_filled(r, 2.0, fill);
                    let border = if focus {
                        egui::Stroke::new(1.5, Color32::from_rgb(30, 110, 220))
                    } else {
                        egui::Stroke::new(1.0, Color32::from_gray(150))
                    };
                    painter.rect_stroke(r, 2.0, border, egui::StrokeKind::Inside);
                    painter.galley(r.min + egui::vec2(5.0, 2.0), galley, Color32::BLACK);
                    y += r.height() + 3.0;
                }
            }
        }
        // Pattern counts as badges at the last element (SolidWorks' "Display
        // instance count").
        if let Some(i) = in_sketch {
            let sk = &self.part().sketches[i].sketch;
            for (_, p) in sk.patterns() {
                let Some(last) = p.instances.last() else {
                    continue;
                };
                let pts: Vec<_> = last
                    .entities
                    .iter()
                    .flat_map(|&e| overlay::polyline(sk, e))
                    .collect();
                if pts.is_empty() {
                    continue;
                }
                let (mut lo, mut hi) = (pts[0], pts[0]);
                for q in &pts {
                    lo = lo.inf(q);
                    hi = hi.sup(q);
                }
                let text = match p.kind {
                    crate::sketch::PatternKind::Rectangular { dir1, dir2 } => match dir2 {
                        Some(d) if d.count > 1 => format!("x{} x {}", dir1.count, d.count),
                        _ => format!("x{}", dir1.count),
                    },
                    _ => format!("x{}", p.instances.len() + 1),
                };
                if let Some(s) = self.camera.project(&sk.plane.point_at(hi.x, hi.y), rect) {
                    let galley = painter.layout_no_wrap(
                        text,
                        egui::FontId::proportional(11.0),
                        Color32::from_rgb(30, 60, 140),
                    );
                    let at = s + egui::vec2(4.0, -galley.size().y - 2.0);
                    painter.rect_filled(
                        egui::Rect::from_min_size(at, galley.size()).expand(2.0),
                        3.0,
                        Color32::from_rgb(225, 235, 255),
                    );
                    painter.galley(at, galley, Color32::from_rgb(30, 60, 140));
                }
            }
        }
        // Edit Pattern previews with the Select tool active, wherever the cursor is.
        if let Some(i) = in_sketch
            && self.tools.pattern_active()
        {
            let ps = &self.part().sketches[i];
            // The dialog draws its own preview; Edit Pattern's is drawn here.
            if self.tools.edit_pattern.is_some() {
                let none = Pick {
                    pos: crate::sketch::Point2::origin(),
                    snap: Snap::None,
                    infer: None,
                    align: Align::default(),
                };
                let preview = self.tools.preview(ps, &none);
                overlay::draw_preview(&painter, &self.camera, rect, &ps.sketch, &preview);
            }
            // Drag handles: spacing / angle and count.
            let hs = self.tools.pattern_handles(&ps.sketch);
            let hot =
                self.pattern_drag.as_ref().map(|(g, _)| *g).or_else(|| {
                    pointer.and_then(|p| self.pattern_handle_at(&hs, &ps.sketch, rect, p))
                });
            let to_screen = |q: crate::sketch::Point2| {
                self.camera
                    .project(&ps.sketch.plane.point_at(q.x, q.y), rect)
            };
            for h in &hs {
                let (Some(a), Some(b)) = (to_screen(h.from), to_screen(h.pos)) else {
                    continue;
                };
                let lit = hot == Some(h.grip);
                let col = if lit {
                    Color32::from_rgb(255, 150, 30)
                } else {
                    Color32::from_rgb(40, 120, 230)
                };
                painter.add(egui::Shape::dashed_line(
                    &[a, b],
                    egui::Stroke::new(1.5, col),
                    6.0,
                    4.0,
                ));
                match h.grip {
                    sketch_tools::PatGrip::Spacing(_) => {
                        let d = (b - a).normalized();
                        let n = egui::vec2(-d.y, d.x);
                        painter.add(egui::Shape::convex_polygon(
                            vec![b + d * 9.0, b - d * 2.0 + n * 5.5, b - d * 2.0 - n * 5.5],
                            col,
                            egui::Stroke::new(1.0, Color32::WHITE),
                        ));
                    }
                    sketch_tools::PatGrip::Angle => {
                        painter.circle(b, 6.0, col, egui::Stroke::new(1.5, Color32::WHITE));
                    }
                    sketch_tools::PatGrip::Count(_) => {
                        painter.circle(b, 7.0, col, egui::Stroke::new(1.5, Color32::WHITE));
                        let w = egui::Stroke::new(1.8, Color32::WHITE);
                        painter
                            .line_segment([b - egui::vec2(3.5, 0.0), b + egui::vec2(3.5, 0.0)], w);
                        painter
                            .line_segment([b - egui::vec2(0.0, 3.5), b + egui::vec2(0.0, 3.5)], w);
                    }
                }
                if lit {
                    let text = match &self.pattern_drag {
                        Some((_, t)) if !t.is_empty() => t.clone(),
                        _ => match h.grip {
                            sketch_tools::PatGrip::Spacing(_) => "Drag: spacing".into(),
                            sketch_tools::PatGrip::Angle => "Drag: angle".into(),
                            sketch_tools::PatGrip::Count(_) => "Drag: count".into(),
                        },
                    };
                    let at = b + egui::vec2(12.0, -14.0);
                    let galley = painter.layout_no_wrap(
                        text,
                        egui::FontId::proportional(13.0),
                        Color32::BLACK,
                    );
                    let r = egui::Rect::from_min_size(at, galley.size()).expand(3.0);
                    painter.rect_filled(r, 3.0, Color32::from_rgb(255, 250, 225));
                    painter.galley(at, galley, Color32::BLACK);
                }
            }
        }
        if let (Some(i), Some(pick)) = (in_sketch, cursor_pick)
            && !selecting
        {
            let ps = &self.part().sketches[i];
            let preview = self.tools.preview(ps, &pick);
            overlay::draw_preview(&painter, &self.camera, rect, &ps.sketch, &preview);
            // The dimension being placed, live (third click puts it here).
            if let Some((sk, id)) = self.tools.dimension_preview(ps, &pick) {
                overlay::draw_dimension(&painter, &self.camera, rect, &sk, id, overlay::HIGHLIGHT);
                if let Some(l) =
                    overlay::label_layout(&painter, &self.camera, rect, &sk, self.dim_display)
                        .into_iter()
                        .find(|l| l.constraint == id)
                {
                    painter.rect_filled(l.rect, 3.0, Color32::from_rgb(255, 215, 140));
                    painter.text(
                        l.rect.center(),
                        egui::Align2::CENTER_CENTER,
                        &l.text,
                        egui::FontId::proportional(12.0),
                        Color32::BLACK,
                    );
                }
            }
            if let Some((_, start, _)) = self.arc_drag {
                overlay::draw_preview(
                    &painter,
                    &self.camera,
                    rect,
                    &ps.sketch,
                    &[sketch_tools::PreviewLine {
                        pts: vec![start, pick.pos],
                        dashed: true,
                        warn: false,
                    }],
                );
            }
            overlay::draw_snap(&painter, &self.camera, rect, &ps.sketch, &pick);
        }
        if self.layout.visible(layout::Element::Triad) {
            viewcube::draw_triad(&painter, &self.camera, rect);
        }
        if self.layout.visible(layout::Element::ViewCube) {
            let cube_active = self.view_cube.dragging();
            viewcube::draw(&painter, &self.camera, rect, pointer, cube_active);
        }
    }

    /// The parameter name of dimension `c` of the shown sketch (named
    /// first if it has no name yet), for a link to it.
    fn dimension_name_for_link(&mut self, c: ConstraintId) -> Option<String> {
        let i = self.dim_sketch()?;
        let is_dim = self.part().sketches[i]
            .sketch
            .constraint(c)
            .is_some_and(crate::sketch::Constraint::is_dimension);
        if !is_dim {
            return None;
        }
        if self.part().sketches[i].sketch.dim_param(c).name.is_empty() {
            self.part_mut().sync_parameters();
        }
        let n = self.part().sketches[i].sketch.dim_param(c).name;
        (!n.is_empty()).then_some(n)
    }

    /// Screen position of the text of dimension `c` in the edited sketch.
    fn dimension_screen_pos(&self, c: ConstraintId) -> Option<egui::Pos2> {
        let sk = &self.part().sketches.get(self.dim_sketch()?)?.sketch;
        let at = overlay::dimension_label(&self.camera, self.viewport_rect, sk, c)?;
        self.camera
            .project(&sk.plane.point_at(at.x, at.y), self.viewport_rect)
    }

    /// Edit Dimension **in place** (Inventor's in-canvas edit box): over the
    /// dimension's text; takes a value or an equation (`d0/2`, `10 mm`);
    /// `±` opens the tolerance. Enter applies, Esc keeps the value.
    fn dimension_editor(&mut self, ctx: &egui::Context) {
        let Some(edit) = self.tools.dim_edit.clone() else {
            return;
        };
        let Some(i) = self.dim_sketch() else {
            self.tools.dim_edit = None;
            return;
        };
        let pos = self
            .dimension_screen_pos(edit.constraint)
            .unwrap_or(self.viewport_rect.center());
        let sk = &self.part().sketches[i].sketch;
        let name = sk.dim_param(edit.constraint).name;
        let mut tol = sk.dim_param(edit.constraint).tol;
        let angle = edit.is_angle;
        let mut apply = false;
        let mut cancel = false;
        let mut tol_changed = false;
        let mut text = edit.text.clone();
        let mut focused = edit.focused;
        let mut measure = false;
        egui::Area::new(egui::Id::new("dim_inplace"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos - egui::vec2(70.0, 14.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if !name.is_empty() {
                            ui.label(RichText::new(format!("{name} =")).weak());
                        }
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut text)
                                .desired_width(110.0)
                                .hint_text("value or equation"),
                        );
                        if !focused {
                            r.request_focus();
                            focused = true;
                        }
                        ui.label(if angle { "deg" } else { "mm" });
                        // The `>` menu: a parameter's name, or a measured value.
                        ui.menu_button(">", |ui| {
                            ui.menu_button("List Parameters", |ui| {
                                let rows = self.part().parameter_table();
                                if rows.iter().all(|p| p.name == name) {
                                    ui.label(RichText::new("No parameters yet").weak());
                                }
                                for p in rows.iter().filter(|p| p.name != name) {
                                    let label = format!(
                                        "{}   {}",
                                        p.name,
                                        crate::part::params::format_value(p.value, p.unit)
                                    );
                                    if ui.button(label).clicked() {
                                        text = p.name.clone();
                                        focused = false;
                                        ui.close();
                                    }
                                }
                            });
                            let recent = valuefield::recent(ui.ctx());
                            ui.menu_button("Recently Used", |ui| {
                                if recent.is_empty() {
                                    ui.label(RichText::new("Nothing yet").weak());
                                }
                                for t in recent {
                                    if ui.button(&t).clicked() {
                                        text = t;
                                        focused = false;
                                        ui.close();
                                    }
                                }
                            });
                            if ui
                                .button("Measure")
                                .on_hover_text(
                                    "Pick two points, a line, a circle, an edge or a face in the view and use the value",
                                )
                                .clicked()
                            {
                                measure = true;
                                ui.close();
                            }
                        })
                        .response
                        .on_hover_text("Parameters, Recently Used, Measure");
                        // Enter in the value field applies (not Enter in the
                        // tolerance fields below).
                        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                            apply = true;
                        }
                        apply |= ui
                            .small_button("✔")
                            .on_hover_text("Apply (Enter)")
                            .clicked();
                        cancel |= ui
                            .small_button("x")
                            .on_hover_text("Keep the measured value (Esc)")
                            .clicked();
                        if ui
                            .selectable_label(self.dim_tol_open, "±")
                            .on_hover_text("Tolerance")
                            .clicked()
                        {
                            self.dim_tol_open = !self.dim_tol_open;
                        }
                    });
                    if self.dim_tol_open {
                        ui.separator();
                        tol_changed = tolerance_editor(ui, &mut tol, angle);
                    }
                });
            });
        if let Some(e) = self.tools.dim_edit.as_mut() {
            e.text = text.clone();
            e.focused = focused;
        }
        if measure {
            self.start_measure_into(measure_pick::MeasureInto::DimEdit);
        }
        if tol_changed {
            self.checkpoint("Tolerance");
            let ps = &mut self.part_mut().sketches[i];
            let before = ps.sketch.clone();
            let ok = ps
                .sketch
                .set_dimension_tolerance(edit.constraint, tol)
                .is_ok()
                && matches!(ps.sketch.solve(), Ok(r) if r.status == SolveStatus::Solved);
            if !ok {
                ps.sketch = before;
                self.history.cancel();
                self.file_status =
                    Some(Err("Tolerance: the evaluated size cannot be reached".into()));
            }
            self.solve();
        }
        if apply {
            valuefield::remember(ctx, &text);
            self.apply_dimension_text(i, edit.constraint, &text);
        } else if (cancel || ctx.input(|i| i.key_pressed(Key::Escape)))
            && self.measure_pick.is_none()
        {
            self.tools.dim_edit = None;
        }
    }

    /// Applies a value or equation typed for a dimension, through the
    /// part's parameters; on failure nothing changes.
    fn apply_dimension_text(&mut self, i: usize, c: ConstraintId, text: &str) {
        self.part_mut().sync_parameters();
        let name = self.part().sketches[i].sketch.dim_param(c).name;
        self.checkpoint("Edit Dimension");
        let before = self.part().clone();
        let r = self.part_mut().set_parameter_expr(&name, text);
        let solved = r.is_ok()
            && matches!(
                self.part_mut().sketches[i].sketch.solve(),
                Ok(rep) if rep.status == SolveStatus::Solved
            );
        if solved {
            self.tools.dim_edit = None;
            self.file_status = None;
            if self.editing.is_none() {
                // Edited from a feature command's preview: the features
                // that already use the sketch follow.
                if let Err(e) = self.part_mut().regenerate() {
                    self.file_status = Some(Err(e.to_string()));
                }
                self.scene_dirty = true;
            }
        } else {
            *self.part_mut() = before;
            self.history.cancel();
            self.file_status = Some(Err(match r {
                Err(e) => format!("Edit Dimension: {e}"),
                Ok(()) => {
                    "Edit Dimension: that value cannot be reached with the other constraints".into()
                }
            }));
        }
        self.solve();
    }

    /// Edit Dimension box and the over-constrained (driven) prompt.
    fn dimension_windows(&mut self, ctx: &egui::Context) {
        if self.tools.dim_edit.is_some() {
            self.dimension_editor(ctx);
        }
        if self.tools.over_prompt.is_some() {
            let mut accept = false;
            let mut cancel = false;
            egui::Window::new("Over-constrained")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 140.0))
                .show(ctx, |ui| {
                    ui.label("Adding this dimension would over-constrain the sketch.");
                    ui.label("Create it as a driven (reference) dimension instead?");
                    ui.horizontal(|ui| {
                        accept = ui.button("Accept").clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                });
            if accept {
                self.tool_op(|t, ps| t.accept_driven(ps));
            } else if cancel {
                self.tools.over_prompt = None;
            }
        }
    }
}

impl eframe::App for CadCraftApp {
    /// While Dynamic Input fields are shown, Tab moves between them: take it
    /// before egui uses it to move its widget focus.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // Ctrl+Tab / Ctrl+Shift+Tab: document tabs (`doc_tabs`).
        raw_input.events.retain(|e| match e {
            egui::Event::Key {
                key: Key::Tab,
                pressed,
                modifiers,
                ..
            } if modifiers.command => {
                if *pressed {
                    self.tab_step += if modifiers.shift { -1 } else { 1 };
                }
                false
            }
            _ => true,
        });
        if self.hud_fields().is_empty() {
            return;
        }
        raw_input.events.retain(|e| match e {
            egui::Event::Key {
                key: Key::Tab,
                pressed,
                modifiers,
                ..
            } => {
                if *pressed {
                    self.hud.tab += if modifiers.shift { -1 } else { 1 };
                }
                false
            }
            _ => true,
        });
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.body_key_frame.replace((true, None));
        // Expression fields everywhere evaluate against the part's parameters.
        valuefield::set_frame_table(&ctx, self.part().parameter_table());
        self.layout_begin(&ctx);
        self.shortcuts(&ctx);
        self.files_ui(&ctx);
        self.confirm_delete_window(&ctx);

        egui::Panel::top("quick_access").show(ui, |ui| self.quick_access(ui));
        self.docked_panels(ui, frame);

        self.options_window(&ctx);
        self.parameters_window(&ctx);
        self.suppress_if_window(&ctx);
        self.material_windows(&ctx);
        self.measure_window(&ctx);
        self.measure_requests(&ctx);
        self.measure_pick_window(&ctx);
        self.show_dims_requests(&ctx);
        self.show_dims_window(&ctx);
        valuefield::draw_whip(&ctx);
        self.alias_window(&ctx);
        self.offset_window(&ctx);
        self.dimension_windows(&ctx);
        self.layout_end(&ctx);
        self.diag_frame(&ctx);
    }

    fn on_exit(&mut self) {
        let options = self.options_snapshot();
        self.layout.flush(&options);
    }
}

/// Display name of a repeatable command (from the command registry).
fn command_label(a: Action) -> &'static str {
    commands::by_action(a).map_or("command", |c| c.name)
}

/// Has dimension `c` of sketch `i` a placed (non-default) text position?
fn sk_has_label(app: &CadCraftApp, i: usize, c: ConstraintId) -> bool {
    app.part().sketches[i].sketch.has_dimension_label(c)
}

/// The tolerance part of the dimension editor; returns whether it changed.
fn tolerance_editor(ui: &mut egui::Ui, tol: &mut crate::sketch::Tolerance, angle: bool) -> bool {
    use crate::sketch::{EvalSize, TolKind};
    let before = *tol;
    let to = |v: Real| if angle { v.to_degrees() } else { v };
    let from = |v: Real| if angle { v.to_radians() } else { v };
    egui::Grid::new("tol_grid").num_columns(2).show(ui, |ui| {
        ui.label("Type");
        egui::ComboBox::from_id_salt("tol_kind")
            .selected_text(tol.kind.label())
            .show_ui(ui, |ui| {
                for k in TolKind::ALL {
                    ui.selectable_value(&mut tol.kind, k, k.label());
                }
            });
        ui.end_row();
        let field = |ui: &mut egui::Ui, label: &str, v: &mut Real| {
            ui.label(label);
            let mut d = to(*v);
            if ui.add(valuefield::ExprValue::new(&mut d)).changed() {
                *v = from(d);
            }
            ui.end_row();
        };
        match tol.kind {
            TolKind::Symmetric => field(ui, "±", &mut tol.upper),
            TolKind::Deviation | TolKind::Limits => {
                field(ui, "Upper", &mut tol.upper);
                field(ui, "Lower", &mut tol.lower);
            }
            TolKind::Min => field(ui, "Upper", &mut tol.upper),
            TolKind::Max => field(ui, "Lower", &mut tol.lower),
            TolKind::Default | TolKind::Basic => {}
        }
        ui.label("Evaluated size");
        egui::ComboBox::from_id_salt("tol_eval")
            .selected_text(tol.eval.label())
            .show_ui(ui, |ui| {
                for e in EvalSize::ALL {
                    ui.selectable_value(&mut tol.eval, e, e.label());
                }
            });
        ui.end_row();
    });
    *tol != before
}

#[cfg(test)]
mod pick_tests {
    use super::closest_on_segment;
    use crate::kernel::{Point3, Vector3};

    /// The eye ray crosses a long edge near one end: the occlusion test
    /// must use that 3D point, not the middle (perspective).
    #[test]
    fn closest_point_of_a_segment_to_a_ray() {
        let a = Point3::new(0.0, 0.0, 0.0);
        let b = Point3::new(100.0, 0.0, 0.0);
        let o = Point3::new(20.0, -50.0, 30.0);
        let d = Vector3::new(0.0, 50.0, -30.0).normalize();
        let q = closest_on_segment(&a, &b, &o, &d);
        assert!((q - Point3::new(20.0, 0.0, 0.0)).norm() < 1e-9, "{q:?}");
        // Past the end: clamped.
        let o2 = Point3::new(150.0, -50.0, 30.0);
        assert!((closest_on_segment(&a, &b, &o2, &d) - b).norm() < 1e-9);
    }
}
