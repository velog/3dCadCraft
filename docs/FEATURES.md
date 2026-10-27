# 3DCadCraft — feature catalogue by level

This document describes the **full scope** of 3DCadCraft as a parametric CAD system for mechanical engineering. The capability set of **Autodesk Inventor 2027** is used as the functional reference: which tools a designer expects from a professional CAD. The text is **our own**. Autodesk's documentation, code and file formats are not copied (clean-room). "Inventor" is a trademark of Autodesk, Inc., and 3DCadCraft is not affiliated with Autodesk.

## How to read this

The detailed specifications (option by option, with status) are in [spec/](spec/README.md). The interface requirements — look, behaviour, guidance — are in [spec/0-ui.md](spec/0-ui.md).

The features are grouped into **levels (tiers)**. Each level builds on the previous ones, and the order reflects both the **dependencies** and the **value** to the user:

| Level | Name | What it unlocks |
|---|---|---|
| **0** | Platform | kernel, files, commands, API — the foundation of everything |
| **1** | Sketching | 2D profiles with constraints and dimensions |
| **2** | Parts (basic) | a real machined part: extrusion, holes, fillets |
| **3** | Assemblies | placing and connecting parts, bill of materials (BOM) |
| **4** | Drawings | 2D documentation for manufacturing |
| **5** | Advanced modelling | complex shapes, surfaces, sheet metal, part families |
| **6** | Specialised environments | frames, weldments, tubes, cables, standard components |
| **7** | Analysis, visualisation, automation, exchange | stress analysis, rendering, rules, AI agent, STEP |

**Levels 0–4 = MVP for manufacturing:** with them you can design a part, assemble it and produce a drawing.

Every feature is described with the same template:

- **What it does** — from the user's point of view.
- **How it works** — how it is implemented in 3DCadCraft (algorithm, data structures).
- **Settings** — the parameters the user sets.
- **Inventor equivalent** — for orientation.
- **Status** — ✅ done · 🟡 partial · ⬜ to do. Complexity: S / M / L / XL.

---

# Level 0 — Platform

## 0.1 Geometric kernel (B-Rep) — 🟡 · XL

**What it does.** Keeps the exact shape of bodies: not as a mesh of triangles but as **boundaries**. A body is a closed shell of faces, each face is part of a surface bounded by edges, and each edge is part of a curve between two vertices. So a cylindrical hole stays a *real* cylinder with radius 5.000 mm, not a polygon.

**How it works.**
- **Geometry** (*where*): `Line`, `Circle`, `BSplineCurve` (NURBS with weights, exact circle), `Plane`, `Cylinder`. Cone, sphere, torus, NURBS surface and offset surface are to do.
- **Topology** (*how it is connected*): `Vertex → Edge → Coedge → Loop → Face → Shell → Solid`. `Coedge` (half-edge) makes every edge of a closed body be used exactly twice, in opposite directions. So "inside" and "outside" are strictly defined.
- All elements live in generational arenas (`slotmap`) with typed IDs. There is no `unsafe`, no pointer cycles, and the model is cheap to copy (for Undo) and serialisable.
- **Checks:** closed 2-manifold shell, the Euler–Poincaré formula `V − E + F − R = 2(S − H)` (H = number of through holes), edges must lie on their curves.

**Settings.** Tolerances (see 0.2).

**Inventor equivalent.** Inventor uses the licensed ShapeManager kernel. 3DCadCraft writes its own kernel from scratch in Rust.

**Exact kernel (stage A, [SOLIDCRAFT.md](SOLIDCRAFT.md)).** Box, Extrude, Revolve, Join/Cut/Intersect, Fillet and Chamfer are built by the SolidCraft exact kernel (on truck) through `kernel::exact`. The result is converted into our B-Rep: exact planes, cylinders, cones, spheres and tori, names of faces, edges and vertices following our scheme, triangles for the view. The test part (box, 8 fillets, a chamfer along the chain) has 26 faces instead of ~20,000 and builds in milliseconds. The Surface output of Extrude and Revolve is exact too (open surface bodies outside the Boolean operations). Whatever the exact kernel does not cover yet (Surface with taper, two-sided taper, coincident faces in Cut) builds the whole part with the old faceted kernel. `CADCRAFT3D_KERNEL=facet` turns it off.

**To do (in order):** building bodies from sketches (extrude/revolve) → Boolean operations (union/subtraction/intersection) → fillets → NURBS surfaces. **Boolean operations are the hardest part of any CAD kernel** (see ROADMAP, kernel-level decisions).

## 0.2 Units and tolerances — 🟡 · S

**What it does.** Defines which units are used and when two points count as one.

**How it works.** Internally everything is in **millimetres and radians**, `f64` (about 15 significant digits). Comparisons go only through `Tolerance` (linear 1e-6 mm, angular 1e-9 rad), never through `==`. Display units (mm, cm, m, in, ft) are converted only on input and output.

**Settings.**
- *Document settings:* length and angle units, number of decimal places shown, mass unit.
- *Model:* linear tolerance, for advanced users.

**Inventor equivalent.** Document Settings → Units.

## 0.3 Visualisation and navigation — 🟡 · L

**What it does.** Shows the model in 3D and lets you look around it.

**How it works.** The kernel triangulates (tessellates) the exact geometry to a given chord, and wgpu draws it with MSAA 4x and a depth buffer. Edges are drawn separately so they are always sharp. Faces turned inwards are coloured **red**, as a diagnostic for kernel bugs.

**Specification:** [spec/0.3-view.md](spec/0.3-view.md)

**Controls (now):** the **Inventor** profile by default — Shift + middle button orbits, middle pans, the wheel zooms, F2/F3/F4 + left, double middle click = Fit, F6 = Home; the **Classic** profile — left orbits, right or middle pans (Tools → Application Options → Navigation). Orbit passes freely over the top and the bottom (🟡 waiting for a visual check). All keys and mouse actions: **[REFERENCE.md](REFERENCE.md)** (§1 mouse, §5 keys); the gaps — [ROADMAP.md](ROADMAP.md#interface-gaps-and-next-steps).

**ViewCube** (🟡 waiting for a visual check): 26 directions (faces, edges, corners) with highlight, arrows and 90° rotation when looking at a face, Home as a house icon, an N/E/S/W compass under the cube (drag = rotation about Z, click on a letter = view from that side), dragging the cube = Orbit, smooth transitions (~300 ms), semi-transparent when the mouse is far away, a context menu with the projections and Home.

**Axis triad** (🟡 waiting for a visual check): X/Y/Z (red/green/blue) at the bottom right of the view, rotates with the camera.

**To do:**
- **ViewCube:** settings (size, screen corner, Snap to closest view, Angle of North, Fit-to-View on click);
- **visual styles:** shaded with edges, shaded only, wireframe, hidden lines;
- orthographic and perspective projection: ✅ (Orthographic / Perspective / Perspective with Ortho Faces in the ViewCube menu, right click and View → Projection);
- F5 (Previous View), the Home key (Zoom All), zoom to cursor, Navigation Bar;
- view section (section view);
- background, lighting and materials (see 7.3).

**Settings.**
- *Application Options → Display:* orbit speed, wheel direction, tessellation accuracy;
- colours and themes (dark/light).

**Inventor equivalent.** Navigation Bar, ViewCube, Visual Styles, Half Section View.

## 0.4 Commands, Undo/Redo and history — 🟡 · M (Undo/Redo with snapshots ✅; Command enum ⬜)

**What it does.** Every action ("draw a line", "extrude 10 mm", "place a bolt") is a **command** that can be undone (Undo) and redone (Redo).

**How it works.**
- A serialisable `enum Command` is the only way to change the model. The GUI, scripts and AI agents use the **same** vocabulary and go through the **same** validation.
- An executed command returns an inverse operation or a model delta, which goes onto the Undo stack.
- *Modelling* commands (sketch, extrude etc.) are also recorded in the part's **feature tree**. This is the parametric history, which regenerates when a dimension changes.

**Settings.** Undo stack depth.

**Inventor equivalent.** Undo/Redo, Model Browser.

**Context menus, keys, aliases and conveniences** — 🟡: right click in the view, the browser and the sketch, Repeat, suggestion bar ✅; menu by object kind, marking menu, aliases and a field for them, command search (Alt+S), a cursor hint for every command, key remapping, F5/Home/End/PageUp, Isolate, visibility by kind ⬜. **Specification:** [spec/0.15-context-menus-shortcuts.md](spec/0.15-context-menus-shortcuts.md)

**Diagnostics and recovery** — 🟡 (2026-10-19, waiting for a visual check): a log of every session, an action journal (JSONL) with part deltas that is replayed step by step (`diagnostics::replay`, `cargo run --example replay`), autosave every 5 minutes and after OK into `%APPDATA%\3dcadcraft\recovery` with a **Restore** offer after a crash, a crash report with backtrace and an emergency copy, **Help → Save Diagnostics…** and **Open Logs Folder**. **Specification:** [spec/0.14-diagnostics.md](spec/0.14-diagnostics.md)

## 0.5 Parameters and expressions — ⬜ · M

**What it does.** Every dimension can be a **named parameter** (`width = 120 mm`) or an **expression** (`hole_spacing = width / 4`). Changing a parameter regenerates the model.

**How it works.**
- A parameter table in the document.
- Expressions are parsed into a tree (AST) with unit checking: mm + mm is allowed, mm + deg is an error.
- Dependencies form a graph that is sorted topologically. Cyclic dependencies are rejected.
- Model parameters (created by dimensions) and user parameters live together.

**Settings.** Name, expression, unit, comment, "export as property" (for the BOM), tolerance table (min, nominal, max).

**Inventor equivalent.** Parameters (fx), User Parameters, Linked Spreadsheet.

**2026-10-14:** every numeric field accepts expressions, parameter names and the `>` menu: feature fields (with equations that are kept), Box sizes (`length/width/height` are parameters), tool and dialog values (`ExprValue`). Spacing/Angle of sketch patterns keep the equation in the pattern's dimension.

## 0.6 Projects and files — ✅ (version 1) · M

See **[FILE_FORMAT.md](FILE_FORMAT.md)**. In short:

- one `.3dcadcraft` file with many documents;
- Link, Append and Pack between projects;
- an unpacked folder for git;
- atomic save with `.bak`;
- versioned format.

**Specification:** [spec/0.6-files.md](spec/0.6-files.md) (the file dialog, recent files, Windows).

**File types and design projects** — 🟡 (2026-10-26): one extension per document kind — `.ccpart`, `.ccasm`, `.ccdrw`, `.ccpres` — and design projects `.ccproj` (workspace, libraries, shortcuts, old versions; Inventor's `.ipj`); `FileType` and `DesignProject` exist with tests, Save As / Open filters / the Projects editor ⬜. **Specification:** [spec/0.6a-file-types-projects.md](spec/0.6a-file-types-projects.md)

**Many documents in one file** — 🟡 (2026-10-27; D1, Place in this file, D3 Save to Separate File / Embed / Embed All / Separate All and the New documents option, waiting for the owner's check): the design is kept in one `.3dcadcraft` file; Manage › Documents creates parts, sheet metal parts, assemblies and drawings inside it, and any document can be saved to its own file and linked back, or embedded again (extract / embed / pack / unpack). **Specification:** [spec/0.6b-multi-document-files.md](spec/0.6b-multi-document-files.md)

**Home** — 🟡 (2026-10-26): the start page as the first tab — project, New (with the kinds) / Open, recent documents as cards or a list with search, sort, filter, pins and Remove Unpinned, learning links; shown at start and after the last document closes. **Specification:** [spec/0.17-home.md](spec/0.17-home.md)

**Inventor equivalent.** Project files (`.ipj`), Pack and Go, Vault (partly).

## 0.7 API for agents (JSON-RPC / MCP) — ⬜ · M

**What it does.** External programs and AI agents drive 3DCadCraft **without a GUI**: they open projects, draw sketches, add constraints, extrude, read results (mass, volume, errors) and save.

**How it works.**
- A standalone server (`--no-default-features`, without the graphics part) that translates JSON-RPC methods and **MCP tools** into commands (0.4).
- Transport: stdio (the standard one for MCP) and a local socket.
- Every agent action is an ordinary command, so it **can be undone**.
- Responses are structured: IDs of the created elements, the solver report, body validity.

**Settings.** Enabling/disabling the server, allowed operations (read-only / full control), a log of the agent's actions.

**Inventor equivalent.** Inventor API, iLogic, Autodesk Assistant (2027).

## 0.8 Persistent topology naming — 🟡 · XL (v0: names from the primitives, immutable feature `id` ✅)

**What it does.** Solves the classic **"topological naming problem"**. If a fillet points at "edge #7" and you then change the sketch and the edges get renumbered, the fillet must not jump to the wrong edge (a known problem in FreeCAD up to version 1.0).

**How it works (plan).** Every face and edge gets a **name derived from the history**: "the side face of Extrude1, generated by line L3 of Sketch1". Features inherit and combine these names. References in features are stored as such names, not as indices. In case of ambiguity a geometric heuristic is used, with a warning to the user.

**Inventor equivalent.** In Inventor it is built into the kernel.

## 0.9 Selection and filters — 🟡 · M

**Specification:** [spec/0.9-selection.md](spec/0.9-selection.md), [spec/0.9a-selection-filters.md](spec/0.9a-selection-filters.md)

**State.** One selection model (`command::selection`): by default everything is pickable, the filters (12 kinds) exclude; Select Priority (Faces and Edges / Features / Bodies / Sketch Features); in a command — the intersection of the selector with the filter; a **Shift + right click** menu; Window/Crossing box by direction or always one of them. 🟡 waiting for a visual check.

**What it does.** Selecting vertices, edges, faces, bodies, features and components with the mouse. There are filters ("edges only"), priority (part/feature/face), a selection box (window/crossing) and cycling through overlapping elements.

**How it works.** GPU picking (rendering IDs into a separate texture) for fast selection under the cursor, plus analytic ray–B-Rep intersection for accuracy.

## 0.10 Measurement and physical properties — 🟡 (Physical Properties) · M

**Specification:** [spec/0.10-physical.md](spec/0.10-physical.md)

**State (2026-10-19):** Inspect → Physical Properties: mass per body and total (g / kg), volume, area, centre of gravity, inertia tensor about the centre of gravity, principal moments and axes, Requested Accuracy, Update, Include per body, Clipboard (`part::physical`). Measure 🟡 (Inspect → Measure, `M`: length, radius, area, volume, position of one item; minimum and centre distance and angle of two; spec §3.1). Waiting for a visual check.

**What it does.**
- **Measure:** distance, angle, radius, chain length, area.
- **Physical properties:** mass, volume, area, centre of gravity, moments of inertia and principal axes.

**How it works.** Volume and inertia are computed with the Gauss–Ostrogradsky (divergence) theorem over the B-Rep with adaptive quadrature, exactly for analytic surfaces. The tessellation check already uses the same idea (the volume of a box is checked via divergence).

**Settings.** Material (density), accuracy (low/medium/high), moment-of-inertia system.

**Inventor equivalent.** Measure, iProperties → Physical.

## 0.11 Application and document settings — 🟡 · S

**State (2026-10-19):** Tools → Application Options has Navigation (mouse profile, Reverse zoom direction), Language (hints in Bulgarian), Sketch, Display and Constraint Settings — Inference. `settings.json` stores the hint language, the recent files, all Application Options (`options`) and the interface layout (`ui`). Keys are hard-coded. Document Settings ⬜. Full list of options, keys and mouse: **[REFERENCE.md](REFERENCE.md)** (§5 keys, §8 settings); the gaps — [ROADMAP.md](ROADMAP.md#interface-gaps-and-next-steps) (U1, U2, U8).
**iProperties** — 🟡 (2026-10-24): every document (part, assembly) has General, Summary, Project, Status (with Design State) and Custom properties (Text, Date, Number, Yes or No); File → iProperties or right-click the document / a component; new documents get Author, Designer and Creation Date; an empty Part Number is the file name. **Specification:** [spec/0.16-iproperties.md](spec/0.16-iproperties.md)

**Interface layout** — 🟡: dragging and remembering the widths of the browser, Properties and the status bar; the label column of Properties (drag its border, remembered); collapsing the ribbon to its tabs; View → User Interface (Browser, Properties, Ribbon, Status Bar, ViewCube, Axis Triad), Clean Screen (Ctrl+0), Full Screen (F11 outside a sketch), Reset UI Layout; browser and Properties on the left or on the right; floating windows remember their place and size. **Specification:** [spec/0.13-ui-layout.md](spec/0.13-ui-layout.md)

**Presets** (ready-made sets in the command panels) — 🟡: Extrude, Revolve, Fillet, Chamfer, Shell and Face Draft have a Preset row (No Preset, Last Used, own and built-in sets, Save/Rename/Delete, New Default, Sort). Presets are a user setting (`%APPDATA%\3dcadcraft\presets`, on Linux `~/.config/3dcadcraft/presets`), not document data. **Specification:** [spec/0.11-presets.md](spec/0.11-presets.md)

**Application Options** (per user):
- default units;
- colour scheme;
- mouse and keyboard shortcuts;
- library paths;
- autosave;
- number of `.bak` copies;
- language.

**Document Settings** (per file):
- units;
- default material;
- drafting standard;
- sheet metal rules;
- display precision.

**Styles & Standards** (for drawings, [spec/4.6-drawing-styles.md](spec/4.6-drawing-styles.md)):
- ISO, ANSI, DIN, JIS, GB, BSI;
- fonts, arrowheads, line weights;
- dimension styles.

🟡 2026-10-26: Manage › Styles Editor in a drawing: Standard (projection type, decimal marker, reset to
ISO / ANSI / DIN), Dimension (Units, Display, Text, Options, Notes and Leaders tabs), Text, Leader, Center
Mark, Hatch, View Annotation, Layers, Object Defaults; New / Rename / Delete; the sheet and the PDF follow at
once. Tables' and symbols' style types are listed greyed.

🟡 2026-10-27: **Edit Dimension** (double-click a drawing dimension): the text round `<<>>`, Hide Dimension Value,
symbols; the value's and the tolerance's precision; all 13 tolerance methods (Basic, Symmetric, Deviation, Limits
stacked / linear, Max, Min, Limits / Fits stacked / linear / size limits / tolerances, Reference) with ISO 286 hole
and shaft fits (IT3–IT12, letters C–U / c–u, up to 500 mm); Inspection Dimension (outline, label, rate)
([spec/4.7-drawing-commands.md](spec/4.7-drawing-commands.md)).

🟡 2026-10-27: **Baseline / Chain / Ordinate Dimension** and their **Sets** (BD, CD, OD, BDS, CDS, ODS): picks of one
view, Continue, placement above / beside decides x or y; baseline stacked by the style's spacing, chain on one line,
ordinates at the ends of their extension lines; sets are one annotation (format revision 29).

🟡 2026-10-26: every other command of Inventor's drawing ribbon has its dialog (interface first,
[spec/4.7-drawing-commands.md](spec/4.7-drawing-commands.md)): Break, Break Out, Slice, Crop, Overlay, Draft View,
align views, Arrange, Retrieve, chamfer / punch / bend notes,
symbols, welding, caterpillar, end fill, FCF, datum identifiers and targets, centrelines, Parts List (source view /
file, BOM view, levels), balloons, tables, hole tables, revisions, Replace Model Reference, Define templates; OK
stays disabled until each command's logic exists.

---

# Level 1 — Sketching

## 1.1 Sketch on a plane or face (Start 2D Sketch) — ✅ · M

**Specification:** [spec/1.1-sketch.md](spec/1.1-sketch.md).

**What it does.** Creates a 2D sketch on an **origin plane** (XY, XZ, YZ) or a **planar face** of the body. Work planes (2.10) will be added as a third option. The sketch remembers *what* it was created on and **follows** it: if the face moves when the model changes, the sketch moves with it.

**Workflow (as in Inventor):**
1. **Start 2D Sketch** (3D Model → Sketch, key **S**). The three origin planes appear (semi-transparent), and the face under the cursor is highlighted. Faces take priority over origin planes. Esc cancels.
   - Alternatives: right click on a face → **New Sketch**; in the browser, Origin → right click on a plane → **New Sketch**.
2. Click → **SketchN** is created and edit mode starts: the Sketch tab opens, the view turns perpendicular (**Look At**), the grid, the axes and the triad are shown.
3. **Finish Sketch** (ribbon or right click) leaves the mode. **Edit Sketch** is a double click in the browser.
4. In the browser, right click on a sketch:
   - Edit Sketch / Finish 2D Sketch;
   - **Redefine** (moves the sketch to another face or plane, the 2D geometry is kept);
   - Look At, Hide/Show, Delete.

**Sketch definition** (`part::SketchPlacement`, modelled on the Inventor API's `PlanarSketch`):

| Property | Inventor | Meaning |
|---|---|---|
| Plane | `PlanarEntity` | origin plane or face, **by persistent name** (`Box1:Top`) |
| Origin | `OriginPoint` | the projected part origin or a model vertex |
| Axis | `AxisEntity` | a straight edge giving the axis direction |
| Axis is X / Y | `AxisIsX` | whether the edge gives X or Y |
| Direction | `NaturalAxisDirection` | flipping the axis |

**Default coordinate system** (Z up):
- **XY:** x = X.
- **XZ:** viewed from −Y; x = X, y = Z.
- **YZ:** viewed from +X; x = Y, y = Z.
- **Face:** the normal points outwards. On a vertical face y = +Z (text reads upright); on a horizontal one x = +X.
- **Origin:** the projection of the part origin.

**Edit Coordinate System** (Sketch → Constrain or the Properties panel) changes the origin (vertex), the axis (edge), X/Y and the direction. The geometry **stays in place in 3D**: only its coordinates are re-expressed. Inventor is Y-up by default, so some initial directions differ, but everything can be adjusted.

**Automatic projection** (Tools → Application Options → Sketch, the option names are Inventor's):
- *Autoproject part origin*: the origin as a fixed point;
- *Autoproject edges of the face*: the face's edges as dashed **reference geometry** with orange points.

The projection is **associative**. On regeneration the points are recomputed, and the user geometry connected to them follows. If the source disappears, the projection "freezes" and the sketch is flagged with a warning ("health").

**Other settings (Application Options):** Look at sketch plane on create/edit, Grid lines, Axes, Coordinate system indicator.

**Undo/Redo** (Ctrl+Z / Ctrl+Y) covers creation, Redefine, coordinate system, visibility, deletion, dragging and dimensions.

**Current limitations:**
- only straight edges are projected;
- there are no work planes;
- Look At is limited by the Z-up camera: on horizontal faces y points up on the screen, on vertical ones Z does.

**Code:** `src/part/` (`placement.rs`, `mod.rs`), `src/kernel/query.rs`, GUI in `src/gui/`.

**Inventor equivalent.** Start 2D Sketch, Finish Sketch, Edit Sketch, Redefine, Edit Coordinate System, Application Options → Sketch.

## 1.2 Sketch elements — ✅ (the basic ones) · L

**Specification:** [spec/1.2-sketch-geometry.md](spec/1.2-sketch-geometry.md).

Every tool follows **Inventor's click sequence**. What a click means depends on what is under the cursor: empty space, a point or a curve.

| Tool | Status | Input (as in Inventor) | What it creates automatically |
|---|---|---|---|
| **Line** (L) | ✅ | click, click…: a chain with shared points; Esc or right click → Done | H/V if nearly horizontal or vertical (Ctrl turns it off) |
| Line → **arc on drag** | ✅ | press on the end of a line or arc and drag: **along the tangent** → tangent arc, **across** → perpendicular arc | Tangent, or the centre lies on the line (perpendicularity) |
| **Center Point Circle** (C) | ✅ | centre, a point on the circle | if the second point is **on a curve** → Tangent to it |
| **Tangent Circle** | ✅ | three lines | 3 × Tangent (the solver finds the inscribed circle) |
| **Three Point Arc** | ✅ | the two ends, then a point that sets the side and the radius | — |
| **Tangent Arc** | ✅ | end of a line or arc, then the end of the arc | Tangent (at the shared point) |
| **Center Point Arc** | ✅ | centre, start (radius), end | — |
| **Two Point Rectangle** (R) | ✅ | two opposite corners | H, V, H, V |
| **Three Point Rectangle** | ✅ | corner, end of the first side, adjacent side | ∥, ∥, ⟂ (free orientation) |
| **Two/Three Point Center Rectangle** | ✅ | centre, corner / centre, half side, adjacent side | + construction diagonals, the centre is at their middle |
| **Center to Center Slot** | ✅ | the arc centres, width | 4 × Tangent, Equal, construction axis |
| **Overall Slot** | ✅ | the ends of the slot, width | the same |
| **Center Point Slot** | ✅ | the slot centre, an arc centre, width | the same + Midpoint |
| **Three Point / Center Point Arc Slot** | ✅ | arc (3 points or centre-start-end), width | 4 × Tangent, radial construction lines |
| **Polygon** | ✅ | centre, vertex (inscribed) / middle of a side (circumscribed); number of sides in Properties | vertices on a construction circle or sides tangent to it; Equal |
| **Control Vertex Spline** | ✅ | control points; Enter or double click finishes | the ends can be shared with other elements |
| Point | ✅ | click | Coincident if on a point or curve |
| **Interpolation Spline** | ✅ | points it passes through; handles at every point, Close Curve, Insert/Delete Point, Convert ↔ CV, Display Curvature ([spec](spec/1.2a-splines.md)) | the ends can be shared with other elements |
| Ellipse | ⬜ | centre, axis, second axis | |
| **Fillet** (Create → Fillet) | ✅ | radius in Properties; two lines (the clicked sides stay) or a click on a corner | Tangent ×2; the first fillet in the command gets an R dimension, the following ones — Equal to it |
| **Chamfer** (Create → Fillet → Chamfer) | ✅ | equal distances / two distances / distance and angle; two lines or a corner | virtual corner (construction) + dimensions to it |
| Text | ⬜ | | |

**Format (Sketch → Format):**
- **Construction:** dashed; does not form a profile.
- **Centerline:** dash-dot; an axis for revolving.
- **Normal.**

Applies to the selected elements. If nothing is selected, Construction toggles a mode for new geometry, as in Inventor.

**Selection:**
- the **Select** tool (Esc) selects with a click (Shift adds to the selection);
- **Delete** deletes the selection;
- projected geometry cannot be deleted.

**Fillet and Chamfer** (`sketch/corner.rs`):
- work between two lines, not yet with arcs;
- a corner that has a dimension or constraint (Fix, Distance…) is not deleted. It stays as a **virtual corner**: a point on both lines. So the dimensions survive, as in Inventor;
- on an error the sketch does not change. An error is, for example, parallel lines or a radius larger than the lines.

**How it works.** The ready-made shapes are in `sketch/shapes.rs`. Tests check the **exact number of degrees of freedom and zero redundant constraints** for every shape. Two things found along the way:
- **Tangency at a shared endpoint:** the condition "distance = radius" has a **zero gradient** (double root), so there the solver uses perpendicularity between the radius and the line.
- **Circumscribed polygon with an even number of sides:** by **Pitot's theorem** one of the "equal sides" is redundant, so it is not added.

## 1.3 Geometric constraints — ✅ (without Smooth) · M

**Specification:** [spec/1.3-constraints.md](spec/1.3-constraints.md).

All are in Sketch → Constrain. You choose the tool and pick the geometry **in Inventor's order**. The tool stays active, and Esc ends it. A constraint that would **over-constrain** the sketch is **not applied** and the sketch stays unchanged.

| Constraint | Status | Selection |
|---|---|---|
| Coincident | ✅ | point + point → coincidence; point + curve (line, circle, arc, **spline**) → the point lies on the curve (can slide) |
| Collinear | ✅ | two lines |
| Concentric | ✅ | two circles or arcs |
| Fix | ✅ | a point; or a curve (fixes its defining points) |
| Parallel / Perpendicular | ✅ | two lines |
| Horizontal / Vertical | ✅ | a line, **or two points** |
| Tangent | ✅ | line–circle/arc, circle–circle (internal/external depending on the geometry) |
| Symmetric | ✅ | two points, two lines or two circles + a symmetry line |
| Equal | ✅ | two lines (lengths) or two circles/arcs (radii) |
| Midpoint | ✅ (in the model) | used by the shapes; in Inventor it is Coincident to a midpoint |
| Smooth (G2) | ⬜ | needs spline curvature |

**How it works.**
- Every constraint is an equation `f(x) = 0` over the degrees of freedom.
- The solver minimises ‖F(x)‖² with **Levenberg** (damping λ·I). As λ → 0 the step becomes **minimum-norm**, i.e. the geometry moves *as little as possible*.
- The rank of the Jacobian gives the **remaining degrees of freedom** and the number of **redundant** constraints.

**Colours (as in Inventor):**
- green: fully defined;
- blue: under-defined;
- orange: redundant constraint;
- red: conflict (the sketch does not change).

**Editing constraints (as in Inventor):**
- hovering a constraint label highlights **the geometry it constrains**;
- **click** selects (Shift adds), **Delete** deletes;
- **right click** on the label: Delete Constraint, and for a dimension also Edit Dimension and Driven/Driving;
- **double click** on a dimension opens Edit Dimension;
- in Properties, hovering a row highlights the geometry, and a click selects it;
- right click on a curve → **Delete Constraints** removes all constraints of the curve and its points.

**Degrees of freedom (Show All Degrees of Freedom):** the **DOF** button in the status bar, Constrain → Show DOF or right click on a curve → Display Degrees of Freedom. The glyphs:
- a cross of arrows: the point is free in the plane;
- a double arrow: the point slides in one direction;
- a curved arrow: the curve can rotate;
- a radial arrow: the circle's radius is free.

The analysis uses the **null space of the Jacobian**. In addition, **fully defined curves are coloured green individually**, even before the whole sketch is defined.

**Inventor equivalent.** Constrain panel.

## 1.4 Dimensions (General Dimension) — ✅ · M

**Specification:** [spec/1.4-dimensions.md](spec/1.4-dimensions.md).

**What gets created depends on the selection and on the placement point** (as in Inventor):

| Selection | Dimension |
|---|---|
| circle | **diameter** (Ø) |
| arc | **radius** (R) |
| line, or two points | distance; placed **above/below** → horizontal, **to the side** → vertical, otherwise aligned |
| point + line | perpendicular distance |
| two parallel lines | the distance between them |
| two non-parallel lines | **angle** (entered in degrees) |

Sequence: selection (1 or 2 clicks) → click in empty space to place → the **Edit Dimension** window with the measured value (Enter applies, "Keep measured" keeps the measured value).

If the dimension would **over-constrain** the sketch, you are asked whether to create it as a **driven (reference) dimension**. It is shown **in parentheses**, is computed from the geometry and does not move it. Every dimension can be switched between driving and driven with the "ref" checkbox in Properties.

**To do:** expressions and parameters (0.5); display as name, expression or value.

**Inventor equivalent.** General Dimension, Driven Dimension, Edit Dimension.

## 1.5 Snapping and automatic constraints while drawing — 🟡 · M

**Specification:** [spec/1.5-inference.md](spec/1.5-inference.md).

**Done:**
- snap to a **point**: own points are shared, and a Coincident is added for projected ones;
- snap **onto a curve** (Coincident to the curve), for all curve kinds, including splines;
- a dragged point dropped on a point or curve gets connected to it (Coincident);
- snap radius: 12 px for a point, 8 px for a curve;
- **H/V** for lines and splines (3°);
- **Ctrl** turns inference off;
- a glyph next to the cursor shows what will be created: "coincident", "on curve", H, V.

- snap to the **midpoint of a line** while drawing (glyph "midpoint") → Midpoint constraint; centres of circles and arcs are snapped as points;
- inference of **⊥ and ∥** relative to the previous line in the chain (glyph "90°" / "//"), with a 3° tolerance, after H/V.

**To do:**
- snap to a quadrant;
- inference of tangency and of ∥ / ⊥ to any line the cursor has passed over;
- inference settings;
- Relax mode;
- automatic dimensioning (Auto Dimension).

**Inventor equivalent.** Constraint Inference, Constraint Persistence, Relax Mode.

## 1.6 Modify — ✅ · M

**Specification:** [spec/1.6-modify.md](spec/1.6-modify.md).

| Tool | Behaviour (as in Inventor) |
|---|---|
| **Trim** (X) | removes the clicked part up to the **nearest** intersecting curves. The ends get Coincident to the boundary; with no intersections it removes the whole curve. On a circle, the arc without the clicked part remains. Shift switches to Extend |
| **Extend** | extends the end nearest the click to the next curve (lines and arcs), with Coincident to the boundary. Shift switches to Trim |
| **Split** | splits the curve at the nearest intersection. The pieces stay Collinear (lines) or Equal with a shared centre (arcs), so they keep the original's H/V/∥/⟂ |
| **Offset** (O) | a curve, or the whole chain with **Loop Select**. Lines are Parallel, arcs concentric, smooth joins Tangent. With **Constrain Offset** all pieces are at one distance, which remains the **only** free degree of freedom (it gets dimensioned) |
| **Move / Copy / Rotate** | first the geometry is selected (Select, or in the command itself + Enter), then base point → end point (Rotate: centre → reference → target). The rest of the sketch follows and constraints are respected. Copy also copies the internal constraints |
| **Drag** | a point or a **whole curve** (Select), with highlighting |
| **Relax Mode** | Constrain → Relax Mode or the Relax button in the status bar: while dragging, dimensions are updated instead of holding the geometry |
| **Scale** | the selected geometry → base point; the factor is in Properties. Radii and dimensions entirely inside the selection are scaled, the rest of the sketch follows |
| **Stretch** | the selected geometry → base point → end point. The selection's points move, connected geometry is stretched. Dimensions between moved and unmoved points take the new values, dimensions entirely inside the selection are kept. If the exact position is impossible (H/V), the nearest valid one remains |

**How it works.** `sketch/modify.rs` intersects the carriers (line or circle) and filters by the curves' extents. Offset adds each constraint **only if it is independent** (rank check). At a smooth join the corner is a point of tangency (a double root), so a first-order Tangent is added. Every tool is covered by tests for degrees of freedom and redundant constraints.

## 1.7 Sketch patterns — ✅ · S

**Specification:** [spec/1.7-patterns.md](spec/1.7-patterns.md).

**Mirror** ✅: the selected geometry is mirrored about a line. The mirror copy is tied by **Symmetric** for each pair of points (and Equal for circles); points on the axis are shared.

**Rectangular** ✅:
- you select the geometry, press Rectangular and pick a line for direction 1 and, if count 2 > 1, a line for direction 2;
- Properties set the count (including the original), the spacing and Flip for each direction;
- a preview is shown while you pick.

**Circular** ✅: you select the geometry, press Circular and pick a point for the centre. Properties set the count, the total angle (360° = evenly spaced) and Flip.

**Associativity (as in Inventor):**
- the first step is a construction line with a **dimension**: a distance, or an angle for the circular pattern;
- every copy repeats that step through the hidden internal constraints Translated / Rotated;
- changing the dimension or moving the original moves the whole pattern;
- the copies **add no degrees of freedom**.

A copy of an arc already has a built-in equation "the end lies on the circle". So one of its ends is tied only tangentially; otherwise every copy would be over-constrained (`sketch/pattern.rs`).

**Inventor equivalent.** Rectangular / Circular / Mirror (sketch pattern).

## 1.8 Project geometry — 🟡 · M

Implemented (waiting for a visual check): Sketch → Create → **Project Geometry** of straight model edges (an associative reference line by the vertex names, `PartDocument::project_edge`), of **points and curves from earlier sketches** (point, centre, line, circle, arc, spline; in another plane — orthogonal projection, circles/arcs only in a parallel one) and of **circular edges** (circle/arc, parallel to the sketch) — associative (`part::link`), sick when the source is missing. A *References* list in Properties with Break Link, Delete, Replace and Select; Break Link / Replace Reference / Delete also in the projection's context menu. To do: ellipses (inclined circles), faces, loops, work features, Project Cut Edges.

**Specification:** [spec/1.8-project-geometry.md](spec/1.8-project-geometry.md).

**What it does.** Brings body edges, faces or other sketches into the current sketch as **associative** reference geometry: if the body changes, the projection follows it. It also includes projecting the body's section (cut edges) with the sketch plane.

**How it works.** Orthogonal projection of the 3D curves into the plane (line → line, circle → circle or ellipse). Requires persistent naming (0.8).

**Inventor equivalent.** Project Geometry, Project Cut Edges.

## 1.9 Profiles and regions — ✅ (without spline intersections) · M — *key for Level 2*

**Specification:** [spec/1.9-profiles.md](spec/1.9-profiles.md).

**What it does.** Finds the **closed areas** in the sketch that can be extruded. For example, a rectangle with two circles inside gives "a plate with two holes".

**How it works.**
1. A planar graph is built from the segments, split at the intersection points.
2. The faces of the graph are found (leftmost traversal).
3. The nesting of the loops is determined (which loop is inside which), giving outer loops and holes.
4. A `Loop` is produced with the correct orientation. This is the same convention as in the kernel: outer loop counter-clockwise, holes clockwise.

**Status.** `Sketch::regions()` (`sketch/profile.rs`):
- lines, arcs and circles are split at all intersection and tangency points;
- splines are for now joined only at their ends;
- dangling edges are cut off; construction and centerline geometry does not take part;
- in the sketch being edited, closed regions are shaded.

Triangulation with holes (`kernel::triangulate_with_holes`) connects every hole to the outer loop with a bridge, then uses ear clipping.

**Inventor equivalent.** The Profile selection in Extrude/Revolve.

## 1.10 3D sketch — ⬜ · L

Lines, splines and helices directly in space, plus intersection of surfaces and projection onto faces. Used for paths (sweep, tubes, frames).

**Inventor equivalent.** 3D Sketch.

## 1.11 Blocks, insertion, images — ⬜ · M

- **Sketch blocks:** groups of elements inserted many times, such as 2D kinematic diagrams.
- **Insertion:** DXF and images (for tracing).
- **Spreadsheet-driven points:** points from a table (for holes).

---

# Level 2 — Parts (basic modelling)

> All operations of this level are **parametric**: they are recorded in the feature tree (0.4), have editable parameters and are regenerated on change.

## 2.1 Extrude — 🟡 (all settings except the Boolean operations) · L — *the first 3D operation*

**What it does.** Pulls a profile (1.9) perpendicular to its plane and forms a body.

**Full specification with the status of every option:** [spec/2.1-extrude.md](spec/2.1-extrude.md).

**In short:**
- **done:**
  - regions with holes and several profiles;
  - From;
  - Default, Flipped, Symmetric, Asymmetric;
  - Distance, To (vertex or parallel face), Between, To Next, Through All;
  - Taper A and B (lines and arcs);
  - live preview;
  - consumed and shared sketches;
  - editing and "sick" features;
- Output: Solid / Surface; operation New Solid / Join / Cut / Intersect (see 2.13);
- body selection (Solids) with several bodies ✅;
- 🟡 (2026-10-19, comparison with Inventor 2027, waiting for a visual check): breadcrumb `Sketch1 > Extrusion1` (editing the sketch and returning, renaming), Advanced Settings (Predict Boolean Operation, Keep sketch visible on (+)), "+" button, Ctrl+Enter, editing the sketch's dimensions during the preview, Output without an operation for the base feature, the preview waits for Solids with several bodies, To a work/origin plane and a point, To an inclined face/plane, To a cylindrical face with Alternate Solution, Extend face to end feature (automatic), Terminator for To Next, open profiles with Match Shape;
- 🟡 **Primitives** (3D Model → Primitives): Box and Cylinder (sketch + Extrude), Sphere and Torus (sketch + full Revolve);
- **to do:** conical/spherical terminations, a point from another sketch as To, taper of splines, Revolve with Match Shape.

**Inventor equivalent.** Extrude.

## 2.2 Revolve — 🟡 · L

**Status (specification: [docs/spec/2.2-revolve.md](spec/2.2-revolve.md)).** Done: profiles, axis (a line from the sketch, with automatic choice of the only centerline, X/Y/Z of the origin), Full / Angle / Symmetric / Asymmetric, Solid / Surface, Join / Cut / Intersect / New Solid, angles as parameters, diameter dimension to a centerline. 🟡 (2026-10-19, Inventor 2027, waiting for a visual check): To (a face or plane through the axis, a point) with Minimum Solution, Predict Boolean Operation, Continue with right-click, breadcrumb, "+", Sphere and Torus primitives; fixed direction of Symmetric/Asymmetric in the exact kernel. To do: work axis and edge as the axis, To Next, Match Shape.

**What it does.** Revolves a profile about an axis: shafts, bushings, flanges.

**How it works.**
- The side faces are surfaces of revolution: a line parallel to the axis → cylinder; an inclined line → cone; an arc → torus or sphere; the general case is NURBS.
- If the profile touches the axis, degenerate edges (poles) appear. The kernel must support them.

**Settings.** Profile, axis (a line from the sketch, a work axis or an edge), angle (360° / angle / symmetric / to a face), operation (Join/Cut/...).

**Inventor equivalent.** Revolve.

## 2.3 Hole and Slot — 🟡 · L

**Specification:** [spec/2.3-hole.md](spec/2.3-hole.md). Implemented (waiting for a visual check): Simple, Clearance (ISO 273, ASME B18.2.8: Close / Normal / Loose), Tapped (ISO metric coarse and fine, UNC / UNF) — cosmetic or **modeled** thread with clearance for 3D printing; Counterbore, Spotface, Countersink seats (90° ISO, 82° ANSI) and filling from the tables (ISO 4762 / DIN 974-1, ISO 10642, ASME B18.3); terminations Distance / Through All / To (flat face), Direction 1/2, drill point Flat / 118°; centres: click on a face (sketch with points), points from a visible sketch, concentric to a circular edge; Edit Feature, parameters, preview. The standards are CSV files in `data/standards/` (user copy in `%APPDATA%\3dcadcraft\standards`). For the drawing: `HoleSpec::note()` ("M8x1.25 - 6H ↧ 12", "⌴ Ø11 ↧ 6.4"), `PartDocument::holes()` and grouping "4x Ø6.6 THRU". Added after the owner's check: the centres are **Center Points**; clicking a straight edge after the centre adds a dimension to it (in the hole's sketch: projected edge + point–line dimension, a parameter); Symmetric; Extend To on/off; Flat W / Angle V / Angle Y; concentric to a cylindrical face; Allow Center Point Creation and the menu (Preselect, Keep sketch visible on (+)); breadcrumb `Sketch > Hole1`; invalid values in red; Ctrl+Enter / +; a deleted point removes its hole; the modeled thread is no longer lost silently. To do: Taper Tapped, Select Feature Dimension, direction from a point + face/edge, window selection, Slot, note and table in the drawing (4.3–4.4).

**What it does.** Creates standard holes with correct geometry and *semantics*, i.e. the hole "knows" it is an M8 with a thread. So the drawing (4.3) shows the correct annotation, and BOM and CAM recognise it.

**Types:**
- simple;
- **counterbore**;
- **countersink**;
- spotface;
- **tapped hole**;
- **clearance hole** (per standard: fine/normal/coarse);
- tapered pipe thread.

**Settings.**
- *Placement:* from centre points in a sketch, on a face with dimensions to two edges, concentric to a circular edge, at a point;
- *Termination:* depth, to a face, through all;
- *Bottom:* flat or with a drill point angle (118°);
- *Thread:* standard (ISO metric, UNC/UNF, G/BSP), size, pitch, class, right/left hand, thread depth.

**Slot (new in Inventor 2027).** A slot with the same panel as the hole: size, orientation, position, plus variants with counterbore and countersink.

**How it works.** Geometrically it is a Revolve of a standard profile (cylinder + cone), subtracted with Cut. The standards tables (diameters, depths) are data in a library, not code. The thread is stored as an *attribute* of the cylindrical face (cosmetic thread), without modelling a helical surface.

**Inventor equivalent.** Hole, Slot (2027), Thread.

## 2.4 Fillet — 🟡 · XL

**Specification:** [spec/2.4-fillet.md](spec/2.4-fillet.md). Done: constant radius on straight edges between planar faces (convex and concave) and on circular edges of cylinders, several edges in one feature, preview, OK/Apply, editing the radius with an expression; **several sets with different radii**, **Variable** (start/end/intermediate points, Smooth), **Chord** (constant width), **Full Round** (by a middle face between parallel sides); selection **Edge / Loop / Feature / Solid** (All Edges / All Fillets / All Rounds).

**What it does.** Rounds edges with a radius. It is the most used operation after Extrude and **technically the hardest**.

**Types:**
- constant radius;
- **variable** radius (values at points along the edge);
- *setback* at corners where three fillets meet;
- fillet between two faces (face fillet);
- **full round** (a fillet that fully replaces a middle face).

**How it works.** For each edge a "rolling ball" of radius r is built, tangent to both neighbouring faces. The path of its centre is the intersection of two offset surfaces, and the fillet surface is a tube around that path. It is trimmed along the lines of tangency, and the neighbouring faces are shortened. Corners where three or more fillets meet are closed with a *vertex blend*.

The analytic cases (plane–plane, plane–cylinder) give a cylinder or a torus and are implemented first. The general case needs NURBS.

**Settings.** Edges (with selection of a chain of tangent edges), radius, mode (edge/face/full round), *Roll along sharp edges*, *Rolling ball where possible*, *Preserve all features*.

**Inventor equivalent.** Fillet.

**Edit Feature** (double-click): the model rolls back to before the feature, the edges are selected again, OK replaces it in place. A Fillet/Chamfer that the exact kernel cannot build gives an error in < 1 s (without the faceted path, which froze).

## 2.5 Chamfer — 🟡 · L

**Specification:** [spec/2.5-chamfer.md](spec/2.5-chamfer.md). Done: Distance, Two Distances (Flip), Distance and Angle; selection Edge / Loop / Feature / Solid as in Fillet.

A chamfer along an edge, given in three ways: **distance**, **distance + angle** or **two distances**. The implementation is simpler than Fillet: a planar face obtained from the intersection of two offset surfaces.

**Inventor equivalent.** Chamfer.

**A chamfer larger than a round in the loop:** the top face gets sharp corners, the chamfer planes cut the round (like Creo Intersect). The **Cone at small rounds** checkbox (a cone under the angle) is in the dialog but not built yet ([spec §2b](spec/2.5-chamfer.md)).

## 2.6 Shell — 🟡 · L

**Specification:** [spec/2.6-shell.md](spec/2.6-shell.md). Implemented (waiting for a visual check): Remove Faces with Automatic Face Chain (selector with guidance, right-click with Continue/Back, Enter/Ctrl+Enter), Inside / Outside / Center, Sharp / Rounded Corners, a closed cavity with no faces removed, thickness with an expression and an arrow in the view, renaming in the breadcrumb, Edit Feature, preview. Advanced Properties (approximation) are shown disabled — the exact kernel does not approximate. To do: Unique Face Thickness.

Hollows the body into walls of thickness t, with the selected faces removed (open). Supports a different thickness for individual faces.

**How it works.** Offset of each face by −t. A cylinder of radius R gives a cylinder of radius R − t, and a NURBS surface gives an offset surface. Then the inner shell is subtracted from the body.

**Settings.** Thickness, direction (inside, outside, both sides), removed faces, individual thicknesses.

**Inventor equivalent.** Shell.

## 2.7 Rib / Web — 🟡 · M

**Specification:** [spec/2.7-rib.md](spec/2.7-rib.md). Implemented (waiting for a visual check): Normal to Sketch Plane (web) and Parallel to Sketch Plane (rib), direction towards the body with Flip, Solids, Extend Profile (shown only when the ends do not reach the body), thickness to one side/the other or symmetric, To Next / Finite, Taper, Draft (Hold Thickness At Top / At Root), bosses (Centers, Select All, Diameter, Offset, Draft Angle), networks of many chains in one sketch, a closed profile for Web, Edit Feature, preview. To do: Presets.

From an open profile in a sketch (a line) creates a rib with a thickness that extends to the body's faces. Used to stiffen castings and plastic parts.

**Settings.** Thickness, direction, to the next face or at a distance, draft angle for removal from the mould, fillet at the root.

## 2.8 Draft — 🟡 · L

**Specification:** [spec/2.8-draft.md](spec/2.8-draft.md). Implemented (waiting for a visual check): Fixed Edge (a fixed edge perpendicular to the direction, shown in the view), Fixed Plane, **Parting Line** (a plane or a 2D sketch at one height), **One Way / Symmetric / Asymmetric** with Draft Angle 2 (the face is split by the plane and each part is tilted in its own direction), **Move Parting Line** (Static Edges, Select Boundary, Clear All, Angle for Top/Both/Bottom), Pull Direction from a face, plane, edge or axis with Flip and an arrow, **Automatic Blending** (rounds along the direction become cones and stay tangent), Automatic Face Chain, angles with expressions, guidance (selectors, right-click Continue/Back, Enter), Edit Feature, preview. To do: inclined/curved parting line, 3D sketch and surface as the tool, Absolute/Relative, Ctrl+click to remove.

Tilts faces at an angle to a pull direction, for castings and die casting. Methods: fixed edge or fixed plane (parting line).

## 2.9 Thread — 🟡 · M

**Specification:** [spec/2.9-thread.md](spec/2.9-thread.md). Implemented (waiting for a visual check): a thread on a cylindrical face (shaft or hole), the size is proposed from the diameter, a check that the size fits the face, Full Length or Offset + Length, Right/Left Hand, cosmetic (data + a texture with helical stripes one pitch apart on the face, only along the thread's length, and in pattern copies; Application Options → Display → Thread texture) or modeled (groove per ISO 68-1; only the zone around the thread is faceted), clearance for 3D printing. To do: several faces at once, tapered thread.

A cosmetic thread on a cylindrical face (attribute + texture in the view + correct representation in the drawing). A modeled (physical) thread is optional, for 3D printing.

**Settings.** Standard, size, class, length, offset, right/left hand.

## 2.10 Work features — 🟡 · M

**Specification:** [spec/2.10-work-features.md](spec/2.10-work-features.md). Done: commands Plane / Axis / Point / UCS (the method is inferred from the selection): planes (Offset, Through Point, Midplane, Midplane of Torus, Angle with an arc, Three Points, Two Coplanar Edges, Normal to Axis, Tangent to Surface Through Edge / Through Point / Parallel to Plane, Normal to Curve at Point, Flip Normal), axes (On Line or a line from a sketch, Parallel Through Point, Two Points, Two Planes, Normal to Plane, Circular or Elliptical Edge, Revolved Face/Feature, Line on Plane), points (Vertex/Sketch Point/Midpoint, Three Planes, Two Lines, Plane or Surface + Line, Loop Center, Torus/Sphere Center, Grounded), UCS (6 parameters or 3 points, with its own planes/axes/centre); Redefine; sketches on work planes and UCS. The new parts are waiting for a visual check. To do: UCS triad, Object Visibility, Show Inputs, Loop Select.

**What it does.** Helper planes, axes, points and local coordinate systems (UCS) relative to which sketches are drawn and features are positioned.

**Work plane variants:**
- offset from a face;
- at an angle about an edge;
- through 3 points;
- tangent to a cylinder;
- midway between two parallel faces;
- perpendicular to a curve at a point.

**Work axis variants:**
- through two points;
- axis of a cylinder;
- intersection of two planes.

**How it works.** Work features are features in the tree and are regenerated too.

**Inventor equivalent.** Work Plane / Axis / Point, UCS.

## 2.11 Feature patterns — 🟡 · M

**Specification:** [spec/2.11-patterns.md](spec/2.11-patterns.md). Implemented (waiting for a visual check): Rectangular Pattern (Direction 1 and 2, Flip, Midplane, Incremental / Fitted, count and spacing as parameters with expressions) and Circular Pattern (axis from an edge, origin or work axis, count and angle, Fitted 360°, Midplane, Flip) of features (Extrude, Revolve, other patterns: a copy of the tool with its operation, all copies with one Boolean operation) or of whole bodies (Join / New Solid); copies outside the body are skipped for Cut; up to 1000 copies; Edit Feature, preview. To do: Suppress of copies, Orientation Fixed, Optimized / Adjust, along a curve, Sketch Driven, irregular copies, manipulators.

**Types:**
- **Rectangular:** two directions, count and spacing;
- **Circular:** axis, count, angle;
- **Sketch-driven:** copies at the points of a sketch;
- **Along a curve.**

**Behaviour.**
- Individual copies can be **suppressed**.
- New in the Inventor 2027 API: **irregular distribution** (irregular distribution set).

**How it works.** The feature is repeated with a transformation. Optimisation: the *resulting* geometry is copied and one common Boolean operation is done instead of N separate ones.

**Settings.** Count, distance or angle, total distance, computation *identical* (the same body) or *adjust* (each copy is recomputed, e.g. against the end of the part).

## 2.12 Mirror — 🟡 · S

**Specification:** [spec/2.12-mirror.md](spec/2.12-mirror.md). Implemented (waiting for a visual check): mirror of features or of bodies about an origin or work plane or a planar face; for bodies Join / New Solid / Remove Original; Edit Feature, preview. To do: automatic addition of dependent features, Optimized / Adjust.

A mirror copy of features or of the whole body about a plane.

## 2.13 Boolean operations and multiple bodies — 🟡 · XL — *critical path*

**Status (specification: [docs/spec/2.13-booleans.md](spec/2.13-booleans.md)).** The exact Boolean operations come from the SolidCraft kernel (`kernel::exact`, see 0.1); the faceted stage 1 remains a fallback. Stage 1 is done: Join / Cut / Intersect / New Solid for Extrude and Revolve through BSP CSG on tagged polygons and reassembly into a B-Rep. Face names are kept, curved faces are facets with smooth shading, and split bodies become separate. To do: exact Boolean operations (stage 2), body selection.

**Combine and Split** — 🟡 implemented, waiting for a visual check. **Specification:** [spec/2.13-combine-split.md](spec/2.13-combine-split.md). Combine: base + tool bodies, Join / Cut / Intersect, Keep Toolbody (the kept body is hidden), deleting it restores the tools. Split: Split Face (a plane, a 2D sketch or a surface of one face; All Faces), Trim Solid (Keep Default / Flip, arrow), Split Solid (the new body `<id>#2`); the tool is a work or origin plane, a 2D sketch (a straight line, closed loops or an **open chain — spline, lines and arcs**, extended along the tangents) or a **surface body** (Extrude / Revolve with Output Surface, now exact); it is selected in the view (the origin planes are shown while the selector is active) or in the tree (Origin, work plane, sketch, Surface Bodies). Face Draft on a piece of a split face works. To do: a surface of several faces for Split Face, a quilt from Sweep/Loft/Stitch, window selection.

**What it does.** Union, subtraction and intersection of bodies. All Cut/Join operations go through here.

**Related operations:**
- **Split** (cutting with a plane/surface);
- **Move/Copy body**;
- parts with **several bodies** (multi-body);
- **Make Components** (the body becomes a separate part in an assembly).

**How it works.**
1. Face–face intersection (analytic for plane/cylinder/cone/sphere, numeric for NURBS).
2. Splitting the faces along the intersection curves.
3. Classification of each piece as inside, outside or on the boundary relative to the other body.
4. Assembly of the result.

Robustness in touching and nearly coincident cases is achieved with tolerances and a "fuzzy" mode. The details are in ROADMAP, kernel-level decisions.

**Inventor equivalent.** Combine, Split, Move Bodies, Make Components.

## 2.14 Thicken / Offset, Delete Face, Replace Face — ⬜ · L

**Command set and dialogs:** [spec/2.0-part-commands.md](spec/2.0-part-commands.md)

- **Thicken/Offset:** turns a surface into a body with a thickness.
- **Delete Face:** removes a face and closes the hole (healing imported models).
- **Replace Face:** replaces a face with another surface.

## 2.15 Materials and appearance — 🟡 · M

**Specification:** [spec/2.15-materials.md](spec/2.15-materials.md)

**State (2026-10-19):** CSV libraries (`data/materials`, `data/appearances`) + a user one in `%APPDATA%\3dcadcraft\materials`; material of the part and of a body; appearance of a part / body / feature / face with precedence Face > Feature > Body > Part > Material; document copies; QAT lists Material and Appearance; Tools → Material and Appearance (Material, Appearance, Adjust, Clear); Manage → Styles Editor (Update, Purge); transparency in the view; colours in GLB and STEP. Waiting for a visual check.

**What it does.** The material carries physical properties (density, Young's modulus, Poisson's ratio, yield strength, thermal conductivity), while the *appearance* is separate (colour, roughness, metal). The appearance can also be set on individual faces.

**Settings.** A material library (steel S235/S355, AlMg3, PA6...), custom materials, colour override for a face.

**Inventor equivalent.** Material and Appearance Browser.

## 2.16 Feature tree (Model Browser) — 🟡 · M

**Specification:** [spec/2.16-browser.md](spec/2.16-browser.md)

**Status (2026-10-19, waiting for a visual check):** End of Part (dragging, Move EOP to Top / End / Here, Delete All Features Below EOP), insertion of new features and sketches at the marker, reordering by dragging with a dependency check, Suppress / Unsuppress with the dependents, Delete with Inventor's options, Rename (F2, slow double-click), highlighting of the faces on hover. To do: conditional suppression, filters, Find, folders.

**What it does.**
- Shows the part's history.
- Allows editing a feature (double-click), **suppressing** and **reordering**.
- **Rolling back** with the *End of Part* marker: the model is shown up to that point, and new features are inserted there.
- **Folders** for grouping (the Inventor 2027 API adds folders to the part browser).

**How it works.** A part is a list of features. Regeneration runs them in order, caches the results and on a change re-runs only the features from the changed one onwards. On an error (for example an edge that no longer exists) the feature is marked red and the rest continue where possible.

## 2.17 Emboss / Engrave, Decal — ⬜ · M

**Command set and dialogs:** [spec/2.0-part-commands.md](spec/2.0-part-commands.md)

- **Emboss / Engrave:** raised or recessed text or outline on a face, including wrapped around a cylinder.
- **Decal:** an image projected onto a face.

---

# Level 3 — Assemblies

## 3.1 Placing components — 🟡 (A1: place, ground, move, rotate) · M

**Specification:** [spec/3.1-assemblies.md](spec/3.1-assemblies.md) (3.1–3.3)

**What it does.** Places parts and subassemblies in an assembly: from the same project, from a linked library (Link) or as a copy (Append). The first component is usually **grounded**.

**How it works.** `Occurrence { id, name, component: DocRef, transform, grounded, visible }` in the file format; a part placed N times is stored once. Step A1 (2026-10-23, waiting for a visual check): File → New Assembly (its own file), Place (a file browser with a preview; part files are linked, named after the file), Place Grounded at Origin and 90° turns, the first component grounded, names `Part:k`, selection and prehighlight, Free Move / Free Rotate, Grounded / Visibility / Open / Delete, browser tree, Properties with position and rotation (`src/assembly.rs`, `src/gui/assembly.rs`).

**Settings.** Position on placement, "place at the origin, grounded" (Place Grounded at Origin), number of copies, name.

**Inventor equivalent.** Place, Place from Content Center, Place Grounded at Origin.

## 3.2 Assembly constraints — 🟡 (A2: Mate / Flush, Angle, Insert; waiting for a visual check) · L

| Constraint | Meaning |
|---|---|
| **Mate** | two planes facing each other (normals opposite), axes coincide, points coincide |
| **Flush** | two planes in one direction (level with each other) |
| **Angle** | angle between faces or edges (directed/undirected) |
| **Tangent** | a cylinder or sphere tangent to a face |
| **Insert** | axis and face at once (a bolt in a hole) |
| **Symmetry** | two components symmetric about a plane |
| **Motion** | transmission ratio (gears, rack and pinion) |
| **Transitional** | cam–follower |

**Settings.** Offset, minimum/maximum (limits of motion), mate/flush solution, preview.

## 3.3 Joints — 🟡 (A3: Rigid, Rotational, Slider, Cylindrical, Planar, Ball, limits; waiting for a visual check) · L

A more intuitive way: one operation sets both the *position* and the *motion*.

**Types:**
- **Rigid** (fixed);
- **Rotational** (hinge);
- **Slider**;
- **Cylindrical**;
- **Planar**;
- **Ball** (spherical).

**Settings.** Angle/travel limits, initial position, direction flip.

**How it works (for 3.2 and 3.3).** Each component has 6 degrees of freedom: 3 translations and 3 rotations, parametrised with a quaternion. The constraints are equations as in the 2D solver (1.3), with the same Levenberg and rank analysis, which shows the "remaining degrees of freedom" of each component.

**Inventor equivalent.** Constrain, Joint.

## 3.4 Dragging and contact — ⬜ · M

- Dragging a component with the mouse within its freedom. It uses the same idea as dragging points in the sketch, which already works.
- **Contact solver:** components cannot penetrate each other while dragging.

## 3.5 Interference check — ⬜ · M

Finds the overlapping volumes between components, shows them in red and gives their volume. Uses the Boolean intersection (2.13) with a fast pre-filter by bounding boxes (AABB/BVH).

**Inventor equivalent.** Analyze Interference.

## 3.6 Bill of materials (BOM) — 🟡 (first level; waiting for a visual check) · M

**Specification:** [spec/3.6-bom.md](spec/3.6-bom.md)

**What it does.** A list of the components with quantities.

**Views:**
- *structured*: by subassembly levels;
- *parts only*: a flat list.

**Properties and numbering.**
- Property columns: part number, description, material, mass, vendor.
- Item numbers, used by the balloons in the drawing (4.4).

**Settings.** Visible columns, grouping, the component's BOM structure (normal / phantom / reference / purchased / inseparable).

**Inventor equivalent.** Bill of Materials.

## 3.7 Component patterns and mirror — ⬜ · M

**Kinds.**
- **Patterns:** rectangular, circular, following a feature pattern in a part (bolts in the holes of a flange).
- **Sketch-driven patterns** (new in 2027).

**Mirror Components.** Mirrors components, with a choice for each component (improved in 2027):
- *reuse*: the same part, only rotated;
- *mirrored new part*: a left/right version.

## 3.8 Replace, copy, make independent — ⬜ · S

- **Replace:** one component with another, trying to keep the constraints.
- **Copy Components.**
- **Make independent:** turns a linked document (Link) into a local copy. Internally it uses Append.

## 3.9 Representations and model states — ⬜ · L

- **View representations:** visibility, colours, transparency.
- **Positional representations:** different positions of the mechanism, e.g. "open" and "closed".
- **Model States:** variants of the same document with different parameters or suppressed features and components, e.g. "short/long" or "simplified" configurations. In 3DCadCraft this is the common mechanism for **configurations** (see 5.7).

## 3.10 Top-down design — ⬜ · L

- **Create in place:** creating a part directly in the assembly, with a sketch on a face of another component.
- **Adaptivity:** a part's size driven by the assembly.
- **Derive:** a part derived from another, e.g. a casting mould from a model.
- **Skeleton / layout:** one master skeleton drives the whole product.

## 3.11 Simplification — ⬜ · M

**Shrinkwrap / Simplify.** The assembly becomes one lightweight part:
- internal components are removed;
- small holes are filled;
- only the outer shell remains.

Used for handing models to suppliers without revealing the details.

---

# Level 4 — Drawings (2D documentation)

**Specification (research first):** [spec/4-drawings.md](spec/4-drawings.md) — views and HLR plan, annotations, tables, output, stages D1–D6.

> A drawing will be a new kind of document (`DocumentData::Drawing`) in the same project and will be **associative**: a change in the model updates the views and dimensions.

## 4.1 Sheets, formats, standards — 🟡 · M

**Specification:** [spec/4-drawings.md](spec/4-drawings.md). 🟡 2026-10-26: drawing documents, sheets
(New / Edit / Delete), ISO 5457 border with zones, a fixed title block from iProperties; waiting for a
visual check.

**What it does.** Sheets A4–A0 (or ANSI A–E), a border, a **title block** with fields filled in automatically from the model's properties (name, material, mass, drafter, date, revision).

**Standards.** ISO, DIN, ANSI, JIS, GB, BSI. They define fonts, arrowheads, line weights, the **projection method** (European/first angle or American/third angle) and the dimension style.

**Settings.** Sheet templates, title block editor, Styles & Standards Editor.

## 4.2 Views — 🟡 · XL

🟡 2026-10-26: Base, Projected, Section (full, straight line, hatched), Detail (circular) and Auxiliary
views with hidden line removal (display mesh, A2 of the spec), move / align / edit / delete; Break, Break
Out, Slice, Crop, Overlay, Draft views are listed greyed. The base view's preview has Inventor's ViewCube
(any orientation, Custom View with free orbit and Rotate at Angle), corner scaling and projected-view triangles.

| View | What it does |
|---|---|
| **Base** | the first view of the model in a chosen orientation and scale |
| **Projected** | orthographic and isometric views from the base view |
| **Section** | a section along a line, with hatching; full, half, offset, aligned |
| **Detail** | an enlarged circle or rectangle from another view |
| **Auxiliary** | perpendicular to an inclined edge |
| **Break** | a shortened long part |
| **Break-out** | a local section inside a contour |
| **Slice** | a thin section without the geometry behind the cut |
| **Crop** | cropping a view |
| **Overlay** | several positions of the mechanism in one view |

**How it works.** The core of drawings is **HLR (Hidden Line Removal)**:
1. The B-Rep's edges and silhouette curves are projected.
2. Which parts are visible and which hidden is decided by exact computation, not by rasterisation.
3. The result is 2D curves with associative references to the original edges (for dimensions).

A section is a Boolean intersection with a half-space (2.13) plus hatching by material.

**Settings.** Scale, style (visible, hidden and tangent lines, shading), label, alignment between views.

## 4.3 Annotations — 🟡 · L

🟡 2026-10-26: Dimension (length, diameter / radius, point to point; horizontal / vertical / aligned),
Center Mark, Centerline, Text; attached to edges by name, orphans marked. Waiting for a visual check.

**Kinds.**
- **Dimensions:** general, baseline (from one datum), ordinate, chain.
- **Retrieve Model Dimensions:** pulls in the dimensions from the sketches and features.
- **Hole and thread notes:** from the Hole semantics (2.3), e.g. "4× M8 – 6H ↧ 15".
- **Centre marks and centrelines.**
- **Text and leaders.**
- **Surface roughness.**
- **GD&T:** feature control frames for form and position tolerances, datums.
- **Welding symbols.**
- **Dimension tolerances:** symmetric, deviation, limits, ISO 286 fits (e.g. H7/g6).

**How it works.** Each annotation is attached to geometry of a view (associatively). If the edge disappears, the annotation is marked as "orphaned" instead of silently disappearing.

## 4.4 Tables — 🟡 · M

🟡 2026-10-26: assembly views; **Parts List** (BOM view, numbering, merge, wrapping, any iProperty as a
column, cell overrides kept by part number, sort / renumber, hidden and custom rows) with **Balloon** and
**Auto Balloon** reading item numbers through it. Waiting for a visual check. Bend tables of flat pattern views with bend tags (Table on a flat view, spec
4.8); hole, revision and general tables to do. **Specification:** [spec/4.4-parts-list.md](spec/4.4-parts-list.md)

- **Parts list** (from the BOM, 3.6), with **balloons** (item numbers);
- **revision table**;
- **hole table** (coordinates relative to a datum);
- general table;
- **bend table** (for sheet metal, 5.6).

The Inventor 2027 API adds replacing the BOM view while **keeping the placement of the annotations**. That is a good target for our behaviour too.

## 4.5 Export — 🟡 · M

🟡 2026-10-26: PDF (krilla, vector, all / current / range of sheets, embedded font). DXF / SVG / print ⬜.

PDF (vector, multi-sheet), DXF (per sheet), SVG and printing.

---

# Level 5 — Advanced modelling

## 5.1 Sweep — 🟡 · L

A profile moved along a trajectory (path).

**Specification:** [spec/5.1-sweep.md](spec/5.1-sweep.md)

**Modes.**
- **Path:** the profile keeps the same angle to the path. 🟡 (exact kernel; path from a 2D sketch in
  another plane or from model edges; 3D sketch ⛔ 1.10)
- **Parallel:** the profile keeps its orientation. 🟡
- **Guide rail:** the profile is controlled by a second curve (Profile Scaling X & Y / X / None). 🟡
- **Guide surface.** ⬜ (clear refusal)
- **Solid Sweep** (a body along the path). ⬜ (clear refusal)

**Settings.** Twist 🟡, taper (scale about the path) 🟡, Join / Cut /
Intersect / New Solid 🟡, Surface ⬜. Waiting for a visual check.

## 5.2 Loft (transition between sections) — 🟡 · XL

A smooth surface or body through several sections. It can be controlled by **rails**, by a **centre line** or with an **area loft** (a given section area along the length).

**Specification:** [spec/5.2-loft.md](spec/5.2-loft.md)

**End conditions:** free, tangent, smooth (G2), direction. Used for transition ducts, housings and blades.

**Ours:** sections from sketches (also with holes — a hollow body), face loops, a point at the end
(pyramid); 2 sections ruled, ≥ 3 smooth; Join / Cut / Intersect / New Solid 🟡.
**Rails** 🟡 (1, 2, 3 and more rails that touch every section; stations fitted to the
rails), **Center Line** 🟡 (sections perpendicular to the curve, frames without twist),
**Area Loft** 🟡 (placed sections with an area or a scale, Proportional / Absolute, Driving /
Driven), the **Direction** (Angle, Weight) and **Tangent** (to the faces adjacent to a face
loop) conditions 🟡, manual matching with **Map Point** 🟡 (twist), **Closed Loop** 🟡 (ring);
tabs Curves / Conditions / Transition with the common selector flow. Smooth (G2), rounded
point, Surface, conditions together with rails ⬜ (clear refusal). Waiting for a visual check.

## 5.3 Coil — 🟡 · M

**Specification:** [spec/5.3-coil.md](spec/5.3-coil.md). Done (waiting for a visual check): profiles and axis (a sketch line/centreline picked automatically, origin or work axis, straight edge; Flip), Pitch and Revolution / Revolution and Height / Pitch and Height, Taper, Right/Left, Close Start/End with Transition and Flat Angle, Solid and Surface, Join/Cut/Intersect/New Solid with Solids, Keep sketch visible, a thread made with Coil + Cut (only the zone around the coil is faceted), Edit Feature, preview. To do: Presets, profile with holes, Spiral.

Springs and helical surfaces. Defined by height and pitch, number of revolutions and height, or number of revolutions and pitch. There are tapered coils and closed ends.

## 5.4 Surfaces — ⬜ · XL

**Command set and dialogs:** [spec/2.0-part-commands.md](spec/2.0-part-commands.md)

**Tools.**
- **Boundary Patch:** a surface inside a closed loop.
- **Stitch:** stitching surfaces into a body.
- **Sculpt:** a body bounded by surfaces.
- **Trim, Extend.**
- **Ruled surface:** normal, tangent, along a vector.
- **Repair:** diagnosing and healing imported models.

**Kernel foundation.** Requires NURBS surfaces and robust surface–surface intersection.

## 5.5 Direct editing and Freeform — ⬜ · L / XL

- **Direct edit:** moving, rotating and scaling faces without history. Useful for imported STEP models.
- **Freeform (T-spline/SubD):** organic shapes by dragging a control cage. A long-term goal.

## 5.6 Sheet Metal — 🟡 (Defaults, Face, Flange, Convert, Hem, Fold, Contour Flange, Cut, Unfold / Refold, Flat Pattern, DXF; waiting for a visual check) · XL

**Specification (research first):** [spec/5.6-sheet-metal.md](spec/5.6-sheet-metal.md) — Inventor's rules, features and flat pattern option by option, what SolidCraft's sheet module already does, stages S1–S5.

**What it does.** Parts from sheet stock that are **unfolded** into a flat pattern for laser/punching and bent on a press brake.

**Sheet metal rules (Sheet Metal Rules / Styles):**
- thickness;
- bend radius;
- **K-factor** (position of the neutral axis; it sets the developed length) or a bend table;
- relief shape and size (2026-10-26: Round / Square 2-bend reliefs are flat cut-outs about the crossing of the bend centre lines, laser-ready);
- corner gap.

**Features:**

| Feature | What it does |
|---|---|
| Face | a plate from a profile with the thickness from the rules |
| Contour Flange | a bent profile from an open line (L, U, Z) |
| Flange | a flange along an edge, with angle, height, position |
| Hem | a hem (single, double, teardrop, rolled) |
| Fold | a fold along a sketch line |
| Bend | a bend between two plates |
| Corner Seam / Round / Chamfer | corner treatment |
| Cut | a cut, also wrapped across bends |
| Punch Tool | standard punch forms (louvres, ribs, holes) from a library |
| Contour Roll | a rolled profile |
| Lofted Flange | a transition between two sections (hopper, funnel) |
| Unfold / Refold | temporary unfolding for features in the unfolded state |
| **Flat Pattern** | the flat pattern with bend lines, DXF export; in drawings: flat pattern views with bend centre lines / extents and Bend Notes ([spec/4.8](spec/4.8-sheet-metal-drawings.md)) |

**How it works.** The part keeps the **bend topology**. Each bend is a cylindrical segment with thickness t and radius r. The developed length of a bend through angle θ is `L = θ·(r + K·t)`. Unfolding "straightens" the cylinders into planes in the order of the bend tree.

## 5.7 Feature reuse and part families — ⬜ · L

**Mechanisms.**
- **Derive Part:** a part derived from another, with scale, mirror and choice of bodies.
- **iFeature:** a saved feature (e.g. a standard keyway) for inserting into other parts.
- **Part families (iPart in Inventor):** a table of variants, e.g. an M6/M8/M10 bolt with lengths. Each row is a configuration.

**3DCadCraft approach.** **Configurations** (see 3.9 Model States) and tables are one common mechanism. One document with a parameter table gives all variants, and a Link to "bolt, row M8×30" is an ordinary `DocRef` + a choice of configuration.

## 5.8 Plastic parts — ⬜ · L

**Command set and dialogs:** [spec/2.0-part-commands.md](spec/2.0-part-commands.md)

- **Grill:** a ventilation grille;
- **Boss:** a screw boss;
- **Rest:** a support;
- **Snap Fit:** a snap joint;
- **Lip / Groove:** a lip/groove between the two halves of a housing;
- **Rule Fillet:** fillets by a rule, e.g. "all edges between these features".

## 5.9 Point clouds and meshes — ⬜ · L

- **Import:**
  - point clouds from 3D scanning (E57, LAS, PLY);
  - meshes (STL, OBJ, 3MF).
- **Tools:**
  - intersecting the cloud with a plane, as the basis for a sketch;
  - fitting planes, cylinders and spheres;
  - mesh to B-Rep for simple shapes.

Inventor 2027 improved working with point clouds: sections, point size, coordinate system alignment, undoing the crop. That is a good guide for the scope.

---

# Level 6 — Specialised environments

## 6.1 Steel structures (Frame Generator) — ⬜ · L

**Specification (research first):** [spec/6.1-frame-generator.md](spec/6.1-frame-generator.md) — stages F1–F4.

**What it does.** Places **standard profiles** on a skeleton (lines in a 3D sketch, or edges): I/H beams, channels, angles, tubes.

**End treatment:**
- **Miter**;
- **Notch** (cut to the profile);
- **Trim/Extend** to a face;
- **Lengthen/Shorten.**

**Output.** A cut list with lengths and angles.

**How it works.** The profiles are parametric parts from a library (5.7). Each frame member is a Sweep (5.1) of the section along a segment of the skeleton.

## 6.2 Design calculators (Design Accelerator) — ⬜ · XL

Generators that **calculate** and **model** standard machine elements:

- **Bolted connections:** choosing the bolt, washers, nuts and holes in several parts at once, with a strength check.
- **Shafts:** segments, chamfers, grooves, keyways, bearing seats; bending, torsion, deflection.
- **Gears:** spur (straight/helical), bevel, worm; geometry per ISO/DIN, strength check.
- **Bearings:** selection by load and life (L10).
- **Keys, splines, pins.**
- **Springs:** compression, extension, torsion.
- **Belt and chain drives.**

**How it works.** The calculation modules are pure logic (tested with examples from the standards), and the output is ordinary parts and assemblies in the project.

## 6.3 Standard parts library (Content Center) — ⬜ · L

**Specification (research first):** [spec/6.3-content-center.md](spec/6.3-content-center.md) — stages L1–L5.

**What it does.** Thousands of standard parts: bolts, nuts, washers, bearings, profiles, seals per ISO/DIN/ANSI. New in Inventor 2027: publishing **whole assemblies** to the library.

**3DCadCraft approach.**
- Libraries are **ordinary `.3dcadcraft` projects** with parametric families (5.7), connected by Link (already implemented).
- A company keeps its `company-standard.3dcadcraft` in a shared folder or in git.
- Pack embeds whatever is used when sending a project out.

## 6.4 Weldments — ⬜ · L

**Sequence.**
1. **Preparation** of the edges (weld chamfers).
2. **Welds:** fillet, groove, cosmetic.
3. **Machining** after welding.

**Properties.** Weld beads have volume and mass, and their symbols go into the drawing (4.3).

## 6.5 Tube & Pipe — ⬜ · XL

Routes with automatic routing. Styles set the diameter, material, minimum bend radius, fittings (elbows, tees, flanges) and bent tubes. Output: isometric drawings, a cut list.

## 6.6 Cable & Harness — ⬜ · XL

Wires, cables, harness segments, connectors with pins, lengths, flexible routes and **flattening the harness** for manufacturing (nailboard).

## 6.7 Mold Design — ⬜ · XL (long term)

Parting line and parting surface, cores and cavities, runner system, cooling, choice of a standard mold base.

---

# Level 7 — Analysis, visualisation, automation, exchange

## 7.1 Stress analysis (FEA) — ⬜ · XL

**What it does.** Linear static analysis (von Mises stresses, deformations, safety factor) and modal analysis (natural frequencies).

**How it works.**
1. A tetrahedral mesh from the B-Rep (refined around small details).
2. Quadratic tetrahedra (10 nodes).
3. Assembling the stiffness matrix.
4. Solving with an iterative method (preconditioned conjugate gradients).
5. **Convergence:** the mesh is refined until the result stops changing by more than X%.

**Settings.**
- Material (from 2.15).
- Loads: force, pressure, gravity, moment, remote load.
- Constraints: fixed, frictionless, pin.
- Contacts between the parts of an assembly.
- Mesh size.

## 7.2 Dynamic simulation — ⬜ · L

Kinematics and dynamics of mechanisms derived from the joints (3.3): forces, moments, friction, springs, drivers, graphs of the quantities, export of the loads to FEA.

## 7.3 Rendering (Studio) — ⬜ · L

Photorealistic images: PBR materials, HDR environment lighting, shadows, depth of field. The implementation is GPU path tracing through wgpu compute. It also covers animations: a 360° turntable, mechanism motion, a camera along a path.

## 7.4 Presentations and instructions — ⬜ · M

Exploded views with trail lines, storyboards, disassembly animation, export to video and images. The views are used in drawings (4.2) for assembly instructions.

## 7.5 Rules and automation (iLogic equivalent) — ⬜ · L

**What it does.** Rules that react to changes, for example:

- "if length > 1000, add a support in the middle";
- "the material determines the price";
- "on save, generate a PDF".

The tools are forms for entering parameters and batch processing of many files.

**How it works.**
- An embedded scripting language in pure Rust (candidate: **Rhai**) or WASM plug-ins.
- A script calls the same Command API (0.4), i.e. every action can be undone.

Inventor 2027 adds **Codeblocks**: visual programming with blocks, without writing code. A good target for level 2 of this module.

## 7.6 AI assistant — ⬜ · M

**What it does.** Natural language → actions in the model. For example:

- "make a ⌀120 flange with 6 M10 holes on a ⌀90 circle";
- "change the material of all plastic parts";
- "export a PDF of all drawings".

Inventor 2027 introduces *Autodesk Assistant* with similar capabilities.

**3DCadCraft approach.**
- The architecture is **agent-ready from day one**: any agent that speaks MCP works through the 0.7 API.
- The built-in chat panel is just another MCP client.
- **Safety:**
  - every action is a command that can be undone;
  - there is a preview before applying;
  - there is an action log;
  - there is a "read-only" mode.

## 7.7 Data exchange — 🟡 · XL

**Specification:** [spec/7.7-export.md](spec/7.7-export.md). Done: export to **STL** (binary, closed), **STEP** AP242 (exact surfaces) and **GLB** from File → Export and from the command line (`examples/export.rs`). To do: STEP import, 3MF, settings.

The table of formats is in [FILE_FORMAT.md, section 8](FILE_FORMAT.md#8-exchange-with-other-programs-not-the-native-format). **STEP AP242** has the highest priority, because exchange with every other CAD system goes through it.

**How it works.** STEP is a text format (ISO 10303-21). The reader parses the entity graph and translates the geometry (B_SPLINE_SURFACE_WITH_KNOTS, CYLINDRICAL_SURFACE...) and the topology (ADVANCED_FACE, EDGE_LOOP...) into our kernel. Compatibility with Inventor/SolidWorks/Fusion is tested on public model sets.

## 7.8 Data management (instead of Vault) — ⬜ · M

- **Versions:** the project is kept in git in unpacked form. Each document is a separate file, so the diff is meaningful.
- **Locking:** prevents two people editing one document at the same time.
- **Revisions and states:**
  - document statuses: work in progress, for review, approved, archived;
  - revision letters (A, B, C);
  - title block fields.
- **Pack** when sending a project out.

## 7.9 Command-line batch processing — ⬜ · S

Example: `3dcadcraft export --pdf drawings/*.3dcadcraft`, or regenerating, validating and exporting many projects. Uses the headless kernel (no GUI) and suits CI.

## 7.10 Model-Based Definition (3D annotations) — ⬜ · L

Dimensions, tolerances and GD&T directly on the 3D model, without a drawing, exported as semantic PMI in STEP AP242. The modern alternative to 2D drawings.

---

## Status summary

| Level | Done | Partial | To do |
|---|---|---|---|
| 0 Platform | 0.6 files | 0.1 kernel, 0.2 units, 0.3 visualisation (waiting for a visual check), 0.4 Undo/Redo (snapshots), 0.8 naming v0, 0.9 selection, 0.10 Physical Properties, 0.11 Application Options (not saved) | 0.5 (expressions and the Parameters table are done, see the section), 0.7 API |
| 1 Sketching | 1.1 sketch on a plane/face; 1.2 drawing tools; 1.3 constraints; 1.4 dimensions; 1.6 modify; 1.7 patterns; 1.9 profiles | 1.5 snapping | 1.8, 1.10, 1.11 |
| 2 Parts | — | 2.1 Extrude, 2.2 Revolve, 2.4 Fillet, 2.5 Chamfer, 2.6 Shell, 2.8 Draft, 2.10 work features, 2.11 patterns, 2.12 Mirror, 2.13 Booleans, 2.15 materials (2.6, 2.8, 2.11, 2.12, 2.15 waiting for a visual check) | 2.3, 2.7 (waiting for a visual check), 2.9, 2.14, 2.16, 2.17 |
| 3 Assemblies | — | 3.1 (data structure) | the rest |
| 4–7 | — | 7.7 export STL/STEP/GLB | the rest |

The implementation plan (phases, acceptance criteria, kernel-level decisions) is in **[ROADMAP.md](ROADMAP.md)**.
