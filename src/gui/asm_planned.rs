//! The interface of the assembly commands whose logic comes later (rule 15,
//! `docs/spec/3.0-assembly-commands.md`): each command opens its whole
//! Inventor 2027 panel in Properties, with selectors that take components,
//! faces and edges in the view. The panel, the selector flow, Enter, Esc,
//! the right-click menu and the prompts are the part's planned commands'
//! (`gui/part_planned.rs`, `Planned::Asm`). OK stays disabled: nothing
//! changes the assembly (rule 5).
//!
//! In an assembly a selector's `PickKind::Body` stands for a **component**
//! (an occurrence); `Face` / `PlanarFace` and `Edge` / `StraightEdge` take a
//! component's faces and edges; `Plane`, `Axis` and `Point` take origin and
//! work geometry (from the browser, later).

use eframe::egui;

use super::CadCraftApp;
use super::icons::Icon;
use super::part_planned::{Page, PartDialog, Planned, Row, selector_defs};
use crate::command::flow::PickKind;

use Row::{Button, Check, Combo, Head, Note, Num, Radio, Sel, Text};

// ---- what the selectors take ------------------------------------------------

const COMPONENTS: &[PickKind] = &[PickKind::Body];
const PLANE: &[PickKind] = &[PickKind::PlanarFace, PickKind::Plane];
const EDGE: &[PickKind] = &[PickKind::Edge, PickKind::StraightEdge];
const CIRCLE: &[PickKind] = &[PickKind::Edge];
const LINE: &[PickKind] = &[PickKind::StraightEdge, PickKind::Axis];
const CYLINDER: &[PickKind] = &[PickKind::Face];
/// A face or edge of a feature (its feature pattern).
const FEATURE_FACE: &[PickKind] = &[PickKind::Face, PickKind::Edge];
const POINT: &[PickKind] = &[PickKind::Point];
/// A frame member (a component of the frame).
const MEMBERS: &[PickKind] = &[PickKind::Body];
/// Skeleton geometry: model edges and sketch curves.
/// Insert Frame's edges: also faces, for Face Edges / Body Edges.
const SKELETON_OR_SOLID: &[PickKind] = &[
    PickKind::Edge,
    PickKind::StraightEdge,
    PickKind::SketchCurve,
    PickKind::Face,
    PickKind::PlanarFace,
];

const SKELETON: &[PickKind] = &[
    PickKind::Edge,
    PickKind::StraightEdge,
    PickKind::SketchCurve,
];
/// What a member's orientation can follow.
const ALIGN: &[PickKind] = &[
    PickKind::StraightEdge,
    PickKind::PlanarFace,
    PickKind::Axis,
    PickKind::Plane,
    PickKind::SketchCurve,
];
/// The end face of a member.
const END_FACE: &[PickKind] = &[PickKind::PlanarFace];
/// A face trimming members (planar or curved).
const TOOL_FACE: &[PickKind] = &[PickKind::Face, PickKind::PlanarFace];
const GEOMETRY: &[PickKind] = &[
    PickKind::Face,
    PickKind::PlanarFace,
    PickKind::Edge,
    PickKind::StraightEdge,
];

const BOM: &[&str] = &["Normal", "Phantom", "Reference", "Purchased", "Inseparable"];
const PLACEMENT: &[&str] = &["Linear", "Concentric", "On Point", "By Hole"];
const MODEL: &[&str] = &["Component", "Feature", "No Model"];
const STATUS: &[&str] = &["Mirror", "Reuse", "Exclude"];
const DEST: &[&str] = &["Insert in Assembly", "Open in New Window"];
const REMOVE: &[&str] = &["None", "All", "Range", "Preserve"];

/// An assembly command with an interface and no logic yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AsmPlanned {
    // Assemble › Component
    PlaceContentCenter,
    PlaceImported,
    Create,
    Pattern,
    Mirror,
    Copy,
    Replace,
    MakeLayout,
    Simplify,
    DerivedSubstitutes,
    // Assemble › Relationships
    Assemble,
    // Inspect › Interference
    Interference,
    // Manage › Author
    CreateIAssembly,
    // Design › Fasten
    BoltedConnection,
    ClevisPin,
    SecurePin,
    RadialPin,
    // Design › Frame
    InsertFrame,
    FrameChange,
    EndCap,
    Miter,
    CornerJoint,
    TrimExtend,
    Notch,
    LengthenShorten,
    RemoveEndTreatments,
    Reuse,
    ChangeReuse,
    FrameMemberInfo,
    FrameRefresh,
    EditFrame,
    BeamColumn,
    // Design › Power Transmission
    Shaft,
    SpurGear,
    WormGear,
    BevelGear,
    Bearing,
    Key,
    VBelts,
    SyncBelts,
    RollerChains,
    DiscCam,
    LinearCam,
    CylindricalCam,
    InvoluteSplines,
    ParallelSplines,
    ORing,
    // Design › Spring
    CompressionSpring,
    ExtensionSpring,
    TorsionSpring,
    BellevilleSpring,
}

impl AsmPlanned {
    #[cfg(test)]
    pub const ALL: [AsmPlanned; 51] = {
        use AsmPlanned::*;
        [
            PlaceContentCenter,
            PlaceImported,
            Create,
            Pattern,
            Mirror,
            Copy,
            Replace,
            MakeLayout,
            Simplify,
            DerivedSubstitutes,
            Assemble,
            Interference,
            CreateIAssembly,
            BoltedConnection,
            ClevisPin,
            SecurePin,
            RadialPin,
            InsertFrame,
            FrameChange,
            EndCap,
            Miter,
            CornerJoint,
            TrimExtend,
            Notch,
            LengthenShorten,
            RemoveEndTreatments,
            Reuse,
            ChangeReuse,
            FrameMemberInfo,
            FrameRefresh,
            EditFrame,
            BeamColumn,
            Shaft,
            SpurGear,
            WormGear,
            BevelGear,
            Bearing,
            Key,
            VBelts,
            SyncBelts,
            RollerChains,
            DiscCam,
            LinearCam,
            CylindricalCam,
            InvoluteSplines,
            ParallelSplines,
            ORing,
            CompressionSpring,
            ExtensionSpring,
            TorsionSpring,
            BellevilleSpring,
        ]
    };

    /// The panel's title (Inventor's command name).
    pub fn title(self) -> &'static str {
        use AsmPlanned::*;
        match self {
            PlaceContentCenter => "Place from Content Center",
            PlaceImported => "Place Imported CAD Files",
            Create => "Create In-Place Component",
            Pattern => "Pattern Component",
            Mirror => "Mirror Components",
            Copy => "Copy Components",
            Replace => "Replace Component",
            MakeLayout => "Make Layout",
            Simplify => "Simplify",
            DerivedSubstitutes => "Create Derived Substitutes",
            Assemble => "Assemble",
            Interference => "Analyze Interference",
            CreateIAssembly => "Create iAssembly",
            BoltedConnection => "Bolted Connection",
            ClevisPin => "Clevis Pin",
            SecurePin => "Secure Pin",
            RadialPin => "Radial Pin",
            InsertFrame => "Insert Frame",
            FrameChange => "Change",
            EndCap => "Insert End Cap",
            Miter => "Miter",
            CornerJoint => "Corner Joint",
            TrimExtend => "Trim/Extend To Face",
            Notch => "Notch",
            LengthenShorten => "Lengthen/Shorten Frame Member",
            RemoveEndTreatments => "Remove End Treatments",
            Reuse => "Reuse",
            ChangeReuse => "Change Reuse",
            FrameMemberInfo => "Frame Member Info",
            FrameRefresh => "Refresh",
            EditFrame => "Edit Frame",
            BeamColumn => "Beam and Column Calculator",
            Shaft => "Shaft",
            SpurGear => "Spur Gears",
            WormGear => "Worm Gear",
            BevelGear => "Bevel Gears",
            Bearing => "Bearing",
            Key => "Parallel Key",
            VBelts => "V-Belts",
            SyncBelts => "Synchronous Belts",
            RollerChains => "Roller Chains",
            DiscCam => "Disc Cam",
            LinearCam => "Linear Cam",
            CylindricalCam => "Cylindrical Cam",
            InvoluteSplines => "Involute Splines",
            ParallelSplines => "Parallel Splines",
            ORing => "O-Ring",
            CompressionSpring => "Compression Spring",
            ExtensionSpring => "Extension Spring",
            TorsionSpring => "Torsion Spring",
            BellevilleSpring => "Belleville Spring",
        }
    }

    /// The panel's icon (the command's own, `gui/icons.rs`).
    pub fn icon(self) -> Icon {
        use AsmPlanned::*;
        match self {
            PlaceContentCenter => Icon::PlaceContent,
            PlaceImported => Icon::PlaceImported,
            Create => Icon::AsmCreate,
            Pattern => Icon::AsmPattern,
            Mirror => Icon::AsmMirror,
            Copy => Icon::AsmCopy,
            Replace => Icon::AsmReplace,
            MakeLayout => Icon::MakeLayout,
            Simplify => Icon::Simplify,
            DerivedSubstitutes => Icon::Substitutes,
            Assemble => Icon::AsmAssemble,
            Interference => Icon::Interference,
            CreateIAssembly => Icon::CreateIAssembly,
            BoltedConnection => Icon::Bolt,
            ClevisPin => Icon::ClevisPin,
            SecurePin => Icon::SecurePin,
            RadialPin => Icon::RadialPin,
            InsertFrame => Icon::InsertFrame,
            FrameChange => Icon::FrameChange,
            EndCap => Icon::EndCap,
            Miter => Icon::Miter,
            CornerJoint => Icon::CornerJoint,
            TrimExtend => Icon::TrimExtend,
            Notch => Icon::Notch,
            LengthenShorten => Icon::LengthenShorten,
            RemoveEndTreatments => Icon::RemoveEndTreatments,
            Reuse => Icon::FrameReuse,
            ChangeReuse => Icon::ChangeReuse,
            FrameMemberInfo => Icon::FrameInfo,
            FrameRefresh => Icon::FrameRefresh,
            EditFrame => Icon::EditFrame,
            BeamColumn => Icon::BeamColumn,
            Shaft => Icon::Shaft,
            SpurGear => Icon::SpurGear,
            WormGear => Icon::WormGear,
            BevelGear => Icon::BevelGear,
            Bearing => Icon::Bearing,
            Key => Icon::KeyJoint,
            VBelts => Icon::VBelt,
            SyncBelts => Icon::SyncBelt,
            RollerChains => Icon::Chain,
            DiscCam => Icon::DiscCam,
            LinearCam => Icon::LinearCam,
            CylindricalCam => Icon::CylCam,
            InvoluteSplines => Icon::InvSpline,
            ParallelSplines => Icon::ParSpline,
            ORing => Icon::ORing,
            CompressionSpring => Icon::CompSpring,
            ExtensionSpring => Icon::ExtSpring,
            TorsionSpring => Icon::TorsionSpring,
            BellevilleSpring => Icon::Belleville,
        }
    }

    /// The `docs/FEATURES.md` section its logic belongs to.
    pub fn section(self) -> &'static str {
        use AsmPlanned::*;
        match self {
            PlaceContentCenter | BoltedConnection | ClevisPin | SecurePin | RadialPin => "6.3",
            PlaceImported => "7.7",
            Create | MakeLayout => "3.10",
            Pattern | Mirror => "3.7",
            Copy | Replace => "3.8",
            Simplify | DerivedSubstitutes => "3.11",
            Assemble => "3.2",
            Interference => "3.5",
            CreateIAssembly => "5.7",
            InsertFrame | FrameChange | EndCap | Miter | CornerJoint | TrimExtend | Notch
            | LengthenShorten | RemoveEndTreatments | Reuse | ChangeReuse | FrameMemberInfo
            | FrameRefresh | EditFrame => "6.1",
            _ => "6.2",
        }
    }

    /// One line on what the command does (under the options).
    pub fn about(self) -> &'static str {
        use AsmPlanned::*;
        match self {
            PlaceContentCenter => {
                "Places a standard part (bolt, nut, washer, profile) from the library."
            }
            PlaceImported => {
                "Places a STEP file as a component: its assembly, or one multi-body part."
            }
            Create => {
                "A new part or subassembly made in place, its first sketch on a face or plane."
            }
            Pattern => {
                "Repeats components after a feature pattern, in rows and columns, or about an axis."
            }
            Mirror => "Mirrors components about a plane: new mirrored files or reused instances.",
            Copy => "Copies components as new independent files or reused instances.",
            Replace => {
                "Swaps components for another file; relationships are kept where the geometry still exists."
            }
            MakeLayout => "A layout part with sketches for top-down design.",
            Simplify => {
                "A simplified part from the assembly: envelopes, fewer components and features."
            }
            DerivedSubstitutes => "A derived substitute part for each first-level subassembly.",
            Assemble => {
                "Pick a face or edge of the moving component, then one of the target: the relationship is inferred."
            }
            Interference => "Finds the volumes where components overlap.",
            CreateIAssembly => "A family table of the assembly: members, components, parameters.",
            BoltedConnection => "A bolt, washers and a nut through holes drilled in the plates.",
            ClevisPin | SecurePin | RadialPin => "A pin from the library with its hole.",
            InsertFrame => "Frame members along sketch lines or edges of a skeleton.",
            FrameChange => {
                "Changes the section, size, material or orientation of placed frame members."
            }
            EndCap => "A cap plate closing the end of a tube or beam member.",
            Miter => "Mitre cuts where two frame members meet.",
            CornerJoint => "Trims or extends two members to each other at a corner.",
            TrimExtend => "Trims or extends frame members to a model face.",
            Notch => "Cuts a member to the shape of the member it meets.",
            LengthenShorten => "Makes frame members longer or shorter at one or both ends.",
            RemoveEndTreatments => "Returns frame members to their inserted state without cuts.",
            Reuse => "Places the same member part on further skeleton lines of the same length.",
            ChangeReuse => "Changes the lines or placement of reused frame members.",
            FrameMemberInfo => "Section and physical properties of a frame member.",
            FrameRefresh => "Updates frame members after their library family changed.",
            EditFrame => "Places all members of a frame again and finishes its corners again.",
            BeamColumn => {
                "Checks a frame member as a beam (bending, deflection) or a column (buckling)."
            }
            Shaft => "A shaft from sections with chamfers, fillets, grooves, keyways and threads.",
            SpurGear | WormGear | BevelGear => {
                "A gear pair from module, number of teeth and pressure angle."
            }
            Bearing => "A rolling bearing from the library sized to a shaft.",
            Key => "A parallel key with its grooves in the shaft and the hub.",
            VBelts | SyncBelts | RollerChains => {
                "A belt or chain over pulleys or sprockets in one plane."
            }
            DiscCam | LinearCam | CylindricalCam => "A cam from a motion diagram and its follower.",
            InvoluteSplines | ParallelSplines => "A splined shaft and hub from the standard.",
            ORing => "An O-ring and its groove.",
            CompressionSpring | ExtensionSpring | TorsionSpring | BellevilleSpring => {
                "A spring between two planes along an axis."
            }
        }
    }

    /// The panel content: tabs and rows (spec 3.0 §3).
    pub fn pages(self) -> &'static [Page] {
        use AsmPlanned::*;
        match self {
            PlaceContentCenter => &[Page(
                "Place from Content Center",
                &[
                    Combo(
                        "Category View",
                        &[
                            "Fasteners",
                            "Shaft Parts",
                            "Structural Shapes",
                            "Other Parts",
                        ],
                        0,
                    ),
                    Combo(
                        "Family",
                        &[
                            "Hex Head Bolt",
                            "Socket Head Cap Screw",
                            "Hex Nut",
                            "Plain Washer",
                        ],
                        0,
                    ),
                    Combo("Standard", &["ISO", "DIN", "ANSI"], 0),
                    Head("Size"),
                    Combo("Thread Description", &["M6", "M8", "M10", "M12"], 1),
                    Num("Nominal Length", 30.0, " mm"),
                    Radio("Insert", &["As Standard", "As Custom"], 0),
                    Note("Needs the standard parts library (FEATURES 6.3)"),
                ],
            )],
            PlaceImported => &[Page(
                "Options",
                &[
                    Button("Browse..."),
                    Text("File", ""),
                    Radio("Import Type", &["Reference Model", "Convert Model"], 1),
                    Head("Object Filters"),
                    Check("Solids", true),
                    Check("Surfaces", true),
                    Check("Meshes", false),
                    Check("Graphical PMI", false),
                    Check("Wires", false),
                    Check("Work Features", false),
                    Check("Points", false),
                    Check("UCS", false),
                    Combo("Length Units", &["mm", "cm", "m", "in", "ft"], 0),
                    Head("Assembly"),
                    Radio(
                        "Import as",
                        &["Assembly", "Multi-body part", "Composite Part"],
                        0,
                    ),
                    Head("Part"),
                    Radio("Surfaces as", &["Composite", "Individual"], 1),
                    Check("Stitch", false),
                    Radio("Placement", &["At the origin", "Pick a point"], 0),
                ],
            )],
            Create => &[Page(
                "Create In-Place Component",
                &[
                    Text("New Component Name", "Part1"),
                    Combo(
                        "Template",
                        &[
                            "Standard (mm).ccpart",
                            "Sheet Metal (mm).ccpart",
                            "Standard (mm).ccasm",
                        ],
                        0,
                    ),
                    Button("Browse Templates..."),
                    Text("New File Location", "(the assembly's folder)"),
                    Combo("Default BOM Structure", BOM, 0),
                    Check("Virtual Component", false),
                    Check("Constrain sketch plane to selected face or plane", true),
                    Sel(
                        "plane",
                        "Sketch Plane",
                        "select a face or plane for the new component's first sketch",
                        PLANE,
                    ),
                ],
            )],
            Pattern => &[
                Page(
                    "Associative",
                    &[
                        Sel(
                            "components",
                            "Components",
                            "select the components to pattern",
                            COMPONENTS,
                        ),
                        Sel(
                            "feature",
                            "Feature Pattern",
                            "select a feature pattern of a part",
                            FEATURE_FACE,
                        ),
                        Note("Count and spacing follow the feature pattern"),
                    ],
                ),
                Page(
                    "Rectangular",
                    &[
                        Sel(
                            "components",
                            "Components",
                            "select the components to pattern",
                            COMPONENTS,
                        ),
                        Head("Column"),
                        Sel(
                            "column",
                            "Column Direction",
                            "select an edge or axis for the columns",
                            LINE,
                        ),
                        Check("Flip column", false),
                        Num("Column Count", 2.0, ""),
                        Num("Column Spacing", 2.0, " mm"),
                        Head("Row"),
                        Sel(
                            "row",
                            "Row Direction",
                            "select an edge or axis for the rows",
                            LINE,
                        ),
                        Check("Flip row", false),
                        Num("Row Count", 2.0, ""),
                        Num("Row Spacing", 2.0, " mm"),
                    ],
                ),
                Page(
                    "Circular",
                    &[
                        Sel(
                            "components",
                            "Components",
                            "select the components to pattern",
                            COMPONENTS,
                        ),
                        Sel(
                            "axis",
                            "Rotation Axis",
                            "select an edge or axis to turn about",
                            LINE,
                        ),
                        Check("Flip", false),
                        Num("Count", 4.0, ""),
                        Num("Angle", 90.0, "°"),
                    ],
                ),
            ],
            Mirror => &[
                Page(
                    "Status",
                    &[
                        Radio(
                            "Operation",
                            &["Create mirror pattern", "Create flat structure"],
                            0,
                        ),
                        Sel(
                            "components",
                            "Components",
                            "select the components to mirror",
                            COMPONENTS,
                        ),
                        Sel(
                            "plane",
                            "Mirror Plane",
                            "select a work plane, a planar face or an origin plane",
                            PLANE,
                        ),
                        Radio("Status of all", STATUS, 0),
                        Button("Copy List of Unsuitable Components"),
                        Check("Mirror Relationships", false),
                        Check("Ground New Components", false),
                        Check("Link Sheet Metal Styles", false),
                        Head("More"),
                        Check("Reuse Standard Content and Factory Parts", true),
                        Check("Preview Components", true),
                    ],
                ),
                Page(
                    "File Names",
                    &[
                        Note("New Name / File Location / status per component (after Next)"),
                        Check("Prefix", false),
                        Text("Prefix text", ""),
                        Check("Suffix", true),
                        Text("Suffix text", "_MIR"),
                        Check("Increment", false),
                        Combo(
                            "File Location",
                            &["Source Path", "Workspace", "User Path"],
                            0,
                        ),
                        Radio("Component Destination", DEST, 0),
                    ],
                ),
            ],
            Copy => &[
                Page(
                    "Status",
                    &[
                        Sel(
                            "components",
                            "Components",
                            "select the components to copy",
                            COMPONENTS,
                        ),
                        Radio("Status of all", &["Copy", "Reuse", "Exclude"], 0),
                        Check("Copy Relationships", true),
                        Check("Ground New Components", false),
                        Check("Reuse Standard Content and Factory Parts", true),
                    ],
                ),
                Page(
                    "File Names",
                    &[
                        Note("New Name / File Location / status per component (after Next)"),
                        Check("Prefix", false),
                        Text("Prefix text", ""),
                        Check("Suffix", true),
                        Text("Suffix text", "_CPY"),
                        Check("Increment", false),
                        Combo(
                            "File Location",
                            &["Source Path", "Workspace", "User Path"],
                            0,
                        ),
                        Radio("Component Destination", DEST, 0),
                    ],
                ),
            ],
            Replace => &[Page(
                "Replace",
                &[
                    Sel(
                        "components",
                        "Components",
                        "select the components to replace",
                        COMPONENTS,
                    ),
                    Check("Replace All occurrences", false),
                    Text("New Component", ""),
                    Button("Browse..."),
                    Note(
                        "Relationships are found again by their face names; missing ones become sick",
                    ),
                ],
            )],
            MakeLayout => &[Page(
                "Make Layout",
                &[
                    Text("Layout Name", "Layout1"),
                    Combo("Template", &["Standard (mm).ccpart"], 0),
                    Text("Location", "(the assembly's folder)"),
                ],
            )],
            Simplify => &[
                Page(
                    "Input",
                    &[
                        Combo(
                            "Preset",
                            &[
                                "No Simplification",
                                "Remove the least detail",
                                "Remove moderate detail",
                                "Remove the most detail",
                            ],
                            0,
                        ),
                        Combo("Model State", &["Primary"], 0),
                        Combo("View", &["Default"], 0),
                        Check("Associative", false),
                        Combo("Position", &["Primary"], 0),
                    ],
                ),
                Page(
                    "Envelopes",
                    &[
                        Radio(
                            "Replace with Envelopes",
                            &[
                                "None",
                                "All in One Envelope",
                                "Each Top Level Component",
                                "Each Part",
                            ],
                            0,
                        ),
                        Combo(
                            "Type",
                            &[
                                "Orthogonal Bounding Box",
                                "Oriented Minimum Bounding Box",
                                "Oriented Minimum Bounding Cylinder",
                            ],
                            0,
                        ),
                    ],
                ),
                Page(
                    "Components",
                    &[
                        Radio("Exclude parts", &["by selection", "by size"], 0),
                        Radio("Priority", &["Part Priority", "Component Priority"], 0),
                        Check("All Occurrences", false),
                        Sel(
                            "excluded",
                            "Components",
                            "select the components to leave out",
                            COMPONENTS,
                        ),
                        Num("Max. Diagonal", 10.0, " mm"),
                        Check("Auto-detect", false),
                        Button("View Included"),
                        Button("View Excluded"),
                    ],
                ),
                Page(
                    "Features",
                    &[
                        Combo("Holes", REMOVE, 0),
                        Combo("Fillets", REMOVE, 0),
                        Combo("Chamfers", REMOVE, 0),
                        Combo("Pockets", REMOVE, 0),
                        Combo("Embosses", REMOVE, 0),
                        Combo("Tunnels", REMOVE, 0),
                        Check("Highlight", false),
                    ],
                ),
                Page(
                    "Output",
                    &[
                        Radio("Type", &["New Part File", "Substitute"], 0),
                        Combo(
                            "Template",
                            &["Standard (mm).ccpart", "Sheet Metal (mm).ccpart"],
                            0,
                        ),
                        Text("Name", "Assembly1_Simplified"),
                        Text("Location", "(the assembly's folder)"),
                        Combo("BOM Structure", &["Normal", "Reference"], 0),
                        Combo(
                            "Style",
                            &[
                                "Single solid without seams",
                                "Single solid with seams",
                                "Maintain each solid",
                                "Single composite",
                            ],
                            0,
                        ),
                    ],
                ),
                Page(
                    "Advanced",
                    &[
                        Check("Break link", false),
                        Check("Suppress link", false),
                        Check("Rename", false),
                        Check("Fill internal voids", false),
                        Check("Remove internal parts", true),
                        Check("Link face color from source component", true),
                        Check("Make independent bodies on failed Boolean", false),
                    ],
                ),
            ],
            DerivedSubstitutes => &[Page(
                "Create Derived Substitutes",
                &[
                    Note("Each first-level subassembly with its proposed file name and location"),
                    Radio("Naming", &["Suffix", "Prefix"], 0),
                    Text("Text", "_SUB"),
                    Combo("Template", &["Standard (mm).ccpart"], 0),
                ],
            )],
            Assemble => &[Page(
                "Assemble",
                &[
                    Sel(
                        "moving",
                        "Component",
                        "select a face or edge of the component to move",
                        GEOMETRY,
                    ),
                    Sel(
                        "target",
                        "Target",
                        "select a face or edge of the target",
                        GEOMETRY,
                    ),
                    Combo("Type", &["Inferred", "Mate", "Flush", "Insert", "Angle"], 0),
                    Num("Offset", 0.0, " mm"),
                    Check("Flip", false),
                ],
            )],
            Interference => &[Page(
                "Interference Analysis",
                &[
                    Sel(
                        "set1",
                        "Define Set #1",
                        "select the components of the first set",
                        COMPONENTS,
                    ),
                    Sel(
                        "set2",
                        "Define Set #2",
                        "select the components of the second set (empty: within set #1)",
                        COMPONENTS,
                    ),
                    Check("Treat subassemblies as components", false),
                    Check("Use precise thread analysis", true),
                    Note(
                        "OK lists the overlaps: components, volume, centroid; filters, Ignore, Copy to Clipboard",
                    ),
                ],
            )],
            CreateIAssembly => &[
                Page(
                    "Components",
                    &[
                        Note("Include / exclude components per member"),
                        Button("Add Member"),
                    ],
                ),
                Page(
                    "Parameters",
                    &[Note(
                        "Parameters of the assembly and its components as columns",
                    )],
                ),
                Page(
                    "Properties",
                    &[Note("iProperties as columns (Part Number, Description…)")],
                ),
                Page(
                    "Work Features",
                    &[Note("Include / exclude work features per member")],
                ),
                Page(
                    "Other",
                    &[
                        Text("Member Name", "Assembly1-01"),
                        Text("Part Number", "Assembly1-01"),
                    ],
                ),
            ],
            BoltedConnection => &[
                Page(
                    "Design",
                    &[
                        Radio("Type", &["Through All", "Blind"], 0),
                        Combo("Placement", PLACEMENT, 0),
                        Sel(
                            "start",
                            "Start Plane",
                            "select the start plane (the bolt goes in from it)",
                            PLANE,
                        ),
                        Sel(
                            "edge1",
                            "Linear Edge 1",
                            "select the first edge to dimension the hole from",
                            EDGE,
                        ),
                        Sel(
                            "edge2",
                            "Linear Edge 2",
                            "select the second edge to dimension the hole from",
                            EDGE,
                        ),
                        Sel(
                            "circle",
                            "Circular Reference",
                            "select a circular edge for the centre",
                            CIRCLE,
                        ),
                        Sel("point", "Point", "select a work point", POINT),
                        Sel("hole", "Existing Hole", "select an existing hole", CYLINDER),
                        Sel("end", "Termination", "select the end face", PLANE),
                        Check("Follow Pattern", false),
                        Head("Thread"),
                        Combo(
                            "Thread",
                            &["ISO Metric profile", "ANSI Unified Screw Threads"],
                            0,
                        ),
                        Combo("Diameter", &["M6", "M8", "M10", "M12", "M16"], 1),
                        Head("Fasteners"),
                        Button("Click to add a fastener"),
                        Combo("Standard", &["ISO", "DIN", "ANSI"], 0),
                        Combo(
                            "Category",
                            &[
                                "Bolts",
                                "Hex Head Bolts",
                                "Socket Head Bolts",
                                "Washers",
                                "Nuts",
                            ],
                            0,
                        ),
                    ],
                ),
                Page(
                    "Calculation",
                    &[Note(
                        "Strength check of the joint (comes after the geometry)",
                    )],
                ),
                Page(
                    "Fatigue Calculation",
                    &[Note(
                        "Fatigue check of the joint (comes after the geometry)",
                    )],
                ),
            ],
            ClevisPin | SecurePin | RadialPin => &[Page(
                "Design",
                &[
                    Combo("Placement", PLACEMENT, 0),
                    Sel("start", "Start Plane", "select the start plane", PLANE),
                    Sel(
                        "ref",
                        "Reference",
                        "select an edge, circle or point for the position",
                        GEOMETRY,
                    ),
                    Sel("end", "Termination", "select the end face", PLANE),
                    Num("Pin Diameter", 8.0, " mm"),
                    Button("Click to add a pin"),
                ],
            )],
            InsertFrame => &[Page(
                "Insert",
                &[
                    Head("Input Geometry"),
                    Radio(
                        "Placement",
                        &["Insert Members on Edges", "Between Points"],
                        0,
                    ),
                    Combo("Select", &["Edge", "Face Edges", "Body Edges"], 0),
                    Sel(
                        "edges",
                        "Edges / Lines",
                        "select skeleton edges or sketch lines (Face Edges / Body Edges: click a face)",
                        SKELETON_OR_SOLID,
                    ),
                    Sel(
                        "points",
                        "Start / End Point",
                        "click near an end of an edge or sketch line for the start, then for the end (Between Points)",
                        POINT,
                    ),
                    Button("+ Add Set"),
                    Note(
                        "+ Add Set keeps these lines with the section below; pick more lines for another family or size, OK places every set in one frame.",
                    ),
                    Button("Flip Direction"),
                    Button("Copy Properties"),
                    Head("Frame Member"),
                    Combo("Category", &["Structural Shapes"], 0),
                    Button("Refresh Categories"),
                    // The section table's lists replace these (gui/frame.rs).
                    Combo("Standard", &["EN"], 0),
                    Combo("Family", &["Square hollow section (EN 10219)"], 0),
                    Combo("Size", &["20x20x2", "30x30x3", "40x40x3"], 2),
                    Combo(
                        "Material",
                        &[
                            "Steel S235",
                            "Steel S355",
                            "Stainless Steel 304",
                            "Aluminium 6061-T6",
                        ],
                        0,
                    ),
                    Combo(
                        "Appearance",
                        &["As Material", "Steel - galvanised", "Paint - red"],
                        0,
                    ),
                    Check("Merge Frame Members", false),
                    Head("Corners"),
                    Combo(
                        "Corner Type",
                        &[
                            "None",
                            "Miter",
                            "Butt - first through",
                            "Butt - second through",
                            "Butt - longer through",
                            "Butt - shorter through",
                        ],
                        0,
                    ),
                    Num("Corner Gap", 0.0, " mm"),
                    Combo("T Joints", &["None", "Butt", "Notch"], 1),
                    Note(
                        "Corners of three or more members: the column runs through, the others stop at it.",
                    ),
                    Head("Orientation"),
                    Combo("Position", &["Center", "Inside", "Outside"], 0),
                    Combo("Level", &["Auto", "Top", "Bottom"], 0),
                    Combo(
                        "Anchor",
                        &[
                            "Center",
                            "Centroid",
                            "Top Left",
                            "Top",
                            "Top Right",
                            "Left",
                            "Right",
                            "Bottom Left",
                            "Bottom",
                            "Bottom Right",
                        ],
                        0,
                    ),
                    Num("Horizontal Offset", 0.0, " mm"),
                    Num("Vertical Offset", 0.0, " mm"),
                    Num("Rotation Angle", 0.0, "°"),
                    Check("Rotate Around Selected Point", false),
                    Check("Mirror", false),
                    Sel(
                        "align",
                        "Align",
                        "select an edge, face, axis, plane or sketch line the member follows (optional)",
                        ALIGN,
                    ),
                    Head("View"),
                    Button("Look At Manipulator"),
                    Button("Zoom to Manipulator"),
                    Button("Initial View"),
                    Head("Advanced Settings"),
                    Check("Apply Appearance to Part", false),
                    Note(
                        "The first Apply asks for the frame's file names (Create New Frame) and each member's (Frame Member Naming).",
                    ),
                ],
            )],
            FrameChange => &[Page(
                "Change",
                &[
                    Sel(
                        "members",
                        "Frame Members",
                        "select the frame members to change",
                        MEMBERS,
                    ),
                    Button("Flip Direction"),
                    Button("Copy Properties"),
                    Head("Frame Member"),
                    Combo("Category", &["Structural Shapes"], 0),
                    Button("Refresh Categories"),
                    // The section table's lists replace these (gui/frame.rs).
                    Combo("Standard", &["EN"], 0),
                    Combo("Family", &["Square hollow section (EN 10219)"], 0),
                    Combo("Size", &["20x20x2", "30x30x3", "40x40x3"], 2),
                    Combo(
                        "Material",
                        &[
                            "Steel S235",
                            "Steel S355",
                            "Stainless Steel 304",
                            "Aluminium 6061-T6",
                        ],
                        0,
                    ),
                    Combo(
                        "Appearance",
                        &["As Material", "Steel - galvanised", "Paint - red"],
                        0,
                    ),
                    Head("Orientation"),
                    Combo(
                        "Anchor",
                        &[
                            "Center",
                            "Centroid",
                            "Top Left",
                            "Top",
                            "Top Right",
                            "Left",
                            "Right",
                            "Bottom Left",
                            "Bottom",
                            "Bottom Right",
                        ],
                        0,
                    ),
                    Num("Horizontal Offset", 0.0, " mm"),
                    Num("Vertical Offset", 0.0, " mm"),
                    Num("Rotation Angle", 0.0, "°"),
                    Check("Rotate Around Selected Point", false),
                    Check("Mirror", false),
                    Sel(
                        "align",
                        "Align",
                        "select an edge, face, axis, plane or sketch line the member follows (optional)",
                        ALIGN,
                    ),
                    Head("View"),
                    Button("Look At Manipulator"),
                    Button("Zoom to Manipulator"),
                    Button("Initial View"),
                    Head("Advanced Settings"),
                    Check("Apply Appearance to Part", false),
                    Note("End treatments of the changed members are kept and solved again."),
                ],
            )],
            EndCap => &[Page(
                "End Cap",
                &[
                    Sel(
                        "faces",
                        "Member End Faces",
                        "select the end faces of the frame members to cap",
                        END_FACE,
                    ),
                    Head("Behavior"),
                    Radio("Placement", &["Internal", "External"], 1),
                    Radio(
                        "Profile",
                        &["Filleted Corners", "Chamfered Corners", "Sharp Corners"],
                        0,
                    ),
                    Num("Thickness", 3.0, " mm"),
                    Radio("Offset Type", &["Distance", "Ratio"], 0),
                    Num("Edge Offset", 0.0, " mm"),
                    Head("Orientation"),
                    Num("Rotate", 0.0, "°"),
                    Num("Offset", 0.0, " mm"),
                    Head("Part Properties"),
                    Combo(
                        "Material",
                        &[
                            "Steel S235",
                            "Steel S355",
                            "Stainless Steel 304",
                            "Aluminium 6061-T6",
                        ],
                        0,
                    ),
                    Combo(
                        "Appearance",
                        &["As Material", "Steel - galvanised", "Paint - red"],
                        0,
                    ),
                    Head("View"),
                    Button("Look At Manipulator"),
                    Button("Zoom to Manipulator"),
                    Button("Initial View"),
                ],
            )],
            Miter => &[Page(
                "Miter",
                &[
                    Sel(
                        "members",
                        "Frame Members",
                        "select the two members to mitre (or several, a cut at each meeting)",
                        MEMBERS,
                    ),
                    Radio("Miter Type", &["Full Miter Cut", "Bisect Miter Cut"], 0),
                    Num("Distance", 0.0, " mm"),
                    Radio("Offset", &["Default", "Flipped", "Symmetric"], 0),
                    Head("Advanced Settings"),
                    Check("Delete existing end treatments", true),
                ],
            )],
            CornerJoint => &[Page(
                "Corner Joint",
                &[
                    Sel(
                        "first",
                        "First Member",
                        "select the first member (yellow)",
                        MEMBERS,
                    ),
                    Sel(
                        "second",
                        "Second Member",
                        "select the member trimmed or extended to it (blue)",
                        MEMBERS,
                    ),
                    Button("Flip Members"),
                    Num("Offset1", 0.0, " mm"),
                    Num("Offset2", 0.0, " mm"),
                    Head("Advanced Settings"),
                    Check("Delete existing end treatments", true),
                ],
            )],
            TrimExtend => &[Page(
                "Trim/Extend",
                &[
                    Sel(
                        "face",
                        "Face",
                        "select the model face to trim or extend to (planar or curved)",
                        TOOL_FACE,
                    ),
                    Sel(
                        "members",
                        "Frame Members",
                        "select the members to trim or extend",
                        MEMBERS,
                    ),
                    Button("Flip Face Normal"),
                    Num("Offset", 0.0, " mm"),
                    Check("Trim Selected Part", false),
                    Head("Advanced Settings"),
                    Check("Delete existing end treatments", true),
                    Note(
                        "The cut angles go into the member's custom properties CUTDETAIL1, CUTDETAIL2...",
                    ),
                ],
            )],
            Notch => &[Page(
                "Notch",
                &[
                    Sel(
                        "notched",
                        "Notched Members",
                        "select the members to notch",
                        MEMBERS,
                    ),
                    Sel(
                        "tools",
                        "Notching Members",
                        "select the members whose shape cuts the notch",
                        MEMBERS,
                    ),
                    Radio(
                        "Profile",
                        &[
                            "Basic Profile",
                            "Custom I Template",
                            "Custom C Template",
                            "Custom T Template",
                            "Custom Profile",
                        ],
                        0,
                    ),
                    Num("Offset", 0.0, " mm"),
                    Num("Flange Offset", 0.0, " mm"),
                    Num("Web Offset", 0.0, " mm"),
                    Check("Extend Notch Profile", false),
                    Check("Perpendicular Cut", false),
                    Head("Advanced Settings"),
                    Check("Delete existing end treatments", true),
                ],
            )],
            LengthenShorten => &[Page(
                "Lengthen/Shorten",
                &[
                    Sel(
                        "members",
                        "Frame Members",
                        "select the members near the end to change (a picked one again: its other end)",
                        MEMBERS,
                    ),
                    Radio(
                        "Offset Type",
                        &["Default", "Flipped", "Symmetric", "Asymmetric"],
                        0,
                    ),
                    Num("Offset", 0.0, " mm"),
                    Num("Offset 2", 0.0, " mm"),
                    Note("A positive offset lengthens the member, a negative one shortens it."),
                    Head("Advanced Settings"),
                    Check("Delete existing end treatments", true),
                ],
            )],
            RemoveEndTreatments => &[Page(
                "Remove End Treatments",
                &[
                    Sel(
                        "members",
                        "Frame Members",
                        "select the members to return to their inserted state",
                        MEMBERS,
                    ),
                    Note(
                        "Every mitre, corner joint, notch, trim and length change of the members is removed.",
                    ),
                ],
            )],
            Reuse => &[Page(
                "Reuse",
                &[
                    Sel(
                        "source",
                        "Source Member",
                        "select the member to reuse (a reused one finds its source)",
                        MEMBERS,
                    ),
                    Sel(
                        "edges",
                        "Edges / Lines",
                        "select skeleton lines of the same length (a window selects many)",
                        SKELETON,
                    ),
                    Button("Flip Direction"),
                    Head("Advanced Settings"),
                    Check("Apply Appearance to Part", false),
                    Note(
                        "Straight members only; end treatments of reused members are changed on the source.",
                    ),
                ],
            )],
            ChangeReuse => &[Page(
                "Change Reuse",
                &[
                    Sel(
                        "members",
                        "Reused Members",
                        "select the reused members to change",
                        MEMBERS,
                    ),
                    Sel(
                        "edges",
                        "Edges / Lines",
                        "select the skeleton lines they move to",
                        SKELETON,
                    ),
                    Button("Flip Direction"),
                    Head("Advanced Settings"),
                    Check("Apply Appearance to Part", false),
                    Note("Straight members only."),
                ],
            )],
            FrameMemberInfo => &[Page(
                "Frame Member Info",
                &[
                    Sel("member", "Frame Member", "select a frame member", MEMBERS),
                    Head("Member"),
                    Text("Family", ""),
                    Text("Size", ""),
                    Text("Material", ""),
                    Text("Length", ""),
                    Text("Mass", ""),
                    Head("Section"),
                    Text("Area", ""),
                    Text("Moment of Inertia Ix", ""),
                    Text("Moment of Inertia Iy", ""),
                    Text("Section Modulus Wx", ""),
                    Text("Section Modulus Wy", ""),
                    Text("Radius of Gyration rx", ""),
                    Text("Radius of Gyration ry", ""),
                    Text("Centroid", ""),
                ],
            )],
            EditFrame => &[Page(
                "Edit Frame",
                &[
                    Sel(
                        "members",
                        "Frame",
                        "select a member of the frame to edit",
                        MEMBERS,
                    ),
                    Head("Position"),
                    Combo("Position", &["Keep", "Center", "Inside", "Outside"], 0),
                    Combo("Level", &["Auto", "Top", "Bottom"], 0),
                    Head("Corners"),
                    Combo(
                        "Corner Type",
                        &[
                            "Keep",
                            "None",
                            "Miter",
                            "Butt - first through",
                            "Butt - second through",
                            "Butt - longer through",
                            "Butt - shorter through",
                        ],
                        0,
                    ),
                    Num("Corner Gap", 0.0, " mm"),
                    Combo("T Joints", &["None", "Butt", "Notch"], 1),
                    Note(
                        "A Corner Type other than Keep replaces the frame's Miter, Corner Joint and Notch treatments; Trim/Extend and Lengthen/Shorten stay.",
                    ),
                ],
            )],
            FrameRefresh => &[Page(
                "Refresh",
                &[
                    Sel(
                        "members",
                        "Frame Members",
                        "select the members to update (none = every member of the frame)",
                        MEMBERS,
                    ),
                    Note(
                        "Members whose family table or template changed in the library are rebuilt; a size no longer in the library keeps its data.",
                    ),
                ],
            )],
            BeamColumn => &[
                Page(
                    "Model",
                    &[
                        Sel(
                            "member",
                            "Frame Member",
                            "select the member to check",
                            MEMBERS,
                        ),
                        Num("Beam Length", 1000.0, " mm"),
                        Combo(
                            "Material",
                            &[
                                "Steel S235",
                                "Steel S355",
                                "Stainless Steel 304",
                                "Aluminium 6061-T6",
                            ],
                            0,
                        ),
                        Head("Supports"),
                        Combo("Support A", &["Fixed", "Pinned", "Free"], 1),
                        Combo("Support B", &["Fixed", "Pinned", "Free"], 1),
                    ],
                ),
                Page(
                    "Loads",
                    &[
                        Combo(
                            "Load",
                            &["Force", "Continuous Load", "Bending Moment", "Axial Force"],
                            0,
                        ),
                        Num("Force", 1000.0, " N"),
                        Num("Position", 500.0, " mm"),
                        Num("Angle", 270.0, "°"),
                    ],
                ),
                Page(
                    "Calculation",
                    &[
                        Radio("Calculation", &["Beam", "Column", "Beam and Column"], 0),
                        Num("Safety Factor", 1.5, ""),
                        Check("Include Own Weight", true),
                        Button("Calculate"),
                    ],
                ),
            ],
            Shaft => &[
                Page(
                    "Design",
                    &[
                        Sel(
                            "axis",
                            "Axis",
                            "select an axis or cylindrical face for the shaft",
                            LINE,
                        ),
                        Sel(
                            "start",
                            "Start Plane",
                            "select the start plane (optional)",
                            PLANE,
                        ),
                        Head("Sections"),
                        Note("Cylinder 100 × 50 mm"),
                        Button("Insert Cylinder"),
                        Button("Insert Cone"),
                        Button("Insert Polygon"),
                        Button("Bore at the left"),
                        Button("Bore at the right"),
                    ],
                ),
                Page(
                    "Calculation",
                    &[Note("Loads, supports, material (comes after the geometry)")],
                ),
                Page("Graphs", &[Note("Shear force, bending moment, deflection")]),
            ],
            SpurGear | WormGear | BevelGear => &[
                Page(
                    "Design",
                    &[
                        Head("Common"),
                        Combo(
                            "Design Guide",
                            &[
                                "Module",
                                "Number of Teeth",
                                "Center Distance",
                                "Total Unit Correction",
                            ],
                            0,
                        ),
                        Num("Pressure Angle", 20.0, "°"),
                        Num("Module", 2.0, " mm"),
                        Num("Desired Gear Ratio", 2.0, ""),
                        Num("Center Distance", 60.0, " mm"),
                        Head("Gear 1"),
                        Combo("Gear 1", MODEL, 0),
                        Num("Number of Teeth 1", 20.0, ""),
                        Num("Facewidth 1", 20.0, " mm"),
                        Sel(
                            "cyl1",
                            "Cylindrical Face 1",
                            "select the first gear's cylindrical face or axis",
                            CYLINDER,
                        ),
                        Sel(
                            "start1",
                            "Start Plane 1",
                            "select the first gear's start plane",
                            PLANE,
                        ),
                        Head("Gear 2"),
                        Combo("Gear 2", MODEL, 0),
                        Num("Number of Teeth 2", 40.0, ""),
                        Num("Facewidth 2", 20.0, " mm"),
                        Sel(
                            "cyl2",
                            "Cylindrical Face 2",
                            "select the second gear's cylindrical face or axis",
                            CYLINDER,
                        ),
                        Sel(
                            "start2",
                            "Start Plane 2",
                            "select the second gear's start plane",
                            PLANE,
                        ),
                    ],
                ),
                Page(
                    "Calculation",
                    &[Note(
                        "Strength of the teeth and life (comes after the geometry)",
                    )],
                ),
            ],
            Bearing => &[
                Page(
                    "Design",
                    &[
                        Sel(
                            "cyl",
                            "Cylindrical Face",
                            "select the shaft's cylindrical face",
                            CYLINDER,
                        ),
                        Sel("start", "Start Plane", "select the start plane", PLANE),
                        Num("Diameter", 20.0, " mm"),
                        Num("Outside Diameter", 47.0, " mm"),
                        Num("Width", 14.0, " mm"),
                        Combo(
                            "Category",
                            &["Ball Bearings", "Roller Bearings", "Thrust Bearings"],
                            0,
                        ),
                        Note("The fitting bearings from the library are listed here"),
                    ],
                ),
                Page(
                    "Calculation",
                    &[Note("Loads and life (comes after the geometry)")],
                ),
            ],
            Key => &[Page(
                "Design",
                &[
                    Combo("Key", &["ISO 2491 A", "DIN 6885 A"], 0),
                    Head("Shaft Groove"),
                    Sel(
                        "shaft",
                        "Cylindrical Face",
                        "select the shaft's cylindrical face",
                        CYLINDER,
                    ),
                    Sel(
                        "start",
                        "Start Plane",
                        "select the start plane of the groove",
                        PLANE,
                    ),
                    Combo("Groove", &["Rounded ends", "Square ends"], 0),
                    Head("Hub Groove"),
                    Sel(
                        "hub",
                        "Hub Face",
                        "select the hub's cylindrical face",
                        CYLINDER,
                    ),
                    Num("Number of Keys", 1.0, ""),
                ],
            )],
            VBelts | SyncBelts | RollerChains => &[Page(
                "Design",
                &[
                    Sel(
                        "plane",
                        "Mid-plane",
                        "select the plane the belt runs in",
                        PLANE,
                    ),
                    Combo(
                        "Belt",
                        &[
                            "ISO 4184 Classical",
                            "ISO 5296 Synchronous",
                            "ISO 606 Roller chain",
                        ],
                        0,
                    ),
                    Head("Pulleys"),
                    Sel(
                        "pulleys",
                        "Pulleys",
                        "select the cylindrical faces or axes of the pulleys",
                        CYLINDER,
                    ),
                    Combo("Pulley placement", &["Fixed", "Free", "Tensioner"], 0),
                    Button("Click to add a pulley"),
                ],
            )],
            DiscCam | LinearCam | CylindricalCam => &[Page(
                "Design",
                &[
                    Sel(
                        "cyl",
                        "Cylindrical Face",
                        "select the cam's cylindrical face or axis",
                        CYLINDER,
                    ),
                    Sel("start", "Start Plane", "select the start plane", PLANE),
                    Num("Base Radius", 30.0, " mm"),
                    Num("Cam Width", 10.0, " mm"),
                    Head("Follower"),
                    Combo("Follower Type", &["Roller", "Sliding", "Flat"], 0),
                    Num("Roller Radius", 8.0, " mm"),
                    Head("Motion"),
                    Note("Segments: start angle, end angle, lift, motion type"),
                    Button("Add Segment"),
                ],
            )],
            InvoluteSplines | ParallelSplines => &[Page(
                "Design",
                &[
                    Combo("Spline", &["ISO 4156", "DIN 5480", "ISO 14 (parallel)"], 0),
                    Sel(
                        "shaft",
                        "Shaft Groove",
                        "select the shaft's cylindrical face",
                        CYLINDER,
                    ),
                    Sel("start", "Start Plane", "select the start plane", PLANE),
                    Sel(
                        "hub",
                        "Hub Groove",
                        "select the hub's cylindrical face",
                        CYLINDER,
                    ),
                    Num("Spline Length", 30.0, " mm"),
                ],
            )],
            ORing => &[Page(
                "Design",
                &[
                    Sel(
                        "cyl",
                        "Cylindrical Face",
                        "select the cylindrical face of the seal",
                        CYLINDER,
                    ),
                    Sel("start", "Start Plane", "select the start plane", PLANE),
                    Radio("Groove", &["Radial outer", "Radial inner", "Axial"], 0),
                    Combo("O-Ring", &["ISO 3601-1"], 0),
                    Num("Offset", 5.0, " mm"),
                ],
            )],
            CompressionSpring | ExtensionSpring | TorsionSpring | BellevilleSpring => &[
                Page(
                    "Design",
                    &[
                        Sel(
                            "axis",
                            "Axis",
                            "select the spring's axis or cylindrical face",
                            LINE,
                        ),
                        Sel("start", "Start Plane", "select the start plane", PLANE),
                        Sel("end", "End Plane", "select the end plane", PLANE),
                        Num("Wire Diameter", 2.0, " mm"),
                        Num("Outer Diameter", 20.0, " mm"),
                        Num("Free Length", 50.0, " mm"),
                        Num("Active Coils", 6.0, ""),
                        Combo("Ends", &["Closed and ground", "Closed", "Open"], 0),
                    ],
                ),
                Page(
                    "Calculation",
                    &[Note(
                        "Forces, stresses and lengths (comes after the geometry)",
                    )],
                ),
            ],
        }
    }
}

impl CadCraftApp {
    /// Opens the panel of planned assembly command `kind`; the selected
    /// components fill its first selector when it takes components (rule 17).
    pub(super) fn start_asm_planned(&mut self, kind: AsmPlanned) {
        self.asm.tool = None;
        let mut d = PartDialog::new(kind);
        let defs = selector_defs(Planned::Asm(kind));
        if !self.frame_preselect(&mut d)
            && let Some(first) = defs.first()
            && first.3.contains(&PickKind::Body)
            && !self.asm.sel.is_empty()
        {
            let names: Vec<String> = self
                .asm
                .sel
                .iter()
                .filter_map(|&id| self.occurrence_label(id))
                .collect();
            d.picks.entry(first.0).or_default().extend(names);
            if let Some(next) = defs.get(1) {
                d.active = Some(next.0);
            }
        }
        self.part_planned = Some(d);
    }

    pub(super) fn occurrence_label_pub(&self, id: u32) -> Option<String> {
        self.occurrence_label(id)
    }

    fn occurrence_label(&self, id: u32) -> Option<String> {
        self.assembly()?
            .find(id)
            .map(|o| format!("{} · component", o.name))
    }

    /// A click in the assembly view while a planned panel is open: a
    /// component, or a face / edge of one, as the active selector takes.
    pub(super) fn asm_planned_click(&mut self, p: egui::Pos2, rect: egui::Rect) {
        // Edit Frame Members: the current member's anchor points first.
        if self.frame_anchor_click(p, rect) {
            return;
        }
        // Insert Frame's Between Points: ends of edges and sketch lines.
        if self.frame_between_click(p, rect) {
            return;
        }
        let Some(kinds) = self
            .part_planned
            .as_ref()
            .and_then(|d| d.flow().current().map(|s| s.accepts))
        else {
            return;
        };
        // Frame commands: a member selector takes only frame members.
        let member = self.asm_pick_pub(p, rect).map(|(h, at)| (h.occurrence, at));
        if self.frame_member_click(member) {
            return;
        }
        if kinds.contains(&PickKind::Body) {
            let hit = self.asm_pick_pub(p, rect);
            let what = hit.and_then(|(h, _)| self.occurrence_label(h.occurrence));
            if let (Some(w), Some(d)) = (what, self.part_planned.as_mut()) {
                if let Some((_, at)) = hit {
                    d.clicks.push((w.clone(), [at.x, at.y, at.z]));
                }
                d.pick(w);
            }
            return;
        }
        // Insert Frame: a line of a component's sketch under the cursor.
        if self.frame_pick_sketch_line(p, rect) {
            return;
        }
        use crate::assembly::relation::GeomRef;
        let face = kinds
            .iter()
            .any(|k| matches!(k, PickKind::Face | PickKind::PlanarFace));
        let edge = kinds
            .iter()
            .any(|k| matches!(k, PickKind::Edge | PickKind::StraightEdge));
        // An axis selector also takes a round face (its axis).
        let axis = kinds.contains(&PickKind::Axis);
        let g = match self.geom_under_pub(p, rect) {
            Some(g @ GeomRef::Face { .. }) if face => g,
            Some(g @ GeomRef::Face { .. })
                if axis
                    && matches!(
                        self.asm_local_geom(&g),
                        Ok(crate::assembly::Geom::Cylinder { .. })
                    ) =>
            {
                g
            }
            Some(g @ GeomRef::Edge { .. }) if edge => g,
            other => {
                if let (Some(d), Some(s)) = (&self.part_planned, other) {
                    let what = self.geom_label(&s);
                    self.file_status = Some(Err(format!(
                        "{}: {what} cannot be used here",
                        d.kind.title()
                    )));
                }
                return;
            }
        };
        self.planned_take_ref(g);
    }

    /// Gives a component's geometry (a click in the view or an origin /
    /// work row of the browser) to the open planned panel's selector.
    pub(super) fn planned_take_ref(&mut self, g: crate::assembly::GeomRef) {
        use crate::assembly::relation::GeomRef;
        let mut label = self.geom_label(&g);
        // Pattern's Feature Pattern row: the pattern's name, not the face.
        if self
            .part_planned
            .as_ref()
            .is_some_and(|d| d.active == Some("feature"))
        {
            let names = match &g {
                GeomRef::Face { name, .. } => vec![name.clone()],
                GeomRef::Edge { edge, .. } => edge.faces.to_vec(),
                _ => Vec::new(),
            };
            let found = self
                .geom_part(&g)
                .and_then(|(component, _)| {
                    self.with_part(component, |p| {
                        names
                            .iter()
                            .find_map(|n| crate::assembly::pattern::pattern_of_face(p, n))
                    })
                })
                .flatten();
            match found {
                Some(id) => label = format!("{id} · feature pattern"),
                None => {
                    self.file_status = Some(Err(
                        "Pattern Component: that face belongs to no feature pattern".into(),
                    ));
                    return;
                }
            }
        }
        let mode = match &self.part_planned {
            Some(d)
                if d.kind == Planned::Asm(AsmPlanned::InsertFrame) && d.active == Some("edges") =>
            {
                Some(d.index(0, "Select"))
            }
            Some(_) => None,
            None => return,
        };
        if let Some(mode) = mode {
            let face = match &g {
                GeomRef::Face { name, .. } => Some(name.clone()),
                _ => None,
            };
            if mode == 0 {
                if face.is_some() {
                    self.file_status = Some(Err(
                        "Insert Frame: pick an edge, or set Select to Face Edges / Body Edges"
                            .into(),
                    ));
                } else if let Some(d) = self.part_planned.as_mut() {
                    d.pick_skeleton(label, crate::assembly::frame::Skeleton::Edge(g));
                }
                return;
            }
            // Face Edges: the clicked face's straight edges (an edge click:
            // that edge); Body Edges: every straight edge of the body.
            let only = if mode == 1 { face.clone() } else { None };
            if mode == 1 && only.is_none() {
                if let Some(d) = self.part_planned.as_mut() {
                    d.pick_skeleton(label, crate::assembly::frame::Skeleton::Edge(g));
                }
                return;
            }
            let (occurrence, sub) = (g.occurrence(), g.sub().to_vec());
            let edges = self
                .geom_part(&g)
                .and_then(|(c, _)| {
                    self.with_part(c, |p| {
                        crate::assembly::frame::straight_edges(p, only.as_deref())
                    })
                })
                .unwrap_or_default();
            let refs: Vec<GeomRef> = edges
                .into_iter()
                .map(|edge| GeomRef::Edge {
                    occurrence,
                    sub: sub.clone(),
                    edge,
                })
                .collect();
            let labels: Vec<String> = refs.iter().map(|r| self.geom_label(r)).collect();
            let Some(d) = self.part_planned.as_mut() else {
                return;
            };
            let mut added = 0;
            for (r, w) in refs.into_iter().zip(labels) {
                let s = crate::assembly::frame::Skeleton::Edge(r);
                if !d.skeletons.contains(&s) {
                    d.pick_skeleton(w, s);
                    added += 1;
                }
            }
            self.file_status = Some(Ok(format!("Insert Frame: {added} edge(s) added")));
            return;
        }
        let Some(d) = self.part_planned.as_mut() else {
            return;
        };
        // Pattern Component's directions and axis take one pick each.
        let single = d.kind == Planned::Asm(AsmPlanned::Pattern)
            && matches!(d.active, Some("column" | "row" | "axis" | "feature"));
        d.pick_ref(label, g, single);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::commands::COMMANDS;
    use crate::gui::ribbon::Action;

    /// Every planned assembly command has a panel, its own icon (not shared
    /// with the part's planned commands either), a registry command with an
    /// alias, and its FEATURES section exists.
    #[test]
    fn every_planned_assembly_command_has_a_panel_and_a_command() {
        let features = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/FEATURES.md"),
        )
        .unwrap_or_default();
        let mut icons: Vec<Icon> = super::super::part_planned::PartPlanned::ALL
            .iter()
            .map(|p| p.icon())
            .collect();
        for k in AsmPlanned::ALL {
            assert!(!k.title().is_empty() && !k.about().is_empty());
            assert!(!icons.contains(&k.icon()), "{} shares an icon", k.title());
            icons.push(k.icon());
            assert!(
                features.contains(&format!("## {} ", k.section())),
                "{}: no FEATURES section {}",
                k.title(),
                k.section()
            );
            for pg in k.pages() {
                assert!(!pg.1.is_empty(), "{} {}", k.title(), pg.0);
            }
            let d = PartDialog::new(k);
            assert!(d.prompt().starts_with(k.title()));
            let Some(cmd) = COMMANDS
                .iter()
                .find(|c| c.action == Some(Action::AsmPlanned(k)))
            else {
                panic!("{} has no command", k.title());
            };
            assert!(!cmd.aliases.is_empty(), "{} has no alias", cmd.id);
            assert_eq!(cmd.icon, k.icon(), "{}", cmd.id);
        }
    }

    /// Pattern Component's panel gives its numbers, check boxes and the
    /// geometry of its picks; a direction takes one pick (a new one
    /// replaces it), and clearing a pick drops its geometry with it.
    #[test]
    fn pattern_panel_values_and_geometry() {
        use crate::assembly::relation::{ASSEMBLY, GeomRef, OriginItem};
        let mut d = PartDialog::new(AsmPlanned::Pattern);
        assert_eq!(d.num(1, "Column Count"), Some(2.0));
        assert_eq!(d.num(2, "Angle"), Some(90.0));
        d.set_num(1, "Column Count", 5.0);
        d.set_check(2, "Flip", true);
        assert_eq!(d.num(1, "Column Count"), Some(5.0));
        assert!(d.check(2, "Flip") && !d.check(1, "Flip row"));
        let axis = |item| GeomRef::Origin {
            sub: Vec::new(),
            occurrence: ASSEMBLY,
            item,
        };
        d.set_tab(1);
        d.active = Some("column");
        d.pick_ref("X Axis".into(), axis(OriginItem::XAxis), true);
        assert_eq!(d.active, Some("row"), "on to the next empty selector");
        d.active = Some("column");
        d.pick_ref("Y Axis".into(), axis(OriginItem::YAxis), true);
        assert_eq!(d.refs["column"], vec![axis(OriginItem::YAxis)]);
        assert_eq!(d.picks_of("column"), ["Y Axis".to_owned()]);
        d.active = Some("column");
        d.pick("Y Axis".into());
        assert!(d.picks_of("column").is_empty() && d.refs["column"].is_empty());
    }

    /// Selector keys are unique within a panel, labels within a tab (a
    /// selector row's label is its widget id).
    #[test]
    fn selector_keys_are_unique() {
        for k in AsmPlanned::ALL {
            let defs = selector_defs(Planned::Asm(k));
            for (i, a) in defs.iter().enumerate() {
                assert!(
                    defs[i + 1..].iter().all(|b| b.0 != a.0),
                    "{}: key {}",
                    k.title(),
                    a.0
                );
            }
            for pg in k.pages() {
                let labels: Vec<&str> =
                    pg.1.iter()
                        .filter_map(|r| match *r {
                            Sel(_, l, ..) => Some(l),
                            _ => None,
                        })
                        .collect();
                for (i, l) in labels.iter().enumerate() {
                    assert!(
                        !labels[i + 1..].contains(l),
                        "{} {}: {l} twice",
                        k.title(),
                        pg.0
                    );
                }
            }
        }
    }

    /// Pattern: the components, then the feature pattern.
    #[test]
    fn pattern_advances_from_components() {
        let mut d = PartDialog::new(AsmPlanned::Pattern);
        assert_eq!(d.active, Some("components"));
        d.pick("bracket:1 · component".into());
        assert_eq!(d.active, Some("feature"));
    }
}
