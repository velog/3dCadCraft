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

use super::demos::Demo;
use super::icons::{self, Icon, Palette};
use super::layout::{Element, UiCmd};
use super::material::MatCmd;
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
    /// Face Draft: tilt faces against a pull direction.
    Draft,
    /// Rectangular Pattern of features or solids.
    RectPattern,
    /// Circular Pattern of features or solids.
    CircPattern,
    /// Mirror of features or solids across a plane.
    MirrorFeature,
    /// Hole: drilled, clearance and tapped holes with seats.
    Hole,
    /// Thread: a cosmetic or modeled thread on a cylindrical face.
    Thread,
    /// Work Axis: the method follows from the picks.
    WorkAxis,
    /// Work Point: the method follows from the picks.
    WorkPoint,
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
    /// Clean Screen on.
    pub clean: bool,
    /// Full Screen on.
    pub full_screen: bool,
}

/// Ribbon tabs of the part environment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
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
    const ALL: [Tab; 6] = [
        Tab::Model,
        Tab::Sketch,
        Tab::Inspect,
        Tab::Tools,
        Tab::Manage,
        Tab::View,
    ];

    fn label(self) -> &'static str {
        match self {
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

/// One ribbon command.
struct Cmd {
    label: &'static str,
    icon: Icon,
    size: Size,
    /// Keyboard shortcut shown in the tooltip (Inventor defaults).
    key: Option<&'static str>,
    kind: Kind,
}

enum Kind {
    /// Implemented: action and tooltip.
    Ready(Action, &'static str),
    /// Split button: a menu of variants (label, icon, action).
    Menu(Vec<(&'static str, Icon, Action)>),
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
}

const fn todo(
    icon: Icon,
    label: &'static str,
    key: Option<&'static str>,
    section: &'static str,
) -> Cmd {
    Cmd {
        label,
        icon,
        size: Size::Small,
        key,
        kind: Kind::Planned(section),
    }
}

const fn ready(
    icon: Icon,
    label: &'static str,
    key: Option<&'static str>,
    action: Action,
    help: &'static str,
) -> Cmd {
    Cmd {
        label,
        icon,
        size: Size::Small,
        key,
        kind: Kind::Ready(action, help),
    }
}

/// `ready` when `available`, otherwise disabled with `why`.
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

/// Icon of a sketch tool (also used in split-button menus).
pub fn tool_icon(t: Tool) -> Icon {
    use super::sketch_tools::ConstraintTool as C;
    match t {
        Tool::Select => Icon::Select,
        Tool::Line => Icon::Line,
        Tool::CircleCenter => Icon::CircleCenter,
        Tool::CircleTangent => Icon::CircleTangent,
        Tool::ArcThreePoint => Icon::ArcThree,
        Tool::ArcTangent => Icon::ArcTangent,
        Tool::ArcCenter => Icon::ArcCenter,
        Tool::RectTwoPoint => Icon::Rect2,
        Tool::RectThreePoint => Icon::Rect3,
        Tool::RectCenter => Icon::RectCenter,
        Tool::RectCenterThreePoint => Icon::RectCenter3,
        Tool::SlotCenterToCenter => Icon::SlotCenter,
        Tool::SlotOverall => Icon::SlotOverall,
        Tool::SlotCenterPoint => Icon::SlotPoint,
        Tool::SlotArcThreePoint | Tool::SlotArcCenter => Icon::SlotArc,
        Tool::Polygon => Icon::Polygon,
        Tool::SplineCv => Icon::Spline,
        Tool::SplineInterp => Icon::SplineInterp,
        Tool::Point => Icon::Point,
        Tool::Fillet => Icon::Fillet2d,
        Tool::Chamfer => Icon::Chamfer2d,
        Tool::Dimension => Icon::Dimension,
        Tool::Trim => Icon::Trim,
        Tool::Extend => Icon::Extend,
        Tool::Split => Icon::Split,
        Tool::Offset => Icon::Offset,
        Tool::Move => Icon::Move,
        Tool::Copy => Icon::Copy,
        Tool::Rotate => Icon::Rotate,
        Tool::Mirror => Icon::Mirror,
        Tool::PatternRect => Icon::PatternRect,
        Tool::PatternCircular => Icon::PatternCirc,
        Tool::PatternPoints => Icon::PatternPoints,
        Tool::PatternPath => Icon::PatternPath,
        Tool::Scale => Icon::Scale,
        Tool::Stretch => Icon::Stretch,
        Tool::Constrain(c) => match c {
            C::Coincident => Icon::Coincident,
            C::Collinear => Icon::Collinear,
            C::Concentric => Icon::Concentric,
            C::Fix => Icon::Fix,
            C::Parallel => Icon::Parallel,
            C::Perpendicular => Icon::Perpendicular,
            C::Horizontal => Icon::Horizontal,
            C::Vertical => Icon::Vertical,
            C::Tangent => Icon::Tangent,
            C::Symmetric => Icon::Symmetric,
            C::Equal => Icon::Equal,
        },
    }
}

fn panels(tab: Tab, st: &State) -> Vec<PanelDef> {
    let in_sketch = "Only while editing a sketch";
    match tab {
        Tab::Model => vec![
            panel(
                "Sketch",
                vec![
                    when(
                        !st.picking,
                        ready(
                            Icon::StartSketch,
                            "Start 2D Sketch",
                            Some("S"),
                            Action::StartSketch,
                            "Pick an origin plane or a planar face to sketch on",
                        ),
                        "Select a plane or face in the graphics window (Esc cancels)",
                    )
                    .large(),
                ],
            ),
            panel(
                "Create",
                vec![
                    ready(
                        Icon::Extrude,
                        "Extrude",
                        Some("E"),
                        Action::Extrude,
                        "Extrude closed sketch regions into a solid",
                    )
                    .large(),
                    ready(
                        Icon::Revolve,
                        "Revolve",
                        Some("R"),
                        Action::Revolve,
                        "Revolve closed sketch regions around an axis",
                    )
                    .large(),
                    todo(Icon::Sweep, "Sweep", None, "5.1"),
                    todo(Icon::Loft, "Loft", None, "5.2"),
                    todo(Icon::Coil, "Coil", None, "5.3"),
                    todo(Icon::Rib, "Rib", None, "2.7"),
                    todo(Icon::Emboss, "Emboss", None, "2.17"),
                ],
            ),
            panel(
                "Modify",
                vec![
                    ready(
                        Icon::Hole,
                        "Hole",
                        Some("H"),
                        Action::Hole,
                        "Hole: click a face, sketch points or a circular edge for the centres (Simple, Clearance, Tapped; Counterbore, Spotface, Countersink)",
                    )
                    .large(),
                    ready(
                        Icon::FilletEdge,
                        "Fillet",
                        None,
                        Action::Fillet,
                        "Fillet: round the picked edges (convex: cut, concave: add)",
                    )
                    .large(),
                    ready(
                        Icon::ChamferEdge,
                        "Chamfer",
                        None,
                        Action::Chamfer,
                        "Chamfer: bevel the picked edges (Distance, Two Distances, Distance and Angle)",
                    ),
                    ready(
                        Icon::Shell,
                        "Shell",
                        None,
                        Action::Shell,
                        "Shell: hollow the solid, remove the picked faces (Inside, Outside, Both)",
                    ),
                    ready(
                        Icon::Draft,
                        "Draft",
                        None,
                        Action::Draft,
                        "Face Draft: tilt faces against a pull direction (Fixed Edge, Fixed Plane)",
                    ),
                    ready(
                        Icon::Thread,
                        "Thread",
                        None,
                        Action::Thread,
                        "Thread: click a cylindrical face near the end the thread starts at (cosmetic or modeled)",
                    ),
                    todo(Icon::SlotFeature, "Slot", None, "2.3"),
                    todo(Icon::Combine, "Combine", None, "2.13"),
                    todo(Icon::SplitBody, "Split", None, "2.13"),
                ],
            ),
            panel(
                "Work Features",
                vec![
                    ready(
                        Icon::WorkPlane,
                        "Plane",
                        None,
                        Action::WorkPlane,
                        "Work Plane: pick a plane (offset), two planes (midplane), \
                         a plane and an edge (angle), a plane and a point, or three points",
                    )
                    .large(),
                    ready(
                        Icon::WorkAxis,
                        "Axis",
                        None,
                        Action::WorkAxis,
                        "Work Axis: an edge, two points, two planes, a plane and a point, \
                         a line and a point, a circular edge or a revolved face",
                    ),
                    ready(
                        Icon::WorkPoint,
                        "Point",
                        None,
                        Action::WorkPoint,
                        "Work Point: a vertex or midpoint, three planes, two lines, a plane \
                         and a line, a circular edge (centre); or Grounded at X, Y, Z",
                    ),
                    todo(Icon::Ucs, "UCS", None, "2.10"),
                ],
            ),
            panel(
                "Pattern",
                vec![
                    ready(
                        Icon::PatternRect,
                        "Rectangular",
                        None,
                        Action::RectPattern,
                        "Rectangular Pattern: copy features or solids in rows along one or two directions",
                    ),
                    ready(
                        Icon::PatternCirc,
                        "Circular",
                        None,
                        Action::CircPattern,
                        "Circular Pattern: copy features or solids at equal angles about an axis",
                    ),
                    ready(
                        Icon::Mirror,
                        "Mirror",
                        None,
                        Action::MirrorFeature,
                        "Mirror: copy features or solids mirrored across a plane",
                    ),
                ],
            ),
            panel(
                "Primitives",
                {
                    use crate::part::PrimitiveKind as P;
                    vec![
                        ready(
                            Icon::Box,
                            "Box",
                            None,
                            Action::Primitive(P::Box),
                            "Box: pick a plane, click the centre, then a corner (a sketch + Extrude)",
                        )
                        .large(),
                        ready(
                            Icon::Cylinder,
                            "Cylinder",
                            None,
                            Action::Primitive(P::Cylinder),
                            "Cylinder: pick a plane, click the centre, then the diameter (a sketch + Extrude)",
                        ),
                        ready(
                            Icon::Sphere,
                            "Sphere",
                            None,
                            Action::Primitive(P::Sphere),
                            "Sphere: pick a plane, click the centre, then the diameter (a sketch + full Revolve)",
                        ),
                        ready(
                            Icon::Torus,
                            "Torus",
                            None,
                            Action::Primitive(P::Torus),
                            "Torus: pick a plane, click the centre, the section centre, then the section diameter",
                        ),
                    ]
                },
            ),
        ],
        Tab::Sketch => {
            let tool = |t: Tool, key: Option<&'static str>, help: &'static str| {
                when(
                    st.editing,
                    ready(tool_icon(t), t.label(), key, Action::Tool(t), help),
                    in_sketch,
                )
            };
            let menu = |label: &'static str, items: &[Tool]| Cmd {
                label,
                icon: tool_icon(items[0]),
                size: Size::Small,
                key: None,
                kind: if st.editing {
                    Kind::Menu(
                        items
                            .iter()
                            .map(|&t| (t.label(), tool_icon(t), Action::Tool(t)))
                            .collect(),
                    )
                } else {
                    Kind::Unavailable(in_sketch)
                },
            };
            let mut constrain = vec![
                tool(
                    Tool::Dimension,
                    Some("D"),
                    "General Dimension: kind follows the selection and placement",
                )
                .large(),
            ];
            constrain.extend(
                ConstraintTool::ALL
                    .iter()
                    .map(|&c| tool(Tool::Constrain(c), None, "Pick the geometry in order").mini()),
            );
            constrain.push(todo(Icon::Smooth, "Smooth (G2)", None, "1.3").mini());
            constrain.push(
                ready(
                    Icon::Settings,
                    "Constraint Settings",
                    None,
                    Action::Options,
                    "Inference: which constraints are inferred and created while sketching",
                )
                .mini(),
            );
            constrain.push(
                when(
                    st.editing,
                    ready(
                        Icon::Relax,
                        "Relax Mode",
                        None,
                        Action::ToggleRelax,
                        "Relax Mode (F11): dragging may change dimensions; changed ones become driven",
                    ),
                    in_sketch,
                )
                .mini(),
            );
            constrain.push(
                when(
                    st.editing,
                    ready(
                        Icon::Dof,
                        "Show DOF",
                        None,
                        Action::ToggleDof,
                        "Show All Degrees of Freedom",
                    ),
                    in_sketch,
                )
                .mini(),
            );
            constrain.push(
                ready(
                    Icon::ShowConstraints,
                    if st.labels_shown {
                        "Hide All"
                    } else {
                        "Show All"
                    },
                    Some(if st.labels_shown { "F9" } else { "F8" }),
                    Action::ShowConstraints(!st.labels_shown),
                    "Show or hide all constraint labels",
                )
                .mini(),
            );
            constrain.push(
                when(
                    st.editing,
                    ready(
                        Icon::CoordSystem,
                        "Coordinate System",
                        None,
                        Action::EditCoordinateSystem,
                        "Edit Coordinate System: origin, axis edge and direction (Properties)",
                    ),
                    in_sketch,
                )
                .mini(),
            );
            let format = |icon: Icon, label: &'static str, style: Style, help: &'static str| {
                when(
                    st.editing,
                    ready(icon, label, None, Action::Format(style), help),
                    in_sketch,
                )
                .mini()
            };
            vec![
                panel(
                    "Create",
                    vec![
                        tool(
                            Tool::Line,
                            Some("L"),
                            "Click points; drag from a curve end for an arc",
                        )
                        .large(),
                        menu("Circle", &[Tool::CircleCenter, Tool::CircleTangent]).large(),
                        menu(
                            "Arc",
                            &[Tool::ArcThreePoint, Tool::ArcTangent, Tool::ArcCenter],
                        )
                        .large(),
                        menu(
                            "Rectangle",
                            &[
                                Tool::RectTwoPoint,
                                Tool::RectThreePoint,
                                Tool::RectCenter,
                                Tool::RectCenterThreePoint,
                            ],
                        )
                        .large(),
                        menu(
                            "Slot",
                            &[
                                Tool::SlotCenterToCenter,
                                Tool::SlotOverall,
                                Tool::SlotCenterPoint,
                                Tool::SlotArcThreePoint,
                                Tool::SlotArcCenter,
                            ],
                        ),
                        menu("Spline", &[Tool::SplineInterp, Tool::SplineCv]),
                        todo(Icon::Ellipse, "Ellipse", None, "1.2"),
                        tool(Tool::Point, None, "Sketch point"),
                        tool(
                            Tool::Polygon,
                            None,
                            "Sides and inscribed/circumscribed in Properties",
                        ),
                        menu("Fillet", &[Tool::Fillet, Tool::Chamfer]),
                        todo(Icon::Text, "Text", None, "1.2"),
                        tool(Tool::Select, Some("Esc"), "Select geometry, drag points"),
                    ],
                ),
                panel(
                    "Modify",
                    vec![
                        tool(
                            Tool::Move,
                            None,
                            "Selected geometry: base point, destination",
                        ),
                        tool(
                            Tool::Copy,
                            None,
                            "Copy selected geometry (with its constraints)",
                        ),
                        tool(
                            Tool::Rotate,
                            None,
                            "Selected geometry: centre, reference, angle",
                        ),
                        tool(
                            Tool::Trim,
                            Some("X"),
                            "Remove the clicked part up to the nearest curves (Shift = Extend)",
                        ),
                        tool(
                            Tool::Extend,
                            None,
                            "Extend to the nearest curve (Shift = Trim)",
                        ),
                        tool(Tool::Split, None, "Split at the nearest intersection"),
                        tool(Tool::Offset, Some("O"), "Offset a curve or loop"),
                        tool(
                            Tool::Scale,
                            None,
                            "Selected geometry about a base point (factor in Properties)",
                        ),
                        tool(
                            Tool::Stretch,
                            None,
                            "Move the selected geometry's points; dimensions update",
                        ),
                    ],
                ),
                panel(
                    "Pattern",
                    vec![
                        when(
                            st.editing,
                            ready(
                                Icon::PatternRect,
                                "Rectangular",
                                None,
                                Action::Tool(Tool::PatternRect),
                                "Selected geometry along one or two lines (count, spacing in Properties)",
                            ),
                            in_sketch,
                        ),
                        when(
                            st.editing,
                            ready(
                                Icon::PatternCirc,
                                "Circular",
                                None,
                                Action::Tool(Tool::PatternCircular),
                                "Selected geometry around a point (count, angle in Properties)",
                            ),
                            in_sketch,
                        ),
                        tool(
                            Tool::Mirror,
                            None,
                            "Mirror selected geometry about a line (Symmetric constraints)",
                        ),
                        tool(
                            Tool::PatternPath,
                            None,
                            "Copies of the selected geometry along a line, arc or spline (equal chords)",
                        )
                        .mini(),
                        tool(
                            Tool::PatternPoints,
                            None,
                            "Copies of the selected geometry at points; each copy follows its point (Inventor: Sketch Driven Pattern)",
                        )
                        .mini(),
                    ],
                ),
                panel("Constrain", constrain),
                panel(
                    "Format",
                    vec![
                        format(
                            Icon::Construction,
                            "Construction",
                            Style::Construction,
                            "Selected geometry → construction; with nothing selected, toggles construction mode",
                        ),
                        format(
                            Icon::Centerline,
                            "Centerline",
                            Style::Centerline,
                            "Selected geometry → centerline",
                        ),
                        format(
                            Icon::Normal,
                            "Normal",
                            Style::Normal,
                            "Selected geometry → normal",
                        ),
                    ],
                ),
                panel(
                    "Demo",
                    vec![Cmd {
                        label: "Demo",
                        icon: Icon::Demo,
                        size: Size::Small,
                        key: None,
                        kind: Kind::Menu(
                            Demo::ALL
                                .iter()
                                .map(|&d| (d.label(), Icon::Demo, Action::Demo(d)))
                                .collect(),
                        ),
                    }],
                ),
                panel(
                    "Exit",
                    vec![
                        when(
                            st.editing,
                            ready(
                                Icon::Finish,
                                "Finish Sketch",
                                None,
                                Action::FinishSketch,
                                "Leave sketch edit mode",
                            ),
                            in_sketch,
                        )
                        .large(),
                    ],
                ),
            ]
        }
        Tab::Inspect => vec![
            panel(
                "Measure",
                vec![
                    todo(Icon::Measure, "Measure", None, "0.10").large(),
                    ready(
                        Icon::Mass,
                        "Physical Properties",
                        None,
                        Action::Material(MatCmd::Physical),
                        "Mass, area, volume, centre of gravity and inertia (iProperties: Physical)",
                    ),
                ],
            ),
            panel(
                "Analysis",
                vec![
                    ready(
                        Icon::Validate,
                        "Validate Solid",
                        None,
                        Action::SelectBody,
                        "Euler–Poincaré and manifold check of the body",
                    ),
                    todo(Icon::Interference, "Interference", None, "3.5"),
                    todo(Icon::Curvature, "Curvature", None, "5.4"),
                    todo(Icon::Draft, "Draft Analysis", None, "2.8"),
                ],
            ),
        ],
        Tab::Tools => vec![
            panel(
                "Options",
                vec![
                    ready(
                        Icon::Options,
                        "Application Options",
                        None,
                        Action::Options,
                        "Mouse profile, wheel direction, sketch options",
                    )
                    .large(),
                    todo(Icon::Settings, "Document Settings", None, "0.11"),
                ],
            ),
            panel(
                "Material and Appearance",
                vec![
                    ready(
                        Icon::Material,
                        "Material",
                        None,
                        Action::Material(MatCmd::MaterialBrowser),
                        "Material Browser: document and library materials",
                    ),
                    ready(
                        Icon::Appearance,
                        "Appearance",
                        None,
                        Action::Material(MatCmd::AppearanceBrowser),
                        "Appearance Browser: document and library appearances",
                    ),
                    ready(
                        Icon::AdjustLook,
                        "Adjust",
                        None,
                        Action::Material(MatCmd::Adjust),
                        "Give the selected faces, features or bodies an appearance or a colour",
                    ),
                    ready(
                        Icon::ClearLook,
                        "Clear",
                        None,
                        Action::Material(MatCmd::Clear),
                        "Remove appearance overrides from faces, features, bodies",
                    ),
                ],
            ),
            panel(
                "Parameters",
                vec![
                    ready(
                        Icon::Parameters,
                        "Parameters",
                        None,
                        Action::Parameters,
                        "Parameters (fx): names, equations, tolerances, user parameters",
                    )
                    .large(),
                ],
            ),
            panel(
                "Update",
                vec![
                    ready(
                        Icon::Rebuild,
                        "Rebuild All",
                        None,
                        Action::RebuildAll,
                        "Recompute every feature of the part from scratch (also clears the build cache)",
                    )
                    .large(),
                ],
            ),
            panel(
                "Automation",
                vec![
                    todo(Icon::Rules, "Rules", None, "7.5"),
                    todo(Icon::Assistant, "AI Assistant", None, "7.6"),
                ],
            ),
            panel(
                "History",
                vec![
                    ready(
                        Icon::Undo,
                        "Undo",
                        Some("Ctrl+Z"),
                        Action::Undo,
                        "Undo the last action",
                    ),
                    ready(
                        Icon::Redo,
                        "Redo",
                        Some("Ctrl+Y"),
                        Action::Redo,
                        "Redo the last undone action",
                    ),
                ],
            ),
        ],
        Tab::Manage => vec![panel(
            "Styles and Standards",
            vec![
                ready(
                    Icon::Styles,
                    "Styles Editor",
                    None,
                    Action::Material(MatCmd::StylesEditor),
                    "Style and Standard Editor: the document's materials and appearances",
                )
                .large(),
                ready(
                    Icon::Rebuild,
                    "Update",
                    None,
                    Action::Material(MatCmd::UpdateStyles),
                    "Update the document's styles from the library",
                ),
                ready(
                    Icon::Purge,
                    "Purge",
                    None,
                    Action::Material(MatCmd::Purge),
                    "Remove document styles nothing uses",
                ),
                todo(Icon::Styles, "Save", None, "2.15"),
            ],
        )],
        Tab::View => vec![
            panel(
                "Visibility",
                vec![
                    ready(
                        Icon::ShowConstraints,
                        "Constraints",
                        Some(if st.labels_shown { "F9" } else { "F8" }),
                        Action::ShowConstraints(!st.labels_shown),
                        "Show or hide constraint labels",
                    ),
                    ready(
                        Icon::Dof,
                        "Degrees of Freedom",
                        None,
                        Action::ToggleDof,
                        "Show All Degrees of Freedom",
                    ),
                ],
            ),
            panel(
                "Navigate",
                vec![
                    ready(
                        Icon::Home,
                        "Home",
                        Some("F6"),
                        Action::View(StandardView::Home),
                        "Home view: isometric, fit all",
                    )
                    .large(),
                    ready(
                        Icon::LookAt,
                        "Look At",
                        None,
                        Action::LookAt,
                        "Look straight at the edited/selected sketch",
                    ),
                    ready(
                        Icon::ViewFront,
                        "Front",
                        None,
                        Action::View(StandardView::Front),
                        "Look from −Y",
                    ),
                    ready(
                        Icon::ViewTop,
                        "Top",
                        None,
                        Action::View(StandardView::Top),
                        "Look from +Z",
                    ),
                    ready(
                        Icon::ViewRight,
                        "Right",
                        None,
                        Action::View(StandardView::Right),
                        "Look from +X",
                    ),
                ],
            ),
            panel(
                "Appearance",
                vec![
                    todo(Icon::VisualStyle, "Visual Style", None, "0.3"),
                    todo(Icon::Section, "Section View", None, "0.3"),
                    Cmd {
                        label: "Projection",
                        icon: Icon::Ortho,
                        size: Size::Small,
                        key: None,
                        kind: Kind::Menu(
                            [
                                super::camera::Projection::Orthographic,
                                super::camera::Projection::Perspective,
                                super::camera::Projection::PerspectiveOrthoFaces,
                            ]
                            .into_iter()
                            .map(|p| (p.label(), Icon::Ortho, Action::Projection(p)))
                            .collect(),
                        ),
                    },
                ],
            ),
            panel(
                "Windows",
                vec![
                    Cmd {
                        label: "User Interface",
                        icon: Icon::UiPanels,
                        size: Size::Small,
                        key: None,
                        kind: Kind::Menu(
                            Element::ALL
                                .into_iter()
                                .map(|e| (e.label(), Icon::UiPanels, Action::Ui(UiCmd::Toggle(e))))
                                .collect(),
                        ),
                    },
                    ready(
                        Icon::CleanScreen,
                        "Clean Screen",
                        Some("Ctrl+0"),
                        Action::Ui(UiCmd::CleanScreen),
                        "Only the graphics window (and the Quick Access bar); again to restore",
                    ),
                    ready(
                        Icon::FullScreen,
                        "Full Screen",
                        Some("F11 outside a sketch"),
                        Action::Ui(UiCmd::FullScreen),
                        "The application fills the screen",
                    ),
                    ready(
                        Icon::ResetLayout,
                        "Reset",
                        None,
                        Action::Ui(UiCmd::ResetLayout),
                        "Reset UI Layout: panels, sizes and window positions back to the defaults",
                    ),
                ],
            ),
        ],
    }
}

/// Height of a small row; three rows make a panel.
const ROW: f32 = 22.0;
const PANEL_H: f32 = ROW * 3.0 + 4.0;

/// Draws the tab strip and the active tab's panels; returns the clicked
/// command, if any.
pub fn show(ui: &mut egui::Ui, tab: &mut Tab, st: &State, fold: &mut Fold) -> Option<Action> {
    ui.horizontal(|ui| {
        for t in Tab::ALL {
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
            for p in panels(*tab, st) {
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
                Action::ToggleDof => st.show_dof,
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
                .find(|(_, _, a)| matches!(a, Action::Tool(t) if *t == active));
            let icon = current.map_or(cmd.icon, |c| c.1);
            let title = match current {
                Some((label, ..)) if cmd.size != Size::Large => format!("{}: {label}", cmd.label),
                _ => cmd.label.to_owned(),
            };
            let r = icon_button(ui, icon, &title, cmd.size, true, current.is_some(), true)
                .on_hover_text(name("Click for the variants"));
            let mut picked = None;
            egui::Popup::menu(&r).show(|ui| {
                for (label, icon, action) in items {
                    // User Interface toggles show as pressed while visible.
                    let on = match action {
                        Action::Ui(UiCmd::Toggle(e)) => Element::ALL
                            .iter()
                            .position(|x| x == e)
                            .is_some_and(|k| st.ui_shown[k]),
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
                    "Предстои — docs/FEATURES.md, раздел {section}{key}"
                )),
            );
            None
        }
    }
}
