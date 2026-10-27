//! Drawings (`docs/spec/4-drawings.md`): a drawing document holds sheets with
//! views of parts. GUI-free, like `assembly`, so the API can make and export
//! drawings headless.
//!
//! - [`DrawingDocument`]: the standard and the sheets.
//! - [`Sheet`]: size, orientation, border, title block and the views.
//! - [`DrawingView`]: what a view shows (a model and an orientation, or a
//!   parent and a direction), where it sits on the sheet, scale and style.
//! - [`hlr`]: the lines of a view (visible / hidden edges and silhouettes).
//!
//! Sheet coordinates are millimetres from the sheet's lower left corner.
//! View results (curves) are never stored: they are recomputed from the
//! model (spec §6.3 h).

pub mod annot;
pub mod bend_table;
pub mod dimtext;
pub mod fits;
pub mod hlr;
pub mod parts;
pub mod pdf;
pub mod render;
pub mod sheet;
pub mod style;

use serde::{Deserialize, Serialize};

use crate::document::DocRef;
use crate::kernel::Vector3;

pub use annot::{Annotation, AnnotationKind, DimKind, Dimension, PointRef, PointRole};
pub use sheet::{Orientation, SheetSize, TitleField};

/// A drawing document.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DrawingDocument {
    /// The drafting standard (projection type, defaults).
    #[serde(default)]
    pub standard: DrawingStandard,
    /// The sheets, in browser order (at least one).
    pub sheets: Vec<Sheet>,
    /// The sheet shown (index into `sheets`).
    #[serde(default)]
    pub active_sheet: usize,
    /// The next view id (ids are never reused).
    #[serde(default = "one")]
    pub next_view_id: u32,
    /// The next annotation id (never reused).
    #[serde(default = "one")]
    pub next_annotation_id: u32,
    /// The drawing's styles (Style and Standard Editor).
    #[serde(default)]
    pub styles: style::DrawingStyles,
}

fn one() -> u32 {
    1
}

impl Default for DrawingDocument {
    fn default() -> Self {
        Self::new(DrawingStandard::default())
    }
}

/// The drafting standard of a drawing (Style and Standard Editor ›
/// Standard): only what the views need so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawingStandard {
    /// The standard's family.
    pub kind: StandardKind,
    /// First angle (ISO) or third angle (ANSI) projection.
    pub projection: Projection,
}

impl Default for DrawingStandard {
    fn default() -> Self {
        Self::of(StandardKind::Iso)
    }
}

impl DrawingStandard {
    /// The defaults of a standard family.
    pub fn of(kind: StandardKind) -> Self {
        Self {
            kind,
            projection: match kind {
                StandardKind::Ansi => Projection::ThirdAngle,
                _ => Projection::FirstAngle,
            },
        }
    }
}

/// Standard families of the New Drawing templates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StandardKind {
    /// ISO (the default).
    Iso,
    /// ANSI (inch-based sheets, third angle).
    Ansi,
    /// DIN.
    Din,
}

impl StandardKind {
    /// Every family, for the UI.
    pub const ALL: [StandardKind; 3] = [StandardKind::Iso, StandardKind::Ansi, StandardKind::Din];

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            StandardKind::Iso => "ISO",
            StandardKind::Ansi => "ANSI",
            StandardKind::Din => "DIN",
        }
    }
}

/// Where projected views go relative to their parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Projection {
    /// ISO: the view of the right side is placed on the left.
    FirstAngle,
    /// ANSI: the view of the right side is placed on the right.
    ThirdAngle,
}

/// One sheet of a drawing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sheet {
    /// Name shown in the browser ("Sheet:1").
    pub name: String,
    /// Paper size.
    pub size: SheetSize,
    /// Landscape or portrait.
    pub orientation: Orientation,
    /// Revision text (Edit Sheet).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub revision: String,
    /// Not counted in "Sheet n of m".
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exclude_from_count: bool,
    /// Skipped when printing / exporting all sheets.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exclude_from_printing: bool,
    /// Draw the border (with zones).
    #[serde(default = "yes")]
    pub border: bool,
    /// Draw the title block (lower right corner).
    #[serde(default = "yes")]
    pub title_block: bool,
    /// The views.
    #[serde(default)]
    pub views: Vec<DrawingView>,
    /// Dimensions, centre marks, centrelines, texts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
}

fn yes() -> bool {
    true
}

impl Sheet {
    /// A sheet with a border and a title block.
    pub fn new(name: impl Into<String>, size: SheetSize, orientation: Orientation) -> Self {
        Self {
            name: name.into(),
            size,
            orientation,
            revision: String::new(),
            exclude_from_count: false,
            exclude_from_printing: false,
            border: true,
            title_block: true,
            views: Vec::new(),
            annotations: Vec::new(),
        }
    }

    /// Width and height in mm.
    pub fn dims(&self) -> (f64, f64) {
        self.size.dims(self.orientation)
    }

    /// The view with `id`.
    pub fn view(&self, id: u32) -> Option<&DrawingView> {
        self.views.iter().find(|v| v.id == id)
    }

    /// The view with `id`, mutable.
    pub fn view_mut(&mut self, id: u32) -> Option<&mut DrawingView> {
        self.views.iter_mut().find(|v| v.id == id)
    }
}

/// Orientations of a base view (model Z up, front looks along +Y): the
/// standard ones and any other set with the ViewCube or Custom View.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ViewOrientation {
    /// From −Y.
    Front,
    /// From +Y.
    Back,
    /// From +Z.
    Top,
    /// From −Z.
    Bottom,
    /// From −X.
    Left,
    /// From +X.
    Right,
    /// Isometric from front, right, top.
    IsoTopRight,
    /// Isometric from front, left, top.
    IsoTopLeft,
    /// Isometric from front, right, bottom.
    IsoBottomRight,
    /// Isometric from front, left, bottom.
    IsoBottomLeft,
    /// Any other direction (format revision 27): towards the viewer and up
    /// on the sheet, unit vectors in model coordinates.
    Custom {
        /// Towards the viewer.
        toward: [f64; 3],
        /// Up on the sheet.
        up: [f64; 3],
    },
}

impl ViewOrientation {
    /// Every orientation, in the dialog's order.
    pub const ALL: [ViewOrientation; 10] = [
        ViewOrientation::Front,
        ViewOrientation::Top,
        ViewOrientation::Right,
        ViewOrientation::Back,
        ViewOrientation::Bottom,
        ViewOrientation::Left,
        ViewOrientation::IsoTopRight,
        ViewOrientation::IsoTopLeft,
        ViewOrientation::IsoBottomRight,
        ViewOrientation::IsoBottomLeft,
    ];

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            ViewOrientation::Front => "Front",
            ViewOrientation::Back => "Back",
            ViewOrientation::Top => "Top",
            ViewOrientation::Bottom => "Bottom",
            ViewOrientation::Left => "Left",
            ViewOrientation::Right => "Right",
            ViewOrientation::IsoTopRight => "Iso Top Right",
            ViewOrientation::IsoTopLeft => "Iso Top Left",
            ViewOrientation::IsoBottomRight => "Iso Bottom Right",
            ViewOrientation::IsoBottomLeft => "Iso Bottom Left",
            ViewOrientation::Custom { .. } => "Custom",
        }
    }

    /// The orientation showing `frame`: a standard one when it is that
    /// (within 1e-6), else `Custom`.
    pub fn from_frame(frame: &ViewFrame) -> Self {
        let near = |a: Vector3, b: Vector3| (a - b).norm() < 1e-6;
        Self::ALL
            .into_iter()
            .find(|o| {
                let f = o.frame();
                near(f.toward, frame.toward) && near(f.up, frame.up)
            })
            .unwrap_or(ViewOrientation::Custom {
                toward: frame.toward.into(),
                up: frame.up.into(),
            })
    }

    /// The view's frame.
    pub fn frame(self) -> ViewFrame {
        let (x, y, z) = (Vector3::x(), Vector3::y(), Vector3::z());
        // (towards the viewer, up on the sheet)
        let (toward, up) = match self {
            ViewOrientation::Front => (-y, z),
            ViewOrientation::Back => (y, z),
            ViewOrientation::Top => (z, y),
            ViewOrientation::Bottom => (-z, -y),
            ViewOrientation::Left => (-x, z),
            ViewOrientation::Right => (x, z),
            ViewOrientation::IsoTopRight => (x - y + z, z),
            ViewOrientation::IsoTopLeft => (-x - y + z, z),
            ViewOrientation::IsoBottomRight => (x - y - z, -z),
            ViewOrientation::IsoBottomLeft => (-x - y - z, -z),
            ViewOrientation::Custom { toward, up } => (toward.into(), up.into()),
        };
        ViewFrame::new(toward, up)
    }
}

/// Where a projected view lies from its parent on the sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectDir {
    /// Above.
    Up,
    /// Below.
    Down,
    /// Left.
    Left,
    /// Right.
    Right,
    /// Diagonal: an isometric view (signs of sheet x and y).
    Iso(i8, i8),
}

impl ProjectDir {
    /// The direction of a sheet offset `(dx, dy)` from the parent: within
    /// ±30° of an axis it is orthographic, otherwise isometric.
    pub fn from_offset(dx: f64, dy: f64) -> Option<Self> {
        if dx.hypot(dy) < 1e-9 {
            return None;
        }
        let a = dy.atan2(dx).to_degrees();
        let near = |c: f64| {
            let d = (a - c + 540.0).rem_euclid(360.0) - 180.0;
            d.abs() <= 30.0
        };
        Some(if near(0.0) {
            ProjectDir::Right
        } else if near(90.0) {
            ProjectDir::Up
        } else if near(180.0) {
            ProjectDir::Left
        } else if near(-90.0) {
            ProjectDir::Down
        } else {
            ProjectDir::Iso(dx.signum() as i8, dy.signum() as i8)
        })
    }

    /// Orthographic (aligned with the parent).
    pub fn is_ortho(self) -> bool {
        !matches!(self, ProjectDir::Iso(..))
    }
}

/// A view's projection frame: unit vectors in model space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewFrame {
    /// Sheet right.
    pub right: Vector3,
    /// Sheet up.
    pub up: Vector3,
    /// Towards the viewer (the eye looks along `-toward`).
    pub toward: Vector3,
}

impl ViewFrame {
    /// The frame looking along `-toward`, with `up` projected square to it.
    pub fn new(toward: Vector3, up: Vector3) -> Self {
        let n = toward.normalize();
        let mut u = up - n * up.dot(&n);
        if u.norm() < 1e-9 {
            u = Vector3::z() - n * n.z;
            if u.norm() < 1e-9 {
                u = Vector3::y();
            }
        }
        let u = u.normalize();
        Self {
            right: u.cross(&n),
            up: u,
            toward: n,
        }
    }

    /// The frame of a view projected from this one in `dir`.
    pub fn projected(&self, dir: ProjectDir, projection: Projection) -> ViewFrame {
        // Third angle: the view on the right shows the right side.
        let s = match projection {
            Projection::ThirdAngle => 1.0,
            Projection::FirstAngle => -1.0,
        };
        let (r, u, n) = (self.right, self.up, self.toward);
        match dir {
            ProjectDir::Right => ViewFrame::new(r * s, u),
            ProjectDir::Left => ViewFrame::new(-r * s, u),
            // Third angle: the parent's near side faces the parent; first
            // angle: away from it.
            ProjectDir::Up => ViewFrame::new(u * s, -n * s),
            ProjectDir::Down => ViewFrame::new(-u * s, n * s),
            ProjectDir::Iso(dx, dy) => {
                let toward = n + r * (s * f64::from(dx)) + u * (s * f64::from(dy));
                ViewFrame::new(toward, u)
            }
        }
    }

    /// A model point on the view plane (unscaled mm).
    pub fn project(&self, p: &crate::kernel::Point3) -> [f64; 2] {
        let v = p.coords;
        [v.dot(&self.right), v.dot(&self.up)]
    }

    /// The depth of a model point (larger = nearer the viewer).
    pub fn depth(&self, p: &crate::kernel::Point3) -> f64 {
        p.coords.dot(&self.toward)
    }
}

/// What a view shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ViewKind {
    /// Base View: a model in a standard orientation.
    Base {
        /// The part (same project or a linked file).
        model: DocRef,
        /// Its orientation.
        orientation: ViewOrientation,
    },
    /// Projected View from another view.
    Projected {
        /// The parent view's id.
        parent: u32,
        /// Where it lies from the parent.
        dir: ProjectDir,
    },
    /// Section View: the parent cut along a line (full depth).
    Section {
        /// The parent view's id.
        parent: u32,
        /// The section line's ends in the parent's view plane (model mm,
        /// unscaled, as `ViewFrame::project`).
        a: [f64; 2],
        /// Second end.
        b: [f64; 2],
        /// The side of the line the view lies on (+1: left of a → b).
        side: i8,
    },
    /// Auxiliary View: square to a line (an edge) of the parent.
    Auxiliary {
        /// The parent view's id.
        parent: u32,
        /// The edge's ends in the parent's view plane.
        a: [f64; 2],
        /// Second end.
        b: [f64; 2],
        /// The side of the line the view lies on.
        side: i8,
    },
    /// Detail View: an enlarged circle of the parent.
    Detail {
        /// The parent view's id.
        parent: u32,
        /// Fence centre in the parent's view plane.
        center: [f64; 2],
        /// Fence radius (model mm).
        radius: f64,
    },
}

/// What a view's lines are computed from.
#[derive(Clone, Debug, PartialEq)]
pub enum LinesJob {
    /// The model seen in a frame (aligned / cut for section and auxiliary
    /// views).
    Model {
        /// The part.
        model: DocRef,
        /// The view's frame.
        frame: ViewFrame,
        /// Alignment with the parent and the cut.
        aligned: Option<hlr::Aligned>,
        /// The flat pattern instead of the folded part.
        flat: bool,
    },
    /// The parent's lines inside a circle.
    Detail {
        /// The parent view.
        parent: u32,
        /// Fence centre (view plane).
        center: [f64; 2],
        /// Fence radius.
        radius: f64,
    },
}

/// Display style (Drawing View › Style).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewStyle {
    /// Visible and hidden lines.
    HiddenShown,
    /// Visible lines only (the default).
    #[default]
    HiddenRemoved,
    /// Shaded (with visible edges).
    Shaded,
}

impl ViewStyle {
    /// Every style, for the dialog.
    pub const ALL: [ViewStyle; 3] = [
        ViewStyle::HiddenShown,
        ViewStyle::HiddenRemoved,
        ViewStyle::Shaded,
    ];

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            ViewStyle::HiddenShown => "Hidden Line",
            ViewStyle::HiddenRemoved => "Hidden Line Removed",
            ViewStyle::Shaded => "Shaded",
        }
    }
}

/// Drawing View › Display Options.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayOptions {
    /// Thread features (off by default).
    pub thread_feature: bool,
    /// Tangent edges.
    pub tangent_edges: bool,
    /// Tangent edges shortened.
    pub foreshortened: bool,
    /// Edges hidden by interference (assemblies).
    pub interference_edges: bool,
    /// Keep a projected view aligned with its parent.
    pub align_to_base: bool,
    /// Show section lines / detail fences of child views.
    pub definition_in_base: bool,
    /// Take the orientation from the parent.
    pub orientation_from_base: bool,
    /// Hatching of section views.
    pub hatching: bool,
    /// The view stays centred on its geometry (else Fixed).
    pub center_justified: bool,
    /// Flat pattern views: bend centre lines.
    pub bend_centerlines: bool,
    /// Flat pattern views: the lines where each bend starts and ends.
    pub bend_extents: bool,
}

impl Default for DisplayOptions {
    fn default() -> Self {
        Self {
            thread_feature: false,
            tangent_edges: true,
            foreshortened: false,
            interference_edges: false,
            align_to_base: true,
            definition_in_base: true,
            orientation_from_base: true,
            hatching: true,
            center_justified: true,
            bend_centerlines: true,
            bend_extents: true,
        }
    }
}

/// Drawing View › Recovery Options.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecoveryOptions {
    /// Include surface bodies.
    pub surface_bodies: bool,
    /// Include mesh bodies.
    pub mesh_bodies: bool,
    /// Retrieve all model dimensions planar to the view.
    pub model_dimensions: bool,
    /// Work features as reference lines.
    pub work_features: bool,
}

/// One view on a sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrawingView {
    /// Identity inside the drawing (never reused).
    pub id: u32,
    /// View identifier shown in the label ("VIEW1", "A").
    pub identifier: String,
    /// What it shows.
    #[serde(flatten)]
    pub kind: ViewKind,
    /// Centre on the sheet (mm).
    pub position: [f64; 2],
    /// Scale (sheet mm per model mm).
    pub scale: f64,
    /// Follow the parent's scale (projected views).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub scale_from_base: bool,
    /// Display style.
    #[serde(default)]
    pub style: ViewStyle,
    /// Follow the parent's style (projected views).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub style_from_base: bool,
    /// The label below the view is shown.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub label_visible: bool,
    /// Raster View (pixel based, fast).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub raster: bool,
    /// Display Options.
    #[serde(default)]
    pub display: DisplayOptions,
    /// Recovery Options.
    #[serde(default)]
    pub recovery: RecoveryOptions,
    /// Sheet Metal View of a sheet metal part (base views; the others
    /// follow their base).
    #[serde(default, skip_serializing_if = "SheetView::is_folded")]
    pub sheet_metal: SheetView,
}

/// What a view of a sheet metal part shows (Drawing View › Sheet Metal
/// View).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SheetView {
    /// The folded model.
    #[default]
    Folded,
    /// The flat pattern, in its own frame (top face towards +Z, flat x along
    /// +X), with its bend lines.
    Flat,
}

impl SheetView {
    fn is_folded(&self) -> bool {
        *self == SheetView::Folded
    }
}

impl DrawingView {
    /// The parent's id (every view but a base view).
    pub fn parent(&self) -> Option<u32> {
        match self.kind {
            ViewKind::Projected { parent, .. }
            | ViewKind::Section { parent, .. }
            | ViewKind::Auxiliary { parent, .. }
            | ViewKind::Detail { parent, .. } => Some(parent),
            ViewKind::Base { .. } => None,
        }
    }

    /// The label under the view (identifier and scale), when shown.
    pub fn label_lines(&self, scale: f64) -> Vec<String> {
        let scale = format!("SCALE {}", scale_text(scale).replace(' ', ""));
        match self.kind {
            ViewKind::Section { .. } => {
                vec![format!("SECTION {0}-{0}", self.identifier), scale]
            }
            ViewKind::Detail { .. } => vec![format!("DETAIL {}", self.identifier), scale],
            ViewKind::Auxiliary { .. } => vec![format!("VIEW {}", self.identifier), scale],
            _ => vec![self.identifier.clone(), scale],
        }
    }
}

/// Inventor's preset view scales.
pub const SCALES: [f64; 15] = [
    0.01, 0.02, 0.05, 0.1, 0.2, 0.25, 0.5, 1.0, 1.5, 2.0, 2.5, 4.0, 5.0, 10.0, 20.0,
];

/// A scale as drawings write it: `1 : 2`, `2 : 1`.
pub fn scale_text(s: f64) -> String {
    let fmt = |v: f64| {
        let t = format!("{v:.3}");
        t.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    if s >= 1.0 {
        format!("{} : 1", fmt(s))
    } else {
        format!("1 : {}", fmt(1.0 / s))
    }
}

/// The largest preset scale at which a model extent `(w, h)` fills at most
/// `share` of a sheet `(sw, sh)`.
pub fn fit_scale(extent: (f64, f64), sheet: (f64, f64), share: f64) -> f64 {
    let (w, h) = (extent.0.max(1e-6), extent.1.max(1e-6));
    let want = (sheet.0 * share / w).min(sheet.1 * share / h);
    SCALES
        .iter()
        .rev()
        .copied()
        .find(|&s| s <= want)
        .unwrap_or(SCALES[0])
}

impl DrawingDocument {
    /// A drawing with one A3 landscape sheet of `standard`.
    pub fn new(standard: DrawingStandard) -> Self {
        let size = match standard.kind {
            StandardKind::Ansi => SheetSize::AnsiB,
            _ => SheetSize::A3,
        };
        Self {
            standard,
            sheets: vec![Sheet::new("Sheet:1", size, Orientation::Landscape)],
            active_sheet: 0,
            next_view_id: 1,
            next_annotation_id: 1,
            styles: style::DrawingStyles::of(standard.kind),
        }
    }

    /// The active sheet.
    pub fn sheet(&self) -> Option<&Sheet> {
        self.sheets.get(self.active_sheet)
    }

    /// The active sheet, mutable.
    pub fn sheet_mut(&mut self) -> Option<&mut Sheet> {
        self.sheets.get_mut(self.active_sheet)
    }

    /// Adds a sheet like the active one (New Sheet) and activates it.
    pub fn new_sheet(&mut self) -> usize {
        let (size, orientation) = self
            .sheet()
            .map_or((SheetSize::A3, Orientation::Landscape), |s| {
                (s.size, s.orientation)
            });
        let n = (1..)
            .find(|k| !self.sheets.iter().any(|s| s.name == format!("Sheet:{k}")))
            .unwrap_or(1);
        self.sheets
            .push(Sheet::new(format!("Sheet:{n}"), size, orientation));
        self.active_sheet = self.sheets.len() - 1;
        self.active_sheet
    }

    /// Deletes sheet `i` (never the last one). Returns whether it did.
    pub fn delete_sheet(&mut self, i: usize) -> bool {
        if self.sheets.len() < 2 || i >= self.sheets.len() {
            return false;
        }
        self.sheets.remove(i);
        if self.active_sheet >= self.sheets.len() || self.active_sheet > i {
            self.active_sheet = self.active_sheet.saturating_sub(1);
        }
        true
    }

    /// "n of m" for sheet `i`: excluded sheets are not counted.
    pub fn sheet_number(&self, i: usize) -> (usize, usize) {
        let counted: Vec<usize> = (0..self.sheets.len())
            .filter(|&k| !self.sheets[k].exclude_from_count)
            .collect();
        let n = counted.iter().position(|&k| k == i).map_or(0, |p| p + 1);
        (n, counted.len())
    }

    fn take_id(&mut self) -> u32 {
        let id = self.next_view_id.max(1);
        self.next_view_id = id + 1;
        id
    }

    fn next_identifier(&self) -> String {
        let used = |s: &str| {
            self.sheets
                .iter()
                .flat_map(|sh| &sh.views)
                .any(|v| v.identifier == s)
        };
        (1..)
            .map(|k| format!("VIEW{k}"))
            .find(|s| !used(s))
            .unwrap_or_default()
    }

    /// Places a base view of `model` on the active sheet; returns its id.
    pub fn add_base_view(
        &mut self,
        model: DocRef,
        orientation: ViewOrientation,
        position: [f64; 2],
        scale: f64,
        style: ViewStyle,
    ) -> Option<u32> {
        let identifier = self.next_identifier();
        let id = self.take_id();
        self.sheet_mut()?.views.push(DrawingView {
            id,
            identifier,
            kind: ViewKind::Base { model, orientation },
            position,
            scale,
            scale_from_base: false,
            style,
            style_from_base: false,
            label_visible: false,
            raster: false,
            display: DisplayOptions::default(),
            recovery: RecoveryOptions::default(),
            sheet_metal: SheetView::Folded,
        });
        Some(id)
    }

    /// Places a view projected from `parent` at sheet point `at`; an
    /// orthographic one is aligned with its parent. Returns its id.
    pub fn add_projected_view(&mut self, parent: u32, at: [f64; 2]) -> Option<u32> {
        let p = self.sheet()?.view(parent)?.clone();
        let dir = ProjectDir::from_offset(at[0] - p.position[0], at[1] - p.position[1])?;
        let identifier = self.next_identifier();
        let id = self.take_id();
        let mut v = DrawingView {
            id,
            identifier,
            kind: ViewKind::Projected { parent, dir },
            position: at,
            scale: p.scale,
            scale_from_base: true,
            style: p.style,
            style_from_base: true,
            label_visible: !dir.is_ortho(),
            raster: p.raster,
            display: p.display,
            recovery: RecoveryOptions::default(),
            sheet_metal: SheetView::Folded,
        };
        align(&mut v.position, p.position, dir);
        self.sheet_mut()?.views.push(v);
        Some(id)
    }

    /// The model of view `id` (through its parents).
    pub fn model_of(&self, sheet: &Sheet, id: u32) -> Option<DocRef> {
        let mut v = sheet.view(id)?;
        for _ in 0..64 {
            match &v.kind {
                ViewKind::Base { model, .. } => return Some(*model),
                _ => v = sheet.view(v.parent()?)?,
            }
        }
        None
    }

    /// Whether view `id` shows a flat pattern (through its parents).
    pub fn flat_of(&self, sheet: &Sheet, id: u32) -> bool {
        let mut v = sheet.view(id);
        for _ in 0..64 {
            match v {
                Some(x) if matches!(x.kind, ViewKind::Base { .. }) => {
                    return x.sheet_metal == SheetView::Flat;
                }
                Some(x) => v = x.parent().and_then(|p| sheet.view(p)),
                None => return false,
            }
        }
        false
    }

    /// The frame of a view square to line `a`–`b` of a parent seen in `fp`,
    /// lying on `side` of it: the line keeps its direction on the sheet (the
    /// views stay aligned), the viewer stands on the view's side in third
    /// angle and on the other side in first angle.
    pub fn line_frame(
        &self,
        fp: &ViewFrame,
        a: [f64; 2],
        b: [f64; 2],
        side: i8,
    ) -> Option<(ViewFrame, [f64; 2])> {
        let d = [b[0] - a[0], b[1] - a[1]];
        let len = d[0].hypot(d[1]);
        if len < 1e-9 {
            return None;
        }
        let l = [d[0] / len, d[1] / len];
        let k = f64::from(side.signum());
        let s2 = [-l[1] * k, l[0] * k];
        let line3 = fp.right * l[0] + fp.up * l[1];
        let side3 = fp.right * s2[0] + fp.up * s2[1];
        let toward = match self.standard.projection {
            Projection::ThirdAngle => side3,
            Projection::FirstAngle => -side3,
        };
        let mut n = fp.toward;
        if line3.cross(&n).dot(&toward) < 0.0 {
            n = -n;
        }
        let up = line3 * l[1] + n * l[0];
        Some((ViewFrame::new(toward, up), l))
    }

    /// What view `id`'s lines are computed from.
    pub fn lines_job(&self, sheet: &Sheet, id: u32) -> Option<LinesJob> {
        let v = sheet.view(id)?;
        let model = self.model_of(sheet, id)?;
        let flat = self.flat_of(sheet, id);
        Some(match v.kind {
            ViewKind::Detail {
                parent,
                center,
                radius,
            } => LinesJob::Detail {
                parent,
                center,
                radius,
            },
            ViewKind::Section { parent, a, b, .. } | ViewKind::Auxiliary { parent, a, b, .. } => {
                let fp = self.frame_of(sheet, parent)?;
                let frame = self.frame_of(sheet, id)?;
                let d = [b[0] - a[0], b[1] - a[1]];
                let len = d[0].hypot(d[1]).max(1e-12);
                let cut = matches!(v.kind, ViewKind::Section { .. })
                    .then(|| crate::kernel::Point3::from(fp.right * a[0] + fp.up * a[1]));
                LinesJob::Model {
                    model,
                    frame,
                    aligned: Some(hlr::Aligned {
                        parent: fp,
                        line: [d[0] / len, d[1] / len],
                        cut,
                    }),
                    flat,
                }
            }
            _ => LinesJob::Model {
                model,
                frame: self.frame_of(sheet, id)?,
                aligned: None,
                flat,
            },
        })
    }

    /// The next free letter for a section / detail / auxiliary view
    /// (I, O, Q, S, X, Z are skipped, as Inventor's standard does).
    pub fn next_letter(&self) -> String {
        let used: Vec<&str> = self
            .sheets
            .iter()
            .flat_map(|s| &s.views)
            .map(|v| v.identifier.as_str())
            .collect();
        let letters: Vec<String> = ('A'..='Z')
            .filter(|c| !"IOQSXZ".contains(*c))
            .map(String::from)
            .collect();
        for round in 0.. {
            for l in &letters {
                let name = if round == 0 {
                    l.clone()
                } else {
                    format!("{l}{round}")
                };
                if !used.contains(&name.as_str()) {
                    return name;
                }
            }
        }
        String::new()
    }

    /// A child view of `parent` (section, auxiliary, detail) at `at`.
    fn add_child(
        &mut self,
        parent: u32,
        kind: ViewKind,
        at: [f64; 2],
        scale: Option<f64>,
    ) -> Option<u32> {
        let p = self.sheet()?.view(parent)?.clone();
        let (pscale, pstyle) = {
            let s = self.sheet()?;
            self.effective(s, parent)?
        };
        let identifier = self.next_letter();
        let id = self.take_id();
        let v = DrawingView {
            id,
            identifier,
            kind,
            position: at,
            scale: scale.unwrap_or(pscale),
            scale_from_base: scale.is_none(),
            style: pstyle,
            style_from_base: true,
            label_visible: true,
            raster: p.raster,
            display: p.display,
            recovery: RecoveryOptions::default(),
            sheet_metal: SheetView::Folded,
        };
        self.sheet_mut()?.views.push(v);
        // Positions are kept to 0.1 mm (readable in Properties).
        let at = self
            .aligned_position(id, at)
            .map(|c| (c * 10.0).round() / 10.0);
        if let Some(v) = self.sheet_mut()?.view_mut(id) {
            v.position = at;
        }
        Some(id)
    }

    /// Section View of `parent` along `a`–`b` (its view plane), placed at
    /// `at` (sheet mm); the side follows `at`.
    pub fn add_section_view(
        &mut self,
        parent: u32,
        a: [f64; 2],
        b: [f64; 2],
        at: [f64; 2],
    ) -> Option<u32> {
        let side = self.side_of(parent, a, b, at)?;
        self.add_child(parent, ViewKind::Section { parent, a, b, side }, at, None)
    }

    /// Auxiliary View of `parent` square to edge `a`–`b`, placed at `at`.
    pub fn add_auxiliary_view(
        &mut self,
        parent: u32,
        a: [f64; 2],
        b: [f64; 2],
        at: [f64; 2],
    ) -> Option<u32> {
        let side = self.side_of(parent, a, b, at)?;
        self.add_child(parent, ViewKind::Auxiliary { parent, a, b, side }, at, None)
    }

    /// Detail View of `parent` inside the circle `center`, `radius` (view
    /// plane), at `scale`, placed at `at`.
    pub fn add_detail_view(
        &mut self,
        parent: u32,
        center: [f64; 2],
        radius: f64,
        at: [f64; 2],
        scale: f64,
    ) -> Option<u32> {
        if radius <= 1e-9 {
            return None;
        }
        self.add_child(
            parent,
            ViewKind::Detail {
                parent,
                center,
                radius,
            },
            at,
            Some(scale),
        )
    }

    /// Which side of line `a`–`b` (parent's view plane) the sheet point `at`
    /// lies on, seen from the parent's position.
    fn side_of(&self, parent: u32, a: [f64; 2], b: [f64; 2], at: [f64; 2]) -> Option<i8> {
        let p = self.sheet()?.view(parent)?;
        let d = [b[0] - a[0], b[1] - a[1]];
        if d[0].hypot(d[1]) < 1e-9 {
            return None;
        }
        let n = [-d[1], d[0]];
        let off = [at[0] - p.position[0], at[1] - p.position[1]];
        Some(if off[0] * n[0] + off[1] * n[1] >= 0.0 {
            1
        } else {
            -1
        })
    }

    /// Where view `id` may be when it wants `at`: aligned views slide on
    /// their axis from the parent.
    pub fn aligned_position(&self, id: u32, at: [f64; 2]) -> [f64; 2] {
        let Some(sheet) = self.sheet() else {
            return at;
        };
        let Some(v) = sheet.view(id) else {
            return at;
        };
        if !v.display.align_to_base {
            return at;
        }
        let Some(p) = v.parent().and_then(|pid| sheet.view(pid)) else {
            return at;
        };
        let mut to = at;
        match v.kind {
            ViewKind::Projected { dir, .. } => align(&mut to, p.position, dir),
            ViewKind::Section { a, b, .. } | ViewKind::Auxiliary { a, b, .. } => {
                let d = [b[0] - a[0], b[1] - a[1]];
                let len = d[0].hypot(d[1]);
                if len > 1e-9 {
                    // Slide square to the line, from the parent's position.
                    let n = [-d[1] / len, d[0] / len];
                    let off = [at[0] - p.position[0], at[1] - p.position[1]];
                    let t = off[0] * n[0] + off[1] * n[1];
                    to = [p.position[0] + n[0] * t, p.position[1] + n[1] * t];
                }
            }
            _ => {}
        }
        to
    }

    /// The projection frame of view `id` (through its parents).
    pub fn frame_of(&self, sheet: &Sheet, id: u32) -> Option<ViewFrame> {
        let v = sheet.view(id)?;
        if v.parent() == Some(id) {
            return None;
        }
        Some(match &v.kind {
            ViewKind::Base { orientation, .. } => orientation.frame(),
            ViewKind::Projected { parent, dir } => self
                .frame_of(sheet, *parent)?
                .projected(*dir, self.standard.projection),
            ViewKind::Section { parent, a, b, side }
            | ViewKind::Auxiliary { parent, a, b, side } => {
                let fp = self.frame_of(sheet, *parent)?;
                self.line_frame(&fp, *a, *b, *side)?.0
            }
            ViewKind::Detail { parent, .. } => self.frame_of(sheet, *parent)?,
        })
    }

    /// The scale and style a view uses (inherited when `*_from_base`).
    pub fn effective(&self, sheet: &Sheet, id: u32) -> Option<(f64, ViewStyle)> {
        let v = sheet.view(id)?;
        let parent = v
            .parent()
            .filter(|&p| p != id)
            .and_then(|p| self.effective(sheet, p));
        let scale = match parent {
            Some((s, _)) if v.scale_from_base => s,
            _ => v.scale,
        };
        let style = match parent {
            Some((_, st)) if v.style_from_base => st,
            _ => v.style,
        };
        Some((scale, style))
    }

    /// Moves view `id` of the active sheet to `at`; an aligned projected
    /// view slides only along its parent's axis, and an aligned child
    /// follows its parent. Returns whether something moved.
    pub fn move_view(&mut self, id: u32, at: [f64; 2]) -> bool {
        let to = self.aligned_position(id, at);
        let Some(sheet) = self.sheet_mut() else {
            return false;
        };
        let Some(v) = sheet.view(id).cloned() else {
            return false;
        };
        let delta = [to[0] - v.position[0], to[1] - v.position[1]];
        if delta[0].hypot(delta[1]) < 1e-9 {
            return false;
        }
        if let Some(m) = sheet.view_mut(id) {
            m.position = to;
        }
        // Aligned children keep their row / column (projected views) or
        // follow along their line (section, auxiliary views).
        let children: Vec<(u32, ViewKind)> = sheet
            .views
            .iter()
            .filter(|c| c.parent() == Some(id) && c.display.align_to_base)
            .map(|c| (c.id, c.kind.clone()))
            .collect();
        for (cid, kind) in children {
            if let Some(c) = sheet.view_mut(cid) {
                match kind {
                    ViewKind::Projected { dir, .. } => match dir {
                        ProjectDir::Left | ProjectDir::Right => c.position[1] = to[1],
                        ProjectDir::Up | ProjectDir::Down => c.position[0] = to[0],
                        ProjectDir::Iso(..) => {}
                    },
                    ViewKind::Section { a, b, .. } | ViewKind::Auxiliary { a, b, .. } => {
                        let d = [b[0] - a[0], b[1] - a[1]];
                        let len = d[0].hypot(d[1]);
                        if len > 1e-9 {
                            let l = [d[0] / len, d[1] / len];
                            let t = delta[0] * l[0] + delta[1] * l[1];
                            c.position[0] += l[0] * t;
                            c.position[1] += l[1] * t;
                        }
                    }
                    _ => {}
                }
            }
        }
        true
    }

    /// Deletes view `id` of the active sheet and every view projected from
    /// it (Inventor asks; dependents go with it). Returns the ids removed.
    pub fn delete_view(&mut self, id: u32) -> Vec<u32> {
        let Some(sheet) = self.sheet_mut().filter(|s| s.view(id).is_some()) else {
            return Vec::new();
        };
        let mut gone = vec![id];
        let mut k = 0;
        while k < gone.len() {
            let p = gone[k];
            for v in &sheet.views {
                if v.parent() == Some(p) && !gone.contains(&v.id) {
                    gone.push(v.id);
                }
            }
            k += 1;
        }
        sheet.views.retain(|v| !gone.contains(&v.id));
        // Their annotations go with them (as in Inventor).
        sheet
            .annotations
            .retain(|a| a.view().is_none_or(|v| !gone.contains(&v)));
        gone
    }

    /// Every model the drawing shows.
    pub fn references(&self) -> Vec<DocRef> {
        let mut out = Vec::new();
        for s in &self.sheets {
            for v in &s.views {
                if let ViewKind::Base { model, .. } = v.kind
                    && !out.contains(&model)
                {
                    out.push(model);
                }
            }
        }
        out
    }

    /// Rewrites every model reference through `f`.
    pub fn remap_references(&mut self, f: &mut impl FnMut(DocRef) -> DocRef) {
        for s in &mut self.sheets {
            for v in &mut s.views {
                if let ViewKind::Base { model, .. } = &mut v.kind {
                    *model = f(*model);
                }
            }
        }
    }

    /// Keeps indexes valid after loading.
    pub fn fix(&mut self) {
        if self.sheets.is_empty() {
            self.sheets
                .push(Sheet::new("Sheet:1", SheetSize::A3, Orientation::Landscape));
        }
        self.active_sheet = self.active_sheet.min(self.sheets.len() - 1);
        let max = self
            .sheets
            .iter()
            .flat_map(|s| &s.views)
            .map(|v| v.id)
            .max()
            .unwrap_or(0);
        self.next_view_id = self.next_view_id.max(max + 1);
        let max = self
            .sheets
            .iter()
            .flat_map(|s| &s.annotations)
            .map(|a| a.id)
            .max()
            .unwrap_or(0);
        self.next_annotation_id = self.next_annotation_id.max(max + 1);
    }

    /// Adds an annotation to the active sheet; returns its id.
    pub fn add_annotation(&mut self, kind: AnnotationKind) -> Option<u32> {
        let id = self.next_annotation_id.max(1);
        let sheet = self.sheets.get_mut(self.active_sheet)?;
        sheet.annotations.push(Annotation { id, kind });
        self.next_annotation_id = id + 1;
        Some(id)
    }

    /// Removes annotation `id` of the active sheet; returns whether it did.
    pub fn delete_annotation(&mut self, id: u32) -> bool {
        let Some(sheet) = self.sheet_mut() else {
            return false;
        };
        let n = sheet.annotations.len();
        sheet.annotations.retain(|a| a.id != id);
        sheet.annotations.len() != n
    }
}

/// Keeps an orthographic projected view in its parent's row or column.
fn align(pos: &mut [f64; 2], parent: [f64; 2], dir: ProjectDir) {
    match dir {
        ProjectDir::Left | ProjectDir::Right => pos[1] = parent[1],
        ProjectDir::Up | ProjectDir::Down => pos[0] = parent[0],
        ProjectDir::Iso(..) => {}
    }
}

#[cfg(test)]
mod tests;
