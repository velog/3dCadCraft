# Test matrix of the main features

`tests/matrix.rs` runs the main features on **simple**, **complex** and **very complex**
geometry. Every case checks that:

- it was built with the exact kernel (or refused with a clear error, where that is the right answer);
- the body is a valid B-Rep (closed, oriented, Euler);
- the volume matches the formula (0.2 %);
- the time is within budget.

`cargo test --test matrix -- --nocapture` prints the table. A case over budget is run once
more and the faster time counts (a busy machine does not fail the case; slow code is slow in
both runs). A new case is added with every
new feature or fixed bug; a budget is never raised above the measured time, a faster
solution is sought instead (`docs/PERF.md`).

| Level | Case | What it checks |
|---|---|---|
| simple | box 40×30×20, cylinder, tube with Revolve, Fillet r5 on an edge, hole Ø10 | basic Extrude/Revolve/Fillet/Cut |
| simple | Extrude Symmetric, Asymmetric 15/5, taper +5°; Revolve 270°; Chamfer Two Distances 2×4 | the options of Extrude/Revolve/Chamfer |
| complex | Intersect (cylinder in a box); boss with a rounded base (concave edge) | Intersect, concave Fillet along a circle |
| complex | plate + a separate body (New Solid, From a face, Cut of that body only); Extrude **To** a face and **To Next** | terminations, body selection (Solids) |
| complex | radial grid (circular pattern of a line through the centre, 4 of 8 sectors) | pattern through the centre, regions, Cut of many profiles |
| complex | checkerboard grid 6×6 with 2 mm ribs | many holes at once |
| complex | "clean" checkerboard (the squares touch at the corners) | edges coincident with the projected outline, regions |
| complex | Cut pointing away from the body | error "does not intersect the body" |
| complex | Edit Feature: hole from Through All to 4 mm, Fillet after it | in-place replacement, the following feature survives |
| very complex | checkerboard grid 12×12 (72 holes) | performance (union_apart) |
| very complex | radial grid with 24 sectors | many regions |
| very complex | 36 holes Ø4 with a 0.5 chamfer / r0.5 fillet on the top edge | Chamfer/Fillet of many circular edges, regeneration cache, parallel surface recognition |

The hollow box (inner/outer fillets and chamfers, edge by edge) is in `src/part/exact.rs`
(`hollow_box`), with the same checks.

## Comparison with Inventor (after every iteration)

| Behaviour | Inventor | Ours |
|---|---|---|
| Circular pattern of a line that starts at the centre | the copies share the centre | ✅ (used to be the error "the two points must differ") |
| Curve on a projected edge | one edge, the profiles close | ✅ (gave 0 regions) |
| Cut, changing the operation | the direction turns towards the material | ✅ `sync_extrude_direction` |
| Cut that removes nothing | a warning | ✅ feature error |
| Regeneration | only from the changed feature onwards | ✅ step cache (`BuildCache`) |
| Every numeric field takes expressions and parameters | yes | ✅ `ExprValue` / `field_row`; Box sizes are parameters |
| Pattern handles in the dialog | yes (live preview) | ✅ |
