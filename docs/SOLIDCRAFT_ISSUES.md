# Known issues in SolidCraft (for our tests)

A review of the issues and PRs in [tangency-labs/solidcraft](https://github.com/tangency-labs/solidcraft)
(formerly `bherbruck/solidcraft`) as of 2026-10-19. Our pinned version is `4687e3b`
(2026-10-14 05:47 UTC). Marked as:
- **≤pin**: fixed before it, i.e. we have the fix;
- **after**: fixed later, will come with a new version;
- **OPEN**: not fixed anywhere.

The numbers are their issue numbers. The text is our own retelling.

## Shell
- #142 OPEN: saddle corners (a convex and a concave edge at one vertex), corners with ≥4 faces next to curved faces.
- #566 OPEN: a bent profile, sheet-metal trays, a shaft with a keyway (a curved face at an angle to planes).
- #1494 OPEN: box 47.5×45×30 + cylinder r7.5 with its axis along the bottom front edge, Shell 1 mm.
- #319 / #476 ≤pin: a disc with a filleted edge smaller than the thickness.
- #342 ≤pin: a fillet equal to the thickness was not refused on Windows.
- #2178 OPEN: Shell after an extrusion with a negative taper.
- #1972 OPEN: half a sphere, Shell on the hidden half.
- #2258 OPEN: an arched block, a cylindrical hole, Shell 2, then the hole moved by 5 → "Cut: no result".
- Not working yet: Shell thicker than a wall or a step, Shell of a half ring.

## Draft and taper
- #547 / #603 ≤pin: draft of a cylindrical face (R10×20, 5° → truncated cone).
- #390 OPEN: draft of a flange's end face.
- #1126 OPEN: a face whose neighbours are not perpendicular: it goes beyond the edge.
- #1183 OPEN: hexagonal pockets with a draft in a plate with R10 rounded corners (R8 works).
- #2810 OPEN, silent wrong result: tapered extrusion of a contour made only of arcs with sharp corners.

## Fillet
- ≤pin: #851, #556/#646, #237/#238, #334, #796.
- After: #1080/#1124 (a fillet that is too large was accepted), #1113, #884, #1668.
- OPEN: #431 (1 mm along a contour of lines and small arcs r0.3), #1207 (a concave edge to a partial arc of a Ø40 boss), #1039, #1488 (thin strips after a retried Boolean break all following fillets), #2811 (speed: every edge triangulates the whole body).

## Chamfer
- After: #814 (hexagonal hole), #1574 (an edge of a hole next to the bottom face added material), #1428.
- OPEN: #1293 (oracle 63), #1390, #2719 (chamfer = the corner radius gives a sphere with edges outside it; it should be a cone), #1986, #2797 (Two Distances / Distance+Angle on a plane–cylinder edge).

## Booleans
- ≤pin: #69, #260, #308, #487/#506, #545, #707.
- After: #788 (crossed holes of equal radius), #1212.
- OPEN:
  - #423 silent wrong result: Through All cuts behind the sketch plane;
  - #451: a hollow tool removes a body inside the cavity;
  - #1279: a tool coaxial with an existing hole at 0.1 mm;
  - #1647: box ∩ cylinder;
  - #1648: sphere ∪ box;
  - #1649: a tolerance that does not depend on the position;
  - #1865: union along cylindrical faces;
  - #1983: a faceted result in the body → the following Booleans hang;
  - #2963 / #2826 / #2591: a cut ending exactly in the plane of a face → a zero-thickness face;
  - #2231 / #2040: extrusion from a face with rounded ends;
  - #1691: Split Body through a chamfer loses 0.6 % of the volume.

## Sweep, Loft, Coil, Thread
- ≤pin: #881 (loft of polygons with the same number of vertices).
- After: #1184 (a ring-shaped section → a solid body), #1213.
- OPEN: #2847 (sections rotated > 90°), #1140, #1391, #2954 (sweep cut with a 30° twist), #793 (coil split by a plane), #2038 (a modeled thread facets the body), #2705/#2969 (thread diameter tolerance), #1024 (the thread tests fail on Windows).
- Coil on our side: #793 — test `coil_split_by_a_plane_solidcraft_793` in `tests/coil_rib.rs` (Cut with
  a box through half of the helix, a valid body, half the volume); #2038 — Coil Cut facets only
  the ring around the helix (`kernel::exact::coil_boolean`); the test
  `coil_cuts_a_thread_into_a_cylinder` checks that the shaft's end faces stay exact.
- Found by us (the owner's session, 2026-10-20; to report): `sweep_path` (sweep by points — a
  spline as the path) with a **full circle as one arc** (one edge, one vertex) fails with the
  panic "This wire is not simple" / "Two same vertices cannot construct an edge" in
  `smooth_walls` and in `ruled`: the walls are built between the start vertices of the segments.
  On our side: the arcs are split into quarters before the call (`kernel::exact::sweep`,
  `split_closed_arcs`); tests `circle_along_a_spline_*` in `src/part/sweep_tests.rs` and
  `tests/sweep_session.rs`.
- Found by us: `curveblend` (a blend of a closed chain over arbitrary surfaces) refused a
  **curved seam** at a corner of the chain ("a curved seam where the edge chain turns") — and the
  faces of a sweep along a spline meet along B-spline seams, i.e. the edge of the cap could not
  be filleted. Our patch in `vendor/` (`curveblend.rs`): a plane along the seam's tangent at the
  point nearest the contact, until the contact lies on the seam (a fixed point); the faces of
  only one of the edges at the vertex (otherwise the nearer of the two faces switches across the
  crease and the solution does not settle). Test `spline_sweep_then_shell_and_fillet`.
- Shell of a body with B-spline faces (sweep/loft along a curve): `offset` accepts only planes,
  cylinders, cones, spheres and tori — a clear refusal on our side ("free-form faces … cannot be
  offset yet").

## Hole
- #559 OPEN: a 1 mm hole in the 2 mm rim of a hollowed box. On our side: a test in `tests/matrix.rs` ("shell, then a Ø1 hole into its rim (#559)") — it passes.
- Modeled thread (#2038 facets the body): on our side only the zone around the thread is faceted (`kernel::exact::cut_threads`); tests `tapped_m8_modeled_*` in `tests/holes.rs` check that the sides and the hole below the thread stay exact.
- Thread diameter tolerance (#2705/#2969): the sizes from the table are kept unchanged, the printing clearance is a separate field; Thread checks that the size fits the face (tests `thread_on_a_shaft_cosmetic_and_modeled`, `printing_clearance_widens_a_modeled_thread`).
- A hole with a 118° point, Distance = the thickness of a 283 × 136 × 10 plate (the owner's
  session, 2026-10-20): Cut of the tool (a cap exactly on the bottom face, the cone outside the
  body) — 6.1 s, with a 150 mm plate an invalid result (an edge used 0× forward and 2× backward),
  with 50 mm 1.4 s. A flat point or a depth ≠ the thickness — 7–80 ms. On our side the tool passes
  through the body (`part::exact::hole`, test `plain_hole_depth_around_the_thickness` in
  `tests/hole_threads.rs`). To propose: an early path for a cap lying in a face of the body.
- Modeled thread in slabs (2026-10-20, `kernel::exact::slab_threads`). Observations about
  `faceted_boolean`, to propose:
  1. The time grows faster than the length of the groove: M8 with 11 turns in one zone — 13.6 s,
     in slabs of 2 turns — 0.2–0.7 s per slab.
  2. `polybool::Sources::edge` returned `None` for an edge between two faces that have several
     edges between them (a circle split into arcs by a plane away from the cylinder's seam): the
     check looks at the chord between the edge's ends, and the facets of an arc do not lie on it.
     One `None` removes the history of the whole result (all face names). Our patch in `vendor/`:
     such an edge has no origin (`Some(vec![])`).
  3. Intersecting a large body (a 300 mm plate) with a small ring gives an inaccurate result: the
     ring's planes tilted by 1.7e-6 rad, the circles shifted by ~1 µm (the tolerances follow the
     size of the operands). The faceted operation on such a piece always refuses ("not
     oriented and closed" or a negative section volume). On our side: the piece is cut again
     from the body in a box around the zone (`near_zone`).
  4. A slab plane ~0.04 mm from a face of the body or near a row of the cylinder's facets (the
     middle of the hole's tool) makes thin strips and a refusal; on our side the planes are kept
     away from the faces (`ThreadGroove::avoid`) and moved on a refusal.
- #2904 OPEN, silent: a distant body breaks the counterbore.
- After: #2401, #950.

## Pattern and Mirror
- ≤pin: #261/#276 (a pattern of holes was superlinear; fixed only for Cut). Above ~2000 copies it
  was superlinear again — see "Speed of patterns with thousands of copies" below (our patch).
- OPEN: #2076 (a pattern of Joins is quadratic: 225 pins 527 s), #2358 (135 counterbores 19 min), #2341, #1669 (Mirror of cut + loft), #2755 (the copies ignore `targets`).

## Speed of patterns with thousands of copies (our patch, to propose)
A 5n×5n×3 plate with an r1 hole, an n×n pattern at 5 mm (release, one Cut with all copies gathered
with `join_apart`). Before: 2500 holes 31 s, 5041 — 24 min and then the error "lumps: topology exceeds
the shell grouping limit". With the changes in `vendor/solidcraft-kernel` (list in `vendor/README.md`)
and ours: 2500 → 2.2 s, 5041 → 6–7 s, 10000 → 14 s. Proposals to SolidCraft, each with its
measured effect at 2500 holes:
1. `ops::join_pieces`: the pieces are bodies already oriented outward — `Body::new_placed(solid, false,
   None)` instead of `Body::new` (a mesh of the whole compound body just for the sign of the volume): −6.5 s.
2. `boolean_whole`, the early prismatic path: the volumes for the plausibility check should come from
   the 2D sections (area × length, exact), not from meshes of a tool of thousands of pieces and of the
   result: −6.5 s and −1.3 s.
3. `recorded::heal_prism_result`: merging coplanar faces keeps their sides —
   `record::build_placed` instead of `record::build`: −1.3 s.
4. Budgets that grow with the operands: `prism_lineage::Context::caps` (otherwise above ~2000 holes
   the history is lost and the names go through `provenance::trace`, which is quadratic: 9–25 s),
   `prism_tagged` / `stack_tagged` (`curve_work`, `wall_work`) and `MAX_PLANAR_SEGMENTS` (8192 →
   32768; otherwise above ~4000 holes the prismatic path gives up).
5. Quadratic spots: `prism_lineage::Context::whole` (walks all caps for every face:
   10000 holes 11.7 s → 0.85 s), the chain of `boolean2d_tagged` (looks for the next piece among
   all: 3.5 s → < 0.1 s), `Outline::overlap` without a box check (400 bosses: 2 s → 0),
   `sew_connected_sections` (every segment against all end points), `face_surface(i)`
   (`face_iter().nth(i)` — `face_surfaces` added for all at once).
6. `build::extrude`: the body's direction from a cap instead of a mesh of the whole body (10000 holes:
   6.2 s → 0.05 s).
Still remaining: `surf_of` of a cylindrical face ~2.4 ms (parameter search along the edges) — for
10000 faces 6 s even in parallel; Join of a pattern of bosses goes through the general path with meshes
of both operands and `sew_touching_with_sources` (1024 bosses: 9.8 s, from 39 s).

## STEP, triangulation, speed
- STEP OPEN: #1985 (the export does not finish), #1452 (`same_sense` of a cylinder), #1706, #1374/#1400.
- Triangulation OPEN: #1666 (STL of a loft), #2356, #818 (a blind cut splits all vertical edges; we see the same after Shell).
- Speed OPEN: #1219 (a fillet of a hole untouched by the cut makes it 350× slower), #742, #1093, #1856.

## Thicken / offset of free-form faces (our patch, to propose)

- `thicken` fits free-form offsets within a fixed 0.01 mm: a spline egg 40 mm tall needs about 0.3 mm
  to thicken by 1.5 mm. Proposal: a tolerance parameter (`thicken_with_tolerance` in our `vendor/`).
- `surface_offset::fit` took a face's (u, v) span from independent closest-point searches of boundary
  samples. On a surface of revolution the angle is periodic and any angle reaches a pole, so the span
  overran or missed the face: a 150° revolved egg thickened 34 % too thick, a 190° one six times, 120°
  and 180° could not be fitted. Proposal: the continued search in our `vendor/` (see its README row).
- A loft from a circle through a rectangle to a circle has seams that are sharp at the rectangle's
  corners and smooth elsewhere; `thicken` panics there ("Two same vertices cannot construct an edge").
  Expected: an error, or a wall that tapers out.

## Tests we should add on our side
1. A cut ending exactly in the plane of a face or along a fillet's tangent edge: face count, no zero-thickness faces (#2963, #2591).
2. Through All from a sketch in the plane of a face: nothing behind the plane changes (#423).
3. Sphere ∪ box (#1648), crossed cylinders, and moved by (1000, −700, 300) (#1649), box ∩ cylinder (#1647).
4. A tool coaxial with an existing hole at 0.1 mm (#1279); crossed holes of equal radius (#788).
5. Shell: box + cylinder along an edge (#1494); a disc with a fillet < / = / > the thickness (#319, #342); after a tapered extrusion (#2178).
6. Chamfer of a hole edge next to another face, of a cap's circular edge, chamfer = the corner radius (#1574, #1293, #2719).
7. Fillet: a concave edge to a partial arc (#1207), after a Join with a retry (#1488), a contour with small arcs (#431).
8. Draft of a cylindrical face (#547), of a face with inclined neighbours (#1126), a tapered contour made only of arcs (#2810).
9. Loft: polygons vertex to vertex (#881), a ring-shaped section (#1184), sections rotated > 90° (#2847).
10. Split Body through the cone of a chamfer: the volumes of the pieces add up (#1691).
11. Speed: a hole next to an untouched fillet (#1219), a pattern of 100+ Joins and counterbores (#2076, #2358).
12. STEP: fillets and cut arcs stay analytic surfaces (#612, #1198).
