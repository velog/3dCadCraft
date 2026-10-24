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
| New Drawing | from a template (ISO / ANSI / DIN / JIS / GB / BSI); one sheet with a border and title block | | ⬜ |
| Sheets | several per drawing; New Sheet, Edit Sheet (name, size A4–A0 / ANSI A–E / custom, orientation), Delete, Copy; Exclude from count / printing | | ⬜ |
| Drawing Resources | Sheet Formats (sheet + views ready to place), Borders, Title Blocks, Sketched Symbols — editable as sketches with property fields | | ⬜ |
| Title block fields | text bound to iProperties of the drawing or the model of the first view (Part Number, Description, Designer, Date, Material, Mass, Scale, Sheet n of m) | | ⬜ |
| Standards (Styles) | Standard: projection type (first angle ISO / third angle ANSI), units, line types and weights per object (visible, hidden, centre, section…), text, dimension, hatch, balloon, parts list styles | | ⬜ ISO first, then ANSI |
| Active standard / Update / Purge | Style and Standard Editor as for parts | | ⬜ |

### 3.2 Views (4.2)

| View | Inventor behaviour | Restrictions | Our status |
|---|---|---|---|
| **Base View** | Drawing View dialog: File, Representation (model state / design view), Orientation (standard views, ViewCube, custom), Style (hidden lines shown / removed / shaded), Scale, Label, view identifier; then click to place; continue with projected views from the cursor | | ⬜ |
| Base view options | Raster View (fast, pixel-based) or precise; Hide small parts; Sheet Metal: folded / flat pattern, bend extents, punch centres; Display Options: tangent edges (shortened), thread features, interference edges, hatching, Align to Base, Definition in Base View, Orientation from Base, Cut Inheritance, standard parts hidden lines / sectioning; Recovery: All Model Dimensions, work features as reference lines | | ⬜ |
| **Projected View** | orthographic (aligned to its parent by the projection method) and isometric views from a parent | | ⬜ |
| **Section View** | a section line drawn over a parent (multi-segment); View identifier, Scale, Label, Style; **Depth**: Full or Distance; **Include Slice** / **Slice All Parts**; **Method**: Projected or Aligned (non-square segments); hatching by material; Half / Offset / Aligned sections from the line's shape | | ⬜ |
| **Detail View** | a circle or rectangle on a parent, enlarged; fence shape, cut-off shape (smooth / jagged), Full Detail Boundary, Display Connection Line *(to confirm)* | | ⬜ |
| **Auxiliary View** | square to a picked edge of a parent | | ⬜ |
| **Break** | shortens a long part between two break lines (style rectangular / structural, gap, symbols) | | ⬜ |
| **Break Out** | a local cut inside a closed sketch profile to a depth | | ⬜ |
| **Slice** | a zero-depth section by a sketch line | | ⬜ |
| **Crop** | clips a view by a sketch / rectangle | | ⬜ |
| **Overlay** | other positional representations drawn in phantom lines | ⛔ positional reps | ⬜ |
| Draft view | a view with only a sketch | | ⬜ |
| Move / align / rotate views; edit view; delete with dependents | | | ⬜ |
| Associativity | the model changes → views recompute; Defer Update for large drawings | | ⬜ |

### 3.3 Annotations (4.3)

| Option | Inventor behaviour | Restrictions | Our status |
|---|---|---|---|
| General Dimension | linear, aligned, angular, radius, diameter between view geometry; text overrides, tolerance, precision | | ⬜ |
| Baseline / Ordinate / Chain dimension sets | | | ⬜ |
| Retrieve Model Annotations | brings in the model's sketch and feature dimensions that lie in the view plane | | ⬜ |
| Hole and Thread Notes | from the Hole feature (`HoleSpec::note`) | | ⬜ (data exists, 2.3) |
| Centre Mark, Centerline, Centerline Bisector, Centered Pattern; Automated Centerlines | | | ⬜ |
| Text, Leader Text | text with iProperty / parameter fields | | ⬜ |
| Symbols | Surface Texture, Welding, Feature Control Frame (GD&T), Datum Identifier, Datum Target | | ⬜ |
| Balloons / Auto Balloon | item balloons from a BOM view: Around / Horizontal / Vertical placement, offset, first instance only | ⛔ BOM (3.6, done first level) | ⬜ |
| Orphaned annotations | an annotation whose geometry is gone is shown marked, never removed silently | | ⬜ |

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
| PDF | vector, all or chosen sheets, line weights, colours / black | ⬜ |
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
   tested against the faces' triangles with a BVH (the picking code already does ray–triangle tests) and
   a tolerance from the body size; visibility changes are refined by bisection, so the curve is cut
   exactly where it goes behind a face. Visible parts are drawn as exact curves (a circle stays a circle /
   ellipse arc in the projection), hidden parts in the hidden line style.
3. **Associativity**: every 2D curve keeps the persistent name of its edge (rule 2) — `(view, occurrence
   path, edge name)` — so dimensions re-attach after a model change, or are marked orphaned.
4. **Sections**: the body cut by the section half-space with our exact Boolean (2.13), the cut faces
   hatched by material (ISO 128 hatch patterns per material class), then the same HLR.
5. **Speed**: views are computed on a worker thread (`preview_async`-like), cached by the model's
   signature; Raster View = the existing GPU renderer to an image.

### 6.2 Data model

- `DocumentData::Drawing(DrawingDocument)` — a new variant: **format revision bump** + fixtures (rule 6).
- `DrawingDocument { sheets, resources (formats, borders, title blocks, symbols), standard }`;
  `Sheet { name, size, orientation, border, title_block, views, annotations, tables }`;
  `View { id, kind (Base / Projected{parent} / Section{parent, line, depth, slice} / Detail{parent, fence, scale} …),
  model: DocRef, orientation, scale, style, position, options }`.
- Annotations reference `(view id, geometry by persistent name)`.

### 6.3 Proposed stages (to agree with the owner)

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
`GUID-491D178A-5C7A-485F-902A-46B1A02D1D09`. Still to read before D1: New Drawing / templates, Sheet Format,
Title Block editing, Edit Sheet, Projected View, Detail View dialog, Drawing Standards (Styles), PDF export.
