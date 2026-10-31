//! Inventor-style ribbon: tabs → panels → commands.
//!
//! The ribbon deliberately lists the *whole* command set users know from
//! Inventor, in the same tabs and panels, so muscle memory carries over.
//! Commands that are not implemented yet are shown disabled, with a tooltip
//! pointing at their entry in `docs/FEATURES.md`; the ribbon doubles as a
//! visual roadmap.
//!
//! Layout follows Inventor's ribbon: each panel holds *large* buttons (icon
//! over label), columns of three *small* buttons (icon and label) and, for
//! the constraints, columns of three icon-only buttons. Icons are our own
//! vector drawings (`icons.rs`).

use eframe::egui::{self, RichText, Sense, Vec2};

use super::commands::{self, Command};
use super::demos::Demo;
use super::icons::{self, Icon, Palette};
use super::layout::{Element, UiCmd};
use super::sketch_tools::{ConstraintTool, Tool};
use super::viewcube::StandardView;
use crate::sketch::Style;

/// Something the user asked for from the ribbon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    /// Start 2D Sketch: pick an origin plane or a planar face.
    StartSketch,
    /// Leave sketch edit mode.
    FinishSketch,
    /// Show the edited sketch's coordinate-system settings.
    EditCoordinateSystem,
    /// Look straight at the edited/selected sketch.
    LookAt,
    /// Undo the last action.
    Undo,
    /// Redo the last undone action.
    Redo,
    /// Create a demo sketch on the box's top face.
    Demo(Demo),
    /// Show (F8) or hide (F9) constraint labels.
    ShowConstraints(bool),
    /// Select the body in the browser and show its properties.
    SelectBody,
    /// Jump to a standard view.
    View(StandardView),
    /// Open the Application Options window.
    Options,
    /// Activate a sketch tool.
    Tool(Tool),
    /// Format: set the style of the selection, or toggle construction mode
    /// for new geometry when nothing is selected.
    Format(Style),
    /// Show All Degrees of Freedom.
    ToggleDof,
    /// Relax mode for dragging.
    ToggleRelax,
    /// Extrude command (opens its dialog).
    Extrude,
    /// Assemble › Place: place a part as occurrences.
    AsmPlace,
    /// Assemble › Free Move.
    AsmFreeMove,
    /// Assemble › Assembly Features: the tools part edited in place (spec
    /// 3.10 §3.8).
    AsmFeatures,
    /// Assemble › Free Rotate.
    AsmFreeRotate,
    /// Assemble › Grounded: toggle on the selected occurrences.
    AsmGround,
    /// Assemble › Constrain: Mate / Flush, Angle, Insert.
    AsmConstrain,
    /// Assemble › Update: reload the linked part files, solve again.
    AsmUpdate,
    /// Assemble › Joint.
    AsmJoint,
    /// Assemble › Bill of Materials.
    AsmBom,
    /// Drawing commands (Place Views, Annotate).
    Drawing(super::drawing::DrwCmd),
    /// 3D Model › Convert to Sheet Metal.
    SmConvert,
    /// Sheet Metal › Sheet Metal Defaults.
    SmDefaults,
    /// Sheet Metal › Face.
    SmFace,
    /// Sheet Metal › Flange.
    SmFlange,
    /// Sheet Metal › Create Flat Pattern / Go to Folded Part.
    SmFlatPattern,
    /// Sheet Metal › Hem.
    SmHem,
    /// Sheet Metal › Fold.
    SmFold,
    /// Sheet Metal › Contour Flange.
    SmContour,
    /// Sheet Metal › Cut.
    SmCut,
    /// Sheet Metal › Unfold.
    SmUnfold,
    /// Sheet Metal › Refold.
    SmRefold,
    /// Sheet Metal › Export Flat Pattern (DXF).
    SmExportFlat,
    /// Sheet Metal › Corner Seam.
    SmCornerSeam,
    /// Sheet Metal commands whose panel is there and logic comes later
    /// (`gui/sm_planned.rs`): Bend, Contour Roll, Lofted Flange, Rip,
    /// Corner Round, Corner Chamfer, Punch Tool.
    SmPlanned(super::sm_planned::PlannedKind),
    /// A part command whose panel exists and whose logic comes later
    /// (`gui/part_planned.rs`, spec 2.0).
    PartPlanned(super::part_planned::PartPlanned),
    /// An assembly command whose panel exists and whose logic comes later
    /// (`gui/asm_planned.rs`, spec 3.0).
    AsmPlanned(super::asm_planned::AsmPlanned),
    /// 3D Model › Modify › Thicken/Offset.
    Thicken,
    /// 3D Model › Modify › Delete Face.
    DeleteFace,
    /// 3D Model › Modify › Move Bodies.
    MoveBodies,
    /// View › Visual Style.
    VisualStyle(super::view_style::VisualStyle),
    /// View › Section View (Quarter / Half / Three Quarter / End).
    SectionView(super::view_style::SectionKind),
    /// Revolve sketch regions around an axis.
    Revolve,
    /// Primitives: Box, Cylinder (sketch + Extrude), Sphere, Torus (sketch
    /// + full Revolve).
    Primitive(crate::part::PrimitiveKind),
    /// Work Plane: the method follows from the picks.
    WorkPlane,
    /// Fillet of model edges.
    Fillet,
    /// Chamfer of model edges.
    Chamfer,
    /// Shell: hollow a solid.
    Shell,
    /// Sweep: profiles along a path.
    Sweep,
    /// Loft: a body through sections.
    Loft,
    /// Coil: a profile screwed about an axis.
    Coil,
    /// Rib / Web: thin walls from open profiles.
    Rib,
    /// Face Draft: tilt faces against a pull direction.
    Draft,
    /// Combine: a base body with toolbodies.
    Combine,
    /// Split: faces, or a solid in one or two.
    Split,
    /// Rectangular Pattern of features or solids.
    RectPattern,
    /// Circular Pattern of features or solids.
    CircPattern,
    /// Mirror of features or solids across a plane.
    MirrorFeature,
    /// Hole: drilled, clearance and tapped holes with seats.
    Hole,
    /// Project Geometry: a click on a model edge projects it into the sketch.
    ProjectGeometry,
    /// Project Cut Edges: the section of the part by the sketch plane.
    ProjectCutEdges,
    /// Project Flat Pattern: a sheet's unfolded outline into the sketch.
    ProjectFlatPattern,
    /// Format → Center Point: new points are Center Points.
    CenterPoint,
    /// Thread: a cosmetic or modeled thread on a cylindrical face.
    Thread,
    /// Work Axis: the method follows from the picks.
    WorkAxis,
    /// Work Point: the method follows from the picks.
    WorkPoint,
    /// UCS: a user coordinate system (relative to the origin or on three
    /// points).
    Ucs,
    /// The Parameters table (fx).
    Parameters,
    /// Rebuild All: recompute every feature of the part from scratch.
    RebuildAll,
    /// View projection (Orthographic / Perspective / … with Ortho Faces).
    Projection(super::camera::Projection),
    /// Manage › Documents: documents of the design file (spec 0.6b).
    Docs(super::documents::DocCmd),
    /// Materials, appearances, styles, Physical Properties.
    Material(super::material::MatCmd),
    /// View → Windows: User Interface toggles, Clean Screen, Full Screen,
    /// Reset UI Layout (`layout.rs`).
    Ui(UiCmd),
    /// File / Help menu commands and keys without a ribbon button
    /// (`commands::AppCmd`).
    App(commands::AppCmd),
}

/// Minimize ribbon state (Inventor: Minimize to Tabs).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fold {
    /// Only the tab strip is shown.
    pub minimized: bool,
    /// Minimized, but the panels are shown until a command is picked.
    pub peek: bool,
    /// Tabs the user turned off (labels; right-click a tab → Show Tabs).
    pub hidden: Vec<String>,
}

impl Tab {
    /// Shown in this environment and not turned off by the user.
    pub fn visible(self, st: &State, hidden: &[String]) -> bool {
        self.shown_in(st) && !hidden.iter().any(|h| h == self.label())
    }

    /// Shown in this environment: [`Tab::shown`], and the Sketch tab while
    /// a drawing's view sketch is open (Inventor's drawing sketches).
    pub fn shown_in(self, st: &State) -> bool {
        self.shown(st.assembly, st.drawing) || (self == Tab::Sketch && st.drawing_sketch)
    }

    /// The first visible tab (the active one when the active tab is hidden).
    pub fn first_visible(st: &State, hidden: &[String]) -> Option<Tab> {
        Tab::ALL.into_iter().find(|t| t.visible(st, hidden))
    }
}

/// The tab strip's right-click menu (Inventor: Show Tabs, Minimize).
fn tabs_menu(ui: &mut egui::Ui, st: &State, fold: &mut Fold) {
    ui.menu_button("Show Tabs", |ui| {
        let tabs: Vec<Tab> = Tab::ALL.into_iter().filter(|t| t.shown_in(st)).collect();
        let on = tabs.iter().filter(|t| t.visible(st, &fold.hidden)).count();
        for t in tabs {
            let mut show = t.visible(st, &fold.hidden);
            // The last visible tab stays.
            let r = ui.add_enabled(!show || on > 1, egui::Checkbox::new(&mut show, t.label()));
            if r.changed() {
                fold.hidden.retain(|h| h != t.label());
                if !show {
                    fold.hidden.push(t.label().to_owned());
                }
            }
        }
        ui.separator();
        if ui
            .add_enabled(!fold.hidden.is_empty(), egui::Button::new("Show All"))
            .clicked()
        {
            fold.hidden.clear();
            ui.close();
        }
    });
    let label = if fold.minimized {
        "Show Full Ribbon"
    } else {
        "Minimize Ribbon"
    };
    if ui.button(label).clicked() {
        fold.minimized = !fold.minimized;
        fold.peek = false;
        ui.close();
    }
}

/// What the ribbon needs to know to enable commands.
pub struct State {
    /// Constraint labels are visible.
    pub labels_shown: bool,
    /// A sketch is in edit mode.
    pub editing: bool,
    /// A sketch plane is being picked.
    pub picking: bool,
    /// Active sketch tool.
    pub tool: Tool,
    /// Construction mode for new geometry.
    pub construction: bool,
    /// Degrees-of-freedom glyphs shown.
    pub show_dof: bool,
    /// Relax mode on.
    pub relax: bool,
    /// Which [`Element`]s are visible (in `Element::ALL` order).
    pub ui_shown: [bool; 6],
    /// View → Object Visibility switches.
    pub obj_vis: super::layout::ObjectVisibility,
    /// Clean Screen on.
    pub clean: bool,
    /// Full Screen on.
    pub full_screen: bool,
    /// Project Geometry is active.
    pub project: bool,
    /// Format → Center Point is on.
    pub center_point: bool,
    /// The active document is an assembly.
    pub assembly: bool,
    /// The active document is a drawing.
    pub drawing: bool,
    /// A drawing's view sketch is open: the Sketch tab is shown.
    pub drawing_sketch: bool,
    /// The view shows the flat pattern.
    pub flat: bool,
    /// The assembly command that is open (shown pressed).
    pub asm_tool: Option<Action>,
}

/// Ribbon tabs of the part environment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    /// Drawing: base, projected and other views, sheets.
    PlaceViews,
    /// Drawing: dimensions, notes, symbols, tables.
    Annotate,
    /// Assembly: place, position, relationships.
    Assemble,
    /// Assembly: Design Accelerator (fasteners, frames, power
    /// transmission, springs).
    Design,
    /// Sheet metal in any part (one part type, owner 2026-10-27): Face,
    /// Flange, Flat Pattern, and the model's holes, patterns, work features.
    SheetMetal,
    /// Features: create, modify, work features, patterns.
    Model,
    /// 2D sketch tools and constraints.
    Sketch,
    /// Measuring and analysis.
    Inspect,
    /// Options, parameters, materials and appearances.
    Tools,
    /// Styles and standards.
    Manage,
    /// Visibility and navigation.
    View,
    /// Part: other environments (Studio, Stress Analysis, Shape Generator,
    /// 3D Print, BIM Content).
    Environments,
}

impl Tab {
    const ALL: [Tab; 12] = [
        Tab::PlaceViews,
        Tab::Annotate,
        Tab::Assemble,
        Tab::Design,
        Tab::Model,
        Tab::SheetMetal,
        Tab::Sketch,
        Tab::Inspect,
        Tab::Tools,
        Tab::Manage,
        Tab::View,
        Tab::Environments,
    ];

    /// Whether the tab belongs to the environment (an assembly has no
    /// 3D Model / Sketch tabs, a part no Assemble tab, a drawing only its
    /// own tabs and the shared Tools / Manage / View).
    pub fn shown(self, assembly: bool, drawing: bool) -> bool {
        match self {
            Tab::PlaceViews | Tab::Annotate => drawing,
            Tab::Inspect if drawing => false,
            Tab::Assemble | Tab::Design => assembly && !drawing,
            Tab::SheetMetal => !assembly && !drawing,
            Tab::Model | Tab::Sketch => !assembly && !drawing,
            Tab::Environments => !drawing,
            _ => true,
        }
    }

    /// The tab with this label (a command's `Place::Ribbon`).
    pub fn by_label(label: &str) -> Option<Tab> {
        Tab::ALL.into_iter().find(|t| t.label() == label)
    }

    fn label(self) -> &'static str {
        match self {
            Tab::PlaceViews => "Place Views",
            Tab::Annotate => "Annotate",
            Tab::Assemble => "Assemble",
            Tab::Design => "Design",
            Tab::SheetMetal => "Sheet Metal",
            Tab::Model => "3D Model",
            Tab::Sketch => "Sketch",
            Tab::Inspect => "Inspect",
            Tab::Tools => "Tools",
            Tab::Manage => "Manage",
            Tab::View => "View",
            Tab::Environments => "Environments",
        }
    }
}

/// How a command is shown in its panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Size {
    /// Icon over label, full panel height.
    Large,
    /// Icon and label, three per column.
    Small,
    /// Icon only (name in the tooltip), three per column.
    Mini,
}

/// One ribbon command. Labels, icons, keys and tooltips come from the
/// command registry (`commands.rs`); this module only lays them out.
struct Cmd {
    /// Registry id (empty for a split button's header).
    id: &'static str,
    label: &'static str,
    icon: Icon,
    size: Size,
    /// Keyboard shortcut shown in the tooltip (Inventor defaults).
    key: Option<&'static str>,
    kind: Kind,
}

/// A variant of a split button.
struct Item {
    id: &'static str,
    label: &'static str,
    icon: Icon,
    action: Action,
}

enum Kind {
    /// Implemented: action and tooltip.
    Ready(Action, &'static str),
    /// Split button: a menu of variants.
    Menu(Vec<Item>),
    /// Implemented but not applicable now: why.
    Unavailable(&'static str),
    /// Not implemented yet: FEATURES.md section.
    Planned(&'static str),
}

impl Cmd {
    fn large(mut self) -> Self {
        self.size = Size::Large;
        self
    }
    fn mini(mut self) -> Self {
        self.size = Size::Mini;
        self
    }
    /// Another label than the registry's (the same command in a second
    /// place, e.g. View › Visibility).
    fn label(mut self, label: &'static str) -> Self {
        self.label = label;
        self
    }
}

/// Icon of a sketch tool (from the command registry).
pub fn tool_icon(t: Tool) -> Icon {
    commands::by_action(Action::Tool(t)).map_or(Icon::Select, |c| c.icon)
}

/// The button of registry command `cmd`.
fn button(cmd: &'static Command) -> Cmd {
    Cmd {
        id: cmd.id,
        label: cmd.ribbon_label(),
        icon: cmd.icon,
        size: Size::Small,
        key: cmd.shown_key(),
        kind: match (cmd.planned, cmd.action) {
            (None, Some(a)) => Kind::Ready(a, cmd.tip),
            (Some(s), _) => Kind::Planned(s),
            (None, None) => Kind::Planned("?"),
        },
    }
}

/// The button of the registry command with this id (a visible "?" button
/// if the id is unknown; the tests catch that).
fn c(id: &'static str) -> Cmd {
    match commands::by_id(id) {
        Some(cmd) => button(cmd),
        None => Cmd {
            id,
            label: id,
            icon: Icon::Settings,
            size: Size::Small,
            key: None,
            kind: Kind::Planned("?"),
        },
    }
}

/// The button of the registry command running `a`.
fn c_of(a: Action) -> Cmd {
    match commands::by_action(a) {
        Some(cmd) => button(cmd),
        None => c("?"),
    }
}

/// Split-button variants from registry ids.
fn items(ids: &[&'static str]) -> Vec<Item> {
    ids.iter()
        .filter_map(|id| commands::by_id(id))
        .filter_map(|cmd| {
            Some(Item {
                id: cmd.id,
                label: cmd.ribbon_label(),
                icon: cmd.icon,
                action: cmd.action?,
            })
        })
        .collect()
}

/// A split button `label` with the variants `items` (icon of the first).
fn split(label: &'static str, items: Vec<Item>) -> Cmd {
    Cmd {
        id: "",
        label,
        icon: items.first().map_or(Icon::Settings, |i| i.icon),
        size: Size::Small,
        key: None,
        kind: Kind::Menu(items),
    }
}

/// `cmd` when `available`, otherwise disabled with `why`.
fn when(available: bool, cmd: Cmd, why: &'static str) -> Cmd {
    if available {
        cmd
    } else {
        Cmd {
            kind: Kind::Unavailable(why),
            ..cmd
        }
    }
}

/// A titled group of commands.
struct PanelDef {
    title: &'static str,
    cmds: Vec<Cmd>,
}

fn panel(title: &'static str, cmds: Vec<Cmd>) -> PanelDef {
    PanelDef { title, cmds }
}

fn panels(tab: Tab, st: &State) -> Vec<PanelDef> {
    let in_sketch = "Only while editing a sketch";
    let constraints = if st.labels_shown {
        "sketch.hide_constraints"
    } else {
        "sketch.show_constraints"
    };
    match tab {
        Tab::PlaceViews => vec![
            panel(
                "Create",
                vec![
                    c("drw.base").large(),
                    c("drw.projected").large(),
                    c("drw.auxiliary"),
                    c("drw.section"),
                    c("drw.detail"),
                    c("drw.overlay"),
                    c("drw.draft_view"),
                ],
            ),
            panel(
                "Modify",
                vec![
                    c("drw.break"),
                    c("drw.break_out"),
                    c("drw.slice"),
                    c("drw.crop"),
                    split(
                        "Align",
                        items(&[
                            "drw.align_h",
                            "drw.align_v",
                            "drw.align_pos",
                            "drw.align_break",
                        ]),
                    ),
                ],
            ),
            panel("Sketch", vec![c("drw.start_sketch").large()]),
            panel(
                "Sheets",
                vec![c("drw.new_sheet").large(), c("drw.edit_sheet")],
            ),
        ],
        Tab::Annotate => vec![
            panel(
                "Dimension",
                vec![
                    c("drw.dimension").large(),
                    split(
                        "Baseline",
                        items(&[
                            "drw.baseline",
                            "drw.baseline_set",
                            "drw.ordinate",
                            "drw.ordinate_set",
                            "drw.chain",
                            "drw.chain_set",
                        ]),
                    ),
                    c("drw.arrange"),
                ],
            ),
            panel(
                "Feature Notes",
                vec![
                    c("drw.hole_note"),
                    c("drw.chamfer_note"),
                    c("drw.punch_note"),
                    c("drw.bend_note"),
                ],
            ),
            panel("Text", vec![c("drw.text").large(), c("drw.leader_text")]),
            panel(
                "Symbols",
                vec![
                    split(
                        "Symbols",
                        items(&[
                            "drw.symbols",
                            "drw.surface",
                            "drw.welding",
                            "drw.caterpillar",
                            "drw.end_fill",
                            "drw.fcf",
                            "drw.datum",
                            "drw.feature_id",
                            "drw.dt_leader",
                            "drw.dt_circle",
                            "drw.dt_line",
                            "drw.dt_point",
                            "drw.dt_rect",
                            "drw.edge_symbol",
                            "drw.transition",
                        ]),
                    ),
                    c("drw.centerline"),
                    c("drw.center_mark"),
                    c("drw.auto_centerlines"),
                    c("drw.bisector"),
                    c("drw.centered_pattern"),
                ],
            ),
            panel("Retrieve", vec![c("drw.retrieve").large()]),
            panel(
                "Table",
                vec![
                    c("drw.parts_list").large(),
                    split(
                        "Hole",
                        items(&[
                            "drw.hole_table",
                            "drw.hole_table_sel",
                            "drw.hole_table_feat",
                        ]),
                    ),
                    c("drw.table"),
                    split("Balloon", items(&["drw.balloon", "drw.auto_balloon"])),
                ],
            ),
            panel(
                "Revision",
                vec![
                    c("drw.revision_tag").mini(),
                    c("drw.revision_table").mini(),
                    c("drw.revision_cloud").mini(),
                ],
            ),
            panel("Format", vec![c("drw.edit_layers")]),
        ],
        Tab::SheetMetal => vec![
            panel(
                "Sketch",
                vec![
                    when(
                        !st.picking,
                        c("part.start_sketch"),
                        "Select a plane or face in the graphics window (Esc cancels)",
                    )
                    .large(),
                ],
            ),
            panel("Setup", vec![c("sm.defaults").large(), c("sm.convert")]),
            panel(
                "Create",
                vec![
                    c("sm.face").large(),
                    c("sm.contour_flange").large(),
                    c("sm.flange"),
                    c("sm.hem"),
                    c("sm.fold"),
                    c("sm.bend"),
                    c("sm.contour_roll"),
                    c("sm.lofted_flange"),
                    c("sm.rip"),
                ],
            ),
            panel(
                "Modify",
                vec![
                    c("sm.cut"),
                    c("part.hole"),
                    c("sm.corner_seam"),
                    c("sm.corner_round"),
                    c("sm.corner_chamfer"),
                    c("sm.punch"),
                    c("sm.unfold"),
                    c("sm.refold"),
                ],
            ),
            panel(
                "Pattern",
                vec![
                    c("part.rect_pattern"),
                    c("part.circ_pattern"),
                    c("part.mirror"),
                ],
            ),
            panel(
                "Work Features",
                vec![
                    c("part.work_plane").large(),
                    c("part.work_axis"),
                    c("part.work_point"),
                    c("part.ucs"),
                ],
            ),
            panel(
                "Flat Pattern",
                vec![c("sm.flat_pattern").large(), c("sm.export_flat")],
            ),
        ],
        Tab::Assemble => vec![
            panel(
                "Component",
                vec![
                    c("asm.place").large(),
                    c("asm.place_cc"),
                    c("asm.place_imported"),
                    c("asm.create").large(),
                    c("asm.replace"),
                ],
            ),
            panel(
                "Position",
                vec![c("asm.free_move"), c("asm.free_rotate"), c("asm.ground")],
            ),
            panel(
                "Relationships",
                vec![
                    c("asm.joint").large(),
                    c("asm.constrain").large(),
                    c("asm.assemble"),
                    c("asm.show_rel"),
                    c("asm.show_sick"),
                    c("asm.hide_rel"),
                ],
            ),
            panel(
                "Pattern",
                vec![c("asm.pattern"), c("asm.mirror"), c("asm.copy")],
            ),
            panel(
                "Productivity",
                vec![
                    c("asm.make_layout"),
                    c("asm.substitutes"),
                    c("asm.features"),
                ],
            ),
            panel("Simplification", vec![c("asm.simplify").large()]),
            panel(
                "Manage",
                vec![c("asm.update").large(), c("asm.bom").large()],
            ),
        ],
        Tab::Design => vec![
            panel(
                "Fasten",
                vec![
                    c("design.bolted").large(),
                    c("design.clevis_pin"),
                    c("design.secure_pin"),
                    c("design.radial_pin"),
                ],
            ),
            panel(
                "Frame",
                vec![
                    c("design.insert_frame").large(),
                    c("design.end_cap"),
                    c("design.frame_change"),
                    c("design.frame_info"),
                    c("design.miter"),
                    c("design.notch"),
                    c("design.corner_joint"),
                    c("design.trim_extend"),
                    c("design.lengthen_shorten"),
                    split(
                        "Reuse",
                        items(&[
                            "design.frame_reuse",
                            "design.change_reuse",
                            "design.remove_end_treatments",
                            "design.frame_refresh",
                            "design.edit_frame",
                        ]),
                    ),
                    c("design.beam_column").large(),
                ],
            ),
            panel(
                "Power Transmission",
                vec![
                    c("design.shaft").large(),
                    c("design.spur_gear").large(),
                    split("Gears", items(&["design.worm_gear", "design.bevel_gear"])),
                    c("design.bearing"),
                    c("design.key"),
                    split(
                        "Belts and Chains",
                        items(&["design.v_belts", "design.sync_belts", "design.chains"]),
                    ),
                    split(
                        "Cams",
                        items(&["design.disc_cam", "design.linear_cam", "design.cyl_cam"]),
                    ),
                    split(
                        "Splines",
                        items(&[
                            "design.involute_splines",
                            "design.parallel_splines",
                            "design.o_ring",
                        ]),
                    ),
                ],
            ),
            panel(
                "Spring",
                vec![
                    c("design.compression_spring"),
                    c("design.extension_spring"),
                    c("design.torsion_spring"),
                    c("design.belleville_spring"),
                ],
            ),
        ],
        Tab::Model => vec![
            panel(
                "Sketch",
                vec![
                    when(
                        !st.picking,
                        c("part.start_sketch"),
                        "Select a plane or face in the graphics window (Esc cancels)",
                    )
                    .large(),
                    c("part.start_3d_sketch").mini(),
                ],
            ),
            panel(
                "Create",
                vec![
                    c("part.extrude").large(),
                    c("part.revolve").large(),
                    c("part.sweep"),
                    c("part.loft"),
                    c("part.coil"),
                    c("part.rib"),
                    c("part.emboss"),
                    c("part.derive").mini(),
                    c("part.import").mini(),
                    split("Decal", items(&["part.decal", "part.mark", "part.unwrap"])).mini(),
                ],
            ),
            panel(
                "Modify",
                vec![
                    c("part.hole").large(),
                    c("part.fillet").large(),
                    c("part.chamfer"),
                    c("part.shell"),
                    c("part.draft"),
                    c("part.thread"),
                    c("part.combine"),
                    c("part.split"),
                    c("part.thicken").mini(),
                    c("part.delete_face").mini(),
                    c("part.direct").mini(),
                    c("part.slot").mini(),
                    split(
                        "Copy Object",
                        items(&["part.copy_object", "part.move_bodies", "part.bend_part"]),
                    )
                    .mini(),
                ],
            ),
            panel(
                "Work Features",
                vec![
                    c("part.work_plane").large(),
                    c("part.work_axis"),
                    c("part.work_point"),
                    c("part.ucs"),
                ],
            ),
            panel(
                "Pattern",
                vec![
                    c("part.rect_pattern"),
                    c("part.circ_pattern"),
                    c("part.mirror"),
                    c("part.sketch_pattern").mini(),
                ],
            ),
            panel(
                "Surface",
                vec![
                    c("part.stitch").mini(),
                    c("part.sculpt").mini(),
                    c("part.patch").mini(),
                    c("part.trim_surface").mini(),
                    c("part.extend_surface").mini(),
                    c("part.ruled_surface").mini(),
                    c("part.replace_face").mini(),
                    c("part.repair_bodies").mini(),
                    c("part.fit_mesh_face").mini(),
                ],
            ),
            panel(
                "Plastic Part",
                vec![
                    c("part.grill").mini(),
                    c("part.boss").mini(),
                    c("part.rest").mini(),
                    c("part.snap_fit").mini(),
                    c("part.rule_fillet").mini(),
                    c("part.lip").mini(),
                ],
            ),
            panel(
                "Freeform",
                vec![
                    split(
                        "Freeform",
                        items(&[
                            "part.ff_box",
                            "part.ff_plane",
                            "part.ff_cylinder",
                            "part.ff_sphere",
                            "part.ff_torus",
                            "part.ff_quadball",
                            "part.ff_face",
                            "part.ff_convert",
                        ]),
                    )
                    .mini(),
                ],
            ),
            panel(
                "Primitives",
                vec![
                    c("part.box").mini(),
                    c("part.cylinder").mini(),
                    c("part.sphere").mini(),
                    c("part.torus").mini(),
                ],
            ),
            panel("Convert", vec![c("sm.convert").mini()]),
        ],
        Tab::Sketch => {
            let sk = |id: &'static str| when(st.editing, c(id), in_sketch);
            let menu = |label: &'static str, ids: &[&'static str]| {
                let m = split(label, items(ids));
                if st.editing {
                    m
                } else {
                    Cmd {
                        kind: Kind::Unavailable(in_sketch),
                        ..m
                    }
                }
            };
            let mut constrain = vec![sk("sketch.dimension").large()];
            constrain.extend(ConstraintTool::ALL.iter().map(|&k| {
                when(
                    st.editing,
                    c_of(Action::Tool(Tool::Constrain(k))),
                    in_sketch,
                )
                .mini()
            }));
            constrain.push(c("sketch.smooth").mini());
            constrain.push(c("sketch.constraint_settings").mini());
            constrain.push(sk("sketch.relax").mini());
            constrain.push(sk("sketch.dof").mini());
            constrain.push(c(constraints).mini());
            constrain.push(sk("sketch.coordinate_system").mini());
            vec![
                panel(
                    "Create",
                    vec![
                        sk("sketch.line").large(),
                        menu("Circle", &["sketch.circle_center", "sketch.circle_tangent"]).large(),
                        menu(
                            "Arc",
                            &[
                                "sketch.arc_three",
                                "sketch.arc_tangent",
                                "sketch.arc_center",
                            ],
                        )
                        .large(),
                        menu(
                            "Rectangle",
                            &[
                                "sketch.rect_two",
                                "sketch.rect_three",
                                "sketch.rect_center",
                                "sketch.rect_center3",
                            ],
                        )
                        .large(),
                        menu(
                            "Slot",
                            &[
                                "sketch.slot_cc",
                                "sketch.slot_overall",
                                "sketch.slot_center",
                                "sketch.slot_arc3",
                                "sketch.slot_arc_center",
                            ],
                        ),
                        menu("Spline", &["sketch.spline_interp", "sketch.spline_cv"]),
                        c("sketch.ellipse"),
                        sk("sketch.point"),
                        menu(
                            "Project Geometry",
                            &[
                                "sketch.project",
                                "sketch.project_cut",
                                "sketch.project_flat",
                            ],
                        )
                        .large(),
                        sk("sketch.polygon"),
                        menu("Fillet", &["sketch.fillet", "sketch.chamfer"]),
                        c("sketch.text"),
                        sk("sketch.select"),
                    ],
                ),
                panel(
                    "Modify",
                    vec![
                        sk("sketch.move"),
                        sk("sketch.copy"),
                        sk("sketch.rotate"),
                        sk("sketch.trim"),
                        sk("sketch.extend"),
                        sk("sketch.split"),
                        sk("sketch.offset"),
                        sk("sketch.scale"),
                        sk("sketch.stretch"),
                    ],
                ),
                panel(
                    "Pattern",
                    vec![
                        sk("sketch.rect_pattern"),
                        sk("sketch.circ_pattern"),
                        sk("sketch.mirror"),
                        sk("sketch.path_pattern").mini(),
                        sk("sketch.point_pattern").mini(),
                    ],
                ),
                panel("Constrain", constrain),
                panel(
                    "Format",
                    vec![
                        sk("sketch.construction").mini(),
                        sk("sketch.centerline").mini(),
                        sk("sketch.normal").mini(),
                        sk("sketch.center_point").mini(),
                    ],
                ),
                panel(
                    "Demo",
                    vec![split(
                        "Demo",
                        items(&[
                            "demo.rectangle",
                            "demo.slot",
                            "demo.tangents",
                            "demo.lines",
                            "demo.revolve",
                        ]),
                    )],
                ),
                panel("Exit", vec![sk("sketch.finish").large()]),
            ]
        }
        Tab::Inspect if st.assembly => vec![
            panel(
                "Measure",
                vec![c("inspect.measure").large(), c("inspect.physical")],
            ),
            panel("Interference", vec![c("inspect.interference").large()]),
            panel(
                "Analysis",
                vec![
                    c("inspect.zebra"),
                    c("inspect.draft_analysis"),
                    c("inspect.curvature"),
                    c("inspect.surface_analysis"),
                    c("inspect.section_analysis"),
                ],
            ),
        ],
        Tab::Inspect => vec![
            panel(
                "Measure",
                vec![c("inspect.measure").large(), c("inspect.physical")],
            ),
            panel(
                "Analysis",
                vec![
                    c("inspect.validate"),
                    c("inspect.zebra"),
                    c("inspect.draft_analysis"),
                    c("inspect.curvature"),
                    c("inspect.surface_analysis"),
                    c("inspect.section_analysis"),
                ],
            ),
        ],
        Tab::Tools => vec![
            panel(
                "Options",
                vec![
                    c("tools.options").large(),
                    c("tools.document_settings"),
                    c("tools.customize"),
                ],
            ),
            panel(
                "Material and Appearance",
                vec![
                    c("tools.material"),
                    c("tools.appearance"),
                    c("tools.adjust"),
                    c("tools.clear"),
                ],
            ),
            panel("Parameters", vec![c("tools.parameters").large()]),
            panel("Update", vec![c("tools.rebuild_all").large()]),
            panel("Automation", vec![c("tools.rules"), c("tools.assistant")]),
            panel("History", vec![c("tools.undo"), c("tools.redo")]),
        ],
        Tab::Manage => {
            let styles = panel(
                "Styles and Standards",
                vec![
                    c("manage.styles_editor").large(),
                    c("manage.update_styles"),
                    c("manage.purge"),
                    c("manage.save_styles"),
                ],
            );
            // Documents of the design file (spec 0.6b), in every environment.
            let docs = panel(
                "Documents",
                vec![
                    c("doc.window").large(),
                    c("doc.new_part"),
                    c("doc.new_assembly"),
                    c("doc.new_drawing"),
                    c("doc.new_presentation"),
                    c("doc.insert"),
                    c("doc.link"),
                    c("doc.duplicate"),
                    c("doc.rename"),
                    c("doc.delete"),
                ],
            );
            let storage = panel(
                "Storage",
                vec![
                    c("doc.extract"),
                    c("doc.embed"),
                    c("doc.pack"),
                    c("doc.unpack"),
                ],
            );
            if st.drawing {
                vec![
                    docs,
                    storage,
                    panel("Update", vec![c("drw.update_all").large()]),
                    styles,
                    panel("Modify", vec![c("drw.replace_model").large()]),
                    panel(
                        "Define",
                        vec![
                            c("drw.define_border"),
                            c("drw.define_title"),
                            c("drw.define_symbol"),
                        ],
                    ),
                ]
            } else if !st.assembly {
                vec![
                    docs,
                    storage,
                    styles,
                    panel(
                        "Author",
                        vec![
                            c("manage.extract_ifeature"),
                            c("manage.create_ipart"),
                            c("manage.author_frame_profile"),
                        ],
                    ),
                    panel(
                        "Insert",
                        vec![c("manage.insert_ifeature"), c("manage.insert_object")],
                    ),
                    panel(
                        "Layout",
                        vec![c("manage.make_part"), c("manage.make_components")],
                    ),
                ]
            } else {
                vec![
                    docs,
                    storage,
                    styles,
                    panel("Author", vec![c("manage.create_iassembly").large()]),
                ]
            }
        }
        Tab::Environments if st.assembly => vec![panel(
            "Assembly",
            vec![
                c("env.tube_pipe").large(),
                c("env.cable_harness").large(),
                c("env.dynamic_sim").large(),
                c("env.asm_stress").large(),
                c("env.asm_studio").large(),
                c("env.presentation").large(),
            ],
        )],
        Tab::Environments => vec![panel(
            "Begin",
            vec![
                c("env.studio").large(),
                c("env.stress").large(),
                c("env.shape_generator").large(),
                c("env.print3d").large(),
                c("env.bim").large(),
            ],
        )],
        Tab::View => vec![
            panel(
                "Visibility",
                vec![
                    split(
                        "Object Visibility",
                        items(&[
                            "view.vis_origin_planes",
                            "view.vis_work_planes",
                            "view.vis_work_axes",
                            "view.vis_work_points",
                            "view.vis_ucs",
                        ]),
                    ),
                    c(constraints).label("Constraints"),
                    if st.assembly {
                        c("view.dof")
                    } else {
                        c("sketch.dof").label("Degrees of Freedom")
                    },
                    c("view.cog"),
                    c("view.analysis_visibility"),
                ],
            ),
            panel(
                "Navigate",
                vec![
                    c("view.home").large(),
                    c("view.look_at"),
                    c("view.front"),
                    c("view.top"),
                    c("view.right"),
                ],
            ),
            panel(
                "Appearance",
                vec![
                    split(
                        "Visual Style",
                        items(&[
                            "view.vs_shaded",
                            "view.visual_style",
                            "view.vs_hidden_edges",
                            "view.vs_wireframe",
                            "view.vs_wire_hidden",
                            "view.vs_wire_visible",
                            "view.vs_monochrome",
                        ]),
                    ),
                    split(
                        "Section View",
                        items(&[
                            "view.section_quarter",
                            "view.section",
                            "view.section_three_quarter",
                            "view.section_end",
                        ]),
                    ),
                    c("view.shadows").mini(),
                    c("view.reflections").mini(),
                    c("view.ground_plane").mini(),
                    c("view.ray_tracing").mini(),
                    split(
                        "Projection",
                        items(&[
                            "view.orthographic",
                            "view.perspective",
                            "view.perspective_ortho",
                        ]),
                    ),
                ],
            ),
            panel(
                "Windows",
                vec![
                    split(
                        "User Interface",
                        items(&[
                            "view.browser",
                            "view.properties",
                            "view.ribbon",
                            "view.status_bar",
                            "view.view_cube",
                            "view.triad",
                        ]),
                    ),
                    c("view.clean_screen"),
                    c("view.full_screen"),
                    c("view.reset_layout"),
                ],
            ),
        ],
    }
}

/// Every ribbon button and split-button variant as (tab, panel, registry
/// id), in all states that change what is shown.
#[cfg(test)]
pub(super) fn all_buttons() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut out = Vec::new();
    for (editing, drawing, assembly) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        for labels_shown in [false, true] {
            let st = State {
                labels_shown,
                editing,
                picking: false,
                tool: Tool::Select,
                construction: false,
                show_dof: false,
                relax: false,
                ui_shown: [true; 6],
                obj_vis: Default::default(),
                clean: false,
                full_screen: false,
                project: false,
                center_point: false,
                drawing_sketch: false,
                assembly,
                drawing,
                asm_tool: None,
                flat: false,
            };
            for tab in Tab::ALL {
                for p in panels(tab, &st) {
                    for cmd in &p.cmds {
                        if let Kind::Menu(items) = &cmd.kind {
                            for i in items {
                                out.push((tab.label(), p.title, i.id));
                            }
                        } else if !cmd.id.is_empty() {
                            out.push((tab.label(), p.title, cmd.id));
                        }
                    }
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// In an assembly or a drawing, the part's commands of the shared tabs
/// (Inspect, Tools…) are greyed with the reason; a drawing's Sketch tab
/// keeps what its view sketch takes.
fn for_env(mut panels: Vec<PanelDef>, st: &State, tab: Tab) -> Vec<PanelDef> {
    if st.drawing && tab == Tab::Sketch {
        for p in &mut panels {
            for cmd in &mut p.cmds {
                if let Kind::Ready(a, _) = cmd.kind
                    && !super::drawing_sketch::view_sketch_takes(a)
                {
                    cmd.kind = Kind::Unavailable("Not available in a drawing sketch yet");
                }
            }
        }
        return panels;
    }
    let (env, why) = if st.drawing {
        (commands::Env::Drawing, "Not available in a drawing")
    } else if st.assembly {
        (commands::Env::Assembly, "Not available in an assembly")
    } else {
        return panels;
    };
    for p in &mut panels {
        for cmd in &mut p.cmds {
            let valid = commands::by_id(cmd.id).is_none_or(|c| c.valid_in(env));
            if !valid && matches!(cmd.kind, Kind::Ready(..)) {
                cmd.kind = Kind::Unavailable(why);
            }
        }
    }
    panels
}

/// Height of a small row; three rows make a panel.
const ROW: f32 = 22.0;
const PANEL_H: f32 = ROW * 3.0 + 4.0;

/// Draws the tab strip and the active tab's panels; returns the clicked
/// command, if any.
pub fn show(ui: &mut egui::Ui, tab: &mut Tab, st: &State, fold: &mut Fold) -> Option<Action> {
    ui.horizontal(|ui| {
        let tabs: Vec<Tab> = Tab::ALL
            .into_iter()
            .filter(|t| t.visible(st, &fold.hidden))
            .collect();
        for t in tabs {
            let was = *tab == t;
            let r = ui.selectable_value(tab, t, RichText::new(t.label()).strong());
            r.context_menu(|ui| tabs_menu(ui, st, fold));
            if r.double_clicked() && was {
                // Inventor: a double click on the active tab minimizes.
                fold.minimized = !fold.minimized;
                fold.peek = false;
            } else if r.clicked() && fold.minimized {
                fold.peek = !(was && fold.peek);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (glyph, tip) = if fold.minimized {
                ("v", "Show the full ribbon")
            } else {
                (
                    "^",
                    "Minimize the ribbon to its tabs (or double-click the active tab)",
                )
            };
            if ui.small_button(glyph).on_hover_text(tip).clicked() {
                fold.minimized = !fold.minimized;
                fold.peek = false;
            }
        });
    });
    if fold.minimized && !fold.peek {
        return None;
    }
    ui.separator();

    let mut clicked = None;
    let panels = for_env(panels(*tab, st), st, *tab);
    // Inventor collapses the rightmost panels into one button each when
    // the window is too narrow (the panel's commands drop down from it).
    // Widths are those measured when a panel was last drawn open.
    let widths_id = egui::Id::new(("ribbon_panel_widths", tab.label()));
    let mut widths: Vec<f32> = ui
        .ctx()
        .data(|d| d.get_temp::<Vec<f32>>(widths_id))
        .unwrap_or_default();
    widths.resize(panels.len(), 0.0);
    let titles: Vec<&str> = panels.iter().map(|p| p.title).collect();
    let collapsed = collapsed_panels(&widths, &titles, ui.available_width());
    egui::ScrollArea::horizontal().show(ui, |ui| {
        ui.horizontal_top(|ui| {
            for (k, p) in panels.iter().enumerate() {
                if collapsed[k] {
                    if let Some(a) = collapsed_panel(ui, p, st) {
                        clicked = Some(a);
                    }
                    ui.separator();
                    continue;
                }
                let x0 = ui.cursor().min.x;
                ui.vertical(|ui| {
                    ui.horizontal_top(|ui| {
                        ui.set_min_height(PANEL_H);
                        if let Some(a) = panel_body(ui, p, st) {
                            clicked = Some(a);
                        }
                    });
                    ui.label(RichText::new(p.title).small().weak());
                });
                ui.separator();
                // The whole span: the panel, its separator and the spacing.
                widths[k] = ui.cursor().min.x - x0;
            }
        });
    });
    ui.ctx().data_mut(|d| d.insert_temp(widths_id, widths));
    clicked
}

/// Width of a collapsed panel (its button, separator and spacing).
const COLLAPSED_W: f32 = 76.0;

/// Panels that collapse first (rarely used), and panels that collapse
/// only when nothing else is left (the everyday ones).
const COLLAPSE_FIRST: &[&str] = &[
    "Demo",
    "Primitives",
    "Convert",
    "Freeform",
    "Plastic Part",
    "Surface",
    "Layout",
    "Insert",
    "Author",
    "Format",
];
const COLLAPSE_LAST: &[&str] = &["Sketch", "Create", "Modify", "Constrain", "Exit"];

/// Which panels are shown collapsed so that the row fits `avail`: starting
/// from everything folded, panels open by importance — the everyday ones
/// (`COLLAPSE_LAST`) first, then the others, the rarely used ones
/// (`COLLAPSE_FIRST`) last, each group from the left — while they fit.
/// Never the first panel, never one narrower than its folded button.
/// `widths` are the spans of the open panels (with their separators); a
/// panel not measured yet (0) counts as open.
fn collapsed_panels(widths: &[f32], titles: &[&str], avail: f32) -> Vec<bool> {
    let n = widths.len();
    let title = |k: usize| titles.get(k).copied().unwrap_or("");
    let foldable = |k: usize| k > 0 && widths[k] > COLLAPSED_W;
    let mut out: Vec<bool> = (0..n).map(foldable).collect();
    let mut total: f32 = (0..n)
        .map(|k| if out[k] { COLLAPSED_W } else { widths[k] })
        .sum();
    let groups: [&dyn Fn(&str) -> bool; 3] = [
        &|t| COLLAPSE_LAST.contains(&t),
        &|t| !COLLAPSE_LAST.contains(&t) && !COLLAPSE_FIRST.contains(&t),
        &|t| COLLAPSE_FIRST.contains(&t),
    ];
    for group in groups {
        for k in 0..n {
            let grow = widths[k] - COLLAPSED_W;
            if out[k] && group(title(k)) && total + grow <= avail {
                out[k] = false;
                total += grow;
            }
        }
    }
    out
}

/// The commands of a panel: large buttons stand alone; runs of small /
/// mini buttons fill columns of three.
fn panel_body(ui: &mut egui::Ui, p: &PanelDef, st: &State) -> Option<Action> {
    let mut clicked = None;
    let mut i = 0;
    while i < p.cmds.len() {
        if p.cmds[i].size == Size::Large {
            if let Some(a) = command_button(ui, &p.cmds[i], st) {
                clicked = Some(a);
            }
            i += 1;
            continue;
        }
        let size = p.cmds[i].size;
        let start = i;
        while i < p.cmds.len() && i - start < 3 && p.cmds[i].size == size {
            i += 1;
        }
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            for cmd in &p.cmds[start..i] {
                if let Some(a) = command_button(ui, cmd, st) {
                    clicked = Some(a);
                }
            }
        });
    }
    clicked
}

/// A collapsed panel: one button with the panel's first icon and its title;
/// a click drops the whole panel down.
fn collapsed_panel(ui: &mut egui::Ui, p: &PanelDef, st: &State) -> Option<Action> {
    let mut clicked = None;
    let icon = p.cmds.first().map_or(Icon::Settings, |c| c.icon);
    // The same rows as an open panel: the button (icon and arrow, full
    // height), then the title.
    let r = ui.vertical(|ui| {
        let b = ui
            .horizontal_top(|ui| {
                ui.set_min_height(PANEL_H);
                icon_button(ui, icon, "", Size::Large, true, false, true)
                    .on_hover_text(format!("{}: click for its commands", p.title))
            })
            .inner;
        ui.label(RichText::new(p.title).small().weak());
        b
    });
    egui::Popup::menu(&r.inner)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.horizontal_top(|ui| {
                ui.set_min_height(PANEL_H);
                clicked = panel_body(ui, p, st);
            });
            if clicked.is_some() {
                ui.close();
            }
        });
    clicked
}

/// Splits a long label of a large button into two lines.
fn two_lines(label: &str) -> String {
    if label.len() <= 10 {
        return label.to_owned();
    }
    let mid = label.len() / 2;
    let split = label
        .match_indices(' ')
        .map(|(i, _)| i)
        .min_by_key(|&i| i.abs_diff(mid));
    match split {
        Some(i) => format!("{}\n{}", &label[..i], &label[i + 1..]),
        None => label.to_owned(),
    }
}

/// A ribbon button: icon (and label) in the command's size. Disabled
/// buttons still report hover (for the tooltip) but no clicks.
pub(super) fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    size: Size,
    enabled: bool,
    selected: bool,
    arrow: bool,
) -> egui::Response {
    icon_button_in(ui, icon, label, size, enabled, false, selected, arrow)
}

/// [`icon_button`] whose icon is drawn grey while the button stays
/// clickable when `grey` (a command with its interface only, rule 18).
#[allow(clippy::too_many_arguments)]
pub(super) fn icon_button_in(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    size: Size,
    enabled: bool,
    grey: bool,
    selected: bool,
    arrow: bool,
) -> egui::Response {
    let text_color = if enabled {
        ui.visuals().text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let font = egui::FontId::proportional(if size == Size::Large { 11.5 } else { 12.5 });
    let text = match size {
        Size::Large => two_lines(label),
        Size::Small => label.to_owned(),
        Size::Mini => String::new(),
    };
    let galley = ui.painter().layout_no_wrap(text, font, text_color);
    let arrow_w = if arrow { 9.0 } else { 0.0 };
    let (icon_px, desired) = match size {
        Size::Large => (
            30.0,
            Vec2::new((galley.size().x + 10.0).max(46.0) + arrow_w * 0.0, PANEL_H),
        ),
        Size::Small => (16.0, Vec2::new(22.0 + galley.size().x + 6.0 + arrow_w, ROW)),
        Size::Mini => (16.0, Vec2::new(24.0 + arrow_w, ROW)),
    };
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(desired, sense);
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter();
    let v = ui.visuals();
    if selected {
        painter.rect_filled(rect, 3.0, v.selection.bg_fill);
    } else if enabled && response.hovered() {
        painter.rect_filled(rect, 3.0, v.widgets.hovered.weak_bg_fill);
    }
    let palette = if enabled && !grey {
        Palette::ENABLED
    } else {
        Palette::DISABLED
    };
    match size {
        Size::Large => {
            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(rect.center().x, rect.top() + 4.0 + icon_px / 2.0),
                Vec2::splat(icon_px),
            );
            icons::paint(painter, icon_rect, icon, &palette);
            let pos = egui::pos2(
                rect.center().x - galley.size().x / 2.0,
                icon_rect.bottom() + 3.0,
            );
            painter.galley(pos, galley, text_color);
            if arrow {
                let y = rect.bottom() - 6.0;
                let x = rect.center().x;
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        egui::pos2(x - 3.0, y - 2.0),
                        egui::pos2(x + 3.0, y - 2.0),
                        egui::pos2(x, y + 1.0),
                    ],
                    text_color,
                    egui::Stroke::NONE,
                ));
            }
        }
        Size::Small | Size::Mini => {
            let icon_rect = egui::Rect::from_min_size(
                egui::pos2(rect.left() + 3.0, rect.center().y - icon_px / 2.0),
                Vec2::splat(icon_px),
            );
            icons::paint(painter, icon_rect, icon, &palette);
            let pos = egui::pos2(
                icon_rect.right() + 4.0,
                rect.center().y - galley.size().y / 2.0,
            );
            painter.galley(pos, galley, text_color);
            if arrow {
                let x = rect.right() - 6.0;
                let y = rect.center().y;
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        egui::pos2(x - 3.0, y - 1.5),
                        egui::pos2(x + 3.0, y - 1.5),
                        egui::pos2(x, y + 1.5),
                    ],
                    text_color,
                    egui::Stroke::NONE,
                ));
            }
        }
    }
    response
}

fn command_button(ui: &mut egui::Ui, cmd: &Cmd, st: &State) -> Option<Action> {
    let active = st.tool;
    // The user's key (Customize) replaces the default one in the tooltip.
    let key = match super::keymap::tip_keys(ui.ctx(), cmd.id) {
        Some(user) => user,
        None => cmd.key.map(str::to_owned),
    }
    .map(|k| format!(" ({k})"))
    .unwrap_or_default();
    // Mini buttons carry their name in the tooltip.
    let name = |rest: &str| {
        if cmd.size == Size::Mini {
            format!("{}\n{rest}", cmd.label)
        } else {
            rest.to_owned()
        }
    };
    match &cmd.kind {
        Kind::Ready(action, help) => {
            // Tools show as pressed while active, toggles while on.
            let selected = match action {
                Action::Tool(t) => *t == active && *t != Tool::Select,
                Action::Format(Style::Construction) => st.construction,
                Action::ToggleRelax => st.relax,
                Action::ProjectGeometry => st.project,
                Action::CenterPoint => st.center_point,
                Action::ToggleDof => st.show_dof,
                Action::SmFlatPattern => st.flat,
                Action::AsmFreeMove | Action::AsmFreeRotate | Action::AsmPlace => {
                    st.asm_tool == Some(*action)
                }
                Action::Ui(UiCmd::CleanScreen) => st.clean,
                Action::Ui(UiCmd::FullScreen) => st.full_screen,
                _ => false,
            };
            let grey = commands::interface_only(*action);
            let help = if grey {
                format!("{help}\nInterface only: the logic comes later{key}")
            } else {
                format!("{help}{key}")
            };
            let r = icon_button_in(
                ui, cmd.icon, cmd.label, cmd.size, true, grey, selected, false,
            )
            .on_hover_text(name(&help));
            // Right click: Assign Shortcut and the other key items.
            super::keymap::button_menu(&r, cmd.id);
            r.clicked().then_some(*action)
        }
        Kind::Menu(items) => {
            let current = items
                .iter()
                .find(|i| matches!(i.action, Action::Tool(t) if t == active));
            let icon = current.map_or(cmd.icon, |c| c.icon);
            let title = match current {
                Some(i) if cmd.size != Size::Large => format!("{}: {}", cmd.label, i.label),
                _ => cmd.label.to_owned(),
            };
            // Grey when none of its variants has logic yet (rule 18).
            let grey = items.iter().all(|i| commands::interface_only(i.action));
            let r = icon_button_in(
                ui,
                icon,
                &title,
                cmd.size,
                true,
                grey,
                current.is_some(),
                true,
            )
            .on_hover_text(name("Click for the variants"));
            let mut picked = None;
            egui::Popup::menu(&r).show(|ui| {
                for Item {
                    id,
                    label,
                    icon,
                    action,
                } in items
                {
                    // User Interface toggles show as pressed while visible.
                    let on = match action {
                        Action::Ui(UiCmd::Toggle(e)) => Element::ALL
                            .iter()
                            .position(|x| x == e)
                            .is_some_and(|k| st.ui_shown[k]),
                        Action::Ui(UiCmd::ObjVis(k)) => st.obj_vis.get(*k),
                        _ => false,
                    };
                    let grey = commands::interface_only(*action);
                    let b = icon_button_in(ui, *icon, label, Size::Small, true, grey, on, false);
                    super::keymap::button_menu(&b, id);
                    if b.clicked() {
                        picked = Some(*action);
                        ui.close();
                    }
                }
            });
            picked
        }
        Kind::Unavailable(why) => {
            icon_button(ui, cmd.icon, cmd.label, cmd.size, false, false, false)
                .on_hover_text(name(&format!("{why}{key}")));
            None
        }
        &Kind::Planned(section) => {
            icon_button(ui, cmd.icon, cmd.label, cmd.size, false, false, false).on_hover_text(
                name(&format!(
                    "Planned — docs/FEATURES.md, section {section}{key}"
                )),
            );
            None
        }
    }
}

#[cfg(test)]
mod collapse_tests {
    /// Rarely used panels collapse first, then the others from the right,
    /// until the row fits; narrow ones stay open; unmeasured ones count as
    /// open.
    #[test]
    fn rightmost_panels_collapse_first() {
        let w = [90.0, 350.0, 430.0, 250.0, 140.0, 140.0, 85.0, 60.0];
        let t = [
            "Sketch",
            "Create",
            "Modify",
            "Work Features",
            "Pattern",
            "Surface",
            "Primitives",
            "Convert",
        ];
        // Surface and Primitives first, then Pattern and Work Features
        // from the right; Work Features made room for the others again.
        let c = super::collapsed_panels(&w, &t, 1390.0);
        assert_eq!(
            c,
            vec![false, false, false, true, false, false, false, false]
        );
        // Narrower still: Modify has to fold, and then the others fit.
        let c = super::collapsed_panels(&w, &t, 1230.0);
        assert_eq!(
            c,
            vec![false, false, true, false, false, false, false, false]
        );
        // Surface folded; Primitives (9 px wider open) fits again.
        let c = super::collapsed_panels(&w, &t, 1500.0);
        assert_eq!(
            c,
            vec![false, false, false, false, false, true, false, false]
        );
        let c = super::collapsed_panels(&w, &t, 1700.0);
        assert!(c.iter().all(|x| !x));
        assert!(
            super::collapsed_panels(&[0.0; 5], &t, 100.0)
                .iter()
                .all(|x| !x)
        );
    }

    /// A sketch-like row: Constrain (everyday) opens before Pattern and the
    /// rarely used Format and Demo to its right, which fold instead (Insert
    /// is only 24 px wider open and still fits).
    #[test]
    fn everyday_panels_open_before_rare_ones() {
        let w = [380.0, 100.0, 240.0, 140.0, 300.0, 110.0, 90.0, 70.0];
        let t = [
            "Create",
            "Insert",
            "Modify",
            "Pattern",
            "Constrain",
            "Format",
            "Demo",
            "Exit",
        ];
        let c = super::collapsed_panels(&w, &t, 1320.0);
        assert_eq!(c, vec![false, false, false, true, false, true, true, false]);
    }
}
