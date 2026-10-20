# SolidCraft: what we take and how

[SolidCraft](https://github.com/bherbruck/solidcraft) is a parametric CAD in Rust licensed
MIT OR Apache-2.0, the same as ours. We borrow code from it with credit in
[ATTRIBUTION.md](../ATTRIBUTION.md). Its kernel is built on
[truck](https://github.com/ricosjp/truck) (Apache-2.0). This file holds the decisions, the
feature-by-feature comparison and the plan.

## 1. Decision: exact kernel (ADR 0001)

**Problem.** Our kernel is our own. Its Boolean operations are faceted: BSP on
polygons. A test part (box 40×30×20, 8 fillets r5 and a chamfer 2 along the top outline)
comes out with about 12 000–20 000 faces. Building it takes seconds, and the surfaces are
approximate. Every following feature and picking in the view become slow.

**Trial** (`tests/exact_kernel_spike.rs`, the same part with `solidcraft-kernel`):

| | our kernel | solidcraft-kernel (truck) |
|---|---|---|
| faces after the fillets | ~12 000 | **18** |
| faces after the chamfer | ~19 600 | **26** |
| time: fillets / chamfer | seconds | **34 ms / 61 ms** |
| volume error against the formula | ~0.3 % | **0.003 %** |

**Decision.** Geometry becomes exact with `solidcraft-kernel`. We use it as a git dependency
pinned to a verified commit, together with their patched copies of truck through
`[patch.crates-io]`. Our layers stay ours:
- `part`: features, regeneration, persistent names;
- `sketch`: the solver, verified in the GUI;
- `document`: format and projects;
- the GUI with Inventor-like behaviour;
- the specifications in `docs/spec`.

The kernel sits behind our interface (`kernel::exact`). No truck or solidcraft type leaves
it. That is their rule and we adopt it.

**Persistent names.** Their kernel records an exact history of every face:
- `FaceOrigin::Profile { segment }`: the side face of a profile segment →
  `Extrusion1:Side:<curve key>`;
- `Cap { end }` → `Extrusion1:Start` / `Extrusion1:End`;
- `Face { operand, face }`: continues a face of the input and inherits its name;
- `Edge { operand, edge }`: fillet or chamfer of an edge → `Fillet1:<names of the two faces>`;
- `Offset`: Shell → `Shell1:<source face>`.

Our rule 2 (names, never indices) stays.

## 2. Inventor ↔ SolidCraft by sections of [FEATURES.md](FEATURES.md)

Legend for the column "Theirs":
- **kernel**: a ready exact operation in `solidcraft-kernel`, we take it directly;
- **engine**: implemented in their command layer (`solidcraft-engine`, `doc`, `io`). The logic
  can be ported, but it is tied to their document model;
- **—**: not there.

| Section (Inventor) | Theirs | What we do |
|---|---|---|
| 0.1 B-Rep kernel | kernel (truck) | **take it** (stage A) |
| 0.3 Visualization | kernel: `display_mesh` (no gaps between faces, bounded facet angle) | take the triangulation, the drawing stays ours |
| 0.4 Commands, Undo | engine: "everything is a command" | **take the principle** (`Command`, JSON); the code is ours |
| 0.5 Parameters and expressions | engine: `parameters.*`, `expr.*` | ours are done; compare for gaps |
| 0.6 Files | kernel: STEP import/export, IGES, STL, OBJ, 3MF | **take** STEP/IGES/STL (stage C) |
| 0.7 API for agents (MCP) | `crates/mcp`, control channel | **take the model** (stage E) |
| 0.8 Persistent names | kernel: `history`, `provenance` | **take the history**, the names stay in our scheme |
| 0.10 Measurement | kernel: `measure` (volume, area, centre, count) | **take it** |
| 1.1–1.9 Sketch | engine: own solver, about 100 commands | ours stays (following Inventor) |
| 1.10 3D sketch | engine: 3D Line/Spline/Point | later, research |
| 1.11 Insert (DXF/SVG, text) | engine: Insert DXF/SVG, Text | later, research |
| 2.1 Extrude | kernel: `extrude`, `extrude_tapered`, `extrude_planar_regions` | **take it** (stage A) |
| 2.2 Revolve | kernel: `revolve` | **take it** (stage A) |
| 2.3 Hole / Slot | engine: `solid.hole` (counterbore, countersink, conical bottom) | interface following Inventor, geometry through the kernel (stage D) |
| 2.4 Fillet | kernel: constant and variable radius, by chord, 3-edge corners (sphere), along a loop | **take it** (stage B) |
| 2.5 Chamfer | kernel: `chamfer`, `chamfer_sides` | **take it** (stage B) |
| 2.6 Shell | kernel: `shell` | **take it** (stage D) |
| 2.7 Rib / Web | engine: `solid.rib`, `solid.web` | research and port (stage D) |
| 2.8 Draft | kernel: `draft` | **take it** (stage D) |
| 2.9 Thread | kernel: `sweep_helix`; engine: `solid.thread` | stage D |
| 2.10 Work features | engine: `construct.*` (19) | ours are done; add the missing ones (Tangent, Along Path) |
| 2.11 Patterns | kernel: `transform`; engine: `solid.pattern.*` | stage D |
| 2.12 Mirror | kernel: `transform_matrix` | stage D |
| 2.13 Booleans and bodies | kernel: `boolean`, `split_body`, `union_apart` | **take it** (stage A) |
| 2.14 Thicken, Offset, Delete/Replace Face | kernel: `offset_faces`, `delete_faces`, `offset_surface`; engine: Replace Face | stage D |
| 2.15 Materials and appearance | engine: `material.*`, `appearance.*` | research |
| 2.16 Tree (Browser) | engine: Timeline (folders, Suppress, reordering) | our interface; the logic is to be ported |
| 2.17 Emboss / Engrave | kernel: `emboss_cylinder`, `raise_band` | stage D |
| 3.x Assemblies | engine: Components, Joints, Contact, Interference, Explode, Motion | long stage, research |
| 3.9 Representations | engine: Configurations | research (like iPart/iAssembly) |
| 4.x Drawings | — | **we develop it ourselves** |
| 5.1 Sweep | kernel: `sweep`, `sweep_path` | stage D |
| 5.2 Loft | kernel: `loft`, `loft_to_point` | stage D |
| 5.3 Coil | kernel: `sweep_helix` | stage D |
| 5.4 Surfaces | kernel and engine: Patch, Stitch, Thicken, Extend, Trim, Offset… | stage F |
| 5.5 Freeform | kernel: `tspline_sheet` | stage F |
| 5.6 Sheet Metal | engine: Flange, Hem, Fold/Unfold, Flat Pattern, DXF | stage F |
| 5.8 Plastic parts | engine: Boss, Lip, Snap Fit, Rest | stage F |
| 5.9 Meshes | kernel: `mesh_body`, `convert_prismatic_mesh` | stage F |
| 6.x Frame, Design Accelerator, Content Center, Weldments, Tube & Pipe, Cable | — | **we develop it ourselves** |
| 7.1 FEA | — | we develop it ourselves |
| 7.2 Dynamic simulation | engine: Motion, Drive Joints | research |
| 7.3 Rendering | `render` (HDR) | research |
| 7.5 Rules (iLogic) | engine: Configuration Rules | research |
| 7.6 AI | MCP, skills, control channel | **take the model** (stage E) |
| 7.7 Data exchange | kernel: STEP, IGES, STL, OBJ, 3MF | stage C |
| 7.9 Command line | `solidcraft-cli` | stage E |

## 3. How they work with AI (and what we take)

- **Instructions in the repo** (`AGENTS.md`, `CLAUDE.md`). A session starts from the local folder
  `plan/` (not in git):
  - `STATUS.md`: current stage and next task;
  - `execution-plan.md`: protocol for autonomous work and which decisions belong to the owner;
  - `architecture.md`: the architecture;
  - ADRs for the decisions (kernel, licences).

  *We take:* ADRs in `docs/adr/` and clearly recorded "owner decisions".
- **"Never crash" comes above everything.**
  - No `unwrap`/`expect`/`panic` in production code (clippy `deny`).
  - Every call into the external kernel goes through `guard`: a panic becomes an error.
  - Every command runs under `catch_unwind` with the design restored.
  - On a crash, a report with the design and the commands for replay is kept.

  *We take:* `guard` around the kernel (stage A), `catch_unwind` around commands, crash reports.
- **Everything is a command.** Commands have stable ids (`solid.extrude`). The ribbon, menus,
  scripts, CLI and AI all go through the same commands with JSON parameters. A programmatic call
  never opens a dialog. *We take:* `Command` in `src/command` (ROADMAP 0.4).
- **AI through MCP.** A server over stdio. It runs without a window or attached to the open
  program, so the user sees how the agent models. The agent measures and takes screenshots; file
  access is limited to the design's folder. *We take:* `api` → MCP with the same commands.
- **Skills for the agent.** Separate skills for modeling, editing and checking. The agent first
  plans the part as numbers, builds it with parameters and measures every step against its own
  calculations. The skills themselves are tested in CI. *We take:* `skills/` with our own skills.
- **Reference tests ("oracle").** Part recipes and expected values from formulas or from
  FreeCAD (run as a separate program). Volume, area, centre, bounding box, face count and
  distance between surfaces are compared. *We take:* `tests/oracle/`. We port their 81 recipes
  as JSON values (with credit), and the expected values are analytic.
- **Control channel.** JSON over a local port with a token: running commands, clicks,
  screenshots. *We take:* instead of our X11 script (`tools/gui_sim.py`), which misses
  clicks when frames are slow.
- **Discipline:**
  - one topic per PR;
  - CI checks fmt, clippy, tests, the layers and the rights to images;
  - for visible changes, screenshots before and after;
  - clean room: no GPL code (not even read), no icons, texts or data from
    commercial CAD.

  *We take* the clean-room and layering rules. Our specifications following Inventor are written
  in our own words; Inventor is not automated and no data is taken from it.

## 4. Plan

| Stage | Content | Check |
|---|---|---|
| **A** | `kernel::exact`: a solidcraft body behind our interface with `guard`. `part` regeneration builds Box/Extrude/Revolve and the Booleans through it; names from the history; triangulation and picking from the exact body | all our tests + oracle 01–06, 13–15 |
| **B** | Fillet/Chamfer through the kernel (our interface and edge selection stay) | your test, oracle 07, 08, 23 |
| **C** | STEP/IGES/STL import and export | round-trip STEP |
| **D** | Shell, Draft, Hole, Mirror, patterns, Sweep, Loft, Coil, Thread, Rib, Emboss | oracle 09–12, 16–18, 21… |
| **E** | Commands → MCP, skills, control channel | skill tests |
| **F** | Surfaces, Sheet Metal, plastic parts, meshes | oracle |

The old faceted kernel stays until stage A passes all tests. Then it is removed.

### State of stage A (2026-10-14)

- `kernel::exact` (`src/kernel/exact.rs`): Box, profiles (lines, arcs, splines as Bézier pieces), Extrude (with a one-direction taper), Revolve, Booleans, Fillet, Chamfer (Distance, Two Distances, Distance and Angle); `guarded` around every call; conversion to our B-Rep.
- `part/exact.rs`: regeneration builds the part exactly. User errors (missing profile, a Cut that does not intersect) become feature health. A kernel error or an unsupported option builds the whole part with the faceted kernel. `PartDocument::exact` tells which one was used.
- Names: faces from the history (`Extrusion1:Side:<segment>`, `Start`/`End`, inherited through the Booleans). Fillets and chamfers are named after the replaced edge (`Fillet1:Round[Box1:Front|Box1:Right]`), so an added edge does not rename the others. Vertices and edges of extrusions and boxes match the old scheme (`VS:<segment>`, `StartEdge`, `Box1:V111`). Old files with faceted face names (`Fillet1#4:Side:Face`) are found by the feature and the edge point.
- Visually checked: the test part (26 faces, valid) and Chamfer with an automatic chain through the GUI.
- Still on the old kernel: Surface output, two-sided taper, Cut with coincident faces (a quarter ring with the same profile).

## 5. What has been taken (2026-10-14)

| What | Where in SolidCraft | Ours |
|---|---|---|
| Ban on panics, enforced by the compiler | `AGENTS.md` ("Never crash"), `clippy.toml` | `#![deny(clippy::unwrap_used, expect_used, panic, unimplemented, todo, unreachable)]` in `src/lib.rs`; `clippy.toml` allows them in tests; only `gui/demos.rs` has a reasoned exception |
| One command for all checks | `xtask/` (`cargo xtask ci`) | `xtask/`: `cargo xtask ci` (fmt, attribution, clippy `-D warnings` with and without GUI, tests with and without GUI, documentation), `cargo xtask oracle`, `cargo xtask assets` |
| Attribution check for files | `xtask/src/assets.rs` | `cargo xtask assets`: every image, font or model must have a row in `ATTRIBUTION.md` |
| Reference tests (oracle) | `tests/oracle/` (81 cases), `docs/oracle.md` | `tests/oracle/` (the same recipes and expected values, without STL) + `tests/oracle.rs`: 36 pass, 44 wait for features we do not have, 1 known failure (`known_failures.txt`, with a ratchet) |
| Test of Boolean identities | `crates/kernel/src/fuzz.rs` | `tests/boolean_identity.rs`: V(A∪B)+V(A∩B)=V(A)+V(B), V(A−B)=V(A)−V(A∩B), every result is a valid B-Rep on our side |
| Full measurement | `measure` → `BodyMeasure` | `ExactSolid::measure_full`: volume, area, centre, bounding box, merged faces, body count |

**What the reference tests found in our code:**
- `Tangent` at a common endpoint joined by `Coincident` (rather than shared) had a double root: the solver stopped at 7e-10 after 200 iterations. It now also recognizes ends joined by Coincident (case 64).
- `to_brep` put all faces of a kernel body into one shell; a union of bodies apart from each other (a box and a sphere) gave an invalid B-Rep (V−E+F−R = 4). Now shells are split by connectivity: separate parts are separate bodies, inner shells are cavities (test `union_of_bodies_apart_gives_valid_solids`).
- The identity test is slow (sphere/torus: up to 50 s per pair): 8 pairs by default, `BOOLEAN_FUZZ_SEEDS=40` for a long run.

## 6. What comes next (by priority)

1. **Recipe runner through `PartDocument`** (not only through the kernel): tests our sketches, features and names; regions from `Sketch::regions` (case 26).
2. **Shell, Draft** (kernel: `shell`, `draft`) → cases 09, 21, 30, 44–47, 59.
3. **Mirror / Pattern** (`transform_matrix`, repeating the feature's tools) → 10–12, 41, 42, 52–54, 60, 62.
4. **Hole** (revolution/tapered extrusion + Cut, like `hole_tools_and_prisms`) → 18, 55–57; the hole matrix as an idea for our own test.
5. **Sweep, Loft, Coil, Thread** (`sweep`, `sweep_path`, `loft`, `sweep_helix`) → 16, 17, 37, 49–51.
6. **STEP/IGES import and export** (`step_import`, `step_export`, `iges_*`).
7. **Everything is a command** (`CommandSpec`, `Session::execute` with a snapshot, `catch_unwind`, recovery on error) → **control channel** (TCP on 127.0.0.1 with a token, synthetic events in `raw_input_hook`, `ViewportCommand::Screenshot`) instead of `tools/gui_sim.py` → **MCP** over stdio.
8. Crash reports with command replay; a "bad parameters do not crash" test for every command.
9. Action matrix (every action on every face/edge of a set of parts, with a ratchet).
