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

- The project owner writes in **Bulgarian** — answer the owner in Bulgarian.
- Everything in the repository is **English**: code, identifiers, comments, rustdoc, commit
  messages and every file in `docs/` (the project is public; contributors are not expected to
  read Bulgarian). Bulgarian appears only in the app's UI translation (`gui/i18n.rs`, `bg`
  names in `gui/commands.rs`) and in the Bulgarian section of the README.
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
cargo run --example replay -- session-…-journal.jsonl   # replay a user's action journal
```

Names: product **3DCadCraft**, binary `3dcadcraft`, Rust package/lib **`cadcraft3d`**
(Cargo forbids a leading digit), project files `*.3dcadcraft`.

## Architecture (src/)

| Module | Responsibility | Key types |
|---|---|---|
| `kernel` | B-Rep geometry + topology, validation, tessellation (`mesh.rs`), queries & persistent names (`query.rs`), extrude, revolve (`revolve.rs`), BSP CSG on tagged polygons (`csg.rs`), polygons ↔ B-Rep and Booleans (`facet.rs`), exact kernel boundary (`exact.rs`: SolidCraft/truck → our B-Rep; guided lofts — rails, centre line, Area Loft, conditions, map points, Closed Loop — as stations in `exact/loft.rs`; surface bodies and Split by a surface in `exact/sheet.rs`) | `BRep`, `Curve`, `Surface`, `Plane`, `Face`, `Tolerance`, `BoolOp` |
| `sketch` | 2D sketch on a plane (dimension names/equations/tolerances in `dimparam.rs`): points, lines, circles, arcs, CV splines, styles (construction/centerline), ~20 constraint/dimension kinds, driven dims, Levenberg solver with DOF/redundancy analysis; composite shapes (`shapes.rs`), Modify ops (`modify.rs`: trim/extend/split/offset/move/copy/rotate/mirror/scale/stretch), 2D fillet/chamfer (`corner.rs`), associative rectangular/circular patterns (`pattern.rs`), profiles/regions (`profile.rs`: `Sketch::regions`), per-geometry DOF analysis (`dof_analysis`) | `Sketch`, `Constraint`, `Shape`, `SolveReport`, `DofAnalysis` |
| `part` | part environment (primitives Box/Cylinder/Sphere/Torus as sketch + Extrude/Revolve in `primitive.rs`; Project Geometry of other sketches' points/curves and circular model edges in `link.rs` (`GeometryLink`, `PartSketch::links`); work planes/axes/points/UCS in `work.rs`, curved-face geometry in `work/surf.rs`; history editing in `history.rs`: End of Part, `insert_feature`, reorder with dependency checks, Suppress, Delete options, Rename): features → body (Box, Extrude `extrude.rs`, Revolve `revolve.rs`, Sweep `sweep.rs` and Loft `loft.rs` (paths `PathRef`), Coil `coil.rs`, Rib / Web `rib.rs`, Shell `shell.rs`, Face Draft `draft.rs`, Rectangular/Circular Pattern and Mirror of features or solids `pattern.rs`, Hole and Thread `hole.rs` (standards tables `standards.rs`, `data/standards/*.csv`), Combine `combine.rs`, Split `split.rs`; `boolean.rs`: Operation/Output applied at regeneration), sketches with placement (plane + coordinate system), projections, associativity, Redefine, Edit Coordinate System; parameters (`expr.rs` unit-aware expressions, `params.rs` model/user parameter table) | `PartDocument`, `PartSketch`, `ExtrudeFeature`, `RevolveFeature`, `PatternFeature`, `Operation`, `UserParam` |
| `materials` | material/appearance libraries: built-in CSV (`data/materials/materials.csv`, `data/appearances/appearances.csv`, `include_str!`) + user CSVs in `user_dir()` (`%APPDATA%\3dcadcraft\materials`), same name overrides | `Material`, `Appearance`, `Library` |
| `part::appearance`, `part::physical` | part material/appearance, overrides by persistent name (`Looks`: bodies, features, faces + hint), document copies, precedence Face > Feature > Body > Part > Material (`Resolver`), Clear/Purge/Update; Physical Properties from the display triangles (mass, COG, inertia) | `Looks`, `LookTarget`, `Resolver`, `physical_properties` |
| `part::hole`, `part::standards` | Hole and Thread (`Feature::Hole` / `Feature::Thread`): `HoleSpec` (type, seat, termination, drill point, thread, fastener), notes for drawings (`note()`), `HoleInstance` + `group_holes` (`PartDocument::holes`, `hole_groups`); cosmetic-thread display mesh `ThreadBands` (texture sleeves); standards tables `data/standards/*.csv` (built in, user override folder) | `HoleFeature`, `HoleSpec`, `ThreadSpec`, `ThreadFeature`, `ThreadBands`, `Standards` |
| `part::measure` | Measure (GUI-free): one item → length / radius / area / volume / position, two → minimum distance (closest pair with box pruning, adapted from SolidCraft `measure_sel.rs`), centre distance, angle; exact for lines, circles (pieces of one circle measured whole), planes, cylinders; window in `gui/measure.rs` | `Target`, `Item`, `Measurement` |
| `part::export` | File → Export: binary STL, STEP (from `PartDocument::exact_bodies` via `kernel::exact::step_text`), GLB; `examples/export.rs` CLI | `ExportFormat`, `ExportMesh` |
| `document` | projects with many documents, `.3dcadcraft` ZIP/folder format, Link/Append/Pack | `Project`, `DocumentData`, `Resolver` |
| `diagnostics` | session log (`log.rs`, own logger + ring buffer), action journal with JSON-delta part snapshots (`journal.rs`), `replay.rs` (journal → parts, first differing step), autosave + Restore decision (`recovery.rs`), panic hook / crash reports / emergency autosave (`crash.rs`), Save Diagnostics zip (`bundle.rs`); GUI hooks in `gui/diag.rs` | `Journal`, `Outcome`, `Step`, `Autosaver`, `Offer` |
| `command` | Undo/Redo (snapshot history, coalesced gestures); selector flow of command dialogs (`flow.rs`: active selector, auto-advance, Continue/Back/Enter, accepted `PickKind`s, Face Draft/Shell selector lists — GUI-free); one selection model (`selection.rs`: categories, user `Filter`, Select `Priority`, `effective` = selector kinds ∩ filter, pick order, `box_takes`, Shift + right-click menu content); later the Command enum | `History<T>`, `Flow`, `Selector`, `Filter`, `Priority` |
| `presets` | feature-command presets (user settings, never document data): `PresetData` trait (setting keys, `strip_inputs`, `capture`/`apply`), `apply_in` (re-evaluates stored equations), `CommandPresets` (Last Used, New Default, Sort, built-ins), `PresetStore` (one atomic JSON file per command in `presets/` of the settings folder); GUI row in `gui/presets.rs` | `PresetData`, `PresetStore`, `CommandPresets` |
| `gui::commands`, `command::matcher` | command registry (`COMMANDS`: id, `Action`, English/Inventor/Bulgarian names, aliases, key, icon, places Tab › Panel, environments, tooltip, spec, FEATURES section if planned; `AppCmd` for menu/key-only commands), `remember` (recent commands → `AppOptions::recent_commands`), `search` (exact alias → recent → alias prefix → word prefix → fuzzy, `*`/`?`, Cyrillic↔Latin; other-environment and planned commands last); the GUI-free matcher is `command::matcher` | `Command`, `Env`, `Hit`, `Tier` |
| `api` | JSON-RPC/MCP server — placeholder | |
| `gui` (feature `gui`) | keyboard rules of every command (`keys.rs`: `OpenState` from `CadCraftApp::open_state` = the one "a command is open" check, Repeat on Enter/Space/menu, letters without Ctrl/Alt, `PanelKey` Enter/Ctrl+Enter for panel OKs); Sweep / Loft commands (`sweep.rs`: selectors Profiles → Path → Guide Rail, Sections, preview, panels); Coil / Rib commands (`coil_rib.rs`: `command_ui::selector_rows`, picks, preview, panels, Properties); breadcrumb of every feature panel (`breadcrumb.rs`: `BreadcrumbState`, `show`); command guidance (`command_ui.rs`: selector rows with dot/count/`x`, the right-click menu of an open command, Enter/Ctrl+Enter, cursor hint, minimal breadcrumb rename; Face Draft, Shell, Combine and Split use it); Combine / Split commands (`combine_split.rs`); panel/window layout and persisted UI settings (`layout.rs`: `UiSettings`, `AppOptions`, `LayoutUi::place` for floating windows, View → User Interface, Clean Screen, Full Screen, debounced atomic save into `settings.json` keys `ui`/`options`); Hole / Thread commands (`hole.rs`, panel `hole_panel.rs`), Materials/appearances/Physical Properties (`material.rs`: QAT lists, browsers, Adjust, Clear, Styles Editor, Manage tab, coloured scene), Rebuild All (`rebuild_all`, QAT + Tools), Fillet/Chamfer select modes and types (`blend.rs`); the Loft panel (`loft.rs`: Curves / Conditions / Transition tabs on the shared selector flow); selection modes and multi-select in the idle view (`select.rs`: `SelectMode`, `SelItem`, box/tangencies/invert/previous; part-side helpers in `part/select.rs`); Inventor-like shell: ribbon (large/small/icon-only buttons), vector icons (`icons.rs`), expression fields with the `>` menu (`valuefield.rs`), feature manipulators (`manip.rs`: distance arrows, taper grip, revolve angle arc), sketch pattern command dialog (`pattern_dialog.rs`: selectors, preview, OK/Cancel), part Rectangular/Circular Pattern and Mirror command (`pattern_feature.rs`; its panel in `panels.rs`), browser, properties, ViewCube, viewport, overlays; sketch tool state machine (`sketch_tools.rs`: Line/Circle/Arc/Rectangle/Slot/Polygon/Spline/Point, constraint tools, General Dimension) | `CadCraftApp`, `SketchTools` |

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
13. **Every numeric input is the sketch's field** (owner's rule, 2026-10-23): no plain `TextEdit` /
    `DragValue` for a number anywhere — panels, in-place boxes in the view, tool dialogs all go through
    `valuefield::value_field` / `ExprValue` / `field_row`, so each one has the same additions:
    expressions with units, parameter names, the `>` menu (List Parameters, Recently Used, Measure),
    a parameter dropped on it by the pick whip, and a tolerance where the value is a dimension.
    Dynamic Input next to the cursor follows the same evaluation (`expr::eval` with the parameters).
    Panels live inside the Properties scroll area; viewport handles keep a fixed size on screen.

## Exact kernel (SolidCraft / truck) — see `docs/SOLIDCRAFT.md`

- **The boundary's contract is `docs/KERNEL_API.md`** (every operation, the naming history it must
  give, the truck workarounds to drop with another kernel). SolidCraft paused its truck kernel work
  (its #3992) for solidcore (a Rust port of NVIDIA's SMLib); keep the boundary clean for that.
  `tests/compat/names.json` holds the face names of every old fixture (`face_names_stay`): a changed
  name breaks old files even with the same volume.

- Geometry is moving to `solidcraft-kernel` (git dependency pinned in `Cargo.toml`, MIT OR
  Apache-2.0, on truck). It is reached **only** through `kernel::exact`; no truck/solidcraft
  type leaves that module. Every call goes through a guard (panic → error).
- Our persistent names are built from its exact history (`FaceOrigin`), never from indices.
- `solidcraft-kernel` is patched (`[patch]` in `Cargo.toml`) to `vendor/solidcraft-kernel`: the
  pinned commit plus our speed fixes, each listed in `vendor/README.md` and proposed upstream in
  `docs/SOLIDCRAFT_ISSUES.md`. Change it only there, with a README row; drop it when upstream has them.
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
  FEATURES section (`**Specification:** [spec/…](spec/…)`).
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
  in "Sources".
- **Content** (template in `docs/spec/README.md`), in English and in our own words.
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

## How to debug a user's session

"Something did not work at the end of the last session":
1. Files are in `%APPDATA%\3dcadcraft\logs` (Linux `~/.config/3dcadcraft/logs`): the newest
   `session-*.log` (UTC times, warnings, sick features, slow regenerations, save/open/export),
   `session-*-journal.jsonl` (one action per line: `checkpoint`, `command_ok` with the feature
   JSON, `error`, `health`, `state`…), `session-*.json` (`clean_exit: false` = crash/killed),
   `crash-*.txt` (panic, backtrace, last log/journal lines). The user may send a zip from
   Help → Save Diagnostics instead. Autosaves: `…\3dcadcraft\recovery`.
2. `cargo run --example replay -- <journal>` rebuilds the part state by state and marks the first
   step whose volume / faces / sick features differ from the recording;
   `--part N out.3dcadcraft` saves the part at state N to open in the app.
3. Regression test: copy the journal into `tests/` and assert
   `diagnostics::replay::first_difference(&replay(path)?)` is `None` (see `tests/diagnostics.rs`),
   or build the failing state with `replay::part_at(path, N)`. A failed OK is an `error` line
   with `command` + `attempt` (the feature it tried): `replay::attempts(text)` lists them (with
   the state before), `replay::retry(&mut part, &a.feature)` adds one (see `tests/sweep_session.rs`).
   Journals before 2026-10-20 have only the error text — rebuild the feature by hand there.
- The kernel's expected panics go through `diagnostics::crash::guard` (no crash report); any new
  `catch_unwind` around kernel calls should use it too.

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
- **Every command lives in `gui/commands.rs` first** (id, `Action`, names incl. Bulgarian,
  aliases, key, icon, places, environments, tip, spec / FEATURES section); the ribbon places it
  with `ribbon::c("<id>")` and takes label, icon, key and tooltip from there. Tests fail when a
  ribbon button has no entry (or an entry's ribbon place has no button), ids repeat, aliases
  collide in one environment, or an implemented entry has no `Action`. Ids are stored in
  settings (recent commands, later keymap / marking menus): never rename one. `bound` = the key
  or first alias already works in `shortcuts`.
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
  rotates the image; `look_from` resets it. Pick order: Home > arrows/roll > cube regions
  > compass letters > compass ring. `viewcube::ViewCube` owns a drag that *starts* on the
  cube/ring for its whole duration (`drag_hit` feeds `on_cube`, nav is skipped via
  `cube_drag`), so new viewport drag handlers must check `on_cube.is_none()`.
  Clicks/standard views animate (`ViewCube::animate(before, &mut camera)` after changing
  the camera; `ViewAnim` slerps the view direction). The transition yields when anyone
  else changes the camera (`tick` compares with the camera it set last) — never rely on
  the camera being at its end value right after `set_view`/`cube_clicked`.
- **Orbit has no pitch clamp**: pitch is wrapped to (−π, π] and may exceed ±90°
  (upside down). The view's up is `OrbitCamera::up()` = d(eye)/d(pitch), never world Z;
  use `frame()` for (back, screen-up) and `set_frame(dir, up)` to place the camera (it picks
  the yaw/pitch branch and roll). Never test "top view" by pitch value — use the eye
  direction (`is_ortho`, `face_on`).
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
- **Projections from other sketches** (`part/link.rs`, GUI `gui/project.rs`): a `GeometryLink` names its
  source by sketch id + point/entity id (`LinkSource::Sketch`) or `EdgeRef` (circular edges), and keeps the
  target points in the source's point order. `update_sketch` takes `(&mut [PartSketch], i)` and calls
  `link::apply` first, so a sketch reads its sources' *current* state: a source must come before the target
  in history (`link::order_key` = `(after_features or MAX, index)`), else the link is sick. Circles/arcs only
  onto parallel planes; a projected circle holds its radius via `Sketch::set_radius_fixed` (no solver DOF).
  Use `PartSketch::is_reference(e)` (not `reference_lines.contains`) to refuse edits of projected curves.
  The GUI regenerates on Finish Sketch when `link_dependents` of the sketch is non-empty.
- **`SecondaryMap<K, ()>` does not round-trip through serde_json** (`Some(())` and `None` both become
  `null`: "inconsistent occupation in Slot"). Store `bool` (see `Sketch::fixed_radius`).
- **Path sweeps of a whole circle** (`sk::sweep_path`, sampled paths such as splines): one closed edge has a
  single vertex and the kernel panicked "two same vertices"; `kernel::exact::sweep::quartered` splits arcs
  into quarter turns first (lofts already did).
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
- **Combine / Split** (`part/combine.rs`, `part/split.rs`, GUI `gui/combine_split.rs`,
  `docs/spec/2.13-combine-split.md`): bodies by persistent name; Combine's result keeps the base's
  name (so its appearance); consumed tools leave `solids` (deleting the feature brings them back),
  kept ones go to `PartDocument::hidden_bodies` (skipped by `material::add_part_meshes`,
  `PickCache`, `edge_polylines`; `body_key` includes the list). Split Face pieces are
  `<face>~<split>.<k>` (k from the tool's back) and `history::names_feature` treats `~<id>.` as a
  reference to the split. **SolidCraft's draft and shell of a convex body work on whole planes**:
  drafting one piece tilted every piece of that plane (volume unchanged) — pieces go through
  `split::draft_pieces` (a prism cut / joined per piece); a shell removing one piece still opens
  the whole plane (known gap). Split Solid: `sk::split_body` (fast imprint) with `split_by_plane`
  as fallback; never classify the parts by `measure` (0.7 s each through rounds) — use an edge
  midpoint. The new body is `<id>#2`; sections are `<id>:Cut` / `<id>#2:Cut`.
- **Shell** (`part/shell.rs`, `kernel::exact::shell`): faces are `FaceRef` (name + point);
  `exact::faces_named` gives the piece nearest the hint and `face_samples` a point on each
  kernel face (centre of its largest triangle at size·1e-4 — the kernel picks faces within
  size·1e-3). Inside uses `sk::shell`; Outside/Both/closed use `offset_faces` minus a cavity
  whose faces are pre-named `Shell1:Inner[..]`. The rim keeps the removed face's name.
  After an Inside shell the outer straight edges are split at the floor level — select
  every piece (blends join them). Rounds ≥ the wall at an open face: the kernel refuses the
  rim; `shell_with_openings` = closed cavity + a prism of the face outline grown by
  0.001·t (a prism side along the round's tangent edge only *touches* it and the cut fails).
- **Face Draft** (`part/draft.rs`, `kernel::exact::draft`): Fixed Edge = a neutral plane square
  to the pull through the picked face's nearest edge square to it; faces with different
  neutral planes are drafted group by group, re-found by name on each result (names are
  kept). Automatic Face Chain can reach faces not parallel to the pull (a bottom round →
  the bottom): the kernel refuses those.
- **Command search and alias field** (`gui/cmd_search.rs`, `gui/alias_input.rs`): both run a command
  through `start_command` (ribbon tab forward + `apply`). Alt+S is read at the very top of `shortcuts`
  (before the "text field focused" return). The alias field takes the letter's `Text` *and* `Key`
  events, or the single-letter shortcuts fire as well. Dynamic Input takes letters only after a first
  digit / sign (then the field is an expression evaluated with the part's parameters).
- **Parameter units** (`part::expr::Unit`, format revision 17): every unit is a factor to mm / rad
  (`from_display` / `to_display`); test `is_angle()` / `is_length()`, never `== Unit::Deg`.
- **Quantities** (`part/units.rs`): equations carry a dimension per value (`Dim`, powers of mm kg s K rad
  A cd mol) and are checked against the parameter's `UnitDef` (`Unit::def()` or a user parameter's
  `unit_spec`). `expr::eval` takes `Vars` returning `(value, dim)`; `eval_simple` gives every name the
  target's dimension (only where that is true). A *bare* result takes the target's unit; a unitless
  target accepts anything (old files had `count = d0 / 10`-style mixes). Names: a parameter wins over a
  unit of the same name; after a number a name is a unit. Physical properties are values
  (`eval_all_with` fixed), never equations, and no Dimension / Feature row may reach them.
- **Decisions** (format revision 18): `expr::Value` is a number with its dimension, true / false or a
  text; `UnitDef::logic` says which a parameter holds. `if()` evaluates only the taken branch for errors
  (`Parser::skip`); `split_assignment` must not split `==` / `<=`. Suppress if conditions live in
  `PartDocument::suppress_if` (by feature id) and are evaluated in `evaluate_parameters` into
  `cond_suppressed` (with dependents) — every place that asks "is it suppressed" uses `is_suppressed`.
- **One visible face / edge from kernel pieces** (`kernel::smooth`): truck has no periodic faces or
  closed edges, so a cylinder comes in 2+ faces and a circle in 2+ arcs. Faces on the same *curved*
  analytic surface sharing an edge are one `smooth_group`; the edge between them is a seam
  (`is_smooth_seam`) — hidden in `visible_edges` / `feature_edges` and in `tessellate_solid`'s edge
  lines. Selection draws `item_faces` = every face of the name and its group, an edge every piece of
  `edge_group` (same circle / line, same visible faces). Planes are never merged (Split Face cuts
  them on purpose). Geometry and names stay per piece — this is display and selection only.
- **`body_key()` runs many times a frame** (every `picker()`, every hidden-line test): it is
  cached per frame (`body_key_frame`, on only inside `ui`) and never walks face meshes —
  `BRep::bounding_box` reads every mesh position (2.4 ms on a 2500-hole plate; a selected
  face made frames take 80 ms). Keep per-frame code off `bounding_box` on big bodies.
- **"A command is open" has one place** (`gui/keys.rs`, `CadCraftApp::open_state`): a new
  draft / pick mode / modal must be added to its list, or Enter/Space and the menu's *Repeat*
  start the last command on top of it (Fillet, Hole, Work… did). Single-letter shortcuts check
  `keys::letter_alias` (no Ctrl/Alt; Alt+S is reserved for the command search). A new panel's
  OK adds `|| super::keys::panel_ok_key(ui, ok)`: `shortcuts` stores this frame's Enter /
  Ctrl+Enter in egui temp data and the panel takes it (Ctrl+Enter typed in a field also counts);
  commands with the `command_ui` flow get Enter there, not from the panel key.
- **Previews** (`preview_async`, `preview_ready`): check `preview_ready(&key)` *before*
  `self.part().clone()` (copying the part every frame made pattern previews crawl); one job at
  a time (a value changed quickly waits for the running job, then starts the newest key). A
  command whose OK would rebuild the same thing can take the preview's part
  (`preview_part` + `take_preview_part`, matched by key and `part_signature`): Pattern does.
- **Windows paths** (`docs/spec/0.6-files.md`): `canonicalize` returns `\\?\C:\…` — pass it
  through `document::library::without_verbatim` before showing or storing it; the home folder is
  `USERPROFILE` (Git Bash sets `HOME=/c/Users/…`); `C:` alone is drive-relative
  (`gui::files::drive_root`); paths handed to other programs (Explorer) need native
  separators (`Path::join`, never `format!("{dir}/x")`); paths compared with text in docs
  (`xtask assets`) are normalised to `/`. The X11 scripts in `tools/` do not run there.
- **Presets** (`src/presets.rs`, `gui/presets.rs`, `docs/spec/0.11-presets.md`): a preset is the
  feature's JSON with only `PresetData::KEYS` kept (a key skipped by `skip_serializing_if` is
  stored as `null` = "default"), equations as `params: {field: expr}` without parameter names.
  A feature stores only `preset: Option<String>` (skipped when `None`, so no format revision).
  **New command with presets** (Hole, Pattern…): `impl PresetData for XFeature` (keys + inputs
  hidden in a setting via `strip_inputs`), add it to `builtins`/`factory`/`COMMANDS`, then the
  GUI hooks: `preset_start` when the command opens, `preset_edit` on Edit Feature, `preset_row`
  right after `panel_header`, `preset_stamp` before and `preset_used` after a successful OK /
  Apply. GUI tests never touch the user's folder (`PresetUi` is in-memory under `cfg(test)`);
  `CADCRAFT3D_PRESETS_DIR` redirects the folder for scripted runs.
- **Panels and floating windows** go through `gui/layout.rs`: docked panels use
  `layout.panel_id(name)` (Reset UI Layout bumps a generation so egui forgets dragged sizes);
  a floating window is wrapped as `self.layout.place("key", ctx.content_rect(), sized, Window…)`
  (the *last* call of the builder, so the remembered rectangle wins over `default_pos`);
  Escape-closable keys are in `ESCAPE_CLOSES`. Do not `.anchor()` a window that should
  remember its place. New persisted options: add the field to `AppOptions` and to
  `options_snapshot`/`apply_options`; loading is field by field (`lenient`), so an old file
  keeps its other values. egui's Ctrl+0/Ctrl+± zoom is off (`zoom_with_keyboard`): Ctrl+0 is
  Clean Screen. F11 is Relax Mode in a sketch and Full Screen elsewhere.
- **Insertion goes through `PartDocument::insert_feature`** (all `add_*` via
  `insert_feature_checked`): a new feature lands at the End of Part (`eop`), sketches after
  it shift, the marker moves below it. Never `features.push` and never assume the new feature
  is the last one — use the returned index / `last_inserted()`; new sketches get
  `after_features = insert_index()`. `build_limit()` = min(Edit Feature `rollback`, EOP).
- **Suppressed features are skipped *before* their cache key is chained**, so the following
  keys equal those of a history without them; reorder/insert change the chain from that
  point on. Clones share `BuildCache`: a timed second run of the same edit is a cache hit.
- **History dependencies are found by persistent-name strings** (`history::feature_refs`):
  a string equal to a feature id or starting with `"<id>:"` (then a non-alphanumeric) in the
  feature JSON (minus id/name/type/sketch) or its sketch's placement/projections. Keep new
  reference fields as plain persistent names so reorder/suppress/delete see them.
- **Python file edits on Windows** write CRLF unless `newline=''` is passed; the repo is LF.
  In a Python string a `\` at the end of a line is a line continuation: a Rust string continuation
  written through a script came out as a run of spaces. Use `concat!` or one-line strings there, and
  never "fix" text with a regex whose `\s` can cross lines (it joined code into comments).
- **Work features on curved faces** (`part/work/surf.rs`) use the face's *infinite* analytic
  surface (`facet_of` for faceted faces); two-solution methods store `other` / `index`, never a
  hint point. Sketch points/lines used with other inputs become hidden in-line work points/axes
  (`WorkDraft::inline_defs`), since `PointRef`/`LineRef` cannot resolve sketches. A UCS stores its
  parts as `UCS1:XY`, `UCS1:X`, `UCS1:Center` … in `WorkGeom`. Exact cones used to come out of
  `exact::to_brep` as the XY plane (radius 0 refused by `Cone::new`); they are real cones now.
- **Sweep / Loft** (format revision 12; `part/sweep.rs`, `part/loft.rs`, `kernel::exact::{sweep, loft, loft_frames,
  loft_to_point}`): a feature may use several sketches — `Feature::sketch_ids` (profile, path, rail,
  sections) drives `consumers`, the regeneration cache key (all their geometry, or editing the
  path sketch hit a stale cache) and history dependencies. A plain sweep is `sk::sweep` /
  `sk::sweep_path`; taper / twist / rail / Parallel loft placed copies of the profile
  (`loft_frames`). The kernel's loft pairs every section with the *last* one by vertex
  directions, so twists and strongly turning paths are built in chunks (≤ 144°/segments, ≤ 60°)
  joined by Union. A whole circle as one arc (one vertex) panics the kernel's loft — arcs are
  split into quarters (`map_loop`). Holes of loft sections are lofted separately and cut (#1184).
  No faceted fallback: kernel failures and results `to_brep` cannot convert are feature health.
  `to_brep_with(.., lenient)` (Sweep/Loft steps only): a fitted line/circle missing an end falls back to the
  polyline (an intersection curve of a lofted face failed "edge end vertex is not on its curve");
  strict elsewhere: a concave partial-arc fillet (combos.rs) gave a wrong exact volume when lenient.
- **Sweep along a spline** (`sk::sweep_path`, point path): a whole circle as one arc panicked it
  too ("This wire is not simple", owner's session 2026-10-20) — `kernel::exact::sweep` splits the
  profile's arcs into quarters first. `check_bend` refuses a profile reaching past the centre of
  a bend (radius of the circle through 3 path points vs the profile carried by the same
  double-reflection frames) before the kernel sees it. An interpolation spline's end tangent leans
  towards its next points: the kernel keeps that angle, so the volume is A·L·cos θ0 (not A·L).
  Fillet of the cap edge works through our `vendor/` patch of `curveblend` (curved seams at chain
  corners); Shell refuses (B-spline walls cannot be offset).
- **Guided lofts** (`kernel/exact/loft.rs`, `docs/spec/5.2-loft.md`): the kernel lofts only through
  planar sections, so rails / centre line / Area Loft / conditions become ≤ 48 *stations* (planar
  degree-7 Bézier loops) lofted by `sk::loft_matched` (our vendor addition: keeps the caller's
  segment pairing — the kernel's own matching re-indexes every section to the lead and undoes a
  twist). Stations must be evenly spaced: the kernel interpolates across them with *uniform*
  parameters. Loops are oriented counter-clockwise about the direction the loft runs, never about
  the first normal (a ring turns right round). Plain Catmull-Rom shrinks a ring of sections (6 on
  a circle: −2.6 %); origin tangents get the arc length (`through`). The kernel's volume of a
  twisted ruled side (a saddle) is ~1 % low (coarse display mesh), not a geometry error.
  A section that is not square to a centre line keeps its plane: stations blend its tilt (a
  rotation vector in the carried frame) out towards the next section — square stations right up
  to it cut through it (owner's journal `tests/data/loft_centerline_journal.jsonl`, wall below
  z = 0). The kernel accepts self-intersecting lofts, so `check_folds` refuses stations that fall
  behind the previous station's plane.
- **Selection filter** (`command::selection`, `docs/spec/0.9a-selection-filters.md`): the viewport
  sets `pick_filter` once per frame (`update_pick_filter`: the active selector's `PickKind`s ∩ the
  user filter, or the selector alone when that is empty). The shared pickers ask it (`cat_on`):
  `edge_under`, `plane_under`, `face_hit` (use it instead of `picker().pick` for a *selectable*
  face; occlusion tests keep `picker().pick`), `vertex_under`, `work_*_under`, `sketch_under`,
  the sketch inputs of Hole/Sweep/Loft/Coil/Rib/Extrude. A new command lists its kinds in
  `active_pick_kinds` (or has a `Flow`) and picks through these helpers. Idle picks go through
  `sel_item_at` / `sel_candidates` (raw candidates → `filter_candidates` → resolved by priority);
  never add a per-mode branch. Shift + right button sets `shift_rmb` on the press: the plain
  context menu and the Classic right-drag pan are skipped while it is set.
- **Surface bodies** (`Solid::open`) are drawn amber and two-sided; they never take
  part in Booleans (amber unless something assigned them an appearance).
- **Exact surface bodies** (`kernel/exact/sheet.rs`): Extrude / Revolve with Output Surface build
  `extrude_sheet` / `revolve_sheet` into an `ExactSolid` with `sheet = true` (`is_open()`: a full
  revolve's sheet *closes* but is still a surface). `build_features` takes them out of `solids`
  before every step except Split and puts them back after it — a new step never sees them, so it
  cannot Boolean or blend them. A revolve profile's lines on the axis are dropped (`off_axis`):
  the kernel refuses a closed curve touching the axis.
- **Split by a surface** (`exact::split_by_sheet`): each sheet face that reaches the body's box
  splits with its *extended* surface (planes/cylinders through the kernel's `Plane` / one-arc
  `Curves` tools — `SplitTool::Face` of a revolved cylinder took 10 s), faces on a surface already
  used are skipped (a full revolve comes in 3 sectors; splitting a part again along its own
  section never returned), parts are sorted by `SheetSide` (nearest sheet triangle) and joined
  per side. The Trim arrow and the classification use the same `SheetSide` (from the B-rep).
- **Split Solid / Trim by an open chain** (spline, lines + arcs): `split.rs::open_region` extends
  both ends along their tangents to a frame round the body and closes the region on the chain's
  *left* (= front, like the single-line plane); prism Booleans. Self-crossing is tested on true
  curve points (`Segment2::point`, not the control polygon) and must count *touching* segments —
  a symmetric figure eight meets itself exactly at a sample point.
- **Volumes of curved parts in tests**: `volume_at(1e-3)` is ~1e-4 off for a cylinder section
  (inscribed mesh) and the replay journal's recorded volumes are coarser still (a loft: 7178.7 vs
  7189.5); compare a split's sum with the whole measured the same way.
- **Two-sided Face Draft** (Symmetric / Asymmetric, Parting Line, Move Parting Line):
  `kernel::exact::draft_two_sided` splits by the plane (`sk::split_by_plane`), finds each
  half's pieces of the chosen faces by history origin (not by name: both pieces share it),
  drafts each half with its own neutral plane (`−pull` below) and unions them. The plane
  must be square to the pull — `draft_walls` tilts only about lines square to it. A
  Parting Line sketch is the feature's `Feature::sketch()` (consumed, cache key).
- **Automatic Blending** = `DraftFeature::faces_with_blends`: tangent cylinders *along the
  pull* next to a picked face join it (they become cones, still tangent). Without them the
  kernel refuses (an upright round next to a tilted wall).
- **Command flow:** dialogs keep only the active selector key and rebuild `Flow` each
  frame from the definition. While a flow command is open, Enter/Space must not repeat
  the last command (it restarted Face Draft). Selector prompts must not contain `": "`
  (the i18n test treats `X: …` as a command name).
- **Shell Center** is the 2027 name; the variant is `#[serde(rename = "Both", alias =
  "Center")]` so files stay readable by old builds. Rounded Corners rounds the offset's
  sharp edges (`round_offset_corners`: convex/concave by probing material around the edge
  midpoint, edges on removed faces skipped) and forces Inside onto the general path.
- **Revolve direction**: the exact kernel sweeps a positive angle *clockwise* about the
  axis direction (seen from its tip). The start rotation (Symmetric / Asymmetric) once used
  the opposite sense and covered 45°..135° instead of ±45°; `RevolveFeature::sweep_in`
  measures To targets in the kernel's sense. Check new angle code against
  `revolve_to_with_minimum_solution`.
- **Extrude terminations not parallel to the sketch** (inclined plane, cylinder) are built
  by extruding past them and clipping (`extrude::Clip`, `exact::clip_tool`); the clip
  solids are named `<id>:End`. The cylinder clip uses a half strip 1.5 r wide so no
  Boolean is tangent. The faceted kernel refuses these.
- **`extend_face` is true by default** (serde and `ExtrudeFeature::new`: old files and API
  users always used the face's plane; false broke the feature matrix). Only the GUI command
  starts with it off (Inventor) and turns it on after a pick when needed (`needs_extended_face`).
- **Predict Boolean vs. auto-flip**: a predicted operation sets `last_op`, so
  `sync_extrude_direction` does not flip the direction for it (they would fight).
- **Breadcrumb jump to the sketch** parks the draft in `suspended_draft`; `finish_sketch`
  brings it back (so `start_feature`, which calls `finish_sketch`, overwrites it).
- **Open profiles (Match Shape)** live in `ExtrudeFeature::open` (curves, side, flag); the
  loop is closed by a frame around the body's box (`extrude::open_loop`); a Join with Match
  Shape is also intersected with that box.
- **Feature patterns copy tools, not faces** (`part/pattern.rs`, `exact::pattern`): every
  Extrude/Revolve (and feature pattern) records its tools (`ToolSet`: solids before
  `combine`, operation, target `solids`) by feature id during `build_features` — also in
  `CachedStep`, or a cache hit leaves a later pattern without sources. A new tool-making
  feature (Hole) only has to push its `ToolSet`. All copies of one tool set go through one
  `combine(.., lenient = true)`: copies missing the body are skipped, an error only if none
  hits. Copy k's faces are `<pattern id>#k:<name>` (`exact::transformed`; no profile frames,
  so its edges get generic `E[..]` names). Solid-mode patterns skip the source lookup.
  Kernel failures of a pattern are health (no faceted rebuild).
- **Patterns of thousands** (`docs/PERF.md`, 5041 holes 7 s, 10000 in 14 s release): never mesh
  the compound tool or the result more than the display needs. `combine` skips `meets` for
  `lenient` (pattern) cuts and decides "did it cut" by the face count first (a volume only when
  equal, at a tolerance from the *smaller* of body and tool: `size·1e-3` of a 355 mm plate
  missed an r1 hole). `apart` tests copies as oriented boxes (turned AABBs of a circular
  pattern touch) from `exact::bounds`, whose tolerance follows the box's smallest side (a long
  Through All tool padded by 2 % of its length hid 1 mm gaps → one Boolean per copy, 22 min).
  `to_brep` takes all face surfaces in one `sk::face_surfaces` walk and chains loops through a
  start-vertex map (both were quadratic). Kernel side: `vendor/solidcraft-kernel` (see
  `vendor/README.md`) — budgets that scale with the operands, no orientation meshes of joined
  or healed pieces, exact prism volumes; a lost prism history falls to
  `provenance::trace`, which is quadratic (2500 holes: 9 s) — keep the lineage budgets.
- **Appearances** (`part::appearance`, GUI in `gui/material.rs`): the scene is built by
  `material::add_part_meshes` from `SolidMesh::face_triangles` (per-face triangle ranges);
  see-through faces go to the renderer's transparent pass (`add_mesh_transparent`: blended,
  no depth write, two-sided, drawn last), never to the red back-face pass. Viewport colours
  are `(srgb/255)^2.2` (`Appearance::shader_rgb`, the shader gamma-encodes with 1/2.2);
  glTF needs true linear sRGB (`Appearance::linear`). A face override's `hint` picks the
  piece when a Boolean split one name into several (`appearance::nearest_face`). GUI
  mutations go through `edit_part` (on a copy; Undo only on success). STEP export must not
  call `exact::face_point` per face (it made the export 2.5 s) — only for split names.
- **Physical Properties** integrate the tessellation (tetrahedra from the origin); planar
  bodies are exact, the exact kernel's `measure_full` is the cross-check in tests.
  part in Booleans.
- **Holes** (`part/hole.rs`, `part/exact.rs::hole`): each centre gives tools of revolution
  (`kernel::exact::revolved_tool`), cut in two stages — every seat, then every bore through
  their floors (one Boolean each, tools joined by `union_apart`). One stepped tool per hole
  made 36 counterbores take 43 s (n^1.4) instead of 1.6 s. Tools start above a start face
  (in the air); a centre not on a face asks `exact::contains` first. Faces are
  `<id>:<role>:<k>` (k from 1). The numbers from the standards tables are copied into the
  feature (the part regenerates the same without the user's tables, rule 8).
- **Coil** (`part/coil.rs`, `kernel::exact::coil`): one kernel call per coil — `sweep_helix`
  (constant pitch, no taper), `sweep_path` along the centre (round sections), one smooth `loft`
  (≤ 48 steps, ≤ 45° apart), else `mesh_body`. Never join pieces of a coil: the union of
  B-spline pieces touching on a section failed after 83 s. Reject a loft that came back ruled
  (more faces than segments + 2): it lost 2.5 % of the volume. Never call `measure_full` on a
  helical body in a hot path (8 s for 3 turns; tests use the display triangles). Coil Booleans
  go through `coil_boolean` (faceted ring zone in ~4 mm slabs, one thread each); a Join that
  only touches the body (`inside` empty) is exact. Start and End caps can lie in one plane:
  name them by the nearer section centre.
- **Rib** (`part/rib.rs`): `Rays` (display triangles, odd hit count = inside) finds where the
  body begins; webs extrude a mitred band (`offset_polyline`) to the deepest entry + a little;
  ribs extrude curve + "shadow" polygon. Extend Profile probes along the end tangent at half
  the web's depth (from the plane itself the ray grazes the rim).
- **Modeled threads**: never cut a smooth `sk::sweep_helix` groove with the exact Boolean —
  measured 2.5 s for half a turn through a bore, > 60 s once it crosses a plane, 8 turns
  did not finish in 15 min. `kernel::exact::cut_threads` splits the solid into the exact
  rest and a ring zone around the thread and cuts only the zone with `faceted_boolean`
  (36 stations a turn; zones in parallel). Grooves are recorded per feature and cut after
  the last feature (`finish_threads`, cached), so no later Boolean meets a faceted zone.
- **Thread zones are cut in slabs** (`exact::slab_threads`, 2 turns each, parallel): the
  faceted Boolean grows faster than the groove (M8 × 11 turns in one zone 13.6 s, slabs 1.6 s).
  A slab's mesh is only the stations reaching into it (`GrooveHelix`, stations on a fixed
  10° grid so neighbours share triangles). Slabs fail by chance where a plane lies near a
  face of the body (`ThreadGroove::avoid`) or a row of the bore's facets: a failed slab moves
  its planes, then merges. Every slab must pass our `to_brep` + `validate_solid`. A slab cut
  out of a very large body is imprecise (planes tilted 1e-6, circles 1 µm off: the kernel's
  tolerances follow operand size) and always fails: `square_to` detects it, `near_zone` cuts
  it again from the body in a box around the zone.
- **A bore reaching the far side** (Distance ≥ the thickness) is cut as Through All: its cap on
  the far face / cone in the air sent the kernel's Cut down a 6 s path (sometimes invalid), and
  a groove "stopping square" there put its corners exactly on the far face (150 s, then refused).
- **`faceted_boolean` history**: one edge whose origin `Sources::edge` could not tell (`None`)
  dropped the whole result's history (all face names). Patched in `vendor/` (no origin instead).
- **Replay vs. sketch edits**: the GUI does not regenerate while a sketch is open, so states
  journaled then carry the old body; they are marked `in_sketch` and the replay does not compare
  them. Finish Sketch regenerates. A replay "difference" on an old journal at Edit Dimension /
  Drag inside a sketch is this, not non-determinism.
- **Thread zones are matched by their box, not their middle**: the zone's top used to
  follow the hole tool's start margin (1 % of the part), so on a thin plate of a large part
  its middle lay above the plate and the groove was skipped silently (the owner's "the thread
  makes no geometry"). `ThreadGroove::zone_box` is the match; the groove runs one pitch past
  the face; a groove no solid takes is the feature's health. Fine threads in thin plates of
  very large parts (300 mm, M3) fail in the kernel's faceted cut (its circles in so large a
  face are approximations; SolidCraft's whole-body cut fails too) — reported, not hidden.
- **Hole sketch**: centres are Center Points (`sketch::PointStyle::Center`); edge dimensions
  of the Hole panel become `project_segment` reference lines (associative, by vertex names)
  plus `PointLineDistance` dimensions of the hole's sketch at OK; `PartDocument::project_edge`
  is also the sketch's Project Geometry (straight edges only).
- **Document tabs** (`gui/doc_tabs.rs`): the active document stays in the app's fields (`project`,
  `part_id`, `history`, `file_path`, `saved_sig`, camera, selection); the others are `DocTab`s in `tabs`
  (the active slot is `None`). `switch_tab` first calls `leave_document` (Finish Sketch, every draft
  cancelled) — a new draft field must be cleared there too, or it points into the wrong document. An
  inactive tab cannot change, so its `dirty` is taken once at the switch. New / Open / drop add a tab
  (no "save changes?"); quit asks tab by tab (`quit_dropped`).
- **Measure into a field** (`gui/measure_pick.rs`): a field asks through egui temp data
  (`request_id` → `(field id, unit)`), the answer comes back as `result_id(field id)` and `value_field`
  takes it as its commit; the Edit Dimension box sets `DimEdit::text`. While it runs, the view's click is
  removed from the `Response` (`flags`), so the open command does not pick the same click; Esc is taken
  at the top of `shortcuts`.
- **Hole placement dimensions** (`gui/hole_dims.rs`): `DimRef::Draft` (this command's clicks,
  `HoleDraft::dims`) and `DimRef::Sketch` (the hole sketch's `PointLineDistance`, on the working copy
  `HoleDraft::sk`, loaded by `hole_load_sketch` on Edit Feature, written into the part at OK and put
  back on failure; `rev` keys the preview). Arrows go through `ManipOwner::Hole`: the dragged target is
  found once by grip (`hole_drag`, cleared on release) because `Handle::field` cannot name it.
- **Countersinks** are tapered discs (`exact::cone_tool`, as SolidCraft's `extrude_tapered`): a tool of
  revolution with the same cone took 2.6× longer in the Boolean. A **90°** countersink whose ring lies on
  one planar face square to the hole (`ring_on_square_face`) is instead the kernel's equal chamfer of the
  bore rim after the bores (`exact::chamfer_circles`, faces renamed `<id>:CSink:k` / `FarCSink:k`; 36
  holes 40 s → 2.3 s); its cone tool still goes into the `ToolSet` (Pattern / Mirror copy it) and is cut
  when the chamfer fails. The far side's `Rays::exits` distance counts from `o`, not from the probe start. Cones still take truck's general path
  (ROADMAP K4: ~0.33 s a hole, more than linear in one feature) — measure before adding cone tools.
- **Hole data for drawings**: `PartDocument::holes()` (filled during regeneration, also from
  cache hits via `CachedStep::extra`). Pattern / Mirror must push `h.transformed(&m)` copies
  (with the element index in `copy`) into `hole_instances` at their place in the loop.
- **Cylinder face side** (`hole::cylinder_face`): decide shaft vs hole wall from the face
  mesh normals against the radial direction; `Face::sense` of an exact-kernel hole wall
  said "Same" (the Thread fit check called an M8 tapped hole a Ø6.6 shaft).
- **Revolved cones** (drill point, countersink) come back from `sk::face_surface` without
  an analytic cone (our `Surface` is a plane there); don't test for `Surface::Cone`.
- **Cosmetic thread texture** (`part::hole::ThreadBands`, renderer pipeline `threads`,
  `viewport.wgsl::fs_thread`): one sleeve per `CosmeticThread` (2 rings, phase
  `h/P ∓ θ/2π` per vertex), lifted `BAND_LIFT` (0.016 mm > the kernel's 0.01 mm display
  tolerance + `BAND_SAG`) towards the air, built in `build_scene` only. It is blended as
  `2·src·dst` over the shaded face (no depth write) so it keeps the appearance and light — no
  per-face colour lookup. Pattern / Mirror push `c.transformed(&m)` copies into
  `extra.cosmetic` like holes (a mirror flips the hand). `fwidth` must come before the
  `discard` (uniform control flow); `renderer::tests::viewport_shader_validates` runs naga on
  the shader. Option: `AppOptions::thread_texture` (Application Options → Display).

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
via `kernel::exact`, see `docs/SOLIDCRAFT.md`), **materials and appearances** + Physical Properties. Next: stages B–F of `docs/SOLIDCRAFT.md`
(priorities in its §6: recipe runner through `PartDocument`, Shell/Draft and Mirror/Pattern (done, waiting for a visual check),
(Join/Cut/Intersect/New Solid, Surface output), **Hole / Thread** (standards CSV, notes,
modeled threads; format revision 6), **exact kernel stage A** (SolidCraft/truck
via `kernel::exact`, see `docs/SOLIDCRAFT.md`). Next: stages B–F of `docs/SOLIDCRAFT.md`
(priorities in its §6: recipe runner through `PartDocument`, Shell/Draft, Mirror/Pattern,
Hole, Sweep/Loft, STEP, commands → control channel → MCP), Properties restyle.
