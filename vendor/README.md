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
| `sheet.rs` `corner_pairs`, `near_far_end` | flanges meeting at a corner at any depth: the base pairs, then the flanges on their far edges' ends, level by level (a closed corner stops a bend short of its parent's end: the nearest one within its reach); `close_return_corners` mitres every level (SolidCraft stopped at the first: a tray rim's third flange ran into its neighbours) | Flange auto-mitre of rims (owner's Box 1 / Box 2) |
| `sheet.rs` `contour_flange_span`, `offset_chain` | a contour flange whose width spans `(from, to)` along the sketch normal and whose thickness can be centred on the chain (the chain offset by T/2, corners mitred) | Contour Flange Direction (Default / Flipped / Symmetric / Asymmetric) and Position Centered |
| `sheet.rs` `add_flanges_spans` | `add_flanges_each` with each flange held back from its edge's ends (the flange ends as where a cut-out ends the edge: the base keeps its edge beyond and gets the rule's bend relief) | Flange Width Extents (Edge / Width / Offset) |
| `sheet.rs` `edge_ends` | the world ends of the sheet edge nearest a point | drag arrows at a flange's ends |
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
| `surfaces.rs` `bridge_edges_tangent`, `Reach`, `outward_row`, `across_edge`, `cubic_facing`; `stitch` (orients a closed result); `body.rs` `orientation_volume` | Loft between edges with the Tangent condition: a cubic across (rows A, A + reach, B + reach, B; reach = weight × the distance between the sections / 3 along the direction out of each surface past its edge: the edge as its face's boundary runs it, crossed with the face's normal), its ends the surface's own boundary curves, whole boundaries' corners shared (directions averaged); Stitch turns a closed result to face out at `Body::new`'s mesh tolerance first, so `Body::new` does not re-mesh 50 × finer to check a negative sign (10 s for tangent bridges) | Loft conditions between surface edges (spec 5.4a F6) |
