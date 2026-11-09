//! The interface of the part commands whose logic comes later (rule 15,
//! `docs/spec/2.0-part-commands.md`): each command opens its whole Inventor
//! 2027 panel in Properties — tabs, selectors that take picks in the view
//! (through the shared selector flow, so Enter, the right-click menu, the
//! pick filter and the status prompt work as in the other commands), every
//! option with its default. OK stays disabled: nothing changes the part
//! (rule 5) until a command's logic is written against its spec rows.
//!
//! Dialogs are data ([`PartPlanned::pages`]): a page is a tab, a row one
//! option. The state ([`PartDialog`]) is not saved; there is nothing to save
//! yet.

use std::collections::HashMap;

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::command_ui::{RowEvents, selector_row};
use super::icons::Icon;
use super::panels::plabel;
use super::select::SelItem;
use super::valuefield::ExprValue;
use crate::command::flow::{Flow, PickKind, Selector, colors};
use crate::part::select as psel;

/// One option of a panel.
#[derive(Clone, Copy, Debug)]
pub(super) enum Row {
    /// A group title.
    Head(&'static str),
    /// A selector: key, label, status-bar prompt, the kinds it takes.
    Sel(
        &'static str,
        &'static str,
        &'static str,
        &'static [PickKind],
    ),
    /// A check box and its default.
    Check(&'static str, bool),
    /// Radio buttons, default index.
    Radio(&'static str, &'static [&'static str], usize),
    /// A drop-down list, default index.
    Combo(&'static str, &'static [&'static str], usize),
    /// A number (expression field), default, unit suffix (`" mm"`, `"°"`,
    /// `""`).
    Num(&'static str, f64, &'static str),
    /// A text box and its default.
    Text(&'static str, &'static str),
    /// A line of explanation.
    Note(&'static str),
    /// A button (shown, disabled until its logic exists).
    Button(&'static str),
}

/// A tab of a panel: its name and rows.
#[derive(Clone, Copy, Debug)]
pub(super) struct Page(pub &'static str, pub &'static [Row]);

use Row::{Button, Check, Combo, Head, Note, Num, Radio, Sel, Text};

// ---- what the selectors take ------------------------------------------------

const FACES: &[PickKind] = &[PickKind::Face, PickKind::PlanarFace];
const EDGES: &[PickKind] = &[PickKind::Edge, PickKind::StraightEdge];
const BODIES: &[PickKind] = &[PickKind::Body];
const FACES_SURFACES: &[PickKind] = &[PickKind::Face, PickKind::PlanarFace, PickKind::Surface];
const SKETCH: &[PickKind] = &[PickKind::Sketch];
const PROFILE: &[PickKind] = &[PickKind::Region, PickKind::Sketch];
const CURVES: &[PickKind] = &[PickKind::SketchCurve, PickKind::Edge];
const LINE: &[PickKind] = &[PickKind::SketchCurve];
const AXIS: &[PickKind] = &[PickKind::StraightEdge, PickKind::Axis];
const PLANE: &[PickKind] = &[PickKind::PlanarFace, PickKind::Plane];
const DIRECTION: &[PickKind] = &[
    PickKind::StraightEdge,
    PickKind::Axis,
    PickKind::PlanarFace,
    PickKind::Plane,
];
const POINTS: &[PickKind] = &[PickKind::Point, PickKind::SketchPoint];
const FEATURES: &[PickKind] = &[PickKind::Face, PickKind::Body];
const TOOL: &[PickKind] = &[
    PickKind::Surface,
    PickKind::Face,
    PickKind::PlanarFace,
    PickKind::Plane,
    PickKind::SketchCurve,
];

const DIRECTIONS: &[&str] = &["Direction 1", "Direction 2", "Symmetric"];
const SKETCH_CURVES: &[PickKind] = &[PickKind::SketchCurve];
const RULES: &[&str] = &[
    "All Edges",
    "Against Part",
    "Free Edges",
    "Against Features",
    "Incident Edges",
];
/// Lip's check box: a click on an edge takes the loop it belongs to.
pub(super) const WHOLE_LOOP: &str = "Whole Loop";
/// One rule of Rule Fillet (a tab of its own).
macro_rules! rule_row {
    ($src:literal, $scope:literal, $axis:literal) => {
        &[
            Check(USE_RULE, true),
            Radio("Source", &["Features", "Faces"], 0),
            Sel(
                $src,
                "Selection",
                "Select the features or faces of this rule",
                FEATURES,
            ),
            Combo("Rule", RULES, 1),
            Sel(
                $scope,
                "Scope",
                "Select the features of the scope (Against Features)",
                FEATURES,
            ),
            Sel($axis, "Axis", "Select the axis (Incident Edges)", DIRECTION),
            Check("Flip Axis", false),
            Num("Tolerance", 1.0, "°"),
            Num("Radius", 1.0, " mm"),
            Check("All Rounds", true),
            Check("All Fillets", true),
        ]
    };
}
/// The check box that keeps a rule of Rule Fillet (off: OK drops it).
pub(super) const USE_RULE: &str = "Use This Rule";
/// The rules Rule Fillet's panel can show; a tab appears once the one before it has a source.
pub(super) const RULE_TABS: usize = 8;
/// The selectors of each rule: source, scope, axis.
pub(super) const RULE_KEYS: [(&str, &str, &str); RULE_TABS] = [
    ("src1", "scope1", "axis1"),
    ("src2", "scope2", "axis2"),
    ("src3", "scope3", "axis3"),
    ("src4", "scope4", "axis4"),
    ("src5", "scope5", "axis5"),
    ("src6", "scope6", "axis6"),
    ("src7", "scope7", "axis7"),
    ("src8", "scope8", "axis8"),
];
const RULE_ROW_1: &[Row] = rule_row!("src1", "scope1", "axis1");
const RULE_ROW_2: &[Row] = rule_row!("src2", "scope2", "axis2");
const RULE_ROW_3: &[Row] = rule_row!("src3", "scope3", "axis3");
const RULE_ROW_4: &[Row] = rule_row!("src4", "scope4", "axis4");
const RULE_ROW_5: &[Row] = rule_row!("src5", "scope5", "axis5");
const RULE_ROW_6: &[Row] = rule_row!("src6", "scope6", "axis6");
const RULE_ROW_7: &[Row] = rule_row!("src7", "scope7", "axis7");
const RULE_ROW_8: &[Row] = rule_row!("src8", "scope8", "axis8");

/// A part command with an interface and no logic yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PartPlanned {
    // 3D Model › Create
    Emboss,
    Decal,
    Mark,
    Derive,
    Import,
    Unwrap,
    // 3D Model › Modify
    Direct,
    CopyObject,
    BendPart,
    // 3D Model › Pattern
    SketchDrivenPattern,
    // 3D Model › Surface
    Stitch,
    Sculpt,
    Patch,
    TrimSurface,
    ExtendSurface,
    RuledSurface,
    ReplaceFace,
    // 3D Model › Plastic Part
    Grill,
    Boss,
    Rest,
    SnapFit,
    RuleFillet,
    Lip,
    // 3D Model › Freeform
    FreeformBox,
    FreeformPlane,
    FreeformCylinder,
    FreeformSphere,
    FreeformTorus,
    FreeformQuadball,
    FreeformFace,
    FreeformConvert,
    // Inspect › Analysis
    Zebra,
    DraftAnalysis,
    Curvature,
    SurfaceAnalysis,
    SectionAnalysis,
    // Manage
    InsertFeatureTemplate,
    ExtractFeatureTemplate,
    CreatePartFamily,
    /// Named geometry for being placed (spec 6.3 §3.5).
    Connection,
    MakePart,
    MakeComponents,
    AuthorFrameProfile,
}

impl PartPlanned {
    /// Every planned command, in ribbon order.
    #[cfg_attr(not(test), allow(dead_code))]
    pub const ALL: [PartPlanned; 43] = [
        PartPlanned::Emboss,
        PartPlanned::Decal,
        PartPlanned::Mark,
        PartPlanned::Derive,
        PartPlanned::Import,
        PartPlanned::Unwrap,
        PartPlanned::Direct,
        PartPlanned::CopyObject,
        PartPlanned::BendPart,
        PartPlanned::SketchDrivenPattern,
        PartPlanned::Stitch,
        PartPlanned::Sculpt,
        PartPlanned::Patch,
        PartPlanned::TrimSurface,
        PartPlanned::ExtendSurface,
        PartPlanned::RuledSurface,
        PartPlanned::ReplaceFace,
        PartPlanned::Grill,
        PartPlanned::Boss,
        PartPlanned::Rest,
        PartPlanned::SnapFit,
        PartPlanned::RuleFillet,
        PartPlanned::Lip,
        PartPlanned::FreeformBox,
        PartPlanned::FreeformPlane,
        PartPlanned::FreeformCylinder,
        PartPlanned::FreeformSphere,
        PartPlanned::FreeformTorus,
        PartPlanned::FreeformQuadball,
        PartPlanned::FreeformFace,
        PartPlanned::FreeformConvert,
        PartPlanned::Zebra,
        PartPlanned::DraftAnalysis,
        PartPlanned::Curvature,
        PartPlanned::SurfaceAnalysis,
        PartPlanned::SectionAnalysis,
        PartPlanned::InsertFeatureTemplate,
        PartPlanned::ExtractFeatureTemplate,
        PartPlanned::CreatePartFamily,
        PartPlanned::Connection,
        PartPlanned::MakePart,
        PartPlanned::MakeComponents,
        PartPlanned::AuthorFrameProfile,
    ];

    /// The panel's title (Inventor's command name).
    pub fn title(self) -> &'static str {
        use PartPlanned::*;
        match self {
            Emboss => "Emboss",
            Decal => "Decal",
            Mark => "Mark",
            Derive => "Derive",
            Import => "Import",
            Unwrap => "Unwrap",
            Direct => "Direct Edit",
            CopyObject => "Copy Object",
            BendPart => "Bend Part",
            SketchDrivenPattern => "Sketch Driven Pattern",
            Stitch => "Stitch",
            Sculpt => "Sculpt",
            Patch => "Boundary Patch",
            TrimSurface => "Trim Surface",
            ExtendSurface => "Extend Surface",
            RuledSurface => "Ruled Surface",
            ReplaceFace => "Replace Face",
            Grill => "Grill",
            Boss => "Boss",
            Rest => "Rest",
            SnapFit => "Snap Fit",
            RuleFillet => "Rule Fillet",
            Lip => "Lip",
            FreeformBox => "Freeform Box",
            FreeformPlane => "Freeform Plane",
            FreeformCylinder => "Freeform Cylinder",
            FreeformSphere => "Freeform Sphere",
            FreeformTorus => "Freeform Torus",
            FreeformQuadball => "Freeform Quadball",
            FreeformFace => "Freeform Face",
            FreeformConvert => "Convert to Freeform",
            Zebra => "Zebra Analysis",
            DraftAnalysis => "Draft Analysis",
            Curvature => "Curvature Analysis",
            SurfaceAnalysis => "Surface Analysis",
            SectionAnalysis => "Section Analysis",
            InsertFeatureTemplate => "Insert Feature Template",
            ExtractFeatureTemplate => "Extract Feature Template",
            CreatePartFamily => "Create Part Family",
            Connection => "Connection",
            MakePart => "Make Part",
            MakeComponents => "Make Components",
            AuthorFrameProfile => "Author Frame Profile",
        }
    }

    /// The panel's icon (the command's own, `gui/icons.rs`).
    pub fn icon(self) -> Icon {
        use PartPlanned::*;
        match self {
            Emboss => Icon::Emboss,
            Decal => Icon::Decal,
            Mark => Icon::Mark,
            Derive => Icon::Derive,
            Import => Icon::Import,
            Unwrap => Icon::Unwrap,
            Direct => Icon::Direct,
            CopyObject => Icon::CopyObject,
            BendPart => Icon::BendPart,
            SketchDrivenPattern => Icon::SketchPattern,
            Stitch => Icon::Stitch,
            Sculpt => Icon::Sculpt,
            Patch => Icon::Patch,
            TrimSurface => Icon::TrimSurface,
            ExtendSurface => Icon::ExtendSurface,
            RuledSurface => Icon::RuledSurface,
            ReplaceFace => Icon::ReplaceFace,
            Grill => Icon::Grill,
            Boss => Icon::Boss,
            Rest => Icon::Rest,
            SnapFit => Icon::SnapFit,
            RuleFillet => Icon::RuleFillet,
            Lip => Icon::Lip,
            FreeformBox => Icon::FfBox,
            FreeformPlane => Icon::FfPlane,
            FreeformCylinder => Icon::FfCylinder,
            FreeformSphere => Icon::FfSphere,
            FreeformTorus => Icon::FfTorus,
            FreeformQuadball => Icon::FfQuadball,
            FreeformFace => Icon::FfFace,
            FreeformConvert => Icon::FfConvert,
            Zebra => Icon::Zebra,
            DraftAnalysis => Icon::DraftAnalysis,
            Curvature => Icon::Curvature,
            SurfaceAnalysis => Icon::SurfaceAnalysis,
            SectionAnalysis => Icon::SectionAnalysis,
            InsertFeatureTemplate => Icon::InsertFeatureTemplate,
            ExtractFeatureTemplate => Icon::ExtractFeatureTemplate,
            CreatePartFamily => Icon::CreatePartFamily,
            Connection => Icon::Connection,
            MakePart => Icon::MakePart,
            MakeComponents => Icon::MakeComponents,
            AuthorFrameProfile => Icon::AuthorFrameProfile,
        }
    }

    /// The `docs/FEATURES.md` section its logic belongs to.
    pub fn section(self) -> &'static str {
        use PartPlanned::*;
        match self {
            Emboss | Decal | Mark => "2.17",
            Derive => "5.7",
            Import => "7.7",
            Unwrap | Stitch | Sculpt | Patch | TrimSurface | ExtendSurface | RuledSurface => "5.4",
            ReplaceFace => "2.14",
            Direct => "5.5",
            CopyObject | BendPart => "2.13",
            SketchDrivenPattern => "2.11",
            Grill | Boss | Rest | SnapFit | RuleFillet | Lip => "5.8",
            FreeformBox | FreeformPlane | FreeformCylinder | FreeformSphere | FreeformTorus
            | FreeformQuadball | FreeformFace | FreeformConvert => "5.5",
            Zebra | DraftAnalysis | Curvature | SurfaceAnalysis | SectionAnalysis => "0.10",
            InsertFeatureTemplate | ExtractFeatureTemplate | CreatePartFamily => "5.7",
            Connection => "6.3",
            MakePart | MakeComponents => "3.10",
            AuthorFrameProfile => "6.1",
        }
    }

    /// One line on what the command does (under the options).
    pub fn about(self) -> &'static str {
        use PartPlanned::*;
        match self {
            Emboss => {
                "Raises or recesses a sketch profile or text on a face, or from the sketch plane."
            }
            Decal => "Puts a sketch image on faces as an appearance (projected or wrapped).",
            Mark => "Marks text or sketch curves on faces for laser marking or etching.",
            Derive => {
                "Inserts another part or assembly as a linked base (bodies, sketches, parameters)."
            }
            Import => "Brings a STEP file's solids and surfaces in as one base feature.",
            Unwrap => "Flattens a group of faces into a flat surface.",
            Direct => "Moves, resizes, scales, rotates or deletes faces without history.",
            CopyObject => "Copies or moves faces or bodies into a surface, composite or solid.",
            BendPart => "Bends a part about a sketch line (not for sheet metal).",
            SketchDrivenPattern => "Copies features or bodies at the points of a sketch.",
            Stitch => "Joins surfaces into a quilt, or a solid when they close a volume.",
            Sculpt => "Adds or removes material bounded by surfaces.",
            Patch => "A surface inside a closed loop of edges or sketch curves.",
            TrimSurface => "Trims a surface by a cutting tool.",
            ExtendSurface => "Lengthens surface edges by a distance or up to a face.",
            RuledSurface => "A surface from edges: normal or tangent to a face, or along a vector.",
            ReplaceFace => "Replaces faces by surfaces or planes: the body follows the new face.",
            Grill => "Vent openings: boundary, islands, ribs and spars.",
            Boss => "A screw boss (head or thread side) with stiffening ribs.",
            Rest => "A raised or recessed platform on a face.",
            SnapFit => "A cantilever snap-fit hook or loop.",
            RuleFillet => {
                "Fillets chosen by rules (edges of a feature, against a face, free edges)."
            }
            Lip => "A lip or groove along the joint of two housing halves.",
            FreeformBox | FreeformPlane | FreeformCylinder | FreeformSphere | FreeformTorus
            | FreeformQuadball => "A T-spline primitive shaped by dragging its control cage.",
            FreeformFace => "A T-spline face from clicked vertices.",
            FreeformConvert => "Converts a solid body into a freeform (T-spline) body.",
            Zebra => "Stripe reflections on faces, to judge their continuity.",
            DraftAnalysis => "Colours faces by their angle to a pull direction.",
            Curvature => "Curvature combs on edges and faces.",
            SurfaceAnalysis => "Colours faces by their curvature.",
            SectionAnalysis => "A live section through the part with a wall-thickness check.",
            InsertFeatureTemplate => {
                "Inserts a saved feature template: placement plane, sizes, position."
            }
            ExtractFeatureTemplate => {
                "Saves selected features as a feature template with size parameters."
            }
            Connection => {
                "Names a circular edge this part lands with: placed, it goes to a hole or a shaft that fits."
            }
            CreatePartFamily => {
                "A family table: parameters, properties and suppression per member."
            }
            MakePart => "A new part derived from chosen bodies of this one.",
            MakeComponents => "Each chosen body becomes a part in an assembly.",
            AuthorFrameProfile => {
                "Saves a closed sketch region as your own frame member profile (Insert Frame lists it)."
            }
        }
    }

    /// The panel content: tabs and rows (spec 2.0 §3).
    pub fn pages(self) -> &'static [Page] {
        use PartPlanned::*;
        match self {
            Emboss => &[Page(
                "Emboss",
                &[
                    Sel(
                        "profile",
                        "Profile",
                        "Select the closed profiles or the sketch text",
                        PROFILE,
                    ),
                    Radio(
                        "Type",
                        &[
                            "Emboss from Face",
                            "Engrave from Face",
                            "Emboss/Engrave from Plane",
                        ],
                        0,
                    ),
                    Num("Depth", 1.0, " mm"),
                    Combo("Direction", DIRECTIONS, 0),
                    Num("Taper", 0.0, "°"),
                    Button("Top Face Appearance..."),
                    Check("Wrap to Face", false),
                    Sel(
                        "face",
                        "Face",
                        "Select the face to wrap the profile onto",
                        FACES,
                    ),
                ],
            )],
            Decal => &[Page(
                "Decal",
                &[
                    Sel(
                        "image",
                        "Image",
                        "Select an image inserted into a sketch",
                        SKETCH,
                    ),
                    Sel("face", "Face", "Select the faces for the decal", FACES),
                    Check("Wrap to Face", false),
                    Check("Chain Faces", false),
                    Note("Images in sketches come with section 1.11"),
                ],
            )],
            Mark => &[Page(
                "Mark",
                &[
                    Sel(
                        "profile",
                        "Profile",
                        "Select the text or sketch curves to mark",
                        PROFILE,
                    ),
                    Sel("faces", "Faces", "Select the faces to mark", FACES),
                    Radio("Mark Type", &["Mark Surface", "Mark Through"], 0),
                    Check("Wrap to Face", false),
                    Check("Flip Direction", false),
                ],
            )],
            Derive => &[
                Page(
                    "Derive",
                    &[
                        Button("Browse..."),
                        Text("Source", ""),
                        Radio(
                            "Derive Style",
                            &[
                                "Single solid body merging seams",
                                "Single solid body keeping seams",
                                "Keep each solid as a body",
                                "Body as work surface",
                            ],
                            2,
                        ),
                        Head("Include"),
                        Check("Solid Bodies", true),
                        Check("Surface Bodies", false),
                        Check("Sketches", false),
                        Check("Work Geometry", false),
                        Check("Parameters", false),
                        Check("Show All Objects", false),
                    ],
                ),
                Page(
                    "Options",
                    &[
                        Num("Scale Factor", 1.0, ""),
                        Check("Mirror Part", false),
                        Combo("Mirror Plane", &["XY Plane", "XZ Plane", "YZ Plane"], 0),
                    ],
                ),
            ],
            Import => &[
                Page(
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
                            1,
                        ),
                        Head("Part"),
                        Radio("Surfaces as", &["Composite", "Individual"], 1),
                        Check("Stitch", false),
                        Radio("Placement", &["At the origin", "Pick a point"], 0),
                    ],
                ),
                Page(
                    "Select",
                    &[Note(
                        "Every body of the file is imported (selective import: spec 7.7a 3.8)",
                    )],
                ),
            ],
            Unwrap => &[Page(
                "Unwrap",
                &[
                    Sel(
                        "faces",
                        "Faces",
                        "Select the faces to unwrap (a tangent chain)",
                        FACES,
                    ),
                    Sel(
                        "edge",
                        "Fixed Edge",
                        "Select the edge that stays in place (optional)",
                        EDGES,
                    ),
                    Sel(
                        "point",
                        "Fixed Point",
                        "Select the point that stays in place (optional)",
                        POINTS,
                    ),
                    Combo("Stiffness", &["Rigid", "Medium", "Plastic"], 0),
                    Num("Rotation", 0.0, "°"),
                ],
            )],
            Direct => &[
                Page(
                    "Direct Edit",
                    &[
                        Radio(
                            "Operation",
                            &["Move", "Size", "Scale", "Rotate", "Delete"],
                            0,
                        ),
                        Radio("Select", &["Faces", "Solids"], 0),
                        Sel(
                            "objects",
                            "Objects",
                            "Select the faces or bodies to edit",
                            FEATURES,
                        ),
                        Button("Locate"),
                        Check("Snap to geometry", true),
                        Check("Automatic Face Chain", true),
                    ],
                ),
                Page(
                    "Values",
                    &[
                        Head("Move"),
                        Combo("Move along", &["Free Move", "X", "Y", "Z"], 0),
                        Num("X", 0.0, " mm"),
                        Num("Y", 0.0, " mm"),
                        Num("Z", 0.0, " mm"),
                        Head("Size"),
                        Radio("Size", &["Offset", "Radius", "Diameter"], 0),
                        Num("Value", 0.0, " mm"),
                        Head("Scale"),
                        Radio("Scale", &["Uniform", "Non-uniform"], 0),
                        Num("Factor", 1.0, ""),
                        Head("Rotate"),
                        Sel("axis", "Axis", "Select the rotation axis", AXIS),
                        Num("Angle", 0.0, "°"),
                        Head("Delete"),
                        Check("Heal", true),
                    ],
                ),
            ],
            CopyObject => &[Page(
                "Copy Object",
                &[
                    Radio("Select", &["Face", "Body"], 0),
                    Sel(
                        "objects",
                        "Objects",
                        "Select the faces or bodies to copy",
                        FEATURES,
                    ),
                    Radio(
                        "Create New",
                        &["Group", "Surface", "Composite", "Solid", "Multiple Solids"],
                        1,
                    ),
                    Check("Select Existing", false),
                    Check("Associative", true),
                    Check("Delete Original", false),
                    Note("Select Existing and Associative work when editing a part in an assembly"),
                ],
            )],
            BendPart => &[Page(
                "Bend Part",
                &[
                    Sel(
                        "line",
                        "Bend Line",
                        "Select the sketch line to bend about",
                        LINE,
                    ),
                    Sel(
                        "bodies",
                        "Bodies",
                        "Select the bodies to bend (multi-body parts)",
                        BODIES,
                    ),
                    Combo(
                        "Bend",
                        &[
                            "Radius + Angle",
                            "Radius + Arc Length",
                            "Arc Length + Angle",
                        ],
                        0,
                    ),
                    Num("Radius", 10.0, " mm"),
                    Num("Angle", 90.0, "°"),
                    Num("Arc Length", 15.708, " mm"),
                    Radio("Side", &["Left", "Right", "Both sides"], 0),
                    Check("Flip Direction", false),
                ],
            )],
            SketchDrivenPattern => &[Page(
                "Sketch Driven Pattern",
                &[
                    Radio("Pattern", &["Features", "Solids"], 0),
                    Sel(
                        "features",
                        "Features",
                        "Select the features or bodies to copy",
                        FEATURES,
                    ),
                    Sel(
                        "sketch",
                        "Sketch",
                        "Select the sketch whose points place the copies",
                        SKETCH,
                    ),
                    Sel(
                        "base",
                        "Base Point",
                        "Select the base point (optional)",
                        POINTS,
                    ),
                    Sel(
                        "faces",
                        "Faces",
                        "Select a face for the copies' normal (optional)",
                        FACES,
                    ),
                    Sel(
                        "solid",
                        "Solid",
                        "Select the solid that takes the copies",
                        BODIES,
                    ),
                    Radio("Output", &["Join", "New Solid"], 0),
                    Radio("Creation Method", &["Identical", "Adjust"], 0),
                ],
            )],
            Stitch => &[
                Page(
                    "Stitch",
                    &[
                        Sel(
                            "surfaces",
                            "Surfaces",
                            "Select the surfaces to stitch",
                            FACES_SURFACES,
                        ),
                        Num("Maximum Tolerance", 0.00025, " mm"),
                        Button("Find Remaining Free Edges"),
                        Check("Keep as Surface", false),
                    ],
                ),
                Page(
                    "Analysis",
                    &[
                        Check("Show Near-Tangent Edges", false),
                        Note("Free edges and gaps are listed here after Find Remaining Free Edges"),
                    ],
                ),
            ],
            Sculpt => &[Page(
                "Sculpt",
                &[
                    Sel(
                        "surfaces",
                        "Surfaces",
                        "Select the surfaces and faces that bound the volume",
                        TOOL,
                    ),
                    Sel(
                        "solids",
                        "Solids",
                        "Select the solid body (multi-body parts)",
                        BODIES,
                    ),
                    Radio("Operation", &["Add", "Remove", "New Solid"], 0),
                    Combo("Side", &["One side", "Both sides"], 0),
                    Check("Flip", false),
                    Check("Automatic Face Chain", true),
                ],
            )],
            Patch => &[Page(
                "Boundary Patch",
                &[
                    Sel(
                        "boundary",
                        "Boundary",
                        "Select the closed loop of edges or sketch curves",
                        CURVES,
                    ),
                    Combo(
                        "Condition",
                        &["Contact (G0)", "Tangent (G1)", "Smooth (G2)"],
                        0,
                    ),
                    Check("Automatic Edge Chain", true),
                    Sel(
                        "rails",
                        "Guide Rails",
                        "Select guide curves (optional)",
                        LINE,
                    ),
                    Radio("Output", &["Surface", "Solid"], 0),
                ],
            )],
            TrimSurface => &[Page(
                "Trim Surface",
                &[
                    Sel(
                        "tool",
                        "Cutting Tool",
                        "Select the surface, face, plane or curve that cuts",
                        TOOL,
                    ),
                    Sel(
                        "remove",
                        "Remove",
                        "Select the pieces to remove",
                        FACES_SURFACES,
                    ),
                    Check("Invert Selection", false),
                ],
            )],
            ExtendSurface => &[Page(
                "Extend Surface",
                &[
                    Sel(
                        "edges",
                        "Edges",
                        "Select the surface edges to extend",
                        EDGES,
                    ),
                    Check("Edge Chain", true),
                    Radio("Extents", &["Distance", "To"], 0),
                    Num("Distance", 5.0, " mm"),
                    Sel("to", "To", "Select the face or plane to extend to", PLANE),
                    Radio("Extend", &["Extend", "Stretch"], 0),
                ],
            )],
            RuledSurface => &[Page(
                "Ruled Surface",
                &[
                    Radio("Type", &["Normal", "Tangent", "Sweep"], 0),
                    Sel("edges", "Edges", "Select the model edges", EDGES),
                    Num("Distance", 5.0, " mm"),
                    Num("Angle", 0.0, "°"),
                    Sel(
                        "vector",
                        "Vector",
                        "Select the sweep direction (Sweep type)",
                        DIRECTION,
                    ),
                    Check("Automatic Edge Chain", true),
                    Check("Flip", false),
                    Button("Alternate All Faces"),
                ],
            )],
            ReplaceFace => &[Page(
                "Replace Face",
                &[
                    Sel(
                        "existing",
                        "Existing Faces",
                        "Select the faces to replace",
                        FACES,
                    ),
                    Sel(
                        "new",
                        "New Faces",
                        "Select the surfaces or planes that replace them",
                        TOOL,
                    ),
                    Check("Automatic Face Chain", true),
                ],
            )],
            Grill => &[
                Page(
                    "Boundary",
                    &[
                        Sel(
                            "boundary",
                            "Profile",
                            "Click inside the closed boundary profile",
                            PROFILE,
                        ),
                        Sel("solid", "Solid", "Select the target body", BODIES),
                        Check("Flip", false),
                        Num("Thickness", 1.0, " mm"),
                        Num("Height", 2.0, " mm"),
                        Num("Outside Height", 0.0, " mm"),
                    ],
                ),
                Page(
                    "Island",
                    &[
                        Sel(
                            "island",
                            "Profile",
                            "Click inside the island profiles",
                            PROFILE,
                        ),
                        Num("Thickness", 0.0, " mm"),
                    ],
                ),
                Page(
                    "Rib",
                    &[
                        Sel(
                            "ribs",
                            "Profiles",
                            "Select the ribs' centre lines",
                            SKETCH_CURVES,
                        ),
                        Num("Thickness", 1.0, " mm"),
                        Num("Height", 2.0, " mm"),
                        Num("Top Offset", 0.0, " mm"),
                    ],
                ),
                Page(
                    "Spar",
                    &[
                        Sel(
                            "spars",
                            "Profiles",
                            "Select the spars' centre lines",
                            SKETCH_CURVES,
                        ),
                        Num("Thickness", 1.0, " mm"),
                        Num("Top Offset", 0.0, " mm"),
                        Num("Bottom Offset", 0.0, " mm"),
                    ],
                ),
                Page(
                    "Draft",
                    &[
                        Num("Draft Angle", 0.0, "°"),
                        Check("Parting Element", false),
                        Head("Flow Area"),
                        Button("Compute Flow Area"),
                    ],
                ),
            ],
            Boss => &[
                Page(
                    "Shape",
                    &[
                        Radio("Placement", &["From Sketch", "On Point"], 0),
                        Radio("Type", &["Head", "Thread"], 1),
                        Sel("centers", "Centers", "Select the boss centres", POINTS),
                        Sel(
                            "direction",
                            "Direction",
                            "Select the boss direction (On Point)",
                            DIRECTION,
                        ),
                        Sel("solid", "Solid", "Select the target body", BODIES),
                        Check("Flip", false),
                        Num("Offset", 0.0, " mm"),
                        Num("Fillet", 0.0, " mm"),
                    ],
                ),
                Page(
                    "Head",
                    &[
                        Radio("Seat", &["Counterbore", "Countersink"], 0),
                        Num("Wall Thickness", 1.5, " mm"),
                        Num("Shank Height", 1.5, " mm"),
                        Num("Clamp Height", 0.5, " mm"),
                        Num("Shank Diameter", 3.0, " mm"),
                        Num("Clamp Diameter", 7.0, " mm"),
                        Num("Head Diameter", 8.0, " mm"),
                        Num("Countersink Angle", 90.0, "°"),
                        Num("Outer Draft Angle", 0.0, "°"),
                        Num("Inner Draft Angle", 0.0, "°"),
                        Num("Shaft Draft Angle", 0.0, "°"),
                        Num("Clamp Draft Angle", 0.0, "°"),
                    ],
                ),
                Page(
                    "Thread",
                    &[
                        Check("Hole", true),
                        Radio("Hole Type", &["Depth", "Full Depth", "Through"], 1),
                        Num("Thread Diameter", 6.0, " mm"),
                        Num("Thread Hole Diameter", 2.5, " mm"),
                        Num("Thread Hole Depth", 5.0, " mm"),
                        Num("Outer Draft Angle", 0.0, "°"),
                        Num("Inner Draft Angle", 0.0, "°"),
                    ],
                ),
                Page(
                    "Ribs",
                    &[
                        Check("Stiffening Ribs", false),
                        Num("Number of Ribs", 4.0, ""),
                        Num("Rib Thickness", 1.0, " mm"),
                        Num("Rib Draft", 0.0, "°"),
                        Num("Shoulder Length", 3.0, " mm"),
                        Num("Top Offset", 1.0, " mm"),
                        Num("Shoulder Radius", 0.0, " mm"),
                        Num("Flare Angle", 0.0, "°"),
                        Num("Rib Radius", 0.0, " mm"),
                        Num("Blend Radius", 0.0, " mm"),
                        Num("Start Angle", 0.0, "°"),
                        Num("Space Angle", 360.0, "°"),
                    ],
                ),
            ],
            Rest => &[
                Page(
                    "Shape",
                    &[
                        Sel(
                            "profile",
                            "Profile",
                            "Click inside the rest's closed profile",
                            PROFILE,
                        ),
                        Sel("solid", "Solid", "Select the target body", BODIES),
                        Check("Flip", false),
                        Num("Thickness", 1.0, " mm"),
                        Radio("Thickness Side", &["Inside", "Outside", "Both Sides"], 0),
                        Radio("Platform", &["Through All", "Distance", "To Surface"], 0),
                        Num("Distance", 2.0, " mm"),
                        Sel(
                            "to",
                            "Surface",
                            "Select the surface the platform runs to",
                            PLANE,
                        ),
                        Num("Offset", 0.0, " mm"),
                    ],
                ),
                Page(
                    "More",
                    &[
                        Radio("Landing", &["Distance", "To Surface"], 0),
                        Num("Landing Distance", 0.0, " mm"),
                        Sel(
                            "land",
                            "Surface",
                            "Select the surface of the landing",
                            PLANE,
                        ),
                        Num("Landing Offset", 0.0, " mm"),
                        Num("Landing Taper", 0.0, "°"),
                        Num("Clearance Taper", 0.0, "°"),
                    ],
                ),
            ],
            SnapFit => &[
                Page(
                    "Shape",
                    &[
                        Radio(
                            "Type",
                            &["Cantilever Snap Fit Hook", "Cantilever Snap Fit Loop"],
                            0,
                        ),
                        Radio("Placement", &["From Sketch", "On Point"], 0),
                        Sel("centers", "Centers", "Select the snap fit points", POINTS),
                        Sel(
                            "direction",
                            "Direction",
                            "Select the beam direction (On Point)",
                            DIRECTION,
                        ),
                        Sel("solid", "Solid", "Select the target body", BODIES),
                        Check("Flip", false),
                        Radio("Hook Direction", &["0°", "90°", "180°", "270°"], 0),
                        Radio("Extents", &["Extend to next", "Stop at the point"], 0),
                    ],
                ),
                Page(
                    "Beam",
                    &[
                        Num("Thickness at Wall", 1.5, " mm"),
                        Num("Thickness at Hook", 1.0, " mm"),
                        Num("Beam Length", 10.0, " mm"),
                        Num("Width at Wall", 4.0, " mm"),
                        Num("Width at Hook", 4.0, " mm"),
                    ],
                ),
                Page(
                    "Hook",
                    &[
                        Num("Tip Length", 0.5, " mm"),
                        Num("Undercut Depth", 1.0, " mm"),
                        Num("Hook Length", 4.0, " mm"),
                        Num("Retention Angle", 90.0, "°"),
                        Num("Insertion Angle", 30.0, "°"),
                    ],
                ),
                Page(
                    "Clip",
                    &[
                        Num("Clip Length", 10.0, " mm"),
                        Num("Clip Width", 8.0, " mm"),
                        Num("Thickness at Wall", 1.5, " mm"),
                        Num("Thickness at Top", 1.0, " mm"),
                    ],
                ),
                Page(
                    "Catch",
                    &[
                        Num("Side Width", 1.5, " mm"),
                        Num("Top Width", 1.5, " mm"),
                        Num("Opening Length", 4.0, " mm"),
                    ],
                ),
            ],
            RuleFillet => &[
                Page("Rule 1", RULE_ROW_1),
                Page("Rule 2", RULE_ROW_2),
                Page("Rule 3", RULE_ROW_3),
                Page("Rule 4", RULE_ROW_4),
                Page("Rule 5", RULE_ROW_5),
                Page("Rule 6", RULE_ROW_6),
                Page("Rule 7", RULE_ROW_7),
                Page("Rule 8", RULE_ROW_8),
                Page(
                    "More",
                    &[
                        Sel(
                            "exclude",
                            "Exclude Faces",
                            "Select faces whose edges stay as they are",
                            FACES,
                        ),
                        Check("Remove Merged Faces", false),
                    ],
                ),
            ],
            Lip => &[
                Page(
                    "Shape",
                    &[
                        Radio("Type", &["Lip", "Groove"], 0),
                        Sel(
                            "path",
                            "Path Edges",
                            "Click an edge of the rim: the whole loop is taken",
                            EDGES,
                        ),
                        Check(WHOLE_LOOP, true),
                        Radio(
                            "Orientation",
                            &["Automatic", "Guide Face", "Pull Direction"],
                            0,
                        ),
                        Sel(
                            "guide",
                            "Guide Face",
                            "Select the wall next to the path",
                            FACES,
                        ),
                        Sel(
                            "pull",
                            "Pull Direction",
                            "Select the pull direction",
                            DIRECTION,
                        ),
                        Sel("solid", "Solid", "Select the target body", BODIES),
                        Radio("Path Extents", &["Full", "Between Points"], 0),
                    ],
                ),
                Page(
                    "Lip / Groove",
                    &[
                        Num("Outside Angle", 0.0, "°"),
                        Num("Inside Angle", 0.0, "°"),
                        Num("Height", 1.0, " mm"),
                        Num("Shoulder Width", 0.0, " mm"),
                        Num("Width", 0.75, " mm"),
                        Num("Clearance", 0.0, " mm"),
                    ],
                ),
            ],
            FreeformBox => &[Page(
                "Box",
                &[
                    Sel("plane", "Plane", "Select the base plane", PLANE),
                    Sel("center", "Center", "Click the centre point", POINTS),
                    Num("Length", 50.0, " mm"),
                    Num("Width", 50.0, " mm"),
                    Num("Height", 50.0, " mm"),
                    Num("Length Faces", 4.0, ""),
                    Num("Width Faces", 4.0, ""),
                    Num("Height Faces", 4.0, ""),
                    Check("Symmetry X", false),
                    Check("Symmetry Y", false),
                    Check("Symmetry Z", false),
                    Combo("Direction", DIRECTIONS, 0),
                ],
            )],
            FreeformPlane => &[Page(
                "Plane",
                &[
                    Sel("plane", "Plane", "Select the base plane", PLANE),
                    Sel("center", "Center", "Click the centre point", POINTS),
                    Num("Length", 50.0, " mm"),
                    Num("Width", 50.0, " mm"),
                    Num("Length Faces", 4.0, ""),
                    Num("Width Faces", 4.0, ""),
                    Check("Symmetry X", false),
                    Check("Symmetry Y", false),
                ],
            )],
            FreeformCylinder => &[Page(
                "Cylinder",
                &[
                    Sel("plane", "Plane", "Select the base plane", PLANE),
                    Sel("center", "Center", "Click the centre point", POINTS),
                    Num("Diameter", 50.0, " mm"),
                    Num("Height", 50.0, " mm"),
                    Num("Around Faces", 8.0, ""),
                    Num("Height Faces", 4.0, ""),
                    Check("Symmetry X", false),
                    Check("Symmetry Y", false),
                    Check("Symmetry Z", false),
                    Combo("Direction", DIRECTIONS, 0),
                ],
            )],
            FreeformSphere => &[Page(
                "Sphere",
                &[
                    Sel("plane", "Plane", "Select the base plane", PLANE),
                    Sel("center", "Center", "Click the centre point", POINTS),
                    Num("Diameter", 50.0, " mm"),
                    Num("Longitude Faces", 8.0, ""),
                    Num("Latitude Faces", 4.0, ""),
                    Check("Symmetry X", false),
                    Check("Symmetry Y", false),
                    Check("Symmetry Z", false),
                ],
            )],
            FreeformTorus => &[Page(
                "Torus",
                &[
                    Sel("plane", "Plane", "Select the base plane", PLANE),
                    Sel("center", "Center", "Click the centre point", POINTS),
                    Num("Diameter", 50.0, " mm"),
                    Num("Ring Diameter", 10.0, " mm"),
                    Num("Around Faces", 8.0, ""),
                    Num("Ring Faces", 4.0, ""),
                    Check("Symmetry X", false),
                    Check("Symmetry Y", false),
                    Check("Symmetry Z", false),
                ],
            )],
            FreeformQuadball => &[Page(
                "Quadball",
                &[
                    Sel("plane", "Plane", "Select the base plane", PLANE),
                    Sel("center", "Center", "Click the centre point", POINTS),
                    Num("Diameter", 50.0, " mm"),
                    Num("Span Faces", 4.0, ""),
                    Check("Symmetry X", false),
                    Check("Symmetry Y", false),
                    Check("Symmetry Z", false),
                ],
            )],
            FreeformFace => &[Page(
                "Face",
                &[
                    Radio("Type", &["Simple", "Edge", "Point by Point"], 0),
                    Sel("points", "Vertices", "Click the face's vertices", POINTS),
                    Num("Faces", 1.0, ""),
                    Check("Symmetry", false),
                ],
            )],
            FreeformConvert => &[Page(
                "Convert",
                &[
                    Sel("body", "Body", "Select the solid body to convert", BODIES),
                    Num("Faces", 8.0, ""),
                    Check("Keep the original body", false),
                ],
            )],
            Zebra => &[Page(
                "Zebra",
                &[
                    Radio("Direction", &["Horizontal", "Vertical", "Radial"], 0),
                    Num("Thickness", 5.0, ""),
                    Num("Opacity", 50.0, " %"),
                    Combo("Quality", &["Low", "Medium", "High"], 1),
                    Radio("Faces", &["All", "Selected"], 0),
                    Sel("faces", "Faces", "Select the faces to analyse", FACES),
                ],
            )],
            DraftAnalysis => &[Page(
                "Draft Analysis",
                &[
                    Sel(
                        "pull",
                        "Pull Direction",
                        "Select the pull direction",
                        DIRECTION,
                    ),
                    Num("Minimum Angle", -3.0, "°"),
                    Num("Maximum Angle", 3.0, "°"),
                    Radio("Display", &["Gradient", "Banded"], 0),
                    Radio("Faces", &["All", "Selected"], 0),
                    Sel("faces", "Faces", "Select the faces to analyse", FACES),
                    Combo("Quality", &["Low", "Medium", "High"], 1),
                ],
            )],
            Curvature => &[Page(
                "Curvature",
                &[
                    Sel(
                        "objects",
                        "Faces / Curves",
                        "Select the faces or edges",
                        FACES_SURFACES,
                    ),
                    Num("Comb Density", 5.0, ""),
                    Num("Comb Scale", 5.0, ""),
                    Radio("Display", &["Edges", "Faces"], 0),
                ],
            )],
            SurfaceAnalysis => &[Page(
                "Surface Analysis",
                &[
                    Combo("Type", &["Gaussian", "Mean", "Maximum", "Minimum"], 0),
                    Num("Range Minimum", -0.01, ""),
                    Num("Range Maximum", 0.01, ""),
                    Radio("Display", &["Gradient", "Banded"], 0),
                    Radio("Faces", &["All", "Selected"], 0),
                    Sel("faces", "Faces", "Select the faces to analyse", FACES),
                ],
            )],
            SectionAnalysis => &[Page(
                "Section Analysis",
                &[
                    Radio("Mode", &["Simple", "Advanced"], 0),
                    Sel("plane", "Section Plane", "Select the section plane", PLANE),
                    Num("Offset", 0.0, " mm"),
                    Num("Minimum Wall Thickness", 1.0, " mm"),
                    Num("Maximum Wall Thickness", 5.0, " mm"),
                    Radio("Show", &["Inside", "Outside"], 0),
                ],
            )],
            InsertFeatureTemplate => &[
                Page("Select", &[Button("Browse..."), Text("Template", "")]),
                Page(
                    "Position",
                    &[
                        Sel(
                            "plane",
                            "Profile Plane",
                            "Select the face or plane to place it on",
                            PLANE,
                        ),
                        Num("Angle", 0.0, "°"),
                        Check("Flip", false),
                    ],
                ),
                Page(
                    "Size",
                    &[Note(
                        "The template's size parameters are listed here (name, value)",
                    )],
                ),
                Page(
                    "Precise Position",
                    &[Check("Activate sketch edit immediately", false)],
                ),
            ],
            ExtractFeatureTemplate => &[Page(
                "Extract Feature Template",
                &[
                    Radio(
                        "Type",
                        &["Standard Feature Template", "Sheet Metal Punch Template"],
                        0,
                    ),
                    Sel(
                        "features",
                        "Selected Features",
                        "Select the features to extract",
                        FACES,
                    ),
                    Note("Size Parameters: name, value, limit (None / Range / List), prompt"),
                    Note("Position Geometry: the placement inputs and their prompts"),
                    Button("Save..."),
                ],
            )],
            Connection => &[Page(
                "Connection",
                &[
                    Sel(
                        "edge",
                        "Circular Edge",
                        "Select the circular edge that lands on the target",
                        &[PickKind::Edge],
                    ),
                    Radio("Role", &["Goes into a hole", "Goes over a shaft"], 0),
                    Text("Name", ""),
                    Text("Diameter Column", ""),
                    Note(
                        "Placing this part then offers the holes or shafts of this diameter. In a family, name the table's column that holds it.",
                    ),
                ],
            )],
            CreatePartFamily => &[
                Page(
                    "Parameters",
                    &[
                        Note("Model and user parameters; >> adds a column to the table"),
                        Button("Add Member"),
                    ],
                ),
                Page(
                    "Properties",
                    &[Note(
                        "Properties as table columns (Part Number, Description…)",
                    )],
                ),
                Page(
                    "Suppression",
                    &[Note("Features computed or suppressed per member")],
                ),
                Page("Connections", &[Note("Connections per member")]),
                Page(
                    "Work Features",
                    &[Note("Work features included per member")],
                ),
                Page(
                    "Threads",
                    &[Note("Thread designation and class per member")],
                ),
                Page(
                    "Other",
                    &[
                        Text("Member Name", "Member1"),
                        Text("Part Number", ""),
                        Check("Key column", false),
                    ],
                ),
            ],
            MakePart => &[Page(
                "Make Part",
                &[
                    Sel(
                        "bodies",
                        "Bodies",
                        "Select the bodies for the new part",
                        BODIES,
                    ),
                    Text("New part name", "Part2"),
                    Combo("Template", &["Standard (mm)", "Sheet Metal (mm)"], 0),
                    Text("Location", ""),
                    Check("Place part in target assembly", false),
                ],
            )],
            AuthorFrameProfile => &[Page(
                "Author Frame Profile",
                &[
                    Sel(
                        "section",
                        "Profile",
                        "select the sketch of the section (its largest closed region, holes allowed)",
                        PROFILE,
                    ),
                    Text("Standard", "Own"),
                    Text("Family", ""),
                    Text("Size", ""),
                    Note(
                        "The sketch origin does not matter: the profile's bounding box is centred on the member's skeleton line (Center anchor).",
                    ),
                    Note(
                        "Saved in the user's profile folder (one file per family); Insert Frame and Change list it.",
                    ),
                ],
            )],
            MakeComponents => &[
                Page(
                    "Bodies",
                    &[
                        Sel(
                            "bodies",
                            "Bodies",
                            "Select the bodies that become components",
                            BODIES,
                        ),
                        Text("Target Assembly", "Assembly1"),
                        Combo("Template", &["Standard (mm)"], 0),
                        Text("Location", ""),
                        Check("Insert components in target assembly", true),
                    ],
                ),
                Page(
                    "Components",
                    &[
                        Note("Per body: component name, template, file location, BOM structure"),
                        Check("Include parameters", false),
                    ],
                ),
            ],
        }
    }
}

/// A command with its panel only: a part command (spec 2.0) or an
/// assembly command (`gui/asm_planned.rs`, spec 3.0). Both share the panel,
/// the selector flow and the prompts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Planned {
    Part(PartPlanned),
    Asm(super::asm_planned::AsmPlanned),
}

impl From<PartPlanned> for Planned {
    fn from(k: PartPlanned) -> Self {
        Planned::Part(k)
    }
}

impl From<super::asm_planned::AsmPlanned> for Planned {
    fn from(k: super::asm_planned::AsmPlanned) -> Self {
        Planned::Asm(k)
    }
}

impl Planned {
    /// Whether OK runs something (the command's logic exists).
    pub fn has_logic(self) -> bool {
        matches!(
            self,
            Planned::Asm(
                super::asm_planned::AsmPlanned::PlaceFromLibrary
                    | super::asm_planned::AsmPlanned::ChangeSize
                    | super::asm_planned::AsmPlanned::Interference
                    | super::asm_planned::AsmPlanned::Create
                    | super::asm_planned::AsmPlanned::Pattern
                    | super::asm_planned::AsmPlanned::Replace
                    | super::asm_planned::AsmPlanned::Copy
                    | super::asm_planned::AsmPlanned::Mirror
                    | super::asm_planned::AsmPlanned::InsertFrame
                    | super::asm_planned::AsmPlanned::Miter
                    | super::asm_planned::AsmPlanned::RemoveEndTreatments
                    | super::asm_planned::AsmPlanned::FrameChange
                    | super::asm_planned::AsmPlanned::EndCap
                    | super::asm_planned::AsmPlanned::Reuse
                    | super::asm_planned::AsmPlanned::CornerJoint
                    | super::asm_planned::AsmPlanned::TrimExtend
                    | super::asm_planned::AsmPlanned::LengthenShorten
                    | super::asm_planned::AsmPlanned::Notch
                    | super::asm_planned::AsmPlanned::EditFrame
                    | super::asm_planned::AsmPlanned::FrameMemberInfo
                    | super::asm_planned::AsmPlanned::PlaceImported
                    | super::asm_planned::AsmPlanned::BoltedConnection
            ) | Planned::Part(
                PartPlanned::Emboss
                    | PartPlanned::Grill
                    | PartPlanned::Boss
                    | PartPlanned::Rest
                    | PartPlanned::SnapFit
                    | PartPlanned::RuleFillet
                    | PartPlanned::Lip
                    | PartPlanned::Stitch
                    | PartPlanned::Patch
                    | PartPlanned::TrimSurface
                    | PartPlanned::ExtendSurface
                    | PartPlanned::RuledSurface
                    | PartPlanned::ReplaceFace
                    | PartPlanned::Sculpt
                    | PartPlanned::CopyObject
                    | PartPlanned::Import
                    | PartPlanned::FreeformPlane
                    | PartPlanned::FreeformCylinder
                    | PartPlanned::Derive
                    | PartPlanned::MakePart
                    | PartPlanned::MakeComponents
                    | PartPlanned::AuthorFrameProfile
                    | PartPlanned::CreatePartFamily
                    | PartPlanned::Connection
            )
        )
    }

    /// Why option `label` (a check box, or choice `choice` of a radio row) is off in a command
    /// whose logic exists: `Some(tooltip)` draws it disabled.
    pub fn option_off(self, label: &str, choice: Option<&str>) -> Option<&'static str> {
        match (self, label, choice) {
            (Planned::Part(PartPlanned::Derive), "Derive Style", Some("Body as work surface"))
            | (
                Planned::Part(PartPlanned::Derive),
                "Sketches" | "Work Geometry" | "Show All Objects",
                None,
            ) => Some("Comes later (spec 3.10 §3.5 Element status)"),
            (Planned::Part(PartPlanned::MakePart), "Place part in target assembly", None)
            | (Planned::Part(PartPlanned::MakeComponents), "Include parameters", None) => {
                Some("Comes later (spec 3.10 §3.6)")
            }
            (
                Planned::Part(PartPlanned::FreeformCylinder),
                "Symmetry X" | "Symmetry Y" | "Symmetry Z" | "Direction",
                None,
            ) => Some(
                "Comes later: the cylinder stands on its plane, along its normal (spec 5.5 §3.1)",
            ),
            (
                Planned::Asm(super::asm_planned::AsmPlanned::Reuse),
                "Apply Appearance to Part",
                None,
            ) => Some("Comes later (spec 6.1 §3.5): the part is the source's"),
            (Planned::Asm(super::asm_planned::AsmPlanned::EndCap), "Appearance", None) => {
                Some("Comes later: the plate takes its material's appearance (spec 6.1 §3.2)")
            }
            (Planned::Part(PartPlanned::Grill), "Parting Element", None) => {
                Some("Comes later (spec 5.8 Grill: Draft)")
            }
            (Planned::Part(PartPlanned::Lip), "Path Extents", Some("Between Points")) => {
                Some("Comes later (spec 5.8 Lip: Path Extents)")
            }
            (Planned::Part(PartPlanned::Patch), "Condition", Some("Smooth (G2)")) => Some(
                "Comes later: Contact (G0) and Tangent (G1) are built (spec 5.4 Boundary Patch)",
            ),
            (Planned::Part(PartPlanned::CopyObject), "Create New", Some("Group")) => Some(
                "Needs the construction environment, not available yet (spec 2.13 Copy Object)",
            ),
            (Planned::Part(PartPlanned::CopyObject), "Select Existing" | "Associative", None) => {
                Some(concat!(
                    "Only when editing a part in an assembly (spec 2.13 Copy Object); ",
                    "in a part a copy always follows its source"
                ))
            }
            (Planned::Asm(_), "Import Type", Some("Reference Model")) => {
                Some("Comes with Update of a changed file (spec 7.7a 3.2, stage I3)")
            }
            (
                k,
                "Meshes" | "Graphical PMI" | "Wires" | "Work Features" | "Points" | "UCS",
                None,
            ) if k.is_import() => Some("The STEP reader skips these yet (spec 7.7a 3.4)"),
            (k, "Length Units", None) if k.is_import() => Some(concat!(
                "The geometry is converted from the file's units to mm ",
                "(document units: spec 7.7a 3.5)"
            )),
            (
                Planned::Part(PartPlanned::Import),
                "Import as",
                Some("Assembly" | "Composite Part"),
            ) => Some("In a part every body comes in as a body of it (spec 7.7a 3.6)"),
            (k, "Import as", Some("Composite Part")) if k.is_import() => {
                Some("Composite features are not available yet (spec 7.7a 3.6)")
            }
            (k, "Surfaces as", Some("Composite")) | (k, "Stitch", None) if k.is_import() => {
                Some("Each open shell is one surface body (spec 7.7a 3.7)")
            }
            (k, "Placement", Some("Pick a point")) if k.is_import() => {
                Some("Bodies come in at the file's origin (spec 7.7a 3.13)")
            }
            (
                Planned::Asm(super::asm_planned::AsmPlanned::InsertFrame),
                "Merge Frame Members" | "Apply Appearance to Part",
                None,
            ) => Some("Comes with stage F3 (spec 6.1 Insert Frame)"),
            (
                Planned::Asm(super::asm_planned::AsmPlanned::FrameChange),
                "Apply Appearance to Part",
                None,
            ) => Some("Comes with stage F3 (spec 6.1 Change)"),
            (
                Planned::Asm(super::asm_planned::AsmPlanned::Miter),
                "Miter Type",
                Some("Bisect Miter Cut"),
            ) => Some("Comes with stage F2 (spec 6.1 Miter: to confirm against the help)"),
            (
                Planned::Asm(super::asm_planned::AsmPlanned::TrimExtend),
                "Trim Selected Part",
                None,
            ) => {
                Some("Comes with stage F2 (spec 6.1 Trim/Extend: the kept side follows the member)")
            }
            (
                Planned::Asm(super::asm_planned::AsmPlanned::Notch),
                "Profile",
                Some(
                    "Custom I Template" | "Custom C Template" | "Custom T Template"
                    | "Custom Profile",
                ),
            ) => Some("Comes with stage F2b (spec 6.1 Notch): Basic Profile only yet"),
            (
                Planned::Asm(super::asm_planned::AsmPlanned::Notch),
                "Extend Notch Profile" | "Perpendicular Cut",
                None,
            ) => Some("Comes with stage F2b (spec 6.1 Notch)"),
            _ => None,
        }
    }

    /// Import (part) or Place Imported CAD Files (assembly).
    pub fn is_import(self) -> bool {
        matches!(
            self,
            Planned::Part(PartPlanned::Import)
                | Planned::Asm(super::asm_planned::AsmPlanned::PlaceImported)
        )
    }

    pub fn title(self) -> &'static str {
        match self {
            Planned::Part(k) => k.title(),
            Planned::Asm(k) => k.title(),
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Planned::Part(k) => k.icon(),
            Planned::Asm(k) => k.icon(),
        }
    }

    pub fn section(self) -> &'static str {
        match self {
            Planned::Part(k) => k.section(),
            Planned::Asm(k) => k.section(),
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Planned::Part(k) => k.about(),
            Planned::Asm(k) => k.about(),
        }
    }

    pub fn pages(self) -> &'static [Page] {
        match self {
            Planned::Part(k) => k.pages(),
            Planned::Asm(k) => k.pages(),
        }
    }

    /// The spec that lists the command.
    pub fn spec(self) -> &'static str {
        match self {
            Planned::Part(_) => "docs/spec/2.0-part-commands.md",
            Planned::Asm(_) => "docs/spec/3.0-assembly-commands.md",
        }
    }
}

/// The open panel of a planned command.
#[derive(Clone, Debug)]
pub(super) struct PartDialog {
    pub kind: Planned,
    tab: usize,
    /// Option values by (tab, row).
    bools: HashMap<(usize, usize), bool>,
    indices: HashMap<(usize, usize), usize>,
    nums: HashMap<(usize, usize), f64>,
    texts: HashMap<(usize, usize), String>,
    /// The active selector's key.
    pub active: Option<&'static str>,
    /// Picks per selector key (descriptions).
    pub picks: HashMap<&'static str, Vec<String>>,
    /// The assembly geometry of a selector's picks, in the order of
    /// `picks` (only selectors whose picks are faces / edges of components).
    pub refs: HashMap<&'static str, Vec<crate::assembly::GeomRef>>,
    /// The assembly pattern being edited (Edit of a Component Pattern).
    pub edit: Option<u32>,
    /// The part items of a selector's picks with the model point clicked, in
    /// the order of `picks` (the surface commands build their references
    /// from them).
    pub items: HashMap<&'static str, Vec<PickedItem>>,
    /// The feature being edited (Edit Feature of a surface command).
    pub feature: Option<usize>,
    /// A button of the panel pressed this frame (only those with logic).
    pub pressed: Option<&'static str>,
    /// A result line the command shows in its panel (free edges found…).
    pub note: Option<String>,
    /// Rule Fillet: the rules of the feature being edited (their tabs stay on show).
    pub rule_rows: usize,
    /// The dimension being edited in the panel's picture: tab, label, unit, where, frames to open.
    pub diagram_edit: Option<(usize, &'static str, &'static str, egui::Rect, u8)>,
    /// Import: the STEP text Browse... read (Edit Feature without it keeps the embedded one).
    pub file_text: Option<std::sync::Arc<str>>,
    /// Insert Frame: the skeleton lines picked (edges and sketch lines), in
    /// the order of `picks["edges"]`.
    pub skeletons: Vec<crate::assembly::frame::Skeleton>,
    /// Where each component pick was clicked (its label, assembly
    /// coordinates): Lengthen/Shorten takes the end nearer the click.
    pub clicks: Vec<(String, [f64; 3])>,
    /// The frame end treatment this panel edits (frame id, index): OK
    /// replaces it (`gui/frame_edit.rs`).
    pub frame_edit: Option<(u32, usize)>,
    /// Insert Frame's Between Points: the start point picked (the line it
    /// ends, which end, where), waiting for the end point.
    pub between_first: Option<(crate::assembly::frame::Skeleton, u8, crate::kernel::Point3)>,
    /// Insert Frame's sets kept by + Add Set: lines (with their labels) and
    /// the section, material and orientation they take.
    pub frame_sets: Vec<FrameSet>,
    /// Insert Frame as the edit of placed members: every member of their
    /// frame (the Members list), the ones edited, the one shown with its
    /// anchor points, each member's anchor (index into `Anchor::ALL`) and
    /// the edited members' skeleton lines when the panel opened.
    pub members: MemberEditState,
    /// Bolted Connection: the library and the fastener stack (`gui/bolted.rs`).
    pub bolt: super::bolted::BoltPanel,
}

/// One set of Insert Frame (+ Add Set): its lines and options.
#[derive(Clone, Debug)]
pub struct FrameSet {
    /// The skeleton lines.
    pub skeletons: Vec<crate::assembly::frame::Skeleton>,
    /// Section, material, orientation and Position of its members.
    pub options: crate::assembly::frame::InsertOptions,
}

/// Insert Frame's edit of placed members (`PartDialog::members`).
#[derive(Clone, Debug, Default)]
pub struct MemberEditState {
    /// Every member of the frame, in its order (empty: a plain Insert Frame).
    pub list: Vec<u32>,
    /// The members OK changes.
    pub edit: Vec<u32>,
    /// The member whose anchor points the view shows.
    pub current: Option<u32>,
    /// Each member's anchor (index into `Anchor::ALL`).
    pub anchor: std::collections::BTreeMap<u32, usize>,
    /// The edited members' skeleton lines when the panel opened.
    pub skeletons: Vec<crate::assembly::frame::Skeleton>,
    /// Zoom to this member next frame.
    pub zoom: Option<u32>,
}

/// A chain of edges the right-click menu of an open command adds ([`CadCraftApp::chain_edges`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Chain {
    /// Select Loop: the boundary loop through the edge.
    Loop,
    /// Select Tangent Chain: the edges going on smoothly from it.
    Tangent,
    /// Select Face Boundary: the outer loop of the face under the cursor.
    Boundary,
    /// Select From-To: the edges along the boundary from the last picked edge to the one under
    /// the cursor (Creo's From-To chain).
    FromTo,
}

/// Buttons that run something (the rest stay disabled).
const LIVE_BUTTONS: &[&str] = &[
    "Find Remaining Free Edges",
    "Browse...",
    "Refresh Categories",
    "+ Add Set",
    "Compute Flow Area",
];

/// Buttons that switch an option on and off (kept like a check box).
const TOGGLE_BUTTONS: &[&str] = &["Alternate All Faces", "Flip Direction", "Flip Members"];

/// The selector rows of `kind`, in order; a key on several tabs (Pattern
/// Component's Components) is one selector.
pub(super) fn selector_defs(
    kind: Planned,
) -> Vec<(
    &'static str,
    &'static str,
    &'static str,
    &'static [PickKind],
)> {
    kind.pages()
        .iter()
        .flat_map(|p| p.1)
        .filter_map(|r| match *r {
            Sel(k, l, p, a) => Some((k, l, p, a)),
            _ => None,
        })
        .fold(Vec::new(), |mut v, s| {
            if !v.iter().any(|t: &(&str, _, _, _)| t.0 == s.0) {
                v.push(s);
            }
            v
        })
}

/// The selector keys shown on tab `ti` of `kind`.
fn tab_keys(kind: Planned, ti: usize) -> Vec<&'static str> {
    kind.pages().get(ti).map_or_else(Vec::new, |p| {
        p.1.iter()
            .filter_map(|r| match *r {
                Sel(k, ..) => Some(k),
                _ => None,
            })
            .collect()
    })
}

/// A part item picked in a panel and the model point clicked on it.
pub(super) type PickedItem = (SelItem, Option<[f64; 3]>);

/// Selectors that take one input: a new pick replaces the old one.
// (Trim's Remove takes several areas, as Inventor's does.)
const SINGLE_KEYS: &[&str] = &["tool", "vector", "to", "new", "land"];

/// Selectors where one item picked in another place is a new pick (Trim's areas, a sketch's
/// regions, curves and points).
const POINT_KEYS: &[&str] = &[
    "remove", "boundary", "island", "ribs", "spars", "profile", "centers",
];

/// Selectors that keep taking component edges / faces: a pick never moves
/// on to the next selector.
const MANY_KEYS: &[&str] = &["edges"];

/// Selector colours by position (the dots of the rows).
const COLORS: [[u8; 3]; 5] = [
    colors::FACES,
    colors::PLANE,
    colors::DIRECTION,
    colors::EDGES,
    colors::POINTS,
];

impl PartDialog {
    pub(super) fn new(kind: impl Into<Planned>) -> Self {
        let kind = kind.into();
        Self {
            kind,
            tab: 0,
            bools: HashMap::new(),
            indices: HashMap::new(),
            nums: HashMap::new(),
            texts: HashMap::new(),
            active: selector_defs(kind).first().map(|s| s.0),
            picks: HashMap::new(),
            refs: HashMap::new(),
            edit: None,
            items: HashMap::new(),
            feature: None,
            pressed: None,
            note: None,
            rule_rows: 0,
            diagram_edit: None,
            file_text: None,
            skeletons: Vec::new(),
            clicks: Vec::new(),
            frame_edit: None,
            between_first: None,
            frame_sets: Vec::new(),
            members: MemberEditState::default(),
            bolt: Default::default(),
        }
    }

    /// The selectors as a flow (shared selector rows, Enter, the right-click
    /// menu and the pick filter read it). None is required: OK stays off
    /// anyway.
    pub(super) fn flow(&self) -> Flow {
        // Colours by place among the selectors on show: hidden ones would
        // leave two shown selectors one colour.
        let sels = selector_defs(self.kind)
            .into_iter()
            .filter(|s| !self.sel_hidden(s.0))
            .enumerate()
            .map(|(i, (key, label, prompt, accepts))| Selector {
                key,
                label,
                prompt,
                accepts: self.accepts(key, accepts),
                single: false,
                required: false,
                count: self.picks.get(key).map_or(0, Vec::len),
                color: COLORS[i % COLORS.len()],
            })
            .collect();
        Flow::new(sels, self.active)
    }

    /// What selector `key` takes now: Copy Object's Objects follow its Select (Face · Body),
    /// Insert Frame's edges its Select (Edge: edges and sketch lines only; Face / Body
    /// Edges: a face or an edge) — the picking and the highlight take nothing else (spec 0-ui rule 19).
    fn accepts(&self, key: &str, def: &'static [PickKind]) -> &'static [PickKind] {
        match (self.kind, key) {
            (
                Planned::Asm(
                    super::asm_planned::AsmPlanned::InsertFrame
                    | super::asm_planned::AsmPlanned::Reuse,
                ),
                "edges",
            ) => {
                if self.index(0, "Select") == 0 {
                    &[
                        PickKind::Edge,
                        PickKind::StraightEdge,
                        PickKind::SketchCurve,
                    ]
                } else {
                    // A face, or an edge (a click near one finds the edge
                    // first): Body Edges takes its body's edges, Face Edges
                    // the edge itself.
                    &[
                        PickKind::Face,
                        PickKind::PlanarFace,
                        PickKind::Edge,
                        PickKind::StraightEdge,
                    ]
                }
            }
            (Planned::Part(PartPlanned::CopyObject), "objects") => {
                if self.index(0, "Select") == 1 {
                    &[PickKind::Body, PickKind::Surface]
                } else {
                    FACES
                }
            }
            _ => def,
        }
    }

    /// The status-bar prompt: `<Command>: <prompt>`.
    pub(super) fn prompt(&self) -> String {
        let t = self.kind.title();
        match self.flow().current() {
            Some(s) => format!("{t}: {}", super::i18n::tr(s.prompt)),
            None if self.kind.is_import() => {
                format!("{t}: choose a STEP file (Browse...), set the options, then OK")
            }
            None if self.kind == Planned::Part(PartPlanned::Derive) => {
                format!("{t}: choose the source part (Browse...), set the options, then OK")
            }
            None if self.kind.has_logic() => format!("{t}: set the options, then OK"),
            None => format!("{t}: set the options (the command itself comes later)"),
        }
    }

    /// Takes a pick for the active selector (described as text): the same
    /// item again removes it; a new one moves on to the next empty
    /// selector, as Inventor's selectors do.
    pub(super) fn pick(&mut self, what: String) {
        self.pick_then(what, true);
    }

    /// [`Self::pick`]; `advance`: a new pick moves on to the next empty selector.
    fn pick_then(&mut self, what: String, advance: bool) {
        let Some(key) = self.active else {
            return;
        };
        let list = self.picks.entry(key).or_default();
        if let Some(i) = list.iter().position(|w| *w == what) {
            list.remove(i);
            if let Some(r) = self.refs.get_mut(key).filter(|r| i < r.len()) {
                r.remove(i);
            }
            if let Some(r) = self.items.get_mut(key).filter(|r| i < r.len()) {
                r.remove(i);
            }
            return;
        }
        list.push(what);
        if advance {
            self.advance(key);
        }
    }

    /// [`Self::pick`] of a part item and the model point clicked on it. A
    /// selector of one input (a cutting tool, a direction, the new face)
    /// replaces its pick.
    pub(super) fn pick_item(&mut self, what: String, item: SelItem, at: Option<[f64; 3]>) {
        let Some(key) = self.active else {
            return;
        };
        if SINGLE_KEYS.contains(&key) {
            self.picks.insert(key, vec![what]);
            self.items.insert(key, vec![(item, at)]);
            self.advance(key);
            return;
        }
        // The same pick again drops it. Picks are told apart by the item itself, not by their
        // label (labels may be just "Face" when internal names are hidden); Trim's Remove takes
        // areas, so there the same face picked elsewhere (another piece of it) is a new pick.
        let n = self.picks.get(key).map_or(0, Vec::len);
        let items = self.items.entry(key).or_default();
        items.truncate(n);
        let same = items.iter().position(|(x, p)| {
            *x == item
                && (!POINT_KEYS.contains(&key)
                    || match (p, at) {
                        (Some(a), Some(b)) => {
                            (crate::kernel::Point3::from(*a) - crate::kernel::Point3::from(b))
                                .norm()
                                < 1e-3
                        }
                        _ => true,
                    })
        });
        match same {
            Some(i) => {
                items.remove(i);
                if let Some(l) = self.picks.get_mut(key)
                    && i < l.len()
                {
                    l.remove(i);
                }
                if let Some(r) = self.refs.get_mut(key)
                    && i < r.len()
                {
                    r.remove(i);
                }
            }
            None => {
                items.push((item, at));
                self.picks.entry(key).or_default().push(what);
            }
        }
    }

    /// Adds part items to the active selector (a chain, a box): those picked already stay
    /// (none is dropped as a second click would); a selector of one input takes the first.
    pub(super) fn add_items(&mut self, list: Vec<(String, SelItem, Option<[f64; 3]>)>) {
        let Some(key) = self.active else {
            return;
        };
        if SINGLE_KEYS.contains(&key) {
            if let Some((what, item, at)) = list.into_iter().next() {
                self.pick_item(what, item, at);
            }
            return;
        }
        let n = self.picks.get(key).map_or(0, Vec::len);
        let items = self.items.entry(key).or_default();
        items.truncate(n);
        for (what, item, at) in list {
            if items.iter().any(|(x, _)| *x == item) {
                continue;
            }
            items.push((item, at));
            self.picks.entry(key).or_default().push(what);
        }
    }

    /// A selector the options chosen have no use for: it is not shown and takes no pick.
    pub(super) fn sel_hidden(&self, key: &str) -> bool {
        if super::bolted::sel_hidden(self, key) {
            return true;
        }
        let Planned::Part(k) = self.kind else {
            return false;
        };
        match (k, key) {
            (PartPlanned::Lip, "guide") => self.index(0, "Orientation") != 1,
            (PartPlanned::Lip, "pull") => self.index(0, "Orientation") != 2,
            (PartPlanned::Boss | PartPlanned::SnapFit, "direction") => {
                self.index(0, "Placement") != 1
            }
            (PartPlanned::Rest, "to") => self.index(0, "Platform") != 2,
            (PartPlanned::Rest, "land") => self.index(1, "Landing") != 1,
            (PartPlanned::RuleFillet, _) => RULE_KEYS.iter().enumerate().any(|(t, keys)| {
                (key == keys.1 && self.index(t, "Rule") != 3)
                    || (key == keys.2 && self.index(t, "Rule") != 4)
            }),
            _ => false,
        }
    }

    /// Selector `key` takes `g` alone (any earlier pick goes).
    pub(super) fn set_ref(&mut self, key: &'static str, what: String, g: crate::assembly::GeomRef) {
        self.refs.insert(key, vec![g]);
        self.picks.insert(key, vec![what]);
        self.items.remove(key);
    }

    /// Selector `key` takes `g` as one more pick (the same again removes it).
    pub(super) fn add_ref(&mut self, key: &'static str, what: String, g: crate::assembly::GeomRef) {
        let refs = self.refs.entry(key).or_default();
        let list = self.picks.entry(key).or_default();
        if let Some(i) = refs.iter().position(|r| *r == g) {
            refs.remove(i);
            if i < list.len() {
                list.remove(i);
            }
        } else {
            refs.push(g);
            list.push(what);
        }
    }

    /// Selector `key` holds nothing.
    pub(super) fn clear_ref(&mut self, key: &str) {
        self.refs.remove(key);
        self.picks.remove(key);
        self.items.remove(key);
    }

    /// The first empty selector on show becomes the active one (none empty:
    /// the active one stays).
    pub(super) fn activate_first_empty(&mut self) {
        if let Some(k) = self
            .keys_on_show(self.tab)
            .into_iter()
            .find(|k| self.picks.get(k).is_none_or(Vec::is_empty))
        {
            self.active = Some(k);
        }
    }

    /// The selectors of tab `ti` on show.
    fn keys_on_show(&self, ti: usize) -> Vec<&'static str> {
        let mut keys = tab_keys(self.kind, ti);
        keys.retain(|k| !self.sel_hidden(k));
        keys
    }

    /// The colour of selector `key`: its dot in the panel and its picks in the view.
    pub(super) fn sel_color(&self, key: &str) -> egui::Color32 {
        let c = selector_defs(self.kind)
            .iter()
            .filter(|s| !self.sel_hidden(s.0))
            .position(|s| s.0 == key)
            .map_or(COLORS[0], |i| COLORS[i % COLORS.len()]);
        egui::Color32::from_rgb(c[0], c[1], c[2])
    }

    /// The value of number row `label` of tab `ti`, to edit in place.
    pub(super) fn num_mut(&mut self, ti: usize, label: &str) -> Option<&mut f64> {
        let (ri, def) = match self.find_row(ti, label)? {
            (ri, Num(_, def, _)) => (ri, *def),
            _ => return None,
        };
        Some(self.nums.entry((ti, ri)).or_insert(def))
    }

    /// Rule Fillet's rule tabs appear one by one: the next is there once the last has a source.
    fn tab_hidden(&self, ti: usize) -> bool {
        if self.kind != Planned::Part(PartPlanned::RuleFillet) || ti == 0 || ti >= RULE_TABS {
            return false;
        }
        let used = |t: usize| t < self.rule_rows || !self.picks_of(RULE_KEYS[t].0).is_empty();
        !(used(ti) || used(ti - 1))
    }

    /// The items picked for selector `key`.
    pub(super) fn items_of(&self, key: &str) -> &[PickedItem] {
        self.items.get(key).map_or(&[], Vec::as_slice)
    }

    /// The choice of radio / drop-down row `label` of tab `ti` (its default
    /// until changed).
    pub(super) fn index(&self, ti: usize, label: &str) -> usize {
        let Some(page) = self.kind.pages().get(ti) else {
            return 0;
        };
        page.1
            .iter()
            .enumerate()
            .find_map(|(ri, r)| match *r {
                Radio(l, _, def) | Combo(l, _, def) if l == label => {
                    Some(self.indices.get(&(ti, ri)).copied().unwrap_or(def))
                }
                _ => None,
            })
            .unwrap_or(0)
    }

    /// Takes a component's face / edge for the active selector: `single`
    /// selectors (a direction, an axis) replace their pick, others add it
    /// (the same geometry again removes it).
    pub(super) fn pick_ref(&mut self, what: String, g: crate::assembly::GeomRef, single: bool) {
        let Some(key) = self.active else {
            return;
        };
        let refs = self.refs.entry(key).or_default();
        let list = self.picks.entry(key).or_default();
        if single {
            *refs = vec![g];
            *list = vec![what];
        } else if let Some(i) = refs.iter().position(|r| *r == g) {
            refs.remove(i);
            if i < list.len() {
                list.remove(i);
            }
            return;
        } else {
            refs.push(g);
            list.push(what);
        }
        // Insert Frame keeps taking edges until Enter (Inventor).
        if !MANY_KEYS.contains(&key) {
            self.advance(key);
        }
    }

    /// Insert Frame's edges selector takes skeleton line `s` (the same one
    /// again removes it).
    pub(super) fn pick_skeleton(&mut self, what: String, s: crate::assembly::frame::Skeleton) {
        let list = self.picks.entry("edges").or_default();
        if let Some(i) = self.skeletons.iter().position(|x| *x == s) {
            self.skeletons.remove(i);
            if i < list.len() {
                list.remove(i);
            }
        } else {
            self.skeletons.push(s);
            list.push(what);
        }
    }

    /// After a pick in selector `key`: on to the next empty selector.
    fn advance(&mut self, key: &'static str) {
        // The next selector of the tab on show (another tab's is not seen).
        let keys = self.keys_on_show(self.tab);
        if let Some(i) = keys.iter().position(|k| *k == key)
            && let Some(next) = keys.get(i + 1)
            && self.picks.get(next).is_none_or(Vec::is_empty)
        {
            self.active = Some(next);
        }
    }

    /// Makes the selector `key` active (and its tab the shown one).
    pub(super) fn set_active(&mut self, key: &str) {
        for (ti, p) in self.kind.pages().iter().enumerate() {
            for r in p.1 {
                if let Sel(k, ..) = *r
                    && k == key
                {
                    self.active = Some(k);
                    self.tab = ti;
                }
            }
        }
    }

    /// The picks of selector `key` (their descriptions).
    pub(super) fn picks_of(&self, key: &str) -> &[String] {
        self.picks.get(key).map_or(&[], Vec::as_slice)
    }

    /// The shown tab.
    pub(super) fn tab(&self) -> usize {
        self.tab
    }

    /// Shows tab `ti`.
    pub(super) fn set_tab(&mut self, ti: usize) {
        self.tab = ti;
    }

    /// Where the row `label` of tab `ti` is, and the row.
    fn find_row(&self, ti: usize, label: &str) -> Option<(usize, &'static Row)> {
        self.kind
            .pages()
            .get(ti)?
            .1
            .iter()
            .enumerate()
            .find(|(_, r)| match **r {
                Num(l, ..) | Check(l, _) | Text(l, _) => l == label,
                Button(l) => TOGGLE_BUTTONS.contains(&l) && l == label,
                _ => false,
            })
    }

    /// The text in row `label` of tab `ti` (its default until changed).
    pub(super) fn text(&self, ti: usize, label: &str) -> String {
        match self.find_row(ti, label) {
            Some((ri, Text(_, def))) => self
                .texts
                .get(&(ti, ri))
                .cloned()
                .unwrap_or_else(|| def.to_string()),
            _ => String::new(),
        }
    }

    /// Sets the text in row `label` of tab `ti`.
    pub(super) fn set_text(&mut self, ti: usize, label: &str, v: &str) {
        if let Some((ri, _)) = self.find_row(ti, label) {
            self.texts.insert((ti, ri), v.to_owned());
        }
    }

    /// The number in row `label` of tab `ti` (its default until changed).
    pub(super) fn num(&self, ti: usize, label: &str) -> Option<f64> {
        match self.find_row(ti, label)? {
            (ri, Num(_, def, _)) => Some(self.nums.get(&(ti, ri)).copied().unwrap_or(*def)),
            _ => None,
        }
    }

    /// The check box `label` of tab `ti`.
    pub(super) fn check(&self, ti: usize, label: &str) -> bool {
        match self.find_row(ti, label) {
            Some((ri, Check(_, def))) => self.bools.get(&(ti, ri)).copied().unwrap_or(*def),
            Some((ri, Button(_))) => self.bools.get(&(ti, ri)).copied().unwrap_or(false),
            _ => false,
        }
    }

    /// Sets the number in row `label` of tab `ti`.
    pub(super) fn set_num(&mut self, ti: usize, label: &str, v: f64) {
        if let Some((ri, _)) = self.find_row(ti, label) {
            self.nums.insert((ti, ri), v);
        }
    }

    /// Sets the choice of radio / drop-down row `label` of tab `ti`.
    pub(super) fn set_index(&mut self, ti: usize, label: &str, v: usize) {
        let Some(page) = self.kind.pages().get(ti) else {
            return;
        };
        if let Some(ri) = page
            .1
            .iter()
            .position(|r| matches!(*r, Radio(l, ..) | Combo(l, ..) if l == label))
        {
            self.indices.insert((ti, ri), v);
        }
    }

    /// Sets the check box `label` of tab `ti`.
    pub(super) fn set_check(&mut self, ti: usize, label: &str, v: bool) {
        if let Some((ri, _)) = self.find_row(ti, label) {
            self.bools.insert((ti, ri), v);
        }
    }

    /// Draws the panel body: whether Cancel or OK was pressed.
    fn ui(&mut self, ui: &mut egui::Ui) -> (bool, bool) {
        let pages = self.kind.pages();
        if pages.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                for (i, p) in pages.iter().enumerate() {
                    if !self.tab_hidden(i) {
                        ui.selectable_value(&mut self.tab, i, p.0);
                    }
                }
            });
            // A tab's selectors only take picks while it is shown.
            let keys = self.keys_on_show(self.tab);
            if self.active.is_none_or(|a| !keys.contains(&a)) {
                self.active = keys
                    .iter()
                    .find(|k| self.picks.get(*k).is_none_or(Vec::is_empty))
                    .or(keys.first())
                    .copied();
            }
            ui.separator();
        }
        super::plastic_diagram::show(self, ui);
        let ti = self.tab.min(pages.len().saturating_sub(1));
        let Some(page) = pages.get(ti) else {
            return (false, false);
        };
        let flow = self.flow();
        let mut ev = RowEvents::default();
        egui::Grid::new(("part_planned", self.kind.title(), ti))
            .num_columns(2)
            .min_col_width(super::panels::label_width(ui.ctx()))
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                for (ri, row) in page.1.iter().enumerate() {
                    self.row(ui, (ti, ri), row, &flow, &mut ev);
                }
            });
        if let Some(k) = ev.activate {
            self.active = Some(k);
        }
        if let Some(k) = ev.clear {
            self.picks.remove(k);
            self.refs.remove(k);
            if k == "edges" {
                self.skeletons.clear();
            }
            self.items.remove(k);
            self.active = Some(k);
        }
        ui.separator();
        ui.label(RichText::new(self.kind.about()).weak());
        let tip = format!(
            "Interface only: the logic comes next ({}; FEATURES {})",
            self.kind.spec(),
            self.kind.section()
        );
        let (mut cancel, mut ok) = (false, false);
        let logic = self.kind.has_logic();
        super::theme::panel_footer(ui, |ui| {
            let b = ui.add_enabled(logic, super::theme::ok_button("OK"));
            ok = b.clicked();
            b.on_disabled_hover_text(&tip);
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
            if !logic {
                ui.add_enabled(false, egui::Button::new("Apply"))
                    .on_disabled_hover_text(&tip);
            }
        });
        (cancel, ok)
    }

    fn row(
        &mut self,
        ui: &mut egui::Ui,
        at: (usize, usize),
        row: &Row,
        flow: &Flow,
        ev: &mut RowEvents,
    ) {
        match *row {
            Head(t) => {
                ui.label(RichText::new(t).strong());
                ui.end_row();
            }
            Note(t) => {
                plabel(ui, "");
                ui.label(RichText::new(t).weak().italics());
                ui.end_row();
            }
            Button(t) if TOGGLE_BUTTONS.contains(&t) => {
                plabel(ui, "");
                let on = self.bools.get(&at).copied().unwrap_or(false);
                if ui
                    .add_enabled(self.kind.has_logic(), egui::Button::new(t).selected(on))
                    .on_disabled_hover_text("Comes with the command's logic")
                    .clicked()
                {
                    self.bools.insert(at, !on);
                }
                ui.end_row();
            }
            Button("Click to add a fastener") if super::bolted::is_bolted(self) => {
                super::bolted::stack_ui(self, ui);
            }
            Button(t) => {
                plabel(ui, "");
                let live = self.kind.has_logic() && LIVE_BUTTONS.contains(&t);
                if ui
                    .add_enabled(live, egui::Button::new(t))
                    .on_disabled_hover_text("Comes with the command's logic")
                    .clicked()
                {
                    self.pressed = Some(t);
                }
                ui.end_row();
                if live && let Some(n) = &self.note {
                    plabel(ui, "");
                    ui.label(RichText::new(n.as_str()).small());
                    ui.end_row();
                }
            }
            Sel(key, ..) => {
                if self.sel_hidden(key) {
                    return;
                }
                let Some(i) = flow.selectors.iter().position(|s| s.key == key) else {
                    return;
                };
                let s = &flow.selectors[i];
                // One pick: its name (the description up to " · ").
                let text = match self.picks.get(key).map(Vec::as_slice) {
                    Some([one]) => one.split(" · ").next().unwrap_or(one).to_owned(),
                    _ => String::new(),
                };
                selector_row(ui, s, flow.active == Some(i), &text, ev);
                ui.end_row();
            }
            Check(label, _) | Num(label, ..) if super::bolted::row_hidden(self, at.0, label) => {}
            Check(label, on) => {
                let off = self
                    .kind
                    .option_off(label, None)
                    .or_else(|| super::bolted::row_off(self, at.0, label));
                let v = self.bools.entry(at).or_insert(on);
                plabel(ui, "");
                let r = ui.add_enabled(off.is_none(), egui::Checkbox::new(v, label));
                if let Some(t) = off {
                    r.on_disabled_hover_text(t);
                }
                ui.end_row();
            }
            Radio(label, opts, def) => {
                plabel(ui, label);
                let kind = self.kind;
                let v = self.indices.entry(at).or_insert(def);
                let before = *v;
                // Two short choices side by side; more one under another, so
                // the panel keeps its width.
                let draw = |ui: &mut egui::Ui, v: &mut usize| {
                    for (i, o) in opts.iter().enumerate() {
                        let off = kind.option_off(label, Some(o));
                        let r = ui.add_enabled(off.is_none(), egui::RadioButton::new(*v == i, *o));
                        if r.clicked() {
                            *v = i;
                        }
                        if let Some(t) = off {
                            r.on_disabled_hover_text(t);
                        }
                    }
                };
                if opts.len() <= 2 && opts.iter().map(|o| o.len()).sum::<usize>() <= 24 {
                    ui.horizontal(|ui| draw(ui, v));
                } else {
                    ui.vertical(|ui| draw(ui, v));
                }
                // Copy Object: switching Face / Body clears the selection (Inventor).
                if *v != before
                    && label == "Select"
                    && kind == Planned::Part(PartPlanned::CopyObject)
                {
                    ev.clear = Some("objects");
                }
                ui.end_row();
            }
            Combo(label, opts, def) => {
                plabel(ui, label);
                let off = self
                    .kind
                    .option_off(label, None)
                    .or_else(|| super::bolted::row_off(self, at.0, label));
                // Frame panels: the section table's lists (gui/frame.rs); Bolted
                // Connection: the thread and library lists (gui/bolted.rs).
                let list: Vec<String> = super::frame::frame_combo(self, at.0, label)
                    .or_else(|| super::bolted::combo(self, at.0, label))
                    .unwrap_or_else(|| opts.iter().map(|o| (*o).to_owned()).collect());
                let v = self.indices.entry(at).or_insert(def);
                *v = (*v).min(list.len().saturating_sub(1));
                let r = ui.add_enabled_ui(off.is_none(), |ui| {
                    egui::ComboBox::from_id_salt(("part_planned_combo", self.kind.title(), at))
                        .selected_text(list.get(*v).map_or("", String::as_str))
                        .show_ui(ui, |ui| {
                            for (i, o) in list.iter().enumerate() {
                                ui.selectable_value(v, i, o.as_str());
                            }
                        });
                });
                if let Some(t) = off {
                    r.response.on_disabled_hover_text(t);
                }
                ui.end_row();
            }
            Num(label, def, unit) => {
                plabel(ui, label);
                // Emboss: From Plane has a taper and no depth, the face types the other way round.
                let plane = self.index(at.0, "Type") == 2;
                let off = (self.kind == Planned::Part(PartPlanned::Emboss)
                    && ((label == "Depth" && plane) || (label == "Taper" && !plane)))
                    || matches!(self.kind, Planned::Part(k)
                        if super::plastic::row_off(self, k, at.0, label));
                let why = super::bolted::row_off(self, at.0, label);
                let v = self.nums.entry(at).or_insert(def);
                let on = !off && why.is_none();
                let r = ui.add_enabled_ui(on, |ui| {
                    // A disabled field is no input of the value next to the cursor.
                    let field = ExprValue::new(v).suffix(unit);
                    ui.add(if on { field.cursor(label) } else { field });
                });
                if let Some(t) = why {
                    r.response.on_disabled_hover_text(t);
                }
                ui.end_row();
            }
            Text(label, def) => {
                plabel(ui, label);
                let why = super::bolted::row_off(self, at.0, label);
                let v = self.texts.entry(at).or_insert_with(|| def.to_string());
                let r = ui.add_enabled(
                    why.is_none(),
                    egui::TextEdit::singleline(v).desired_width(180.0),
                );
                if let Some(t) = why {
                    r.on_disabled_hover_text(t);
                }
                ui.end_row();
            }
        }
    }
}

/// Whether `item` is something `kinds` takes (the pick filter already
/// limits the categories; this keeps a feature row from filling a face
/// selector).
pub(super) fn takes(kinds: &[PickKind], item: &SelItem) -> bool {
    use PickKind as K;
    let any = |ks: &[K]| ks.iter().any(|k| kinds.contains(k));
    match item {
        SelItem::Face(_) => any(&[K::Face, K::PlanarFace, K::Surface]),
        SelItem::Edge(_) => any(&[K::Edge, K::StraightEdge]),
        SelItem::Vertex(_) | SelItem::Point(..) => any(&[K::Point]),
        SelItem::Body(_) => any(&[K::Body, K::Surface]),
        SelItem::Sketch(_) => any(&[K::Sketch, K::Region, K::SketchCurve, K::SketchPoint]),
        SelItem::Feature(_) => any(&[K::Plane, K::Axis, K::Point, K::Face, K::Body]),
    }
}

impl CadCraftApp {
    /// Opens the panel of planned command `kind`. What is selected and
    /// fits the first selector fills it (rule 17).
    pub(super) fn start_part_planned(&mut self, kind: PartPlanned) {
        if kind == PartPlanned::CreatePartFamily {
            self.finish_sketch();
            self.part_family_start();
            return;
        }
        let kind = Planned::Part(kind);
        let pre = self.sel.clone();
        self.finish_sketch();
        self.mode = super::Mode::Normal;
        self.cancel_extrude();
        self.work_draft = None;
        self.blend_draft = None;
        self.face_sel = None;
        self.edge_sel = None;
        let mut d = PartDialog::new(kind);
        if let Some(first) = selector_defs(kind).first() {
            for item in pre {
                if takes(first.3, &item) {
                    d.picks
                        .entry(first.0)
                        .or_default()
                        .push(self.item_label(&item));
                    let at = self.item_point(&item).map(|p| [p.x, p.y, p.z]);
                    d.items.entry(first.0).or_default().push((item, at));
                }
            }
            if d.picks.contains_key(first.0)
                && let Some(next) = selector_defs(kind).get(1)
            {
                d.active = Some(next.0);
            }
        }
        self.sel.clear();
        self.part_planned = Some(d);
    }

    /// A click in the view while a planned panel is open: the item under
    /// the cursor (through the pick filter of the active selector).
    pub(super) fn part_planned_click(&mut self, p: egui::Pos2, rect: egui::Rect) {
        let Some(item) = self.part_planned_item_at(p, rect) else {
            return;
        };
        let what = self.item_label(&item);
        let at = self.click_point(&item, p, rect).map(|q| [q.x, q.y, q.z]);
        let mut chain = self.patch_chain(&item);
        chain.extend(self.lip_loop(&item));
        let Some(d) = self.part_planned.as_mut() else {
            return;
        };
        let count = |d: &PartDialog| d.active.and_then(|k| d.picks.get(k)).map_or(0, Vec::len);
        let before = count(d);
        d.pick_item(what, item, at);
        // The same click again takes the edge off: then nothing joins it.
        if count(d) > before && !chain.is_empty() {
            self.part_planned_add(chain);
        }
    }

    /// Lip with Whole Loop: the loop of the rim the edge belongs to. Of the edge's planar faces the
    /// rim is the one with a hole (the open top of a housing), else the one facing most upwards,
    /// as the feature itself decides without a guide or a pull.
    pub(super) fn lip_loop(&self, item: &SelItem) -> Vec<SelItem> {
        let SelItem::Edge(e) = item else {
            return Vec::new();
        };
        let on = self.part_planned.as_ref().is_some_and(|d| {
            d.kind == Planned::Part(PartPlanned::Lip)
                && d.active == Some("path")
                && d.check(0, WHOLE_LOOP)
        });
        if !on {
            return Vec::new();
        }
        let body = &self.part().body;
        let Some(near) = self.item_point(item) else {
            return Vec::new();
        };
        let rim = self
            .picker()
            .faces_of(*e)
            .iter()
            .filter_map(|&f| {
                let face = body.face(f)?;
                match body.surface(face.surface)? {
                    crate::kernel::Surface::Plane(p) => {
                        Some((f, !face.inner.is_empty(), p.normal.z.abs()))
                    }
                    _ => None,
                }
            })
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| a.2.total_cmp(&b.2)))
            .map(|x| x.0);
        rim.map(|f| psel::loop_edges(body, f, near))
            .unwrap_or_default()
            .into_iter()
            .filter(|x| x != e)
            .map(SelItem::Edge)
            .collect()
    }

    /// Boundary Patch with Automatic Edge Chain: the free edges joined to the picked one, so the
    /// panel's count and the highlight show the whole loop at once.
    fn patch_chain(&self, item: &SelItem) -> Vec<SelItem> {
        let SelItem::Edge(e) = item else {
            return Vec::new();
        };
        let on = self.part_planned.as_ref().is_some_and(|d| {
            d.kind == Planned::Part(PartPlanned::Patch)
                && d.active == Some("boundary")
                && d.check(0, "Automatic Edge Chain")
        });
        if !on {
            return Vec::new();
        }
        crate::part::select::free_chain(&self.part().body, *e, false)
            .into_iter()
            .map(SelItem::Edge)
            .collect()
    }

    /// Adds `items` to the planned panel's active selector (those it takes), each with a point
    /// on it (a chain from the right-click menu, a box).
    pub(super) fn part_planned_add(&mut self, items: Vec<SelItem>) {
        let Some(kinds) = self
            .part_planned
            .as_ref()
            .and_then(|d| d.flow().current().map(|s| s.accepts))
        else {
            return;
        };
        let list: Vec<_> = items
            .into_iter()
            .filter(|i| takes(kinds, i))
            .map(|i| {
                let what = self.item_label(&i);
                let at = self.item_point(&i).map(|q| [q.x, q.y, q.z]);
                (what, i, at)
            })
            .collect();
        if let Some(d) = self.part_planned.as_mut() {
            d.add_items(list);
        }
    }

    /// Whether a drag from empty space is a box for the planned panel: a command with logic
    /// whose active selector takes faces or edges.
    pub(super) fn part_planned_box_wanted(&self) -> bool {
        self.part_planned.as_ref().is_some_and(|d| {
            d.kind.has_logic()
                && d.flow().current().is_some_and(|s| {
                    s.accepts
                        .iter()
                        .any(|k| matches!(k, PickKind::Face | PickKind::Edge))
                })
        })
    }

    /// The edges a chain entry of the right-click menu adds, from the edge under the cursor
    /// when the menu opened (`context_edge`) or the face there: Loop — a free edge's whole
    /// boundary (across the faces it runs along), else the loop of the face there; Tangent
    /// Chain — the edges going on smoothly; Face Boundary — the face's outer loop.
    /// The last edge picked for the planned panel's active selector (where a From-To chain
    /// starts).
    pub(super) fn last_planned_edge(&self) -> Option<crate::kernel::EdgeId> {
        let d = self.part_planned.as_ref()?;
        let key = d.flow().current()?.key;
        d.items_of(key).iter().rev().find_map(|(i, _)| match i {
            SelItem::Edge(e) => Some(*e),
            _ => None,
        })
    }

    pub(super) fn chain_edges(&self, chain: Chain) -> Vec<SelItem> {
        let body = &self.part().body;
        let face = self.context_face.as_deref().and_then(|n| body.find_face(n));
        let edges = match (chain, self.context_edge) {
            (Chain::Loop, Some(e)) if psel::is_free_edge(body, e) => {
                psel::free_chain(body, e, false)
            }
            (Chain::Tangent, Some(e)) if psel::is_free_edge(body, e) => {
                psel::free_chain(body, e, true)
            }
            (Chain::Tangent, Some(e)) => psel::tangent_edges(body, e),
            (Chain::Loop, Some(e)) => {
                let faces = self.picker().faces_of(e);
                let f = face
                    .filter(|f| faces.contains(f))
                    .or_else(|| faces.first().copied());
                match (f, self.item_point(&SelItem::Edge(e))) {
                    (Some(f), Some(near)) => psel::loop_edges(body, f, near),
                    _ => Vec::new(),
                }
            }
            (Chain::Boundary, _) => face
                .map(|f| psel::outer_loop_edges(body, f))
                .unwrap_or_default(),
            (Chain::FromTo, Some(e)) => match self.last_planned_edge() {
                Some(from) => psel::from_to_chain(body, from, e),
                None => Vec::new(),
            },
            _ => Vec::new(),
        };
        edges.into_iter().map(SelItem::Edge).collect()
    }

    /// What a click at `p` would give the active selector of the planned panel (also the
    /// prehighlight under the cursor).
    pub(super) fn part_planned_item_at(&self, p: egui::Pos2, rect: egui::Rect) -> Option<SelItem> {
        let d = self.part_planned.as_ref()?;
        let kinds = d.flow().current().map(|s| s.accepts)?;
        let plastic_sketch = matches!(
            d.kind,
            Planned::Part(
                PartPlanned::Grill | PartPlanned::Boss | PartPlanned::Rest | PartPlanned::SnapFit
            )
        ) && kinds.iter().any(|k| {
            matches!(
                k,
                PickKind::Region | PickKind::SketchCurve | PickKind::SketchPoint
            )
        });
        if plastic_sketch && let Some(i) = self.plastic_sketch_at(p, rect, kinds) {
            return Some(SelItem::Sketch(i));
        }
        let item = if kinds.contains(&PickKind::Body) && !kinds.contains(&PickKind::Face) {
            // A selector of whole bodies (Copy Object's Body mode): the body of the face under
            // the cursor (the pick filter leaves faces out, so they are not asked through it).
            let (o, dir) = self.camera.ray(p, rect)?;
            let dir = dir.try_normalize(1e-12)?;
            let (f, t) = self.picker().pick(&o, &dir)?;
            let name = self.part().body.face(f).and_then(|x| x.name.clone())?;
            let at = o + dir * t;
            SelItem::Body(self.body_of_face(&name, Some([at.x, at.y, at.z]))?)
        } else {
            let edge = self.edge_under(p, rect);
            let face = match (edge, self.plane_under(p, rect, false)) {
                (None, Some(crate::part::PlaneRef::Face(n))) => Some(n),
                _ => None,
            };
            self.sel_item_at(p, rect, edge, face.as_deref())?
        };
        takes(kinds, &item).then_some(item)
    }

    /// A body row of the browser for the planned panel's active selector (when it takes
    /// bodies).
    pub(super) fn part_planned_pick_body(&mut self, name: String) {
        let Some(d) = &self.part_planned else {
            return;
        };
        let Some(kinds) = d.flow().current().map(|s| s.accepts) else {
            return;
        };
        let item = SelItem::Body(name);
        if !takes(kinds, &item) {
            return;
        }
        let what = self.item_label(&item);
        if let Some(d) = self.part_planned.as_mut() {
            d.pick_item(what, item, None);
        }
    }

    /// Every item picked in the planned panel's selectors (drawn as selected in the view).
    pub(super) fn part_planned_items(&self) -> Vec<SelItem> {
        let Some(d) = &self.part_planned else {
            return Vec::new();
        };
        let mut out: Vec<SelItem> = Vec::new();
        for (item, _) in d.items.values().flatten() {
            if !out.contains(item) {
                out.push(item.clone());
            }
        }
        out
    }

    /// The planned command's panel (Properties).
    pub(super) fn part_planned_panel(&mut self, ui: &mut egui::Ui) {
        let Some(mut d) = self.part_planned.clone() else {
            return;
        };
        let title = if d.members.list.is_empty() {
            d.kind.title()
        } else {
            "Edit Frame Members"
        };
        self.panel_header(ui, d.kind.icon(), title);
        if !d.members.list.is_empty() {
            self.member_list_ui(ui, &mut d);
        }
        if !d.frame_sets.is_empty() {
            super::frame::sets_ui(ui, &mut d);
        }
        // Insert Frame's active input follows Placement (Enter / a failed OK
        // moved it on to the points, which took no lines any more).
        if d.kind == Planned::Asm(super::asm_planned::AsmPlanned::InsertFrame)
            && d.members.list.is_empty()
            && d.active.is_some()
        {
            d.active = Some(if d.index(0, "Placement") == 1 {
                "points"
            } else {
                "edges"
            });
        }
        let (cancel, ok) = d.ui(ui);
        // A plastic command says why its definition cannot be built, before OK.
        if super::plastic::is_plastic(&d) {
            match self.plastic_preview() {
                Err(e) if e == super::PREVIEW_PENDING => {
                    ui.label(e);
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(50));
                }
                Err(e) if !e.is_empty() => {
                    ui.label(RichText::new(e).color(egui::Color32::from_rgb(230, 120, 20)));
                }
                _ => {}
            }
        }
        // The Anchor list of Edit Frame Members moves the current member
        // at once (like a click on one of its points).
        let mut moved = None;
        if let Some(c) = d.members.current {
            let k = d.index(0, "Anchor");
            if d.members.anchor.insert(c, k) != Some(k) {
                moved = Some((c, k));
            }
        }
        if let Some(b) = d.pressed.take() {
            if d.kind == Planned::Asm(super::asm_planned::AsmPlanned::Replace) {
                self.replace_browse();
            } else if d.kind == Planned::Part(PartPlanned::Derive) {
                self.derive_browse();
            } else if d.kind.is_import() {
                self.open_dialog(super::files::DialogPurpose::ImportCad);
            } else if b == "Refresh Categories" {
                d.note = Some(super::frame::refresh_categories());
            } else if b == "+ Add Set" {
                d.note = Some(super::frame::add_set(&mut d));
            } else {
                d.note = self.surface_button(&d, b);
            }
        }
        self.part_planned = (!cancel).then_some(d);
        if let Some((c, k)) = moved {
            self.apply_member_anchor(c, k);
        }
        if ok {
            self.planned_ok();
        }
    }

    /// OK of a planned command that has its logic.
    pub(super) fn planned_ok(&mut self) {
        let Some(d) = self.part_planned.clone() else {
            return;
        };
        match d.kind {
            Planned::Part(PartPlanned::Connection) => {
                if self.connection_ok(&d) {
                    self.part_planned = None;
                }
            }
            Planned::Asm(super::asm_planned::AsmPlanned::Interference) => {
                if self.start_interference(&d) {
                    self.part_planned = None;
                }
            }
            Planned::Asm(super::asm_planned::AsmPlanned::Create) => {
                if self.create_ok(&d) {
                    self.part_planned = None;
                }
            }
            Planned::Asm(super::asm_planned::AsmPlanned::Pattern) => {
                if self.pattern_ok(&d) {
                    self.part_planned = None;
                }
            }
            Planned::Asm(super::asm_planned::AsmPlanned::Replace) => {
                if self.replace_ok(&d) {
                    self.part_planned = None;
                }
            }
            Planned::Asm(super::asm_planned::AsmPlanned::Copy) => {
                if self.copy_ok(&d) {
                    self.part_planned = None;
                }
            }
            Planned::Asm(super::asm_planned::AsmPlanned::Mirror) => {
                if self.mirror_ok(&d) {
                    self.part_planned = None;
                }
            }
            Planned::Asm(super::asm_planned::AsmPlanned::InsertFrame) => {
                if self.frame_ok(&d) {
                    self.part_planned = None;
                }
            }
            Planned::Asm(super::asm_planned::AsmPlanned::Reuse) => {
                if self.reuse_ok(&d) {
                    self.part_planned = None;
                }
            }
            Planned::Asm(super::asm_planned::AsmPlanned::EndCap) => {
                if self.end_cap_ok(&d) {
                    self.part_planned = None;
                }
            }
            Planned::Asm(super::asm_planned::AsmPlanned::Miter) => {
                if self.miter_ok(&d) {
                    self.part_planned = None;
                }
            }
            Planned::Part(PartPlanned::AuthorFrameProfile) => {
                if self.author_profile_ok(&d) {
                    self.part_planned = None;
                }
            }
            Planned::Asm(super::asm_planned::AsmPlanned::FrameChange) => {
                if self.change_ok(&d) {
                    self.part_planned = None;
                }
            }
            // Frame Member Info only shows: OK closes it.
            Planned::Asm(super::asm_planned::AsmPlanned::FrameMemberInfo) => {
                self.part_planned = None;
            }
            Planned::Asm(super::asm_planned::AsmPlanned::EditFrame) => {
                if self.edit_frame_ok(&d) {
                    self.part_planned = None;
                }
            }
            Planned::Asm(super::asm_planned::AsmPlanned::RemoveEndTreatments) => {
                if self.remove_treatments_ok(&d) {
                    self.part_planned = None;
                }
            }
            Planned::Asm(
                k @ (super::asm_planned::AsmPlanned::CornerJoint
                | super::asm_planned::AsmPlanned::TrimExtend
                | super::asm_planned::AsmPlanned::LengthenShorten
                | super::asm_planned::AsmPlanned::Notch),
            ) => {
                if self.treatment_ok(k, &d) {
                    self.part_planned = None;
                }
            }
            Planned::Asm(super::asm_planned::AsmPlanned::BoltedConnection) => {
                if self.bolted_ok(&d) {
                    self.part_planned = None;
                }
            }
            Planned::Asm(super::asm_planned::AsmPlanned::PlaceImported) => {
                if self.place_imported_ok(&d) {
                    self.part_planned = None;
                }
            }
            Planned::Part(PartPlanned::Derive) => {
                if self.derive_ok(&d) {
                    self.part_planned = None;
                }
            }
            Planned::Part(PartPlanned::MakePart) => {
                if self.make_part_ok(&d) {
                    self.part_planned = None;
                }
            }
            Planned::Part(PartPlanned::MakeComponents) => {
                if self.make_components_ok(&d) {
                    self.part_planned = None;
                }
            }
            Planned::Part(_) if d.kind.has_logic() => {
                if self.surface_ok(&d) {
                    self.part_planned = None;
                }
            }
            k => {
                self.file_status = Some(Err(format!(
                    "{}: only the interface exists yet ({})",
                    k.title(),
                    k.spec()
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::commands::COMMANDS;
    use crate::gui::ribbon::Action;

    /// Every planned command has a panel with options, a title, a
    /// description, its own icon and a registry command; its FEATURES
    /// section exists.
    #[test]
    fn every_planned_part_command_has_a_panel_and_a_command() {
        let features = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/FEATURES.md"),
        )
        .unwrap_or_default();
        let mut icons: Vec<Icon> = Vec::new();
        for p in PartPlanned::ALL {
            assert!(!p.title().is_empty() && !p.about().is_empty());
            assert!(!icons.contains(&p.icon()), "{} shares an icon", p.title());
            icons.push(p.icon());
            assert!(
                features.contains(&format!("## {} ", p.section())),
                "{}: no FEATURES section {}",
                p.title(),
                p.section()
            );
            for pg in p.pages() {
                assert!(!pg.1.is_empty(), "{} {}", p.title(), pg.0);
            }
            let mut d = PartDialog::new(p);
            assert!(!d.prompt().is_empty());
            if d.active.is_some() {
                d.pick("Face Box1:Top".into());
            }
            let cmd = COMMANDS
                .iter()
                .find(|c| c.action == Some(Action::PartPlanned(p)));
            let Some(cmd) = cmd else {
                panic!("{} has no command", p.title());
            };
            assert!(!cmd.aliases.is_empty(), "{} has no alias", cmd.id);
            assert_eq!(cmd.icon, p.icon(), "{}", cmd.id);
        }
    }

    /// A pick fills the active selector and moves to the next empty one;
    /// the same pick again removes it.
    #[test]
    fn selectors_advance() {
        let mut d = PartDialog::new(PartPlanned::SketchDrivenPattern);
        assert_eq!(d.active, Some("features"));
        d.pick("Face Box1:Top".into());
        assert_eq!(d.active, Some("sketch"));
        assert_eq!(d.flow().selectors[0].count, 1);
        d.set_active("features");
        d.pick("Face Box1:Top".into());
        assert!(d.picks.get("features").is_none_or(Vec::is_empty));
    }

    /// A feature row is not a face; a face is.
    #[test]
    fn selectors_take_their_kinds() {
        assert!(takes(FACES, &SelItem::Face("Box1:Top".into())));
        assert!(!takes(BODIES, &SelItem::Face("Box1:Top".into())));
        assert!(takes(SKETCH, &SelItem::Sketch(0)));
        assert!(!takes(EDGES, &SelItem::Sketch(0)));
    }
}
