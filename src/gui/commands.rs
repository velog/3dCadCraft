//! The command registry: one table of every command the application has or
//! lists (`docs/spec/0.15-context-menus-shortcuts.md` §9, group G0).
//!
//! Each [`Command`] has a stable `id`, the [`Action`] it runs (when it is
//! implemented), its names (English, Inventor's when different, Bulgarian),
//! Inventor's default aliases, the default key, its [`Icon`], where it lives
//! (ribbon Tab › Panel, a menu, or only the keyboard), the environments it
//! is valid in, the tooltip, the spec file F1 opens and, for unimplemented
//! commands, the `docs/FEATURES.md` section.
//!
//! Who reads it: the ribbon (labels, icons, keys, tooltips — the layout of
//! the panels stays in `ribbon.rs`), the Repeat label (`command_label`), F1
//! for sketch tools (`open_help`), the recent-command list
//! ([`remember`]) and the matcher ([`search`]) that the alias input (G6),
//! the command search (G7) and the keymap (G5) build on.
//!
//! **A new command is added here first**, then placed on the ribbon with
//! `ribbon::c("<id>")`; the tests check that both agree.

// The search API (`search`, `Hit`, `Env`, `path`, `unavailable`) is the
// base of groups G5–G7, which call it from the GUI; until then only the
// tests do.
#![allow(dead_code)]

use std::sync::OnceLock;

use super::camera::Projection;
use super::demos::Demo;
use super::icons::Icon;
use super::layout::{Element, UiCmd, VisKind};
use super::material::MatCmd;
use super::ribbon::Action;
use super::sketch_tools::{ConstraintTool as Ct, Tool};
use super::viewcube::StandardView;
use crate::command::matcher::{self, Fields, Folded, Query, Tier};
use crate::part::PrimitiveKind as Prim;
use crate::part::export::ExportFormat;
use crate::sketch::Style;

/// An environment a command can run in. Assemblies and drawings add
/// variants here (rule 8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Env {
    /// The part, no sketch in edit.
    Part,
    /// A 2D sketch in edit.
    Sketch,
    /// An assembly is the active document.
    Assembly,
}

impl Env {
    /// Every environment (for tests and the keymap's filter).
    pub const ALL: [Env; 3] = [Env::Part, Env::Sketch, Env::Assembly];

    /// English name, as in messages ("not available in a sketch").
    pub fn label(self) -> &'static str {
        match self {
            Env::Part => "a part",
            Env::Sketch => "a sketch",
            Env::Assembly => "an assembly",
        }
    }
}

/// A set of [`Env`]s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Envs(u8);

impl Envs {
    /// Only the part environment.
    pub const PART: Envs = Envs(1);
    /// Only while a sketch is edited.
    pub const SKETCH: Envs = Envs(2);
    /// The part and its sketches (not an assembly).
    pub const ANY: Envs = Envs(3);
    /// Only in an assembly.
    pub const ASSEMBLY: Envs = Envs(4);

    /// Whether `e` is in the set.
    pub const fn has(self, e: Env) -> bool {
        let bit = match e {
            Env::Part => 1,
            Env::Sketch => 2,
            Env::Assembly => 4,
        };
        self.0 & bit != 0
    }
}

/// Where a command can be found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// A ribbon button: tab label, panel title.
    Ribbon(&'static str, &'static str),
    /// A Quick Access menu: File, View, Help.
    Menu(&'static str),
    /// Only a key (F7, Ctrl+A…) or a mouse gesture.
    Keyboard,
}

/// Commands without a ribbon button that still need an [`Action`]
/// (File / Help menus, keys) so menus, the keymap and the search can run
/// them like any other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppCmd {
    /// File → New.
    New,
    /// File → New Assembly: an assembly document in the open project.
    NewAssembly,
    /// File → Open.
    Open,
    /// File → Save.
    Save,
    /// File → Save As.
    SaveAs,
    /// File → Save as Folder.
    SaveAsFolder,
    /// File → Export in a format.
    Export(ExportFormat),
    /// Help (F1): the spec of what the user is doing.
    Help,
    /// Help → Save Diagnostics.
    SaveDiagnostics,
    /// Help → Open Logs Folder.
    OpenLogsFolder,
    /// Slice Graphics (F7, in a sketch).
    SliceGraphics,
    /// Select All (Ctrl+A).
    SelectAll,
    /// Delete the selection (Delete, in a sketch).
    Delete,
    /// Zoom All: fit the model into the view.
    ZoomAll,
    /// Inspect → Measure (M): the Measure window.
    Measure,
}

/// One command of the registry.
#[derive(Clone, Copy, Debug)]
pub struct Command {
    /// Stable id (`part.extrude`, `sketch.line`): settings (recent
    /// commands, keymap, marking menus) store it. Never rename one.
    pub id: &'static str,
    /// What runs it; `None` while it is not implemented (or only a held
    /// key, see `held`).
    pub action: Option<Action>,
    /// English name (status bar, Repeat, search).
    pub name: &'static str,
    /// Shorter ribbon label, when the button says less than the name.
    pub label: Option<&'static str>,
    /// Inventor's name, when it differs from `name`.
    pub inventor: Option<&'static str>,
    /// Bulgarian name (search only: the UI keeps English command names).
    pub bg: &'static str,
    /// Inventor's default aliases (letters; `]`, `/`, `.`, `=`, `-`).
    pub aliases: &'static [&'static str],
    /// Default shortcut (`F6`, `Ctrl+Z`).
    pub key: Option<&'static str>,
    /// `key`, or else the first alias, already works (today's
    /// `CadCraftApp::shortcuts`); the others wait for the keymap (G5).
    pub bound: bool,
    /// Ribbon / menu icon.
    pub icon: Icon,
    /// Where it is; the first place is the one search shows.
    pub places: &'static [Place],
    /// Where it is valid.
    pub envs: Envs,
    /// Tooltip text (without the key).
    pub tip: &'static str,
    /// The spec F1 opens (`docs/spec/<file>`).
    pub spec: Option<&'static str>,
    /// Not implemented yet: the `docs/FEATURES.md` section.
    pub planned: Option<&'static str>,
    /// Works only while a key or button is held (Pan F2, Zoom F3, Orbit F4):
    /// nothing to run from a menu or the search.
    pub held: bool,
}

impl Command {
    const fn new(id: &'static str, name: &'static str, bg: &'static str, icon: Icon) -> Self {
        Self {
            id,
            action: None,
            name,
            label: None,
            inventor: None,
            bg,
            aliases: &[],
            key: None,
            bound: false,
            icon,
            places: &[],
            envs: Envs::ANY,
            tip: "",
            spec: None,
            planned: None,
            held: false,
        }
    }
    const fn run(self, a: Action) -> Self {
        Self {
            action: Some(a),
            ..self
        }
    }
    const fn short(self, l: &'static str) -> Self {
        Self {
            label: Some(l),
            ..self
        }
    }
    const fn inv(self, n: &'static str) -> Self {
        Self {
            inventor: Some(n),
            ..self
        }
    }
    const fn alias(self, a: &'static [&'static str]) -> Self {
        Self { aliases: a, ..self }
    }
    const fn key(self, k: &'static str) -> Self {
        Self {
            key: Some(k),
            ..self
        }
    }
    const fn bound(self) -> Self {
        Self {
            bound: true,
            ..self
        }
    }
    const fn at(self, p: &'static [Place]) -> Self {
        Self { places: p, ..self }
    }
    const fn env(self, e: Envs) -> Self {
        Self { envs: e, ..self }
    }
    const fn tip(self, t: &'static str) -> Self {
        Self { tip: t, ..self }
    }
    const fn spec(self, s: &'static str) -> Self {
        Self {
            spec: Some(s),
            ..self
        }
    }
    const fn todo(self, section: &'static str) -> Self {
        Self {
            planned: Some(section),
            ..self
        }
    }
    const fn held(self) -> Self {
        Self { held: true, ..self }
    }

    /// The ribbon label.
    pub fn ribbon_label(&self) -> &'static str {
        self.label.unwrap_or(self.name)
    }

    /// The key shown in tooltips: the shortcut or first alias, if it works
    /// today.
    pub fn shown_key(&self) -> Option<&'static str> {
        if self.bound {
            self.key.or(self.aliases.first().copied())
        } else {
            None
        }
    }

    /// "Tab › Panel" (or the menu name) where it is found; empty for
    /// keyboard-only commands.
    pub fn path(&self) -> String {
        match self.places.first() {
            Some(Place::Ribbon(t, p)) => format!("{t} › {p}"),
            Some(Place::Menu(m)) => (*m).to_owned(),
            Some(Place::Keyboard) | None => String::new(),
        }
    }

    /// Implemented and runnable from a menu, the alias input or the search.
    pub fn ready(&self) -> bool {
        self.action.is_some() && self.planned.is_none()
    }

    /// Whether it runs in `env`. The application's own commands (File,
    /// Help, View, Undo / Redo, Options) run in an assembly too; part
    /// exports do not (they export the part).
    pub fn valid_in(&self, env: Env) -> bool {
        self.envs.has(env)
            || (env == Env::Assembly
                && (self.id.starts_with("view.")
                    || self.id.starts_with("help.")
                    || (self.id.starts_with("file.") && !self.id.starts_with("file.export"))
                    || matches!(self.id, "tools.undo" | "tools.redo" | "tools.options")))
    }

    /// Why it cannot run in `env` (for grey items and red alias text), or
    /// `None` when it can.
    pub fn unavailable(&self, env: Env) -> Option<String> {
        if let Some(s) = self.planned {
            Some(format!("Planned — docs/FEATURES.md, section {s}"))
        } else if self.held {
            self.key.map(|k| format!("Hold {k} and drag"))
        } else if !self.valid_in(env) {
            Some(format!("not available in {}", env.label()))
        } else {
            None
        }
    }
}

// ---- places -----------------------------------------------------------------

const M_SKETCH: Place = Place::Ribbon("3D Model", "Sketch");
const M_CREATE: Place = Place::Ribbon("3D Model", "Create");
const M_MODIFY: Place = Place::Ribbon("3D Model", "Modify");
const M_WORK: Place = Place::Ribbon("3D Model", "Work Features");
const M_PATTERN: Place = Place::Ribbon("3D Model", "Pattern");
const M_PRIM: Place = Place::Ribbon("3D Model", "Primitives");
const S_CREATE: Place = Place::Ribbon("Sketch", "Create");
const S_MODIFY: Place = Place::Ribbon("Sketch", "Modify");
const S_PATTERN: Place = Place::Ribbon("Sketch", "Pattern");
const S_CONSTRAIN: Place = Place::Ribbon("Sketch", "Constrain");
const S_FORMAT: Place = Place::Ribbon("Sketch", "Format");
const S_DEMO: Place = Place::Ribbon("Sketch", "Demo");
const S_EXIT: Place = Place::Ribbon("Sketch", "Exit");
const I_MEASURE: Place = Place::Ribbon("Inspect", "Measure");
const I_ANALYSIS: Place = Place::Ribbon("Inspect", "Analysis");
const T_OPTIONS: Place = Place::Ribbon("Tools", "Options");
const T_LOOK: Place = Place::Ribbon("Tools", "Material and Appearance");
const T_PARAMS: Place = Place::Ribbon("Tools", "Parameters");
const T_UPDATE: Place = Place::Ribbon("Tools", "Update");
const T_AUTO: Place = Place::Ribbon("Tools", "Automation");
const T_HISTORY: Place = Place::Ribbon("Tools", "History");
const G_STYLES: Place = Place::Ribbon("Manage", "Styles and Standards");
const V_VIS: Place = Place::Ribbon("View", "Visibility");
const A_COMPONENT: Place = Place::Ribbon("Assemble", "Component");
const A_POSITION: Place = Place::Ribbon("Assemble", "Position");
const A_RELATIONSHIPS: Place = Place::Ribbon("Assemble", "Relationships");
const A_PATTERN: Place = Place::Ribbon("Assemble", "Pattern");
const V_NAV: Place = Place::Ribbon("View", "Navigate");
const V_LOOK: Place = Place::Ribbon("View", "Appearance");
const V_WIN: Place = Place::Ribbon("View", "Windows");
const FILE: Place = Place::Menu("File");
const VIEW: Place = Place::Menu("View");
const HELP: Place = Place::Menu("Help");
const KEY: Place = Place::Keyboard;

const PART: Envs = Envs::PART;
const ASSEMBLY: Envs = Envs::ASSEMBLY;
const SKETCH: Envs = Envs::SKETCH;

const fn c(id: &'static str, name: &'static str, bg: &'static str, icon: Icon) -> Command {
    Command::new(id, name, bg, icon)
}

/// A sketch tool: name = `Tool::label`, valid in a sketch.
const fn t(
    id: &'static str,
    tool: Tool,
    name: &'static str,
    bg: &'static str,
    icon: Icon,
) -> Command {
    Command::new(id, name, bg, icon)
        .run(Action::Tool(tool))
        .env(SKETCH)
}

/// A constraint tool.
const fn con(
    id: &'static str,
    k: Ct,
    name: &'static str,
    inv: &'static str,
    bg: &'static str,
    icon: Icon,
) -> Command {
    t(id, Tool::Constrain(k), name, bg, icon)
        .inv(inv)
        .at(&[S_CONSTRAIN])
        .tip("Pick the geometry in order")
        .spec("1.3-constraints.md")
}

/// Every command, in ribbon order, then menus and keys.
pub static COMMANDS: &[Command] = &[
    // ---- 3D Model --------------------------------------------------------
    c("part.start_sketch", "Start 2D Sketch", "Нова 2D скица", Icon::StartSketch)
        .run(Action::StartSketch)
        .inv("Start 2D Sketch")
        .alias(&["S"])
        .bound()
        .at(&[M_SKETCH])
        .env(PART)
        .tip("Pick an origin plane or a planar face to sketch on")
        .spec("1.1-sketch.md"),
    c("part.start_3d_sketch", "Start 3D Sketch", "Нова 3D скица", Icon::StartSketch)
        .alias(&["S3"])
        .at(&[KEY])
        .env(PART)
        .todo("1.10"),
    c("part.extrude", "Extrude", "Изтегляне (екструдиране)", Icon::Extrude)
        .run(Action::Extrude)
        .alias(&["E"])
        .bound()
        .at(&[M_CREATE])
        .tip("Extrude closed sketch regions into a solid")
        .spec("2.1-extrude.md"),
    c("part.revolve", "Revolve", "Ротационно тяло (завъртане)", Icon::Revolve)
        .run(Action::Revolve)
        .alias(&["R"])
        .bound()
        .at(&[M_CREATE])
        .env(PART)
        .tip("Revolve closed sketch regions around an axis")
        .spec("2.2-revolve.md"),
    c("part.sweep", "Sweep", "Протегляне по път", Icon::Sweep)
        .run(Action::Sweep)
        .alias(&["SW"])
        .at(&[M_CREATE])
        .env(PART)
        .tip("Sweep: move closed profiles along a path (sketch curves or edges)")
        .spec("5.1-sweep.md"),
    c("part.loft", "Loft", "Преход между сечения", Icon::Loft)
        .run(Action::Loft)
        .alias(&["LO"])
        .at(&[M_CREATE])
        .env(PART)
        .tip("Loft: a body through sections in order (profiles, faces, a point)")
        .spec("5.2-loft.md"),
    c("part.coil", "Coil", "Спирала (пружина)", Icon::Coil)
        .run(Action::Coil)
        .at(&[M_CREATE])
        .env(PART)
        .tip("Coil: screw closed profiles about an axis (springs, threads with Cut)")
        .spec("5.3-coil.md"),
    c("part.rib", "Rib", "Ребро", Icon::Rib)
        .run(Action::Rib)
        .at(&[M_CREATE])
        .env(PART)
        .tip("Rib: thin walls from open sketch curves out to the part (ribs and webs)")
        .spec("2.7-rib.md"),
    c("part.emboss", "Emboss", "Релеф (Emboss)", Icon::Emboss)
        .at(&[M_CREATE])
        .env(PART)
        .todo("2.17"),
    c("part.hole", "Hole", "Отвор", Icon::Hole)
        .run(Action::Hole)
        .alias(&["H"])
        .bound()
        .at(&[M_MODIFY])
        .env(PART)
        .tip("Hole: click a face, sketch points or a circular edge for the centres (Simple, Clearance, Tapped; Counterbore, Spotface, Countersink)")
        .spec("2.3-hole.md"),
    c("part.fillet", "Fillet", "Закръгление на ръбове", Icon::FilletEdge)
        .run(Action::Fillet)
        .alias(&["F", "FI"])
        .at(&[M_MODIFY])
        .env(PART)
        .tip("Fillet: round the picked edges (convex: cut, concave: add)")
        .spec("2.4-fillet.md"),
    c("part.chamfer", "Chamfer", "Фаска на ръбове", Icon::ChamferEdge)
        .run(Action::Chamfer)
        .alias(&["CH"])
        .at(&[M_MODIFY])
        .env(PART)
        .tip("Chamfer: bevel the picked edges (Distance, Two Distances, Distance and Angle)")
        .spec("2.5-chamfer.md"),
    c("part.shell", "Shell", "Черупка (кухо тяло)", Icon::Shell)
        .run(Action::Shell)
        .alias(&["SH"])
        .at(&[M_MODIFY])
        .env(PART)
        .tip("Shell: hollow the solid, remove the picked faces (Inside, Outside, Both)")
        .spec("2.6-shell.md"),
    c("part.draft", "Face Draft", "Наклон на стени", Icon::Draft)
        .run(Action::Draft)
        .short("Draft")
        .alias(&["D"])
        .at(&[M_MODIFY])
        .env(PART)
        .tip("Face Draft: tilt faces against a pull direction (Fixed Edge, Fixed Plane)")
        .spec("2.8-draft.md"),
    c("part.thread", "Thread", "Резба", Icon::Thread)
        .run(Action::Thread)
        .alias(&["TH"])
        .at(&[M_MODIFY])
        .env(PART)
        .tip("Thread: click a cylindrical face near the end the thread starts at (cosmetic or modeled)")
        .spec("2.9-thread.md"),
    c("part.slot", "Slot", "Прорез (операция)", Icon::SlotFeature)
        .at(&[M_MODIFY])
        .env(PART)
        .todo("2.3"),
    c("part.combine", "Combine", "Комбиниране на тела", Icon::Combine)
        .run(Action::Combine)
        .at(&[M_MODIFY])
        .env(PART)
        .tip("Combine: join, cut or intersect a base body with toolbodies (multi-body parts)")
        .spec("2.13-combine-split.md"),
    c("part.split", "Split", "Разделяне на тяло", Icon::SplitBody)
        .run(Action::Split)
        .at(&[M_MODIFY])
        .env(PART)
        .tip("Split: split faces, trim a solid, or split it in two (a plane or a sketch)")
        .spec("2.13-combine-split.md"),
    c("part.work_plane", "Work Plane", "Работна равнина", Icon::WorkPlane)
        .run(Action::WorkPlane)
        .short("Plane")
        .inv("Plane")
        .alias(&["]"])
        .at(&[M_WORK])
        .env(PART)
        .tip("Work Plane: pick a plane (offset), two planes (midplane), \
              a plane and an edge (angle), a plane and a point, three points, \
              a curved face (tangent, torus midplane) or a curve and a point")
        .spec("2.10-work-features.md"),
    c("part.work_axis", "Work Axis", "Работна ос", Icon::WorkAxis)
        .run(Action::WorkAxis)
        .short("Axis")
        .inv("Axis")
        .alias(&["/"])
        .at(&[M_WORK])
        .env(PART)
        .tip("Work Axis: an edge or sketch line, two points, two planes, a plane \
              and a point, a line and a point, a circular or elliptical edge, or a \
              cylindrical, conical or toroidal face")
        .spec("2.10-work-features.md"),
    c("part.work_point", "Work Point", "Работна точка", Icon::WorkPoint)
        .run(Action::WorkPoint)
        .short("Point")
        .inv("Point")
        .alias(&["."])
        .at(&[M_WORK])
        .env(PART)
        .tip("Work Point: a vertex, sketch point or midpoint, three planes, two \
              lines, a plane or curved face and a line, a circular edge (centre), a \
              sphere or torus (centre); or Grounded at X, Y, Z")
        .spec("2.10-work-features.md"),
    c("part.ucs", "UCS", "Потребителска координатна система", Icon::Ucs)
        .run(Action::Ucs)
        .at(&[M_WORK])
        .env(PART)
        .tip("UCS: a coordinate system with its own planes, axes and centre; \
              set X, Y, Z and angles, or pick three points (centre, X, Y)")
        .spec("2.10-work-features.md"),
    c("part.rect_pattern", "Rectangular Pattern", "Правоъгълен масив", Icon::PatternRect)
        .run(Action::RectPattern)
        .short("Rectangular")
        .alias(&["RP"])
        .at(&[M_PATTERN])
        .env(PART)
        .tip("Rectangular Pattern: copy features or solids in rows along one or two directions")
        .spec("2.11-patterns.md"),
    c("part.circ_pattern", "Circular Pattern", "Кръгов масив", Icon::PatternCirc)
        .run(Action::CircPattern)
        .short("Circular")
        .alias(&["CP"])
        .at(&[M_PATTERN])
        .env(PART)
        .tip("Circular Pattern: copy features or solids at equal angles about an axis")
        .spec("2.11-patterns.md"),
    c("part.mirror", "Mirror", "Огледално копие", Icon::Mirror)
        .run(Action::MirrorFeature)
        .alias(&["MI"])
        .at(&[M_PATTERN])
        .env(PART)
        .tip("Mirror: copy features or solids mirrored across a plane")
        .spec("2.12-mirror.md"),
    c("part.box", "Box", "Паралелепипед (кутия)", Icon::Box)
        .run(Action::Primitive(Prim::Box))
        .at(&[M_PRIM])
        .env(PART)
        .tip("Box: pick a plane, click the centre, then a corner (a sketch + Extrude)")
        .spec("2.1-extrude.md"),
    c("part.cylinder", "Cylinder", "Цилиндър", Icon::Cylinder)
        .run(Action::Primitive(Prim::Cylinder))
        .at(&[M_PRIM])
        .env(PART)
        .tip("Cylinder: pick a plane, click the centre, then the diameter (a sketch + Extrude)")
        .spec("2.1-extrude.md"),
    c("part.sphere", "Sphere", "Сфера", Icon::Sphere)
        .run(Action::Primitive(Prim::Sphere))
        .at(&[M_PRIM])
        .env(PART)
        .tip("Sphere: pick a plane, click the centre, then the diameter (a sketch + full Revolve)")
        .spec("2.2-revolve.md"),
    c("part.torus", "Torus", "Тор", Icon::Torus)
        .run(Action::Primitive(Prim::Torus))
        .at(&[M_PRIM])
        .env(PART)
        .tip("Torus: pick a plane, click the centre, the section centre, then the section diameter")
        .spec("2.2-revolve.md"),
    // ---- Sketch: Create --------------------------------------------------
    t("sketch.line", Tool::Line, "Line", "Линия", Icon::Line)
        .alias(&["L"])
        .bound()
        .at(&[S_CREATE])
        .tip("Click points; drag from a curve end for an arc")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.circle_center", Tool::CircleCenter, "Center Point Circle", "Окръжност по център", Icon::CircleCenter)
        .alias(&["C"])
        .bound()
        .at(&[S_CREATE])
        .tip("Circle: centre, then a point on it")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.circle_tangent", Tool::CircleTangent, "Tangent Circle", "Допирателна окръжност", Icon::CircleTangent)
        .at(&[S_CREATE])
        .tip("Circle tangent to three lines")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.arc_three", Tool::ArcThreePoint, "Three Point Arc", "Дъга през три точки", Icon::ArcThree)
        .alias(&["A3"])
        .at(&[S_CREATE])
        .tip("Arc: start, end, then a point on it")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.arc_tangent", Tool::ArcTangent, "Tangent Arc", "Допирателна дъга", Icon::ArcTangent)
        .alias(&["A"])
        .at(&[S_CREATE])
        .tip("Arc tangent to a curve at its end")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.arc_center", Tool::ArcCenter, "Center Point Arc", "Дъга по център", Icon::ArcCenter)
        .at(&[S_CREATE])
        .tip("Arc: centre, start, end")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.rect_two", Tool::RectTwoPoint, "Two Point Rectangle", "Правоъгълник по две точки", Icon::Rect2)
        .alias(&["R", "REC"])
        .bound()
        .at(&[S_CREATE])
        .tip("Rectangle by two opposite corners")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.rect_three", Tool::RectThreePoint, "Three Point Rectangle", "Правоъгълник по три точки", Icon::Rect3)
        .alias(&["REC3"])
        .at(&[S_CREATE])
        .tip("Rectangle by one side, then its width")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.rect_center", Tool::RectCenter, "Two Point Center Rectangle", "Правоъгълник от центъра", Icon::RectCenter)
        .at(&[S_CREATE])
        .tip("Rectangle by its centre and a corner")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.rect_center3", Tool::RectCenterThreePoint, "Three Point Center Rectangle", "Правоъгълник от центъра по три точки", Icon::RectCenter3)
        .at(&[S_CREATE])
        .tip("Rectangle by its centre, a side midpoint and a corner")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.slot_cc", Tool::SlotCenterToCenter, "Center to Center Slot", "Прорез център до център", Icon::SlotCenter)
        .at(&[S_CREATE])
        .tip("Slot by its two arc centres and the width")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.slot_overall", Tool::SlotOverall, "Overall Slot", "Прорез по обща дължина", Icon::SlotOverall)
        .at(&[S_CREATE])
        .tip("Slot by its overall length and the width")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.slot_center", Tool::SlotCenterPoint, "Center Point Slot", "Прорез от центъра", Icon::SlotPoint)
        .at(&[S_CREATE])
        .tip("Slot by its centre, an arc centre and the width")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.slot_arc3", Tool::SlotArcThreePoint, "Three Point Arc Slot", "Дъгов прорез през три точки", Icon::SlotArc)
        .at(&[S_CREATE])
        .tip("Curved slot along a three point arc")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.slot_arc_center", Tool::SlotArcCenter, "Center Point Arc Slot", "Дъгов прорез по център", Icon::SlotArc)
        .at(&[S_CREATE])
        .tip("Curved slot along a centre point arc")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.spline_interp", Tool::SplineInterp, "Interpolation Spline", "Интерполационен сплайн", Icon::SplineInterp)
        .inv("Spline Interpolation")
        .alias(&["SP"])
        .at(&[S_CREATE])
        .tip("Spline through the clicked points")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.spline_cv", Tool::SplineCv, "Control Vertex Spline", "Сплайн по контролни върхове", Icon::Spline)
        .inv("Spline Control Vertex")
        .at(&[S_CREATE])
        .tip("Spline pulled by its control vertices")
        .spec("1.2-sketch-geometry.md"),
    c("sketch.ellipse", "Ellipse", "Елипса", Icon::Ellipse)
        .at(&[S_CREATE])
        .env(SKETCH)
        .todo("1.2"),
    t("sketch.point", Tool::Point, "Point", "Точка", Icon::Point)
        .alias(&["PO"])
        .at(&[S_CREATE])
        .tip("Sketch point")
        .spec("1.2-sketch-geometry.md"),
    c("sketch.project", "Project Geometry", "Проектиране на геометрия", Icon::Project)
        .run(Action::ProjectGeometry)
        .alias(&["P"])
        .at(&[S_CREATE])
        .env(SKETCH)
        .tip("Project points and curves of earlier sketches, or model edges, into this sketch (associative; Shift: construction; Esc ends)")
        .spec("1.8-project-geometry.md"),
    t("sketch.polygon", Tool::Polygon, "Polygon", "Многоъгълник", Icon::Polygon)
        .alias(&["POL"])
        .at(&[S_CREATE])
        .tip("Sides and inscribed/circumscribed in Properties")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.fillet", Tool::Fillet, "Fillet", "Закръгление (2D)", Icon::Fillet2d)
        .alias(&["F"])
        .at(&[S_CREATE])
        .tip("Round the corner of two curves")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.chamfer", Tool::Chamfer, "Chamfer", "Фаска (2D)", Icon::Chamfer2d)
        .at(&[S_CREATE])
        .tip("Bevel the corner of two lines")
        .spec("1.2-sketch-geometry.md"),
    c("sketch.text", "Text", "Текст", Icon::Text)
        .alias(&["T"])
        .at(&[S_CREATE])
        .env(SKETCH)
        .todo("1.2"),
    t("sketch.select", Tool::Select, "Select", "Избор", Icon::Select)
        .key("Esc")
        .bound()
        .at(&[S_CREATE])
        .tip("Select geometry, drag points")
        .spec("1.1-sketch.md"),
    // ---- Sketch: Modify --------------------------------------------------
    t("sketch.move", Tool::Move, "Move", "Преместване", Icon::Move)
        .alias(&["MO"])
        .at(&[S_MODIFY])
        .tip("Selected geometry: base point, destination")
        .spec("1.6-modify.md"),
    t("sketch.copy", Tool::Copy, "Copy", "Копиране", Icon::Copy)
        .at(&[S_MODIFY])
        .tip("Copy selected geometry (with its constraints)")
        .spec("1.6-modify.md"),
    t("sketch.rotate", Tool::Rotate, "Rotate", "Завъртане", Icon::Rotate)
        .at(&[S_MODIFY])
        .tip("Selected geometry: centre, reference, angle")
        .spec("1.6-modify.md"),
    t("sketch.trim", Tool::Trim, "Trim", "Отрязване", Icon::Trim)
        .alias(&["X", "TR"])
        .bound()
        .at(&[S_MODIFY])
        .tip("Remove the clicked part up to the nearest curves (Shift = Extend)")
        .spec("1.6-modify.md"),
    t("sketch.extend", Tool::Extend, "Extend", "Удължаване", Icon::Extend)
        .alias(&["EX"])
        .at(&[S_MODIFY])
        .tip("Extend to the nearest curve (Shift = Trim)")
        .spec("1.6-modify.md"),
    t("sketch.split", Tool::Split, "Split", "Разцепване на крива", Icon::Split)
        .at(&[S_MODIFY])
        .tip("Split at the nearest intersection")
        .spec("1.6-modify.md"),
    t("sketch.offset", Tool::Offset, "Offset", "Отместване", Icon::Offset)
        .alias(&["O"])
        .bound()
        .at(&[S_MODIFY])
        .tip("Offset a curve or loop")
        .spec("1.6-modify.md"),
    t("sketch.scale", Tool::Scale, "Scale", "Мащабиране", Icon::Scale)
        .at(&[S_MODIFY])
        .tip("Selected geometry about a base point (factor in Properties)")
        .spec("1.6-modify.md"),
    t("sketch.stretch", Tool::Stretch, "Stretch", "Разтягане", Icon::Stretch)
        .at(&[S_MODIFY])
        .tip("Move the selected geometry's points; dimensions update")
        .spec("1.6-modify.md"),
    // ---- Sketch: Pattern -------------------------------------------------
    t("sketch.rect_pattern", Tool::PatternRect, "Rectangular Pattern", "Правоъгълен масив в скица", Icon::PatternRect)
        .short("Rectangular")
        .alias(&["RP"])
        .at(&[S_PATTERN])
        .tip("Selected geometry along one or two lines (count, spacing in Properties)")
        .spec("1.7-patterns.md"),
    t("sketch.circ_pattern", Tool::PatternCircular, "Circular Pattern", "Кръгов масив в скица", Icon::PatternCirc)
        .short("Circular")
        .alias(&["CP"])
        .at(&[S_PATTERN])
        .tip("Selected geometry around a point (count, angle in Properties)")
        .spec("1.7-patterns.md"),
    t("sketch.mirror", Tool::Mirror, "Mirror", "Огледално копие в скица", Icon::Mirror)
        .alias(&["MI"])
        .at(&[S_PATTERN])
        .tip("Mirror selected geometry about a line (Symmetric constraints)")
        .spec("1.7-patterns.md"),
    t("sketch.path_pattern", Tool::PatternPath, "Pattern along Path", "Масив по крива", Icon::PatternPath)
        .at(&[S_PATTERN])
        .tip("Copies of the selected geometry along a line, arc or spline (equal chords)")
        .spec("1.7-patterns.md"),
    t("sketch.point_pattern", Tool::PatternPoints, "Pattern at Points", "Масив по точки", Icon::PatternPoints)
        .inv("Sketch Driven Pattern")
        .at(&[S_PATTERN])
        .tip("Copies of the selected geometry at points; each copy follows its point (Inventor: Sketch Driven Pattern)")
        .spec("1.7-patterns.md"),
    // ---- Sketch: Constrain -----------------------------------------------
    t("sketch.dimension", Tool::Dimension, "Dimension", "Размер", Icon::Dimension)
        .inv("General Dimension")
        .alias(&["D"])
        .bound()
        .at(&[S_CONSTRAIN])
        .tip("General Dimension: kind follows the selection and placement")
        .spec("1.4-dimensions.md"),
    con("sketch.coincident", Ct::Coincident, "Coincident", "Coincident Constraint", "Съвпадение", Icon::Coincident),
    con("sketch.collinear", Ct::Collinear, "Collinear", "Collinear Constraint", "Колинеарност", Icon::Collinear),
    con("sketch.concentric", Ct::Concentric, "Concentric", "Concentric Constraint", "Концентричност", Icon::Concentric),
    con("sketch.fix", Ct::Fix, "Fix", "Fix Constraint", "Фиксиране", Icon::Fix),
    con("sketch.parallel", Ct::Parallel, "Parallel", "Parallel Constraint", "Успоредност", Icon::Parallel),
    con("sketch.perpendicular", Ct::Perpendicular, "Perpendicular", "Perpendicular Constraint", "Перпендикулярност", Icon::Perpendicular),
    con("sketch.horizontal", Ct::Horizontal, "Horizontal", "Horizontal Constraint", "Хоризонталност", Icon::Horizontal)
        .alias(&["-"]),
    con("sketch.vertical", Ct::Vertical, "Vertical", "Vertical Constraint", "Вертикалност", Icon::Vertical)
        .alias(&["I"]),
    con("sketch.tangent", Ct::Tangent, "Tangent", "Tangent Constraint", "Допирателност", Icon::Tangent),
    con("sketch.symmetric", Ct::Symmetric, "Symmetric", "Symmetric Constraint", "Симетрия", Icon::Symmetric),
    con("sketch.equal", Ct::Equal, "Equal", "Equal Constraint", "Равенство", Icon::Equal)
        .alias(&["="]),
    c("sketch.smooth", "Smooth (G2)", "Гладкост (G2)", Icon::Smooth)
        .inv("Smooth (G2) Constraint")
        .at(&[S_CONSTRAIN])
        .env(SKETCH)
        .todo("1.3"),
    c("sketch.relax", "Relax Mode", "Свободно влачене (Relax)", Icon::Relax)
        .run(Action::ToggleRelax)
        .key("F11")
        .bound()
        .at(&[S_CONSTRAIN])
        .env(SKETCH)
        .tip("Relax Mode: dragging may change dimensions; changed ones become driven")
        .spec("1.4-dimensions.md"),
    c("sketch.dof", "Show All Degrees of Freedom", "Степени на свобода", Icon::Dof)
        .run(Action::ToggleDof)
        .short("Show DOF")
        .key("Ctrl+Shift+E")
        .at(&[S_CONSTRAIN, V_VIS])
        .tip("Show All Degrees of Freedom")
        .spec("1.3-constraints.md"),
    c("sketch.show_constraints", "Show All Constraints", "Показване на всички ограничения", Icon::ShowConstraints)
        .run(Action::ShowConstraints(true))
        .short("Show All")
        .key("F8")
        .bound()
        .at(&[S_CONSTRAIN, V_VIS])
        .tip("Show or hide all constraint labels")
        .spec("1.3-constraints.md"),
    c("sketch.hide_constraints", "Hide All Constraints", "Скриване на всички ограничения", Icon::ShowConstraints)
        .run(Action::ShowConstraints(false))
        .short("Hide All")
        .key("F9")
        .bound()
        .at(&[S_CONSTRAIN, V_VIS])
        .tip("Show or hide all constraint labels")
        .spec("1.3-constraints.md"),
    c("sketch.coordinate_system", "Edit Coordinate System", "Координатна система на скицата", Icon::CoordSystem)
        .run(Action::EditCoordinateSystem)
        .short("Coordinate System")
        .at(&[S_CONSTRAIN])
        .env(SKETCH)
        .tip("Edit Coordinate System: origin, axis edge and direction (Properties)")
        .spec("1.1-sketch.md"),
    // ---- Sketch: Format, Demo, Exit --------------------------------------
    c("sketch.construction", "Construction", "Конструктивна линия", Icon::Construction)
        .run(Action::Format(Style::Construction))
        .at(&[S_FORMAT])
        .env(SKETCH)
        .tip("Selected geometry → construction; with nothing selected, toggles construction mode")
        .spec("1.2-sketch-geometry.md"),
    c("sketch.centerline", "Centerline", "Осева линия", Icon::Centerline)
        .run(Action::Format(Style::Centerline))
        .at(&[S_FORMAT])
        .env(SKETCH)
        .tip("Selected geometry → centerline")
        .spec("1.2-sketch-geometry.md"),
    c("sketch.normal", "Normal", "Нормална линия", Icon::Normal)
        .run(Action::Format(Style::Normal))
        .at(&[S_FORMAT])
        .env(SKETCH)
        .tip("Selected geometry → normal")
        .spec("1.2-sketch-geometry.md"),
    c("sketch.center_point", "Center Point", "Централна точка", Icon::Point)
        .run(Action::CenterPoint)
        .at(&[S_FORMAT])
        .env(SKETCH)
        .tip("New points are Center Points (a cross; Hole takes them as centres)")
        .spec("2.3-hole.md"),
    c("demo.rectangle", Demo::Rectangle.label(), "Пример: напълно определен правоъгълник", Icon::Demo)
        .run(Action::Demo(Demo::Rectangle))
        .at(&[S_DEMO])
        .tip("Demo sketch on the box's top face"),
    c("demo.slot", Demo::Slot.label(), "Пример: прорез", Icon::Demo)
        .run(Action::Demo(Demo::Slot))
        .at(&[S_DEMO])
        .tip("Demo sketch on the box's top face"),
    c("demo.tangents", Demo::Tangents.label(), "Пример: допирателни окръжности", Icon::Demo)
        .run(Action::Demo(Demo::Tangents))
        .at(&[S_DEMO])
        .tip("Demo sketch on the box's top face"),
    c("demo.lines", Demo::Lines.label(), "Пример: успоредни и перпендикулярни линии", Icon::Demo)
        .run(Action::Demo(Demo::Lines))
        .at(&[S_DEMO])
        .tip("Demo sketch on the box's top face"),
    c("demo.revolve", Demo::Revolve.label(), "Пример: половин сечение на вал", Icon::Demo)
        .run(Action::Demo(Demo::Revolve))
        .at(&[S_DEMO])
        .tip("Demo sketch on the box's top face"),
    c("sketch.finish", "Finish Sketch", "Край на скицата", Icon::Finish)
        .run(Action::FinishSketch)
        .inv("Finish 2D Sketch")
        .at(&[S_EXIT])
        .env(SKETCH)
        .tip("Leave sketch edit mode")
        .spec("1.1-sketch.md"),
    // ---- Inspect ---------------------------------------------------------
    c("inspect.measure", "Measure", "Измерване", Icon::Measure)
        .run(Action::App(AppCmd::Measure))
        .at(&[I_MEASURE])
        .tip("Length, area, volume, radius of the selection; distance and angle of two items")
        .spec("0.10-physical.md")
        .key("m")
        .bound(),
    c("inspect.physical", "Physical Properties", "Физични свойства (маса, център на тежестта)", Icon::Mass)
        .run(Action::Material(MatCmd::Physical))
        .inv("iProperties: Physical")
        .at(&[I_MEASURE])
        .tip("Mass, area, volume, centre of gravity and inertia (iProperties: Physical)")
        .spec("0.10-physical.md"),
    c("inspect.validate", "Validate Solid", "Проверка на тялото", Icon::Validate)
        .run(Action::SelectBody)
        .at(&[I_ANALYSIS])
        .tip("Euler–Poincaré and manifold check of the body")
        .spec("2.16-browser.md"),
    c("inspect.interference", "Interference", "Застъпване", Icon::Interference)
        .at(&[I_ANALYSIS])
        .todo("3.5"),
    c("inspect.curvature", "Curvature", "Кривина", Icon::Curvature)
        .at(&[I_ANALYSIS])
        .todo("5.4"),
    c("inspect.draft_analysis", "Draft Analysis", "Анализ на наклона", Icon::Draft)
        .at(&[I_ANALYSIS])
        .todo("2.8"),
    // ---- Tools -----------------------------------------------------------
    c("tools.options", "Application Options", "Настройки на приложението", Icon::Options)
        .run(Action::Options)
        .at(&[T_OPTIONS])
        .tip("Mouse profile, wheel direction, sketch options")
        .spec("0.13-ui-layout.md"),
    c("sketch.constraint_settings", "Constraint Settings", "Настройки на ограниченията", Icon::Settings)
        .run(Action::Options)
        .at(&[S_CONSTRAIN])
        .tip("Inference: which constraints are inferred and created while sketching")
        .spec("1.5-inference.md"),
    c("tools.document_settings", "Document Settings", "Настройки на документа", Icon::Settings)
        .at(&[T_OPTIONS])
        .todo("0.11"),
    c("tools.material", "Material", "Материал", Icon::Material)
        .run(Action::Material(MatCmd::MaterialBrowser))
        .inv("Material Browser")
        .at(&[T_LOOK])
        .tip("Material Browser: document and library materials")
        .spec("2.15-materials.md"),
    c("tools.appearance", "Appearance", "Външен вид", Icon::Appearance)
        .run(Action::Material(MatCmd::AppearanceBrowser))
        .inv("Appearance Browser")
        .at(&[T_LOOK])
        .tip("Appearance Browser: document and library appearances")
        .spec("2.15-materials.md"),
    c("tools.adjust", "Adjust", "Промяна на вида (цвят)", Icon::AdjustLook)
        .run(Action::Material(MatCmd::Adjust))
        .at(&[T_LOOK])
        .tip("Give the selected faces, features or bodies an appearance or a colour")
        .spec("2.15-materials.md"),
    c("tools.clear", "Clear", "Премахване на вида", Icon::ClearLook)
        .run(Action::Material(MatCmd::Clear))
        .inv("Clear Appearance Overrides")
        .at(&[T_LOOK])
        .tip("Remove appearance overrides from faces, features, bodies")
        .spec("2.15-materials.md"),
    c("tools.parameters", "Parameters", "Параметри", Icon::Parameters)
        .run(Action::Parameters)
        .at(&[T_PARAMS])
        .tip("Parameters (fx): names, equations, tolerances, user parameters")
        .spec("0.5-parameters.md"),
    c("tools.rebuild_all", "Rebuild All", "Пълна регенерация", Icon::Rebuild)
        .run(Action::RebuildAll)
        .at(&[T_UPDATE])
        .tip("Recompute every feature of the part from scratch (also clears the build cache)")
        .spec("0-ui.md"),
    c("tools.rules", "Rules", "Правила (iLogic)", Icon::Rules)
        .at(&[T_AUTO])
        .todo("7.5"),
    c("tools.assistant", "AI Assistant", "AI асистент", Icon::Assistant)
        .at(&[T_AUTO])
        .todo("7.6"),
    c("tools.undo", "Undo", "Отмяна", Icon::Undo)
        .run(Action::Undo)
        .key("Ctrl+Z")
        .bound()
        .at(&[T_HISTORY])
        .tip("Undo the last action")
        .spec("0-ui.md"),
    c("tools.redo", "Redo", "Повторение (връщане на отменено)", Icon::Redo)
        .run(Action::Redo)
        .key("Ctrl+Y")
        .bound()
        .at(&[T_HISTORY])
        .tip("Redo the last undone action")
        .spec("0-ui.md"),
    // ---- Manage ----------------------------------------------------------
    c("manage.styles_editor", "Styles Editor", "Редактор на стилове", Icon::Styles)
        .run(Action::Material(MatCmd::StylesEditor))
        .inv("Style and Standard Editor")
        .at(&[G_STYLES])
        .tip("Style and Standard Editor: the document's materials and appearances")
        .spec("2.15-materials.md"),
    c("manage.update_styles", "Update Styles", "Обновяване на стиловете", Icon::Rebuild)
        .run(Action::Material(MatCmd::UpdateStyles))
        .short("Update")
        .inv("Update")
        .at(&[G_STYLES])
        .tip("Update the document's styles from the library")
        .spec("2.15-materials.md"),
    c("manage.purge", "Purge Styles", "Изчистване на неизползвани стилове", Icon::Purge)
        .run(Action::Material(MatCmd::Purge))
        .short("Purge")
        .inv("Purge")
        .at(&[G_STYLES])
        .tip("Remove document styles nothing uses")
        .spec("2.15-materials.md"),
    c("manage.save_styles", "Save Styles", "Запис на стиловете в библиотеката", Icon::Styles)
        .short("Save")
        .inv("Save")
        .at(&[G_STYLES])
        .todo("2.15"),
    // ---- View ------------------------------------------------------------
    c("view.home", "Home View", "Начален изглед", Icon::Home)
        .run(Action::View(StandardView::Home))
        .short("Home")
        .key("F6")
        .bound()
        .at(&[V_NAV])
        .tip("Home view: isometric, fit all")
        .spec("0.3-view.md"),
    c("view.look_at", "Look At", "Поглед перпендикулярно (Look At)", Icon::LookAt)
        .run(Action::LookAt)
        .key("PageUp")
        .at(&[V_NAV])
        .tip("Look straight at the edited/selected sketch")
        .spec("0.3-view.md"),
    c("view.front", "Front View", "Изглед отпред", Icon::ViewFront)
        .run(Action::View(StandardView::Front))
        .short("Front")
        .at(&[V_NAV])
        .tip("Look from −Y")
        .spec("0.3-view.md"),
    c("view.top", "Top View", "Изглед отгоре", Icon::ViewTop)
        .run(Action::View(StandardView::Top))
        .short("Top")
        .at(&[V_NAV])
        .tip("Look from +Z")
        .spec("0.3-view.md"),
    c("view.right", "Right View", "Изглед отдясно", Icon::ViewRight)
        .run(Action::View(StandardView::Right))
        .short("Right")
        .at(&[V_NAV])
        .tip("Look from +X")
        .spec("0.3-view.md"),
    c("view.visual_style", "Visual Style", "Визуален стил", Icon::VisualStyle)
        .at(&[V_LOOK])
        .todo("0.3"),
    c("view.section", "Section View", "Изглед в разрез", Icon::Section)
        .inv("Half Section View")
        .at(&[V_LOOK])
        .todo("0.3"),
    c("view.orthographic", "Orthographic", "Ортогонална проекция", Icon::Ortho)
        .run(Action::Projection(Projection::Orthographic))
        .at(&[V_LOOK])
        .tip("Parallel projection")
        .spec("0.3-view.md"),
    c("view.perspective", "Perspective", "Перспектива", Icon::Ortho)
        .run(Action::Projection(Projection::Perspective))
        .at(&[V_LOOK])
        .tip("Perspective projection")
        .spec("0.3-view.md"),
    c("view.perspective_ortho", "Perspective with Ortho Faces", "Перспектива с ортогонални стени", Icon::Ortho)
        .run(Action::Projection(Projection::PerspectiveOrthoFaces))
        .at(&[V_LOOK])
        .tip("Perspective, but orthographic while looking straight at a ViewCube face")
        .spec("0.3-view.md"),
    c("view.vis_origin_planes", "Origin Planes", "Основни равнини (видимост)", Icon::WorkPlane)
        .run(Action::Ui(UiCmd::ObjVis(VisKind::OriginPlanes)))
        .at(&[V_VIS])
        .tip("Show or hide the origin planes XY, XZ, YZ (Object Visibility)")
        .spec("2.10-work-features.md"),
    c("view.vis_work_planes", "User Work Planes", "Работни равнини (видимост)", Icon::WorkPlane)
        .run(Action::Ui(UiCmd::ObjVis(VisKind::WorkPlanes)))
        .at(&[V_VIS])
        .tip("Show or hide every user work plane (a plane hidden on its own stays hidden)")
        .spec("2.10-work-features.md"),
    c("view.vis_work_axes", "User Work Axes", "Работни оси (видимост)", Icon::WorkAxis)
        .run(Action::Ui(UiCmd::ObjVis(VisKind::WorkAxes)))
        .at(&[V_VIS])
        .tip("Show or hide every user work axis")
        .spec("2.10-work-features.md"),
    c("view.vis_work_points", "User Work Points", "Работни точки (видимост)", Icon::WorkPoint)
        .run(Action::Ui(UiCmd::ObjVis(VisKind::WorkPoints)))
        .at(&[V_VIS])
        .tip("Show or hide every user work point")
        .spec("2.10-work-features.md"),
    c("view.vis_ucs", "UCS Triads", "UCS (видимост)", Icon::Ucs)
        .run(Action::Ui(UiCmd::ObjVis(VisKind::Ucs)))
        .at(&[V_VIS])
        .tip("Show or hide the UCS triads and their planes")
        .spec("2.10-work-features.md"),
    c("view.browser", "Browser", "Дърво на модела (показване)", Icon::UiPanels)
        .run(Action::Ui(UiCmd::Toggle(Element::Browser)))
        .at(&[V_WIN, VIEW])
        .tip("Show or hide the model browser")
        .spec("0.13-ui-layout.md"),
    c("view.properties", "Properties", "Панел Properties (показване)", Icon::UiPanels)
        .run(Action::Ui(UiCmd::Toggle(Element::Properties)))
        .at(&[V_WIN, VIEW])
        .tip("Show or hide the Properties panel")
        .spec("0.13-ui-layout.md"),
    c("view.ribbon", "Ribbon", "Лента с команди (показване)", Icon::UiPanels)
        .run(Action::Ui(UiCmd::Toggle(Element::Ribbon)))
        .at(&[V_WIN, VIEW])
        .tip("Show or hide the ribbon")
        .spec("0.13-ui-layout.md"),
    c("view.status_bar", "Status Bar", "Лента на състоянието (показване)", Icon::UiPanels)
        .run(Action::Ui(UiCmd::Toggle(Element::StatusBar)))
        .at(&[V_WIN, VIEW])
        .tip("Show or hide the status bar")
        .spec("0.13-ui-layout.md"),
    c("view.view_cube", "ViewCube", "Куб на изгледите (показване)", Icon::UiPanels)
        .run(Action::Ui(UiCmd::Toggle(Element::ViewCube)))
        .at(&[V_WIN, VIEW])
        .tip("Show or hide the ViewCube")
        .spec("0.13-ui-layout.md"),
    c("view.triad", "Axis Triad", "Триада на осите (показване)", Icon::UiPanels)
        .run(Action::Ui(UiCmd::Toggle(Element::Triad)))
        .at(&[V_WIN, VIEW])
        .tip("Show or hide the axis triad")
        .spec("0.13-ui-layout.md"),
    c("view.clean_screen", "Clean Screen", "Чист екран", Icon::CleanScreen)
        .run(Action::Ui(UiCmd::CleanScreen))
        .key("Ctrl+0")
        .bound()
        .at(&[V_WIN, VIEW])
        .tip("Only the graphics window (and the Quick Access bar); again to restore")
        .spec("0.13-ui-layout.md"),
    c("view.full_screen", "Full Screen", "Цял екран", Icon::FullScreen)
        .run(Action::Ui(UiCmd::FullScreen))
        .key("F11")
        .bound()
        .at(&[V_WIN, VIEW])
        .env(PART)
        .tip("The application fills the screen (outside a sketch; there F11 is Relax Mode)")
        .spec("0.13-ui-layout.md"),
    c("view.reset_layout", "Reset UI Layout", "Начално разположение на панелите", Icon::ResetLayout)
        .run(Action::Ui(UiCmd::ResetLayout))
        .short("Reset")
        .at(&[V_WIN, VIEW])
        .tip("Reset UI Layout: panels, sizes and window positions back to the defaults")
        .spec("0.13-ui-layout.md"),
    c("view.minimize_ribbon", "Minimize Ribbon", "Свиване на лентата", Icon::UiPanels)
        .run(Action::Ui(UiCmd::MinimizeRibbon))
        .inv("Minimize to Tabs")
        .at(&[VIEW])
        .tip("Tabs only; a click on a tab shows its panels (double-click the active tab)")
        .spec("0.13-ui-layout.md"),
    c("view.zoom_all", "Zoom All", "Побиране на всичко", Icon::Home)
        .run(Action::App(AppCmd::ZoomAll))
        .key("Home")
        .at(&[KEY])
        .tip("Fit the whole model into the view (double middle click)")
        .spec("0.3-view.md"),
    c("view.previous", "Previous View", "Предишен изглед", Icon::Home)
        .key("F5")
        .at(&[KEY])
        .todo("0.3"),
    c("view.next", "Next View", "Следващ изглед", Icon::Home)
        .key("Shift+F5")
        .at(&[KEY])
        .todo("0.3"),
    c("view.zoom_window", "Zoom Window", "Мащаб с прозорец", Icon::Home)
        .key("Shift+F3")
        .at(&[KEY])
        .todo("0.3"),
    c("view.pan", "Pan", "Панорама", Icon::Move)
        .key("F2")
        .bound()
        .held()
        .at(&[KEY])
        .tip("Drag to pan (also the middle mouse button)")
        .spec("0.3-view.md"),
    c("view.zoom", "Zoom", "Мащабиране на изгледа", Icon::Scale)
        .key("F3")
        .bound()
        .held()
        .at(&[KEY])
        .tip("Drag to zoom (also the wheel)")
        .spec("0.3-view.md"),
    c("view.orbit", "Orbit", "Въртене на изгледа", Icon::Rotate)
        .key("F4")
        .bound()
        .held()
        .at(&[KEY])
        .tip("Drag to orbit (also Shift + middle mouse button)")
        .spec("0.3-view.md"),
    c("sketch.slice", "Slice Graphics", "Срез на графиката", Icon::Section)
        .run(Action::App(AppCmd::SliceGraphics))
        .key("F7")
        .bound()
        .at(&[KEY])
        .env(SKETCH)
        .tip("Hide the model in front of the sketch plane")
        .spec("1.1-sketch.md"),
    c("edit.select_all", "Select All", "Избор на всичко", Icon::Select)
        .run(Action::App(AppCmd::SelectAll))
        .key("Ctrl+A")
        .bound()
        .at(&[KEY])
        .tip("Select everything of the selection mode (in a sketch: all curves)")
        .spec("0.9-selection.md"),
    c("sketch.delete", "Delete", "Изтриване", Icon::ClearLook)
        .run(Action::App(AppCmd::Delete))
        .key("Delete")
        .bound()
        .at(&[KEY])
        .env(SKETCH)
        .tip("Delete the selected sketch geometry and constraints")
        .spec("1.1-sketch.md"),
    // ---- Sheet Metal (docs/spec/5.6-sheet-metal.md) ----------------------
    c("sm.convert", "Convert to Sheet Metal", "Превръщане в листов материал", Icon::SheetMetal)
        .short("Convert")
        .run(Action::SmConvert)
        .at(&[Place::Ribbon("3D Model", "Convert"), Place::Ribbon("Sheet Metal", "Setup")])
        .env(PART)
        .tip("A solid to sheet metal: pick the face that stays put and the edges that become bends (a shelled solid keeps its walls); a part without solids gets the sheet metal rule and tab")
        .spec("5.6-sheet-metal.md"),
    c("sm.defaults", "Sheet Metal Defaults", "Настройки на листовия материал", Icon::Settings)
        .short("Defaults")
        .run(Action::SmDefaults)
        .at(&[Place::Ribbon("Sheet Metal", "Setup")])
        .env(PART)
        .tip("The sheet metal rule of the part: thickness, bend radius, K-factor, reliefs, gap")
        .spec("5.6-sheet-metal.md"),
    c("sm.face", "Face", "Плоча", Icon::SheetMetal)
        .run(Action::SmFace)
        .at(&[Place::Ribbon("Sheet Metal", "Create")])
        .env(PART)
        .tip("A sheet from a closed sketch profile, the rule's thickness")
        .spec("5.6-sheet-metal.md"),
    c("sm.flange", "Flange", "Фланец", Icon::SheetMetal)
        .run(Action::SmFlange)
        .at(&[Place::Ribbon("Sheet Metal", "Create")])
        .env(PART)
        .tip("Bend flanges on sheet edges: height, angle, bend radius, Bend Position")
        .spec("5.6-sheet-metal.md"),
    c("sm.flat_pattern", "Flat Pattern", "Развивка", Icon::SheetMetal)
        .run(Action::SmFlatPattern)
        .at(&[Place::Ribbon("Sheet Metal", "Flat Pattern")])
        .env(PART)
        .tip("Show the part unfolded (its extents and bends in Properties); again: back to the folded part")
        .spec("5.6-sheet-metal.md"),
    c("sm.contour_flange", "Contour Flange", "Контурен фланец", Icon::SheetMetal)
        .at(&[Place::Ribbon("Sheet Metal", "Create")]).env(PART)
        .tip("A bent strip from an open sketch chain").todo("5.6"),
    c("sm.hem", "Hem", "Подгъв", Icon::SheetMetal)
        .at(&[Place::Ribbon("Sheet Metal", "Create")]).env(PART)
        .tip("Single, teardrop, rolled or double hem on an edge").todo("5.6"),
    c("sm.fold", "Fold", "Сгъване", Icon::SheetMetal)
        .at(&[Place::Ribbon("Sheet Metal", "Create")]).env(PART)
        .tip("Fold a face along a sketch line").todo("5.6"),
    c("sm.bend", "Bend", "Огъвка", Icon::SheetMetal)
        .at(&[Place::Ribbon("Sheet Metal", "Create")]).env(PART)
        .tip("A bend joining two faces").todo("5.6"),
    c("sm.contour_roll", "Contour Roll", "Валцоване", Icon::SheetMetal)
        .at(&[Place::Ribbon("Sheet Metal", "Create")]).env(PART)
        .tip("A rolled sheet from an open profile").todo("5.6"),
    c("sm.lofted_flange", "Lofted Flange", "Преходен фланец", Icon::SheetMetal)
        .at(&[Place::Ribbon("Sheet Metal", "Create")]).env(PART)
        .tip("A transition between two profiles (square to round)").todo("5.6"),
    c("sm.rip", "Rip", "Разрез", Icon::SheetMetal)
        .at(&[Place::Ribbon("Sheet Metal", "Create")]).env(PART)
        .tip("Split a closed shape so it unfolds").todo("5.6"),
    c("sm.cut", "Cut", "Изрязване", Icon::SheetMetal)
        .at(&[Place::Ribbon("Sheet Metal", "Modify")]).env(PART)
        .tip("A sketch cut through the sheet, also across bends").todo("5.6"),
    c("sm.corner_seam", "Corner Seam", "Ъглов шев", Icon::SheetMetal)
        .at(&[Place::Ribbon("Sheet Metal", "Modify")]).env(PART)
        .tip("Close or open the corner between two flanges").todo("5.6"),
    c("sm.punch", "Punch Tool", "Щанца", Icon::SheetMetal)
        .at(&[Place::Ribbon("Sheet Metal", "Modify")]).env(PART)
        .tip("Louvres, lances, embosses from the punch library").todo("5.6"),
    c("sm.unfold", "Unfold", "Разгъване", Icon::SheetMetal)
        .at(&[Place::Ribbon("Sheet Metal", "Modify")]).env(PART)
        .tip("Flatten chosen bends to add features in the flat").todo("5.6"),
    c("sm.refold", "Refold", "Обратно сгъване", Icon::SheetMetal)
        .at(&[Place::Ribbon("Sheet Metal", "Modify")]).env(PART)
        .tip("Fold the unfolded bends again").todo("5.6"),
    c("sm.export_flat", "Export Flat Pattern", "Експорт на развивката", Icon::SheetMetal)
        .short("Export")
        .at(&[Place::Ribbon("Sheet Metal", "Flat Pattern")]).env(PART)
        .tip("The flat pattern as DXF with layers (stage S3)").todo("5.6"),
    // ---- Assemble (docs/spec/3.1-assemblies.md) ---------------------------
    c("asm.place", "Place", "Поставяне на компонент", Icon::Place)
        .run(Action::AsmPlace)
        .inv("Place Component")
        .alias(&["P"])
        .bound()
        .at(&[A_COMPONENT])
        .env(ASSEMBLY)
        .tip("Place a part as a new occurrence: click in the view for each one, right-click for OK, Place Grounded at Origin or a 90° turn")
        .spec("3.1-assemblies.md"),
    c("asm.create", "Create", "Създаване на компонент", Icon::Part)
        .at(&[A_COMPONENT])
        .env(ASSEMBLY)
        .tip("Create a new part inside the assembly")
        .todo("3.10"),
    c("asm.free_move", "Free Move", "Свободно преместване", Icon::FreeMove)
        .run(Action::AsmFreeMove)
        .alias(&["V"])
        .bound()
        .at(&[A_POSITION])
        .env(ASSEMBLY)
        .tip("Drag a component in the view plane (grounded ones too); without the command a drag moves components that are not grounded")
        .spec("3.1-assemblies.md"),
    c("asm.free_rotate", "Free Rotate", "Свободно завъртане", Icon::FreeRotate)
        .run(Action::AsmFreeRotate)
        .alias(&["G"])
        .bound()
        .at(&[A_POSITION])
        .env(ASSEMBLY)
        .tip("Click a component, then drag inside the circle to turn it freely or on its rim to turn it in the screen plane")
        .spec("3.1-assemblies.md"),
    c("asm.ground", "Grounded", "Фиксиран", Icon::Ground)
        .run(Action::AsmGround)
        .at(&[A_POSITION])
        .env(ASSEMBLY)
        .tip("Ground the selected components (they keep their place) or free them again")
        .spec("3.1-assemblies.md"),
    c("asm.constrain", "Constrain", "Ограничение", Icon::Coincident)
        .run(Action::AsmConstrain)
        .inv("Place Constraint")
        .alias(&["C"])
        .bound()
        .at(&[A_RELATIONSHIPS])
        .env(ASSEMBLY)
        .tip("Mate (or Flush), Angle or Insert between faces or edges of two components; Tangent and Symmetry are planned")
        .spec("3.1-assemblies.md"),
    c("asm.joint", "Joint", "Съединение", Icon::Rotate)
        .run(Action::AsmJoint)
        .alias(&["J"])
        .bound()
        .at(&[A_RELATIONSHIPS])
        .env(ASSEMBLY)
        .tip("Rigid, Rotational, Slider, Cylindrical, Planar or Ball joint between origins of two components (face or circle centres, axes, points)")
        .spec("3.1-assemblies.md"),
    c("asm.pattern", "Pattern", "Масив от компоненти", Icon::PatternRect)
        .at(&[A_PATTERN])
        .env(ASSEMBLY)
        .tip("Pattern components")
        .todo("3.7"),
    c("asm.mirror", "Mirror", "Огледало на компоненти", Icon::Mirror)
        .at(&[A_PATTERN])
        .env(ASSEMBLY)
        .tip("Mirror components about a plane")
        .todo("3.7"),
    c("asm.copy", "Copy", "Копиране на компоненти", Icon::Copy)
        .at(&[A_PATTERN])
        .env(ASSEMBLY)
        .tip("Copy components as new documents")
        .todo("3.8"),
    c("asm.update", "Update", "Обновяване", Icon::Rebuild)
        .run(Action::AsmUpdate)
        .at(&[Place::Ribbon("Assemble", "Manage")])
        .env(ASSEMBLY)
        .tip("Read the linked part files again (changes saved in them come in) and solve the constraints")
        .spec("3.1-assemblies.md"),
    c("asm.bom", "Bill of Materials", "Спецификация", Icon::Parameters)
        .short("BOM")
        .run(Action::AsmBom)
        .at(&[Place::Ribbon("Assemble", "Manage")])
        .env(ASSEMBLY)
        .tip("The parts of the assembly with their quantities and iProperties: Model Data, Structured, Parts Only; Export to CSV")
        .spec("3.6-bom.md"),
    // ---- File / Help -----------------------------------------------------
    c("file.new_assembly", "New Assembly", "Ново сглобяване", Icon::Assembly)
        .run(Action::App(AppCmd::NewAssembly))
        .at(&[FILE])
        .tip("A new assembly in the open project; place the project's parts in it")
        .spec("3.1-assemblies.md"),
    c("file.new", "New", "Нов проект", Icon::Part)
        .run(Action::App(AppCmd::New))
        .key("Ctrl+N")
        .bound()
        .at(&[FILE])
        .tip("A new project with an empty part")
        .spec("0.6-files.md"),
    c("file.open", "Open", "Отваряне", Icon::Part)
        .run(Action::App(AppCmd::Open))
        .key("Ctrl+O")
        .bound()
        .at(&[FILE])
        .tip("Open a .3dcadcraft project")
        .spec("0.6-files.md"),
    c("file.save", "Save", "Запис", Icon::Part)
        .run(Action::App(AppCmd::Save))
        .key("Ctrl+S")
        .bound()
        .at(&[FILE])
        .tip("Save the project")
        .spec("0.6-files.md"),
    c("file.save_as", "Save As", "Запис като", Icon::Part)
        .run(Action::App(AppCmd::SaveAs))
        .key("Ctrl+Shift+S")
        .bound()
        .at(&[FILE])
        .tip("Save the project under another name")
        .spec("0.6-files.md"),
    c("file.save_folder", "Save as Folder", "Запис като папка", Icon::Part)
        .run(Action::App(AppCmd::SaveAsFolder))
        .at(&[FILE])
        .tip("Unpacked directory, git-friendly")
        .spec("0.6-files.md"),
    c("file.export_stl", "Export STL", "Износ в STL", Icon::Part)
        .run(Action::App(AppCmd::Export(ExportFormat::Stl)))
        .at(&[FILE])
        .tip("Triangles in mm, for 3D printing")
        .spec("7.7-export.md"),
    c("file.export_step", "Export STEP", "Износ в STEP", Icon::Part)
        .run(Action::App(AppCmd::Export(ExportFormat::Step)))
        .at(&[FILE])
        .tip("Exact B-rep for other CAD programs")
        .spec("7.7-export.md"),
    c("file.export_glb", "Export GLB", "Износ в GLB (glTF)", Icon::Part)
        .run(Action::App(AppCmd::Export(ExportFormat::Glb)))
        .at(&[FILE])
        .tip("Binary glTF for viewers and the web")
        .spec("7.7-export.md"),
    c("file.print", "Print", "Печат", Icon::Part)
        .key("Ctrl+P")
        .at(&[FILE])
        .todo("4.5"),
    c("help.help", "Help", "Помощ", Icon::Options)
        .run(Action::App(AppCmd::Help))
        .key("F1")
        .bound()
        .at(&[HELP])
        .tip("The specification of the current command")
        .spec("0-ui.md"),
    c("help.save_diagnostics", "Save Diagnostics", "Запис на диагностика", Icon::Options)
        .run(Action::App(AppCmd::SaveDiagnostics))
        .at(&[HELP])
        .tip("Zip the session log, the action journal, crash reports, the open project and settings.json (to send with a problem report)")
        .spec("0.14-diagnostics.md"),
    c("help.logs_folder", "Open Logs Folder", "Папка с дневниците", Icon::Options)
        .run(Action::App(AppCmd::OpenLogsFolder))
        .at(&[HELP])
        .tip("The folder with the session logs and crash reports")
        .spec("0.14-diagnostics.md"),
];

/// The command with this id.
pub fn by_id(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// The first command that runs `a` (the canonical one when several
/// buttons share an action, e.g. Application Options / Constraint
/// Settings).
pub fn by_action(a: Action) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.action == Some(a))
}

/// Length of the recent-command list (spec §5.1 rule 4).
pub const RECENT_MAX: usize = 10;

/// Puts the command of `a` first in `recent` (ids, newest first, at most
/// [`RECENT_MAX`]). Undo/Redo and Select are not "commands" there.
pub fn remember(recent: &mut Vec<String>, a: Action) {
    if matches!(a, Action::Undo | Action::Redo | Action::Tool(Tool::Select)) {
        return;
    }
    let Some(c) = by_action(a) else { return };
    recent.retain(|x| x != c.id);
    recent.insert(0, c.id.to_owned());
    recent.truncate(RECENT_MAX);
}

/// One search / alias-input result.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    /// The command.
    pub cmd: &'static Command,
    /// How it matched.
    pub tier: Tier,
    /// Score inside the tier (higher first).
    pub score: i32,
    /// Valid in the environment searched from.
    pub valid: bool,
    /// Implemented (not a planned or held-key command).
    pub ready: bool,
}

impl Hit {
    /// Can run now (valid here and implemented).
    pub fn runnable(&self) -> bool {
        self.valid && self.ready
    }
}

fn index() -> &'static [Folded] {
    static INDEX: OnceLock<Vec<Folded>> = OnceLock::new();
    INDEX.get_or_init(|| COMMANDS.iter().map(fold_command).collect())
}

/// A command's texts for the matcher.
fn fold_command(c: &Command) -> Folded {
    let mut names = vec![c.name, c.bg];
    names.extend(c.inventor);
    names.extend(c.label);
    let mut aliases = c.aliases.to_vec();
    aliases.extend(c.key);
    Folded::new(&Fields {
        aliases: &aliases,
        names: &names,
        tip: c.tip,
    })
}

/// Commands matching `query` in `env`, best first: runnable commands by
/// tier (exact alias or name → recent → alias prefix → word prefix →
/// fuzzy) and score, then the ones of another environment, then the
/// unimplemented ones (spec §5.1 rule 4, §5.3). An empty query lists the
/// recent commands. `recent` holds ids, newest first.
pub fn search(query: &str, env: Env, recent: &[String]) -> Vec<Hit> {
    let q = Query::new(query);
    let mut hits: Vec<Hit> = COMMANDS
        .iter()
        .zip(index())
        .filter_map(|(cmd, folded)| {
            let r = recent.iter().position(|id| id == cmd.id);
            let (tier, score) = matcher::rank(&q, folded, r)?;
            Some(Hit {
                cmd,
                tier,
                score,
                valid: cmd.valid_in(env),
                ready: cmd.ready() && !cmd.held,
            })
        })
        .collect();
    let group = |h: &Hit| match (h.ready, h.valid) {
        (true, true) => 0,
        (true, false) => 1,
        _ => 2,
    };
    hits.sort_by(|a, b| {
        (group(a), a.tier, -a.score, a.cmd.name).cmp(&(group(b), b.tier, -b.score, b.cmd.name))
    });
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique_and_stable_looking() {
        let mut seen = HashSet::new();
        for c in COMMANDS {
            assert!(seen.insert(c.id), "duplicate id {}", c.id);
            assert!(
                c.id.contains('.')
                    && c.id.chars().all(|ch| ch.is_ascii_lowercase()
                        || ch.is_ascii_digit()
                        || ch == '.'
                        || ch == '_'),
                "id {} should be `area.name`",
                c.id
            );
            assert!(
                !c.name.is_empty() && !c.bg.is_empty(),
                "{} needs names",
                c.id
            );
            assert!(!c.places.is_empty(), "{} needs a place", c.id);
        }
    }

    #[test]
    fn aliases_are_unique_per_environment() {
        for env in Env::ALL {
            let mut seen = std::collections::HashMap::new();
            for c in COMMANDS.iter().filter(|c| c.valid_in(env)) {
                for a in c.aliases {
                    let key = a.to_ascii_uppercase();
                    if let Some(other) = seen.insert(key, c.id) {
                        panic!("alias {a} of {} and {other} in {env:?}", c.id);
                    }
                }
            }
        }
    }

    #[test]
    fn implemented_entries_have_an_action() {
        for c in COMMANDS {
            if c.planned.is_some() {
                assert!(c.action.is_none(), "{} is planned but runs something", c.id);
            } else if !c.held {
                assert!(
                    c.action.is_some(),
                    "{} is implemented but has no Action",
                    c.id
                );
                assert!(!c.tip.is_empty(), "{} needs a tooltip", c.id);
            }
        }
    }

    #[test]
    fn spec_files_and_feature_sections_exist() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs");
        let features = std::fs::read_to_string(root.join("FEATURES.md")).unwrap_or_default();
        for c in COMMANDS {
            if let Some(s) = c.spec {
                assert!(
                    root.join("spec").join(s).is_file(),
                    "{}: no docs/spec/{s}",
                    c.id
                );
            }
            if let Some(sec) = c.planned {
                assert!(
                    features.contains(&format!("## {sec} ")),
                    "{}: no section {sec} in FEATURES.md",
                    c.id
                );
            }
        }
    }

    /// The Repeat label and the old `command_label` agree for every sketch
    /// tool (names come from the registry now).
    #[test]
    fn tool_names_are_the_tool_labels() {
        for c in COMMANDS {
            if let Some(Action::Tool(t)) = c.action {
                assert_eq!(c.name, t.label(), "{}", c.id);
                assert_eq!(by_action(Action::Tool(t)).map(|x| x.id), Some(c.id));
            }
        }
        for (a, name) in [
            (Action::Draft, "Face Draft"),
            (Action::WorkPlane, "Work Plane"),
            (Action::RectPattern, "Rectangular Pattern"),
            (Action::Primitive(Prim::Torus), "Torus"),
            (Action::StartSketch, "Start 2D Sketch"),
            (Action::Options, "Application Options"),
        ] {
            assert_eq!(by_action(a).map(|c| c.name), Some(name));
        }
    }

    /// Every ribbon button has an entry at that place, and every entry with
    /// a ribbon place is on the ribbon.
    #[test]
    fn ribbon_and_registry_agree() {
        let buttons = super::super::ribbon::all_buttons();
        let mut on_ribbon = HashSet::new();
        for (tab, panel, id) in &buttons {
            let Some(c) = by_id(id) else {
                panic!("ribbon button {id} ({tab} › {panel}) is not in the registry");
            };
            assert!(
                c.places.contains(&Place::Ribbon(tab, panel)),
                "{id} is on the ribbon at {tab} › {panel}, the registry says {:?}",
                c.places
            );
            on_ribbon.insert((*tab, *panel, *id));
        }
        for c in COMMANDS {
            for p in c.places {
                if let Place::Ribbon(tab, panel) = *p {
                    assert!(
                        on_ribbon.contains(&(tab, panel, c.id)),
                        "{} says {tab} › {panel} but is not on the ribbon there",
                        c.id
                    );
                }
            }
        }
    }

    fn first(q: &str, env: Env, recent: &[&str]) -> &'static str {
        let recent: Vec<String> = recent.iter().map(|s| (*s).to_owned()).collect();
        search(q, env, &recent).first().map_or("-", |h| h.cmd.id)
    }

    fn ids(q: &str, env: Env) -> Vec<&'static str> {
        search(q, env, &[]).iter().map(|h| h.cmd.id).collect()
    }

    #[test]
    fn matcher_order() {
        use Env::{Part, Sketch};
        // Exact alias.
        assert_eq!(first("E", Part, &[]), "part.extrude");
        assert_eq!(
            first("e", Sketch, &[]),
            "part.extrude",
            "E leaves the sketch"
        );
        assert_eq!(first("R", Part, &[]), "part.revolve");
        assert_eq!(first("R", Sketch, &[]), "sketch.rect_two");
        assert_eq!(first("d", Sketch, &[]), "sketch.dimension");
        assert_eq!(first("d", Part, &[]), "part.draft");
        assert_eq!(first("SW", Part, &[]), "part.sweep");
        // S: the exact alias first, then the longer ones (S3 is planned: last).
        let s = ids("s", Part);
        assert_eq!(s[0], "part.start_sketch");
        let pos = |id: &str| s.iter().position(|x| *x == id).unwrap_or(usize::MAX);
        assert!(pos("part.shell") < pos("part.start_3d_sketch"));
        assert!(pos("part.sweep") < pos("part.start_3d_sketch"));
        // Recent before alias prefix and word prefix.
        assert_eq!(first("s", Part, &["part.shell"]), "part.start_sketch");
        assert_eq!(first("sh", Part, &[]), "part.shell");
        assert_eq!(first("e", Sketch, &["sketch.extend"]), "part.extrude");
        let ext = ids("ext", Sketch);
        assert_eq!(&ext[..2], &["sketch.extend", "part.extrude"], "{ext:?}");
        let recent = search("ext", Sketch, &["part.extrude".to_owned()]);
        assert_eq!(recent[0].cmd.id, "part.extrude");
        assert_eq!(recent[0].tier, Tier::Recent);
        // Word prefix ("point" is the second word of many tools).
        assert_eq!(first("two point c", Sketch, &[]), "sketch.rect_center");
        // Inventor's name.
        assert_eq!(first("general dim", Sketch, &[]), "sketch.dimension");
        assert_eq!(first("sketch driven", Sketch, &[]), "sketch.point_pattern");
        // Bulgarian, in Cyrillic and in Latin letters.
        assert_eq!(first("изтегляне", Part, &[]), "part.extrude");
        assert_eq!(first("отв", Part, &[]), "part.hole");
        assert_eq!(first("zakr", Part, &[]), "part.fillet");
        assert_eq!(first("ЧЕРУП", Part, &[]), "part.shell");
        assert_eq!(first("масив по", Sketch, &[]), "sketch.path_pattern");
        // Wildcards.
        assert_eq!(first("*slot", Sketch, &[]), "sketch.slot_arc_center");
        assert!(ids("*slot", Sketch).len() >= 6);
        assert_eq!(first("lo?t", Part, &[]), "part.loft");
        // Fuzzy: letters in order, and the tooltip.
        assert_eq!(first("rbldall", Part, &[]), "tools.rebuild_all");
        assert_eq!(first("hollow", Part, &[]), "part.shell");
        // Another environment: marked invalid, after the valid ones.
        let h = search("H", Sketch, &[]);
        let hole = h.iter().find(|x| x.cmd.id == "part.hole");
        assert!(hole.is_some_and(|x| !x.valid && x.tier == Tier::Exact));
        assert!(h[0].valid, "valid commands first");
        // Unimplemented: flagged and last.
        let t = search("text", Sketch, &[]);
        let text = t.iter().find(|x| x.cmd.id == "sketch.text");
        assert!(text.is_some_and(|x| !x.ready && !x.runnable()));
        assert!(t.last().is_some_and(|x| !x.ready));
        // Empty: the recent commands in order.
        let r = search(
            "",
            Part,
            &["part.hole".to_owned(), "part.fillet".to_owned()],
        );
        assert_eq!(
            r.iter().map(|h| h.cmd.id).collect::<Vec<_>>(),
            ["part.hole", "part.fillet"]
        );
        assert!(search("qqq", Part, &[]).is_empty());
    }

    #[test]
    fn remember_keeps_ten_newest_first() {
        let mut r = Vec::new();
        remember(&mut r, Action::Extrude);
        remember(&mut r, Action::Hole);
        remember(&mut r, Action::Extrude);
        remember(&mut r, Action::Undo);
        remember(&mut r, Action::Tool(Tool::Select));
        assert_eq!(r, ["part.extrude", "part.hole"]);
        for t in [
            Tool::Line,
            Tool::CircleCenter,
            Tool::ArcThreePoint,
            Tool::RectTwoPoint,
            Tool::Polygon,
            Tool::Point,
            Tool::Trim,
            Tool::Extend,
            Tool::Offset,
            Tool::Move,
        ] {
            remember(&mut r, Action::Tool(t));
        }
        assert_eq!(r.len(), RECENT_MAX);
        assert_eq!(r[0], "sketch.move");
        assert!(!r.contains(&"part.hole".to_owned()));
    }

    #[test]
    fn recent_commands_survive_settings_save_and_load() {
        use super::super::layout::{AppOptions, UiSettings, load_from, write_to};
        let dir = match tempfile::tempdir() {
            Ok(d) => d,
            Err(e) => panic!("tempdir: {e}"),
        };
        let p = dir.path().join("settings.json");
        let mut o = AppOptions::default();
        remember(&mut o.recent_commands, Action::Sweep);
        remember(&mut o.recent_commands, Action::Tool(Tool::Line));
        assert!(write_to(&p, &UiSettings::default(), &o, false).is_ok());
        let l = load_from(&p);
        assert_eq!(l.options.recent_commands, ["sketch.line", "part.sweep"]);
        assert!(l.warning.is_none());
    }

    /// The search runs per keystroke: 300 commands (the table, repeated)
    /// well under 1 ms (`tests/perf.rs` has the budget case).
    #[test]
    fn search_is_fast() {
        let _ = index();
        let t0 = std::time::Instant::now();
        let mut n = 0;
        for q in ["e", "ex", "ext", "extr", "изт", "rect*", "two point"] {
            n += search(q, Env::Sketch, &[]).len();
        }
        let per = t0.elapsed().as_secs_f64() * 1000.0 / 7.0;
        assert!(n > 0);
        // Loose bound for loaded debug builds; the real budget is in perf.rs.
        assert!(per < 20.0, "{per} ms per keystroke");
    }
}
