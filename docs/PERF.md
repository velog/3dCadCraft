# Performance

Every new feature gets a case in `tests/perf.rs` with a **budget** (median of several
runs, test profile: our crate at opt-level 1, dependencies at 3). A case over budget is
not "fixed" by raising the number: look for a faster solution (first how other systems do
it, see `CLAUDE.md`). The idea of a performance log comes from SolidCraft (`docs/perf.md`).

Target times for interactive work:

| What | Target |
|---|---|
| idle frame (hover, highlighting) | < 33 ms |
| command preview | < 300 ms (computed on a background thread; above that the UI shows "Computing the preview...") |
| OK / regeneration of a typical part | < 1 s |

## Budgets (`tests/perf.rs`)

| Case | Budget | Measured (2026-10-14) |
|---|---|---|
| regeneration of the test part (box, 8 fillets, chamfer along the chain) | 400 ms | 135 ms |
| hollow box (box, sketch on a face, Cut) | 50 ms | 3.5 ms |
| fillet r2 of the 20 edges below the pocket edge | 600 ms | 187 ms |
| chamfer 2 of the same edges (wedges) | 1200 ms | 371 ms |
| Face Draft 3° of the sides and the vertical fillets of a box (`add_draft`) | 200 ms | 55 ms (2026-10-19) |
| Face Draft 3° Symmetric about z = 10 (split, draft of both halves, merge) | 700 ms | 350 ms (2026-10-19, under load) |
| Shell 2 Outside with Rounded Corners on a box without its top face | 400 ms | 190 ms (2026-10-19, under load) |
| Combine: Cut of a cylindrical body from a box (`add_combine`, the whole regeneration) | 300 ms | 28 ms (2026-10-20) |
| Split Solid of a box with 4 vertical fillets by an inclined plane (`add_split`) | 1500 ms | 0.87 s (2026-10-20; `split_body` itself is ~0.8 s; classifying the pieces by volume added 1.4 s — replaced with a point from an edge) |
| Split Face, All Faces of the same box with the plane z = 10 | 500 ms | 110–130 ms (2026-10-20) |
| Split Solid of a 60×40×20 box with a surface from an extruded arc | 500 ms | 18.5 ms (2026-10-20; a cylinder through the kernel's fast tool — `SplitTool::Face` of a rotated cylinder was 10 s) |
| Split Solid of the same box with an open spline (a prism on the front side) | 500 ms | 284 ms (2026-10-20) |
| Shell 1.5 of the test part, top face removed (`add_shell`, the whole regeneration) | 600 ms | 370 ms (2026-10-19) |
| Rectangular Pattern 6×6 of a hole in a plate (`add_pattern`, the whole regeneration) | 2500 ms | 77–81 ms (2026-10-20; before 373 ms) |
| Rectangular Pattern 10×10 of a boss (Join, SolidCraft #2076) | 3000 ms | 1.3–1.4 s (2026-10-20; before 2.2 s) |
| Rectangular Pattern 32×32 = 1024 holes + the preview edges | 3000 ms | 1.46–1.49 s (2026-10-20) |
| Rectangular Pattern 71×71 = 5041 holes + the preview edges | 15000 ms | 7.7–7.9 s (2026-10-20; before 24 min and an error) |
| Mirror of the body with the 36 holes, Join through the right face (the mirror only) | 1500 ms | 260 ms (2026-10-19) |
| Physical Properties of the filleted hollow box (Low) | 100 ms | 0.5 ms (2026-10-19) |
| the appearance of all faces of the same box (`Resolver::face_looks`) | 20 ms | < 0.1 ms (2026-10-19) |
| STEP export with colours: `exact::face_point` only for split names (otherwise 2.5 s) | 200 ms | within budget (2026-10-19) |
| Presets: 100 frames of the Preset row (list with the built-in ones + asterisk check, 20 presets) | 30 ms | 2.5 ms (2026-10-19) |
| Hole: 36 holes Ø5.5 with a counterbore in one feature (`add_hole`) | 2500 ms | 1620 ms (2026-10-19) |
| Hole: M8 with a modeled thread 10 mm (`add_hole`, the whole regeneration) | 12000 ms | 5100 ms (2026-10-19); 4.7 s in slabs on a loaded machine (2026-10-20) |
| Hole: M8 10 deep through a 283 × 136 × 10 plate, 118° point (the owner's session, regeneration) | 500 ms | 6–9 ms (2026-10-20; before 6.1 s in release) |
| Hole: the same M8 made modeled (`replace_feature`, the zone in slabs) | 8000 ms | 2.0 s alone, 4.6 s on a loaded machine; release 1.6 s (2026-10-20; before a refusal after 150 s in release) |
| Journal: 1000 actions (checkpoint line + part delta + volume) on the filleted box | 1000 ms | 425 ms (2026-10-19) |
| Reorder: feature 30 of 30 to the top + regeneration (15 blocks, 14 holes; 28 steps are rebuilt) | 4000 ms | 2.0 s on a loaded machine (2026-10-19) |
| End of Part: to 15 of 30 and back to the end (warm cache) | 300 ms | 1–3 ms (2026-10-19) |
| Regeneration of a box with 30 work features (10 planes, 10 axes, 10 points, a chain) | 20 ms | 2.7 ms (2026-10-19) |
| Sweep: circle along a quarter arc (exact sweep) | 150 ms | 46 ms (2026-10-19) |
| Sweep: square 40 with a 90° twist (3 loft pieces + Join) | 400 ms | 113 ms (2026-10-19) |
| Sweep: circle R17.7 along an interpolation spline (the owner's session; by points, smooth B-spline faces) | 300 ms | 104 ms release, 114 ms test profile (2026-10-20) |
| Loft square → circle (2 sections) | 60 ms | 10 ms (2026-10-19) |
| Loft through 4 sections (smooth) | 100 ms | 20 ms (2026-10-19) |
| Loft with 2 arc rails (bottle Ø20, 21 stations) | 120 ms | 26 ms (2026-10-20) |
| Loft along a quarter-arc centerline (Ø10) | 120 ms | 23 ms (2026-10-20) |
| Area Loft, squares 20 → 40 over 100 mm | 60 ms | 9 ms (2026-10-20) |
| Loft with a Direction 60° condition | 120 ms | 27 ms (2026-10-20) |
| Closed Loop of 6 circles (ring) | 250 ms | 62 ms (2026-10-20) |
| Coil: spring Ø2, 5 turns (exact `sweep_helix`) | 1000 ms | 630 ms on a loaded machine (2026-10-20) |
| Coil: spring Ø2, 5 turns, closed ends (`sweep_path`) | 1000 ms | 790 ms on a loaded machine (2026-10-20) |
| Coil: Cut thread, 6 turns in Ø20 (faceted zone in slabs, one thread per slab; it was 12 s in one zone; the budget is the same as for the M8 modeled thread) | 12000 ms | ~5 s alone, 6.5–12 s on a loaded machine (2026-10-20) |
| Rib: a network of 3 ribs in an open box | 1500 ms | 340 ms (2026-10-20) |
| Rib: a rib with 2 bosses in an open box | 1500 ms | 850 ms (2026-10-20) |
| Project Geometry: regeneration of 200 circles projected from another sketch | 10 ms | 0.1 ms (2026-10-20) |
| Sweep of a circle along a spline between projected centres (the owner's scenario) | 400 ms | 67 ms (2026-10-20) |
| Selection: filter + priority + order of 10 000 candidates under the cursor, ×10 (`command::selection::filter_candidates`) | 20 ms | 3.8 ms (2026-10-20, under load) |
| Command search: one keystroke over 300 commands (`command_search_per_keystroke`, a separate test) | 1 ms | 0.23 ms (2026-10-20) |

## Resource log

Every budget case also records what its median run used and what the machine had (owner's request,
2026-10-26): `cpu` (CPU time of the test process, all threads), `cores` (cpu / wall: how many cores it kept
busy), `peak MB` (highest resident memory during the run, sampled every 5 ms), `machine` (the whole machine's CPU
load over the run) and `others` (the part other programs caused). The table prints them; every run also writes
`target/perf-log/perf-<unix time>.jsonl` — its first line is the machine (CPU model, logical / physical cores,
RAM total / free, OS), then one line per case. A missed budget while other programs used more than 25 % of the
machine says so in the failure message: rerun on a quiet machine before looking for a slowdown.

The process's CPU time includes every test of the `perf` binary running at the same time; for exact per-case
numbers run `cargo test --no-default-features --test perf -- --test-threads=1 --nocapture`.

## Log

- 2026-10-26 — **Laser-ready sheet metal corners.** Round / Square reliefs are now flat cut-outs checked by a flat
  trial (`reliefs_fit`): *Flange on 4 edges of a 200 x 120 Face + flat pattern* 120 ms (budget 200); the other
  sheet cases unchanged. In the same run (another session compiling) *shell 1.5*, *split by an open spline*
  (the open two-mode item below), *36 countersunk holes* and *primitives* were over budget; none of them touches
  sheet metal code.

- 2026-10-26: **Drawing views of large assemblies** (`tests/perf_drawing.rs`, `examples/big_assembly.rs`). 5 000
  components went from 15.7 s to 3.9 s, 20 000 from 114 s to 14.2 s, and the 2 500-hole plate from 3.1 s to 0.84 s
  (front + iso, debug profile). The causes and fixes:
  - The occlusion grid's cells came from the long side only. An assembly seen from the front had about 256 thin
    cells of thousands of triangles each. Cells are now square at about two triangles each, sorted nearest first,
    and a lookup stops at the first triangle behind the point.
  - Silhouette candidates (smooth mesh edges) were welded for every view and every occurrence. They are now
    computed once per component (`ViewModel::smooth`) and only transformed.
  - Duplicate-line cells were 4 × the piece step. They are now about the 0.025 mm tolerance, and an isometric
    crowd went from 8 s to 1 s.
  - Pieces are classified on every core.

  With *Hide small parts* below 30 mm (`Simplify::min_part`), 100 000 components take 2.6 s. Phase times are
  printed with `CADCRAFT3D_HLR_TIMES=1`.

- 2026-10-26 — **Open: two-mode timings.** *shell 1.5 of the rounded box* and *split solid by an open spline* run
  ~400 / ~300 ms in some `perf` runs and ~720 / ~620 ms in others (budgets 600 / 500), with the same code and a
  quiet machine (others < 5 % in the resource log: perf-1793010700 vs perf-1793019755). Alone in a scratch test
  they take 359 / 273 ms. Not yet explained (CPU power state? allocator state after earlier cases?) — to
  investigate before raising anything. New case: *flat extents + bend lines of the tray* 0.5 ms (budget 2).


- 2026-10-26 — **Assembly update, 1000 stacked cubes: 190 → 41 ms** (perf table 91 ms with the sampler). Symmetry
  (`f9fdede`) made `Con::ends()` return a `Vec`, and `passes` scanned every constraint for every component: 1000 ×
  3000 allocations per pass. `ends()` is a fixed array again and the passes look constraints up by component.

- **2026-10-23** — Measure (0.10): the closest pair of the 36-hole plate body against its top face
  took 1027 ms brute force (every sampled point × every triangle), 3.1 ms with box pruning (a
  triangle whose box is farther than the best pair is skipped; touching ends the search). Budget
  300 ms (`tests/perf.rs`).
- **2026-10-20** — the owner's session (`tests/data/m8_modeled_blind_journal.jsonl`): M8, 10
  deep = the thickness of a 283 × 136 × 10 plate, 118° point. (1) The hole itself: 6.1 s in the
  kernel's Cut (a cap exactly on the bottom face, the cone in the air; in a 150 mm plate an
  invalid result). The tool now passes through the body as with Through All: 6–9 ms. (2) The
  modeled thread: a refusal after 150 s (release) — the groove stopped with its corners exactly
  on the bottom face; now it runs out. The zone in one piece is cut in 13.6 s (release), in
  slabs of 2 turns in parallel (`kernel::exact::slab_threads`) — 1.6 s. Slabs cut from a very
  large body (M3 in a 300 mm plate) come out inaccurate and are cut again from the body in a
  box around the zone (`near_zone`, otherwise a refusal after 80–370 s). Tests:
  `tests/hole_threads.rs`.

- **2026-10-20** — patterns with thousands of copies (Rectangular/Circular Pattern of a hole,
  Join of bosses). Probe: `cargo run --release --no-default-features --example pattern_probe -- hole 10 20 32 50 71 100`
  (also `boss`, `circ`); tests `tests/pattern_scale.rs` (the larger ones with `--ignored`).
  Release, plate 5n×5n×3 with a hole r1, n×n at 5 mm (`add_pattern` + the view edges):

  | Holes | Before | After |
  |---|---|---|
  | 100 | 0.43 s | 0.06 s |
  | 400 | 2.1 s | 0.21 s |
  | 1024 | ~9 s | 0.8–1.5 s |
  | 2500 | 31 s | 2.2–3.3 s |
  | 5041 | 24 min, then an error | 6.1–7.7 s |
  | 10000 | — | 14–20 s |
  | Circular 1000 (r0.4, at 1.2 mm) | > 10 min (one Boolean per copy) | 0.5 s |
  | Circular 5000 | — | 9.3 s |
  | Join 400 bosses | 13 s | 1.9–4.9 s |
  | Join 1024 bosses | 35–39 s | 9.8 s |

  Where the time went at 2500 (31 s): a mesh of the compound tool for the sign of the volume in
  `join_apart` (6.5 s), the volumes of the tool and the result for the Boolean check (6.5 + 1.3 s),
  `meets` (2.5 s), lost history → `provenance::trace` (9 s, quadratic), a mesh of the result
  during "healing" (1.3 s), `to_brep` (3.6 s). Fixed on our side: no `meets` for copies, "did it
  cut anything" by the face count, the copies' boxes as oriented boxes with an exact tolerance
  (otherwise Circular = one Boolean per copy), `to_brep` without quadratic spots. Fixed in the
  kernel (`vendor/solidcraft-kernel`, `vendor/README.md`): budgets that grow with the operands,
  no meshes just for the direction, exact volumes of the prisms, the quadratic spots.
  At 5041 now: Boolean ~4 s (2D prismatic path), `to_brep` ~4 s (of which 3 s is surface
  recognition — `surf_of` ~2.4 ms per face, in parallel), the edge preview 0.05 s.

- **2026-10-19** — reordering in the browser: the dependency check (the strings of 30
  features) takes < 1 ms; the cost is the regeneration from the moved place on. A plate with 29
  holes in one body: each Cut ~0.8 s (grows with the face count), a move to the top = 16–27 s
  (as long as Rebuild All) — that is why the case in `perf.rs` uses separate blocks. Improvement
  for later: independent neighbouring features (swapped without shared faces) could use the cache.

- **2026-10-14** — the exact kernel (SolidCraft/truck): the test part from seconds and ~20 000
  faces (faceted kernel) to 135 ms and 26 faces.
- **2026-10-14** — Fillet/Chamfer that the exact kernel refuses no longer build the whole part
  with the faceted kernel (13–19 s per preview, freezing, a possible crash), but give an error in
  < 1 s. The Extrude/Revolve/Fillet/Chamfer preview runs on a background thread.

- **2026-10-14** — many profiles in one Cut (grids, holes): the tools are gathered with
  SolidCraft's `union_apart` (no intersection) and the body is cut once: 36 round holes
  30 s → 1.2 s, a 12×12 checkerboard grid 2.3 s → 0.2 s.
- **2026-10-14** — a cache of the regeneration steps (`part::BuildCache`): key = the previous
  step + the feature + its sketch (geometry and plane); for a new or changed feature only it and
  those after it are built. Edit Feature in the matrix 141 → 93 ms; 36 holes with a chamfer
  5.4 → 4.2 s.

- **2026-10-14** — `to_brep`: surface recognition (`surface_of`) for each face is independent
  and heavy with fillets (torus); it is done in parallel (`std::thread::scope`):
  36 filleted holes 469 → ~80 ms, the whole conversion 0.9 → 0.5 s. A point for each circle
  instead of each half: the same result, no speed-up (the kernel merges the halves itself).

- **2026-10-14** — sketch solving: the Jacobian is a **sparse matrix** (every equation is
  differentiated only over the unknowns it reads); the rank — Gaussian elimination with partial
  pivoting instead of SVD; the degrees-of-freedom analysis — a check "is this motion in the row
  space" instead of an eigendecomposition of JᵀJ; above 150 unknowns the step is solved with a
  sparse LDLᵀ factorization (minimum-degree ordering). A 10×10 pattern of rectangles (808
  unknowns): solve 438 → 2 ms, DOF analysis 730 → 4 ms, a drag step 416 → 12 ms. Tried:
  conjugate gradients (Jacobi and SSOR) — ~500 iterations, 40–50 ms (the matrix of an
  under-constrained sketch is nearly singular). In the tests the fast rank and analysis are
  compared with the slow ones (SVD/eigen).
- **2026-10-14** — the speed tests measured the regeneration cache (0 ms): the clone now gets
  an empty cache (`p.build_cache = Default::default()`).

- **2026-10-19** — Shell: on the test part (26 faces, fillets and a chamfer) the kernel itself
  takes ~230 ms. The body is not convex, so `sk::shell` takes the general path: triangulation
  plus a Boolean cut of the cavity. Our steps (points on the faces, names) ~6 ms,
  `to_brep` ~27 ms. The preview runs on a background thread.
- **2026-10-19** — Shell refused by the kernel with "the rim would have no thickness" (a fillet
  at the removed face ≥ the thickness): the fallback path is no longer run after the refusal,
  which is geometrically proven. The refusal 6.9 s → 78 ms.
- **2026-10-19** — patterns and mirror: the copies of one feature are made with
  `sk::transform_matrix` (under 3 ms for 100 copies) and gathered with `union_apart` into one
  tool, the body is cut/joined once. 100 bosses (SolidCraft #2076, there 225 pins
  in 527 s): `union_apart` ~0.3 s, the union with the plate ~1.0 s, `to_brep` ~70 ms.
  The times swing with the machine's load (parallel compilations).

- **2026-10-19** — Hole: one stepped tool per hole (counterbore + hole in one profile) made 36
  counterbored holes in 43 s (the time grew as n^1.4). Like SolidCraft: first all seats (one
  Boolean), then all holes through their floors (a second Boolean) — 1.6 s.
- **2026-10-19** — modeled thread: a Boolean of the exact kernel with the smooth helical surface
  of `sweep_helix` (B-spline) — 2.5 s for half a turn through a hole, over 60 s once it crosses a
  plane, 8 turns did not finish in 15 min. Instead only the zone around the thread (a ring around
  the hole) is cut faceted (`faceted_boolean` with a mesh of the groove, 36 sections per turn),
  and the rest of the body stays exact: M8 × 10 mm in ~5 s (the zones of different holes are cut
  in parallel). Threads are cut after all features (cached).

## Open

- Assembly solver: v1 (dense, 2026-10-23) took 185 ms for 30 components. v2 (2026-10-24): passes along
  the constraint graph, per-constraint Jacobians — 30 cubes 1.2 ms, 200 / 500 / 1000 cubes with the first
  Mate changed 7 / 20 / 45 ms (budgets 40 / 100 / 200 ms). Loops still solve densely over their
  components: a 1000-part loop would be slow (sparse LDLᵀ as in the sketch solver if it ever matters).

- Countersinks (ROADMAP K4, 2026-10-23): a cone tool through truck's general Boolean, ~0.33 s per
  hole and superlinear in one feature (36 holes ~40 s). `exact::cone_tool` (a tapered disc, as
  SolidCraft builds countersinks) instead of a tool of revolution: 0.87 → 0.33 s for one hole.
  Rejected: cropping truck's interference meshes, bores first.
  **90° countersinks on a face square to the hole** (start and far side) are now the kernel's
  equal chamfer of each bore rim after the bores (`exact::chamfer_circles`): 6 holes chamfered at
  both ends 3.0 s → 0.34 s (budget 1 s), 36 countersunk holes ~40 s → 2.3 s (budget 5 s). Other
  angles, tilted or curved faces and a failed chamfer still cut the cone tools.

- Patterns: recognizing the cylindrical faces in `to_brep` (`sk::face_surfaces`, ~2.4 ms per
  face) is half of the time above 5000 copies; Join of bosses (SolidCraft's general path with
  meshes of both operands, `sew_touching_with_sources`) — 1024 in 9.8 s. See
  `docs/SOLIDCRAFT_ISSUES.md`.

- Modeled thread: in slabs ~0.2–0.7 s per slab of 2 turns (the faceted Boolean), in parallel; a
  refused slab (depending on the position of its plane) is cut again — one more round. A faster
  way: the groove built directly as faces of the zone, without a Boolean.

- Chamfer of 72 semicircular edges (36 holes): ~3 s in the kernel itself (SolidCraft builds a
  tool for each edge and unites them one by one). **Tried and rejected (2026-10-14):** a revolved
  tool for each circle, gathered with `union_apart`, one cut — 100 s (cutting with a compound
  tool of 36 bodies is much more expensive for truck than 36 small ones). Next: in groups of
  nearby holes, or in parallel over independent regions.
- Booleans with a sphere or a torus in `tests/boolean_identity.rs`: up to ~50 s per pair
  (cylinder/sphere, torus/cylinder). Investigate how SolidCraft speeds them up (and whether the
  time is in the operation itself or in the measuring/triangulation) before giving them to the
  user.
- Shell 2 of a disc r20 × 5 with a filleted top edge r1 (bottom removed): ~850 ms in `sk::shell`
  (the rounded face is offset and degenerates into a sharp corner). In the matrix it has a
  budget of 1500 ms.
- Primitives (2026-10-19): a box, a cylinder with a Cut to an inclined work plane (extrusion
  beyond + intersection with a half-space) and a sphere with Join — ~2.9 s in total in the test
  profile (alone, without other compilations), budget 4000 ms. The most expensive are truck's
  Booleans with the sphere; speed-up: one Boolean instead of two for the cut-off (the half-space
  directly as a Cut).
