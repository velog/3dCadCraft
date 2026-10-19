# CLAUDE.md — working on 3DCadCraft

Guide for Claude (and humans) developing this repository. **Keep it current:** when
you add a module, change a rule, hit a non-obvious pitfall or finish a roadmap step,
update this file in the same change. Short and true beats long and stale.

## Project in one paragraph

3DCadCraft is an open-source parametric 3D CAD system in pure Rust (alternative to
Autodesk Inventor). Clean-room B-Rep kernel, 2D constraint solver, single-file
projects (`.3dcadcraft`), egui + wgpu desktop UI that mirrors Inventor's layout for
easy migration, and (planned) a JSON-RPC/MCP API so AI agents can drive it headless.
Inventor 2027 is the **functional** reference; never copy Autodesk text, code, icons
or file formats.

## Communication

- The project owner writes in **Bulgarian** — answer in Bulgarian.
- Code, identifiers, comments and rustdoc: **English**. Files in `docs/`: **Bulgarian**.
- The owner validates work **visually in the running app**, not only by tests: every
  user-facing feature must be reachable in the GUI and checked there (see
  [Visual testing](#visual-testing)).

## Commands

```bash
cargo build                                   # GUI binary target/debug/3dcadcraft
cargo test --no-default-features              # all tests, no graphics stack (fast)
cargo test                                    # same + GUI-module tests
cargo clippy --all-targets                    # must be warning-free
cargo fmt                                     # rustfmt defaults (run before finishing)
cargo doc --no-default-features               # must be warning-free
cargo xtask ci                                # every check above, in order (run before finishing)
cargo xtask oracle                            # reference cases table (tests/oracle/)
cargo run -- path/to/project.3dcadcraft       # open a project
CADCRAFT3D_DEMO=slot cargo run                # start with a demo sketch (rectangle|slot|tangents|lines|revolve)
cargo run --example make_project -- /tmp/b.3dcadcraft   # build a project via the API
```

Names: product **3DCadCraft**, binary `3dcadcraft`, Rust package/lib **`cadcraft3d`**
(Cargo forbids a leading digit), project files `*.3dcadcraft`.

## Architecture (src/)

| Module | Responsibility | Key types |
|---|---|---|
| `kernel` | B-Rep geometry + topology, validation, tessellation (`mesh.rs`), queries & persistent names (`query.rs`), extrude, revolve (`revolve.rs`), BSP CSG on tagged polygons (`csg.rs`), polygons ↔ B-Rep and Booleans (`facet.rs`), exact kernel boundary (`exact.rs`: SolidCraft/truck → our B-Rep) | `BRep`, `Curve`, `Surface`, `Plane`, `Face`, `Tolerance`, `BoolOp` |
| `sketch` | 2D sketch on a plane (dimension names/equations/tolerances in `dimparam.rs`): points, lines, circles, arcs, CV splines, styles (construction/centerline), ~20 constraint/dimension kinds, driven dims, Levenberg solver with DOF/redundancy analysis; composite shapes (`shapes.rs`), Modify ops (`modify.rs`: trim/extend/split/offset/move/copy/rotate/mirror/scale/stretch), 2D fillet/chamfer (`corner.rs`), associative rectangular/circular patterns (`pattern.rs`), profiles/regions (`profile.rs`: `Sketch::regions`), per-geometry DOF analysis (`dof_analysis`) | `Sketch`, `Constraint`, `Shape`, `SolveReport`, `DofAnalysis` |
| `part` | part environment: features → body (Box, Extrude `extrude.rs`, Revolve `revolve.rs`, Shell `shell.rs`; `boolean.rs`: Operation/Output applied at regeneration), sketches with placement (plane + coordinate system), projections, associativity, Redefine, Edit Coordinate System; parameters (`expr.rs` unit-aware expressions, `params.rs` model/user parameter table) | `PartDocument`, `PartSketch`, `ExtrudeFeature`, `RevolveFeature`, `Operation`, `UserParam` |
| `part::export` | File → Export: binary STL, STEP (from `PartDocument::exact_bodies` via `kernel::exact::step_text`), GLB; `examples/export.rs` CLI | `ExportFormat`, `ExportMesh` |
| `document` | projects with many documents, `.3dcadcraft` ZIP/folder format, Link/Append/Pack | `Project`, `DocumentData`, `Resolver` |
| `command` | Undo/Redo (snapshot history, coalesced gestures); later the Command enum | `History<T>` |
| `api` | JSON-RPC/MCP server — placeholder | |
| `gui` (feature `gui`) | Rebuild All (`rebuild_all`, QAT + Tools), Fillet/Chamfer select modes and types (`blend.rs`); selection modes and multi-select in the idle view (`select.rs`: `SelectMode`, `SelItem`, box/tangencies/invert/previous; part-side helpers in `part/select.rs`); Inventor-like shell: ribbon (large/small/icon-only buttons), vector icons (`icons.rs`), expression fields with the `>` menu (`valuefield.rs`), feature manipulators (`manip.rs`: distance arrows, taper grip, revolve angle arc), sketch pattern command dialog (`pattern_dialog.rs`: selectors, preview, OK/Cancel), browser, properties, ViewCube, viewport, overlays; sketch tool state machine (`sketch_tools.rs`: Line/Circle/Arc/Rectangle/Slot/Polygon/Spline/Point, constraint tools, General Dimension) | `CadCraftApp`, `SketchTools` |

Data flow: GUI action → `checkpoint(label)` (Undo snapshot) → `part::`/`sketch::`
operation on the active `PartDocument` inside `Project` → `regenerate()` if the body
changed (rebuilds body, re-resolves every sketch) → `scene_dirty = true`.

## Rules (invariants — do not break)

1. **No `unsafe`** (`forbid` at crate level). Numbers are `f64`, mm and radians.
   Compare geometry only through `Tolerance`, never with `==`.
2. **Reference model topology by persistent name, never by arena ID or index.** IDs
   change on every regeneration. Faces/edges/vertices carry `name: Option<String>`
   (`"<feature id>:<role>"`, e.g. `Box1:Top`, `Box1:V101`, `Box1:E000-100`).
3. **Feature identity ≠ display name.** A feature's `id` is immutable and is the
   topology-name prefix; `name` is free to rename. Never derive names from `name`.
4. **Every user mutation records an Undo checkpoint first** (`CadCraftApp::checkpoint`).
   Continuous gestures use the same label so they coalesce into one step; call
   `history.break_coalescing()` when a gesture ends.
5. **Failure leaves state unchanged.** Solver failure reverts the sketch; an
   unresolvable placement sets `PartSketch::health` and keeps the last good plane
   (Inventor: sick sketch / frozen projections). Never silently "fix" user data.
6. **File format is additive.** New fields get `#[serde(default)]` (+
   `skip_serializing_if` where sensible). Breaking changes bump `FORMAT_VERSION` and
   add a migration in `document::format::load_with`. Something old builds cannot read
   (a new enum variant) bumps `FORMAT_REVISION` and adds fixtures under
   `tests/compat/r<N>/` (`CADCRAFT_WRITE_COMPAT=1 cargo test --test compat`); never
   edit existing fixtures or `tests/compat/expected.json`. Saves must stay atomic and
   deterministic; `serde_json` keeps `float_roundtrip`.
7. **Ribbon lists the full Inventor command set.** Unimplemented commands stay
   disabled with the `docs/FEATURES.md` section in the tooltip; implemented ones get
   an `Action`. Keep Inventor's command names (English).
8. **Assembly-ready part data** (assemblies come next; don't block them):
   - a `PartDocument` must regenerate from its own features only — deterministic,
     no dependency on where/how often it is placed (it is shared by N occurrences);
   - no per-occurrence state in the part (placement, per-instance visibility/colour,
     positional reps belong to the assembly);
   - everything in part-local coordinates; assembly code transforms (e.g. picking
     transforms the ray into the occurrence's local frame — `BRep::pick_face` takes
     a local ray);
   - assembly references will be `(occurrence path, persistent name)`; cross-part
     (adaptive/in-place) references must be explicit new `PlaneRef`/ref variants with
     visible dependencies, never hidden lookups;
   - a missing referenced face/edge must surface as "health", like sketches do.
9. **Docs move with code.** Update statuses in `docs/FEATURES.md`, progress in
   `docs/ROADMAP.md`, shortcuts/UI in `docs/UI.md`, and this file.
10. **Every feature has a spec** in `docs/spec/<section>-<name>.md` (template in
    `docs/spec/README.md`): Inventor's behaviour option by option, in our own words
    (never copied Autodesk text), with our status per option. Write it *before*
    implementing; keep the statuses true (✅ only after a visual check).
11. **UI requirements** (look, colours, glyphs, command lifecycle, picking priorities,
    status-bar prompts, error/warning wording, previews) are in
    `docs/spec/0-ui.md`. New commands must meet them.
12. **Preview + manipulators + expression fields.** Every command that can have them
    gets a live preview and drag handles in the viewport (arrow for distances, arc
    for angles) that edit the same value as Properties; every numeric field accepts
    expressions and parameter names, with the `>` menu (`docs/spec/0-ui.md` §2.7–2.8).

## Exact kernel (SolidCraft / truck) — see `docs/SOLIDCRAFT.md`

- Geometry is moving to `solidcraft-kernel` (git dependency pinned in `Cargo.toml`, MIT OR
  Apache-2.0, on truck). It is reached **only** through `kernel::exact`; no truck/solidcraft
  type leaves that module. Every call goes through a guard (panic → error).
- Our persistent names are built from its exact history (`FaceOrigin`), never from indices.
- Anything adapted from another project gets a row in `ATTRIBUTION.md`. Never read or copy
  GPL/LGPL code (OpenCASCADE, FreeCAD, SolveSpace…); never take data from commercial CAD.
- No `unwrap`/`expect`/`panic`/`unreachable!`/`todo!` in non-test code — enforced by `#![deny(...)]` in `src/lib.rs` (`clippy.toml` allows them in tests); only `gui/demos.rs` has a reasoned allow. Tests use `panic!` instead of `unreachable!` (clippy has no in-tests exemption for it).
- Regeneration (`part/exact.rs`): exact first; `Fail::User` → feature health, `Fail::Kernel`
  or an unsupported option (`Ok(None)`) → the **whole part** is rebuilt faceted (the faceted
  CSG cannot work on a converted exact body). `PartDocument::exact` tells which ran.
- `kernel::exact::to_brep` stores the kernel's triangles in `Face::mesh` (display/picking);
  seams (a face meeting itself) are not drawn. Blend faces are named after the replaced
  edge (`Fillet1:Round[A|B]`); `blend::face_matcher` finds old faceted names by feature.
- **Reference tests** (`tests/oracle/`, from SolidCraft): never edit `expected.json` to match
  the program; a case that cannot pass yet goes into `known_failures.txt` with the reason,
  and a known failure that starts passing fails the run. Every crash or geometry fix lands
  with a regression test. `tests/boolean_identity.rs` checks Boolean volume identities.
- Report geometry changes with measured vs expected values (volume, area, faces) and,
  for visible behaviour, screenshots taken by the agent itself.
- **Edit Feature** = `PartDocument::rollback` (build only features before k; the edited
  feature's own sketch follows that body, later sketches keep their state) +
  `replace_feature` (keeps id/name/sketch; a failing edit restores the old one). Drafts carry
  `edit: Option<usize>`; Cancel/Esc call `end_feature_edit`. Previews run on a worker thread
  (`preview_async`, `PREVIEW_PENDING`) — never compute kernel work in the frame.
- Blends never fall back to the faceted kernel (it froze the UI 15–20 s and could overflow
  the stack); chain: kernel → `loop_blend` → `wedge_blend` (chamfers of straight edges,
  Cut convex / Join concave) → feature error. Nearest-edge lookups must measure distance
  to edge *segments* (a straight edge split by a Boolean has 2 pieces ending at the same
  point; `edge_points`), and split pieces are joined (one kernel point / one prism).
- **Regeneration cache** (`part::BuildCache`, shared by clones): a step's key is the previous
  key + the feature (JSON) + its sketch's *geometry* (`ps.sketch`, never the `PartSketch` —
  its visibility flips after `add_extrude` and made every key miss). The feature's own sketch
  is updated *before* the key is taken. Work features are not cached.
- **Many profiles in one cut**: `exact::union_apart` joins tools apart from each other into one
  (SolidCraft's fast path); never cut a body once per tool (36 holes took 30 s).
- **Regions:** coincident pieces (a curve on a projected edge, squares on the plate outline)
  are deduplicated in `Sketch::regions`; two edges between the same vertices lost regions.
- **Circular pattern through its centre**: `copy_geometry_keeping(.., Some(center))` keeps the
  centre shared and `tie_copy` skips points a copy shares.
- **Every numeric field takes expressions** (rule 12): feature fields via `field_row`
  (equations stay with the feature, also in drafts via `apply_draft_field`); plain tool
  values via `valuefield::ExprValue` (drop-in for `DragValue`, unit from the suffix,
  parameters from `set_frame_table`). Box sizes are parameters (`length/width/height`).
- A chamfer larger than an arc of its loop: the kernel's chamfer fails; `loop_blend` cuts
  wedges along the extended lines (sharp corners). A tool face must never end on an edge of
  the result (it left sliver faces) and a cone tangent to a plane breaks truck's Booleans —
  hence `cone_corners` (cone variant) still reports "not supported yet".

## Feature workflow (mandatory, in this order)

Every feature — new or a fix to an existing one — goes **spec → code → proof**.
Never implement from memory of "how Inventor probably does it".

### 1. Specify: `docs/spec/<section>-<name>.md`
- **Where:** the section number comes from `docs/FEATURES.md`, e.g. `2.2-revolve.md`,
  `1.4-dimensions.md`. UI-wide rules are in `0-ui.md`. One file per feature or
  mechanism. Add it to the table in `docs/spec/README.md` and link it from the
  FEATURES section (`**Спецификация:** [spec/…](spec/…)`).
- **Hard algorithms** (blends at corners, Booleans, offsets, shell…): before inventing one,
  look up how others solve it — SolidCraft source (`~/.cargo/git/checkouts/solidcraft-*`,
  MIT/Apache, adapt with attribution), FreeCAD docs/wiki and FreeCAD run as an external
  oracle, papers. Never read GPL/LGPL source (FreeCAD, OpenCASCADE, SolveSpace).
- **Research:** Inventor Help **2027** first. Readable pages are
  `https://help.autodesk.com/cloudhelp/2027/ENU/Inventor-Help/files/GUID-….htm`;
  the viewer at `help.autodesk.com/view/INVNTOR/2027/ENU/` is JavaScript and not
  fetchable. Find GUIDs by web search (older years share the GUIDs), then fetch them
  with the 2027 path. Use Inventor Help (help.autodesk.com `Inventor-Help`, newest
  year first)
  and the API object model (`Inventor-API`). Use WebSearch/WebFetch; note the GUIDs
  in "Източници".
- **Content** (template in `docs/spec/README.md`), in Bulgarian and in our own words.
  Never copy Autodesk text. Option and command names stay in English.
  1. purpose;
  2. inputs;
  3. **every option** in a table: Inventor behaviour · restrictions · our status;
  4. browser/editing behaviour;
  5. edge cases and errors;
  6. implementation (modules, topology names);
  7. sources.
- **Statuses:** ✅ done (only after a visual check), 🟡 partial (say what is
  missing), ⬜ to do, ⛔ blocked by another feature (say which). A new spec starts
  with everything ⬜.

### 2. Implement against the spec
- Go through the spec table **row by row**. Each row you implement must match the
  described behaviour: click order, defaults, what gets constrained or named, errors.
  If you deviate on purpose, write the deviation into the row (🟡 + why).
- **Data model** in the right layer (`kernel` geometry, `sketch`, `part`, `document`):
  serde-compatible and additive (rule 6). Respect rules 1–8.
- **Unit tests first** (`cargo test --no-default-features`), one per spec row group.
  Prove the facts that can break silently: DOF and `redundant == 0` for sketch
  builders; volume, Euler and orientation for solids; persistent names; save/load
  round trip; "failure leaves state unchanged".
- **GUI** per `0-ui.md`:
  - ribbon command with an icon (`gui/ribbon.rs`, `gui/icons.rs`); the panel must
    still fit 1400 px;
  - status-bar prompt for every step;
  - preview where possible;
  - options in Properties;
  - Undo checkpoint (and `history.cancel()` on failure);
  - errors as `<Command>: <reason>`;
  - F1 mapping in `CadCraftApp::open_help`.

### 3. Prove
- `cargo fmt && cargo clippy --all-targets && cargo test`: zero warnings, all green.
- **Speed:** every new feature adds a case with a budget to `tests/perf.rs`
  (`docs/PERF.md` has the targets and the log). Over budget → look for a faster way
  (how others do it first), never just raise the number.
- **Visual test of every implemented row** in the running app (recipe below):
  screenshot, read it, fix, repeat. Only then flip the row to ✅.
- Update the spec statuses, `docs/FEATURES.md`, `docs/ROADMAP.md`, `docs/UI.md`
  (keys) and this file (modules, pitfalls) **in the same change** (rule 9).

## Visual testing

Real interaction can be scripted on this machine (X11, `DISPLAY=:0`, python3-xlib,
ImageMagick):

```bash
cargo build
WID=$(tools/gui_launch.sh CADCRAFT3D_DEMO=slot)          # or: tools/gui_launch.sh -- file.3dcadcraft
tools/gui_sim.py $WID shot /tmp/s0.png                   # look at it, read coordinates
tools/gui_sim.py $WID key s move 705 385 click 705 385 shot /tmp/s1.png
pkill -x 3dcadcraft                                      # NEVER `pkill -f` (kills your shell)
```

Default window 1400×900: ribbon y≈60–90, browser x<250, properties x>1100,
viewport ≈ (250..1100, 110..875). Coordinates shift with layout — always take a
screenshot and measure first (crop + `-scale 300%` to locate small dots).

## Pitfalls already paid for

- **Tangency at a shared endpoint** (line→arc chains, slots): "distance(centre,
  line) = r" has a *zero gradient* there (double root) → the solver can't see it,
  reports redundancy and converges slowly. `Constraint::Tangent` detects a shared
  endpoint — or ends joined by a `Coincident` constraint (`joined_endpoints`; found by
  oracle case 64, which stalled at 7e-10) — and uses the first-order form (radius ⟂
  line / centres collinear with the point). Tools should still **share** point ids for
  chains and tangent arcs.
- **Redundancy in composite shapes:** every builder in `sketch/shapes.rs` has a test
  asserting exact DOF *and* `redundant == 0`. Known traps: circumscribed polygon with
  even N (Pitot — drop one Equal), arc slot (a construction centre arc would be
  implied by the 4 tangencies — use radial lines), centre rectangle (one diagonal
  midpoint suffices).
- **New constraint kinds:** implement residual via the `Geo` trait in
  `sketch/constraint.rs` (shared by solver and `measure`), and update `validate`,
  `references_point/entity`, `name`, `value(_mut)` and the overlay labels.
- **Offset corners at smooth joins** are double roots too (offset line *touches* the
  offset arc). `Sketch::offset` adds Tangent at those joins and every further
  constraint via `add_if_independent` (rank check) — reuse that for any builder whose
  independent set depends on the geometry.
- **Snap radii** live in `gui/overlay.rs` (`POINT_SNAP_PX`, `CURVE_SNAP_PX`) —
  use them, never literal pixels. At 8 px users missed endpoints and got
  unconnected lines. Dropping a dragged point attaches it
  (`CadCraftApp::attach_dropped_point`); its own curves are excluded from the targets.
- **`Sketch::chain` on a circle/spline** returns just that curve. Before that fix,
  the Offset hover preview indexed a circle's missing endpoints and crashed the app.
  Any helper that walks endpoints must handle closed curves.
- **Point on spline** (`PointOnSpline`) is a *signed* distance at the closest
  point (`constraint::spline_closest`), with no extra solver unknown. `|d|` has a zero
  gradient at 0. Use `Sketch::on_curve` to build any point-on-curve constraint;
  it handles all kinds and refuses a curve's own defining points.
- **Fillet/chamfer corners:** an old corner point that has constraints (dimensions,
  Fix) is kept as a virtual sharp with PointOnLine on both lines. Deleting it silently
  dropped the user's dimensions (seen in the GUI test).
- **Copies of arcs are over-constrained by default.** Each arc carries an implicit
  "end on the circle" equation, so tying all 3 arc points of a copy (pattern, and
  any future "copy tied to original") adds 1 redundant equation per arc. Tie one end
  tangentially only (`arc_center` in Translated/Rotated, see `Sketch::pattern_ties`).
- **Regeneration order:** features are built in history order, and an Extrude
  first re-resolves *its* sketch against the body built so far (`update_sketch`).
  A failing feature goes to `PartDocument::feature_health` and is skipped, so the
  part still builds. `add_extrude` refuses (and rolls back) a feature that fails.
- **Sketches have an immutable `id`** (features reference it); old files get it
  from the name in `regenerate`.
- **Consumed vs shared sketches:** a sketch is consumed if a feature references it
  (`PartDocument::consumers`). It is listed under its first feature and hidden,
  unless `PartSketch::shared` is set. Reusing a consumed sketch shares it
  (`add_extrude`). `owner_of_sketch` decides where the browser puts it.
- **Extrude levels:** `kernel::ExtrudeLevel` (offset + inset) describes any
  extrusion. Two levels give a plain or tapered extrusion; a two-sided taper adds a
  kink level at the start plane. Taper insets offset *every loop to its right*
  (outer CCW grows, holes CW shrink). A cone's rails sample differently, so ruled
  faces are tessellated with a forced common segment count.
- **Ribbon width:** all panels of a tab must fit the default 1400 px window
  (screenshot after adding commands). Use `.mini()` (icon only, name in the tooltip)
  for rarely used toggles before adding columns. New commands need an `Icon` in
  `gui/icons.rs`: draw it on the 16×16 grid, with `ink` + `accent`.
- **Inference settings** (`SketchTools::inference`) gate the snaps in `sketch_pick`
  and the H/V/∥/⊥ inference. Without *Persist*, `SketchTools::click` strips the
  on-curve/midpoint snaps before the tools see them.
- **Dimension text** is stored per dimension as an offset from
  `Sketch::dimension_anchor` (`dim_labels`), so it follows the geometry. Lines,
  arrows and arcs are drawn in `overlay::draw_dimension` from the label position.
  A press on a dimension's text drags the text (`dragging_dim`), never the
  geometry.
- **Sketch patterns are objects** (`Sketch::patterns`, `SketchPattern`): they record
  their source, instances, helper lines and constraints. `edit_pattern` rebuilds on a
  clone and keeps the id and suppressions. Copies cannot be deleted one by one (use
  Suppress / Delete Pattern). Geometry-picking tools use `POINT_PICK_PX` (6 px), so
  a small circle is pickable by its curve and not only by its centre.
- **Angle dimensions measure line directions.** The supplement (180° − θ, chosen by
  the placement quadrant) is *not* just another value: the residual wraps by π.
  It needs `Angle { supplement: true }` (line `b` reversed).
- **Tests on free geometry:** the min-norm solver moves *every* free point, so
  assert relative positions/angles, not absolute ones.
- **Signs of "offset":** a line's signed distance and an arc's radius change have
  opposite signs for the same side of a chain; `EqualOffset` compares magnitudes.
- **Status-bar `right_to_left` layouts reverse widget order** — read coordinates off a
  screenshot before scripting clicks (the "DOF" button sits left of "Relax").
- **egui text fields in popups:** call `request_focus()` only once (track a flag);
  re-requesting every frame swallows Enter.
- **Scripted clicks on menus/buttons** need a hovered frame before the press
  (`tools/gui_sim.py click` sleeps 0.15 s after moving); otherwise menu items and
  even tool buttons are silently missed and later clicks go to the wrong tool.
- **egui 0.36 / eframe 0.36:** `App::ui(&mut Ui, &mut Frame)` (not `update`);
  panels are `egui::Panel::left/right/top/bottom(..).show(ui, ..)`, sizes via
  `default_size`; `ui.close()` closes menus.
- **Missing glyphs** in egui default fonts: `→`, `≡`, `⚠`, `✕`, `✘` render as boxes.
  Use plain text (`:` `x` `Warning:`). `✔`, `°`, `±`, `Ø` work.
- **Drag start:** egui reports a drag only after a few pixels of travel — pick the
  target at `ui.input(|i| i.pointer.press_origin())`, not at the current position.
- **wgpu 30 via eframe:** use `eframe::wgpu` (don't add a direct wgpu dependency).
  The viewport renders offscreen into `Rgba8Unorm` with **gamma-encoded** output
  (egui samples it as gamma data); MSAA 4× when supported. Back faces get a separate
  red pipeline with larger depth bias (red = orientation bug).
- **Solver damping:** Levenberg `λ·I`, not Marquardt `λ·diag(JᵀJ)` — the latter
  lets weakly-coupled DOFs fly away (a tangent line slid 33 km once).
- **Picking order:** faces beat origin planes (origin planes are large and usually in
  front of the face the user aims at).
- **Sketch frame defaults** (Z up): origin planes XY(x=X), XZ(normal −Y, x=X, y=Z),
  YZ(normal +X, x=Y, y=Z); walls have y = +Z; Inventor itself is Y-up by default.
- **`serde_json`** needs `float_roundtrip` or f64 values lose 1 ULP.
- **Cargo** package names cannot start with a digit (`cadcraft3d`).
- **Booleans are faceted (stage 1).** Curved faces after a Boolean/Revolve are planar
  facets with `Face::facet_of` = the exact surface (smooth normals, hidden facet edges,
  `BRep::feature_edges`). Coincident curved faces must share facet vertices or the CSG
  leaves slivers: revolve angles and arc samples use a global grid (360°/96,
  `revolve.rs::grid_angles`). Facets of one surface must share one tag in
  `solid_polygons`, or every old facet edge reappears after the next Boolean.
- **Triangulation with collinear points** (split artefacts) needs scale-relative
  orientation tests (`mesh.rs::clip_ears`, no bridge through a vertex); the fan
  fallback produced overlapping triangles and broke every following Boolean.
- **Sketch ids must be unique**: demo sketches are named `Sketch1 (Slot)` with id
  `Sketch1`; `next_sketch_name` counts digits in names *and* ids. A duplicate id made a
  feature resolve the wrong sketch ("profile no longer exists").
- **Slice Graphics** discards fragments in front of the sketch plane (shader uniform
  `clip`); through the cut one sees the solid's *back faces*, so `fs_backface` paints
  them as the hatched section when slicing (otherwise red = orientation bug).
- **ViewCube** picks 26 directions from 3×3 regions per visible face (`BAND`); arrows
  and roll buttons exist only while looking straight at a face. `OrbitCamera::roll`
  rotates the image; `look_from` resets it.
- **Pattern element offsets:** pattern copies are chained (`Translated` from the
  neighbour). An offset element gets `offset = own − neighbour's` and so does the
  next one (negative), so the chain returns to the grid. Defining copies (first of
  each direction, last when Fitted) cannot be offset. `SketchPattern::offsets` and
  `targets` (Pattern at Points) survive `edit_pattern`.
- **Pattern along Path** places stations `H_k` on the curve (`on_curve`) at equal
  *chords* (first chord dimensioned, the others Equal construction lines) and moves
  each copy by `H_k − start` (`Translated`). Station points are listed in the
  instance's `points` so `clear_pattern` removes them. Copies do not rotate yet.
- **Command dialogs own the Properties panel** (`0-ui.md` §2.9): while a pattern
  dialog is open, clicks go to its active selector (`SketchTools::dialog_pick`),
  `picks_geometry()` follows the selector kind, nothing is created before OK
  (`dialog_ok`), and the sketch's own properties are hidden. The pre-selection is the
  Geometry input, so a stale selection becomes the input — clear it in tests.
- **`tools/gui_launch.sh` runs the app under `timeout 300`**: after 5 minutes it
  silently exits and later scripted clicks hit nothing. Relaunch for long sessions.
- **Sketches resolve against the body at their place in history**
  (`PartSketch::after_features`), inside `regenerate`'s feature loop. Resolving them
  against the final body made a sketch on a face that a later Join swallowed "sick".
- **Model picking in the view** (idle 3D): `edge_under` (screen distance to sampled
  `BRep::visible_edges`, occluded ones skipped) beats faces; `candidates_under` feeds
  Select Other. The edge cache key is a cheap body signature — keep it when bodies change.
- **`gui/i18n.rs`**: `tr()` translates *whole* hint sentences; a changed English prompt
  silently falls back to English, so update the table with it.
- **Fillet / Chamfer** (`part/blend.rs`, `Feature::Blend`): each edge becomes a tool body
  (profile across the edge extruded along it, or revolved about a cylinder axis) that is
  subtracted (convex) or joined (concave). Edges are `EdgeRef` = two face names + hint
  point. All tools of one feature are built from the body *before* it, and targets are
  found by `Solid::name` (ids change with each Boolean). Circular edges: decide the
  sides from the cylinder *face* (its mesh and sense), never from the edge's start→end
  (a closed circle has one vertex → NaN). Tangent contact can leave degenerate slivers:
  the Boolean is retried with the tool moved 1 µm into the material, and
  `add_polygons` drops loops that collapse below three vertices.
- **Fillet/Chamfer of whole loops**: when the selected edges cover the outer loop of a
  planar face whose walls are perpendicular (lines + arcs, `cap_loop`), the loop is treated
  at once (`apply_loop`): a tapered extrusion through section levels (`ExtrudeLevel`) is the
  kept region; tool = frame − kept. Corners come out spherical; the strips are then marked
  as facets of the exact cylinder/torus/sphere (`BRep::mark_facets`). The exact tangent
  levels are skipped (near-parallel faceted faces break BSP CSG): a ~r/1000 step remains.
- **Circles are sampled on a global angular grid** (`mesh.rs`: multiples of 2π/96k from
  world X projected), so coaxial arcs of different features share facet angles.
- **GUI picking uses `PickCache`** (triangles + edge→faces, built once per body via
  `body_key`). Never call `BRep::pick_face`/`edge_faces` per frame or in loops: on a
  faceted part (10–20k faces) that made frames take 200+ ms. Normalize ray directions
  before comparing ray parameters with mm tolerances.
- **Work features** (planes, axes, points) are `Feature::Work(WorkFeature)` (`part/work.rs`),
  computed in the regeneration loop into `PartDocument::work` (`WorkGeom`, runtime only);
  others refer to them by `PlaneRef::Work` / `LineRef::Work` / `PointRef::Work` and resolve
  via `placement::plane_of/point_of`, `LineRef::resolve`, `SketchPlacement::resolve_with`.
  Any new `match` on `Feature` must handle them (no body). GUI: `gui/work.rs` (one
  command per kind; the method follows from the picks, `WorkDraft::def`).
- **Bodies have persistent names** (`Solid::name` = id of the creating feature; a
  boolean result keeps the target's name). Features pick bodies by it
  (`solids`); display names `SolidN` come from `PartDocument::bodies_before`.
- **References** of a sketch: `PartSketch::references/break_link/delete_reference`;
  ends shared by another reference edge stay linked.
- **Interpolation spline handles**: `SplineFit::handle(k, n)`; an interior handle is a
  double knot (C1 there); a closed curve repeats its first point at the end of the
  point list (`closed`), so `points.len()` counts it twice.
- **Dynamic Input (HUD)** lives in `gui/mod.rs` (`Hud`, `hud_fields/apply/keys/click`).
  Tab must be taken in `App::raw_input_hook`, before egui uses it to move widget
  focus (otherwise Tab focuses "File" and the next Enter opens the menu).
- **Interpolation splines** are `Entity::Spline` + `Sketch::interp` (fit data). Their
  control points *and knots* come from `Sketch::spline_def` (chord-length parameters,
  knots at the fit points); every consumer must pass the knots on
  (`ProfileCurve::Spline`/`Segment2::Spline::knots`, `BSplineCurve::sketch_spline`,
  `Geo::spline`). Empty knots = clamped uniform (CV splines, old files). End handles
  are dimensionless (1 = chord speed); set them via `set_spline_handle_grip`.
- **Manipulators** (`gui/manip.rs`) are built from the feature definition each frame
  (`extrude_handles`, `revolve_handles`); a drag maps the eye ray to a value
  (`drag_value`), snaps it, updates the dialog live but an existing feature only on
  release (one regeneration, one Undo step). They are picked before sketch drags and
  orbiting (`manip_drag`).
- **Field expressions:** feature fields go through `params::eval_field_in` on a *copy*
  of the parameter table (the dialog holds `&mut self.extrude`); drafts store the
  equation in `FeatureParams` before the feature exists, `sync_parameters` names it.
- **Sketch solver is sparse**: `solver.rs` differentiates each equation only over the
  unknowns it reads (`Eval` logs them), takes the rank by sparse elimination
  (`Echelon`), and the DOF analysis reduces "is this motion fixed" functionals against
  that echelon form — no SVD/eigen (both stay as `#[cfg(test)]` cross-checks that run on
  every lib test). Above 150 unknowns a step uses a sparse LDLᵀ in minimum-degree order
  (`sparse_ldl_solve`; CG needed ~500 iterations on the near-singular matrix of an
  under-constrained sketch) and starts nearly undamped. A new residual must read geometry **only through `Geo`** or the sparsity
  misses it.
- **Polygon triangulation with many holes** (`mesh.rs::triangulate_with_holes`): several
  holes can bridge to the same outer vertex; each bridge must attach to the *pass* of that
  vertex whose wedge holds it, and an ear test must check the edges leaving other passes
  of its corners (by position in the ring, not by index — indices repeat). Otherwise
  ear clipping stalls and the fan fallback overlaps (a 40-hole plate shaded 6× its area).
- **Selection drawing**: selected/prehighlighted faces go into the 3D scene (offset
  outward, depth-tested; `scene_sel` triggers a rebuild); edges are drawn in 2D only where
  the eye ray does not hit a face first. Overlay fills without depth showed faces through walls.
- **Perf tests and the build cache**: clones share `BuildCache`, so a timed `regenerate`
  on a clone measured cache hits (0 ms). Give timed clones `build_cache = Default::default()`.
- **Extrude starting profiles** (`extrude::default_profiles`): one region → it; many →
  every region closed by the user's own curves (no projected edge) that does not fill
  another one's hole. Window/crossing box picks profiles too (`extrude_box_pick`).
- **Edge picking under perspective**: the occlusion test must use the 3D point of the edge
  segment nearest the eye ray (`closest_on_segment`), never the screen parameter mapped onto
  the 3D segment — on a long straight edge (2 samples) that was millimetres off and the edge
  was "hidden" (unpickable). Highlights skip hidden pieces via `draw_visible_polyline`.
- **Edge middle**: `sample_edge` of a straight edge has 2 points, so `pts[len / 2]` is an
  *end*; use `select::polyline_middle` (convexity was wrong at pocket corners).
- **Fillet types** (`BlendFeature::fillet_type`: Constant / Variable / Chord / FullRound) and
  further constant sets (`sets`, fields `radius2..9`) go through `part/exact.rs::special_fillet`
  (kernel `fillet_each` / `fillet_variable_points` / `fillet_chord`); failures are feature
  health, never a faceted rebuild. Full Round derives edges and radius from the middle face
  (`select::full_round_edges`). Select modes (Loop / Feature / Solid) live in `gui/blend.rs`.
- **Tangent snap** (`Snap::Tangent`, Line tool): the end lies on the circle (PointOnCircle) and
  the line is Tangent — a double root; `Constraint::Tangent` uses the first-order form
  (radius ⟂ line) when a line end is held on the circle, like for shared endpoints.
- **Shell** (`part/shell.rs`, `kernel::exact::shell`): faces are `FaceRef` (name + point);
  `exact::faces_named` gives the piece nearest the hint and `face_samples` a point on each
  kernel face (centre of its largest triangle at size·1e-4 — the kernel picks faces within
  size·1e-3). Inside uses `sk::shell`; Outside/Both/closed use `offset_faces` minus a cavity
  whose faces are pre-named `Shell1:Inner[..]`. The rim keeps the removed face's name.
  After an Inside shell the outer straight edges are split at the floor level — select
  every piece (blends join them).
- **Windows paths** (`docs/spec/0.6-files.md`): `canonicalize` returns `\\?\C:\…` — pass it
  through `document::library::without_verbatim` before showing or storing it; the home folder is
  `USERPROFILE` (Git Bash sets `HOME=/c/Users/…`); `C:` alone is drive-relative
  (`gui::files::drive_root`); paths handed to other programs (Explorer) need native
  separators (`Path::join`, never `format!("{dir}/x")`); paths compared with text in docs
  (`xtask assets`) are normalised to `/`. The X11 scripts in `tools/` do not run there.
- **Surface bodies** (`Solid::open`) are drawn amber and two-sided; they never take
  part in Booleans.

## Status and next steps

See `docs/ROADMAP.md`. Done: kernel base, sketch solver, viewport, file format v1,
Inventor-like UI shell, **sketch on face/origin plane** (placement, coordinate system,
auto-projection, associativity, Redefine, Undo/Redo), **sketch tools** (all Inventor
line/circle/arc/rectangle/slot/polygon/CV-spline variants, constraint tools,
General Dimension with placement rules, driven dimensions, construction format),
**editing** (constraint glyph select/delete/edit, DOF glyphs, per-curve fully-constrained
colour, entity drag, Relax mode, context menus) and **Modify** (Trim/Extend/Split/Offset/
Move/Copy/Rotate/Mirror/Scale/Stretch), Fillet/Chamfer, sketch patterns, midpoint snap,
⊥/∥ inference, point-on-spline, **profiles/regions** (shaded in the sketch), **Extrude**
(all options, editable, named faces), **parameters** (expressions, Parameters table,
tolerances, live dimension preview, in-place edit), **Revolve** and **Booleans stage 1**
(Join/Cut/Intersect/New Solid, Surface output), **exact kernel stage A** (SolidCraft/truck
via `kernel::exact`, see `docs/SOLIDCRAFT.md`). Next: stages B–F of `docs/SOLIDCRAFT.md`
(priorities in its §6: recipe runner through `PartDocument`, Shell/Draft, Mirror/Pattern,
Hole, Sweep/Loft, STEP, commands → control channel → MCP), Properties restyle.
