# Vendored crates

## solidcore probe (branch `solidcore-probe`, not on main)

This branch pins `solidcraft-kernel` and `solidcraft-doc` at SolidCraft `8f67c7f` and adds
`solidcore-record`; `vendor/truck-*` is gone. The commit in the section below and its rows of
changes describe main's truck-based copy: rows were carried over only where they still apply.
Additions of this branch:

| File | Change | Why |
|---|---|---|
| `solidcraft-kernel/src/bridge_edges.rs` (new; `ChainLine`, `of`, `stations` in `bridge.rs` made `pub(crate)`) | Loft between surface edges: boundaries cut into sides, one `bridge` per pair, stitched. G0 across, or tangent to a section's surface | upstream has no bridge between picked edges; our truck version did not port |
| `solidcraft-doc/src/sheet.rs` `centre_line` | `#[allow(dead_code)]` | unused on this pin |
| `solidcraft-kernel/src/blend.rs` `face_extent`, `fillet_fits`, `chamfer_fits` | a round or chamfer reaching past the planar face it lies on is refused before SMLib | SMLib built them with the wrong volume; a refusal sends the blend on to `loop_blend` |
| `solidcraft-kernel/src/build.rs` `loft_matched` | a loft keeping the caller's segment pairing | the kernel's `loft` matches corners by direction, which undid a twist and Map Points (volume 50 % off) |
| `solidcraft-kernel/src/body.rs` `Body::without_void` | a void's shell taken out of the record; the faces left keep their history | Delete Face fills a void with no Boolean: SMLib's Union with a solid exactly filling a void stops at export ("a face between two material regions or two empty ones") |
| `solidcraft-kernel/src/surface_trim.rs` `mesh_sides` | a piece's side read off its mesh when SMLib finds no point inside it | SMLib's `face_points` gives status 1001 on a piece of a spline face; Trim failed there |
| `solidcraft-kernel/src/surfaces.rs` `stitch_within_gap` (`repair.rs` `TOLERANCE_BOUND_TESTS` made `pub(crate)`) | Stitch closes gaps up to the user's Maximum Gap, with the check Repair Part already uses | SMLib's plain check refuses an edge sewn across a gap |
| `solidcraft-smlib` (whole crate, from SolidCraft `c7dcab1`) | follows solidcore's main (`513e6fc`, 1344 commits past SolidCraft's pin): retired flags removed (the three `precise_properties_*` flags at `513e6fc`), offset errors by `ErrorKind`, `extended_by_distance` in place of the fitted extension solve | solidcore's speed work (`87ec64f`, `2afb4f9`: the check) and fixes reach us before SolidCraft moves its pin; drop the crate when it has |
| `solidcraft-kernel/src/offset_backend.rs` `thicken` | a closed sheet goes through SMLib's `shell` with no opening | SMLib's thicken takes open sheets only (spheres and eggs were refused) |
| `solidcore` (`[patch]` in `Cargo.toml`: git `velog/solidcore`, branch `perf/boolean-imprint`, `5cc433f72` on solidcore main `513e6fc`) | a B-rep snapshot shares every arena copy-on-write; the curve analysis shortcuts also with `query_statuses_pass_on` (not at or above `GW_MAX_DEGREE`); face test 38 skips loops whose hulls are apart; deleting a face walks the shell only until its vertices are reached; the Boolean's imprint skips the coincident-vertex solves of proto vertices that have proto edges, and a split face keeps each edge definition's UV point (behind `memoize_curve_caches`); removing a face use relinks only its neighbours; a linear sweep renews one notification state over its faces, a stitch's glues and every edit renew their records instead of comparing every key, and a vertex glue rebinds only the geometry it touched; on a plane a point classification passes over inner loops apart (`face_classify_skips_loops_apart`); `CreateFacesWithCurves` checks the HANG-29 coordinate bound once; a linear sweep's caps are one face per outer loop, each loop started at its closing edge (`linear_sweep_caps_one_face`); a shell merge moves its face uses as one list and relinks only the appended ones; a shell's wire walk keeps visited sets instead of a fresh `Marks` | a plate with 1024 holes: 119 s → 32 s with the first three (the check 92 s → 6 s), 27.3 s → 20.7 s with the fourth (measured at different times, each against the run before it); the two imprint fixes 11.3 s → 9.6 s for the Boolean of 1024 holes, 4.31 s → 3.88 s for 512 (side by side on a quiet machine); the extrude of a plate with 1024 holes 36 s → 3.9 s and of 5041 holes 60 s → 31.8 s with the sweep fixes (profiled on a loaded machine; the rest is the body check, 17 s, and the stitch's closure test, 5.9 s); to be proposed upstream, then drop the patch |
| `solidcraft-kernel/src/surfaces.rs` `nearest_face` | a face is found by the distance to its triangles, not to their corners and centres | a ruled side of a square-to-circle loft (few large triangles at this mesh) resolved to the bottom face: Thicken, Copy Object and every pick by a point took the wrong face |

Not rebuilt yet: `thicken_with_tolerance` ignores the tolerance, `chamfer_loops` is plain `chamfer`.
`docs/ROADMAP.md`, "solidcore probe", lists the open failures.

## `solidcraft-kernel`

SolidCraft's kernel crate (https://github.com/tangency-labs/solidcraft),
commit `d4eeedbf95bfcd3cf7a2ec3ce17d4ddf9435e516`, MIT OR Apache-2.0 (`LICENSE-MIT`,
`LICENSE-APACHE`), used through `[patch."https://github.com/tangency-labs/solidcraft"]` in our
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

An addition for frame members (`docs/spec/6.1-frame-generator.md`):

| File | Change | Effect |
|---|---|---|
| `build.rs` `prism_between` (exported in `lib.rs`) | a new function: a region's loops (outer and holes), each as its section on a start and an end plane (matched segments, arcs as conics), joined by ruled sides and closed by planar caps with holes (`uniform_wire`, `ruled_loft_faces`, `cap_and_close`) | a member mitred at both ends in ~15 ms (extrusion + two Trim Solids: ~0.75 s, the split tracing the section and meshing its spline edges) |

An addition for library fasteners (`docs/spec/6.3-content-center.md` §6.0b); not proposed upstream yet:

| File | Change | Effect |
|---|---|---|
| `axial.rs` (new), `lib.rs` export | `pipe_bend`: a pipe bend face by face, no orientation mesh; `pipe_tee`: a straight tee face by face, equal cylinders meeting in exact half ellipses (through the Boolean 25-39 s); `axial_stack_drilled`: round holes parallel to the axis between its planar faces (a flange's bolt holes, DN 600 with 20 holes: ~5 s through the Boolean, see `docs/spec/6.3-content-center.md`); `axial_stack`: a stepped body about Z from its meridian's runs (turned chains of lines and arcs, polygon prisms, a cone cut on a prism's end meeting each flat in one exact rational quadratic), the runs' rings joined by planar faces that keep their edges; a new function, nothing else in the crate changes | ISO 4017 M8 x 30: 234 ms → 4.2 ms; chamfered: 1465 ms → 7 ms; orientation read off the lowest planar face (`Body::new_placed`), no mesh |

A fix (not about speed): a chamfer of a concave straight edge went the wrong way.

| File | Change | Effect |
|---|---|---|
| `blend.rs` `edge_frame` | its convexity test could never fail (the directions were turned to pass it just before), so a concave edge got a convex edge's cutter; now each face's interior side comes from its boundary, as `blend_one` does | an L's inner edge, 1 mm: −102 mm³ → +15 mm³ (the same bug on the d4eeedb pin ends in a refusal there) |

Speed of every general Boolean: what comes after the intersection cost more than the intersection
(a cut over a rounded box: 122 ms in truck, 579 ms in the result's assembly).

| File | Change | Effect |
|---|---|---|
| `heal.rs` `analytic_curve`, `ellipse_arc` | a plane meeting a plane or a cylinder is written from the surfaces (a line, a circle, an ellipse as exact rational quadratics), three samples checking it, instead of 17 samples of the intersection curve; other pairs are fitted as before | heal 260 → 99 ms; an ellipse no longer stays an intersection curve, so the checks after it cost nothing: result mesh 59 → 5 ms, edge reach 72 → 0 ms |
| `boolean_record.rs` `edges_evaluate` | each edge once (`edge_iter` gives a shared edge once per face), and only intersection curves | 283 → 0 ms on that cut |

An addition for Thicken/Offset's Allow Approximation (`docs/spec/2.14-thicken-offset.md`):

| File | Change | Effect |
|---|---|---|
| `surfaces.rs` `thicken_with_tolerance` (exported in `lib.rs`) | `thicken` with the free-form fitting tolerance as a parameter (was the constant 0.01 mm; `thicken` keeps it) | a revolved spline egg 40 mm tall thickens 1.5 mm (its offset is off by up to 0.12 mm at 0.01; fits at 0.3) |
| `delete_face.rs` `delete_shell` (exported in `lib.rs`) | a new function: the shell holding a face taken out of the solid whole | Delete Face's Lump or Void: a closed shell's void fills with material, a lump of a body split in two goes |
| `surface_offset.rs` `fit`: the face's (u, v) span | the boundary is walked in wire order, each search continuing from the last point's parameters (starting at a point that is no pole, the first one inside the surface's range); a hinted search that stalls short of its point falls back to one from nowhere; degenerate edges (poles) are skipped; at a pole the vanishing parameter keeps its last value; a bounded parameter is clamped to its range; a periodic angle takes the turn nearest the last point | thickened revolved egg pieces: 150° was 34 % too thick, 190° 6×, 120° / 180° could not be fitted — now each is its share of the whole egg |

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
| `build.rs` `loft_numbered_loops` (`same_way`) | normals more than a right angle apart mean a plane facing back only if the planes also face opposite ways along the step between the sections | two circles on planes turned 120 degrees about an axis were wound against each other: a ruled loft crossing itself, refused by SMLib's check |
| `build.rs` `loft_matched`, `matched_wires`; `lib.rs` export | a loft through sections whose segments the caller has already paired (same count, same start, all oriented alike): no re-splitting, no turning to a new start, side face `i` numbered `i` | our stations (rails, map points, a twist the automatic matching would undo) keep their pairing |
| `build.rs` `loft_matched_ring`; `lib.rs` export | a closed loop of matched sections: two smooth halves (`smooth_loft_faces`) sharing their end wires, closed into one shell without caps | Closed Loop gives a valid ring (the old smooth loft + ruled closing span + Union was invalid) |

Kernel tests (`cargo test -p solidcraft-kernel --release` in a SolidCraft checkout with this
`src/`): 476 pass; `issue_851_tests::arc_top_edge_sizes_never_panic` fails on Windows with the
original source as well.

A fix made when the pin moved to `d4eeedb` (the upstream commit has the defect):

| File | Change | Effect |
|---|---|---|
| `coplanar.rs` `tool_past_flush_faces`, `ops.rs` `boolean_whole` (before the shifted tries), `whole_face.rs` `traced_onto` (visible in the crate) | a prism tool of a Cut flush with a face of the target, whose faces could not be pushed apart, is swept again from its end face past that face (history kept) before the shifted tries and the nudge: a nudged tool left a skin over the hole, a hair off the face, which `has_skin` cannot see | a bore through a tee pipe: 9 faces and the right area (was 10 faces, area 5 % too large, the same volume) |
| `polyhedron.rs` `shell_convex`, `sew_cavity` | a convex body with analytic faces is shelled by sewing its faces, their offsets and a rim round each open face (the barrel's construction, generalised), checked against the tangent planes at its edges; anything else still goes through the cavity's Cut | shell of a chamfered rounded box 320 ms → 90 ms, and right: the pushed-out cavity took 2.5 % too much |
| `ops.rs` `conical_cut_junction` | the finer tolerance for cuts between conical bodies only for a tool under half the target's size (a countersink, a pattern's copy) | shell of a chamfered rounded box: the cavity's Cut 4 s → 0.3 s |
| `prism.rs` `simple_prism_indexed` | an intersection whose section is one operand's whole section leaves the 2D route only when the heights say the result is that operand too | a cylinder tangent to a box's walls, longer than the box: 1.7 s by a nudged tool → 25 ms, exact |
| `blend.rs` `chamfer_loops` (exported), `record.rs` `rebuilt_placed`, `body.rs` `face_corners` | equal chamfers of whole loops of planar faces by editing the faces (`loopblend`), the loop found on the edge curves, no Boolean, no mesh of the whole body per rim | 6 holes countersunk at both ends: 954 ms to 134 ms; 36 holes 3.2 s to 0.86 s |
| `blend.rs` `blend_wires` (in `chamfer`, `fillet`), `loopblend.rs` `one_circle`, edges by joint vertex, walls in one walk | a pick on a whole circle is blended by the same face edit, when the band it takes of the face is clear of the face's other boundaries (which stands in for `blend_fit`'s checks); `loopblend` no longer walks the whole body per joint | chamfer of 6 / 12 / 36 picked hole rims: 318 / 625 / 2049 ms to 85 / 177 / 541 ms; one rim's edit at 222 faces 12 ms to about 5 ms |
| `blend_fit.rs` `meshed_volumes` (in `right_way`, `volume_in`) | a mesh at 1e-4 of the size, with its sag as the slack, before the full measure; only an unclear result is measured | a chamfer chain round a rounded box: the check 296 ms → a few ms (the chamfer itself 65 ms) |

An addition for library fasteners (`docs/spec/6.3-content-center.md` §6.0b); not proposed upstream yet:

| File | Change | Effect |
|---|---|---|
| `axial.rs` (new), `lib.rs` export | `axial_stack`: a stepped body about Z from its meridian's runs (turned chains of lines and arcs, polygon prisms, a cone cut on a prism's end meeting each flat in one exact rational quadratic), the runs' rings joined by planar faces that keep their edges; a new function, nothing else in the crate changes | ISO 4017 M8 x 30: 234 ms → 4.2 ms; chamfered: 1465 ms → 7 ms; orientation read off the lowest planar face (`Body::new_placed`), no mesh |


A fix (not about speed): a chamfer of a concave straight edge went the wrong way.

| File | Change | Effect |
|---|---|---|
| `blend.rs` `edge_frame` | its convexity test could never fail (the directions were turned to pass it just before), so a concave edge got a convex edge's cutter; now each face's interior side comes from its boundary, as `blend_one` does | an L's inner edge, 1 mm: −102 mm³ → +15 mm³ (the same bug on the d4eeedb pin ends in a refusal there) |


## `solidcraft-doc`

SolidCraft's document crate (`crates/doc`, same commit, MIT OR Apache-2.0, licence files in the
folder), used for its sheet metal model only (`kernel::exact::sheet_metal`). `src/` is the original
apart from the rows below; `Cargo.toml` spells out the workspace inheritance; the two
`#[path = "../../../assets/…/library.rs"]` modules (built-in appearance and material tables) are copied
to `assets/` and their paths point there.

| File | Change | Effect |
|---|---|---|
| `sheet.rs` `corner_pairs`, `near_far_end` | flanges meeting at a corner at any depth: the base pairs, then the flanges on their far edges' ends, level by level (a closed corner stops a bend short of its parent's end: the nearest one within its reach); `close_return_corners` mitres every level (SolidCraft stopped at the first: a tray rim's third flange ran into its neighbours) | Flange auto-mitre of rims (owner's Box 1 / Box 2) |
| `sheet.rs` `contour_flange_span`, `offset_chain` | a contour flange whose width spans `(from, to)` along the sketch normal and whose thickness can be centred on the chain (the chain offset by T/2, corners mitred) | Contour Flange Direction (Default / Flipped / Symmetric / Asymmetric) and Position Centered |
| `sheet.rs` `add_flanges_spans` | `add_flanges_each` with each flange held back from its edge's ends (the flange ends as where a cut-out ends the edge: the base keeps its edge beyond and gets the rule's bend relief) | Flange Width Extents (Edge / Width / Offset) |
| `sheet.rs` `edge_ends` | the world ends of the sheet edge nearest a point | drag arrows at a flange's ends |
| `polybool.rs` `polyhedron_body` (before `Sources`) | a B-rep of planar faces from a closed triangle mesh wound outward (through `rebuild`), history over the triangles | Lofted Flange's thickened strip (a `mesh_body` has no B-rep) |
| `sheet.rs` `cut_through`, `CutMode` | regions of a world plane cut into the panels parallel to it: through the face's panel (`Face`), as a pocket so deep (`Depth`, `record_pocket`), through every parallel panel (`All`), or mapped through the sketch's panel into the flat whole (`AcrossBends`); the material between two signed distances along the normal, whole panels cut out and partial ones pocketed (`Span`), through the nearest panel ahead / behind (`Next`), `cut_side` (the Default direction), `parallel_panels_at`; unfolded, the flat pattern is cut | the sheet metal Cut (`SheetOp::Cut`) |
| `sheet.rs` `SeamKind`, `close_corner_as`, `flange_corners`, `seam_corner` | `close_corner` generalised to a seam kind (Gap: the halving plane as before; Overlap / Reverse Overlap: one flange runs past the other's inner face by a ratio of the thickness, the other stops the gap short of the first's inner face); corners listed and re-closed by flange tags (found with `base_pairs`, which sees corners whose bends a relief moved) | Corner Seam (`SheetOp::CornerSeam`) |
| `sheet.rs` `edge_frame` | a sheet edge's ends, the normal of the face it lies on and the direction out of its panel | Flange / Hem arrows |
| `sheet.rs` `corner_relief`, `reliefs_fit`, `close_corners` | a Round / Rectangular 2-bend corner relief is a cut-out of the flat pattern (`holes`) centred where the two bends' *centre lines* cross (Inventor's placement), through both bends, the base's corner and the closed panels' ends; refused with a note when it cannot clear the point where the bend zones touch, cuts the flat apart or takes a bend away; the corners of one feature get theirs together or not at all (SolidCraft cut a round about the base's vertex and stopped the bends there, leaving steps where the panels ran on) | laser-ready corners (owner, 2026-10-26) |
| `sheet.rs` `close_returns`, `mitre_end`, `SheetFlange::mitre`, `mitred_zone`, `end_offset`, `retract_far_end` | a return's bend is mitred like its panel: its end is a curve in the flat (cubic Hermite pieces, within g/2000 of the exact one) where every point of both faces keeps the half gap from the mitre plane, from its parent's end to where its panel starts — one even gap through the bends, as in Inventor; a bend that must start in from its parent's end has the parent's end lean back to it; only if no curve fits it stops square as before. Gated by `laser_corners` (old files keep SolidCraft's) | rim corners without steps in the flat (owner's Box 1 / Box 2, owner's Inventor picture 2026-10-26) |
| `sheet.rs` `relieve_bend_ends_on_flange` | the rule's bend relief also where a flange on a flange is held back from its edge's ends (Width Extents): a cut-out `relief_width` wide from `relief_depth` into the parent across the bend zone | Flange Offset on a lip |
| `sheet.rs` `RimCorner`, `CornerCut`, `rim_corner`, `cut_corner` (end of file) | Corner Round / Corner Chamfer: the corner between two named rims of a panel (or, flat, the material's outline corner nearest a point) and a cut-out (`holes`) taking the material beyond a tangent round or a straight cut; an inside corner gets the fill instead (`adds`: material unioned into its panel in `cells_with_work` and into `flat_material`); refused for sizes past the rims, cuts taking more than the corner or splitting the flat | Corner Round / Corner Chamfer in the folded part and the flat pattern |
| `sheet.rs` `on_child_bend` (used by `rim_face_at`) | a point by a panel's rim where a flange's bend hangs off that rim is not on the rim face: the bend's cylinders there were named after the rim or not, by where their sample fell (one triangle among equal ones, chosen by the platform's last bits) | the same face names on Windows and Linux (CI `face_names_stay`) |
| `sheet.rs` `HemShape`, `add_hem_shape`, `edge_on_line` | Inventor's hem types as chains of bends on each other's far edges (Single 180°; Double 90°, 90°, 180°; Teardrop a bend past 180° and a run ending T/4 off the face; Rolled a bend alone); the edge's panel is trimmed by the first bend's outer radius first, so the hem's outer face stays where the edge was (SolidCraft's `add_hem` rolls round the edge, and its gap is the inner radius) | Hem types (spec 5.6 §6.1c) |
| `sheet.rs` `DoubleBend`, `join_bend`, `grown` | Inventor's Bend: a sheet of one face joined to this one between an edge of each — one bend tangent to the faces' inner sides (faces extended or trimmed to it), or for parallel faces a Z (Fix Edges: tangent bends and a panel, the edge moved back by √((2R+T)² − dz²) when too close; 45 Degree) or a U (90 Degree; Full Radius); the second face becomes the last flange's panel (`shape`) through `region_to_flat`, its holes cut-outs; edges of different lengths bent over the part they share, each face extended there only or (`aligned`) over its whole edge — a flange panel by a tab in `adds` | Sheet metal Bend (spec 5.6 §6.1d) |
| `splitface.rs` `fit_on` (plane tool) | a plane's section of a round whose traced points lie on one circle is an exact circular arc, not the spline through the points (2 % of the body apart, 0.01 mm off the circle on a r 10 round): a sketch arc on the section then meets the edge, and a Join flush with it no longer fails | lid / base split through rounded corners (owner's box, 2026-10-27) |
| `blend.rs` `chamfer` (before the edge-by-edge fallback) | a chain of curved edges joined smoothly (an ellipse the kernel split where an inclined hole leaves a face) is chamfered as one chain, as `fillet` does: edge by edge each later piece met the earlier chamfer at its ends ("seams on both sides of a corner") | Chamfer of an inclined hole's exit (owner, 2026-10-28) |
| `blend.rs` `blend` (reordering) | an edge order is retried only for failures an order can change (not "not supported yet"), within four times the first attempt plus 2 s: four edges tried all 24 orders, 100–200 s | the inclined hole's exit over a corner is refused in seconds |
| `surfaces.rs` `bridge_edges`, `bridge_loops`, `longest_loop`, `loop_sides`, `picked_chain`, `chain_points`, `side_points`, `interpolated`; `fill.rs` `fill_face` | a surface bridging two open chains of edges: each chain interpolated as a cubic B-spline (chord length), the ruled (homotopy) surface between them, straight lines at the ends; whole closed boundaries cut into sides at corners and bridged pair by pair on the surfaces' own edges (rows through 256 points of each edge), sharing the corner lines; `fill_loop` split so its face can be reused | Loft between the free edges of two surfaces (owner's surface test, 2026-10-29: a fill of the loop stayed 3·10⁻³ mm off, over its tolerance) |
| `surfaces.rs` `stitch` (`t_junction_pieces`, `on_curve`) | before sewing, an edge is cut where another edge's vertex lies on it within the tolerance; edges match when they share their ends and the middle of one lies on the other (by geometry: two curves along one edge may be parametrised differently) | Stitch of surfaces whose edges are split at different places (the owner's lofts between pieces of two surfaces) closes a solid |
| `surfaces.rs` `patch_edges` → `four_sided_patch` | when the fit of a non-planar loop is refused (a hole cut out of a wave folds over its best-fit plane), a loop of four sides is filled by the Coons patch of its sides (grid → cubic tensor B-spline, its boundary curves as the face's edges) | Boundary Patch of the owner's circular hole in a spline surface |
| `body.rs` `mesh_shells_within` → `revolved_cap` | a shell's chord tolerance is no coarser than 2 % of its smallest face on a revolved B-spline profile (cylinders, cones, spheres untouched) | truck's mesher did not finish on a B-spline revolved about an axis in its own plane at a chord of about 4 % of the face (a supplier Creo assembly never finished reading; ROADMAP K7) |
| `splitface.rs` `Field::Curve` (closed), `polyline_on` (in `fit_on`); `smooth_edges.rs` `classify` | a closed polyline of split curves is not extended at its ends (they crossed the outside at its first corner): its field is the distance, positive inside; on a planar face parallel to the curves' plane the split line is the curves' own polyline (corners kept: the spline through the traced points rounded them by the tracing step, 0.6 % of a 20 × 10 hole); a face with a closed edge (one vertex) is not mapped for the smooth-edge test (truck panicked "two same vertices") | Trim / Split Face by a closed sketch loop or a tube's section (spec 5.4a T2) |
| `surfaces.rs` `bridge_edges_tangent`, `Reach`, `outward_row`, `across_edge`, `cubic_facing`; `stitch` (orients a closed result); `body.rs` `orientation_volume` | Loft between edges with the Tangent condition: a cubic across (rows A, A + reach, B + reach, B; reach = weight × the distance between the sections / 3 along the direction out of each surface past its edge: the edge as its face's boundary runs it, crossed with the face's normal), its ends the surface's own boundary curves, whole boundaries' corners shared (directions averaged); each point's surface parameters are searched from the previous point's (1 s → 60 ms); Stitch turns a closed result to face out at `Body::new`'s mesh tolerance first, so `Body::new` does not re-mesh 50 × finer to check a negative sign (10 s for tangent bridges) | Loft conditions between surface edges (spec 5.4a F6) |
| `surfaces.rs` `merged_sides` (in `bridge_loops`), `interpolated_at` (C0 runs) | Loft between whole boundaries with different numbers of sides: the boundary with more keeps the corners nearest the other's (dynamic programming over every start and both directions) and joins its other sides; a bridge row through a corner is interpolated run by run and joined with a triple knot (a smooth spline through a corner rang past it) | Loft between surfaces of different side counts (spec 5.4a F13) |
| `surfaces.rs` `cut_at` (in `bridge_loops`), `on_curve` (range) | a smooth closed boundary (no corners) in Loft between surfaces: cut where it comes nearest the other boundary's corners, or its edges' joints taken as corners against another smooth one; `on_curve` keeps the nearest parameter within the edge's range (the two halves of a circle, the same ends, were taken for one edge and Stitch left the shell open) | Loft from a circle to a square or a circle (spec 5.4a F13) |
| `surfaces.rs` `all_loops` (was `longest_loop`), `bridge_edges_tangent` (holes), `bridge_loops` (returns faces) | Loft between whole boundaries also bridges the surfaces' holes, paired by nearest centre, when both have as many: one surface body of every bridge | a solid through the holes (spec 5.4a F13) |
| `surfaces.rs` `patch_edges_tangent`, `four_sided_patch_with` / `four_sided_from` (Hermite Coons), `smooth_loop`, `span_direction`, `cut_at_points`; `outward_row` (per-edge count, hinted searches retried from the neighbour's parameters, pieces cut from a body edge) | Boundary Patch with the Tangent condition: a bicubic Coons patch with the bounding faces' directions past their edges as cross derivatives (weight × the width), blended near the corners to the neighbouring sides' derivatives, no corner twists (estimated ones pressed the middle flat); a loop without corners is refused (creased) | Boundary Patch Condition Tangent (spec 5.4) |
| `surfaces.rs` `bridge_loops` (corner fade) | Loft Tangent between whole boundaries: the change that makes two bridges share a corner curve is faded in over the last 15 % of each reach row (moving the end point alone kinked the row and twisted the bridge next to the corner) | smoother corners of tangent lofts (spec 5.4a F6) |
| `surfaces.rs` `bspline_sheet`, `bspline_tube` | Freeform: a clamped cubic B-spline sheet whose control net is a cage, and a tube closed (C2) around (the first three rings repeated, uniform knots, the period cut into two clamped faces sharing their seams) | Freeform Plane / Cylinder (spec 5.5) |
| `splitface.rs` `cornered_fit`, `fit_kinked` (in `fit_on`) | sketch curves with corners splitting a curved face: each corner put in exactly (the face's point over it along the sketch's normal) and the split line kinked there, instead of the spline through the traced points rounding it | Trim of a curved surface by a closed sketch rectangle (spec 5.4a T2) |
| `body.rs` `Body::edges` → `edge_polylines`; `blend.rs` `chamfer_tool_sides` (one mesh for its probes) | the edges a mesh would have, divided the same way without meshing a face (looking up the edge nearest a point was most of a blend's meshes); a chamfer tool's seven inside probes share one mesh. The polylines were compared with the mesh's, bit for bit, on all 5996 calls of the whole test suite | 8 edges filleted 55 → 37 ms, chamfered chain of 8 round a rounded top 74 → 38 ms, fillet chain 56 → 42 ms, 4 edges chamfered 38 → 32 ms |
| `body.rs` `mesh_shells_within` (shells side by side); `ops.rs` `union_apart_untimed` (tools' boxes side by side), `join_pieces`; `record.rs` `rebuilt_blend` (all blends in `blend.rs`) | a body of many pieces meshes its shells in parallel; pieces and closed blends of oriented bodies are not meshed again to orient them (open shells still are): the check that would have turned them gave a positive volume on all 1182 calls of the whole test suite, and STEP and stitched solids are tested through them. A closed solid already inside out (`new_placed` with a wrong flag) is not turned here | 36 hole rims 331 → 256 ms, 8 edges filleted 37 → 33 ms, chamfered chain 38 → 34 ms, fillet chain 42 → 37 ms, a cabinet side with 32 shelf-pin tools 435 → 301–316 ms |

## `truck-geometry`, `truck-modeling`

truck (https://github.com/ricosjp/truck, RICOS Co. Ltd., Apache-2.0, `LICENSE`), used through
`[patch.crates-io]` in our `Cargo.toml`: `truck-geometry` 0.5.0 as SolidCraft's fixed copy at commit
`d4eeedbf95bfcd3cf7a2ec3ce17d4ddf9435e516` (its `vendor/truck-geometry`), `truck-modeling` 0.6.0 as
published. Originals apart from the changes below; drop them when the pin moves to a commit that
has them.

A nearest-point search without a hint evaluates a whole grid of surface points on every call:
51 × 51 for a NURBS surface, 101 × 101 for a revolved one (`truck-modeling` does not call
`RevolutedCurve`'s own search there). A Boolean asks this for each point of its intersection curves.

| File | Change | Effect |
|---|---|---|
| `truck-geometry/src/presearch_grid.rs` (new), `nurbs/nurbssurface.rs` `presearch_kept`, `truck-modeling/src/geometry.rs` `revolved_presearch` | the grid is kept per thread for the surfaces searched last (16 NURBS, 8 revolved straight lines: about 3 MB a thread); the same points in the same order give the same answer | wider chamfer 311 → 115 ms, turned tool's Cut 582 → 127 ms, hexagon and shank's Join 100 → 46 ms |

The answers were compared with the plain search, bit for bit, on every search of the whole test
suite (1 013 381, no difference). The crates' own tests keep that comparison; they are outside the
workspace, so run them from each crate's folder: `cargo test --lib presearch` in `truck-geometry`, and in
`truck-modeling` with `--config 'patch.crates-io.truck-geometry.path="../truck-geometry"'`.
