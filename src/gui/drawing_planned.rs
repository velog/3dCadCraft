//! The interface of the drawing commands whose logic comes later (rule 15,
//! `docs/spec/4.7-drawing-commands.md`): each command opens its whole
//! Inventor 2027 dialog — tabs, selectors that take picks on the sheet,
//! every option with its default — so the dialog can be checked before the
//! command works. OK stays disabled: nothing changes the drawing (rule 5)
//! until a command's logic is written against its spec rows.
//!
//! Dialogs are data ([`Planned::pages`]): a page is a tab, a row one option.
//! The state ([`PlannedDialog`]) is not saved; there is nothing to save yet.

use std::collections::HashMap;

use eframe::egui;

use super::valuefield::ExprValue;

/// One option of a dialog.
#[derive(Clone, Copy, Debug)]
pub(super) enum Row {
    /// A group title.
    Head(&'static str),
    /// A selector: key, label, status-bar prompt.
    Sel(&'static str, &'static str, &'static str),
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

/// A tab of a dialog: its name and rows.
#[derive(Clone, Copy, Debug)]
pub(super) struct Page(pub &'static str, pub &'static [Row]);

use Row::{Button, Check, Combo, Head, Note, Num, Radio, Sel, Text};

/// A drawing command with an interface and no logic yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Planned {
    // Place Views
    Overlay,
    DraftView,
    Break,
    BreakOut,
    Slice,
    Crop,
    AlignHorizontal,
    AlignVertical,
    AlignInPosition,
    AlignBreak,
    StartSketch,
    // Annotate › Dimension
    Retrieve,
    // Feature Notes
    ChamferNote,
    PunchNote,
    // Symbols
    SketchedSymbols,
    SurfaceTexture,
    Welding,
    Caterpillar,
    EndFill,
    ControlFrame,
    DatumIdentifier,
    FeatureIdentifier,
    DatumTargetLeader,
    DatumTargetCircle,
    DatumTargetLine,
    DatumTargetPoint,
    DatumTargetRectangle,
    EdgeSymbol,
    TransitionSymbol,
    CenterlineBisector,
    CenteredPattern,
    AutoCenterlines,
    // Table
    RevisionTag,
    RevisionTable,
    RevisionCloud,
    // Manage
    UpdateAllSheets,
    ReplaceModel,
    DefineBorder,
    DefineTitleBlock,
    DefineSymbol,
}

// ---- shared pages ------------------------------------------------------------

const DATUM_TARGET_PAGES: &[Page] = &[Page(
    "Datum Target",
    &[
        Sel("target", "Target", "click the point or area of the target"),
        Sel("place", "Placement", "click where the symbol goes"),
        Text("Datum Identifier", "A1"),
        Num("Target Size", 6.0, " mm"),
        Check("Movable Target", false),
        Combo("Style", &["Datum Target (ISO)"], 0),
    ],
)];

impl Planned {
    #[cfg(test)]
    pub const ALL: [Planned; 40] = [
        Self::Overlay,
        Self::DraftView,
        Self::Break,
        Self::BreakOut,
        Self::Slice,
        Self::Crop,
        Self::AlignHorizontal,
        Self::AlignVertical,
        Self::AlignInPosition,
        Self::AlignBreak,
        Self::StartSketch,
        Self::Retrieve,
        Self::ChamferNote,
        Self::PunchNote,
        Self::SketchedSymbols,
        Self::SurfaceTexture,
        Self::Welding,
        Self::Caterpillar,
        Self::EndFill,
        Self::ControlFrame,
        Self::DatumIdentifier,
        Self::FeatureIdentifier,
        Self::DatumTargetLeader,
        Self::DatumTargetCircle,
        Self::DatumTargetLine,
        Self::DatumTargetPoint,
        Self::DatumTargetRectangle,
        Self::EdgeSymbol,
        Self::TransitionSymbol,
        Self::CenterlineBisector,
        Self::CenteredPattern,
        Self::AutoCenterlines,
        Self::RevisionTag,
        Self::RevisionTable,
        Self::RevisionCloud,
        Self::UpdateAllSheets,
        Self::ReplaceModel,
        Self::DefineBorder,
        Self::DefineTitleBlock,
        Self::DefineSymbol,
    ];

    /// The dialog's title (Inventor's command name).
    pub fn title(self) -> &'static str {
        match self {
            Self::Overlay => "Overlay View",
            Self::DraftView => "Draft View",
            Self::Break => "Break",
            Self::BreakOut => "Break Out",
            Self::Slice => "Slice",
            Self::Crop => "Crop",
            Self::AlignHorizontal => "Horizontally Align View",
            Self::AlignVertical => "Vertically Align View",
            Self::AlignInPosition => "Positionally Align View",
            Self::AlignBreak => "Remove View Alignment",
            Self::StartSketch => "New Sketch",
            Self::Retrieve => "Retrieve Model Annotations",
            Self::ChamferNote => "Chamfer Note",
            Self::PunchNote => "Punch Notes",
            Self::SketchedSymbols => "Symbols",
            Self::SurfaceTexture => "Surface Texture",
            Self::Welding => "Welding Symbol",
            Self::Caterpillar => "Caterpillar",
            Self::EndFill => "End Fill",
            Self::ControlFrame => "Feature Control Frame",
            Self::DatumIdentifier => "Datum Identifier Symbol",
            Self::FeatureIdentifier => "Feature Identifier Symbol",
            Self::DatumTargetLeader => "Datum Target - Leader",
            Self::DatumTargetCircle => "Datum Target - Circle",
            Self::DatumTargetLine => "Datum Target - Line",
            Self::DatumTargetPoint => "Datum Target - Point",
            Self::DatumTargetRectangle => "Datum Target - Rectangle",
            Self::EdgeSymbol => "Edge Symbol",
            Self::TransitionSymbol => "Transition Symbol",
            Self::CenterlineBisector => "Centerline Bisector",
            Self::CenteredPattern => "Centered Pattern",
            Self::AutoCenterlines => "Automated Centerlines",
            Self::RevisionTag => "Revision Tag",
            Self::RevisionTable => "Revision Table",
            Self::RevisionCloud => "Revision Cloud",
            Self::UpdateAllSheets => "Update All Sheets",
            Self::ReplaceModel => "Replace Model Reference",
            Self::DefineBorder => "Define Border",
            Self::DefineTitleBlock => "Define Title Block",
            Self::DefineSymbol => "Define Sketched Symbol",
        }
    }

    /// The dialog's tabs and options.
    pub fn pages(self) -> &'static [Page] {
        match self {
            Self::Overlay => &[Page(
                "Overlay",
                &[
                    Sel(
                        "parent",
                        "Parent View",
                        "select the parent view of an assembly",
                    ),
                    Combo("Positional Representation", &["Master"], 0),
                    Combo("Design View Representation", &["Default"], 0),
                    Text("View Identifier", "VIEW"),
                    Check("Toggle Label Visibility", false),
                    Radio(
                        "Style",
                        &["Hidden Line", "Hidden Line Removed", "Shaded"],
                        1,
                    ),
                    Combo("Layer", &["Phantom (ISO)"], 0),
                    Note(
                        "Each positional representation is drawn over the parent in phantom lines",
                    ),
                ],
            )],
            Self::DraftView => &[Page(
                "Draft View",
                &[
                    Text("View Identifier", "VIEW"),
                    Combo("Scale", &["1:1", "1:2", "1:5", "2:1", "5:1"], 0),
                    Check("Toggle Label Visibility", true),
                    Button("Edit View Label..."),
                    Sel(
                        "place",
                        "Placement",
                        "click on the sheet: a sketch opens in the new view",
                    ),
                    Note("A view holding only a 2D sketch (no model)"),
                ],
            )],
            Self::Break => &[Page(
                "Break",
                &[
                    Sel("view", "View", "select the view to break"),
                    Radio("Style", &["Rectangular", "Structural"], 0),
                    Radio(
                        "Orientation",
                        &["Horizontal", "Vertical", "Aligned to geometry"],
                        0,
                    ),
                    Sel(
                        "align",
                        "Align To",
                        "select an edge the break lines are square to",
                    ),
                    Num("Display (Min - Max, % of the gap)", 50.0, ""),
                    Num("Gap", 10.0, " mm"),
                    Num("Symbols (Structural)", 1.0, ""),
                    Check("Propagate to parent view", false),
                    Sel(
                        "first",
                        "First Break Line",
                        "click the first break line in the view",
                    ),
                    Sel("second", "Second Break Line", "click the second break line"),
                    Note("One break per dialog; the orientation cannot be changed later"),
                ],
            )],
            Self::BreakOut => &[
                Page(
                    "Shape",
                    &[
                        Sel("view", "View", "select the view"),
                        Sel(
                            "profile",
                            "Boundary Profile",
                            "select a closed sketch profile drawn on the view",
                        ),
                        Head("Depth"),
                        Combo(
                            "Depth Type",
                            &["From Point", "To Sketch", "To Hole", "Through Part"],
                            0,
                        ),
                        Sel(
                            "depth",
                            "Depth Point",
                            "click the point or hole the depth is measured from",
                        ),
                        Num("Depth", 5.0, " mm"),
                        Check("Show Hidden Edges", false),
                        Check("Section All Parts", false),
                    ],
                ),
                Page("Display", &[Combo("Hatch", &["By Material", "ANSI31"], 0)]),
            ],
            Self::Slice => &[Page(
                "Slice",
                &[
                    Sel("view", "View", "select the view to slice"),
                    Sel(
                        "line",
                        "Slice Line",
                        "select a sketch line drawn on another view",
                    ),
                    Check("Slice All Parts", false),
                    Note("A zero-depth section: only the cut faces stay"),
                ],
            )],
            Self::Crop => &[Page(
                "Crop",
                &[
                    Sel("view", "View", "select the view to crop"),
                    Radio("Boundary", &["Rectangular", "Sketch Profile"], 0),
                    Sel(
                        "boundary",
                        "Boundary",
                        "click two corners or select a closed sketch",
                    ),
                    Radio("Cut Line Display", &["Show", "Hide"], 1),
                ],
            )],
            Self::AlignHorizontal | Self::AlignVertical | Self::AlignInPosition => &[Page(
                "Align",
                &[
                    Sel("view", "View", "select the view to align"),
                    Sel("base", "Base View", "select the view it aligns to"),
                    Note("The aligned view then moves only along the alignment"),
                ],
            )],
            Self::AlignBreak => &[Page(
                "Alignment",
                &[
                    Sel("view", "View", "select a view whose alignment is removed"),
                    Note("The view can then be moved freely"),
                ],
            )],
            Self::StartSketch => &[Page(
                "Sketch",
                &[
                    Sel(
                        "target",
                        "Sheet or View",
                        "select a view (the sketch belongs to it) or click the sheet",
                    ),
                    Note(
                        "Sketch tools of the drawing: lines, arcs, text, Fill/Hatch Sketch Region, Project Geometry",
                    ),
                ],
            )],
            Self::Retrieve => &[
                Page(
                    "Selection",
                    &[
                        Sel("view", "Select View", "select a view"),
                        Radio("Source Selection", &["Select Features", "Select Parts"], 0),
                        Sel("source", "Source", "select features or parts in the view"),
                        Sel(
                            "dims",
                            "Select Dimensions",
                            "select the retrieved dimensions to keep",
                        ),
                        Head("Retrieve"),
                        Check("Dimensions", true),
                        Check("Model Annotations (3D)", false),
                        Check("Sketch Dimensions", true),
                        Check("Feature Dimensions", true),
                        Check("Only Planar to the View", true),
                    ],
                ),
                Page(
                    "Tolerances",
                    &[
                        Check("Include Tolerances", true),
                        Check("Include Notes", true),
                    ],
                ),
            ],
            Self::ChamferNote => &[Page(
                "Chamfer Note",
                &[
                    Sel("chamfer", "Chamfer Edge", "select the chamfer's edge"),
                    Sel(
                        "reference",
                        "Reference Edge",
                        "select the edge the chamfer is measured from",
                    ),
                    Sel("place", "Placement", "click where the note goes"),
                    Text("Note Format", "<DIST1> X <ANGLE>"),
                    Note(
                        "Edit Chamfer Note: Distance 1, Distance 2, Angle values with their precision and tolerance",
                    ),
                ],
            )],
            Self::PunchNote => &[Page(
                "Punch Notes",
                &[
                    Sel(
                        "punch",
                        "Punch",
                        "select a punch centre in a flat pattern view",
                    ),
                    Sel("place", "Placement", "click where the note goes"),
                    Text("Note Format", "<PUNCH ID> <DIRECTION> <DEPTH>"),
                ],
            )],
            Self::SketchedSymbols => &[Page(
                "Symbols",
                &[
                    Combo("Symbol", &["(no symbols in Drawing Resources)"], 0),
                    Num("Scale", 1.0, ""),
                    Num("Rotation", 0.0, "°"),
                    Check("Static", false),
                    Check("Leader", false),
                    Sel("place", "Placement", "click where the symbol goes"),
                    Note("Symbols are sketches in Drawing Resources > Sketched Symbols"),
                ],
            )],
            Self::SurfaceTexture => &[Page(
                "Surface Texture",
                &[
                    Sel(
                        "surface",
                        "Surface",
                        "click an edge (with a leader) or the sheet",
                    ),
                    Head("Surface Type"),
                    Radio(
                        "Type",
                        &[
                            "Basic",
                            "Material removal required",
                            "Material removal prohibited",
                        ],
                        0,
                    ),
                    Head("Options"),
                    Check("Force tail", false),
                    Check("All-round", false),
                    Check("Place as majority symbol", false),
                    Head("Requirements"),
                    Text("A (roughness value)", "Ra 3.2"),
                    Text("A'", ""),
                    Text("B (production method)", ""),
                    Text("C (sampling length)", ""),
                    Combo("D (lay)", &["", "=", "⊥", "X", "M", "C", "R", "P"], 0),
                    Text("E (machining allowance)", ""),
                    Head("Preset"),
                    Combo("Preset", &["(none)"], 0),
                    Button("Create Preset"),
                ],
            )],
            Self::Welding => &[
                Page(
                    "Welding Symbol",
                    &[
                        Sel(
                            "arrow",
                            "Leader",
                            "click the weld's edge (arrow), then the symbol's place",
                        ),
                        Combo("Identification Line", &["None", "Above", "Below"], 0),
                        Button("Swap Arrow/Other Symbols"),
                        Check("Stagger", false),
                        Check("Field weld flag", false),
                        Check("All-around symbol", false),
                        Text("Tail note", ""),
                        Check("Closed note tail", false),
                    ],
                ),
                Page(
                    "Arrow Side",
                    &[
                        Combo(
                            "Symbol",
                            &[
                                "None",
                                "Fillet",
                                "Square Groove",
                                "V Groove",
                                "Bevel Groove",
                                "U Groove",
                                "J Groove",
                                "Flare V",
                                "Flare Bevel",
                                "Plug / Slot",
                                "Spot",
                                "Seam",
                                "Back / Backing",
                                "Surfacing",
                                "Edge",
                            ],
                            1,
                        ),
                        Text("Prefix", ""),
                        Text("Leg / Size", "a5"),
                        Text("Length - Pitch", ""),
                        Text("Number", ""),
                        Combo(
                            "Contour",
                            &[
                                "None",
                                "Flush / Flat",
                                "Convex",
                                "Concave",
                                "Toes Blended Smooth",
                            ],
                            0,
                        ),
                        Text("Method", ""),
                        Text("Angle", ""),
                        Text("Root Gap", ""),
                        Text("Depth / Penetration", ""),
                    ],
                ),
                Page(
                    "Other Side",
                    &[
                        Combo(
                            "Symbol",
                            &[
                                "None",
                                "Fillet",
                                "Square Groove",
                                "V Groove",
                                "Bevel Groove",
                            ],
                            0,
                        ),
                        Text("Prefix", ""),
                        Text("Leg / Size", ""),
                        Text("Length - Pitch", ""),
                        Combo("Contour", &["None", "Flush / Flat", "Convex", "Concave"], 0),
                        Check("Spacer (double groove)", false),
                    ],
                ),
                Page(
                    "Symbol Box",
                    &[
                        Button("Add"),
                        Button("Delete"),
                        Button("Previous"),
                        Button("Next"),
                        Note("Several reference lines stacked on one leader"),
                    ],
                ),
            ],
            Self::Caterpillar => &[Page(
                "Caterpillar",
                &[
                    Sel("edges", "Edges", "select the weld's edges in the view"),
                    Radio("Direction", &["Normal", "Flipped"], 0),
                    Num("Width", 4.0, " mm"),
                    Num("Arc Length", 2.0, " mm"),
                    Num("Spacing", 1.0, " mm"),
                    Num("Angle", 90.0, "°"),
                    Head("Extents"),
                    Radio("Extent", &["Full", "Partial"], 0),
                    Num("Start Offset", 0.0, " mm"),
                    Num("Length", 20.0, " mm"),
                ],
            )],
            Self::EndFill => &[Page(
                "End Fill",
                &[
                    Sel(
                        "weld",
                        "Weld",
                        "select the end of a weld bead in an end view",
                    ),
                    Num("Size", 5.0, " mm"),
                    Combo("Fill", &["Solid", "Hatch"], 0),
                    Combo("Shape", &["Fillet", "Groove"], 0),
                ],
            )],
            Self::ControlFrame => &[Page(
                "Feature Control Frame",
                &[
                    Sel(
                        "leader",
                        "Leader",
                        "click the edge (leader start), then where the frame goes",
                    ),
                    Text("Note (top)", ""),
                    Head("Row 1"),
                    Combo(
                        "Sym",
                        &[
                            "",
                            "⏤ Straightness",
                            "⏥ Flatness",
                            "○ Circularity",
                            "⌭ Cylindricity",
                            "⌒ Profile of a line",
                            "⌓ Profile of a surface",
                            "∠ Angularity",
                            "⟂ Perpendicularity",
                            "∥ Parallelism",
                            "⌖ Position",
                            "◎ Concentricity",
                            "⌯ Symmetry",
                            "↗ Circular runout",
                            "⌰ Total runout",
                        ],
                        0,
                    ),
                    Text("Tolerance 1", ""),
                    Text("Tolerance 2 (ANSI)", ""),
                    Text("Datum 1", ""),
                    Text("Datum 2", ""),
                    Text("Datum 3", ""),
                    Head("Row 2"),
                    Combo(
                        "Sym ",
                        &["", "⌖ Position", "⟂ Perpendicularity", "∥ Parallelism"],
                        0,
                    ),
                    Text("Tolerance", ""),
                    Text("Datums", ""),
                    Head("Notes"),
                    Text("Notes", ""),
                    Text("Datum Identifier", ""),
                    Radio("Profile", &["None", "All Around", "All Over"], 0),
                    Button("Insert Modifier: Ⓜ Ⓛ Ⓟ Ⓕ Ⓣ Ⓤ"),
                ],
            )],
            Self::DatumIdentifier => &[Page(
                "Datum Identifier",
                &[
                    Sel(
                        "leader",
                        "Leader",
                        "click the edge, then where the symbol goes",
                    ),
                    Text("Datum", "A"),
                    Combo("Style", &["Datum Identifier (ISO)"], 0),
                ],
            )],
            Self::FeatureIdentifier => &[Page(
                "Feature Identifier",
                &[
                    Sel(
                        "leader",
                        "Leader",
                        "click the feature, then where the symbol goes",
                    ),
                    Text("Identifier", "1"),
                ],
            )],
            Self::DatumTargetLeader
            | Self::DatumTargetCircle
            | Self::DatumTargetLine
            | Self::DatumTargetPoint
            | Self::DatumTargetRectangle => DATUM_TARGET_PAGES,
            Self::EdgeSymbol => &[Page(
                "Edge Symbol",
                &[
                    Sel("edge", "Edge", "click the edge, then where the symbol goes"),
                    Text("Upper Deviation", "+0,3"),
                    Text("Lower Deviation", ""),
                    Check("Applies to All Edges", false),
                    Note("ISO 13715: burr / passing / undercut of an edge"),
                ],
            )],
            Self::TransitionSymbol => &[Page(
                "Transition Symbol",
                &[
                    Sel("edge", "Transition Edge", "click the tangent edge to mark"),
                    Note("Marks where a surface transition (tangent edge) lies"),
                ],
            )],
            Self::CenterlineBisector => &[Page(
                "Centerline Bisector",
                &[
                    Sel("first", "First Edge", "select the first edge"),
                    Sel("second", "Second Edge", "select the second edge"),
                ],
            )],
            Self::CenteredPattern => &[Page(
                "Centered Pattern",
                &[
                    Sel(
                        "center",
                        "Pattern Center",
                        "click the pattern's centre circle",
                    ),
                    Sel(
                        "features",
                        "Features",
                        "click the pattern's circles in order (right-click: Create)",
                    ),
                ],
            )],
            Self::AutoCenterlines => &[Page(
                "Automated Centerlines",
                &[
                    Sel("views", "Views", "select the views (or keep the selection)"),
                    Head("Apply To"),
                    Check("Hole Features", true),
                    Check("Fillet Features", false),
                    Check("Cylindrical Features", true),
                    Check("Revolved Features", true),
                    Check("Work Features", false),
                    Check("Punches", false),
                    Check("Sheet Metal Bends", false),
                    Check("Circular Patterns", true),
                    Check("Rectangular Patterns", false),
                    Head("Projection"),
                    Check("Axis Normal", true),
                    Check("Axis Parallel", true),
                    Head("Radius Threshold"),
                    Num("Fillet Minimum", 0.0, " mm"),
                    Num("Fillet Maximum", 0.0, " mm"),
                    Num("Circular Minimum", 0.0, " mm"),
                    Num("Circular Maximum", 0.0, " mm"),
                    Num("Arc Angle Minimum", 0.0, "°"),
                ],
            )],
            Self::RevisionTag => &[Page(
                "Revision Tag",
                &[
                    Sel(
                        "place",
                        "Placement",
                        "click the place of the tag (an edge gives a leader)",
                    ),
                    Text("Revision", "A"),
                    Combo("Style", &["Revision Tag (ISO)"], 0),
                ],
            )],
            Self::RevisionTable => &[
                Page(
                    "Table Scope",
                    &[
                        Radio("Scope", &["Entire Drawing", "Active Sheet"], 0),
                        Head("Revision Index"),
                        Radio("Index", &["Alpha", "Numeric"], 0),
                        Text("Start Value", "A"),
                        Check("Auto-Index", true),
                        Check("Update property on revision number edit", true),
                        Head("Delete Rule"),
                        Radio("Delete", &["Manual", "Auto (keep the last rows)"], 0),
                        Num("Rows to Keep", 5.0, ""),
                        Sel("place", "Placement", "click where the table goes"),
                    ],
                ),
                Page(
                    "Columns",
                    &[
                        Button("Column Chooser..."),
                        Note(
                            "REV, DESCRIPTION, DATE, APPROVED (rows from Revision Number and the dates)",
                        ),
                    ],
                ),
            ],
            Self::RevisionCloud => &[Page(
                "Revision Cloud",
                &[
                    Sel(
                        "points",
                        "Outline",
                        "click the cloud's corners (double-click closes)",
                    ),
                    Num("Arc Length", 8.0, " mm"),
                    Check("Associate to View", true),
                ],
            )],
            Self::UpdateAllSheets => &[Page(
                "Update All Sheets",
                &[
                    Note(
                        "Recomputes the views of every sheet from their models (also the sheets not shown)",
                    ),
                    Check("Defer Updates (Document Settings)", false),
                ],
            )],
            Self::ReplaceModel => &[Page(
                "Replace Model Reference",
                &[
                    Combo("Current Model", &["(models referenced by the drawing)"], 0),
                    Button("Browse..."),
                    Text("New Model", ""),
                    Combo("Model State", &["[Primary]"], 0),
                    Note(
                        "The new model must be a copy or a related file: annotations follow by persistent names, the missing ones become orphans",
                    ),
                ],
            )],
            Self::DefineBorder => &[Page(
                "Define Border",
                &[
                    Radio("Kind", &["Default (zones)", "Sketched"], 0),
                    Num("Horizontal Zones", 6.0, ""),
                    Num("Vertical Zones", 4.0, ""),
                    Radio("Label", &["Alphabetical", "Numeric", "None"], 0),
                    Check("Label from Bottom Right", true),
                    Check("Delimit by Lines", true),
                    Check("Center Marks", true),
                    Num("Margin", 10.0, " mm"),
                    Text("Name", "Default Border"),
                ],
            )],
            Self::DefineTitleBlock => &[Page(
                "Define Title Block",
                &[
                    Note(
                        "A sketch with lines, texts and property fields (iProperties of the drawing or of the model of the first view), prompted entries",
                    ),
                    Text("Name", "ISO"),
                    Button("Edit Definition..."),
                ],
            )],
            Self::DefineSymbol => &[Page(
                "Define Sketched Symbol",
                &[
                    Note(
                        "A sketch with geometry, texts and property fields; insertion and connection grips",
                    ),
                    Text("Name", "Symbol1"),
                    Button("Edit Definition..."),
                ],
            )],
        }
    }

    /// What the command does (Inventor's behaviour, our words).
    pub fn about(self) -> &'static str {
        match self {
            Self::Overlay => {
                "Positional representations of an assembly drawn over a view in phantom lines"
            }
            Self::DraftView => "A view with only a 2D sketch",
            Self::Break => {
                "Shortens a long part between two break lines; dimensions across it keep the true length"
            }
            Self::BreakOut => "Removes material inside a closed sketch profile to a depth",
            Self::Slice => "A zero-depth section of a view along a sketch line",
            Self::Crop => "Clips a view to a rectangle or a closed sketch",
            Self::AlignHorizontal | Self::AlignVertical | Self::AlignInPosition => {
                "Aligns a view to another one"
            }
            Self::AlignBreak => "Frees a view from its alignment",
            Self::StartSketch => "A sketch on the sheet or in a view",
            Self::Retrieve => {
                "Brings the model's sketch / feature dimensions and 3D annotations into a view"
            }
            Self::ChamferNote => "A note with the chamfer's distances and angle",
            Self::PunchNote => "A note of a punch in a flat pattern view",
            Self::SketchedSymbols => "Places a sketched symbol from Drawing Resources",
            Self::SurfaceTexture => "A surface texture (roughness) symbol, ISO 1302 / 21920",
            Self::Welding => "A welding symbol (ISO 2553): arrow and other side, flags, tail",
            Self::Caterpillar => "Weld beads drawn along edges",
            Self::EndFill => "The filled end section of a weld bead",
            Self::ControlFrame => "A GD&T feature control frame (ISO 1101)",
            Self::DatumIdentifier => "A datum letter on an edge or surface",
            Self::FeatureIdentifier => "A feature identifier symbol",
            Self::DatumTargetLeader
            | Self::DatumTargetCircle
            | Self::DatumTargetLine
            | Self::DatumTargetPoint
            | Self::DatumTargetRectangle => "A datum target (ISO 5459): point, line or area",
            Self::EdgeSymbol => "An edge state symbol (ISO 13715)",
            Self::TransitionSymbol => "Marks a tangent transition edge",
            Self::CenterlineBisector => "A centre line half-way between two edges",
            Self::CenteredPattern => "Centre lines of a circular pattern of holes",
            Self::AutoCenterlines => {
                "Centre marks and lines on every chosen feature type of the views"
            }
            Self::RevisionTag => "A revision tag next to a change",
            Self::RevisionTable => "Rows of revisions (Revision Number of the drawing)",
            Self::RevisionCloud => "A cloud round a change",
            Self::UpdateAllSheets => "Updates the views of every sheet",
            Self::ReplaceModel => "Points the drawing's views at another model file",
            Self::DefineBorder => "A new border template in Drawing Resources",
            Self::DefineTitleBlock => "A new title block template in Drawing Resources",
            Self::DefineSymbol => "A new sketched symbol template in Drawing Resources",
        }
    }
}

/// The open dialog of a planned command.
#[derive(Clone, Debug)]
pub(super) struct PlannedDialog {
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
}

impl PlannedDialog {
    pub(super) fn new(kind: Planned) -> Self {
        let active = kind.pages().iter().flat_map(|p| p.1).find_map(|r| match r {
            Sel(k, ..) => Some(*k),
            _ => None,
        });
        Self {
            kind,
            tab: 0,
            bools: HashMap::new(),
            indices: HashMap::new(),
            nums: HashMap::new(),
            texts: HashMap::new(),
            active,
            picks: HashMap::new(),
        }
    }

    /// The active selector's prompt (status bar).
    pub(super) fn prompt(&self) -> String {
        let t = self.kind.title();
        match self.active.and_then(|k| self.selector(k)) {
            Some((_, prompt)) => format!("{t}: {prompt}"),
            None => format!("{t}: set the options (the command itself comes later)"),
        }
    }

    fn selector(&self, key: &str) -> Option<(&'static str, &'static str)> {
        self.kind
            .pages()
            .iter()
            .flat_map(|p| p.1)
            .find_map(|r| match r {
                Sel(k, label, prompt) if *k == key => Some((*label, *prompt)),
                _ => None,
            })
    }

    /// The active selector takes whole views (not their geometry).
    pub(super) fn wants_view(&self) -> bool {
        matches!(self.active, Some("view" | "views" | "parent" | "base"))
    }

    /// Takes a pick for the active selector (described as text) and moves
    /// to the next selector, as Inventor's selectors auto-advance.
    pub(super) fn pick(&mut self, what: String) {
        let Some(key) = self.active else {
            return;
        };
        let list = self.picks.entry(key).or_default();
        if let Some(i) = list.iter().position(|w| *w == what) {
            list.remove(i);
            return;
        }
        list.push(what);
        let keys: Vec<&'static str> = self
            .kind
            .pages()
            .iter()
            .flat_map(|p| p.1)
            .filter_map(|r| match r {
                Sel(k, ..) => Some(*k),
                _ => None,
            })
            .collect();
        if let Some(i) = keys.iter().position(|k| *k == key)
            && let Some(next) = keys.get(i + 1)
            && self.picks.get(next).is_none_or(Vec::is_empty)
        {
            self.active = Some(next);
        }
    }

    /// Draws the dialog's body; returns true when Cancel was pressed.
    pub(super) fn ui(&mut self, ui: &mut egui::Ui) -> bool {
        let pages = self.kind.pages();
        if pages.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                for (i, p) in pages.iter().enumerate() {
                    ui.selectable_value(&mut self.tab, i, p.0);
                }
            });
            ui.separator();
        }
        let ti = self.tab.min(pages.len().saturating_sub(1));
        let Some(page) = pages.get(ti) else {
            return false;
        };
        let tip =
            "Interface only: the command's logic comes next (docs/spec/4.7-drawing-commands.md)";
        egui::ScrollArea::vertical()
            .max_height(520.0)
            .show(ui, |ui| {
                egui::Grid::new(("drw_planned", self.kind.title(), ti))
                    .num_columns(2)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        for (ri, row) in page.1.iter().enumerate() {
                            self.row(ui, (ti, ri), row);
                        }
                    });
            });
        ui.separator();
        ui.label(egui::RichText::new(self.kind.about()).weak());
        let mut cancel = false;
        ui.horizontal(|ui| {
            ui.add_enabled(false, egui::Button::new("OK"))
                .on_disabled_hover_text(tip);
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
            ui.add_enabled(false, egui::Button::new("Apply"))
                .on_disabled_hover_text(tip);
        });
        cancel
    }

    fn row(&mut self, ui: &mut egui::Ui, at: (usize, usize), row: &Row) {
        match *row {
            Head(t) => {
                ui.label(egui::RichText::new(t).strong());
                ui.end_row();
            }
            Note(t) => {
                ui.label("");
                ui.label(egui::RichText::new(t).weak().italics());
                ui.end_row();
            }
            Button(t) => {
                ui.label("");
                ui.add_enabled(false, egui::Button::new(t))
                    .on_disabled_hover_text("Comes with the command's logic");
                ui.end_row();
            }
            Sel(key, label, prompt) => {
                let active = self.active == Some(key);
                let n = self.picks.get(key).map_or(0, Vec::len);
                ui.horizontal(|ui| {
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    let c = if active {
                        egui::Color32::from_rgb(40, 120, 230)
                    } else if n > 0 {
                        egui::Color32::from_rgb(60, 160, 80)
                    } else {
                        egui::Color32::GRAY
                    };
                    ui.painter().circle_filled(r.center(), 4.0, c);
                    if ui
                        .selectable_label(active, label)
                        .on_hover_text(prompt)
                        .clicked()
                    {
                        self.active = Some(key);
                    }
                });
                ui.horizontal(|ui| {
                    let text = match self.picks.get(key).map(Vec::as_slice) {
                        None | Some([]) => "nothing selected".to_string(),
                        Some([one]) => one.clone(),
                        Some(v) => format!("{} selected", v.len()),
                    };
                    ui.label(egui::RichText::new(text).weak());
                    if n > 0 && ui.small_button("x").on_hover_text("Clear").clicked() {
                        self.picks.remove(key);
                    }
                });
                ui.end_row();
            }
            Check(label, on) => {
                let v = self.bool_at(at, on);
                ui.label("");
                ui.checkbox(v, label);
                ui.end_row();
            }
            Radio(label, opts, def) => {
                ui.label(label);
                let v = self.index_at(at, def);
                ui.horizontal_wrapped(|ui| {
                    for (i, o) in opts.iter().enumerate() {
                        ui.radio_value(v, i, *o);
                    }
                });
                ui.end_row();
            }
            Combo(label, opts, def) => {
                ui.label(label);
                let v = self.index_at(at, def);
                egui::ComboBox::from_id_salt(("drw_planned_combo", at))
                    .selected_text(opts.get(*v).copied().unwrap_or(""))
                    .show_ui(ui, |ui| {
                        for (i, o) in opts.iter().enumerate() {
                            ui.selectable_value(v, i, *o);
                        }
                    });
                ui.end_row();
            }
            Num(label, def, unit) => {
                ui.label(label);
                let v = self.num_at(at, def);
                ui.add(ExprValue::new(v).suffix(unit));
                ui.end_row();
            }
            Text(label, def) => {
                ui.label(label);
                let v = self.text_at(at, def);
                ui.add(egui::TextEdit::singleline(v).desired_width(220.0));
                ui.end_row();
            }
        }
    }

    fn bool_at(&mut self, at: (usize, usize), def: bool) -> &mut bool {
        self.bools.entry(at).or_insert(def)
    }

    fn index_at(&mut self, at: (usize, usize), def: usize) -> &mut usize {
        self.indices.entry(at).or_insert(def)
    }

    fn num_at(&mut self, at: (usize, usize), def: f64) -> &mut f64 {
        self.nums.entry(at).or_insert(def)
    }

    fn text_at(&mut self, at: (usize, usize), def: &str) -> &mut String {
        self.texts.entry(at).or_insert_with(|| def.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::commands::COMMANDS;
    use crate::gui::drawing::DrwCmd;
    use crate::gui::ribbon::Action;

    /// Every planned command has a dialog with options, a selector's
    /// prompt per selector, a title and a description; every one but Edit
    /// Dimension (a double-click on a dimension) is a registry command.
    #[test]
    fn every_planned_command_has_a_dialog_and_a_command() {
        for p in Planned::ALL {
            assert!(!p.title().is_empty() && !p.about().is_empty());
            let pages = p.pages();
            assert!(!pages.is_empty(), "{}", p.title());
            for pg in pages {
                assert!(!pg.1.is_empty(), "{} {}", p.title(), pg.0);
            }
            let mut d = PlannedDialog::new(p);
            assert!(!d.prompt().is_empty());
            if d.active.is_some() {
                d.pick("View 1".into());
            }
            assert!(
                COMMANDS
                    .iter()
                    .any(|c| c.action == Some(Action::Drawing(DrwCmd::Planned(p)))),
                "{} has no command",
                p.title()
            );
        }
    }

    /// A pick fills the active selector and moves to the next one; the
    /// same pick again removes it.
    #[test]
    fn selectors_advance() {
        let mut d = PlannedDialog::new(Planned::Break);
        assert_eq!(d.active, Some("view"));
        d.pick("View 1".into());
        assert_eq!(d.active, Some("align"));
        d.active = Some("view");
        d.pick("View 1".into());
        assert!(d.picks.get("view").is_none_or(Vec::is_empty));
    }
}
