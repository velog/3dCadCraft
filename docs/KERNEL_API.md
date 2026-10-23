# The exact-kernel boundary (`kernel::exact`)

The contract between 3DCadCraft and its exact geometry kernel. Today the kernel is
SolidCraft's `solidcraft-kernel` on truck (`docs/SOLIDCRAFT.md`); a candidate for later is
[solidcore](https://github.com/tangency-labs/solidcore) (a Rust port of NVIDIA's SMLib,
Apache-2.0), which SolidCraft itself is moving to. A new kernel is a new implementation of
**this list**; nothing outside `src/kernel/exact*` may change for it.

## 1. Rules of the boundary

- No kernel type leaves `kernel::exact` (enforced by review; `grep solidcraft_ truck_ src/`
  outside `src/kernel/exact*` must stay empty). Everything crosses as our types: `Point3`,
  `Vector3`, `Plane`, `BRep`, `ExactProfile`, `Real` (mm, radians).
- **`ExactSolid`** is the opaque body: the kernel's solid, `face_names` (one per kernel face, our
  persistent names), `name` (the body's persistent name), `sheet` (a surface body), `profiles`
  (the sketch frames, for edge names).
- **Names come from history, never from indices.** Every operation reports where each result
  face comes from: `Face { operand, face }` (an input face, kept / modified / split),
  `Profile { segment }` (swept from a profile segment), `Cap { end }` (a sweep's start / end),
  `Edge { operand, edge }` (a blend made in place of an edge), `Offset { operand, face }` (a
  shell's inner wall). `names_from` turns that into `<feature>:<role>`. A new kernel must give
  this history for every operation below, or names (and with them fillets, holes, appearances,
  sketches on faces) break.
- **Every call is guarded** (`guarded`: a panic is an error) and **fails without changing its
  inputs**. A refused operation is an `Err(String)`, a "this needs another path" is `Ok(None)`.
- **Deterministic:** the same inputs give the same body, faces in the same order.
- Tolerances: the kernel's own, scaled to the operands; `contains` / `meets` / `bounds` say what
  they measure.

## 2. Operations

| Group | Functions | Contract |
|---|---|---|
| Construction | `box_named`, `profile`, `extrude` (start / end offsets, taper), `revolve` (start, angle, full turn) | faces `Side:<segment>`, `Start`, `End`; a taper grows outward for a positive angle |
| Booleans | `boolean` (`Join`, `Cut`, `Intersect`; `None` when nothing is left), `union_apart`, `join_apart`, `lumps` | the result keeps the first operand's body name; `union_apart` joins bodies apart without intersection work (patterns of thousands) |
| Queries | `bounds`, `contains`, `meets`, `faces_named`, `face_point`, `face_corners` | `faces_named` gives the pieces of a split face nearest a hint first |
| Blends | `fillet`, `fillet_each`, `fillet_variable`, `fillet_chord`, `chamfer` (`Same`, `Distance`, `Angle`) | edges by points nearest them; blend faces named after the replaced edge (`Round`, `Bevel`) |
| Shell, draft | `shell`, `shell_with` (Inside / Outside / Both, Rounded Corners), `draft`, `draft_two_sided` | inner walls `Inner[<face>]`; drafted faces keep their names |
| Split | `split_faces`, `split_faces_lenient`, `split_by_plane`, `split_by_sheet`, `sheet_nearest` | pieces `<face>~<split>.<k>`; parts behind / in front of the tool |
| Transforms | `transformed` (rigid, reflections), `moved` | names kept (a pattern prefixes `<pattern>#k:`) |
| Holes, threads | `revolved_tool`, `cone_tool`, `helix_tool`, `cut_threads` | tools of revolution `[r, z]` profiles; countersinks as tapered discs |
| Sweeps, lofts | `sweep`, `check_bend`, `path_points`, `loft`, `loft_frames`, `loft_to_point`, `loft_guided`, `loft_stations`, `profile_outline`, `profile_area` | faces `Side:<segment of the first section>`, `Start`, `End` |
| Coils | `coil`, `coil_mesh`, `coil_boolean` | |
| Surface bodies | `extrude_sheet`, `revolve_sheet` | `ExactSolid::sheet` |
| Our B-rep, export | `to_brep`, `to_brep_with` (lenient), `step_text`, `step_text_painted`, `step_volumes` | `to_brep`: named topology, analytic surfaces where the kernel knows them, its display triangles per face |

## 3. How a new kernel is proven

The same suite runs on it, unchanged:

- `tests/oracle/` (SolidCraft's reference cases, expected values never edited),
  `tests/boolean_identity.rs`, `tests/matrix.rs` (features × features), `tests/holes.rs` and
  the other feature tests with exact volumes;
- `tests/compat` — every old file opens with the same volume, faces, features, sketches
  **and face names** (`tests/compat/names.json`, see below);
- `tests/perf.rs` with its budgets (`docs/PERF.md`).

Planned (when the candidate is ready): `kernel::exact` as two modules behind cargo features
(`kernel-truck`, `kernel-solidcore`) with this same API, `cargo xtask ci --kernel solidcore`, and
`examples/kernel_diff` building a part (or a replayed journal) with both and listing the
differences in volume, faces and names.

## 4. Truck workarounds (drop them where a new kernel does not need them)

| Workaround | Where | Why (truck / solidcraft-kernel) |
|---|---|---|
| Whole circles split into quarter arcs | `exact/sweep.rs` `quartered`, `map_loop` | one closed edge has a single vertex: sweep / loft panicked ("two same vertices", "wire is not simple") |
| Lofts of strong twists / turns in chunks (≤ 144°, ≤ 60°) joined by Union | `exact/sweep.rs` `loft_frames` | the loft pairs every section with the last one by vertex direction |
| Guided lofts as ≤ 48 planar stations, `sk::loft_matched` (vendor) | `exact/loft.rs` | the kernel lofts only through planar sections, and its matching undid twists |
| `union_apart` / `join_apart` for many tools | `exact.rs` | cutting once per tool took 30 s for 36 holes |
| Modeled threads cut in faceted ring zones, in slabs | `exact.rs` `cut_threads`, `slab_threads` | the exact Boolean with a helical groove took minutes |
| Coil Booleans through a faceted ring zone | `exact/coil.rs` `coil_boolean` | unions of B-spline pieces touching on a section failed after 83 s |
| Countersinks as tapered discs | `exact.rs` `cone_tool` | a revolved cone took 2.6× longer; cones are still slow (ROADMAP K4) |
| A bore reaching the far side cut as Through All | `part/exact.rs` `hole` | a cap on the far face sent the Cut down a 6 s path |
| Lenient B-rep conversion for sweeps / lofts | `to_brep_with` | fitted lines / circles missing an end on lofted faces |
| Pre-checks before the kernel (`check_bend`, `check_folds`) | `exact/sweep.rs`, `exact/loft.rs` | the kernel accepts self-intersecting sweeps / lofts |
| Exact first, faceted rebuild of the whole part on a kernel failure | `part/exact.rs` | the faceted CSG cannot work on a converted exact body |
| Vendor patches | `vendor/solidcraft-kernel` (`vendor/README.md`) | speed of patterns of thousands, history of faceted Booleans |
