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
use super::documents::DocCmd;
use super::drawing::DrwCmd;
use super::drawing_planned::Planned as Dp;
use super::icons::Icon;
use super::layout::{Element, UiCmd, VisKind};
use super::material::MatCmd;
use super::ribbon::Action;
use super::sketch_tools::{ConstraintTool as Ct, Tool};
use super::sm_planned::PlannedKind;
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
    /// A drawing is the active document.
    Drawing,
}

impl Env {
    /// Every environment (for tests and the keymap's filter).
    pub const ALL: [Env; 4] = [Env::Part, Env::Sketch, Env::Assembly, Env::Drawing];

    /// English name, as in messages ("not available in a sketch").
    pub fn label(self) -> &'static str {
        match self {
            Env::Part => "a part",
            Env::Sketch => "a sketch",
            Env::Assembly => "an assembly",
            Env::Drawing => "a drawing",
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
    /// Only in a drawing.
    pub const DRAWING: Envs = Envs(8);
    /// The part, its sketches and drawings.
    pub const PART_DRAWING: Envs = Envs(11);
    /// Every document (part, assembly, drawing) outside a sketch: the
    /// commands on the design file's documents (spec 0.6b).
    pub const DOCUMENTS: Envs = Envs(13);

    /// Whether `e` is in the set.
    pub const fn has(self, e: Env) -> bool {
        let bit = match e {
            Env::Part => 1,
            Env::Sketch => 2,
            Env::Assembly => 4,
            Env::Drawing => 8,
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
    /// File → New Drawing: a drawing document in the open project.
    NewDrawing,
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
    /// today — directly (`bound`) or typed into the alias field / command
    /// search (every implemented command).
    pub fn shown_key(&self) -> Option<&'static str> {
        if self.bound || self.ready() {
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
            || (matches!(env, Env::Assembly | Env::Drawing)
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
// Model commands the Sheet Metal tab shows as well (Inventor 2027 has them there).
const SM_SKETCH: Place = Place::Ribbon("Sheet Metal", "Sketch");
const SM_MODIFY: Place = Place::Ribbon("Sheet Metal", "Modify");
const SM_WORK: Place = Place::Ribbon("Sheet Metal", "Work Features");
const SM_PATTERN: Place = Place::Ribbon("Sheet Metal", "Pattern");
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
const D_CREATE: Place = Place::Ribbon("Place Views", "Create");
const D_MODIFY: Place = Place::Ribbon("Place Views", "Modify");
const D_SKETCH: Place = Place::Ribbon("Place Views", "Sketch");
const D_SHEETS: Place = Place::Ribbon("Place Views", "Sheets");
const N_DIM: Place = Place::Ribbon("Annotate", "Dimension");
const N_NOTES: Place = Place::Ribbon("Annotate", "Feature Notes");
const N_TEXT: Place = Place::Ribbon("Annotate", "Text");
const N_SYMBOLS: Place = Place::Ribbon("Annotate", "Symbols");
const N_TABLE: Place = Place::Ribbon("Annotate", "Table");
const N_RETRIEVE: Place = Place::Ribbon("Annotate", "Retrieve");
const N_REVISION: Place = Place::Ribbon("Annotate", "Revision");
const N_FORMAT: Place = Place::Ribbon("Annotate", "Format");
const G_UPDATE: Place = Place::Ribbon("Manage", "Update");
const G_MODIFY: Place = Place::Ribbon("Manage", "Modify");
const G_DEFINE: Place = Place::Ribbon("Manage", "Define");
const G_DOCS: Place = Place::Ribbon("Manage", "Documents");
const G_STORE: Place = Place::Ribbon("Manage", "Storage");
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
const DRAWING: Envs = Envs::DRAWING;

/// A drawing command (`docs/spec/4-drawings.md`); planned ones add their
/// FEATURES section with `.todo`.
const fn d(id: &'static str, name: &'static str, bg: &'static str, icon: Icon) -> Command {
    Command::new(id, name, bg, icon)
        .env(DRAWING)
        .spec("4-drawings.md")
}

/// A drawing command whose dialog exists and whose logic comes later
/// (`gui/drawing_planned.rs`, `docs/spec/4.7-drawing-commands.md`).
const fn dp(id: &'static str, name: &'static str, bg: &'static str, icon: Icon, p: Dp) -> Command {
    Command::new(id, name, bg, icon)
        .env(DRAWING)
        .spec("4.7-drawing-commands.md")
        .run(Action::Drawing(DrwCmd::Planned(p)))
}

const fn c(id: &'static str, name: &'static str, bg: &'static str, icon: Icon) -> Command {
    Command::new(id, name, bg, icon)
}

/// A command on the design file's documents (`docs/spec/0.6b-multi-document-files.md`).
const fn doc(id: &'static str, name: &'static str, bg: &'static str, icon: Icon) -> Command {
    Command::new(id, name, bg, icon)
        .env(Envs::DOCUMENTS)
        .spec("0.6b-multi-document-files.md")
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
        .at(&[M_SKETCH, SM_SKETCH])
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
        .alias(&["CO"])
        .run(Action::Coil)
        .at(&[M_CREATE])
        .env(PART)
        .tip("Coil: screw closed profiles about an axis (springs, threads with Cut)")
        .spec("5.3-coil.md"),
    c("part.rib", "Rib", "Ребро", Icon::Rib)
        .alias(&["RI"])
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
        .at(&[M_MODIFY, SM_MODIFY])
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
        .alias(&["CMB"])
        .run(Action::Combine)
        .at(&[M_MODIFY])
        .env(PART)
        .tip("Combine: join, cut or intersect a base body with toolbodies (multi-body parts)")
        .spec("2.13-combine-split.md"),
    c("part.split", "Split", "Разделяне на тяло", Icon::SplitBody)
        .alias(&["SPL"])
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
        .at(&[M_WORK, SM_WORK])
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
        .at(&[M_WORK, SM_WORK])
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
        .at(&[M_WORK, SM_WORK])
        .env(PART)
        .tip("Work Point: a vertex, sketch point or midpoint, three planes, two \
              lines, a plane or curved face and a line, a circular edge (centre), a \
              sphere or torus (centre); or Grounded at X, Y, Z")
        .spec("2.10-work-features.md"),
    c("part.ucs", "UCS", "Потребителска координатна система", Icon::Ucs)
        .alias(&["UCS"])
        .run(Action::Ucs)
        .at(&[M_WORK, SM_WORK])
        .env(PART)
        .tip("UCS: a coordinate system with its own planes, axes and centre; \
              set X, Y, Z and angles, or pick three points (centre, X, Y)")
        .spec("2.10-work-features.md"),
    c("part.rect_pattern", "Rectangular Pattern", "Правоъгълен масив", Icon::PatternRect)
        .run(Action::RectPattern)
        .short("Rectangular")
        .alias(&["RP"])
        .at(&[M_PATTERN, SM_PATTERN])
        .env(PART)
        .tip("Rectangular Pattern: copy features or solids in rows along one or two directions")
        .spec("2.11-patterns.md"),
    c("part.circ_pattern", "Circular Pattern", "Кръгов масив", Icon::PatternCirc)
        .run(Action::CircPattern)
        .short("Circular")
        .alias(&["CP"])
        .at(&[M_PATTERN, SM_PATTERN])
        .env(PART)
        .tip("Circular Pattern: copy features or solids at equal angles about an axis")
        .spec("2.11-patterns.md"),
    c("part.mirror", "Mirror", "Огледално копие", Icon::Mirror)
        .run(Action::MirrorFeature)
        .alias(&["MI"])
        .at(&[M_PATTERN, SM_PATTERN])
        .env(PART)
        .tip("Mirror: copy features or solids mirrored across a plane")
        .spec("2.12-mirror.md"),
    c("part.box", "Box", "Паралелепипед (кутия)", Icon::Box)
        .alias(&["BX"])
        .run(Action::Primitive(Prim::Box))
        .at(&[M_PRIM])
        .env(PART)
        .tip("Box: pick a plane, click the centre, then a corner (a sketch + Extrude)")
        .spec("2.1-extrude.md"),
    c("part.cylinder", "Cylinder", "Цилиндър", Icon::Cylinder)
        .alias(&["CY"])
        .run(Action::Primitive(Prim::Cylinder))
        .at(&[M_PRIM])
        .env(PART)
        .tip("Cylinder: pick a plane, click the centre, then the diameter (a sketch + Extrude)")
        .spec("2.1-extrude.md"),
    c("part.sphere", "Sphere", "Сфера", Icon::Sphere)
        .alias(&["SPE"])
        .run(Action::Primitive(Prim::Sphere))
        .at(&[M_PRIM])
        .env(PART)
        .tip("Sphere: pick a plane, click the centre, then the diameter (a sketch + full Revolve)")
        .spec("2.2-revolve.md"),
    c("part.torus", "Torus", "Тор", Icon::Torus)
        .alias(&["TOR"])
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
        .alias(&["TC"])
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
        .alias(&["AC"])
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
        .alias(&["RC"])
        .at(&[S_CREATE])
        .tip("Rectangle by its centre and a corner")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.rect_center3", Tool::RectCenterThreePoint, "Three Point Center Rectangle", "Правоъгълник от центъра по три точки", Icon::RectCenter3)
        .alias(&["RCT"])
        .at(&[S_CREATE])
        .tip("Rectangle by its centre, a side midpoint and a corner")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.slot_cc", Tool::SlotCenterToCenter, "Center to Center Slot", "Прорез център до център", Icon::SlotCenter)
        .alias(&["SL"])
        .at(&[S_CREATE])
        .tip("Slot by its two arc centres and the width")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.slot_overall", Tool::SlotOverall, "Overall Slot", "Прорез по обща дължина", Icon::SlotOverall)
        .alias(&["SLO"])
        .at(&[S_CREATE])
        .tip("Slot by its overall length and the width")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.slot_center", Tool::SlotCenterPoint, "Center Point Slot", "Прорез от центъра", Icon::SlotPoint)
        .alias(&["SLC"])
        .at(&[S_CREATE])
        .tip("Slot by its centre, an arc centre and the width")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.slot_arc3", Tool::SlotArcThreePoint, "Three Point Arc Slot", "Дъгов прорез през три точки", Icon::SlotArc)
        .alias(&["SLA"])
        .at(&[S_CREATE])
        .tip("Curved slot along a three point arc")
        .spec("1.2-sketch-geometry.md"),
    t("sketch.slot_arc_center", Tool::SlotArcCenter, "Center Point Arc Slot", "Дъгов прорез по център", Icon::SlotArc)
        .alias(&["SLR"])
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
        .alias(&["SPV"])
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
        .alias(&["CHA"])
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
        .alias(&["CO"])
        .at(&[S_MODIFY])
        .tip("Copy selected geometry (with its constraints)")
        .spec("1.6-modify.md"),
    t("sketch.rotate", Tool::Rotate, "Rotate", "Завъртане", Icon::Rotate)
        .alias(&["RO"])
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
        .alias(&["SPS"])
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
        .alias(&["SC"])
        .at(&[S_MODIFY])
        .tip("Selected geometry about a base point (factor in Properties)")
        .spec("1.6-modify.md"),
    t("sketch.stretch", Tool::Stretch, "Stretch", "Разтягане", Icon::Stretch)
        .alias(&["STR"])
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
        .alias(&["PTH"])
        .at(&[S_PATTERN])
        .tip("Copies of the selected geometry along a line, arc or spline (equal chords)")
        .spec("1.7-patterns.md"),
    t("sketch.point_pattern", Tool::PatternPoints, "Pattern at Points", "Масив по точки", Icon::PatternPoints)
        .alias(&["PPT"])
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
    con("sketch.coincident", Ct::Coincident, "Coincident", "Coincident Constraint", "Съвпадение", Icon::Coincident)
        .alias(&["CN"]),
    con("sketch.collinear", Ct::Collinear, "Collinear", "Collinear Constraint", "Колинеарност", Icon::Collinear)
        .alias(&["CL"]),
    con("sketch.concentric", Ct::Concentric, "Concentric", "Concentric Constraint", "Концентричност", Icon::Concentric)
        .alias(&["CC"]),
    con("sketch.fix", Ct::Fix, "Fix", "Fix Constraint", "Фиксиране", Icon::Fix)
        .alias(&["GND"]),
    con("sketch.parallel", Ct::Parallel, "Parallel", "Parallel Constraint", "Успоредност", Icon::Parallel)
        .alias(&["PA"]),
    con("sketch.perpendicular", Ct::Perpendicular, "Perpendicular", "Perpendicular Constraint", "Перпендикулярност", Icon::Perpendicular)
        .alias(&["PE"]),
    con("sketch.horizontal", Ct::Horizontal, "Horizontal", "Horizontal Constraint", "Хоризонталност", Icon::Horizontal)
        .alias(&["-"]),
    con("sketch.vertical", Ct::Vertical, "Vertical", "Vertical Constraint", "Вертикалност", Icon::Vertical)
        .alias(&["I"]),
    con("sketch.tangent", Ct::Tangent, "Tangent", "Tangent Constraint", "Допирателност", Icon::Tangent)
        .alias(&["TA"]),
    con("sketch.symmetric", Ct::Symmetric, "Symmetric", "Symmetric Constraint", "Симетрия", Icon::Symmetric)
        .alias(&["SY"]),
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
        .alias(&["ECS"])
        .run(Action::EditCoordinateSystem)
        .short("Coordinate System")
        .at(&[S_CONSTRAIN])
        .env(SKETCH)
        .tip("Edit Coordinate System: origin, axis edge and direction (Properties)")
        .spec("1.1-sketch.md"),
    // ---- Sketch: Format, Demo, Exit --------------------------------------
    c("sketch.construction", "Construction", "Конструктивна линия", Icon::Construction)
        .alias(&["CT"])
        .run(Action::Format(Style::Construction))
        .at(&[S_FORMAT])
        .env(SKETCH)
        .tip("Selected geometry → construction; with nothing selected, toggles construction mode")
        .spec("1.2-sketch-geometry.md"),
    c("sketch.centerline", "Centerline", "Осева линия", Icon::Centerline)
        .alias(&["CE"])
        .run(Action::Format(Style::Centerline))
        .at(&[S_FORMAT])
        .env(SKETCH)
        .tip("Selected geometry → centerline")
        .spec("1.2-sketch-geometry.md"),
    c("sketch.normal", "Normal", "Нормална линия", Icon::Normal)
        .alias(&["NO"])
        .run(Action::Format(Style::Normal))
        .at(&[S_FORMAT])
        .env(SKETCH)
        .tip("Selected geometry → normal")
        .spec("1.2-sketch-geometry.md"),
    c("sketch.center_point", "Center Point", "Централна точка", Icon::Point)
        .alias(&["CPT"])
        .run(Action::CenterPoint)
        .at(&[S_FORMAT])
        .env(SKETCH)
        .tip("New points are Center Points (a cross; Hole takes them as centres)")
        .spec("2.3-hole.md"),
    c("demo.rectangle", Demo::Rectangle.label(), "Пример: напълно определен правоъгълник", Icon::Demo)
        .alias(&["QDR"])
        .run(Action::Demo(Demo::Rectangle))
        .at(&[S_DEMO])
        .tip("Demo sketch on the box's top face"),
    c("demo.slot", Demo::Slot.label(), "Пример: прорез", Icon::Demo)
        .alias(&["QDS"])
        .run(Action::Demo(Demo::Slot))
        .at(&[S_DEMO])
        .tip("Demo sketch on the box's top face"),
    c("demo.tangents", Demo::Tangents.label(), "Пример: допирателни окръжности", Icon::Demo)
        .alias(&["QDT"])
        .run(Action::Demo(Demo::Tangents))
        .at(&[S_DEMO])
        .tip("Demo sketch on the box's top face"),
    c("demo.lines", Demo::Lines.label(), "Пример: успоредни и перпендикулярни линии", Icon::Demo)
        .alias(&["QDL"])
        .run(Action::Demo(Demo::Lines))
        .at(&[S_DEMO])
        .tip("Demo sketch on the box's top face"),
    c("demo.revolve", Demo::Revolve.label(), "Пример: половин сечение на вал", Icon::Demo)
        .alias(&["QDV"])
        .run(Action::Demo(Demo::Revolve))
        .at(&[S_DEMO])
        .tip("Demo sketch on the box's top face"),
    c("sketch.finish", "Finish Sketch", "Край на скицата", Icon::Finish)
        .alias(&["ESK"])
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
        .alias(&["PPR"])
        .run(Action::Material(MatCmd::Physical))
        .inv("iProperties: Physical")
        .at(&[I_MEASURE])
        .tip("Mass, area, volume, centre of gravity and inertia (iProperties: Physical)")
        .spec("0.10-physical.md"),
    c("inspect.validate", "Validate Solid", "Проверка на тялото", Icon::Validate)
        .alias(&["IVS"])
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
        .alias(&["AO"])
        .run(Action::Options)
        .at(&[T_OPTIONS])
        .tip("Mouse profile, wheel direction, sketch options")
        .spec("0.13-ui-layout.md"),
    c("sketch.constraint_settings", "Constraint Settings", "Настройки на ограниченията", Icon::Settings)
        .alias(&["CSE"])
        .run(Action::Options)
        .at(&[S_CONSTRAIN])
        .tip("Inference: which constraints are inferred and created while sketching")
        .spec("1.5-inference.md"),
    c("tools.document_settings", "Document Settings", "Настройки на документа", Icon::Settings)
        .at(&[T_OPTIONS])
        .todo("0.11"),
    c("tools.material", "Material", "Материал", Icon::Material)
        .alias(&["MAT"])
        .run(Action::Material(MatCmd::MaterialBrowser))
        .inv("Material Browser")
        .at(&[T_LOOK])
        .tip("Material Browser: document and library materials")
        .spec("2.15-materials.md"),
    c("tools.appearance", "Appearance", "Външен вид", Icon::Appearance)
        .alias(&["APP"])
        .run(Action::Material(MatCmd::AppearanceBrowser))
        .inv("Appearance Browser")
        .at(&[T_LOOK])
        .tip("Appearance Browser: document and library appearances")
        .spec("2.15-materials.md"),
    c("tools.adjust", "Adjust", "Промяна на вида (цвят)", Icon::AdjustLook)
        .alias(&["ADJ"])
        .run(Action::Material(MatCmd::Adjust))
        .at(&[T_LOOK])
        .tip("Give the selected faces, features or bodies an appearance or a colour")
        .spec("2.15-materials.md"),
    c("tools.clear", "Clear", "Премахване на вида", Icon::ClearLook)
        .alias(&["APC"])
        .run(Action::Material(MatCmd::Clear))
        .inv("Clear Appearance Overrides")
        .at(&[T_LOOK])
        .tip("Remove appearance overrides from faces, features, bodies")
        .spec("2.15-materials.md"),
    c("tools.parameters", "Parameters", "Параметри", Icon::Parameters)
        .alias(&["PRM"])
        .run(Action::Parameters)
        .at(&[T_PARAMS])
        .tip("Parameters (fx): names, equations, tolerances, user parameters")
        .spec("0.5-parameters.md"),
    c("tools.rebuild_all", "Rebuild All", "Пълна регенерация", Icon::Rebuild)
        .alias(&["RBA"])
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
        .alias(&["STE"])
        .run(Action::Material(MatCmd::StylesEditor))
        .inv("Style and Standard Editor")
        .at(&[G_STYLES])
        .env(Envs::PART_DRAWING)
        .tip("Style and Standard Editor: the document's materials and appearances (in a drawing: its dimension, text, leader, centre mark, hatch, view annotation styles, layers and Object Defaults)")
        .spec("2.15-materials.md"),
    c("manage.update_styles", "Update Styles", "Обновяване на стиловете", Icon::Rebuild)
        .alias(&["UST"])
        .run(Action::Material(MatCmd::UpdateStyles))
        .short("Update")
        .inv("Update")
        .at(&[G_STYLES])
        .tip("Update the document's styles from the library")
        .spec("2.15-materials.md"),
    c("manage.purge", "Purge Styles", "Изчистване на неизползвани стилове", Icon::Purge)
        .alias(&["PUR"])
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
    // ---- Manage › Documents (spec 0.6b) ---------------------------------------
    doc("doc.window", "Documents", "Документи", Icon::Documents)
        .run(Action::Docs(DocCmd::Window))
        .alias(&["DM"])
        .at(&[G_DOCS])
        .tip("The documents of this design file and the files it links: open, rename, duplicate, delete"),
    doc("doc.new_part", "New Part in This File", "Нова част във файла", Icon::DocNewPart)
        .run(Action::Docs(DocCmd::NewPart))
        .short("Part")
        .alias(&["NP"])
        .at(&[G_DOCS])
        .tip("A new part stored inside this design file; it becomes the active document"),
    doc("doc.new_sheet_metal", "New Sheet Metal in This File", "Нова листова част във файла", Icon::DocNewSheetMetal)
        .run(Action::Docs(DocCmd::NewSheetMetal))
        .short("Sheet Metal")
        .alias(&["NSM"])
        .at(&[G_DOCS])
        .tip("A new sheet metal part stored inside this design file"),
    doc("doc.new_assembly", "New Assembly in This File", "Нова сглобка във файла", Icon::DocNewAssembly)
        .run(Action::Docs(DocCmd::NewAssembly))
        .short("Assembly")
        .alias(&["NA"])
        .at(&[G_DOCS])
        .tip("A new assembly stored inside this design file; Place offers the file's parts"),
    doc("doc.new_drawing", "New Drawing in This File", "Нов чертеж във файла", Icon::DocNewDrawing)
        .run(Action::Docs(DocCmd::NewDrawing))
        .short("Drawing")
        .alias(&["ND"])
        .at(&[G_DOCS])
        .tip("A new drawing stored inside this design file; Base View offers the file's models"),
    doc("doc.new_presentation", "New Presentation in This File", "Нова презентация във файла", Icon::DocNewPresentation)
        .short("Presentation")
        .alias(&["NPN"])
        .at(&[G_DOCS])
        .tip("A new presentation inside this design file (presentations are still to come)")
        .todo("0.6"),
    doc("doc.insert", "Insert Document", "Вмъкване на документ", Icon::DocInsert)
        .short("Insert")
        .alias(&["IDF"])
        .at(&[G_DOCS])
        .tip("Copy documents of another file into this one, with what they reference")
        .todo("0.6"),
    doc("doc.link", "Link File", "Връзка към файл", Icon::DocLink)
        .alias(&["LF"])
        .at(&[G_DOCS])
        .tip("Link another file: its documents can be placed and shown without copying them")
        .todo("0.6"),
    doc("doc.duplicate", "Duplicate Document", "Дублиране на документ", Icon::DocDuplicate)
        .run(Action::Docs(DocCmd::Duplicate))
        .short("Duplicate")
        .alias(&["DUP"])
        .at(&[G_DOCS])
        .tip("A copy of the document in this file (named <name> Copy)"),
    doc("doc.rename", "Rename Document", "Преименуване на документ", Icon::DocRename)
        .run(Action::Docs(DocCmd::Rename))
        .short("Rename")
        .alias(&["RND"])
        .at(&[G_DOCS])
        .tip("Rename the document; names are unique inside a file"),
    doc("doc.delete", "Delete Document", "Изтриване на документ", Icon::DocDelete)
        .run(Action::Docs(DocCmd::Delete))
        .short("Delete")
        .alias(&["DDC"])
        .at(&[G_DOCS])
        .tip("Remove the document from this file (refused while another document uses it)"),
    doc("doc.extract", "Save to Separate File", "Запис в отделен файл", Icon::DocExtract)
        .short("Separate File")
        .alias(&["XF"])
        .at(&[G_STORE])
        .tip("Move the document into its own .ccpart / .ccasm / .ccdrw file and link it back")
        .todo("0.6"),
    doc("doc.embed", "Embed", "Вграждане", Icon::DocEmbed)
        .alias(&["MKI"])
        .at(&[G_STORE])
        .tip("Copy a linked document into this file; its references move to the copy")
        .todo("0.6"),
    doc("doc.pack", "Embed All", "Вграждане на всички", Icon::DocPack)
        .alias(&["PK"])
        .at(&[G_STORE])
        .tip("Pack: every linked document into this file, so it stands alone")
        .todo("0.6"),
    doc("doc.unpack", "Separate All", "Разделяне на всички", Icon::DocUnpack)
        .alias(&["UPK"])
        .at(&[G_STORE])
        .tip("Unpack: every document of this file into its own file, linked back")
        .todo("0.6"),
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
        .alias(&["NFR"])
        .run(Action::View(StandardView::Front))
        .short("Front")
        .at(&[V_NAV])
        .tip("Look from −Y")
        .spec("0.3-view.md"),
    c("view.top", "Top View", "Изглед отгоре", Icon::ViewTop)
        .alias(&["NTP"])
        .run(Action::View(StandardView::Top))
        .short("Top")
        .at(&[V_NAV])
        .tip("Look from +Z")
        .spec("0.3-view.md"),
    c("view.right", "Right View", "Изглед отдясно", Icon::ViewRight)
        .alias(&["NRT"])
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
        .alias(&["NOR"])
        .run(Action::Projection(Projection::Orthographic))
        .at(&[V_LOOK])
        .tip("Parallel projection")
        .spec("0.3-view.md"),
    c("view.perspective", "Perspective", "Перспектива", Icon::Ortho)
        .alias(&["NPE"])
        .run(Action::Projection(Projection::Perspective))
        .at(&[V_LOOK])
        .tip("Perspective projection")
        .spec("0.3-view.md"),
    c("view.perspective_ortho", "Perspective with Ortho Faces", "Перспектива с ортогонални стени", Icon::Ortho)
        .alias(&["NPO"])
        .run(Action::Projection(Projection::PerspectiveOrthoFaces))
        .at(&[V_LOOK])
        .tip("Perspective, but orthographic while looking straight at a ViewCube face")
        .spec("0.3-view.md"),
    c("view.vis_origin_planes", "Origin Planes", "Основни равнини (видимост)", Icon::WorkPlane)
        .alias(&["NVO"])
        .run(Action::Ui(UiCmd::ObjVis(VisKind::OriginPlanes)))
        .at(&[V_VIS])
        .tip("Show or hide the origin planes XY, XZ, YZ (Object Visibility)")
        .spec("2.10-work-features.md"),
    c("view.vis_work_planes", "User Work Planes", "Работни равнини (видимост)", Icon::WorkPlane)
        .alias(&["NVP"])
        .run(Action::Ui(UiCmd::ObjVis(VisKind::WorkPlanes)))
        .at(&[V_VIS])
        .tip("Show or hide every user work plane (a plane hidden on its own stays hidden)")
        .spec("2.10-work-features.md"),
    c("view.vis_work_axes", "User Work Axes", "Работни оси (видимост)", Icon::WorkAxis)
        .alias(&["NVA"])
        .run(Action::Ui(UiCmd::ObjVis(VisKind::WorkAxes)))
        .at(&[V_VIS])
        .tip("Show or hide every user work axis")
        .spec("2.10-work-features.md"),
    c("view.vis_work_points", "User Work Points", "Работни точки (видимост)", Icon::WorkPoint)
        .alias(&["NVT"])
        .run(Action::Ui(UiCmd::ObjVis(VisKind::WorkPoints)))
        .at(&[V_VIS])
        .tip("Show or hide every user work point")
        .spec("2.10-work-features.md"),
    c("view.vis_ucs", "UCS Triads", "UCS (видимост)", Icon::Ucs)
        .alias(&["NVU"])
        .run(Action::Ui(UiCmd::ObjVis(VisKind::Ucs)))
        .at(&[V_VIS])
        .tip("Show or hide the UCS triads and their planes")
        .spec("2.10-work-features.md"),
    c("view.browser", "Browser", "Дърво на модела (показване)", Icon::UiPanels)
        .alias(&["UIB"])
        .run(Action::Ui(UiCmd::Toggle(Element::Browser)))
        .at(&[V_WIN, VIEW])
        .tip("Show or hide the model browser")
        .spec("0.13-ui-layout.md"),
    c("view.properties", "Properties", "Панел Properties (показване)", Icon::UiPanels)
        .alias(&["UIP"])
        .run(Action::Ui(UiCmd::Toggle(Element::Properties)))
        .at(&[V_WIN, VIEW])
        .tip("Show or hide the Properties panel")
        .spec("0.13-ui-layout.md"),
    c("view.ribbon", "Ribbon", "Лента с команди (показване)", Icon::UiPanels)
        .alias(&["UIR"])
        .run(Action::Ui(UiCmd::Toggle(Element::Ribbon)))
        .at(&[V_WIN, VIEW])
        .tip("Show or hide the ribbon")
        .spec("0.13-ui-layout.md"),
    c("view.status_bar", "Status Bar", "Лента на състоянието (показване)", Icon::UiPanels)
        .alias(&["UIS"])
        .run(Action::Ui(UiCmd::Toggle(Element::StatusBar)))
        .at(&[V_WIN, VIEW])
        .tip("Show or hide the status bar")
        .spec("0.13-ui-layout.md"),
    c("view.view_cube", "ViewCube", "Куб на изгледите (показване)", Icon::UiPanels)
        .alias(&["UIV"])
        .run(Action::Ui(UiCmd::Toggle(Element::ViewCube)))
        .at(&[V_WIN, VIEW])
        .tip("Show or hide the ViewCube")
        .spec("0.13-ui-layout.md"),
    c("view.triad", "Axis Triad", "Триада на осите (показване)", Icon::UiPanels)
        .alias(&["UIT"])
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
        .alias(&["UIL"])
        .run(Action::Ui(UiCmd::ResetLayout))
        .short("Reset")
        .at(&[V_WIN, VIEW])
        .tip("Reset UI Layout: panels, sizes and window positions back to the defaults")
        .spec("0.13-ui-layout.md"),
    c("view.minimize_ribbon", "Minimize Ribbon", "Свиване на лентата", Icon::UiPanels)
        .alias(&["UIM"])
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
        .alias(&["SCV"])
        .at(&[Place::Ribbon("3D Model", "Convert"), Place::Ribbon("Sheet Metal", "Setup")])
        .env(PART)
        .tip("A solid to sheet metal: pick the face that stays put and the edges that become bends (a shelled solid keeps its walls); a part without solids gets the sheet metal rule and tab")
        .spec("5.6-sheet-metal.md"),
    c("sm.defaults", "Sheet Metal Defaults", "Настройки на листовия материал", Icon::Settings)
        .short("Defaults")
        .run(Action::SmDefaults)
        .alias(&["SMD"])
        .at(&[Place::Ribbon("Sheet Metal", "Setup")])
        .env(PART)
        .tip("The sheet metal rule of the part: thickness, bend radius, K-factor, reliefs, gap")
        .spec("5.6-sheet-metal.md"),
    c("sm.face", "Face", "Плоча", Icon::SmFace)
        .run(Action::SmFace)
        .alias(&["SFA"])
        .at(&[Place::Ribbon("Sheet Metal", "Create")])
        .env(PART)
        .tip("A sheet from a closed sketch profile, the rule's thickness")
        .spec("5.6-sheet-metal.md"),
    c("sm.flange", "Flange", "Фланец", Icon::SmFlange)
        .run(Action::SmFlange)
        .alias(&["SMF"])
        .at(&[Place::Ribbon("Sheet Metal", "Create")])
        .env(PART)
        .tip("Bend flanges on sheet edges: height, angle, bend radius, Bend Position")
        .spec("5.6-sheet-metal.md"),
    c("sm.flat_pattern", "Flat Pattern", "Развивка", Icon::SmFlatPattern)
        .run(Action::SmFlatPattern)
        .alias(&["SFP"])
        .at(&[Place::Ribbon("Sheet Metal", "Flat Pattern")])
        .env(PART)
        .tip("Show the part unfolded (its extents and bends in Properties); again: back to the folded part")
        .spec("5.6-sheet-metal.md"),
    c("sm.contour_flange", "Contour Flange", "Контурен фланец", Icon::SmContour)
        .run(Action::SmContour)
        .alias(&["SCF"])
        .at(&[Place::Ribbon("Sheet Metal", "Create")]).env(PART)
        .tip("A new sheet from an open chain of sketch lines: every corner a bend of the rule").spec("5.6-sheet-metal.md"),
    c("sm.hem", "Hem", "Подгъв", Icon::SmHem)
        .run(Action::SmHem)
        .alias(&["SHM"])
        .at(&[Place::Ribbon("Sheet Metal", "Create")]).env(PART)
        .tip("A single hem folded back on sheet edges: length and gap").spec("5.6-sheet-metal.md"),
    c("sm.fold", "Fold", "Сгъване", Icon::SmFold)
        .run(Action::SmFold)
        .alias(&["SFO"])
        .at(&[Place::Ribbon("Sheet Metal", "Create")]).env(PART)
        .tip("Fold the sheet along a sketch line on one of its faces").spec("5.6-sheet-metal.md"),
    c("sm.bend", "Bend", "Огъвка", Icon::SmBend)
        .run(Action::SmPlanned(PlannedKind::Bend))
        .alias(&["SBD"])
        .at(&[Place::Ribbon("Sheet Metal", "Create")]).env(PART)
        .tip("A bend joining two faces by an edge of each; Double Bend for parallel faces (panel only: the geometry comes next)").spec("5.6-sheet-metal.md"),
    c("sm.contour_roll", "Contour Roll", "Валцоване", Icon::SmContourRoll)
        .run(Action::SmPlanned(PlannedKind::ContourRoll))
        .alias(&["SCR"])
        .at(&[Place::Ribbon("Sheet Metal", "Create")]).env(PART)
        .tip("A rolled sheet from an open profile turned about an axis (panel only: the geometry comes next)").spec("5.6-sheet-metal.md"),
    c("sm.lofted_flange", "Lofted Flange", "Преходен фланец", Icon::SmLoftedFlange)
        .run(Action::SmPlanned(PlannedKind::LoftedFlange))
        .alias(&["SLF"])
        .at(&[Place::Ribbon("Sheet Metal", "Create")]).env(PART)
        .tip("A transition between two profiles (square to round), die formed or press brake (panel only: the geometry comes next)").spec("5.6-sheet-metal.md"),
    c("sm.rip", "Rip", "Разрез", Icon::SmRip)
        .run(Action::SmPlanned(PlannedKind::Rip))
        .alias(&["SRP"])
        .at(&[Place::Ribbon("Sheet Metal", "Create")]).env(PART)
        .tip("Split a closed shape so it unfolds: at a point, between two points or a whole face (panel only: the geometry comes next)").spec("5.6-sheet-metal.md"),
    c("sm.cut", "Cut", "Изрязване", Icon::SmCut)
        .run(Action::SmCut)
        .alias(&["SCT"])
        .at(&[Place::Ribbon("Sheet Metal", "Modify")]).env(PART)
        .tip("Closed sketch profiles cut through the sheet face they lie on (kept in the flat pattern); across bends: Unfold, Cut, Refold").spec("5.6-sheet-metal.md"),
    c("sm.corner_seam", "Corner Seam", "Ъглов шев", Icon::SmCornerSeam)
        .run(Action::SmCornerSeam)
        .alias(&["SCS"])
        .at(&[Place::Ribbon("Sheet Metal", "Modify")]).env(PART)
        .tip("Close the corner between two flanges again: Symmetric Gap, Overlap or Reverse Overlap, gap, percent overlap").spec("5.6-sheet-metal.md"),
    c("sm.corner_round", "Corner Round", "Заобляне на ъгъл", Icon::SmCornerRound)
        .run(Action::SmPlanned(PlannedKind::CornerRound))
        .alias(&["SRD"])
        .at(&[Place::Ribbon("Sheet Metal", "Modify")]).env(PART)
        .tip("Round the sheet's corners through the thickness, sets of corners with their radii (panel only: the geometry comes next)").spec("5.6-sheet-metal.md"),
    c("sm.corner_chamfer", "Corner Chamfer", "Скосяване на ъгъл", Icon::SmCornerChamfer)
        .run(Action::SmPlanned(PlannedKind::CornerChamfer))
        .alias(&["SCH"])
        .at(&[Place::Ribbon("Sheet Metal", "Modify")]).env(PART)
        .tip("Chamfer the sheet's corners: one distance, distance and angle, or two distances (panel only: the geometry comes next)").spec("5.6-sheet-metal.md"),
    c("sm.punch", "Punch Tool", "Щанца", Icon::SmPunch)
        .run(Action::SmPlanned(PlannedKind::Punch))
        .alias(&["SPT"])
        .at(&[Place::Ribbon("Sheet Metal", "Modify")]).env(PART)
        .tip("Louvres, lances, embosses from the punch library at sketch points (panel only: needs the punch library, 6.3)").spec("5.6-sheet-metal.md"),
    c("sm.unfold", "Unfold", "Разгъване", Icon::SmUnfold)
        .run(Action::SmUnfold)
        .alias(&["SUF"])
        .at(&[Place::Ribbon("Sheet Metal", "Modify")]).env(PART)
        .tip("Flatten the sheet (all its bends): later features (Cut) work on the flat pattern until Refold").spec("5.6-sheet-metal.md"),
    c("sm.refold", "Refold", "Обратно сгъване", Icon::SmRefold)
        .run(Action::SmRefold)
        .alias(&["SRF"])
        .at(&[Place::Ribbon("Sheet Metal", "Modify")]).env(PART)
        .tip("Fold the unfolded sheet again, with what was cut while flat").spec("5.6-sheet-metal.md"),
    c("sm.export_flat", "Export Flat Pattern", "Експорт на развивката", Icon::SmExportFlat)
        .short("Export")
        .run(Action::SmExportFlat)
        .alias(&["SFX"])
        .at(&[Place::Ribbon("Sheet Metal", "Flat Pattern")]).env(PART)
        .tip("The flat pattern as DXF: outline and cut-outs on OUTER, bend lines on BEND_UP / BEND_DOWN").spec("5.6-sheet-metal.md"),
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
        .alias(&["FIX"])
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
        .alias(&["UPD"])
        .run(Action::AsmUpdate)
        .at(&[Place::Ribbon("Assemble", "Manage")])
        .env(ASSEMBLY)
        .tip("Read the linked part files again (changes saved in them come in) and solve the constraints")
        .spec("3.1-assemblies.md"),
    c("asm.bom", "Bill of Materials", "Спецификация", Icon::Parameters)
        .alias(&["BOM"])
        .short("BOM")
        .run(Action::AsmBom)
        .at(&[Place::Ribbon("Assemble", "Manage")])
        .env(ASSEMBLY)
        .tip("The parts of the assembly with their quantities and iProperties: Model Data, Structured, Parts Only; Export to CSV")
        .spec("3.6-bom.md"),
    // ---- Drawing: Place Views and Annotate --------------------------------
    d("drw.base", "Base View", "Основен изглед", Icon::BaseView)
        .run(Action::Drawing(DrwCmd::BaseView))
        .alias(&["BV"])
        .at(&[D_CREATE])
        .tip("Place the first view of a part: choose the file, orientation, style and scale, then click on the sheet"),
    d("drw.projected", "Projected View", "Проекционен изглед", Icon::ProjectedView)
        .run(Action::Drawing(DrwCmd::ProjectedView))
        .alias(&["PV"])
        .at(&[D_CREATE])
        .tip("Pick a parent view, then click beside it for orthographic views or diagonally for isometric ones"),
    d("drw.auxiliary", "Auxiliary View", "Спомагателен изглед", Icon::AuxView)
        .run(Action::Drawing(DrwCmd::Auxiliary))
        .alias(&["AV"])
        .at(&[D_CREATE])
        .tip("Click a straight edge of a view, then place the view square to it"),
    d("drw.section", "Section View", "Разрез", Icon::SectionView)
        .run(Action::Drawing(DrwCmd::Section))
        .alias(&["SV"])
        .at(&[D_CREATE])
        .tip("Click the parent view, two points of the section line, then place the hatched section"),
    d("drw.detail", "Detail View", "Детайлен изглед", Icon::DetailView)
        .run(Action::Drawing(DrwCmd::Detail))
        .alias(&["DV"])
        .at(&[D_CREATE])
        .tip("Click the parent view, the circle's centre and size, then place the enlarged view"),
    dp("drw.overlay", "Overlay View", "Изглед с наслагване", Icon::ProjectedView, Dp::Overlay)
        .alias(&["OV"])
        .at(&[D_CREATE])
        .tip("Positional representations of an assembly in phantom lines"),
    dp("drw.draft_view", "Draft View", "Изглед-скица", Icon::DraftView, Dp::DraftView)
        .alias(&["DFV"])
        .at(&[D_CREATE])
        .tip("A view holding only a 2D sketch"),
    dp("drw.break", "Break", "Прекъсване", Icon::BreakView, Dp::Break)
        .alias(&["BRK"])
        .at(&[D_MODIFY])
        .tip("Shorten a long part between two break lines"),
    dp("drw.break_out", "Break Out", "Локален разрез", Icon::BreakOut, Dp::BreakOut)
        .alias(&["BO"])
        .at(&[D_MODIFY])
        .tip("Cut away material inside a closed profile to a depth"),
    dp("drw.slice", "Slice", "Сечение", Icon::SliceView, Dp::Slice)
        .alias(&["SL"])
        .at(&[D_MODIFY])
        .tip("A zero-depth section along a sketch line"),
    dp("drw.crop", "Crop", "Изрязване", Icon::CropView, Dp::Crop)
        .alias(&["CR"])
        .at(&[D_MODIFY])
        .tip("Clip a view by a rectangle or a closed sketch"),
    dp("drw.start_sketch", "New Sketch", "Нова скица", Icon::StartSketch, Dp::StartSketch)
        .alias(&["DS"])
        .at(&[D_SKETCH])
        .tip("A sketch on the sheet or on a view"),
    d("drw.new_sheet", "New Sheet", "Нов лист", Icon::NewSheet)
        .run(Action::Drawing(DrwCmd::NewSheet))
        .alias(&["NS"])
        .at(&[D_SHEETS])
        .tip("Add a sheet like the active one and make it active"),
    d("drw.edit_sheet", "Edit Sheet", "Редакция на лист", Icon::Drawing)
        .run(Action::Drawing(DrwCmd::EditSheet))
        .alias(&["ES"])
        .at(&[D_SHEETS])
        .tip("Name, size, orientation, revision and exclusions of the active sheet"),
    d("drw.dimension", "Dimension", "Размер", Icon::Dimension)
        .run(Action::Drawing(DrwCmd::Dimension))
        .alias(&["D"])
        .at(&[N_DIM])
        .tip("Click an edge (length), a circle (diameter / radius) or two points, then click where the dimension goes"),
    dp("drw.baseline", "Baseline Dimension", "Размери от база", Icon::Dimension, Dp::Baseline)
        .alias(&["BD"])
        .at(&[N_DIM])
        .tip("Dimensions from one datum"),
    dp("drw.ordinate", "Ordinate Dimension", "Ординатни размери", Icon::Dimension, Dp::Ordinate)
        .alias(&["OD"])
        .at(&[N_DIM])
        .tip("Ordinate dimensions from an origin"),
    dp("drw.chain", "Chain Dimension", "Верига от размери", Icon::Dimension, Dp::Chain)
        .alias(&["CD"])
        .at(&[N_DIM])
        .tip("A chain of dimensions"),
    dp("drw.retrieve", "Retrieve Model Annotations", "Извличане на размерите от модела", Icon::Dimension, Dp::Retrieve)
        .short("Retrieve")
        .alias(&["RMA"])
        .at(&[N_RETRIEVE])
        .tip("Bring the model's sketch and feature dimensions that lie in the view plane"),
    d("drw.hole_note", "Hole and Thread", "Бележка за отвор", Icon::HoleNote)
        .run(Action::Drawing(DrwCmd::HoleNote))
        .alias(&["HN"])
        .at(&[N_NOTES])
        .tip("Click a hole's circle, then place its note (diameter, depth, thread from the Hole feature)"),
    d("drw.text", "Text", "Текст", Icon::Text)
        .run(Action::Drawing(DrwCmd::Text))
        .alias(&["T"])
        .at(&[N_TEXT])
        .tip("Click where the note goes and type it"),
    d("drw.leader_text", "Leader Text", "Текст с водеща линия", Icon::Text)
        .run(Action::Drawing(DrwCmd::LeaderText))
        .alias(&["LE"])
        .at(&[N_TEXT])
        .tip("Click where the arrow points, then where the text goes, and type it"),
    d("drw.center_mark", "Center Mark", "Маркер на център", Icon::CenterMark)
        .run(Action::Drawing(DrwCmd::CenterMark))
        .alias(&["CM"])
        .at(&[N_SYMBOLS])
        .tip("Click circles or arcs of a view: a centre mark on each"),
    d("drw.centerline", "Centerline", "Осева линия", Icon::CenterlineMark)
        .run(Action::Drawing(DrwCmd::Centerline))
        .alias(&["CL"])
        .at(&[N_SYMBOLS])
        .tip("Click two circles of a view: a centreline through their centres"),
    dp("drw.surface", "Surface Texture Symbol", "Грапавост", Icon::SurfaceSymbol, Dp::SurfaceTexture)
        .alias(&["ST"])
        .at(&[N_SYMBOLS])
        .tip("A surface texture symbol"),
    dp("drw.datum", "Datum Identifier Symbol", "База", Icon::Datum, Dp::DatumIdentifier)
        .alias(&["DI"])
        .at(&[N_SYMBOLS])
        .tip("A datum identifier symbol"),
    dp("drw.fcf", "Feature Control Frame", "Рамка на допуск", Icon::ControlFrame, Dp::ControlFrame)
        .alias(&["FCF"])
        .at(&[N_SYMBOLS])
        .tip("A GD&T feature control frame"),
    d("drw.parts_list", "Parts List", "Спецификация", Icon::PartsList)
        .run(Action::Drawing(DrwCmd::PartsList))
        .spec("4.4-parts-list.md")
        .alias(&["PL"])
        .at(&[N_TABLE])
        .tip("A parts list from the BOM of a view's assembly: select the view, OK, then place the table"),
    d("drw.balloon", "Balloon", "Позиция", Icon::Balloon)
        .run(Action::Drawing(DrwCmd::Balloon))
        .spec("4.4-parts-list.md")
        .alias(&["B"])
        .at(&[N_TABLE])
        .tip("Item balloons: click a component's edge, then where the balloon goes"),
    dp("drw.hole_table", "Hole Table - View", "Таблица на отворите", Icon::PartsList, Dp::HoleTableView)
        .alias(&["HT"])
        .at(&[N_TABLE])
        .tip("Holes of a view with coordinates from a datum"),
    dp("drw.revision_table", "Revision Table", "Таблица на ревизиите", Icon::PartsList, Dp::RevisionTable)
        .alias(&["RT"])
        .at(&[N_REVISION])
        .tip("Rows of revisions"),
    dp("drw.align_h", "Horizontally Align View", "Хоризонтално подравняване", Icon::ProjectedView, Dp::AlignHorizontal)
        .alias(&["ALH"])
        .at(&[D_MODIFY])
        .tip("Align a view horizontally to another one"),
    dp("drw.align_v", "Vertically Align View", "Вертикално подравняване", Icon::ProjectedView, Dp::AlignVertical)
        .alias(&["ALV"])
        .at(&[D_MODIFY])
        .tip("Align a view vertically to another one"),
    dp("drw.align_pos", "Positionally Align View", "Подравняване по положение", Icon::ProjectedView, Dp::AlignInPosition)
        .alias(&["ALP"])
        .at(&[D_MODIFY])
        .tip("Keep a view in place relative to another one"),
    dp("drw.align_break", "Remove View Alignment", "Премахване на подравняването", Icon::ProjectedView, Dp::AlignBreak)
        .alias(&["ALB"])
        .at(&[D_MODIFY])
        .tip("Free a view from its alignment"),
    dp("drw.baseline_set", "Baseline Dimension Set", "Група размери от база", Icon::Dimension, Dp::BaselineSet)
        .alias(&["BDS"])
        .at(&[N_DIM])
        .tip("Baseline dimensions kept as one set"),
    dp("drw.ordinate_set", "Ordinate Dimension Set", "Група ординатни размери", Icon::Dimension, Dp::OrdinateSet)
        .alias(&["ODS"])
        .at(&[N_DIM])
        .tip("Ordinate dimensions kept as one set"),
    dp("drw.chain_set", "Chain Dimension Set", "Група верижни размери", Icon::Dimension, Dp::ChainSet)
        .alias(&["CDS"])
        .at(&[N_DIM])
        .tip("Chain dimensions kept as one set"),
    dp("drw.arrange", "Arrange Dimensions", "Подреждане на размерите", Icon::Dimension, Dp::Arrange)
        .alias(&["AD"])
        .at(&[N_DIM])
        .tip("Space the selected dimensions evenly"),
    dp("drw.chamfer_note", "Chamfer Note", "Бележка за фаска", Icon::HoleNote, Dp::ChamferNote)
        .alias(&["CN"])
        .at(&[N_NOTES])
        .tip("A note with the chamfer distances and angle"),
    dp("drw.punch_note", "Punch Notes", "Бележка за щанцоване", Icon::HoleNote, Dp::PunchNote)
        .alias(&["PN"])
        .at(&[N_NOTES])
        .tip("Punch notes in a flat pattern view"),
    d("drw.bend_note", "Bend Notes", "Бележка за огъвка", Icon::HoleNote)
        .run(Action::Drawing(DrwCmd::BendNote))
        .spec("4.8-sheet-metal-drawings.md")
        .alias(&["BN"])
        .at(&[N_NOTES])
        .tip("Click the bend lines of a flat pattern view: direction, angle and radius on each"),
    dp("drw.symbols", "Symbols", "Символи", Icon::Datum, Dp::SketchedSymbols)
        .alias(&["SY"])
        .at(&[N_SYMBOLS])
        .tip("Place a sketched symbol from Drawing Resources"),
    dp("drw.welding", "Welding Symbol", "Символ за заварка", Icon::ControlFrame, Dp::Welding)
        .alias(&["WS"])
        .at(&[N_SYMBOLS])
        .tip("A welding symbol: arrow and other side, flags, tail note"),
    dp("drw.caterpillar", "Caterpillar", "Заваръчен шев (гъсеница)", Icon::CenterlineMark, Dp::Caterpillar)
        .alias(&["CAT"])
        .at(&[N_SYMBOLS])
        .tip("Weld beads drawn along edges"),
    dp("drw.end_fill", "End Fill", "Сечение на шев", Icon::SectionView, Dp::EndFill)
        .alias(&["EF"])
        .at(&[N_SYMBOLS])
        .tip("The filled end section of a weld bead"),
    dp("drw.feature_id", "Feature Identifier Symbol", "Идентификатор на елемент", Icon::Datum, Dp::FeatureIdentifier)
        .alias(&["FI"])
        .at(&[N_SYMBOLS])
        .tip("A feature identifier symbol"),
    dp("drw.dt_leader", "Datum Target - Leader", "Базова цел - водеща линия", Icon::Datum, Dp::DatumTargetLeader)
        .alias(&["DTL"])
        .at(&[N_SYMBOLS])
        .tip("A datum target with a leader"),
    dp("drw.dt_circle", "Datum Target - Circle", "Базова цел - кръг", Icon::Datum, Dp::DatumTargetCircle)
        .alias(&["DTC"])
        .at(&[N_SYMBOLS])
        .tip("A circular datum target area"),
    dp("drw.dt_line", "Datum Target - Line", "Базова цел - линия", Icon::Datum, Dp::DatumTargetLine)
        .alias(&["DTN"])
        .at(&[N_SYMBOLS])
        .tip("A datum target line"),
    dp("drw.dt_point", "Datum Target - Point", "Базова цел - точка", Icon::Datum, Dp::DatumTargetPoint)
        .alias(&["DTP"])
        .at(&[N_SYMBOLS])
        .tip("A datum target point"),
    dp("drw.dt_rect", "Datum Target - Rectangle", "Базова цел - правоъгълник", Icon::Datum, Dp::DatumTargetRectangle)
        .alias(&["DTR"])
        .at(&[N_SYMBOLS])
        .tip("A rectangular datum target area"),
    dp("drw.edge_symbol", "Edge Symbol", "Символ за ръб", Icon::SurfaceSymbol, Dp::EdgeSymbol)
        .alias(&["EGS"])
        .at(&[N_SYMBOLS])
        .tip("An edge state symbol (ISO 13715)"),
    dp("drw.transition", "Transition Symbol", "Символ за преход", Icon::SurfaceSymbol, Dp::TransitionSymbol)
        .alias(&["TS"])
        .at(&[N_SYMBOLS])
        .tip("Mark a tangent transition edge"),
    dp("drw.bisector", "Centerline Bisector", "Ъглополовяща ос", Icon::CenterlineMark, Dp::CenterlineBisector)
        .alias(&["CB"])
        .at(&[N_SYMBOLS])
        .tip("A centre line half-way between two edges"),
    dp("drw.centered_pattern", "Centered Pattern", "Оси на кръгов масив", Icon::CenterMark, Dp::CenteredPattern)
        .alias(&["CP"])
        .at(&[N_SYMBOLS])
        .tip("Centre lines of a circular pattern of holes"),
    dp("drw.auto_centerlines", "Automated Centerlines", "Автоматични оси", Icon::CenterMark, Dp::AutoCenterlines)
        .alias(&["ACL"])
        .at(&[N_SYMBOLS])
        .tip("Centre marks and lines on chosen feature types of the views"),
    d("drw.auto_balloon", "Auto Balloon", "Автоматични позиции", Icon::Balloon)
        .run(Action::Drawing(DrwCmd::AutoBalloon))
        .spec("4.4-parts-list.md")
        .alias(&["AB"])
        .at(&[N_TABLE])
        .tip("Balloons for every component of a view at once, around it or in a row"),
    d("drw.table", "Table", "Таблица", Icon::PartsList)
        .run(Action::Drawing(DrwCmd::Table))
        .spec("4.8-sheet-metal-drawings.md")
        .alias(&["TB"])
        .at(&[N_TABLE])
        .tip("A table from a view: on a flat pattern view, the bend table with tags (blank and CSV / Excel tables planned)"),
    dp("drw.hole_table_sel", "Hole Table - Selection", "Таблица на избрани отвори", Icon::PartsList, Dp::HoleTableSelection)
        .alias(&["HTS"])
        .at(&[N_TABLE])
        .tip("A hole table of the selected holes"),
    dp("drw.hole_table_feat", "Hole Table - Selected Feature", "Таблица на отворите на елемент", Icon::PartsList, Dp::HoleTableFeature)
        .alias(&["HTF"])
        .at(&[N_TABLE])
        .tip("A hole table of the holes of one feature type"),
    dp("drw.revision_tag", "Revision Tag", "Маркер на ревизия", Icon::Text, Dp::RevisionTag)
        .alias(&["RTG"])
        .at(&[N_REVISION])
        .tip("A revision tag next to a change"),
    dp("drw.revision_cloud", "Revision Cloud", "Облак на ревизия", Icon::Text, Dp::RevisionCloud)
        .alias(&["RC"])
        .at(&[N_REVISION])
        .tip("A cloud round a change"),
    dp("drw.update_all", "Update All Sheets", "Обновяване на всички листове", Icon::Rebuild, Dp::UpdateAllSheets)
        .alias(&["UAS"])
        .at(&[G_UPDATE])
        .tip("Update the views of every sheet"),
    dp("drw.replace_model", "Replace Model Reference", "Смяна на модела", Icon::Drawing, Dp::ReplaceModel)
        .alias(&["RMR"])
        .at(&[G_MODIFY])
        .tip("Point the drawing views at another model file"),
    dp("drw.define_border", "Define Border", "Дефиниране на рамка", Icon::Drawing, Dp::DefineBorder)
        .alias(&["DBR"])
        .at(&[G_DEFINE])
        .tip("A new border template"),
    dp("drw.define_title", "Define Title Block", "Дефиниране на основен надпис", Icon::Drawing, Dp::DefineTitleBlock)
        .alias(&["DTB"])
        .at(&[G_DEFINE])
        .tip("A new title block template"),
    dp("drw.define_symbol", "Define Sketched Symbol", "Дефиниране на символ", Icon::Drawing, Dp::DefineSymbol)
        .alias(&["DSS"])
        .at(&[G_DEFINE])
        .tip("A new sketched symbol template"),
    d("drw.edit_layers", "Edit Layers", "Редакция на слоевете", Icon::Settings)
        .run(Action::Drawing(DrwCmd::EditLayers))
        .alias(&["EL"])
        .at(&[N_FORMAT])
        .tip("The drawing's layers in the Style and Standard Editor: line type, weight, colour, on / off")
        .spec("4.6-drawing-styles.md"),
    // ---- File / Help -----------------------------------------------------
    d("drw.export_pdf", "Export PDF", "Износ в PDF", Icon::Drawing)
        .run(Action::Drawing(DrwCmd::ExportPdf))
        .alias(&["PDF"])
        .at(&[FILE])
        .tip("The drawing's sheets as a vector PDF (File › Export › PDF)"),
    c("file.new_drawing", "New Drawing", "Нов чертеж", Icon::Drawing)
        .alias(&["NDW"])
        .run(Action::App(AppCmd::NewDrawing))
        .at(&[FILE])
        .tip("A new drawing in the open project; place views of its parts")
        .spec("4-drawings.md"),
    c("file.new_assembly", "New Assembly", "Ново сглобяване", Icon::Assembly)
        .alias(&["NAS"])
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
        .alias(&["SVF"])
        .run(Action::App(AppCmd::SaveAsFolder))
        .at(&[FILE])
        .tip("Unpacked directory, git-friendly")
        .spec("0.6-files.md"),
    c("file.export_stl", "Export STL", "Износ в STL", Icon::Part)
        .alias(&["STL"])
        .run(Action::App(AppCmd::Export(ExportFormat::Stl)))
        .at(&[FILE])
        .tip("Triangles in mm, for 3D printing")
        .spec("7.7-export.md"),
    c("file.export_step", "Export STEP", "Износ в STEP", Icon::Part)
        .alias(&["STP"])
        .run(Action::App(AppCmd::Export(ExportFormat::Step)))
        .at(&[FILE])
        .tip("Exact B-rep for other CAD programs")
        .spec("7.7-export.md"),
    c("file.export_glb", "Export GLB", "Износ в GLB (glTF)", Icon::Part)
        .alias(&["SGL"])
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
        .alias(&["SDG"])
        .run(Action::App(AppCmd::SaveDiagnostics))
        .at(&[HELP])
        .tip("Zip the session log, the action journal, crash reports, the open project and settings.json (to send with a problem report)")
        .spec("0.14-diagnostics.md"),
    c("help.logs_folder", "Open Logs Folder", "Папка с дневниците", Icon::Options)
        .alias(&["SLG"])
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
/// What a typed alias buffer matches in `env` (spec 0.15, alias input rule
/// 2): the command whose alias it is exactly, and the runnable commands
/// with a longer alias starting with it (first by registry order).
#[derive(Clone, Copy, Debug, Default)]
pub struct AliasMatch {
    /// The command whose alias the buffer is.
    pub exact: Option<&'static Command>,
    /// How many runnable commands have a longer alias starting with it.
    pub longer: usize,
    /// The first of them.
    pub first_longer: Option<&'static Command>,
}

/// Matches the alias buffer `buf` (case-insensitive) against the runnable
/// commands of `env`.
pub fn alias_match(buf: &str, env: Env) -> AliasMatch {
    let b = buf.to_ascii_uppercase();
    let mut m = AliasMatch::default();
    for c in COMMANDS
        .iter()
        .filter(|c| c.ready() && !c.held && c.valid_in(env))
    {
        let mut longer = false;
        for a in c.aliases {
            let a = a.to_ascii_uppercase();
            if a == b {
                m.exact.get_or_insert(c);
            } else if a.starts_with(&b) {
                longer = true;
            }
        }
        if longer {
            m.longer += 1;
            m.first_longer.get_or_insert(c);
        }
    }
    m
}

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
    fn every_implemented_command_has_a_shortcut() {
        // Owner's rule 14: a key or an alias for each runnable command.
        let missing: Vec<&str> = COMMANDS
            .iter()
            .filter(|c| c.ready() && !c.held && c.aliases.is_empty() && c.key.is_none())
            .map(|c| c.id)
            .collect();
        assert!(missing.is_empty(), "no key or alias: {missing:?}");
    }

    #[test]
    fn the_most_used_letters_start_at_once() {
        // No longer alias starts with them, so typing them waits for nothing.
        for (letter, env) in [
            ("E", Env::Part),
            ("H", Env::Part),
            ("L", Env::Sketch),
            ("D", Env::Sketch),
            ("O", Env::Sketch),
            ("X", Env::Sketch),
        ] {
            let m = alias_match(letter, env);
            assert!(m.exact.is_some(), "{letter} in {env:?}");
            assert_eq!(
                m.longer,
                0,
                "{letter} in {env:?}: {:?}",
                m.first_longer.map(|c| c.id)
            );
        }
    }

    #[test]
    fn typed_aliases_find_their_commands() {
        // SMF is Flange in a part; S starts a sketch but waits for longer ones.
        let m = alias_match("smf", Env::Part);
        assert_eq!(m.exact.map(|c| c.id), Some("sm.flange"));
        assert_eq!(m.longer, 0);
        let m = alias_match("S", Env::Part);
        assert!(m.exact.is_some());
        assert!(m.longer > 0);
        let m = alias_match("SM", Env::Part);
        assert!(m.exact.is_none());
        assert!(m.first_longer.is_some());
        assert_eq!(alias_match("QQ", Env::Part).longer, 0);
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
