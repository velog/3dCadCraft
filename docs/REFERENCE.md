# 3DCadCraft — user reference (GUI)

This reference describes the graphical interface **as it is in the code** (`src/gui/*.rs`) as of
2026-10-19. Command and option names are in English, as in Inventor. Where
`docs/UI.md` says otherwise, the code wins (the differences found while writing this were fixed in
UI.md on 2026-10-19; see the last section).

Contents:
1. [Mouse in the view](#1-mouse-in-the-view)
2. [Context menus](#2-context-menus-right-click)
3. [Selection](#3-selection)
4. [Sketch](#4-sketch)
5. [Keyboard shortcuts](#5-keyboard-shortcuts)
6. [Value fields](#6-value-fields)
7. [Manipulators and preview](#7-manipulators-and-preview)
8. [Settings](#8-settings)
9. [Ribbon and Quick Access](#9-ribbon-and-quick-access)
10. [Status bar](#10-status-bar)
11. [Differences from Inventor](#11-differences-from-inventor)
12. [Differences from docs/UI.md](#12-differences-from-docsuimd)

Window layout (1400×900 by default): at the top Quick Access + ribbon; on the left
**Model** (the browser, ~250 px, resizable); on the right **Properties** (~300 px); in the middle the 3D
view with the ViewCube at the top right and an X/Y/Z triad at the bottom right; at the bottom the status bar.

---

## 1. Mouse in the view

### 1.1 Navigation (orbit / pan / zoom)

The profile is changed in **Tools → Application Options → Navigation**. The default is
**Inventor**.

| Action | Profile **Inventor** | Profile **Classic (left = orbit)** |
|---|---|---|
| Orbit | **Shift + middle button** (drag); or hold **F4** + left | left button (drag), unless dragging a point, a manipulator or a pattern handle |
| Pan | **middle button** (drag); or hold **F2** + left | right or middle button (drag) |
| Zoom | wheel; or hold **F3** + left (up = zoom in) | wheel |
| Zoom to fit everything (Fit) | **double-click with the middle button** | double-click (with any button) or middle double-click |
| Home view | **F6** or the ViewCube's house | the same |

- The wheel zooms in/out about the **centre of rotation** (target), not about the cursor.
  The direction is reversed with *Reverse zoom direction*.
- F2/F3/F4 work only in the Inventor profile and take priority over dragging points.
- Orbit passes over the pole: the model rolls over and the view stays upside down; a click on the
  ViewCube or F6 returns an upright view (and resets the Roll rotation).
- A drag or the wheel stops a running ViewCube transition.
- In the Classic profile a left drag always orbits, unless it starts on a point
  (of a sketch), a manipulator or a pattern handle: dragging a whole curve, a dimension's text and the
  selection box **do not work** in Classic.

### 1.2 ViewCube (top right)

| Action | Result |
|---|---|
| Click on a face / edge / corner (26 directions, each face has 3×3 zones) | view from that direction, smooth 0.3 s transition |
| Click on the house (top left of the cube) | Home: isometric (yaw −60°, pitch 30°) or the one set with *Set Current View as Home*, + Fit |
| Arrows around the cube (only when looking straight at a face) | the neighbouring face in that direction |
| The two curved arrows (only when looking straight at a face) | rotate the image by ±90° (Roll) |
| Compass (the ring under the cube): click on N / E / S / W | look from that side, keeping the tilt (N = +Y, E = +X, S = −Y, W = −X) |
| Drag the cube | free Orbit |
| Drag the compass ring | rotate about Z (the ring follows the mouse) |
| Right-click on the cube | ViewCube menu (section 2.6) |

The cube is semi-transparent (50 %) when the mouse is far from it. A drag that starts on the cube
or the compass selects nothing in the model.

### 1.3 Idle view (part, no command, no sketch)

| Mouse | Action |
|---|---|
| Cursor over an edge / face | prehighlight (green) + a yellow label next to the cursor: `Edge <name> · L <length> mm` or `Face <name> · <feature> · <SolidN>` |
| Left click (priority *Faces and Edges*) | selects what is under the cursor in the order of 3.2 — work point, vertex, work axis, curve of a visible sketch, edge, face, work plane (blue); also selects the feature in the browser/Properties (for Box — the body); for an edge/face it shows the **suggestion bar** (section 3.5). Kinds excluded by the filter (3.1) are not picked |
| Left click on a work plane/axis/point | selects the work feature (in the browser and Properties). Work points and axes take priority over faces and edges |
| **Shift + right-click** | the **Selection** menu next to the cursor (section 3.3a); does not open the normal menu and does not start Pan (also with the Classic mouse) |
| Left click on empty space | clears the selection (the old one goes to *Previous Selection*) |
| **Shift**+click | adds to the selection (no suggestion bar) |
| **Ctrl**+click | toggles (adds or removes) |
| Double-click on a face/edge | **Select Tangencies**: adds the tangent faces / the chain of tangent edges |
| Drag from empty space (Inventor profile only) | selection box: **left to right** (solid blue frame) = only those fully inside (window); **right to left** (dashed green) = also those crossed (crossing); in the Select menu it can be always Window or always Crossing. It takes only the kinds enabled in the filter. With no key it replaces the selection, Shift adds, Ctrl toggles |
| Rest for ~0.7 s (within 3 px) over a place with several candidates | **Select Other** (section 3.3) |
| Drag a manipulator of a selected feature | changes its value (applied on release, section 7) |

### 1.4 Sketch in edit

| Mouse | **Select** tool | Drawing / modify tool |
|---|---|---|
| Left click on a curve | selects it; Shift toggles (adds/removes) | a step of the tool (section 4) |
| Left click on a constraint glyph / dimension text | selects it; Shift toggles | — (with the **Dimension** tool a click on a dimension = Edit Dimension) |
| Left click on empty space | clears the selection | a step of the tool |
| Double-click on a dimension's text | **Edit Dimension** in place | the same |
| Double-click on a curve | selects the whole connected chain (Shift: adds to the selection) | Spline: a double-click finishes the spline |
| Drag a point | moves it; the solver does the rest. Dropped on another point → **Coincident**; on a curve → a point on the curve (if the solution is possible) | — |
| Drag a curve (not from a point) | moves the whole curve | — |
| Drag a dimension's text | moves the text (not the geometry) | — |
| Drag a spline handle (small square) | sets/changes the handle; grey = inactive, dragging activates it | — |
| Drag from empty space (Inventor profile) | window/crossing box for curves; Shift adds, Ctrl toggles | — |
| Line: press on the end of a line/arc and drag (>10 px) | — | arc: **tangent** if the drag starts along the continuation (up to 45°), otherwise **perpendicular**; the chain continues from the end of the arc |
| Drag a pattern handle (Edit Pattern) | spacing (arrow), angle (small circle), count (small circle with "+") | — |
| Right-click | context menu (section 2.3) | context menu with **Done** (end of the chain / create the spline) and **Cancel (Esc)** |

In Relax Mode, dragging a point may change dimensions; the changed ones become driven.

### 1.5 Feature commands

| Command | Left click | Drag | Other |
|---|---|---|---|
| **Start 2D Sketch** (choosing a plane) | a model face, a work plane or an origin plane (XY/XZ/YZ) → the sketch is created | — | faces win over origin planes; Esc cancels |
| **Extrude / Revolve** | toggles the region under the cursor (the smallest one containing it) in the current sketch; a region of another visible sketch switches the sketch; Extrude: an open line/arc (one that does not bound a region) joins the open profile (Match Shape), a second click removes it; a double-click on a dimension of the profile sketch edits it in place; right-click: Continue (Revolve, to the axis), OK, Cancel | from empty space (Inventor profile): a box for profiles, window/crossing, Shift adds, Ctrl toggles, no key replaces; dragging an arrow/arc = manipulator | Revolve: while the **Axis** selector is active, a click picks a sketch line (within 8 px); X/Y/Z buttons for the origin axes. *From*/*To*: click the field, then a face or a vertex (a vertex within 12 px wins) |
| **Fillet / Chamfer** | Select *Edge*: an edge (a second click removes it; *Automatic* takes the tangent chain). *Loop*: a face → its loop nearest to the click. *Feature*: a face → the edges of its feature against the others. *Solid*: buttons in the panel. Full Round: the middle face. Variable: a straight edge | — | pre-selected edges join the set |
| **Shell** | a face → to remove (a second click returns it; *Automatic* also takes the tangent ones); the thickness arrow can be dragged | — | pre-selected faces join the list; right-click → the command's menu |
| **Combine** | *Base* — a body (click on a face); then *Toolbodies* — bodies (a second click removes a body) | — | a body selected before the command becomes the base; right-click → menu: OK, Apply, Continue, Back, Cancel + Join / Cut / Intersect |
| **Split** | *Split Tool* — a work or origin plane (the origin planes are shown while the selector is active), a curve of a visible sketch or a face of a surface body; from the browser: Origin → XY / XZ / YZ, a work plane row, a sketch row, Surface Bodies → a surface (a sketch — also from the list in the panel); *Faces* — a face (a second click removes it); *Solid* — a body (also a row in Solid Bodies) | — | after a single-pick selector it moves on to the next one by itself; right-click → menu + the methods and Keep Flip side |
| **Draft (Face Draft)** | depending on the active selector (only what it accepts is highlighted): *Pull Direction* — a work axis, a straight edge, a planar face, a work or origin plane (a faint arrow shows the candidate); *Fixed Plane* / *Parting Tool* — a plane (a sketch — from the list in the dialog); *Faces* — a face (with Fixed Edge the click decides the fixed edge, purple); *Static Edges* — an edge (a second click removes it) | — | after a single-pick selector it moves on to the next empty one by itself; right-click → the command's menu |
| **Rectangular / Circular Pattern, Mirror** (3D) | selector *Features*: a face → its feature (a second click removes it; a click on a browser row too); *Solids*: a face → its body. *Direction 1/2*: a straight edge or a plane (face, work, origin); *Axis*: a straight edge; *Mirror Plane*: a plane | — | after the first picked object the selector moves on to the direction/axis/plane by itself; buttons for origin/work axes and planes in the dialog |
| **Work Plane / Axis / Point / UCS** | work axis/point and UCS elements (6/8 px) > a vertex or the midpoint of a straight edge (8 px) > a point/line/curve of a visible sketch > an edge (straight = line, otherwise a curved edge) > a curved analytic face (cylinder, cone, sphere, torus) > a planar face / work / origin plane; for Axis a faceted face of a Revolve = the Revolve's axis. A second click on the same thing removes it; an incompatible pick starts over | an offset plane's arrow, an arc for the angle | X/Y/Z/Center buttons in the dialog; a sketch point/line in a method with further inputs becomes a hidden work point/axis |
| **Pattern (sketch)** | fills the active selector (section 4.9) | — | Enter: next selector / OK |

In every command the middle button, F2/F3/F4, the wheel and the ViewCube work as usual.

---

## 2. Context menus (right-click)

### 2.1 View, no sketch (idle or during a command)

| Item | Action |
|---|---|
| **Repeat <command>** | repeats the last command; shown with the Select tool and without an open Extrude/Revolve or a command with selectors (with Fillet, Hole, Work etc. it is still shown — see 5.3) |
| **Appearance ▸** | only outside a sketch and with selected objects: *As Material* (removes the override), *Document Appearances*, the library by category — for the selected faces/features/bodies |
| **Projection ▸** Orthographic / Perspective / Perspective with Ortho Faces | projection type |
| **New Sketch** | a new sketch on the face (or work plane) under the cursor; otherwise grey text "No face under the cursor" |

There is no separate menu for a face, an edge or a body: **Edit Feature**, Fillet/Chamfer etc. are in
the suggestion bar after a left click (3.5).

### 2.2 View in a sketch — common items

Besides 2.1 (Repeat, Projection, New Sketch):

| Item | When | Action |
|---|---|---|
| **Done** | active tool ≠ Select | end of the line chain / create the spline / Geometry → next step in transforms / OK of a pattern if it is ready |
| **Cancel (Esc)** | active tool ≠ Select | like Esc |
| **Finish 2D Sketch** | always in a sketch | leaves the sketch |

### 2.3 In a sketch: on a dimension or a constraint glyph

Title: the constraint's name.

| Item | When | Action |
|---|---|---|
| **Edit Dimension...** | a dimension that is not driven | opens the field in place |
| **Driven Dimension** / **Make Driving** | a dimension | toggles driven; refuses if making it driving would over-constrain |
| **Flip Direction** | Horizontal/Vertical Distance, Point–Line Distance, Angle (not driven) | moves the geometry to the other side |
| **Reset Position** | moved text | returns the text to its default place |
| **Delete Constraint** | always | deletes the constraint |

### 2.4 In a sketch: on a curve

On a **projected** curve or point (Project Geometry): title = the source (`Sketch1 · Circle`),
a warning when the source is missing, **Break Link** (becomes plain geometry), **Replace Reference**
(click a new source of the same kind; the constraints stay), **Delete** (with the constraints on it).
While Project Geometry is on, the menu starts with **Done (Esc)**.

Title: Line / Circle / Arc / Spline.

| Item | When | Action |
|---|---|---|
| (spline) *Interpolation* / *Control vertex* | spline | kind |
| **Display Curvature** (check) | spline | curvature comb |
| **Insert Point** | interpolation spline | a new point at the place of the right-click |
| **Delete Point** | right-click near a spline point (<1.5 mm) | removes it |
| **Activate Handle** / **Deactivate Handle** | near a point of an interpolation spline | a handle (tangent) at the point |
| **Reset All Handles** | there are handles | removes them all |
| **Close Curve** (check) | interpolation spline | closes / opens |
| **Convert to CV** / **Convert to Interpolation** | spline | changes the kind |
| *Pattern* (+ a warning if the pattern is "sick") | a curve of a pattern | — |
| **Edit Pattern...** | a pattern without problems | opens Edit Pattern |
| **Suppress Element** / **Unsuppress Element** | a pattern copy | hides / restores the copy |
| **Offset Element...** | a copy of a rectangular pattern | window *Offset Element*: Along direction 1, Along direction 2 / across; OK / Reset / Cancel |
| **Delete Pattern** | a curve of a pattern | deletes the pattern (the copies) |
| **Break Pattern** | a curve of a pattern | the copies become independent geometry |
| **Construction** / **Centerline** / **Normal** | own (not projected) curve, outside a pattern | style |
| **Fix** | own curve | Fix of its points in place |
| **Delete Constraints** | outside a pattern | removes all constraints of the curve and its points |
| **Delete** | own curve | deletes it (and its unused points) |
| **Display Degrees of Freedom** / **Hide Degrees of Freedom** | outside a pattern | toggles the degree-of-freedom glyphs |

### 2.5 Browser (Model Browser)

| Node | Left click | Double-click | Right-click (menu) |
|---|---|---|---|
| Project | selects (Properties: Name, documents, libraries, ID) | — | — |
| Another document | selects (view only) | — | — |
| Active part (title) | expands/collapses | — | — |
| *Solid Bodies(n)* → body `SolidN` | selects the body (Properties: Box/Solid + validation) | — | **Material ▸** (materials by category, with density), **Appearance ▸** |
| *Surface Bodies(n)* | — | — | — |
| *Origin* → XY / XZ / YZ Plane | — | — | **New Sketch** |
| *Origin* → X/Y/Z Axis, Center Point | (label only) | — | — |
| Box | selects the body | — | **Appearance ▸**, the common items (below) |
| Extrude / Revolve, Hole, Sweep, Loft, Coil, Rib | selects | **Edit Feature** | **Edit Feature**, **Edit Sketch**, **Appearance ▸**, the common items |
| Fillet / Chamfer, Shell, Draft, Thread, Combine, Split, Rectangular / Circular Pattern, Mirror | selects | **Edit Feature** | **Edit Feature**, **Appearance ▸**, the common items |
| The view while Face Draft, Shell, Combine, Split or Loft is open | — | — | the command's menu: **OK**, **Apply (+)**, **Continue**, **Back**, **Cancel**, **Repeat …** and the options (kind, direction, Flip / Inside, Outside, Center, Sharp/Rounded) |
| Work plane / axis / point | selects; the check box = visibility | **Redefine** | **New Sketch** (plane only), the common items |
| Common items for a feature | | | **Rename**, **Suppress Features** / **Unsuppress Features**, **Move EOP Here**, **Delete** |
| Sketch | selects; the check box = visibility | **Edit Sketch** | **Edit Sketch** (or **Finish 2D Sketch** if it is being edited), **Share Sketch** (used, not shared), **Unshare** (shared, ≤1 feature), **Redefine**, **Look At**, **Hide**/**Show**, **Rename**, **Delete** |
| *End of Part* (red bar) | — (can be dragged) | — | **Move EOP to Top**, **Move EOP to End**, **Delete All Features Below EOP** |

While the *Features* selector is active in 3D Pattern/Mirror, a click on a feature row in the
browser adds/removes it (it does not select it). An orange label with "! " = a problem (the tooltip starts with
`Warning:`). Grey italic = below *End of Part* (not built); grey strike-through = suppressed
(Suppress). A click on a feature colours its faces in the view (blue), hovering — green.

- **Dragging a feature** onto another: it lands above it (upper half of the row) or below it
  (lower half). A forbidden move (above something it depends on, or across the marker) is refused with
  the reason in the status bar; if something becomes sick after the move, it is reverted.
- **Dragging *End of Part*** onto a feature: the marker lands above/below it. Esc aborts.
  New features and sketches are inserted at the marker.
- **Rename:** F2 (mouse over the browser), a slow second click on a selected row, or the menu;
  Enter/click elsewhere = done, Esc = cancel. Changes only the name, not the id.
- **Delete** of a feature that takes other things with it (its sketch, dependents) or breaks something
  opens *Delete Features*: check boxes **Consumed sketches and features** and **Dependent sketches
  and features** (both on), a list of what else disappears and what will become sick,
  **OK** / **Cancel** (Esc = Cancel).

### 2.6 ViewCube

| Item | Action |
|---|---|
| **Home** | Home view |
| Orthographic / Perspective / Perspective with Ortho Faces | projection |
| **Set Current View as Home** | the current direction becomes Home |
| **Reset Home** | (if set) returns to the isometric view |
| **Fit (F6 zooms Home)** | fits everything without changing the direction |

### 2.7 Status bar, ribbon, panels

No context menus. Ribbon commands with variants (Circle, Arc, Rectangle, Slot,
Spline, Fillet, Demo, Projection) open their menu with a **left** click.

---

## 3. Selection

### 3.1 Priority and filters (Quick Access → **Select: …**)

Specification: [spec/0.9a-selection-filters.md](spec/0.9a-selection-filters.md). By
default **everything** is pickable; filters exclude kinds. The choice is remembered between
launches (`settings.json` → `options.selection`). The title is `Select: <priority>`,
with `*` when some filter is off.

| Priority (Select Priority) | A click on a face/edge/vertex selects | Ctrl+A / Invert select |
|---|---|---|
| **Select Faces and Edges** (default) | the object itself (sketches and work features are selected too) | all faces (if faces are off — the edges, then the vertices) + the visible sketches and work features |
| **Select Features** | the feature that created it | all features + sketches and work features |
| **Select Bodies** | the body (Solid Bodies / Surface Bodies) | all bodies + sketches and work features |
| **Select Sketch Features** | like Faces and Edges, but the visible sketch under the cursor wins | like Faces and Edges |

Changing the priority clears the selection. The other items of the menu:

| Item | Action |
|---|---|
| **Filters ▸** | check boxes: Faces, Edges, Vertices, Features, Solid Bodies, Surface Bodies, Sketches, Sketch Entities, Work Planes, Work Axes, Work Points, Origin; **Select All Filters** turns them all back on |
| **Window / Crossing by Direction**, **Always Window**, **Always Crossing** | how the selection box decides (3D and sketch) |
| **Enable Prehighlight** (check, on) | green highlight under the cursor; the label next to the cursor is shown without it as well |
| **Select Loop** | the loop of the last selected edge (or the outer loop of the selected face) |
| **Select Tangencies** | adds the tangent ones to the last selected edge/face (also double-click) |
| **Select Invert** | everything from Select All that is not selected |
| **Select All (Ctrl+A)** | everything that passes the filter (see the table) |
| **Previous Selection** | restores the previous set (one is kept; swap) |
| **Clear Selection (Esc)** | clears the selection |

**In a command** only the intersection is pickable: what the active selector accepts **and** what is
enabled in the filter. If the filter excludes everything the selector accepts, the filter does not
apply, and the status bar says in orange `Selection filter ignored: this input takes
only …`. The Select menu is also available while the command is open.

The status bar shows `Selected: 3 faces, 1 edge…`. When the body changes
(regeneration, Undo), objects that disappeared drop out of the selection; edges always drop out.

### 3.2 Picking priorities

| Context | Order (the first wins) |
|---|---|
| Idle view (and Select Other) | work point (8 px) > vertex (8 px) > work axis (6 px) > curve of a visible sketch (8 px) > visible edge (6 px, those hidden behind a face do not count) > face > work plane; with *Select Sketch Features* the sketch comes first. Filtered kinds are skipped; a filtered face still hides what is behind it |
| Start 2D Sketch / Redefine | model face > work or origin plane (the nearest along the ray) |
| Sketch, drawing | point (12 px; it is shared) > midpoint of a line > curve (8 px) > free + inference |
| Sketch, geometry tools (constraints, dimension, Trim…) | point (6 px — a small circle is also caught by its curve) > curve (8 px) |
| Sketch, Select | constraint text/glyph > point (12 px) > curve (8 px) |
| Extrude From/To, Revolve To | model vertex (12 px) > work point > face > work / origin plane |
| Primitives (Box, Cylinder, Sphere, Torus) | first a plane (face, origin or work), then clicks in it; the first click snaps to the sketch origin (the point radius) |
| Work Plane/Axis/Point | see 1.5 |
| Face Draft, Pull | straight edge > plane |

### 3.3 Select Other

With any priority, when the cursor rests ~0.7 s over a place with more than one candidate (or
from the Shift + right-click menu → **Select Other**), a small bar `< >  k / n  <description>` is shown:

- **←/↑** or `<` — previous; **→/↓** or `>` — next; **Shift + wheel** — cycles
  (the wheel does not zoom then);
- the current candidate is highlighted; **a click on the name** selects it (and its feature);
- moving the cursor more than 60 px from the place, or Esc, closes it.

Candidates: all kinds enabled in the filter under the cursor, in the order of 3.2 — work
features, vertices, sketches, the edges within 6 px (hidden ones too), the faces along the ray
(nearest first) and the faces on both sides of the edges. With *Features* / *Bodies*
the candidates are their features / bodies (without repetitions).

### 3.3a Selection menu (Shift + right-click)

It opens next to the cursor; Esc or a click outside it closes it. Its content follows the context:

| Context | Items |
|---|---|
| Idle part | Select Other, Select Loop, Select Tangencies, Select Invert, Select All, Previous Selection, Clear Selection; Window/Crossing; **Filters ▸** (all); the priorities; Enable Prehighlight |
| Open command | Select Other (⬜ not active yet); Window/Crossing; **Filters ▸** only with the kinds the active selector accepts; Enable Prehighlight |
| Sketch | Select All, Select Invert, Clear Selection (the sketch's curves); Window/Crossing; Enable Prehighlight |

### 3.4 Sketch

See 1.4. Ctrl+A in a sketch selects all curves. Delete deletes the selected curves (and their points
that nothing else uses) and the selected constraints. A pattern copy cannot be
deleted on its own (Suppress Element / Delete Pattern); deleting the original deletes the
pattern. Automatically projected edges of the face are not deleted; geometry from **Project Geometry**
(from sketches and circular edges) is deleted together with the constraints on it. A row in the *Constraints* list (Properties):
hover = highlights the geometry, click = selection.

### 3.5 Suggestion bar (after a click without Shift/Ctrl)

| Selected | Buttons |
|---|---|
| Face | **New Sketch on this face**, **Extrude this face** (Press/Pull: a sketch on the face, profile = the region under the click, Join outwards), **Edit Feature** (a face of the base Box: Properties shows its sizes), **Edit Sketch**, **Look At this face** |
| Edge | **Fillet this edge**, **Chamfer this edge**, **Edit Feature** |

---

## 4. Sketch

### 4.1 Entering and leaving

- **Start 2D Sketch** (S): the origin planes appear, the faces light up; click on a
  face, a work plane or an origin plane. The sketch is created (with a projected centre and the
  face's edges according to the options), the view turns towards it (if enabled) and
  the Sketch tab opens.
- Editing: double-click in the browser, **Edit Sketch** (browser, Properties, the bar on a face).
- Leaving: **Finish Sketch** (ribbon), **Finish 2D Sketch** (right-click), a button in Properties;
  also any feature command (E, Extrude, Fillet…).
- Sketch colour / state: green = fully defined, blue = under-defined,
  orange = redundant constraints, red = conflict. Individual fully constrained curves are
  dark green.

### 4.2 Drawing tools (click order)

| Tool | Clicks | Notes |
|---|---|---|
| **Line** (L) | start, next, next… | the chain shares points; end: Esc, Enter or right-click → Done. Dragging from the end of a curve → arc (1.4). A start on a circle/arc, if the line leaves almost tangentially (up to 8°) → Tangent. The cursor near the tangent point from the previous point (12 px) → glyph *tangent*: end on the curve + Tangent |
| **Center Point Circle** (C) | centre, point on the circle | a point on a line/curve → Tangent to it |
| **Tangent Circle** | three lines | inscribed circle, tangent to all three |
| **Three Point Arc** | end, end, point on the arc (side and radius) | |
| **Tangent Arc** | end of a line/arc, end of the arc | |
| **Center Point Arc** | centre, start (radius), end | the shorter arc in the direction of the clicks |
| **Two Point Rectangle** (R) | two opposite corners | |
| **Three Point Rectangle** | corner, end of the first side, adjacent side | |
| **Two Point Center Rectangle** | centre, corner | |
| **Three Point Center Rectangle** | centre, half side, adjacent side | |
| **Center to Center Slot** | arc centre, arc centre, width | |
| **Overall Slot** | end, end, width | |
| **Center Point Slot** | slot centre, arc centre, width | |
| **Three Point Arc Slot** | start, end, point on the arc, width | |
| **Center Point Arc Slot** | centre, start, end, width | |
| **Polygon** | centre, vertex (inscribed) or midpoint of a side (circumscribed) | Properties: *Sides* 3–120 (6), *Inscribed* / *Circumscribed* |
| **Interpolation Spline** | points it passes through | Enter / double-click / Done finishes (≥2 points) |
| **Control Vertex Spline** | control points | the same |
| **Point** | one click | |

A first click caught on a point or a curve gets Coincident / point on curve / Midpoint.
Format → **Construction** with nothing selected toggles the *construction* mode for new geometry
(`[construction]` is shown in the status bar).

### 4.3 Dynamic Input (fields next to the cursor)

| Tool, step | Fields |
|---|---|
| Line, Center Point Circle, Two Point / Center Rectangle, Point, Move, Copy, Rotate — first point | X, Y (coordinates in the sketch) |
| Line — second | Length, Angle |
| Center Point Circle — second | Diameter |
| Two Point / Two Point Center Rectangle — second | Width, Height |
| Move / Copy — second | Delta X, Delta Y |
| Rotate — third | Angle |

| Key | Action |
|---|---|
| digits, `.`, `,`, `-` | type into the active field (`,` becomes `.`) |
| Backspace | deletes a character |
| **Tab** / **Shift+Tab** | locks the input and moves to the next / previous field |
| **Enter** | locks and places the point (only if something is typed or locked) |
| **Esc** | clears what is typed and locked (a second Esc — the usual) |
| **↑** | the last value entered in this field |

Locked values (orange background) become dimensions: Length → Distance; an Angle that is a multiple of
90° → Horizontal/Vertical, otherwise Angle to the previous line of the chain (for the first line
it only places the point); Diameter → Diameter; Width/Height → Distance. The coordinates of the first
point only place it. The same fields are also in Properties (*Values*, with **Apply** and `x` =
"follow the cursor") for Move/Copy/Rotate.

### 4.4 Inference and snapping

- Snapping: point 12 px (shares the point; a projected point → Coincident), midpoint of a
  line (glyph *midpoint* → Midpoint), curve 8 px (*on curve* → point on the curve).
- Inference relative to the previous point (Line and splines): within 3° of horizontal/vertical →
  **H** / **V**; otherwise relative to the previous line of the chain → **//** (Parallel) or **90°**
  (Perpendicular).
- Point alignment: a dashed line to a point with the same X or Y (6 px); for a line → Horizontal/
  Vertical between the points.
- **Ctrl** (held): no H/V/∥/⊥, no tangent snap and no point alignment for the current
  click. Snapping to points, midpoints and curves **stays**.
- **Align** button in the status bar: point alignment on/off.
- Settings: Application Options → *Constraint Settings — Inference* (8.1). Without *Persist*
  the glyphs are shown but no constraints are created (the position stays aligned).
- If the inferred constraint contradicts the others, the line is created without it.

### 4.5 Constraint tools (Constrain)

| Constraint | Selection order |
|---|---|
| **Coincident** | point, then point or curve (or curve, then point) |
| **Collinear** / **Parallel** / **Perpendicular** | line, line |
| **Concentric** | circle/arc, circle/arc |
| **Fix** | point or curve (all of its points) |
| **Horizontal** / **Vertical** | line; or point, point |
| **Tangent** | curve, curve (line–circle or circle–circle) |
| **Symmetric** | point, point, symmetry line; or line, line, line; or circle, circle, line (+ Equal) |
| **Equal** | line, line; or circle/arc, circle/arc |

A constraint that would over-constrain the sketch is refused (nothing changes, an error in
the status bar). The tool stays active.

### 4.6 Dimensions (Dimension, D)

1. Select 1–2 objects (point, line, circle, arc; not splines), then click on empty space
   = the text position. The dimension is shown live before it is placed.
2. The kind follows the selection and the position:

| Selection | Dimension |
|---|---|
| circle | Diameter |
| arc | Radius |
| line or two points | position above/below (within the X range, outside Y) → horizontal; left/right (within the Y range) → vertical; otherwise along the line (aligned) |
| point + line | point–line distance |
| two parallel lines | distance |
| two lines at an angle | Angle; the quadrant of the position chooses θ or 180° − θ |
| point / line / centre + **centerline**, placed on the other side of the axis | diameter about the axis (for Revolve) |

3. The **Edit Dimension** field opens in place: `name =` + a value or an equation (`d0/2`,
   `10 mm`), `mm`/`deg`; **Enter** or ✔ applies; **x** or **Esc** keeps the measured value;
   **±** shows the tolerance (Type, ±/Upper/Lower, Evaluated size). An unachievable value →
   nothing changes, error `Edit Dimension: …`.
4. If the dimension would over-constrain: an *Over-constrained* window → **Accept** (a driven dimension,
   in parentheses) / **Cancel**.

Editing: double-click on the text; Dimension + click on a dimension; right-click → *Edit
Dimension...*; the *Constraints* list in Properties (field, `ref` = driven). A driven dimension cannot
be edited ("make it driving to edit"). Display: the drop-down in the status
bar — Value / Name / Expression / Tolerance. F8/F9 show/hide all
constraint glyphs.

### 4.6a Object Visibility (View → Visibility)

**Object Visibility** ▾ switches whole kinds of construction geometry: **Origin Planes**
(off by default; on, XY / XZ / YZ are always drawn — otherwise only while a command picks a
plane), **User Work Planes**, **User Work Axes**, **User Work Points**, **UCS Triads** (on). A
feature hidden on its own (its browser check box) stays hidden; a switched-off kind cannot be
picked. Remembered with the Application Options.

### 4.7 Relax Mode, DOF, format

- **Relax Mode** (F11, the *Relax* button in the status bar, a mini button in Constrain):
  dimensions give way while dragging; the changed ones become driven.
- **Show All Degrees of Freedom** (*DOF* in the status bar, a mini button, View →
  Degrees of Freedom, right-click on a curve, Properties): purple double arrows on
  free points (X and Y or in one direction), an arc with an arrow on rotating curves, an arrow
  on a free radius. Properties: `Free points · rotating curves · free radii`.
- **Format**: Construction / Centerline / Normal for the selected ones (projected ones do not
  change); Construction with nothing selected = the mode for new geometry.
- **Slice Graphics** (F7, the *Slice* button): cuts away the model in front of the sketch plane;
  turned off with F7 and on Finish Sketch.

### 4.8 Modify

| Tool | Use |
|---|---|
| **Trim** (X) | click on the part to remove (up to the nearest curves). **Shift** = Extend |
| **Extend** | click near the end. **Shift** = Trim |
| **Split** | click near the split location |
| **Offset** (O) | click on a curve, then click for the distance. Properties: *Loop Select* (the whole chain, on), *Constrain Offset* (common offset, on) |
| **Move** | geometry (pre-selection or clicks on curves + Enter / Done), base point, end point. Properties: *Copy* |
| **Copy** | the same, the original stays (also copies the constraints) |
| **Rotate** | geometry, centre, reference point, angle |
| **Mirror** | geometry, mirror line (Symmetric constraints) |
| **Scale** | geometry, base point; *Scale factor* in Properties (2) |
| **Stretch** | geometry, base point, end point (the dimensions update) |
| **Fillet** (2D) | two lines (the clicked sides are kept) or a click on a corner with exactly two lines. *Radius* (2 mm); the first fillet gets a dimension, the following ones — Equal |
| **Chamfer** (2D) | the same. *Equal distance* / *Two distances* / *Distance and angle*; Distance 1 (2), Distance 2 (3), Angle (45°) |

Projected geometry is not modified ("Projected geometry cannot be modified").
The transforms show a dialog in Properties: selector *Geometry* (click = select again),
the steps, *Values*, **Done** (= Esc).

### 4.9 Sketch patterns (Pattern)

The command opens a dialog in Properties; the pre-selection is *Geometry*. Clicks
fill the active selector (`> …`); nothing is created before **OK**; **Cancel** / Esc
close it. **Enter**: from Geometry to the next selector, otherwise OK (if ready).

| Command | Selectors | Values (default) |
|---|---|---|
| **Rectangular** | Geometry, Direction 1 (a line or an X/Y button), Direction 2 (optional; `x` removes it) | Count (3), Spacing / Length (10 mm), Flip, Fitted |
| **Circular** | Geometry, Centre (a point or **By radius**: Radius 20 mm, at angle 0°) | Count (6), Angle (360°), Flip, Fitted (on) |
| **Pattern along Path** (mini) | Geometry, Path (line, arc, spline) | Count (4), Spacing = chord (10 mm) |
| **Pattern at Points** (mini) | Geometry, Base point, Targets (a point, or a click on empty space = a new point; *Clear targets*) | — |
| **Mirror** | see 4.8 | |

The counter `xN` sits next to the last copy. A pattern is edited with a right-click on one of its
curves (2.4): Edit Pattern (Geometry *Add Selected* / *Remove Selected*, *Skip elements by
click*, values, handles in the view, OK / Cancel), Suppress, Offset Element, Delete, Break.

### 4.9a Project Geometry

Sketch → Create → **Project Geometry** (the button is a toggle). A click in the view projects, by priority:
a point of an earlier sketch (within 12 px; centres, ends, spline points) → a curve of an earlier sketch (within 8 px)
→ a model edge (straight: a line; circular and parallel to the sketch: a circle/arc). Earlier sketches are
drawn while the command is on — the hidden (consumed) ones too, in bluish. The candidate under the cursor is highlighted.
**Shift+click** = as construction. The projection is fixed, orange, and follows the source on regeneration
(in the GUI — on Finish Sketch of the source sketch). Own curves attach to a projected point with
Coincident. A circle in a non-parallel plane is refused: project the centre. Esc or right-click →
**Done** stops it. F1 → specification 1.8.

### 4.10 Sketch properties (Properties)

Name (editable), *Plane* + **Redefine**, **Edit/Finish Sketch**, **Look At**;
*Coordinate System*: Origin (part/vertex), Axis edge, *Edge defines the Y axis*, *Flip axis
direction*; *References* (a row for each projection: `Sketch1 · Point`, `Extrusion1 · edge 1`…; ✔ / `!`
missing source, **Break Link**, **Replace** (projections from sketches/curved edges), **Select** (in edit),
`x` = delete; hovering highlights); in edit: *Solver* (state, DOF, redundant, iterations), DOF, number selected +
**Delete**, *Constraints* (`x`, selection, value, `ref`); **Delete Sketch**.

---

## 5. Keyboard shortcuts

Keys do nothing while a text field has focus or a file window /
"Unsaved changes" is open. They are hard-wired (no remapping). The single-letter ones (S, E, R, H, L,
C, D, X, O) work only alone or with Shift — **never with Ctrl or Alt** (Alt+S is reserved for
the command search, `docs/spec/0.15-context-menus-shortcuts.md` §5.3).

### 5.1 Table

| Key | Action | Context | Inventor |
|---|---|---|---|
| **F1** | opens the specification of the current command (`docs/spec/…`) with the system's program: the material/appearance windows → 2.15, Physical Properties → 0.10, work features → 2.10, Shell, Draft, Pattern (2.11) / Mirror (2.12), Fillet, Chamfer, Extrude, Revolve, the sketch tools (1.1–1.7), otherwise `0-ui.md` | global | F1 = help |
| **F2** (held) + left | Pan | view, Inventor profile | the same |
| **F3** (held) + left | Zoom (up = zoom in) | view, Inventor profile | the same |
| **F4** (held) + left | Orbit | view, Inventor profile | the same |
| **F6** | Home view + Fit | global | the same |
| **F7** | Slice Graphics on/off | sketch | the same |
| **F8** | shows the constraint glyphs | global | Show All Constraints |
| **F9** | hides the glyphs | global | Hide All Constraints |
| **F11** | Relax Mode on/off | sketch | the same |
| **F11** | Full Screen on/off | outside a sketch | Full Screen (no key by default) |
| **Ctrl+0** | Clean Screen on/off: only the view and Quick Access | global | the same |
| **Esc** | closes the floating window under the cursor (Parameters, Options, Physical Properties, Measure, Styles Editor, the browsers, Adjust, Clear, Offset Element) without aborting the command | the cursor is over the window and no field is being typed in | closes the dialog |
| **Enter** / **Space** | repeats the last command | **no** open command (panel, plane/target pick, sketch tool or dialog), no selected curves | Inventor: Enter |
| **Enter** | **Continue** to the empty required selector, otherwise **OK** | Face Draft, Shell, Combine, Split, Loft (outside a field) | Inventor: Enter = Continue / OK |
| **Enter** | **OK** if the command can finish; otherwise nothing (input is awaited) — Revolve without an axis: **Continue** to the axis pick | Extrude, Revolve, primitives, Fillet, Chamfer, Hole, Thread, work features, Pattern, Mirror, Sweep, Coil, Rib (outside a field) | the same |
| **Ctrl+Enter** | **OK** (if possible), also while typing in a panel field | every feature panel, Edit Pattern and the sketch pattern dialog | Inventor: Ctrl+Enter |
| **Enter** | end of the chain / spline; Geometry → next step in transforms; pattern: next selector / OK | sketch | |
| **Enter** | places the point with the typed values | Dynamic Input | |
| **Enter** | applies | Edit Dimension, file window (name/folder) | |
| **Esc** | aborts: Select Other, Work/Fillet/Shell/Draft/Pattern/Mirror/Extrude/Revolve (and Edit Feature), plane pick, From/To, Edit Pattern; clears the selection (→ Previous Selection); in a sketch: the first Esc removes the current clicks, the second returns to Select; pattern = Cancel | global | |
| **Esc** | clears the input | Dynamic Input | |
| **Esc** | closes without changes | Edit Dimension, value field, file window, Unsaved changes, Delete Feature | |
| **Delete** | deletes the selected curves and constraints | sketch | Inventor deletes in 3D too |
| **S** | Start 2D Sketch | outside a sketch | the same |
| **H** | Hole | outside a sketch | the same |
| **E** | Extrude (and leaves the sketch) | global | the same |
| **R** | Revolve | outside a sketch | the same |
| **H** | Hole | outside a sketch | the same |
| **R** | Two Point Rectangle | sketch | the same |
| **L** | Line | sketch | the same |
| **C** | Center Point Circle | sketch | the same (in an assembly C = Constrain) |
| **D** | Dimension | sketch | the same |
| **X** | Trim | sketch | the same |
| **O** | Offset | sketch | the same |
| **Shift** (held) | Trim ↔ Extend | Trim / Extend | the same |
| **Shift**+click | adds (3D) / toggles (sketch) | selection | |
| **Ctrl**+click | toggles | selection in 3D | |
| **Shift** / **Ctrl** + box | adds / toggles | selection | |
| **Ctrl** (held) | no H/V/∥/⊥, tangent, alignment inference | drawing in a sketch | the same |
| **Ctrl** (held) | finer step: 0.1 mm / 1° | dragging a manipulator | |
| **Ctrl+Z** | Undo | global | the same |
| **Ctrl+Y**, **Ctrl+Shift+Z** | Redo | global | the same |
| **Ctrl+S** | Save (no file → Save As) | global | the same |
| **Ctrl+Shift+S** | Save As | global | |
| **Ctrl+O** | Open (asks about unsaved changes) | global | the same |
| **Ctrl+N** | New (empty part; asks about unsaved changes) | global | the same |
| **Ctrl+A** | Select All of the selection mode | idle view | |
| **Ctrl+A** | selects all curves | sketch | |
| **Tab** / **Shift+Tab** | next / previous field | Dynamic Input (taken before egui) | the same |
| **↑** | last value of the field | Dynamic Input | |
| **Backspace**, digits, `.` `,` `-` | typing | Dynamic Input | |
| **← ↑** / **→ ↓** | previous / next candidate | Select Other | |
| **Shift + wheel** | cycles the candidate | Select Other | |

Not bound (for comparison with Inventor): F5 (Previous View), Home (Zoom All), Alt+S (command search — coming),
P/C in an assembly, a keymap file.

### 5.2 Mouse + key

See 1.1 (Shift+middle = Orbit) and 1.3–1.4.

### 5.3 Repeating a command

Apart from Repeat, every started command (except Undo/Redo and Select) goes first into the list of
**recent commands** (up to 10; `settings.json` → `options.recent_commands`); the alias
field and the command search will read from it (spec 0.15 §5.1, §5.3).

Remembered: Start 2D Sketch, Extrude, Revolve, Fillet, Chamfer, Shell, Face Draft,
Rectangular/Circular Pattern, Mirror, Work Plane/Axis/Point and every sketch tool (repeated only in a sketch). It is repeated with
Enter/Space or **Repeat <name>** in the right-click menu — only when no command is open
(one shared check, `gui/keys.rs` `OpenState`); in the menu of a command with selectors, Repeat
aborts that command first.

---

## 6. Value fields

Two kinds of fields:

1. **Feature fields** (Extrude, Revolve, Fillet/Chamfer, Shell, Draft, work
   features, Box): show the equation if there is one; on focus the whole text is selected.
2. **Tool fields** (`ExprValue`: Polygon, 2D Fillet/Chamfer, patterns, Scale,
   Offset Element, work dialogs, the Constraints list, tolerance): the same expressions;
   the result is computed at once (for patterns the equation is kept in the dimension).

| Property | Behaviour |
|---|---|
| Syntax | `+ - * / ^ ( )`, functions `sin cos tan asin acos atan sqrt abs floor ceil round min max`, `pi`/`PI`. Example: `20*2`, `width + 5 mm`, `atan(h / w)` |
| Units | `mm cm m in ft um deg rad ul`. A number without a unit = the field's unit (mm; degrees for angles; `sin(30)` = sin 30°) |
| Names | the names from *Parameters* (`d0`, `length`, user ones); `name = expression` names the field (a taken name → error; a field cannot refer to itself) |
| Applying | **Enter** or leaving the field (click elsewhere, Tab) with a valid change |
| Cancel | **Esc** — restores the old value |
| Error | red text; tooltip `Invalid: …`; not applied |
| Tooltip | `name = value` and "Accepts expressions…" |
| Menu **`>`** | **List Parameters** (name + value → inserts the name), **Recently Used** (the last 8, shared by all fields), *Measure* and *Show Dimensions* (grey, to come) |
| Dragging | `ExprValue` and feature fields can **not** be dragged. Only the *Values* fields of the sketch transforms (egui `DragValue`) can be dragged with the mouse |

The **Parameters** window (Tools → Parameters): columns Parameter Name, Consumed by, Unit,
Equation, Nominal Value, Tolerance (Evaluated size), Model Value, Comment; sections *Model
Parameters* and *User Parameters*; cells are applied on leaving/Enter; **Add Numeric**
adds a user parameter (mm), Unit mm/deg/ul, `x` deletes (only if unused).
Error → `Parameters: …`, the change is reverted.

Note: while a sketch has Dynamic Input fields, Tab does not move the focus in Properties.

---

## 7. Manipulators and preview

### 7.1 Manipulators

| Where | Handles |
|---|---|
| Extrude (only Extents = Distance, without From) | an arrow for Distance A (Symmetric: half an arrow on each side), arrow B with Asymmetric, a handle for Taper (and Taper B) near the tip |
| Revolve (Extents = Angle) | an arc about the axis with a handle for Angle A (Flipped — in the other direction, Symmetric — at half the angle), Angle B with Asymmetric |
| Work Plane — Offset | an arrow for the Distance from the base plane |
| Work Plane — Angle to Plane | an arc about the edge for the Angle |

- They are shown in the command's dialog and when a feature is selected in the browser (not in a sketch and not for
  a feature with a problem).
- Hovering (7 px) shows the value (`12.00 mm`, `45.0 deg`); dragging changes it.
- Step: 1 mm (Ctrl: 0.1 mm, at least one step); Revolve angles 5° (Ctrl: 1°, up to
  360°); Taper 1° (±80°).
- In an Extrude/Revolve dialog the change is live; on an existing feature and on a work
  plane — on release (one Undo step). Dragging replaces the equation with a number.

### 7.2 Preview

- Extrude, Revolve, Fillet/Chamfer, Shell, Face Draft, Pattern/Mirror: orange edges of the result,
  computed on a background thread ("Computing the preview..."). OK is enabled while it computes.
  An impossible operation → a yellow box `Warning: …`, OK is grey.
- Extrude/Revolve profiles: the sketch regions light up green (the selected ones darker);
  the Revolve axis — a green dashed line.
- Fillet/Chamfer: the selected edges blue (inactive sets grey-blue), the one under the cursor green.
  Shell/Draft: the selected faces blue, the one under the cursor green; Draft shows an arrow for the direction.
  Pattern/Mirror: the faces of the selected features/bodies blue; Direction 1 — an orange arrow,
  Direction 2 — a blue one, the axis — a long line.
- Work features: a live blue plane / axis / point.
- Sketch: the tool's rubber-band line, the snap glyph, the live dimension, the pattern
  copies.

---

## 8. Settings

### 8.1 Application Options (Tools → Application Options; Sketch → Constraint Settings)

| Group | Option | Effect | Default |
|---|---|---|---|
| Navigation | **Inventor** / **Classic (left = orbit)** | mouse profile (1.1) | Inventor |
| | **Reverse zoom direction** | reverses the wheel | off |
| Language | **Hints in Bulgarian (подсказки на български)** | status-bar hints in Bulgarian (the commands stay in English); also the **БГ** button | off |
| Sketch | **Autoproject part origin on sketch create** | the part's centre is projected into a new sketch | on |
| | **Autoproject edges of the face on sketch create** | the face's edges are projected | on |
| | **Look at sketch plane on sketch creation and edit** | the view turns towards the sketch | on |
| | **Grid lines** / **Axes** / **Coordinate system indicator** | the sketch frame | on |
| Display | **Show constraint labels** | like F8/F9 | on |
| | **Thread texture** | cosmetic threads (Hole Tapped, Thread) are drawn as a texture with helical stripes on the face; off: nothing is drawn (the data stays) | on |
| Constraint Settings — Inference | **Infer constraints** | inference glyphs (Ctrl suppresses them) | on |
| | **Persist constraints** | creates the inferred constraints | on |
| | **Point alignment** | alignment (dashed line); with Persist → H/V between points | on |
| | Horizontal, Vertical, Parallel, Perpendicular, Midpoint, On Curve | which inferences are active | on |
| | **Coincident (snap to and share points)** | off: new lines do not connect to points | on |

All options are remembered between launches (`settings.json`, key `options`; the language — in
`hints_bulgarian`). The window can be resized and scrolled; it remembers its place and size.

### 8.2 settings.json

| Key | Content |
|---|---|
| `hints_bulgarian` | `true`/`false` — language of the hints |
| `recent_files` | up to 10 paths (the newest first; missing ones are not shown) |
| `ui` | the layout: visible panels (`show_browser`, `show_properties`, `show_ribbon`, `show_status_bar`, `show_view_cube`, `show_triad`), `ribbon_minimized`, sides (`browser_side`, `properties_side`: `Left`/`Right`), widths/height (`browser_width`, `properties_width`, `status_height`), floating windows (`windows`: key → `x`, `y`, `w`, `h`), the main window (`main_size`, `maximized`) |
| `options` | Application Options: `mouse` (`Inventor`/`Classic`), `invert_wheel`, `sketch` (the sketch options), `inference`, `show_labels`, `thread_texture`, `home_view` |

`ui` and `options` are written 0.5 s after the last change (dragging a border writes
once) and on close; the write is atomic and keeps the other keys. Unknown keys are
skipped, missing or wrong values stay at the default (field by field, with a
`Settings: …` message in the status bar). A corrupt file → a message, default values
and `settings.json.bak` on the next write; the program does not crash.

There are no materials, templates or profiles in settings.json. The material and appearance libraries
are built in (`data/materials/materials.csv`, `data/appearances/appearances.csv`) and are
extended from a user folder (8.4); **Reload Library** in the Material Browser reads them
again, loading problems are shown in the status bar.

### 8.3 Environment variables

| Variable | Effect |
|---|---|
| `CADCRAFT3D_DEMO` = `rectangle` \| `slot` \| `tangents` \| `lines` \| `revolve` | starts with a demo sketch (in the XY plane, in edit) |
| `CADCRAFT3D_KERNEL=facet` | turns off the exact kernel (everything is built faceted; for comparison) |
| `RUST_LOG` (e.g. `info`, `debug`, `cadcraft3d=trace`) | only raises the log level; a bare level (`debug`) also applies to other libraries (default: our code `info`, others `warn`) |
| `CADCRAFT3D_LOG` = `trace` \| `debug` \| `info` \| `warn` \| `error` \| `off` | the log level for our code and the kernel |
| `CADCRAFT3D_DIAG_DIR` | another folder for `logs/` and `recovery/` (instead of `%APPDATA%\3dcadcraft`) |
| `CADCRAFT_WRITE_COMPAT=1` | tests only (`cargo test --test compat`): writes new fixtures |
| `APPDATA`, `USERPROFILE`/`HOME`, `OneDrive` | where the paths below come from |

Command line: `3dcadcraft [PROJECT]` — a `.3dcadcraft` file or an unpacked folder.

### 8.4 Files and locations

| What | Windows | Linux |
|---|---|---|
| settings.json | `%APPDATA%\3dcadcraft\settings.json` | `~/.config/3dcadcraft/settings.json` (`XDG_CONFIG_HOME` is not used) |
| Start folder of the dialogs | the current file's folder, otherwise `%USERPROFILE%\Documents` or `%OneDrive%\Documents`, otherwise the profile | the file's folder, otherwise `~/Documents`, otherwise `~` |
| User materials / appearances | `%APPDATA%\3dcadcraft\materials\materials.csv`, `…\appearances.csv` (columns as in the built-in CSVs) | `~/.config/3dcadcraft/materials/…` |
| F1 (specifications) | `<source code folder>\docs\spec\…` (the path comes from the build) | the same, opened with `xdg-open` |
| Session log | `%APPDATA%\3dcadcraft\logs\session-YYYYMMDD-HHMMSS.log` (UTC; the last 20 sessions) | `~/.config/3dcadcraft/logs/…` |
| Action journal | `…\logs\session-YYYYMMDD-HHMMSS-journal.jsonl`; session description (`clean_exit`) `…\logs\session-….json` | the same |
| Crash reports | `…\logs\crash-YYYYMMDD-HHMMSS.txt` (the last 20) | the same |
| Autosaves | `%APPDATA%\3dcadcraft\recovery\<project>-<hash>-<time>.3dcadcraft` + `.json` (the last 3 per project) | `~/.config/3dcadcraft/recovery/…` |

**Help** (menu on the right of Quick Access): **Help** (F1), **Save Diagnostics…** (a zip with the log, the journal and the description of the current and the previous session, the crash reports, the current project and `settings.json`), **Open Logs Folder**. After a crash, on start **Restore** appears (Restore / Open Original / Discard). Details: [spec/0.14-diagnostics.md](spec/0.14-diagnostics.md).

Files: **Save** → one `.3dcadcraft` (ZIP); **Save as Folder...** → an unpacked folder
(`name.3dcadcraft/` with `manifest.json`, convenient for git). Open shows such folders too.
A project dropped onto the window (drag&drop) is opened (after the question about unsaved
changes). The title is `<file>[ *] - 3DCadCraft`; `*` = unsaved changes.

File window: **Up** (from a drive root on Windows — a list of drives), **Home**,
a folder field (Enter), a list (a click on a folder enters it; a click on a file selects it; a double-click
— opens/saves), *File name* (Enter), **Save**/**Open**, **Cancel**, **Recent** (with
Open), Esc. Saving over an existing file needs a second press of Save. The extension is
added automatically.

---

## 9. Ribbon and Quick Access

### 9.1 Quick Access (at the very top)

| Element | Action |
|---|---|
| **File** ▸ | New (Ctrl+N), Open... (Ctrl+O), Open Recent ▸, Save (Ctrl+S), Save As... (Ctrl+Shift+S), Save as Folder..., Export ▸ STL... / STEP... / GLB..., the file's path |
| **View** ▸ | User Interface: check boxes **Browser**, **Properties**, **Ribbon**, **Status Bar**, **ViewCube**, **Axis Triad**, **Minimize Ribbon**; **Clean Screen (Ctrl+0)**, **Full Screen (F11)**; side of the **Browser** and **Properties** (Left / Right); **Reset UI Layout**. The Quick Access row always stays visible, so everything can be brought back from here |
| **Open**, **Save** | the same as in File |
| **Undo**, **Redo** | the tooltip shows what (`Undo Drag (Ctrl+Z)`) |
| **Rebuild** | Rebuild All: all features again, without the cache; the result in the status bar |
| **Select: …** ▸ | selection mode and actions (3.1) |
| **Material: <name>** ▸ | material of the part (or of the selected bodies): categories → material (density in the tooltip); **Material Browser...** |
| **Appearance: <name>[*]** ▸ | appearance of the selected faces/features/bodies (with no selection — of the part): *As Material*, *Document Appearances*, the library by category; **Appearance Browser...**, **Adjust...**, **Clear...**. `*` = some faces/features/bodies have their own appearance |
| document name (`*` with changes) | on the right: "3DCadCraft" |

Export: STL (triangles in mm), STEP (exact B-rep), GLB (glTF).

### 9.2 Tabs

The names, icons, keys and tooltips of the buttons come from the **command registry**
(`src/gui/commands.rs`): every command has an immutable id (`part.extrude`), an English name, the name
in Inventor, a Bulgarian name, Inventor's aliases, a key, a place (Tab › Panel), an environment (part
/ sketch) and the FEATURES section if it is not ready yet.

Tabs: **3D Model**, **Sketch**, **Inspect**, **Tools**, **Manage**, **View** (always all of them).
Entering a sketch opens Sketch, leaving it — 3D Model. Grey commands say why in
the tooltip; those not implemented yet say "Planned — docs/FEATURES.md, section X". The key (if
any) is in the tooltip.

**3D Model**

| Panel | Command | Meaning |
|---|---|---|
| Sketch | **Start 2D Sketch** (S) | a sketch on a plane or a face (grey while a plane is being picked) |
| Create | **Extrude** (E) | extrudes closed regions |
| | **Revolve** (R) | revolves regions about an axis |
| | **Sweep** | sweeps profiles along a path (a sketch curve in another plane, or edges); Guide Rail, Parallel, Taper, Twist |
| | **Loft** | a body through sections in order (profiles, faces, a point at the end); Rails, Center Line, Area Loft, Direction / Tangent conditions, Map Point, Closed Loop |
| | **Coil** | winds a closed profile about an axis: Pitch and Revolution / Revolution and Height / Pitch and Height, Taper, Right/Left, closed ends; with Cut it makes a thread |
| | **Rib** | a rib (Parallel) or a web (Normal) from open curves to the body: To Next / Finite, thickness to one side or symmetric, Draft, bosses, rib networks |
| | Emboss (2.17) | to come |
| Modify | **Hole** (H) | hole: Simple / Clearance / Tapped / Taper Tapped, seats, terminations, standards (2.3) |
| | **Fillet** | rounds edges (convex — cuts, concave — adds) |
| | **Chamfer** | chamfer: Distance, Two Distances, Distance and Angle |
| | **Shell** | shell; Inside, Outside, Center; Sharp / Rounded Corners |
| | **Draft** | Face Draft: Fixed Edge, Fixed Plane, Parting Line; One Way / Symmetric / Asymmetric; Move Parting Line |
| | **Thread**, **Combine**, **Split** | thread (2.9); joining bodies; splitting faces and bodies (2.13) |
| | Slot (2.3) | to come |
| Work Features | **Plane** | work plane; the method follows the selection |
| | **Axis** | work axis |
| | **Point** | work point (also Grounded X, Y, Z; centre of a torus/sphere; piercing a curved face) |
| | **UCS** | user coordinate system: X, Y, Z and angles, or three points (centre, X, Y); its own planes, axes and centre |
| Pattern | **Rectangular** | copies of features or bodies in rows along one or two directions |
| | **Circular** | copies at equal angles about an axis |
| | **Mirror** | a mirrored copy of features or bodies about a plane |
| Primitives | **Box**, **Cylinder**, **Sphere**, **Torus** | first a plane, then clicks in it (2.1) |

**Sketch** (the commands are active only in a sketch)

| Panel | Commands |
|---|---|
| Create | **Line** (L); **Circle** ▸ Center Point Circle, Tangent Circle; **Arc** ▸ Three Point, Tangent, Center Point; **Rectangle** ▸ Two Point, Three Point, Two Point Center, Three Point Center; **Slot** ▸ Center to Center, Overall, Center Point, Three Point Arc, Center Point Arc; **Spline** ▸ Interpolation, Control Vertex; Ellipse (1.2, to come); **Point**; **Polygon**; **Fillet** ▸ Fillet, Chamfer; Text (1.2, to come); **Select** (Esc) |
| Modify | **Move**, **Copy**, **Rotate**, **Trim** (X), **Extend**, **Split**, **Offset** (O), **Scale**, **Stretch** |
| Pattern | **Rectangular**, **Circular**, **Mirror**, **Pattern along Path** (mini), **Pattern at Points** (mini) |
| Constrain | **Dimension** (D); mini: Coincident, Collinear, Concentric, Fix, Parallel, Perpendicular, Horizontal, Vertical, Tangent, Symmetric, Equal; Smooth (G2) (1.3, to come); **Constraint Settings** (opens Application Options); **Relax Mode**; **Show DOF**; **Show All**/**Hide All** (F8/F9); **Coordinate System** (the sketch's Properties) |
| Format | mini: **Construction**, **Centerline**, **Normal** |
| Demo | **Demo** ▸ Rectangle (fully defined), Slot (arcs + tangent lines), Tangent circles, Parallel / perpendicular, Shaft half section (centerline closes it) — also works outside a sketch |
| Exit | **Finish Sketch** |

**Inspect**

| Panel | Command |
|---|---|
| Measure | **Measure** (`M`) — a window: one item → length / radius / diameter / arc angle / area / volume / position; two → minimum distance (drawn in the view), dX/dY/dZ, centre distance, angle; while it is open a plain click adds the item (the third starts a new pair); **Physical Properties** — a window: Requested Accuracy, **Update** (when the values are out of date), `kg` (otherwise g), a table by body (Include, Solid, Material, Density, Mass, Volume), Mass, Area, Volume, Center of Gravity, Principal Moments, Mass Moments of Inertia, **Clipboard** |
| Analysis | **Validate Solid** (selects the body: Properties shows V/E/F, Euler–Poincaré, validity); Interference (3.5), Curvature (5.4), Draft Analysis (2.8) — to come |

**Tools**

| Panel | Command |
|---|---|
| Options | **Application Options**; Document Settings (0.11, to come) |
| Material and Appearance | **Material** (Material Browser: Search, *Assign to*, Document Materials, Library by category, Reload Library), **Appearance** (Appearance Browser: Search, As Material, Document Appearances, Library), **Adjust** (Colour, Opacity, **Apply** = a new Custom appearance for the targets; the status bar prompts), **Clear** (Clear Appearance Overrides: Faces, Features, Bodies, Part, *Only the selection*, OK/Cancel) |
| Parameters | **Parameters** (fx) |
| Update | **Rebuild All** |
| Automation | Rules (7.5), AI Assistant (7.6) — to come |
| History | **Undo** (Ctrl+Z), **Redo** (Ctrl+Y) |

**Manage**

| Panel | Command |
|---|---|
| Styles and Standards | **Styles Editor** (Style and Standard Editor: the document's materials and appearances, Document/Both, Update, Purge; Lighting — grey), **Update** (the document's styles from the library), **Purge** (removes the unused ones), Save (2.15, to come) |

Appearance precedence when drawing: face > feature > body > part > material.

**View**

| Panel | Command |
|---|---|
| Visibility | **Constraints** (F8/F9), **Degrees of Freedom** |
| Navigate | **Home** (F6), **Look At** (the edited/selected sketch), **Front** (from −Y), **Top** (from +Z), **Right** (from +X) |
| Appearance | Visual Style (0.3), Section View (0.3) — to come; **Projection** ▸ Orthographic / Perspective / Perspective with Ortho Faces |
| Windows | **User Interface** ▸ Browser / Properties / Ribbon / Status Bar / ViewCube / Axis Triad (pressed = visible); **Clean Screen** (Ctrl+0); **Full Screen** (F11 outside a sketch); **Reset** (Reset UI Layout) |

The ribbon collapses to the tabs with the `^` button to the right of the tabs or with a double-click on the
active tab (`v` restores it); when collapsed, a click on a tab shows its panels until a command is chosen
or you click elsewhere. The borders of the browser, Properties and the status bar can be dragged;
the sizes are remembered. Floating windows can be resized (except the small Offset Element, Adjust,
Clear) and open where they were; on a smaller screen they are moved inside.

### 9.3 Dialogs in Properties (in short)

Every dialog has a title with an icon and **?** (= F1).

| Command | Sections and buttons |
|---|---|
| Extrude | Input Geometry (Sketch, Profiles), Behavior (From, Direction: Default/Flipped/Symmetric/Asymmetric, Extents: Distance/To/To Next/Through All, Distance A/B), Output (Solid/Surface, Join/Cut/Intersect/New Solid, Solids), Advanced Properties (Taper A/B; Match Shape and Infer iMates — grey); **OK**, **Cancel**. Cut/Intersect turns the direction towards the material |
| Revolve | Input Geometry (Profiles, Axis, Origin axis X/Y/Z), Behavior (Full/Angle, Direction, Angle A/B), Output; **OK**, **Cancel** |
| Fillet | Type (Constant/Variable/Chord/Full Round), Select (Edge/Loop/Feature/Solid → All Edges/All Fillets/All Rounds), Edges + **Add set** (up to 9 sets), Edge chain *Automatic*; Variable: points along the edge (Start/End/Point, **Add point**, *Smooth radius*); Size; **OK**, **Apply** (creates and continues), **Cancel** |
| Chamfer | the same + Method, Flip, *Partial* (one straight edge: To Start, Chamfer length, To End), Corners *Cone at small rounds* |
| Shell | `Part > Shell1` (two clicks — rename), Shell Type Sharp/Rounded Corners, Direction Inside/Outside/Center, Thickness (2 mm), Remove Faces (selector), Automatic Face Chain, Solid (with several bodies), Advanced Properties (approximation — inactive); **OK**, **Apply**, **Cancel** |
| Face Draft | Type Fixed Edge/Fixed Plane/Parting Line; selectors Pull Direction / Fixed Plane / Parting Tool / Faces / Static Edges (dot, count, `x`); Flip, X/Y/Z and work axes, Sketch (with Parting Line); Automatic Face Chain, Automatic Blending; One Way/Symmetric/Asymmetric, Draft Angle (1°), Draft Angle 2; Move Parting Line (Angle for Top/Both/Bottom, Select Boundary, Clear All); **OK**, **Apply**, **Cancel** |
| Rectangular Pattern (3D) | Input Geometry (Copy: **Features** / **Solids**, list), Direction 1 (selector, Origin axis X/Y/Z and work axes, Flip, Count, Distribution Incremental/Fitted, Spacing/Length, Midplane), Direction 2 (*Use* + the same), Output (Solids only: Join / New Solid), Creation (Identical; Optimized/Adjust — grey); **OK**, **Apply**, **Cancel** |
| Circular Pattern (3D) | Input Geometry, Rotation Axis (selector, axes, Flip), Placement (Count, Incremental/Fitted, Angle / Total Angle, Midplane; Orientation — grey), Output, Creation; **OK**, **Apply**, **Cancel** |
| Mirror (3D) | Input Geometry, Mirror Plane (selector, Origin plane XY/XZ/YZ), Output (Join / New Solid, *Remove Original*), Creation; **OK**, **Apply**, **Cancel** |
| Work Plane/Axis/Point/UCS | Input (the selected ones, `x`), Origin X/Y/Z/Center, Method, Distance (10 mm) / Angle (90°) / X,Y,Z, Grounded (point), Solution + Next (piercing), Other side (tangent), Flip Normal (plane), X/Y/Z/Angle X/Y/Z (UCS); **OK**, **Cancel**. Properties of a finished feature: inputs, values with expressions, Other side, Flip Normal, Visible, New Sketch, **Redefine**, Delete; double-click = Redefine |
| Selected feature | the same fields, applied at once (an Undo step); **Edit Feature**, **Delete** |
| Box (body) | Length (X), Width (Y), Height (Z) + Validation |

---

## 10. Status bar

From left to right:

| Element | Content |
|---|---|
| State / prompt | Plane pick: "Select a planar face or an origin plane…" / "Select the new plane for …". In a sketch: `<sketch>: Fully constrained` / `Needs N dimension(s)/constraint(s)` / `Over-constrained (N redundant)` / `Constraints conflict — sketch unchanged` (in the sketch's colour) + the tool's prompt (+ `[construction]`). With Fillet/Chamfer, Adjust, Shell, Face Draft, Pattern/Mirror, Work: the command's step. Otherwise: `Selected: …` or "Ready — Start 2D Sketch (S), or right-click a face: New Sketch" |
| Message | the result of the last operation (red = error `<Command>: <reason>`), otherwise a navigation hint for the profile |
| `mm`, `ISO` | units and standard (labels only) |
| `NN ms` | frame time (smoothed) |
| (in a sketch) drop-down | Value / Name / Expression / Tolerance of the dimensions |
| (in a sketch) **DOF** | Show All Degrees of Freedom |
| (in a sketch) **Slice** | Slice Graphics (F7) |
| (in a sketch) **Align** | point alignment (Ctrl suppresses it for one click) |
| (in a sketch) **Relax** | Relax Mode (F11) |
| **БГ** | hints in Bulgarian (saved) |
| `Mouse: …` | the current mouse profile |

---

## 11. Differences from Inventor

- Z is up (Inventor uses Y by default); FRONT looks from −Y. The triad is at the bottom **right**.
- The Classic profile (left button orbits) has no counterpart in Inventor; the SolidWorks/Fusion profiles
  do not exist.
- The wheel zooms about the centre of rotation, not about the cursor; F5 (Previous View) and
  the Home key (Zoom All) are not bound — Fit is a middle-button double-click or the ViewCube
  menu.
- The right-click is a plain menu, not a marking menu; there is no Shift+right-click for Select Other —
  it comes when the cursor rests.
- Keys cannot be remapped; only the hints are translated.
- More quick actions in the sketch, taken from other CAD systems: double-click on a curve = chain,
  Pattern along Path, Pattern at Points, Offset Element, Skip elements by click, By radius
  centre, the `xN` counter.
- The suggestion bar next to a selected face/edge (New Sketch, Extrude/Press-Pull, Edit
  Feature, Edit Sketch, Look At; Fillet/Chamfer) replaces Inventor's mini-toolbar.
- One part per window; the other documents of the project are only visible in the browser.
- The ribbon collapses only to the tabs (Inventor has two more intermediate states); the browser and
  Properties dock on the left or the right but do not float; there is no Navigation Bar.
- Delete works only in a sketch; features are deleted from the browser or Properties.
- In Inventor F6 is Home; here F6 is both Home and Fit; Home can be set from the
  ViewCube menu and is remembered between launches.

---

## 12. Differences from docs/UI.md

**Fixed in `docs/UI.md` on 2026-10-19.** While writing this reference, 21
differences were found; all of them are reflected in UI.md (what is planned but not done is marked there with ⬜), and
the gaps are in [ROADMAP.md](ROADMAP.md#interface-gaps-and-next-steps). In short, for the record:

- Quick Access: there is no New button, there is Rebuild; there are always 6 tabs, without Assemble/Annotate; in
  the Sketch tab there is no Insert panel, there are Format/Demo/Exit.
- The mouse profiles are in Tools → Application Options → Navigation; the Classic profile orbits with
  a left drag everywhere except from a point/manipulator/handle; F6 = Home + Fit, middle double-click =
  Fit, the Home key is not bound.
- Keys are hard-wired (there is no `keymap.toml`); Ctrl+N and Ctrl+Shift+S work; Space
  repeats like Enter; E also works in a sketch, R/S depend on the sketch; Delete only in a sketch; Ctrl
  suppresses only H/V/∥/⊥, tangent and alignment; added Ctrl+A in a sketch, Select Other, the fine
  manipulator step, double-click = chain.
- Dynamic Input also applies to Point, the centred rectangle, Move, Copy, Rotate; `,` is a decimal
  comma.
- Status bar, context menus, the suggestion bar (instead of the mini-toolbar), Edit
  Feature (in the suggestion bar, not in the right-click menu), Export (file window), language (hints
  only) and `settings.json` (only language + `recent_files`) — described as in the code.
- Still open (not in UI.md): the comment in `src/gui/mod.rs` lists
  `CADCRAFT3D_DEMO=rectangle|slot|tangents|lines` without `revolve` (ROADMAP U20).
