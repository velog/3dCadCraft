# Implementation and usage plan

Legend: ✅ done · 🟡 in progress · ⬜ to do. The numbers (2.1, 3.3…) point to [FEATURES.md](FEATURES.md).

Every phase ends with **acceptance criteria**, checked by tests *and visually* in the application.

---

## ⚠ Critical fixes

Area: view speed. Found by the owner on 2026-10-20 on a hole with a modeled thread (M8). Before any new features:

| # | Problem | Measured | Likely cause | Where to look |
|---|---|---|---|---|
| K1 | Mouse over the thread: the frame becomes slow | ~60 ms/frame | the faceted thread zone has thousands of small faces and edges; picking (`edge_under` over `visible_edges`, a hidden-edge check with a ray on every segment) and `PickCache` cannot cope with them | `gui/mod.rs` `edge_under`, `select::point_hidden`, `draw_visible_polyline`, `PickCache` |
| K2 | Slice Graphics (sketch through the thread) | ~600 ms/frame | probably the section/hatching or the scene is recomputed every frame instead of once on change | `gui/renderer.rs` (`clip`, `fs_backface`), `build_scene`, `scene_dirty` on Slice |
| K3 | In the thread zone the edge of every triangle is drawn | visible in a screenshot | the faces from `faceted_boolean` are not marked as facets of one surface (`facet_of`), so each of their edges is "real" | `kernel::exact::cut_threads` / `slab_threads`, `BRep::mark_facets`, `feature_edges` |
| K4 | Countersinks are slow, more than linearly in one feature | 1 hole 0.33 s (0.87 s before `exact::cone_tool`), 6 holes 2.3 s, 36 holes ~40 s; a far side countersink doubles it (found by the perf test of 2026-10-23) | the cones go through truck's general Boolean (not the prism route straight bores take); per cone ~90 ms against the face, ~200 ms where the bore meets it, and something in truck's face splitting / classification grows with the holes on one face. **Tried and rejected:** cropping the meshes before truck's interference sweep (no gain), bores before the cones (36 holes: 68 s) | 🟡 2026-10-23: 90° countersinks on a face square to the hole are chamfers of the bore rims (`exact::chamfer_circles`): 6 holes at both ends 0.34 s, 36 holes 2.3 s. Still open for other angles / tilted faces: profile `truck_shapeops` (faces_classification, divide_face) on 12 cones; or cut each hole's cones in a small zone around it (as modeled threads); or the kernel's chamfer with an angle (`ChamferSecond::Angle`) |
| K5 | `hollow_box::picking_edges_one_by_one_is_exact_or_refused` fails its 8 s step budget under `cargo xtask ci` | one step ~3 s alone, > 8 s with all 507 lib tests in parallel (2026-10-23; also on b4c8290, before the countersink change) | the step is a fillet of many edges of a hollow box; the budget measures wall time on a loaded CPU | find what makes a step 3 s (the kernel's fillet of the growing edge set) and make it faster, or run the timed part single-threaded; never raise the number |
| K6 | CI (Linux) failed `tests/compat.rs::face_names_stay` from 2026-10-25 (run #45, dc549b7) to 2026-10-27: `r20/sheet_r20.3dcadcraft` got a new face `Box1:Bend:2`; passed on Windows | every Linux run #45–#94 | the cylinders of a bend hanging off a rim were named after that rim (`base:rim.E2`, `rim.far`) or `Bend:k` by where their one sample fell (`exact::face_samples`: the largest of many equal triangles, picked by last-bit differences between platforms) | ✅ fixed 2026-10-27: `rim_face_at` skips points where a child flange's bend hangs off the rim (`vendor` `on_child_bend`), so every bend cylinder is `Bend:k`; `names.json` of r20 / r21 re-recorded (their features reference none of the changed names); the test diffs as multisets |

### Hole: working with the centres (the owner, 2026-10-20)

| # | Problem / wish | How it should be (Inventor) | Where to look |
|---|---|---|---|
| H1 | In Edit Feature of a Hole, new holes cannot be added in the sketch itself | a list of the centres in the panel with an "add" / "don't add" mode (a click on a point adds it to or removes it from the hole) | `gui/hole.rs`, `gui/hole_panel.rs`, `HoleFeature::centers` — 🟡 done 2026-10-23: Mode Add / Remove, a list of centres with ✕, face clicks in Edit Feature become Center Points of the hole's sketch; waiting for a visual check |
| H2 | Edit Sketch opened from a Hole (breadcrumb `Sketch > Hole1`) does not return to the Hole after Finish Sketch | Finish Sketch returns to the Hole panel with everything as it was (as Extrude already does: `suspended_draft`) | `gui/hole.rs`, `finish_sketch`, `gui/breadcrumb.rs` — 🟡 done: `hole_return` + `resume_hole` reopen the panel after Finish Sketch; waiting for a visual check |
| H3 | New center points added in the sketch are not selected in the hole | on returning to the Hole (or Edit Feature) the new unused Center Points of the sketch are added automatically (Inventor: center points are selected automatically) | `unused_center_points`, `HoleDraft` — 🟡 done: `new_center_points` adds them on return (test); waiting for a visual check |
| H4 | An ordinary point cannot become a center point | Format → Center Point on selected points converts them (and back); also in a point's context menu | `sketch::PointStyle`, `gui/sketch_tools.rs`, the point menu — 🟡 done: right-click a standalone sketch point → Convert to Center Point / Normal Point (points are not selectable, so the Format button stays for new points); waiting for a visual check |

Goal: under 16 ms per frame while picking and with Slice on a part with a modeled thread and with a pattern of 2500 holes. Every fix comes with a speed test in `tests/perf.rs`.

### Display: one circle / one cylinder (the owner, 2026-10-23)

| # | Problem | How it should be | Where to look |
|---|---|---|---|
| D1 | Cylinders and holes show as 2–3 faces with seam lines; picking lights one piece | the kernel (truck) has no periodic faces or closed edges, so a circle is ≥ 2 arcs and a cylinder ≥ 2 faces — show and select them as **one** face / one edge: hide seams between pieces of one analytic surface, a click selects every piece, Properties and names treat them as one (Measure already does) | `kernel::exact::to_brep`, `BRep::feature_edges`, `gui/select.rs`, the kernel's own merge in `vendor/solidcraft-kernel/src/topo.rs` — 🟡 done 2026-10-23 (`kernel::smooth`): seams between pieces of one cylinder / cone / sphere / torus are neither drawn nor pickable, a face selects its whole smooth group, an edge every arc of its circle; planes keep their edges (Split Face); test `tests/smooth_display.rs`; waiting for a visual check |

### Working rule: SolidCraft first

Before a new feature or a fix, read how SolidCraft does it (`crates/doc/src/eval*.rs` for how each
feature calls the kernel, `crates/engine`, `crates/io`, `crates/mcp`) and adapt it with an
`ATTRIBUTION.md` row; invent our own only where it has none. Most of our kernel failures were in
*how* we called the kernel, which SolidCraft had already solved.

## Kernel-level decisions

The decisions that determine everything else are made now, while changing them is cheap.

| # | Decision | Choice | Why | Status |
|---|---|---|---|---|
| K1 | Numbers | `f64`, mm/rad internally, comparisons only with a tolerance | `f32` loses microns on parts over 1 m | ✅ |
| K2 | Representation | B-Rep with half-edges (`Coedge`), geometry separated from topology | the industry standard; exact surfaces, exact STEP | ✅ |
| K3 | Memory | arenas + typed IDs (`slotmap`), no `Rc<RefCell>` and no `unsafe` | cheap copying (Undo), serialization, detection of stale IDs | ✅ |
| K4 | Geometric types | a closed `enum` (Line/Circle/BSpline; Plane/Cylinder; Cone/Sphere/Torus/NURBS to come) | every algorithm is forced to handle every type; easy serialization | ✅ the base |
| K5 | Kernel curves | NURBS with an exact derivative (homogeneous coordinates) | exact circles and cones, exchange via STEP | ✅ |
| K6 | Sketch solver | Levenberg (λ·I), minimum norm, rank analysis | minimal change while dragging; DOF and redundancy as in Inventor | ✅ |
| K7 | Files | a single `.3dcadcraft` (ZIP or folder), UUID, Link/Append/Pack, atomic save | see [FILE_FORMAT.md](FILE_FORMAT.md) | ✅ |
| K8 | Model changes | only through commands with an inverse operation | Undo/Redo, agents and scripts through one door | ⬜ Phase 1 |
| K9 | Parametrics | the feature tree is the source of truth, the B-Rep is a regenerable cache | changing a dimension updates everything | ⬜ Phase 1–2 |
| K10 | Persistent naming | face/edge names derived from history (feature + generating element) | no "jumping" fillets on change (FreeCAD's problem) | ⬜ Phase 2 |
| K11 | Boolean operations | exact B-Rep Booleans with tolerances (fuzzy). Order: planar → quadrics (cylinder, cone, sphere) → NURBS. Analytic intersection where possible, numeric (marching) for NURBS | 90% of machined parts are planes + cylinders; stability in tangent cases | 🟡 from the SolidCraft kernel (`kernel::exact`) |
| K12 | Fillets | "rolling ball": first the analytic cases (plane–plane → cylinder, plane–cylinder → torus), then general NURBS | the most used operation after Extrude | ⬜ Phase 2–3 |
| K13 | Parallelism | regeneration and tessellation of independent bodies/documents in parallel (`rayon`) | large assemblies | ⬜ Phase 3 |
| K14 | Heavy data | point clouds, meshes and textures as binary entries in the ZIP, loaded on demand | JSON is not for millions of points | ⬜ when needed |

---

## Phase 1 — Foundation (current)

| Task | Status |
|---|---|
| B-Rep kernel: geometry, topology, validation, box, tessellation | ✅ |
| Sketch solver: 10 constraints, DOF, redundancy, dragging | ✅ |
| 3D view (wgpu), camera, sketch overlay, demo sketches | ✅ |
| Inventor-like view (0.3): Orbit over the poles, ViewCube with a Home icon, compass, dragging and smooth transitions, axis triad ([spec](spec/0.3-view.md)) | 🟡 waiting for a visual check |
| `.3dcadcraft` file format: project, Part/Assembly, Link/Append/Pack | ✅ |
| Inventor-style interface: ribbon, model browser, properties, ViewCube, status bar, mouse and keys | ✅ shell |
| Undo/Redo with snapshots and gesture coalescing (0.4) | ✅ |
| Command enum for the API (0.4) | ⬜ |
| Parameters and expressions (0.5) | ⬜ |
| JSON-RPC/MCP server with the basic commands (0.7) | ⬜ |

**Acceptance criteria:**
- An agent over MCP creates a project and a sketch with constraints and saves a file that the GUI opens.
- Every action (from the GUI and from an agent) is undone with Ctrl+Z.

## Phase 2 — First real part (part MVP)

### Step 2.1 — Sketch on a face: plan and implementation ✅

Researched in Inventor's help and API (Start 2D Sketch, Edit Coordinate System, Redefine, Application Options → Sketch, `PlanarSketch` / `SketchOptions`).

| # | Task | How it is in Inventor | Implementation | Check |
|---|---|---|---|---|
| 1 | Persistent names of faces, edges, vertices | stable references in the kernel | topology `name`, `make_box_named`, `find_*`; immutable feature `id` | tests in `kernel::query` |
| 2 | Picking a face with the mouse | highlight on hover | `BRep::pick_face` (ray + point in polygon), `face_mesh` for the highlight | test + visual |
| 3 | Sketch definition | `PlanarEntity`, `OriginPoint`, `AxisEntity`, `AxisIsX`, `NaturalAxisDirection` | `part::SketchPlacement::resolve` | 4 tests |
| 4 | Automatic projection | `AutoProjectPartOrigin`, `AutomaticReferenceEdges` | fixed points + reference lines, `Projection` with a source by name | test + visual |
| 5 | Associativity | the sketch follows the face; missing source → freezes | `PartDocument::regenerate` → `update_sketches`, `health` | 3 tests + visual (height 20 → 65) |
| 6 | Redefine | right click → Redefine → new face | `redefine_sketch` (the 2D coordinates are kept) | test + visual |
| 7 | Edit Coordinate System | origin to a vertex, axis to an edge, Flip | `set_coordinate_system` (the 3D position is kept) | test |
| 8 | Look At on creation | `ParallelViewOnSketchCreationInPart` | `OrbitCamera::look_at` | visual |
| 9 | GUI workflow | ribbon, S, context menus, Finish/Edit, browser, properties, grid, axes, triad | `gui/` | visual (XTest scripts) |
| 10 | Undo/Redo | Ctrl+Z / Ctrl+Y | `command::History` | 3 tests + visual |

**Bugs found in the interactive test and fixed:**
- the XZ origin plane "covered" the front face; faces now win;
- dragging a point did not start on fast movement (it is now looked up at the press point);
- after Undo the ribbon stayed on the Sketch tab.

### Step 2.2 — Sketch tools, constraints, dimensions ✅

Compared with Inventor's help (Line, Circle, Arc, Rectangle, Slot, Polygon, Spline, Constrain, General Dimension, Driven Dimension).

| Area | Done |
|---|---|
| Line | chain with shared points, H/V while drawing (Ctrl turns it off), **arc on drag** (tangent or perpendicular depending on the direction) |
| Circle / Arc | Center Point (+Tangent if the radius lies on a curve), Tangent Circle (3 lines), Three Point, Tangent, Center Point |
| Shapes | Rectangle ×4, Slot ×5, Polygon (inscribed/circumscribed), with the same constraints as in Inventor; a test for the degrees of freedom and **0 redundant** |
| Spline | Control Vertex (Enter or double click) |
| Constraints | Coincident (point/curve), Collinear, Concentric, Fix, Parallel, Perpendicular, H/V (line or 2 points), Tangent, Symmetric, Equal; refusal on over-constraining |
| Dimensions | diameter/radius, distance depending on the placement (H/V/aligned), point–line, angle; Edit Dimension; **driven** on over-constraining |
| Format | Construction/Centerline/Normal, Construction mode, selection + Delete |

**Mathematical findings (with tests):**
- a tangency at a shared endpoint has a zero gradient, so a first-order formulation is used;
- a circumscribed polygon with an even number of sides is covered by Pitot's theorem;
- a centre arc in the arc slot would be redundant, so it is not created.

### Step 2.3 — Editing, degrees of freedom, Modify ✅

| Area | Done |
|---|---|
| Constraint editing | hover (highlights the geometry), selection, Delete, double click on a dimension → Edit Dimension, context menus (Delete, Edit, Driven/Driving), Delete Constraints on a curve, the list in Properties linked with the view |
| Degrees of freedom | null-space analysis for every point and curve, glyphs as in Inventor (DOF), green for every fully defined curve |
| Geometry | dragging whole curves, **Relax Mode**, context menu (Construction/Centerline/Normal, Fix, Delete) |
| Modify | Trim, Extend, Split, Offset (Loop Select, Constrain Offset), Move, Copy, Rotate, Mirror |

**Findings:**
- offset corners at smooth joins are a double root, so a Tangent is added and constraints are added only if they are independent;
- the offset of a line and of an arc have opposite signs for the same side, so they are compared by magnitude.

### Step 2.4 — Finishing the sketch ✅

| Area | Done |
|---|---|
| Snapping | 12 px to a point, 8 px to a curve; a dropped point gets attached (Coincident); line midpoint → Midpoint; ⊥ / ∥ inference relative to the previous line |
| Spline | Coincident point–spline (signed distance to the closest point) |
| Fillet / Chamfer | between two lines or at a corner; Equal to the first radius; a virtual corner keeps the dimensions |
| Patterns | Rectangular (1–2 directions) and Circular, associative: dimension of the first step, internal Translated/Rotated |
| Modify | Scale, Stretch |

**Bugs found and fixed:**
- Offset on a circle crashed the application, because the preview looked for the endpoints of a circle;
- Mirror of an arc gave a redundant constraint. SymmetricOnArc is used now, and the solver does not duplicate the equation for two halves of one circle;
- lines did not connect on a slightly inaccurate click, because the snap radius was 8 px.

| Profiles | regions with holes from lines, arcs, circles and splines; shading in the sketch |

**Project Geometry (1.8), 2026-10-20 (waiting for a visual check):** projection of points and curves from earlier sketches (also on another plane) and of circular model edges, associative (`part::link`), References with Break Link / Delete / Replace / Select, context menu. Along the way: Sweep of a circle along a spline (the path is made of points) failed in the kernel — the circle is now split into quarters.

**To do in Level 1:** Project Geometry — ellipses, faces, work features, Project Cut Edges; Ellipse, Text.

### Step 2.5 — Extrude (New Solid) ✅

| Area | Done |
|---|---|
| Kernel | `BRep::extrude_profile`: planes, cylinders and `ExtrusionSurface`; faces with holes; persistent names; test for volume, Euler and orientation |
| Part | `ExtrudeFeature` (sketch by immutable `id`, region by the curves of the outline), regeneration in history order, "sick" features |
| GUI | Extrude (E): picking regions by click, Distance and Direction, OK/Cancel; entry in the browser; editing and Delete |

**Added:** all Extrude options except the Boolean operations ([spec](spec/2.1-extrude.md)), live preview, consumed and shared sketches, feature specifications in `docs/spec/`.

### Step 2.6 — Parameters, tolerances, Revolve, Boolean operations (stage 1) ✅

| Area | Done |
|---|---|
| Parameters | expressions with units (`part/expr.rs`), Parameters table (model + user), names `d<N>`, renaming, `fx:` in the sketch ([spec](spec/0.5-parameters.md)) |
| Dimensions | live preview, a third click places, editing on the dimension itself, tolerances (Symmetric/Deviation/Limits/Max/Min/Basic, Evaluated size), display Value/Name/Expression/Tolerance, diametral dimension to a centerline |
| Revolve | axis from the sketch or X/Y/Z, Full/Angle/Symmetric/Asymmetric, Solid/Surface ([spec](spec/2.2-revolve.md)) |
| Booleans | Join/Cut/Intersect/New Solid in Extrude and Revolve: BSP CSG + assembly into a B-Rep with the face names ([spec](spec/2.13-booleans.md)) |
| Combine / Split | Combine (Join/Cut/Intersect, Keep Toolbody), Split Face / Trim Solid / Split Solid with a plane, a sketch (also open chains/splines for bodies) or an exact surface (Extrude/Revolve Surface); selection also from the browser; waiting for a visual check ([spec](spec/2.13-combine-split.md)) |
| Browser | empty part, history order, Solid Bodies / Surface Bodies ([spec](spec/2.16-browser.md)) |

| Exact kernel (stage A) | SolidCraft/truck behind `kernel::exact`: Box, Extrude, Revolve, Booleans, Fillet, Chamfer; names from the history; fallback to the faceted kernel ([SOLIDCRAFT.md](SOLIDCRAFT.md)) |

**Next:** stages B–F of [SOLIDCRAFT.md](SOLIDCRAFT.md) (STEP, Hole, Sweep, Loft…), reference tests (oracle), commands → MCP.

### Step 2.7 — Shell, Draft, patterns, Mirror, materials, view 🟡 (2026-10-19)

Implemented and covered by tests; **waiting for a visual check** in the application before the rows in the specifications become ✅.

| Area | Done | Number |
|---|---|---|
| Shell | Remove Faces (+ Automatic Face Chain), Inside / Outside / Center, Sharp / Rounded Corners, closed cavity, thickness with an expression and an arrow, breadcrumb, Edit Feature, preview ([spec](spec/2.6-shell.md)) | 2.6 🟡 |
| Face Draft | Fixed Edge, Fixed Plane, Parting Line, One Way / Symmetric / Asymmetric, Move Parting Line, Automatic Blending, Pull Direction from a face / plane / edge / axis, Flip, Face chain, angles with expressions, Edit Feature, preview ([spec](spec/2.8-draft.md)) | 2.8 🟡 |
| Guidance in commands | shared module `command::flow` + `gui/command_ui.rs`: selectors with colour/count/`x`, auto-advance, right click OK/Apply/Continue/Back/Cancel/Repeat + options, Enter = Continue/OK, Ctrl+Enter, hint next to the cursor; Face Draft and Shell ([0-ui §4.1](spec/0-ui.md)); the other commands — to do | 0.x 🟡 |
| Rectangular / Circular Pattern | features or bodies; Direction 1/2, Flip, Midplane, Incremental / Fitted; axis from an edge or an origin/work axis; one Boolean operation for all copies; Join / New Solid ([spec](spec/2.11-patterns.md)) | 2.11 🟡 |
| Mirror | features or bodies about an origin/work plane or a planar face; Join / New Solid / Remove Original ([spec](spec/2.12-mirror.md)) | 2.12 🟡 |
| Materials and appearance | CSV libraries + user folder, precedence Face > Feature > Body > Part > Material, QAT lists, Material/Appearance Browser, Adjust, Clear, Styles Editor (Update, Purge), transparency, colours in GLB/STEP ([spec](spec/2.15-materials.md)) | 2.15 🟡 |
| Physical Properties | mass per body and in total, volume, area, centre of gravity, inertia, principal moments and axes, Requested Accuracy, Update, Clipboard ([spec](spec/0.10-physical.md)) | 0.10 🟡 |
| Diagnostics | session log, action journal with replay (`cargo run --example replay`), autosave + Restore after a crash, crash report, Help → Save Diagnostics / Open Logs Folder ([spec](spec/0.14-diagnostics.md)) | 0.4 🟡 |
| View | Orbit over the pole, ViewCube (26 directions, arrows, Roll, compass, Home, menu), smooth transitions, axis triad bottom right ([spec](spec/0.3-view.md)) | 0.3 🟡 |

| Task | Number |
|---|---|
| Interactive drawing: line, circle, arc, rectangle, slot; snapping; automatic constraints | 1.2, 1.5 |
| More constraints: Equal, Concentric, Collinear, Symmetric, Midpoint; angle dimension | 1.3, 1.4 |
| Profiles and regions from a sketch | 1.9 |
| **Extrude**, **Revolve** (new body / Join / Cut) | 2.1, 2.2 |
| Boolean operations: planar + cylindrical | 2.13 |
| Hole (simple, counterbores, ISO thread), Slot — Hole 🟡 (2026-10-19: Simple/Clearance/Tapped, seats, CSV standards, modeled thread (2026-10-20: in slabs, M8 in a 283 mm plate 1.6 s instead of a refusal after 150 s; M3 in a 300 mm plate is now cut), note; [spec](spec/2.3-hole.md)), Thread 🟡 ([spec](spec/2.9-thread.md)), Slot ⬜ | 2.3, 2.9 |
| Chamfer; Fillet with a constant radius (analytic cases) | 2.4, 2.5 |
| Work planes and axes | 2.10 |
| Feature browser: editing, suppression, End of Part — 🟡 (2026-10-19: EOP, insertion at the marker, reordering, Suppress, Delete with options, Rename; [spec](spec/2.16-browser.md)) | 2.16 |
| Work planes, axes, points and UCS — 🟡 (2026-10-19: every Inventor 2027 method the model allows: tangent ones, torus/sphere, curve + point, ellipse, sketch points/lines, UCS, Redefine, Flip Normal; [spec](spec/2.10-work-features.md)) | 2.10 |
| Feature browser: editing, suppression, End of Part | 2.16 |
| Persistent naming v1 (names for the faces of Extrude/Revolve, derived from the IDs of the sketch elements) | 0.8 |
| Measurement, mass and centre of gravity (Physical Properties 🟡, Measure ⬜) | 0.10 |
| STL and STEP export (export only) | 7.7 |

**Acceptance criteria:**
- A bracket (an L-shaped plate with 4 M8 holes, chamfers and fillets) is modeled parametrically.
- Changing the thickness from 10 to 15 mm regenerates everything without breaking the fillets.
- The STEP file opens correctly in FreeCAD and Fusion.

## Phase 3 — Assemblies and drawings

**Decision (2026-10-23, spec [3.1-assemblies](spec/3.1-assemblies.md)):** constraints and joints run on
**one simultaneous solver** — 6 unknowns per non-grounded occurrence, every constraint a few
residuals, every joint a predefined set of the same residuals plus its driven values — built like
the sketch solver (sparse Levenberg, rank / DOF analysis). From SolidCraft (MIT/Apache) we take the
joint frame algebra, the snap origins (face centre, circle centre, point, frame; re-found by
persistent name), occurrence transforms, `world_bodies` and the URDF export — **not** its placement
solver (it walks the joint graph and only checks closed loops; Inventor solves loops). Steps and
rough cost per focused session:

| Step | Content | Rough cost | Status |
|---|---|---|---|
| A1 | `AssemblyDocument`, Place / Ground / Move / Rotate, browser, instanced display, picking in the occurrence's frame | ~250–350k tokens | 🟡 2026-10-23: done except GPU instancing (meshes are transformed per occurrence on a scene rebuild) and Free Rotate's handles; waiting for a visual check |
| A2 | Solver core + Mate / Flush / Angle / Insert, DOF per occurrence, sick relations | ~300–400k | 🟡 2026-10-23: done (Undirected Angle only, no DOF glyph); waiting for a visual check |
| A3 | Joints on the same solver, joint origins, limits, dragging | ~150–250k | 🟡 2026-10-24: done (no Automatic type, no Animate); waiting for a visual check. Next by the owner's order: BOM, then sheet metal, drawings, frames, standard components (research + specs first) |
| A4 | Tangent, Symmetry, limits while dragging, Drive / Animate | ~150–200k | ⬜ |
| A5 | Pattern Component, BOM, Interference, subassemblies | ~300k+ | ⬜ |

Before each step: verify its rows against the Inventor 2027 help (spec §7).

| Task | Number |
|---|---|
| Placing components, Joints (Rigid, Rotational, Slider), Mate/Flush/Insert | 3.1–3.3 |
| Dragging within the freedom, interference check | 3.4, 3.5 |
| BOM | 3.6 |
| Component patterns | 3.7 |
| Drawing: sheets, title block, ISO/ANSI; Base, Projected, Section, Detail views; HLR (🟡 2026-10-26: sheets, title block, Base / Projected / Section / Detail / Auxiliary views with HLR, dimensions, centre marks, centrelines, hole notes, texts, PDF; 2026-10-26 later: assembly views, Parts List, Balloon, Auto Balloon (spec 4.4); next: Break / Break Out / Crop, angular dimensions, tangent-edge option, DXF) | 4.1, 4.2 |
| Dimensions, hole notes, centerlines, Parts List + balloons; export to PDF and DXF | 4.3–4.5 |
| Patterns and mirror of features and bodies (🟡 done, waiting for a visual check — step 2.7) | 2.11, 2.12 |
| Home start page and one file per document (owner, 2026-10-26; specs [0.17](spec/0.17-home.md), [0.6a](spec/0.6a-file-types-projects.md)): H1 Home page + `FileType` / `DesignProject` (🟡 2026-10-26, waiting for the owner's check) → H2 Save As per kind (`.ccpart` / `.ccasm` / `.ccdrw`), Open filters, drawings / assemblies linking single files → H3 Projects editor, active project, per-project recent lists, workspace start folders → H4 Pack and Go to `.3dcadcraft`, `OldVersions`, Home commands with aliases, reduced ribbon on Home | 0.6, 0.17 |
| Many documents in one design file (owner, 2026-10-27; spec [0.6b](spec/0.6b-multi-document-files.md)): D1 interface (🟡 2026-10-27: Manage › Documents, the Documents window, browser menus, New / Duplicate / Rename / Delete in the file, Place *In This File*; D3 Save to Separate File / Embed / Embed All / Separate All with ids kept, Link File, Insert Document, document tabs of the file, Application Options › New documents; per-project default waits for H3; waiting for the owner's check) — Manage › Documents panel, Documents window, browser nodes, commands disabled → D2 New Part / Sheet Metal / Assembly / Drawing in this file, tabs sharing one file → D3 Save to Separate File, Embed, Pack, Unpack → D4 Create Component / Make Components (after derived parts) | 0.6 |

**Acceptance criteria:**
- A vise of 6 parts and standard bolts from a linked library is assembled.
- The movable jaw moves with the mouse up to its limits.
- The drawing with a section and a parts list is exported to PDF.
- After Pack the project opens on another machine without the library.

## Phase 4 — Advanced modeling and exchange

- STEP **import** (7.7).
- Sweep 🟡 and Loft 🟡 (2026-10-19: path from a sketch or edges, Guide Rail, Parallel, Taper, Twist; sections from sketches, faces and a point, holes; [spec](spec/5.1-sweep.md), [spec](spec/5.2-loft.md); Guide Surface, Solid Sweep to do; 2026-10-20: Loft with Rails, Center Line, Area Loft, Direction / Tangent conditions, Map Point and Closed Loop 🟡 — Smooth (G2), rounded apex, Surface to do), Coil 🟡 (2026-10-20: three methods, Taper, closed ends, Surface, Booleans through a faceted zone, thread with Cut; [spec](spec/5.3-coil.md)) (5.1–5.3).
- Materials and appearance 🟡 (2026-10-19: CSV libraries, overrides Face > Feature > Body > Part > Material, QAT lists, browsers, Adjust, Clear, Styles Editor, transparency, colours in GLB/STEP; [spec](spec/2.15-materials.md)) and Physical Properties 🟡 (mass, centre of gravity, inertia; [spec](spec/0.10-physical.md)) (2.15, 0.10).
- Shell 🟡 (2026-10-19: Inside/Outside/Center, Sharp/Rounded Corners, Remove Faces, closed cavity; [spec](spec/2.6-shell.md)), Face Draft 🟡 (2026-10-19: Fixed Edge, Fixed Plane, Parting Line, Symmetric/Asymmetric, Move Parting Line, Automatic Blending; [spec](spec/2.8-draft.md)), Rib / Web 🟡 (2026-10-20: Normal/Parallel, To Next/Finite, Extend Profile, Draft, bosses, webs; [spec](spec/2.7-rib.md)) (2.6–2.8).
- Rectangular / Circular Pattern and Mirror of features and bodies 🟡 (2026-10-19: copies of the Extrude/Revolve tools with one Boolean operation, bodies Join / New Solid / Remove Original; [spec](spec/2.11-patterns.md), [spec](spec/2.12-mirror.md)); Suppress of copies, along a curve, Sketch Driven to do.
- Shell 🟡 (2026-10-19: Inside/Outside/Center, Sharp/Rounded Corners, Remove Faces, closed cavity; [spec](spec/2.6-shell.md)), Face Draft 🟡 (2026-10-19: Fixed Edge, Fixed Plane, Parting Line, Symmetric/Asymmetric, Move Parting Line, Automatic Blending; [spec](spec/2.8-draft.md)), Rib (2.6–2.8).
- Rectangular / Circular Pattern and Mirror of features and bodies 🟡 (2026-10-19: copies of the Extrude/Revolve tools with one Boolean operation, bodies Join / New Solid / Remove Original; [spec](spec/2.11-patterns.md), [spec](spec/2.12-mirror.md)); Suppress of copies, along a curve, Sketch Driven to do. 2026-10-20: patterns with thousands of copies — 5000 holes in ~7 s, 10000 in ~14 s (kernel fixes: `vendor/solidcraft-kernel`, [PERF](PERF.md)).
- Presets in the command panels 🟡 (2026-10-19: Extrude, Revolve, Fillet, Chamfer, Shell, Face Draft; Last Used / No Preset / built-in; [spec](spec/0.11-presets.md)); ⬜ Hole, Pattern, tolerances in the sets, Hide Presets.
- Variable-radius Fillet: ✅ straight edges (2026-10-19, `fillet_variable_points`); ⬜ curved edges, NURBS cases.
- Extrude / Revolve per Inventor 2027 🟡 (2026-10-19, waiting for a visual check): breadcrumb (`gui/breadcrumb.rs`, shared by all panels), Predict Boolean Operation, Keep sketch visible on (+), "+", Ctrl+Enter, sketch dimensions during the preview, To up to work/origin planes and points, inclined and cylindrical limits (Alternate Solution), Extend face, Terminator, Match Shape for open profiles, Revolve To with Minimum Solution, Box / Cylinder / Sphere / Torus primitives ([spec](spec/2.1-extrude.md), [spec](spec/2.2-revolve.md)); ⬜ conical/spherical limits, Revolve To Next and Match Shape.
- **Sheet metal** with a flat pattern to DXF (5.6). 🟡 S1 (2026-10-25, waiting for a visual check): Sheet Metal Defaults (corner relief shapes), Face, Flange (Width Extents with drag arrows, preview), Convert to Sheet Metal of a solid or shell, Flat Pattern ([spec](spec/5.6-sheet-metal.md)); S2 (2026-10-26): Hem, Fold, Contour Flange, Cut, Unfold / Refold, DXF export; not in SolidCraft yet: Bend, Corner Seam, Rip, Punch Tool, Contour Roll, Lofted Flange. Laser-ready corners (2026-10-26): 2-bend reliefs about the crossing of the bend centre lines, rim corners torn or leaned back (no steps), bend relief on lips held back. Corner Round / Corner Chamfer of outside corners (2026-10-27, revision 30).
- Configurations and families (5.7, 3.9).
- Surfaces (5.4).
- Direct editing (5.5).

## Phase 5 — Specialized environments

- Frame Generator (6.1).
- Libraries of standard components (6.3).
- Bolted connections, shafts and gears (6.2).
- Weldments (6.4).
- Tube & pipe and cables (6.5, 6.6).

## Phase 6 — Analysis, visualization, automation

- FEA, linear and modal (7.1).
- Dynamics (7.2).
- Rendering (7.3).
- Presentations (7.4).
- Rules and scripts (7.5).
- Built-in AI assistant (7.6).
- MBD (7.10).
- Batch processing from the command line (7.9).

---

## Part and assembly interface review (2026-10-27)

The owner asked for a review of the part and assembly interface: what to improve and what is missing.
It was checked in the running app (a 12-component assembly, a part with 111 faces) and in the code.
Order agreed with the owner: **spec → interface (containers, buttons, dialogs with every option, OK
disabled) → logic, command by command** (rules 7, 14, 15, 17). The part command set is specified in
[spec/2.0-part-commands.md](spec/2.0-part-commands.md); the assembly command set gets its own spec
(3.0) in step AU0.

### P0 — defects found in the review

| # | Defect | Where | Status |
|---|---|---|---|
| P0.1 | `plabel(ui, "")` takes its drag-handle id from the text: the 23 empty labels collide ("First / Second use of widget ID" in Extrude › Advanced Properties) | `gui/panels.rs` `plabel` | 🟡 2026-10-27 fixed (the cell's own id); visual check of Extrude ⬜ |
| P0.2 | "Suppress if..." is listed twice in a feature's browser menu (the block is repeated) | `gui/panels.rs` browser menu | 🟡 2026-10-27 fixed; visual check ⬜ |
| P0.3 | Validation prints `genus Some(11)` (Debug format) and χ shows as "x" | `gui/panels.rs` Validation | ✅ 2026-10-27 "V - E + F = -20, genus 11" |
| P0.4 | Occurrence Properties: "Rotation Y −0 deg"; the label "Degrees of …" is cut off | `gui/assembly.rs` | 🟡 2026-10-27 fixed (round-off shows 0, label DOF); visual check ⬜ |
| P0.5 | A selected component is not highlighted in the view (click in the view or in the browser); browser and Properties follow | `assembly_scene` (scene not rebuilt on a selection change?) | ⬜ to confirm |
| P0.6 | Edit Feature (double-click) marks the document changed (`*`) before anything is changed | rollback | ⬜ |
| P0.7 | The hover tooltip of a face covers the mini-toolbar | `gui/mod.rs` | ⬜ |

### P — part environment

| Step | Content | Spec | Status |
|---|---|---|---|
| P1 | Spec of the whole part command set with every dialog | [2.0](spec/2.0-part-commands.md) | 🟡 2026-10-27 written; rows *(to confirm)* checked against the help before their logic |
| P2 | 🟡 2026-10-27 done, waiting for the owner's check (44 panels in `gui/part_planned.rs`, 20 disabled ribbon entries, own icons, aliases; ribbon panels collapse like Inventor's when the window is narrow). Interface of every missing command (rule 15): ribbon in Inventor's panels (Create: Emboss, Decal, Mark, Derive, Import, Unwrap, Shape Generator; Modify: Thicken/Offset, Delete Face, Direct, Copy Object, Move Bodies, Bend Part; Pattern: Sketch Driven; Surface: Stitch, Sculpt, Patch, Trim, Extend, Ruled Surface, Replace Face, Repair Bodies, Fit Mesh Face; Plastic Part; Freeform; Inspect › Analysis; Manage › Author / Insert / Layout; View › Appearance; Environments tab), each with alias, own icon, the dialog with selectors that pick, OK disabled — `gui/part_planned.rs` | 2.0 §3.1–3.8 | ⬜ |
| P3 | 🟡 2026-10-27 done (`gui/sel_menu.rs`; Find in Browser, Hide / Isolate, Show Dimensions ⬜), waiting for the owner's check. Right-click menus by object kind (face, cylinder, edge, vertex, body, surface body, sketch, work feature, feature, empty view), the command started with the selection as its first input (rule 17) | 2.0 §3.9, 0.15 G1 | ⬜ |
| P4 | 🟡 2026-10-27 done (`gui/view_style.rs`, renderer: edges of their own, depth-only faces, dashed hidden edges, two cut planes), waiting for the owner's check; rendered looks (Realistic…) and dragging the section plane ⬜. Visual Styles (Shaded, Shaded with Edges, Hidden Edges, Wireframe variants, Monochrome…) and Section View (Quarter / Half / Three Quarter / End) — the most used missing view commands | 0.3 | ⬜ |
| P5 | Browser: search / filter, Show Dimensions, Find in Window, Properties, Copy; Properties restyle (labels look disabled) | 2.16 | ⬜ |
| P6 | 🟡 2026-10-27: Thicken/Offset done ([spec 2.14](spec/2.14-thicken-offset.md), `Feature::BodyOp`, format revision 31; Optimized / Specify Tolerance, tested on eggs, sphere shells, surface lofts and sweeps — Sweep / Loft Output Surface added for it); next Delete Face (kernel `remove_faces`), Move Bodies, Copy Object (`copy_faces`), Stitch (`stitch`), Trim / Extend / Patch (`trim`, `extend`, `patch_edges`) as further `BodyOp` variants. Logic, in this order (each: spec rows → tests → visual check): Thicken/Offset → Delete Face → Move Bodies → Copy Object → Emboss / Mark → Sketch Driven Pattern → Replace Face → Stitch / Sculpt / Trim / Extend / Ruled Surface / Patch → Bend Part → Direct → Derive → Import (STEP) → analyses (Zebra, Draft, Curvature, Section) → Make Part / Make Components → iFeature / iPart → Plastic Part → Unwrap → Freeform / Shape Generator (blocked by their kernels) | 2.0 | ⬜ |

### AU — assembly environment

| Step | Content | Status |
|---|---|---|
| AU0 | Spec 3.0 of the whole assembly command set: Assemble (Component: Place, Place from Content Center, Place Imported CAD Files, Create, Replace, Pattern, Mirror, Copy, Shrinkwrap / Simplify; Position: Free Move, Free Rotate, Grounded; Relationships: Joint, Constrain, Show, Show Sick, Hide All; Work Features; Start 2D Sketch; Productivity: Make Layout; Manage: BOM, Parameters, Update), Design tab (Bolted Connection, Shaft, Spur Gear, Frame…), Environments tab (Tube & Pipe, Cable & Harness, Frame Generator, Dynamic Simulation, Stress Analysis, Presentation, Studio) | ⬜ |
| AU1 | Interface of all of them (rule 15), plus the Constrain panel brought to the shared selector rows (dot / count / `x`), type icons, Motion / Transitional / Constraint Set tabs (disabled), presets row | ⬜ |
| AU2 | Occurrence menu (rule 17): Constrain / Joint with the selection, Edit (in place), Replace, Pattern, Isolate / Undo Isolate, Transparent, Suppress, Enabled, Find in Browser, Measure, Hide Others / Show All | ⬜ |
| AU3 | Quick Access in an assembly: selection priority Component / Part / Feature / Faces and Edges instead of "Faces and Edges"; no Material list (parts carry materials); Appearance per occurrence | ⬜ |
| AU4 | Inspect in an assembly: Interference (3.5), Physical Properties of the assembly, Center of Gravity | ⬜ |
| AU5 | Browser: the part's features under an occurrence, visibility toggle, filter, Assembly / Modeling view; components listed under the assembly only | ⬜ |
| AU6 | View in an assembly: Visual Styles, Section View (P4 serves both), Representations (View, Positional, Model States — 3.9), DOF glyph | ⬜ |
| AU7 | Logic in the A-plan order: A4 (Drive / Animate rest) → A5 (Pattern Component, Interference, subassemblies) → Replace (3.8) → Edit in place / adaptive (3.10) → Representations (3.9) → Contact Solver (3.4); Mate point–point (vertex picking), Angle Directed / Explicit, Lock Rotation, automatic joint type, Free Rotate handles | ⬜ |

---

## Interface: gaps and next steps

Collected from [REFERENCE.md](REFERENCE.md) (the reference written from the code on 2026-10-19), the sections "Differences from Inventor" and "Differences from docs/UI.md". The numbers point to [FEATURES.md](FEATURES.md).

| # | Gap | What is needed | Number | Status |
|---|---|---|---|---|
| U1 | Settings are not kept between launches: mouse profile, wheel direction, the sketch options (autoproject, Look At, grid), inference, label display, the Home view. Only the language of the prompts and the recent files are kept | all Application Options in `settings.json` (additive, with `#[serde(default)]`) | 0.11 | ✅ code (`gui/layout.rs`, key `options`; waiting for a visual check) — together with the arrangement of panels and windows ([spec/0.13-ui-layout.md](spec/0.13-ui-layout.md)) |
| U2 | No key file, keys cannot be reassigned | `keymap` in `settings.json` + Tools → Customize → Keyboard — [spec/0.15](spec/0.15-context-menus-shortcuts.md) group G5 | 0.11 | ⬜ |
| U3 | F5 (Previous View) and Home (Zoom All) are not bound | a view stack for F5; Home = Fit / Zoom All | 0.3 | ⬜ |
| U4 | H (Hole) is not bound | comes with the Hole command | 2.3 | ⛔ Hole |
| U5 | Delete works only in a sketch; in 3D, features and work features are deleted only from the browser or Properties | Delete in the view on the selected features/work features (with the *Delete Feature* dialog) | 0.9, 2.16 | 🟡 |
| U6 | Classic profile: left drag orbits, so the selection rectangle, dragging a curve and dragging dimension text do not work | a modifier (e.g. Alt/Shift) for selection or dragging in Classic; or drag from an object = drag, from empty space = orbit | 0.3, 0.9 | 🟡 |
| U7 | The ribbon does not depend on the document type: always 6 tabs, Sketch is visible outside a sketch too | tabs per environment (part / assembly / drawing), Sketch only in a sketch | 3.1, 4.1 | ⬜ |
| U8 | Only the status-bar prompts are translated (`gui/i18n.rs`); menus, dialogs and errors are in English | translation files, language choice in Application Options | 0.11 | 🟡 |
| U9 | The suggestion bar next to a selected face/edge replaces Inventor's mini-toolbar, but there are no in-place values in the view (e.g. Distance in Extrude) | a floating field next to the manipulator with an expression field and the `>` menu | 2.1, 0.9 | 🟡 |
| U10 | Right click is an ordinary menu, not a marking menu; on a face/edge there is no Edit Feature (it is in the suggestion bar) | radial menu; context items for face/edge/body — [spec/0.15](spec/0.15-context-menus-shortcuts.md) groups G1, G4 | 0.9 | 🟡 |
| U11 | No Shift+right click for Select Other (it comes only after hovering still for ~0.7 s) | Shift+right click opens Select Other immediately | 0.9 | ⬜ |
| U10 | Right click is an ordinary menu, not a marking menu; on a face/edge there is no Edit Feature (it is in the suggestion bar) | radial menu; context items for face/edge/body | 0.9 | 🟡 |
| U11 | No Shift+right click for Select Other (it comes only after hovering still for ~0.7 s) | Shift+right click opens the Selection menu (Select Other, Loop, Tangencies, Invert, All, Previous, Clear, Window/Crossing, filters, priority) | 0.9 | 🟡 implemented ([0.9a](spec/0.9a-selection-filters.md)); Select Other inside a command ⬜; waiting for a visual check |
| U12 | The wheel zooms about the orbit centre, not about the cursor | option *Zoom towards cursor* (the default in Inventor) | 0.3 | ✅ 2026-10-27 *Zoom to cursor*, also Free / Constrained orbit, orbit about the point under the cursor, zoom out when a sketch / placed component leaves the view (`gui/nav.rs`) |
| U13 | No Navigation Bar (Pan / Zoom / Orbit / Look At buttons) | a bar in the view; then a check box in View → User Interface | 0.3 | ⬜ |
| U14 | SolidWorks and Fusion profiles | two more tables in `MousePreset` | 0.3 | ⬜ |
| U15 | Greyed-out commands in the ribbon: Measure, Visual Style, Section View, Document Settings, Box, Project Geometry, Ellipse, Text, Smooth (G2) | per their sections | 0.10, 0.3, 0.11, 2.1, 1.8, 1.2, 1.3 | ⬜ |
| U16 | One part per window; the other documents in the project are only visible in the browser | opening/activating another document, document tabs | 0.6 | ⬜ |
| U17 | Expression fields (`ExprValue`, feature fields) cannot be dragged with the mouse; only the *Values* of the sketch transformations can | drag or wheel over a field (optional) | 0.5 | ⬜ |
| U18 | *Measure* and *Show Dimensions* in the fields' `>` menu are greyed out | they come with Measure | 0.10 | ⛔ Measure |
| U19 | The visual check of step 2.7 (Shell, Draft, patterns, Mirror, materials, Physical Properties, view) | screenshots and ✅ on the rows of the specifications | 2.6, 2.8, 2.11, 2.12, 2.15, 0.10, 0.3 | 🟡 |
| U21 | Interface arrangement: dragging and remembering panels, collapsing the ribbon, View → User Interface, Clean Screen (Ctrl+0), Full Screen (F11), Reset UI Layout, floating windows with place and size | [spec/0.13-ui-layout.md](spec/0.13-ui-layout.md); remaining: the intermediate states of the collapsed ribbon, floating browser/Properties, a corner button when the browser is hidden | 0.11 | 🟡 waiting for a visual check |
| U22 | Right click does not depend on the object type (face, edge, body, sketch, work feature get one menu); the browser has no menus for the part, folders, Origin, surface body | context menus per an environment × object table ([spec/0.15](spec/0.15-context-menus-shortcuts.md) §3, groups G1–G3) | 0.4 | ⬜ |
| U23 | Enter/Space and *Repeat* repeat even while Fillet, Hole, Work, Sweep, Coil, Rib, Pattern, Thread is open; the letters (S, E, R, H…) do not check Alt (Alt+S starts Start 2D Sketch); Edit Feature on a Box in the suggestion bar does nothing | [spec/0.15](spec/0.15-context-menus-shortcuts.md) §9 group A0 | 0.4 | ⬜ |
| U24 | No command registry (name, alias, key, icon, ribbon path, Bulgarian name in one place) | `gui/commands.rs` — group G0; base for G4–G7 | 0.4 | ⬜ |
| U25 | No hint next to the cursor outside the commands with selectors; no alias field with autocomplete (Application Options → General → *Prompting interaction*) | group G6 | 0.11 | 🟡 |
| U26 | No command search | a field in the Quick Access, **Alt+S** — group G7 | 0.4 | ⬜ |
| U27 | No F5/Shift+F5, Home/End/PageUp, Zoom Window, Isolate, Object Visibility by type (Alt+], Ctrl+], Alt+V) | group G8 (takes over U3, U12) | 0.3 | ⬜ |
| U28 | Tooltips are short only; no Undo list; no Annotation scale; no Show Dimensions on a feature | groups G9, G10 | 0.4, 1.4 | ⬜ |
| U20 | The comment in `src/gui/mod.rs` lists `CADCRAFT3D_DEMO=rectangle\|slot\|tangents\|lines` without `revolve` | a small fix in the code | — | ⬜ |

**Plan for the right button, the keys and the conveniences** ([spec/0.15](spec/0.15-context-menus-shortcuts.md) §9; one agent per group): **A0** fixes (Repeat/Enter with open commands, Alt with the letters, Box Edit Feature, Ctrl+Enter everywhere) → **G0** command registry (`gui/commands.rs`, fuzzy matching, recent commands) → in parallel **G1** menus in the view by object type and the menu of every command, **G2** browser (menus, Find in Window, folders), **G3** sketch (menus, Back/Restart, changing the dimension type), **G6** hint next to the cursor + alias field (Application Options → General → *Prompting interaction*), **G7** command search (Alt+S, Quick Access), **G8** view and visibility (F5, Home/End/PageUp, Isolate, Object Visibility) → in parallel **G4** marking menu, **G5** Customize → Keyboard, **G9** tooltips/Undo list/Annotation scale, **G10** Show Dimensions. The selection menus (Shift+right click, filters) are in 0.9 (separate work).

---

## Obstacles for assemblies that are solved already in the part

Assemblies (Phase 3) build on the part data. The following decisions are followed **already now**, so nothing has to be reworked later. They are also written as rules in `CLAUDE.md`.

| # | Obstacle | Solution | Status |
|---|---|---|---|
| A1 | Assembly constraints point to faces and edges of parts; on regeneration the IDs change | references are **by persistent name** + path to the component (`Bolt:3 / Box1:Top`), never by ID or index | ✅ for the part (names v0) |
| A2 | Renaming a feature breaks all references | the feature has an **immutable `id`** (prefix of the names) and a separate display name | ✅ |
| A3 | One part is placed N times | `PartDocument` regenerates only from its own features, deterministically; the result is cached and used for all occurrences | ✅ (regeneration is pure) |
| A4 | Per-occurrence data (placement, visibility, colour, positional reps) | lives in the **assembly** (`Occurrence`), not in the part | ✅ in the data model |
| A5 | Coordinates | the part is in its own coordinates; the transformation belongs to the occurrence; picking in an assembly transforms the ray into local coordinates (`pick_face` takes a local ray) | ✅ the API is ready |
| A6 | Rendering many occurrences | today the geometry is written in world coordinates; **instancing** is needed (one mesh + a transformation per occurrence) | ⬜ Phase 3 |
| A7 | Sketch/feature in the context of the assembly (adaptivity, in-place) | new explicit reference variants (e.g. `PlaneRef::External{path, name}`) with visible dependencies and a cycle check; no hidden lookups | ⬜ Phase 3 (the enum is extensible) |
| A8 | A missing face after a change of a linked part | the same "health" mechanism as for sketches: the constraint becomes "sick" with a clear message, no silent fixes | 🟡 the model is ready for sketches |
| A9 | Undo in a large assembly | snapshots of the whole project become expensive, so deltas/commands are needed (0.4, Command enum) | ⬜ |
| A10 | Mass and centre of gravity of the assembly | the part's properties × the occurrence's transformation | ⬜ (0.10) |
| A11 | A part from a library (Link) changes | regeneration on load; "health" for lost references; later Link to a version | 🟡 (Link/Append/Pack ✅) |

## Usage plan (workflows)

How 3DCadCraft is used at each stage. The steps marked ⬜ become possible with the phase given.

### A. Part (Phase 2)

1. **New → Part.** The project `bracket.3dcadcraft` gets a document "Part1".
2. **Start 2D Sketch (S)** on the XY plane.
3. You draw a rectangle (R) and a circle (C). The program itself suggests H/V and coincidences.
4. **Constraints and dimensions (D):** 120 × 80, the hole is 20 mm from the edges. The sketch turns **green** (0 degrees of freedom). Blue means something is missing, and orange points to a redundant constraint.
5. **Finish Sketch → Extrude (E):** 10 mm, New Solid.
6. **Hole (H):** M8 tapped, 4 pcs. at the center points.
7. **Fillet:** R5 on the vertical edges. **Chamfer:** 1 mm.
8. **Parameters (fx):** `thickness = 10`. Changing it to 15 regenerates the model.
9. **Save (Ctrl+S).** The previous version stays as `.bak`.

### B. Assembly with reuse (Phase 3)

1. The company library `company-standard.3dcadcraft` is in a shared folder.
2. **New → Assembly**, **Place (P)**: the housing from the same project (grounded), the bolts **Link**ed from the library.
3. **Joints / Constrain (C):** Insert of the bolts into the holes, Rotational for the lid.
4. **Inspect → Interference:** no interferences.
5. **BOM:** 1 housing, 1 lid, 4 M8×20 bolts from the library.

### C. Drawing and hand-over (Phase 3)

1. **New → Drawing** in the same project. Base view of the assembly, Projected views, Section A–A.
2. **Parts List** + balloons, title block with data from the properties.
3. **Export → PDF** for production.
4. **Pack** → `bracket-for-supplier.3dcadcraft`, a single self-contained file without links to the company library, for sending.

### D. Working with git (already possible)

1. **Save as folder** → `bracket.3dcadcraft/` with `manifest.json` and `documents/*.json`.
2. `git add`, `git commit`. Every change to one part shows up as a diff only in its own file.
3. For sending: **Open** the folder → **Save** as a single file.

### E. AI agent (Phase 1, MCP)

1. The agent (Claude or another MCP client) connects to `3dcadcraft --mcp`.
2. "Make a 120×80×10 plate with 4 M8 holes 20 mm from the edges."
3. The agent calls `create_sketch`, `add_rectangle`, `add_constraint`, `extrude`, `hole` and gets the IDs and the solver reports.
4. The user sees the result in the GUI. Every step of the agent is undone with Ctrl+Z.

---

## How progress is checked

- `cargo test --no-default-features`: the tests of the kernel, the solver, the part and the files (122 now; 142 with the GUI tests).
- Interactive GUI tests: `tools/gui_launch.sh` + `tools/gui_sim.py` (click, drag, keys, screenshots); see `CLAUDE.md`.
- `cargo clippy --all-targets`: no warnings.
- **Visually:**
  - the demo sketches (`CADCRAFT3D_DEMO=…`) and the example projects (`cargo run --example make_project`);
  - red faces mean an orientation error;
  - the sketch colours show the solver state.
- Every new feature comes with a test **and** with a way to see it in the application.
