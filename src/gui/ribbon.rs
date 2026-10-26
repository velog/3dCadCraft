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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fold {
    /// Only the tab strip is shown.
    pub minimized: bool,
    /// Minimized, but the panels are shown until a command is picked.
    pub peek: bool,
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
    /// The active part is a sheet metal part.
    pub sheet_metal: bool,
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
    /// Sheet metal parts: Face, Flange, Flat Pattern.
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
}

impl Tab {
    const ALL: [Tab; 10] = [
        Tab::PlaceViews,
        Tab::Annotate,
        Tab::Assemble,
        Tab::SheetMetal,
        Tab::Model,
        Tab::Sketch,
        Tab::Inspect,
        Tab::Tools,
        Tab::Manage,
        Tab::View,
    ];

    /// Whether the tab belongs to the environment (an assembly has no
    /// 3D Model / Sketch tabs, a part no Assemble tab, a drawing only its
    /// own tabs and the shared Tools / Manage / View).
    pub fn shown(self, assembly: bool, sheet_metal: bool, drawing: bool) -> bool {
        match self {
            Tab::PlaceViews | Tab::Annotate => drawing,
            Tab::Inspect if drawing => false,
            Tab::Assemble => assembly && !drawing,
            Tab::SheetMetal => sheet_metal && !assembly && !drawing,
            Tab::Model | Tab::Sketch => !assembly && !drawing,
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
            Tab::SheetMetal => "Sheet Metal",
            Tab::Model => "3D Model",
            Tab::Sketch => "Sketch",
            Tab::Inspect => "Inspect",
            Tab::Tools => "Tools",
            Tab::Manage => "Manage",
            Tab::View => "View",
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
    #[cfg_attr(not(test), allow(dead_code))]
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
    #[cfg_attr(not(test), allow(dead_code))]
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
                    c("drw.revision_tag"),
                    c("drw.revision_table"),
                    c("drw.revision_cloud"),
                ],
            ),
            panel("Format", vec![c("drw.edit_layers")]),
        ],
        Tab::SheetMetal => vec![
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
                    c("sm.corner_seam"),
                    c("sm.corner_round"),
                    c("sm.corner_chamfer"),
                    c("sm.punch"),
                    c("sm.unfold"),
                    c("sm.refold"),
                ],
            ),
            panel(
                "Flat Pattern",
                vec![c("sm.flat_pattern").large(), c("sm.export_flat")],
            ),
        ],
        Tab::Assemble => vec![
            panel("Component", vec![c("asm.place").large(), c("asm.create")]),
            panel(
                "Position",
                vec![c("asm.free_move"), c("asm.free_rotate"), c("asm.ground")],
            ),
            panel(
                "Relationships",
                vec![c("asm.constrain").large(), c("asm.joint").large()],
            ),
            panel(
                "Pattern",
                vec![c("asm.pattern"), c("asm.mirror"), c("asm.copy")],
            ),
            panel(
                "Manage",
                vec![c("asm.update").large(), c("asm.bom").large()],
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
                    c("part.slot"),
                    c("part.combine"),
                    c("part.split"),
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
                ],
            ),
            panel(
                "Primitives",
                vec![
                    c("part.box").large(),
                    c("part.cylinder"),
                    c("part.sphere"),
                    c("part.torus"),
                ],
            ),
            panel("Convert", vec![c("sm.convert")]),
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
                        sk("sketch.project").mini(),
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
        Tab::Inspect => vec![
            panel(
                "Measure",
                vec![c("inspect.measure").large(), c("inspect.physical")],
            ),
            panel(
                "Analysis",
                vec![
                    c("inspect.validate"),
                    c("inspect.interference"),
                    c("inspect.curvature"),
                    c("inspect.draft_analysis"),
                ],
            ),
        ],
        Tab::Tools => vec![
            panel(
                "Options",
                vec![c("tools.options").large(), c("tools.document_settings")],
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
            if st.drawing {
                vec![
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
            } else {
                vec![styles]
            }
        }
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
                    c("sketch.dof").label("Degrees of Freedom"),
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
                    c("view.visual_style"),
                    c("view.section"),
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
    for (editing, drawing) in [(false, false), (true, false), (false, true)] {
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
                assembly: false,
                drawing,
                asm_tool: None,
                sheet_metal: true,
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
/// (Inspect, Tools…) are greyed with the reason.
fn for_env(mut panels: Vec<PanelDef>, st: &State) -> Vec<PanelDef> {
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
        for t in Tab::ALL
            .into_iter()
            .filter(|t| t.shown(st.assembly, st.sheet_metal, st.drawing))
        {
            let was = *tab == t;
            let r = ui.selectable_value(tab, t, RichText::new(t.label()).strong());
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
    egui::ScrollArea::horizontal().show(ui, |ui| {
        ui.horizontal_top(|ui| {
            for p in for_env(panels(*tab, st), st) {
                ui.vertical(|ui| {
                    ui.horizontal_top(|ui| {
                        ui.set_min_height(PANEL_H);
                        // Large buttons stand alone; runs of small / mini
                        // buttons fill columns of three.
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
                    });
                    ui.label(RichText::new(p.title).small().weak());
                });
                ui.separator();
            }
        });
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
    let palette = if enabled {
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
    let key = cmd.key.map(|k| format!(" ({k})")).unwrap_or_default();
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
            let r = icon_button(ui, cmd.icon, cmd.label, cmd.size, true, selected, false)
                .on_hover_text(name(&format!("{help}{key}")));
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
            let r = icon_button(ui, icon, &title, cmd.size, true, current.is_some(), true)
                .on_hover_text(name("Click for the variants"));
            let mut picked = None;
            egui::Popup::menu(&r).show(|ui| {
                for Item {
                    label,
                    icon,
                    action,
                    ..
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
                    if icon_button(ui, *icon, label, Size::Small, true, on, false).clicked() {
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
