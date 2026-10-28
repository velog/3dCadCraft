# 4 Drawings (2D documentation)

Status: ✅ done · 🟡 partial · ⬜ to do · ⛔ blocked by another feature.

> **Research document first** (the owner's rule, 2026-10-24). How Inventor 2027 makes drawings, what we can
> reuse, the hard part (hidden line removal) and a staged plan. Every row starts ⬜; *(to confirm)* rows come
> from general Inventor knowledge and are checked against the 2027 help before they are implemented.

## 1. Purpose

A drawing document (`.idw` / `.dwg` in Inventor) holds sheets with views of parts and assemblies,
dimensions and notes, a title block filled from iProperties, parts lists with balloons and tables. It is
**associative**: a change of the model updates the views; annotations follow the edges they are attached
to, and an annotation whose edge is gone is marked (not deleted).

## 2. Inputs

- Part, assembly and (later) sheet metal documents — linked files, as an assembly links parts (3.1).
- Their iProperties (0.16), BOM (3.6), Hole data (`PartDocument::holes`, 2.3), sketch / feature
  dimensions (for Retrieve), sheet metal flat pattern and bends (5.6).

## 3. Options

### 3.1 Document, sheets and resources (4.1)

| Option | Inventor behaviour | Restrictions | Our status |
|---|---|---|---|
| New Drawing | from a template (ISO / ANSI / DIN / JIS / GB / BSI); one sheet with a border and title block | | 🟡 File → New → Drawing (and File → New Drawing) adds an ISO drawing (one A3 sheet, first angle) to the open project; template choice ANSI / DIN not in the UI yet; waiting for a visual check |
| Sheets | several per drawing; New Sheet, Edit Sheet (name, size A4–A0 / ANSI A–E / custom — Height / Width editable only for a custom size, orientation), Delete (several at once, a dialog lists them), Copy / Paste, Next / Previous Sheet in the canvas menu; Exclude from count / printing | | 🟡 New Sheet (NS), Edit Sheet (ES: name, size incl. custom, orientation, revision, border / title block, exclude from count / printing), Delete Sheet, Activate (browser); Copy / Paste and Next / Previous Sheet ⬜; waiting for a visual check |
| Sheet Formats | a saved sheet (size, border, title block, views with their settings) in Drawing Resources; Create Sheet Format from a sheet's menu (option *Fit views to sheet*); New Sheet from a format asks for a model for its views; a format is never edited in place — make a sheet from it, change it, save it under the same name | | ⬜ |
| Drawing Resources | Sheet Formats (sheet + views ready to place), Borders, Title Blocks, Sketched Symbols — editable as sketches with property fields | | ⬜ |
| Title block fields | text bound to iProperties of the drawing or the model of the first view (Part Number, Description, Designer, Date, Material, Mass, Scale, Sheet n of m); **prompted entries** asked when the block is placed, changed later in Edit Property Fields (only prompted values are editable there; iProperties through their own dialog); Reorder Prompted Entries in the definition | | 🟡 a fixed ISO 7200-like block (title, part number, designed / checked by, date, material, scale, revision, sheet n / m) filled from the drawing's and the first view's model iProperties; prompted entries ⬜; waiting for a visual check |
| Title block / border definition | drawn as a sketch (Manage › Define › Title Block), text with property fields, images; Save Title Block; Edit Definition from the sheet | | ⬜ |
| Standards (Styles) | Standard — General: units, decimal marker, preset values (line weights, text heights, scales, hatch angles), characters excluded from automatic identifiers, Global Line Scale; View Preferences: view label per view type (prefix, text with scale / identifier fields, above / below, visible), default cut edges (Jagged / Smooth) for detail and section, thread edge display, **projection type** (first angle / third angle), Front View Plane (XY / YZ / XZ / From Model, new views only); Available Styles; Object Defaults (style + layer per object type: visible, hidden, tangent, centre, section, dimension…); **Material Hatch Pattern Defaults** (material → hatch pattern table, default pattern) | | 🟡 the drawing stores its standard (ISO / ANSI / DIN) and projection type (ISO, DIN first angle; ANSI third); zones of the border follow it; the rest ⬜ |
| Active standard / Update / Purge | Style and Standard Editor as for parts | | ⬜ |

### 3.2 Views (4.2)

| View | Inventor behaviour | Restrictions | Our status |
|---|---|---|---|
| **Base View** | Drawing View dialog, Component tab: File (open models or browse), Representation (Model State; Design View + Associative; Camera View; Include 3D Annotations; Position View — assemblies), Style (Hidden lines displayed / removed / Shaded, *Style from Base*), Raster View, Scale (+ *Scale from Base*), label visibility and text, view identifier, Hide small parts (size threshold); orientation is chosen with the ViewCube in the preview; click to place; continue with projected views from the cursor | | 🟡 Drawing View dialog with all four tabs; File: the project's parts or Browse... (links a part file, the drawing saved first); orientation with the ViewCube in the preview (row below); Hidden Line / Hidden Line Removed (Shaded drawn as Hidden Line Removed for now); scale presets or any number, fitted to the sheet until set; identifier and label; OK places the view and Projected View continues; Representation, Raster View, Hide small parts shown disabled; waiting for a visual check |
| Base view preview (§3.2a) | the preview appears at a temporary place on the sheet and is dragged (Move manipulator); a corner drag changes the scale, snapping to the standard's scales (Ctrl: any value) with the frame following the pointer and the resulting scale shown next to it, projected children follow; triangles on the preview's border add projected views (orthographic and isometric) at a default place, an x removes one; a ViewCube next to the preview's upper right corner sets the orientation (faces, edges, corners, arrows to the neighbouring face, 90° roll buttons, drag to orbit); its menu: Orthographic / Perspective / Perspective with Ortho Faces, Custom View Orientation, Current View Orientation, Saved Camera (presentations); Custom View: free orbit, Rotate at Angle (Incremental View Rotate), View Face, Perspective, Finish Custom View; Edit View offers the same cube and corner scaling | — | 🟡 preview dragged or moved by a click; corner scaling with snapping / Ctrl (resize cursor and an enlarged handle over a corner, move cursor over the frame, while dragging a frame at the pointer's size and a label with the scale a release gives); triangles + x add and remove projected views (created at OK); ViewCube: faces, edges, corners, arrows, roll, Home, drag to orbit (the model's edges drawn while orbiting, the hidden lines follow); menu and right click; Custom View (Change View Orientation in the dialog too): orbit by dragging anywhere, Rotate at Angle about the sheet's horizontal / vertical / normal axis by an increment, Finish / Cancel / Esc; any orientation is stored (`ViewOrientation::Custom`, format revision 27). Perspective, Current View Orientation, Saved Camera and View Face are shown disabled |
| Base view options | Sheet Metal View: Folded Model / Flat Pattern (needs a flat pattern), Bend Extents, Punch Center; Model tab: reference data (hidden lines separately / with all bodies, style, margin), weldment state and welding symbols; Display Options (only the relevant ones shown): Standard Parts hidden lines (Never / Obey Browser) and Section (Never / Always / Obey Browser, default Obey Browser), Cut Inheritance, Thread Feature (off by default), Tangent Edges + Foreshortened, Interference Edges, Align to Base, Definition in Base View, Orientation from Base (on by default), Hatching, Center Justified / Fixed; Recovery Options: Include Surface Bodies, Include Mesh Bodies (both off), All Model Dimensions (planar to the view, not yet on the sheet, with tolerances), Use Work Features (as reference lines) | | 🟡 Align to Base works; the other options are in the dialog, disabled with their stage in the tooltip |
| Raster View | pixel-based view computed fast; precise views are computed in the background and replace it; green frame in the view, marked browser icon; context menu converts raster ↔ precise; some commands unavailable; printing a raster view is not recommended | | ⬜ |
| **Projected View** | orthographic (aligned to its parent by the projection type of the standard) and isometric views from any existing view; inherits the parent's scale and style unless *from Base* is cleared | | 🟡 PV, or right after a base view: click beside the parent (orthographic, aligned, first / third angle from the standard) or diagonally (isometric); scale and style from the base unless cleared; Esc / right-click ends; waiting for a visual check |
| **Section View** | a section line drawn over a parent (multi-segment); View identifier, Scale, Label, Style; **Depth**: Full or Distance; **Include Slice** / **Slice All Parts**; **Method**: Projected or Aligned (non-square segments); hatching by material; Half / Offset / Aligned sections from the line's shape | | 🟡 SV: click the parent, two points of the line (snaps to end points / centres, horizontal / vertical within 4°), then the side to look from; full depth, straight line only; the cut faces are found on the display mesh (crossings chained into loops), hatched 45° / 3 mm, the material on the viewer's side removed, hidden lines behind the cut face hidden; the view stays aligned square to the line; the section line on the parent: chain line, thick ends, arrows, letters; label SECTION A-A + scale; Depth Distance, Include Slice, Aligned method, half / offset sections, material hatch patterns ⬜; waiting for a visual check; **owner's requirement 2026-10-28** (Inventor's model): the section line is a **sketch** of the parent view — drawn with the sketch tools and their snaps (end points, midpoints, centres, horizontal / vertical, on-curve), one or more segments; its segments and points can be dragged on the sheet (the section follows); Edit Section Line opens that sketch to change or add lines. 🟡 2026-10-28 step 1: the line's points snap as sketch points (end points, centres, the middle of a straight edge, horizontal / vertical to the other point); hovering a section line on its parent shows its grips (both ends, the middle); dragging an end moves it with the same snaps, dragging the line moves it whole; the section is computed again on release (one Undo step); visually checked (line moved above the holes, end snapped to a hole centre). 🟡 2026-10-28 step 2: **lines of several segments** (offset sections, Projected method): click the points, right-click / Enter / double-click ends the line, then place the view; the view looks square to the first segment; each segment cuts along its own plane square to the parent (the removed region = convex cells, one per segment of the line extended at its ends, bounded by the bisectors at its bends), the cut faces of all segments projected into the view and hatched, jog faces edge-on; grips on every point and segment middle; on the parent: chain line, thick ends and thick marks at each bend; visually checked (C-C through two holes, its jog dragged; format revision 39). 🟡 2026-10-28 step 3: **Method** in the section view's Properties (Projected / Aligned; Aligned only for a line of several segments): Aligned turns the material behind each later segment about its bend (the boundary between segments is the bisector of the bend) into the first segment's plane, so an inclined segment's cut face shows its true length; then one plane cuts; old builds read such a view as Projected; visually checked (a vertical segment to the centre hole, then 45° to the bottom edge: the inclined cut face 27 mm long instead of its 19 mm projection). Constraints that keep the points on the model's geometry when it changes, and dimensions in the line's sketch ⬜ |
| **Detail View** | dialog: identifier, scale, label, style (Show / Remove hidden lines / Shaded; default = the parent's), **Fence Shape** Circular / Rectangular (same in parent and detail), **Cutout Shape** Jagged (default) / Smooth; with Smooth: Display Full Detail Boundary, and with it Display Connection Line (vertices can be added / moved / deleted; gone when the view moves to another sheet); Attach Fence. Clicks: centre, outer boundary, place. The fence is *not* attached to the parent's geometry by default — Attach (menu) to a vertex keeps it there across model updates; Attach on empty space detaches | | 🟡 DV: click the parent (or start with it selected), the centre (snaps), the size, then the place; circular fence, smooth cutout with a full boundary, letter at the fence on the parent, scale = the preset ≥ 2 × the parent's, label DETAIL A + scale; rectangular fence, jagged cutout, connection line, Attach ⬜; waiting for a visual check |
| **Auxiliary View** | pick a parent, set scale / style / label, pick an edge or line of it; the view slides only square to or along that edge; for a line outside the geometry use a Section View | | 🟡 AV: click a straight edge of a view, then place the view square to it (aligned; the line keeps its direction on the sheet); stored as the edge's ends at creation (does not follow a later change of that edge yet); waiting for a visual check |
| **Break** | shortens a long part: Style Rectangular / Structural, Orientation horizontal / vertical / along picked geometry (fixed after creation), Display, Gap, Symbols (1–3, Structural), Propagate to parent view, slider for the count / amplitude of the break edge; dimensions across a break show the true length with a break symbol, dimensions ending in the removed part are hidden; Edit Break, drag the break lines | | ⬜ |
| **Break Out** | a local cut inside a closed sketch profile to a depth | | ⬜ |
| **Slice** | a zero-depth section by a sketch line | | ⬜ |
| **Crop** | clips a view by a sketch / rectangle | | ⬜ |
| **Overlay** | other positional representations drawn in phantom lines | ⛔ positional reps | ⬜ |
| Draft view | a view with only a sketch | | ⬜ |
| Move / align / rotate views; edit view; delete with dependents | | | 🟡 drag a view (aligned children follow, an aligned child slides on its axis), Edit View (double-click, browser, menu), Delete (with the views projected from it, one Undo step), position in Properties; rotate ⬜; waiting for a visual check |
| Associativity | the model changes → views recompute; Defer Update for large drawings | | 🟡 a view's lines are cached by the model's body signature: a model change recomputes them (worker thread); linked part files are read again when their time changes; Defer Update ⬜ |

### 3.3 Annotations (4.3)

| Option | Inventor behaviour | Restrictions | Our status |
|---|---|---|---|
| General Dimension | linear, aligned, angular, radius, diameter between view geometry; text overrides, tolerance, precision | | 🟡 Dimension (D): an edge gives its length, a circle its diameter (an arc its radius), two points (end points, centres) their distance; horizontal / vertical / aligned follows where the text is put (above / beside / corner); ISO 129 layout (extension lines 1 mm off, 2 mm over, filled arrows, text above, readable from the bottom or the right), values in model mm with 2 decimals, zeros dropped; the text is dragged afterwards; tolerances and text overrides: Edit Dimension (4.7). 🟡 2026-10-28 (owner's report: the text could not be moved): a press on the **text** slides it along the dimension line (past an extension line the line runs on under it; near the middle it snaps back) and moves the line with it, a press on the **line** moves only the line; a click on an **arrowhead** of the selected dimension flips it inside / outside (each end apart; Auto = inside when both fit, else outside with a tail); Properties: Dimension Type (Diameter ↔ Radius, Horizontal / Vertical / Aligned), Arrowhead 1 / 2 (Auto / Inside / Outside), Center Text. **Angular**: two edges that are not parallel give their angle — an arc about the crossing of their lines through the text, the text's place picks the sector (acute, obtuse or the other side), extension lines out to the arc, arrowheads along it, `90°` in the angular precision (format revision 38). Angle through three points, arc length, chord, quadrant / tangent picks, linear diameter ⬜; waiting for a visual check |
| Baseline / Ordinate / Chain dimension sets | dimensions of many points from one origin or end to end, separate or kept as a set | | 🟡 see spec 4.7 (Baseline / Ordinate / Chain rows) |
| Retrieve Model Annotations | brings in the model's sketch and feature dimensions that lie in the view plane | | ⬜ |
| Hole and Thread Notes | from the Hole feature (`HoleSpec::note`) | 🟡 HN: click a hole's circle (a circle on the axis of a Hole feature's hole), then place the note; the text comes from the hole (Ø, depth / THRU, counterbore / countersink, thread) at every redraw, led by the quantity of every identical hole on its plane from any Hole feature, pattern or mirror (`2x Ø6.6 THRU`); symbols written as text (DEEP, CBORE, CSK) until the drawing font has them |
| Centre Mark, Centerline, Centerline Bisector, Centered Pattern; Automated Centerlines | | | 🟡 Center Mark (CM) on circles / arcs seen face-on, Centerline (CL) through two circle centres, both in centre-line style (long dash, dot) passing the circle by 2 mm; Bisector, Centered Pattern, automated centrelines ⬜; waiting for a visual check |
| Text, Leader Text | text with iProperty / parameter fields | | 🟡 Text (T): a note with several lines and a height (Format Text window), double-click edits it, drag moves it; Leader Text (LE): the arrow's tip (snaps to end points / centres), then the text's place, same window; property fields and attaching the leader to geometry ⬜; waiting for a visual check |
| Symbols | Surface Texture, Welding, Feature Control Frame (GD&T), Datum Identifier, Datum Target | | ⬜ |
| Balloons / Auto Balloon | item balloons from a BOM view: Around / Horizontal / Vertical placement, offset, first instance only | ⛔ BOM (3.6, done first level) | ⬜ |
| Orphaned annotations | an annotation whose geometry is gone is shown marked, never removed silently | | 🟡 annotations reference (view, edge name); a missing edge leaves the annotation in the drawing, marked with a red "?" where it was; deleting a view deletes its annotations (as in Inventor); waiting for a visual check |

### 3.4 Tables (4.4)

| Table | Inventor behaviour | Our status |
|---|---|---|
| **Parts List** | from a BOM view (Structured / Parts Only), its columns, sorting, splitting, wrapping | ⬜ (from `assembly::bom`) |
| Revision Table | rows of revisions, zones | ⬜ |
| Hole Table | holes of a view with coordinates from a datum; by view / by feature / selected | ⬜ (data: `PartDocument::holes`) |
| General Table | free cells, or from a CSV / Excel file | ⬜ |
| Bend Table | bends of a flat pattern view (direction, angle, radius, K-factor) | ⛔ sheet metal (5.6) |

### 3.5 Output (4.5)

| Output | Inventor behaviour | Our status |
|---|---|---|
| Print | sheets to a printer, scale 1:1 or fit | ⬜ |
| PDF | Current Sheet / All Sheets / Sheets in Range (From–To), Print Excluded Sheets, All Colors Black (images and shaded views stay coloured), Remove Object Line Weights, Vector Resolution (DPI of raster parts), Display Published File in Viewer | 🟡 File › Export › PDF... (or PDF in the command search): PDF Drawing Options (Current Sheet / All Sheets / Sheets in Range, Print Excluded Sheets, Remove Object Line Weights, Display Published File; All Colors Black is always true until colours come; Vector Resolution matters with raster views only) then the file name; one page per sheet at paper size, ISO line weights, hidden lines dashed, centre lines dash-dot, texts in an embedded subset of the UI font (Ubuntu Light: Latin + Cyrillic; final font choice open); waiting for a visual check |
| DXF / DWG | per sheet, layers per object type | ⬜ (SolidCraft's DXF writer, `crates/io/src/dxf.rs`) |
| SVG | (ours) | ⬜ |

## 4. Browser and editing behaviour

- The drawing browser: Drawing Resources, Sheets › views (each view: its model, dependent views below
  it), tables. Double-click edits a view; right-click: Edit View, Open the model, Delete, Align View.
- Annotations belong to a view or the sheet; moving a view moves its annotations.

## 5. Edge cases and errors

- A model file that is missing: views keep their last geometry, drawn marked; Resolve Link.
- A view whose source geometry vanished (a feature deleted): the dependent section / detail views and
  annotations are kept and marked.
- Very large assemblies: Raster View first, precise when needed.

## 6. Implementation (research)

### 6.1 The hard part: hidden line removal (HLR)

Nothing to adapt: SolidCraft has no drawings (only a DXF writer). Our plan:

1. **Edges to draw** of each body: every B-rep edge (exact curves: lines, circles, B-splines) except smooth
   seams (`kernel::smooth`) and, when Tangent Edges is off, edges between tangent faces; plus
   **silhouettes** of curved faces for the view direction (cylinder / cone: the generator lines where the
   normal is square to the view; sphere / torus / B-spline faces: traced on the display mesh where the
   normal's dot with the view changes sign).
2. **Visibility**: each curve is sampled; a sample is visible if no face lies between it and the eye —
   tested against the faces' triangles with a BVH (ours, GUI-free; the picking code's ray–triangle test is reused) and
   a tolerance from the body size; visibility changes are refined by bisection, so the curve is cut
   exactly where it goes behind a face. Visible parts are drawn as exact curves (a circle stays a circle /
   ellipse arc in the projection), hidden parts in the hidden line style.
3. **Associativity**: every 2D curve keeps the persistent name of its edge (rule 2) — `(view, occurrence
   path, edge name)` — so dimensions re-attach after a model change, or are marked orphaned.
4. **Sections**: the body cut by the section half-space with our exact Boolean (2.13), the cut faces
   hatched by material (ISO 128 hatch patterns per material class), then the same HLR.
5. **Speed**: views are computed on a worker thread (`preview_async`-like), cached by the model's
   signature; Raster View = the existing GPU renderer to an image.

### 6.1a Implemented (D1, first part, 2026-10-26)

- `src/drawing/mod.rs`: `DrawingDocument` (standard, sheets, active sheet, view ids never reused),
  `Sheet`, `DrawingView` (`ViewKind::Base { model, orientation }` / `Projected { parent, dir }`),
  `ViewFrame` (right, up, toward), `ViewOrientation`, `ProjectDir`, `Projection`, scales; placing, moving
  (alignment), deleting with dependents, references for Pack / Link (`DocumentData::references`).
- `src/drawing/sheet.rs`: sheet sizes, the border with zones and centring marks, the title block as
  `Prim`s (lines and texts in sheet mm — the list the screen draws and PDF / DXF will write).
- `src/drawing/hlr.rs`: option A2 on the display mesh: model edges + silhouettes of curved faces (mesh
  edges between a front- and a back-facing triangle of one smooth surface), each cut into pieces of
  1/600 of the model size and classified by its middle against the front-facing triangles binned in a
  uniform grid on the view plane; pieces on a visible line are not drawn twice or hidden. Circles are still
  polylines here (exact curves and edge names come with dimensions, D3).
- GUI `src/gui/drawing.rs` (`DrwUi`): the sheet view (pan with the middle button, zoom at the cursor, Zoom
  All), views drawn with ISO weights (visible 0.5, hidden 0.35 dashed), the Drawing View and Edit Sheet
  dialogs, browser tree (Drawing Resources, sheets, views under their parents), Properties of a view or the
  sheet. Ribbon tabs Place Views and Annotate list the whole Inventor command set; the planned ones are
  greyed with their FEATURES section. Format revision 23 (`DocumentData::Drawing`).

### 6.1b Implemented (2026-10-26, second part)

- HLR keeps each piece's model edge (`ViewLines::visible_src` / `hidden_src`, `ViewLines::edges`:
  persistent name, projected end points, exact centre and radius of circles seen face-on), built from
  the B-rep's edges (`hlr::part_model`) instead of the display mesh's anonymous polylines.
- `drawing::annot`: `Annotation` (Center Mark, Centerline, Dimension, Text) on `Sheet::annotations`
  (additive field), `resolve` → `Prim`s + value, or `Orphan`.
- `drawing::render::sheet_list`: the whole sheet as one display list; `drawing::pdf::write` (krilla).
- GUI `gui/drawing_annot.rs`: the tools, geometry picking (points 9 px, edges 7 px, as in sketches),
  hover / selection / drag / Delete of annotations, the Text and PDF windows.

### 6.2 Data model

- `DocumentData::Drawing(DrawingDocument)` — a new variant: **format revision bump** + fixtures (rule 6).
- `DrawingDocument { sheets, resources (formats, borders, title blocks, symbols), standard }`;
  `Sheet { name, size, orientation, border, title_block, views, annotations, tables }`;
  `View { id, kind (Base / Projected{parent} / Section{parent, line, depth, slice} / Detail{parent, fence, scale} …),
  model: DocRef, orientation, scale, style, position, options }`.
- Annotations reference `(view id, geometry by persistent name)`.

### 6.3 Technology choices (Rust): options, pros and cons

**Summary — what we use and for what** (status 2026-10-26):

| Need | We use | Why | Status |
|---|---|---|---|
| Hidden lines of a view | our HLR on the kernel's display mesh (exact edges + silhouettes, visibility by rays into a grid of triangles) | fast, headless, keeps true curves; no GPL code to lean on | 🟡 first version (polyline edges) |
| Drawing the sheet on screen | egui `Painter` from one display list in sheet mm | already used for all overlays, no new dependency | 🟡 |
| Big views later | our own wgpu line pipeline (CADCraft's `gpu.rs` as the model) | only if a perf case needs it | ⬜ |
| PDF | `krilla` (MIT / Apache, font subsetting, PDF/A) | Cyrillic title blocks need embedded fonts | ⬜ D1 next |
| Text | one embedded TrueType font with Cyrillic (read by `skrifa`, already in the tree) | same widths on screen and paper | ⬜ font to choose |
| DXF | our own writer (grown from the sheet metal one) | deterministic, no dependency | ⬜ D5 |
| DWG | `acadrust` (MPL-2.0) in a separate module, only with the owner's yes | the only pure-Rust DWG writer; LibreDWG is GPL | ⬜ D5 |
| SVG | written by us from the display list | also the snapshot format of our tests | ⬜ |
| Hatching, ellipses | our own clipping; `kurbo` (in the tree) for Bézier conversion | small, exact | ⬜ D2 |
| Parallel work | worker threads per view (done), `rayon` per edge batch later | the UI never waits | 🟡 |
| Annotations, title blocks, symbols | our `Sketch` module on the sheet plane | the solver and tools exist | ⬜ D3 |

What the tree already has (checked in `Cargo.lock`, 2026-10-26): `rayon` (through the kernel), `kurbo`,
`skrifa`, `ttf-parser` and `vello_cpu` (through `epaint` 0.36, egui's font and shape rasteriser). The
project is `MIT OR Apache-2.0`: every new dependency must be MIT / Apache / BSD / zlib (MPL-2.0 only
after the owner agrees; never GPL / LGPL). Rule: **the drawing model and its computation are GUI-free**
(`src/drawing/`, like `assembly`), so the API / MCP can make and export drawings headless.

#### a. Hidden line removal (the core)

| Option | How | Pros | Cons |
|---|---|---|---|
| A1 Exact analytic HLR (Appel's quantitative invisibility, Elber–Cohen on exact surfaces) | curve–surface intersections of every edge / silhouette with every face along the view direction | exact; the classic CAD approach | months of work; needs robust curve / surface projections and intersections that truck does not give (no periodic faces, fragile B-spline intersections); no MIT code to adapt; slow on thousands of faces |
| **A2 Hybrid: exact curves, mesh visibility (recommended)** | exact edges (lines, circles, B-splines) + silhouettes; each curve sampled, each sample tested by a ray against the display triangles in a **BVH**, visibility changes refined by bisection; visible / hidden *intervals of the exact curve* go out | reuses the kernel's display mesh and our ray–triangle test; the output keeps true circles / arcs (dimensions and centre marks recognise them); parallel per view and per edge (`rayon`); a few weeks of work | the visible / hidden switch point is as precise as the mesh (kernel display tolerance ~0.01 mm, far below a 0.25 mm line on paper, but a dimension snapped to it is mesh-precise); silhouettes of free-form faces are traced on the mesh and must be fitted (polyline → spline); faces seen edge-on need an epsilon from the body size |
| A3 GPU depth / ID buffer | render the view into a depth buffer (our wgpu renderer), test samples against it | very fast (large assemblies); the same image *is* Inventor's Raster View | pixel precision (aliasing where faces are seen edge-on); needs the GPU, so not headless and not in tests; read-back sync |
| A4 Polygon HLR of the mesh (triangle feature edges) | classic HLR of triangle edges | simplest | circles become polylines: no radius / diameter dimensions, no centre marks, ugly DXF; every facet edge must be filtered |

Proposal: **A2 for precise views**, **A3 later as Raster View** (large assemblies), A1 only if a future
kernel offers exact HLR (solidcore, `docs/KERNEL_API.md`). The BVH is ours (~200 lines, median or SAH split
over the triangles): `PickCache` in the GUI is a flat list and GUI-free code cannot use it; `parry3d` /
`bvh` would add a large dependency for one structure.

#### b. Drawing the sheet in the app

| Option | Pros | Cons |
|---|---|---|
| **B1 egui `Painter` (epaint) with cached meshes (recommended first)** | already draws every overlay (`gui/overlay.rs`): dashes, text, hit tests in the same coordinates; no new deps; each view tessellated once per zoom level into an `epaint::Mesh` and reused | CPU tessellation: 100k+ segments (big assembly views) can cost frames; ISO line types (dash patterns in paper mm) are generated by us |
| B2 our own wgpu line pipeline in `renderer.rs` (instanced segments, width and dash pattern in the shader, paper units) | fast at any size; exact line weights at any zoom | new shaders and picking; about a week; GPU only |
| B3 `vello` (GPU 2D renderer) | best 2D quality, curves without tessellation | must use the same `wgpu` version as eframe (they move apart often: upgrade lock-step); compute shaders; large dependency |

Proposal: a renderer-independent **display list** (`drawing::Draw2D`: polylines, arcs, ellipse arcs,
splines, text runs, hatch fills, each with a line style in paper mm) made by the model, painted with B1;
B2 only when a perf case shows the need. The same list feeds PDF / DXF / SVG, so screen and paper never
differ.

#### c. Text and fonts

| Option | Pros | Cons |
|---|---|---|
| **C1 one embedded TrueType font with Latin + Cyrillic (recommended)**, an open ISO 3098-like sans (candidates to check: DejaVu Sans, Liberation Sans; *osifont* is GPL with a font exception and needs the owner's decision), read with `skrifa` / `ttf-parser` (already in the tree) | the same glyph widths on screen, in PDF and in our layout (text boxes, title block cells, balloons); Cyrillic for Bulgarian title blocks | the font in the binary (~0.5–1 MB); the font's own license must be checked |
| C2 single-stroke (Hershey-like) font | plotter / DXF / engraving friendly, tiny | looks old; the public-domain sets have no Cyrillic |
| C3 PDF base-14 fonts (Helvetica) only | no embedding | **no Cyrillic**: not acceptable |

#### d. PDF

| Option | License | Pros | Cons |
|---|---|---|---|
| **D1 `krilla`** (by a typst developer, built on `pdf-writer`) | MIT / Apache | high-level strokes, fills, glyphs, images; **font subsetting** (TTF / CFF); PDF/A-1…4 modes (archived drawings); used in production by typst | a new dependency tree (shares `skrifa`); no text layout (we lay text out ourselves anyway) |
| D2 `pdf-writer` | MIT / Apache | tiny, low level, fast | font embedding / subsetting (`subsetter`) and content streams by hand |
| D3 `printpdf` | MIT | simple API | its API changed a lot between versions; less control of font subsets |
| D4 our own writer (like the DXF ones) | — | no deps | fonts, Unicode, compression by hand: weeks for a worse result |

Proposal: **krilla**. Vector lines in paper mm, weights from the standard; All Colors Black and Remove
Object Line Weights are display-list filters; shaded / raster views go in as images.

#### e. DXF / DWG / SVG / printing

| Item | Options | Proposal |
|---|---|---|
| DXF | our writer (sheet metal `part/sheet_metal.rs::dxf`; SolidCraft's `crates/io/src/dxf.rs`, R12 style, deterministic) or the `dxf` crate (ixmilia: read + write, many entities and versions) | **our writer**, grown into `drawing::dxf`: layers per object type, LTYPE table, LINE / ARC / ELLIPSE / SPLINE / TEXT / MTEXT / HATCH; dimensions exploded into blocks (DIMENSION entities are version-sensitive). The `dxf` crate only if we later *import* DXF title blocks (license checked then) |
| DWG | `acadrust` (pure Rust, read / write R13–R2018, **MPL-2.0**: file-level copyleft, fine as an unmodified dependency, but the owner decides); LibreDWG (**GPL**, never) | **not before D5**; DXF covers exchange |
| SVG | written by hand from the display list | yes, and it is our **snapshot test** format (a drawing's SVG compared in `tests/`, deterministic like the file format) |
| Print | no cross-platform printing crate in Rust | write a PDF and hand it to the system (Windows: the shell's *print* verb or the default viewer); a native print dialog later |

#### f. Hatching, 2D geometry, parallel work

- Section hatching is ours: pattern lines (angle, spacing, dashes of the hatch style) clipped against the
  cut faces' 2D outline with holes (scan-line intersections; the sketch module already finds regions).
  `i_overlay` / `geo` (MIT / Apache) only if Break Out / Crop need 2D Booleans of hatches.
- A circle seen at an angle is an exact ellipse arc in the display list (PDF / SVG as Béziers, DXF as
  `ELLIPSE`); `kurbo` (already in the tree) converts curves to Béziers.
- `rayon` (in the tree) per view and per edge batch, on a worker thread like `preview_async`; views cached
  by `(model signature, view definition)` as the regeneration cache does.

#### g0. CADCraft: a Rust 2D CAD to adapt from (MIT OR Apache-2.0)

[CADCraft](https://github.com/storytold/cadcraft) (ArtCraft team, v0.13.0, pre-alpha, AutoCAD-2027-like 2D
drafting; read 2026-10-26) solved most of the 2D side with the stack proposed above: egui / eframe 0.37,
a **display list** (`crates/render`: linetypes, hatches, text, dimensions, GD&T, paper sizes, pens) drawn
by its own **wgpu** pipeline (`crates/ui-egui/src/gpu.rs`: one upload per document revision, hairlines as
`LineList`, wide lines as instanced screen-space quads, f32 positions relative to a moving origin, CPU
fallback with `tiny-skia`), **krilla** for PDF (`crates/io/src/pdf*`), an own DXF tag reader / writer
(`crates/dxf`), `acadrust` (MPL-2.0) isolated in `crates/dwg`, an original single-stroke font "CADCraft
Stroke" (CC0, `crates/fonts/src/stroke.rs`) plus TrueType through `skrifa` and shaping with `harfrust`.

| What to adapt (with an `ATTRIBUTION.md` row) | Where in CADCraft | For stage |
|---|---|---|
| GPU canvas for big views (option B2) | `crates/ui-egui/src/gpu.rs` | D1 / when B1 is slow |
| Linetypes (dash patterns along polylines, scale) | `crates/render/src/linetype.rs` | D1 |
| Paper sizes, sheets, pens / plot styles | `crates/render/src/{paper,pen,plotstyle}.rs` | D1, D5 |
| PDF export of a display list through krilla | `crates/io/src/pdf.rs`, `pdf/` | D1 |
| Hatch patterns and region fill with islands | `crates/render/src/{hatch,fill}.rs` | D2 |
| Dimension geometry (arrows, text placement), tolerances, GD&T frames | `crates/render/src/{dim,tolerance}.rs` | D3, D6 |
| Text layout (TEXT / MTEXT, alignment, wrap), stroke font, font subsetting | `crates/fonts` | D1–D3 |
| Tables | `crates/render/src/table.rs` | D4 |
| DXF writer (layers, LTYPE, entities) | `crates/dxf`, `crates/io/src/dxf_write.rs` | D5 |
| DWG through `acadrust` in a separate crate | `crates/dwg` | D5 (owner's decision) |

Limits: it has **no 3D model** — hidden line removal, projections, sections and associativity to edges
stay ours; its data model is AutoCAD's (entities, layers, ACI colours, handles), so we take functions,
not modules; it is on egui 0.37 (we are on 0.36); the stroke font's Cyrillic coverage is not checked yet.

#### g. Annotations reuse the sketch module

Title blocks, borders, sketched symbols, Draft views, section / detail / break lines and view sketches are
**our `Sketch`** (solver, dimensions, constraints, the same tools) on the sheet plane; a drawing sketch can
project view curves by their persistent names (like `GeometryLink`). Drawing dimensions reuse the layout
of `overlay::draw_dimension`, moved to a GUI-free place so that PDF / DXF draw the same arrows. This is the
largest saving of the whole module.

#### h. File

`DocumentData::Drawing(DrawingDocument)` in the project container, or a separate file (the owner's queued
request: separate formats for part / assembly / drawing; Inventor's model: the drawing is its own file
and *links* model files, as assemblies already do through `LibCache`). A new enum variant is a format
revision bump + compat fixtures (rule 6). View results (curves) are **not stored**, they are recomputed
and cached, except a last-good copy for a missing model file (§5).

### 6.4 Proposed stages (to agree with the owner)

| Stage | Content |
|---|---|
| D1 | Drawing document, one ISO sheet (A4–A0, border, simple title block from iProperties), Base + Projected views of a part with exact HLR (visible / hidden / tangent lines), move views, PDF export |
| D2 | Section (full / half / offset, Depth, hatching), Detail, Auxiliary; view labels; associativity on model change |
| D3 | Annotations: general dimensions (linear, radius, diameter, angle), centre marks / centrelines, text and leaders, hole notes, Retrieve Model Dimensions; orphan marking |
| D4 | Assemblies in views; Parts List + Balloons / Auto Balloon from the BOM; Revision and Hole tables |
| D5 | Drawing resources editors (sheet formats, title blocks, symbols as sketches), standards ANSI / DIN, DXF / DWG export, printing |
| D6 | Break, Break Out, Slice, Crop, GD&T / surface / welding symbols, sheet metal flat views with Bend Table (⛔ 5.6) |

## 7. Sources

Inventor help: *Drawing View Dialog Reference* `GUID-69BAAC7A-A8EB-48FE-9E57-6F6E2D56FED6`, *To Create a
Base View and Projected Views* `GUID-67CAF102-A3FF-41D3-B842-194A089287FE`, *Section View dialog box*
`GUID-869670D9-20F1-4529-A6E9-D1C4A668F1A4`, *To Create and Edit Section Views*
`GUID-B3D7ED2B-06D4-479E-81F0-D2A8F494FFF9`, *About Section Views* `GUID-3E5A89C1-5560-40FF-8565-AE607DAFEB6D`,
*Auto Balloon Reference* `GUID-85EA4CA6-652C-49DC-915B-B9ECAD077CC0`, *Add balloons to drawing views*
`GUID-491D178A-5C7A-485F-902A-46B1A02D1D09`, *About Drawing View Fundamentals*
`GUID-A546FF1D-8703-4F3E-91A0-7AA1A35CC380`, *To Create and Edit Detail Views*
`GUID-4747000B-AEF1-4E4C-88EC-0DC9115E2215`, *To Work with Sheets and Sheet Formats*
`GUID-F99097C7-0BB4-4B8E-A31E-91C246F93D4E`, *Style and Standard Editor - Standard Reference*
`GUID-B37BB51A-FC14-4D78-BDE0-D3DC8FA9C9DF`, *PDF Drawing Reference* `GUID-77781860-2DEB-43B3-A597-0EF99CC1A31B`,
*To Create and Edit an Auxiliary View* `GUID-9B18C6E1-E42A-40A0-860A-9AFC2BBE9C38`, *To Work with Break View
*To Create a Base View and Projected Views* `GUID-67CAF102-A3FF-41D3-B842-194A089287FE` (ViewCube and its menu, corner scaling, projected view arrows), *Customize orientation of a drawing view* `GUID-2DBDD433-2FE1-442C-8BF3-E42D9186323E` (Custom View tools), What's New 2021 *Base and Projected Views Enhancements* `GUID-493B5FC5-3AAB-4DF8-8987-1D4C5B5CA7F2`, Operations* `GUID-BE8494ED-A3E2-4945-86B6-9908BCD0F184`, *Edit Property Fields, Prompted Texts, Attributes
Reference* `GUID-708CC945-36C4-4402-AB99-4403FFAB1878`, *To Work with Drawings for Large Assemblies*
`GUID-E9B71505-1938-447E-8DD3-5754593B6317` (raster views). Still to read before D1: New Drawing / templates,
the Edit Sheet dialog's fields, Object Defaults and line types; before D3: dimension styles.

Technology: A. Appel, *The notion of quantitative invisibility* (1967); G. Elber, E. Cohen, *Hidden curve
removal for free form surfaces* (SIGGRAPH 1990); crates `krilla` (github.com/LaurenzV/krilla), `pdf-writer`,
`dxf` (github.com/ixmilia/dxf-rs), `acadrust` (docs.rs/acadrust, MPL-2.0); LibreDWG is GPL (not usable).
