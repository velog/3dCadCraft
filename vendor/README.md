# Vendored crates

## `solidcraft-kernel`

SolidCraft's kernel crate (https://github.com/bherbruck/solidcraft, now tangency-labs/solidcraft),
commit `4687e3b0734e41d761be0d0a2611e5402f088348`, MIT OR Apache-2.0 (`LICENSE-MIT`,
`LICENSE-APACHE`), used through `[patch."https://github.com/bherbruck/solidcraft"]` in our
`Cargo.toml`. `src/` is the original apart from the changes below (each with a comment saying
why); `Cargo.toml` spells out the original's workspace inheritance. Everything
else (`solidcraft-geom`, the truck crates) still comes from the pinned commit.

The changes make feature patterns of thousands of copies fast (and keep the face history of
faceted Booleans, last row) (see `docs/PERF.md`,
"Масиви с хиляди копия"); they are proposed upstream in `docs/SOLIDCRAFT_ISSUES.md`. Drop this
copy when the pin moves to a commit that has them.

| File | Change | Effect (release, 2500 holes unless said) |
|---|---|---|
| `ops.rs` `join_pieces` | above `body::ORIENT_MESH_FACES` (256) faces `Body::new_placed` instead of `Body::new`: the pieces are bodies, already outward (smaller solids keep the orientation mesh and its missing-face count) | no orientation mesh of the compound: −6.5 s |
| `ops.rs` `boolean_whole`, `prism.rs` `take_volumes` | the simple prism route reports exact volumes (section area × length) used for its plausibility check | no mesh of the compound tool and the result: −6.5 s and −1.3 s |
| `boolean_record.rs` `heal_prism_result` | above 256 faces `record::build_placed` (merging coplanar faces keeps their sides) | no orientation mesh of the result: −1.3 s |
| `prism_lineage.rs` `Context::caps` | budgets grow with the operands' faces | the prism history is kept beyond ~2000 holes (else `provenance::trace`, 9–25 s) |
| `prism.rs` `MAX_PLANAR_SEGMENTS`, `prism_tagged`, `stack_tagged` | 8192 → 32768 segments; work budgets grow with the body | 5041 holes take the 2D route (before: 24 min, then "lumps: topology exceeds the shell grouping limit") |
| `prism_lineage.rs` `Context::whole` | cap edges per wall counted once for all walls | 10000 holes: 11.7 s → 0.85 s |
| `prism.rs` `boolean2d_tagged` | pieces leaving a point found on a grid, not by a scan | 10000 holes: 3.5 s → < 0.1 s |
| `prism_lineage.rs` `Outline::overlap` | box test before the point tests | 400 bosses: 1.96 s → 0 |
| `prism.rs` `sew_connected_sections` | points that may split a segment taken from a sorted index | quadratic → n log n (bosses) |
| `build.rs` `extrude` | above 256 faces the orientation is read off a cap instead of meshing the solid | 10000 holes: 6.2 s → 0.05 s |
| `surface_kind.rs` `face_surfaces` | every face's surface in one walk (in parallel) | `face_surface(i)` walked to face i each time |
| `polybool.rs` `Sources::edge` | an edge between two faces that share several edges (a circle split into arcs), whose chord test finds none or several, gets no origin (`Some(vec![])`) instead of `None` | one such edge dropped the whole result's history: a modeled thread cut in slabs lost every face name (not a speed change) |

A fix (not about speed): a rolling-ball blend of a closed chain whose corners meet **curved**
seams. Proposed upstream in `docs/SOLIDCRAFT_ISSUES.md` (Sweep).

| File | Change | Effect |
|---|---|---|
| `curveblend.rs` (vertex sections) | a curved seam at a corner of the chain (the walls of a sweep along a spline meet along B-spline curves) was refused ("a curved seam where the edge chain turns"); now the contact is put on the seam by a fixed-point iteration (plane along the seam's tangent at its point nearest the last contact, square to the side), solved against the faces of one edge at the vertex only (with both, the nearer face flips across the crease and the Newton step never settles) | the cap edge of a sweep along a spline can be filleted (1 mm on R6–R17.7 tubes: volume within 0.1 % of Pappus) |

Additions for Loft with rails, a centre line, Area Loft, conditions, point mapping and Closed Loop
(`docs/spec/5.2-loft.md`); not proposed upstream yet (they only expose what the smooth loft
already does without its own matching).

| File | Change | Effect |
|---|---|---|
| `build.rs` `loft_matched`, `matched_wires`; `lib.rs` export | a loft through sections whose segments the caller has already paired (same count, same start, all oriented alike): no re-splitting, no turning to a new start, side face `i` numbered `i` | our stations (rails, map points, a twist the automatic matching would undo) keep their pairing |
| `build.rs` `loft_matched_ring`; `lib.rs` export | a closed loop of matched sections: two smooth halves (`smooth_loft_faces`) sharing their end wires, closed into one shell without caps | Closed Loop gives a valid ring (the old smooth loft + ruled closing span + Union was invalid) |

Kernel tests (`cargo test -p solidcraft-kernel --release` in a SolidCraft checkout with this
`src/`): 476 pass; `issue_851_tests::arc_top_edge_sizes_never_panic` fails on Windows with the
original source as well.

## `solidcraft-doc`

SolidCraft's document crate (`crates/doc`, same commit, MIT OR Apache-2.0, licence files in the
folder), used for its sheet metal model only (`kernel::exact::sheet_metal`). `src/` is the original
apart from the rows below; `Cargo.toml` spells out the workspace inheritance; the two
`#[path = "../../../assets/…/library.rs"]` modules (built-in appearance and material tables) are copied
to `assets/` and their paths point there.

| File | Change | Effect |
|---|---|---|
| `sheet.rs` `add_flanges_spans` | `add_flanges_each` with each flange held back from its edge's ends (the flange ends as where a cut-out ends the edge: the base keeps its edge beyond and gets the rule's bend relief) | Flange Width Extents (Edge / Width / Offset) |
| `sheet.rs` `edge_ends` | the world ends of the sheet edge nearest a point | drag arrows at a flange's ends |
| `sheet.rs` `cut_through`, `CutMode` | regions of a world plane cut into the panels parallel to it: through the face's panel (`Face`), as a pocket so deep (`Depth`, `record_pocket`), through every parallel panel (`All`), or mapped through the sketch's panel into the flat whole (`AcrossBends`); unfolded, the flat pattern is cut | the sheet metal Cut (`SheetOp::Cut`) |
| `sheet.rs` `SeamKind`, `close_corner_as`, `flange_corners`, `seam_corner` | `close_corner` generalised to a seam kind (Gap: the halving plane as before; Overlap / Reverse Overlap: one flange runs past the other's inner face by a ratio of the thickness, the other stops the gap short of the first's inner face); corners listed and re-closed by flange tags (found with `base_pairs`, which sees corners whose bends a relief moved) | Corner Seam (`SheetOp::CornerSeam`) |
| `sheet.rs` `edge_frame` | a sheet edge's ends, the normal of the face it lies on and the direction out of its panel | Flange / Hem arrows |
