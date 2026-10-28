# Hard cases other CAD programs report (for our tests)

What users and developers of FreeCAD, OpenCASCADE (OCCT), SolidWorks, Inventor and Onshape report as
geometry that fails, hangs or — worst — comes out wrong without an error. Collected 2026-10-28 from
public issue trackers and forums, to turn into preventive tests (`tests/*_hard_cases.rs`,
`docs/TESTING.md`). It complements [FREECAD_ISSUES.md](FREECAD_ISSUES.md) (FreeCAD's Shell, Draft,
Pattern, Hole, Fillet, Chamfer, Taper, Boolean issues, 2026-10-19) and
[SOLIDCRAFT_ISSUES.md](SOLIDCRAFT_ISSUES.md) (our kernel's).

**Legal.** FreeCAD and OCCT are LGPL: only the issue *texts* were read (through the public GitHub API),
never code, diffs, patches or attached models. SolidWorks and Inventor: public forum threads and help
pages, read as descriptions of behaviour. Everything below is our own retelling; nothing is copied.

Symptoms: **silent** (no error, wrong geometry or result), **fail** (an error), **hang** (minutes or
forever), **crash**, **order** (the result depends on selection or face order).

## 1. Booleans (OCCT; our kernel is truck, the same kinds of traps)

| Source | Case | Symptom | Our test idea |
|---|---|---|---|
| OCCT #1659 | B-spline loft cut by a large cylinder whose flat cap crosses it: the section curve stops ~1 unit short, the result is an imprint (7 faces instead of 5), not a cut | silent | cut a loft by a cylinder whose cap crosses it; volume must drop by the analytic part |
| OCCT #1629 | thread block: helical sweep fused with a bore cylinder, cut from the block; at 11.488 mm depth right, at 11.489 mm two solids: the block whole + the groove inverted (negative volume), reported valid | silent | modeled thread depth sweep in 0.001 mm steps around a few values; volume monotonic, one solid |
| OCCT #1543 | cylinder cut by a circle swept on a helix that leaves through the top face: nothing removed, depending only on where the tool crosses relative to the cylinder's **seam** | silent | same tool turned in 10° steps about the axis: the removed volume must not change |
| OCCT #1534 | cut on a periodic cylindrical / conical face leaves a disconnected wire | invalid | cut tools crossing a cylinder's seam (cf. our Trim along the seam, §6) |
| OCCT #1643 | two valid lofts sharing an end section: Fuse empty, Common zero, Cut larger than its object | silent | two lofts sharing a section: V(A∪B) = V(A)+V(B), V(A∩B) = 0, V(A−B) = V(A) |
| OCCT #1687 | Fuse(S, copy of S) empty with success reported, Fuse(S, S) fine | silent | Boolean identities with a *copy* of the same body (boolean_identity has S with S) |
| OCCT #1610 | two swept tubes meeting at a shallow angle along a shared circle: fused unmerged, still overlapping (a regression in 7.7) | silent | two pipes swept to a common circle at 5–20°: one solid, volume = sum − overlap |
| OCCT #245 | cut where argument and tool share an edge: not completed; rotating or offsetting the tool helps | fail | tool edge on a body edge (also SolidWorks' "zero-thickness geometry") |
| OCCT #1041 / FreeCAD #27252 | fuse fails for 2…7 copies of a body rotated about an axis (180°, 120°, …) but not for one | fail | circular pattern Join of n = 2…7 copies of a blade touching at the axis |
| OCCT #1676, #724 | Boolean of two simple faces takes 12 min / needs > 10 GB | hang | time budget on face–face Booleans (Split by a surface) |
| OCCT #1541 | merging coplanar faces after box − sphere makes the vertex tolerance explode (a 180° phase flip of the circle's 2D curve) | invalid | box − sphere then a later feature on the cut face: valid solid, edges within tolerance |
| SolidWorks help / forums, Onshape forum | **zero-thickness geometry**: bodies meeting only along an edge or at a point (a cut circle tangent to an edge, two blocks touching at an edge) are refused | fail (correct) | ours must refuse too (or keep two lumps) — never a non-manifold edge: Join of two boxes touching along an edge / a vertex |

## 2. Fillet and chamfer (OCCT, SolidWorks, Inventor)

| Source | Case | Symptom | Our test idea |
|---|---|---|---|
| OCCT #1639 | the same fillet (same edges, radius) depends on the order of the shell's faces: one order right, the other reports success with ⅓ of the volume | silent, order | fillet a body built two ways (features in another order, same shape): same volume |
| OCCT #1177, #1568 | a fillet / chamfer that should reach the opposite edge (10 on a 10 mm box; two R5 meeting) fails or crashes; 9.999 works | fail, crash | Full Round is Inventor's answer; plain fillet = half the width: refused cleanly or full round, never a sliver face |
| OCCT #1571, #1569, #1570 | fillet on a cube with rounded corners / a simple shape / beyond some radius: invalid shape | invalid | fillet edges next to existing rounds, radius ramped until refusal: every success valid |
| OCCT #1427 | fillet next to a face drafted 2.9° fails, 2.8° works | fail | draft angle sweep 1°…10° then fillet the drafted edge |
| OCCT #737 | blend of a curved edge bends up on one side and down on the other | invalid | fillet a curved edge between a plane and a cylinder, both convex and concave |
| OCCT #1371 | fillet of the rim of a prism whose profile is one closed B-spline: success reported, self-intersecting | silent | extrude a closed spline, fillet its top rim: valid, volume < prism |
| OCCT #1665 | hook-like solid, smooth edges, fillet 1–1.4 mm not produced | fail | (owner's style part) — small fillets on a swept hook |
| SolidWorks forum, Inventor help | three edges meeting at a vertex: setback corners are hard; same convexity on all three edges has no corner patch in SolidWorks; Inventor's "Minimal" setback succeeds more often | fail | box corner with three different radii (2, 3, 5) and with all equal: valid, the corner patch inside the three |

## 3. Shell / Thicken (Inventor forum; FreeCAD in FREECAD_ISSUES.md)

| Source | Case | Symptom | Our test idea |
|---|---|---|---|
| Inventor forum ("Can't shell or thicken", "Trouble with shell…", "Shell radii problem") | inside radii smaller than the wall: the offset of the round vanishes; users shell before filleting or move End of Part above the fillets | fail | shell t over a part with rounds r < t, r = t, r > t (convex and concave): valid or a clear refusal — our `tests/shell_cases.rs` has part of it |
| Inventor forum (Cadtutor "Edit shell feature failed") | "unable to solve for a vertex": thin faces in the input or a thickness too large | fail | shell of a part with a 0.2 mm face strip |
| Inventor forum ("Loft operation won't allow me to shell") | a loft whose tangent faces are not merged: shell fails; merging tangent faces and unchecking Automatic Face Chain helps | fail | loft with a tangent condition, then Shell removing one cap |

## 4. Sheet metal

| Source | Case | Symptom | Our test idea |
|---|---|---|---|
| FreeCAD SheetMetal #537, #538 | unfold of a part with spline edges / a wrapped cut-out: "intersection cannot be computed", or the spline straightened; a B-spline that is really a line made the arc conversion fail | fail, silent | Cut of a spline profile on a flange, then unfold: the spline stays a spline in the flat, DXF too |
| FreeCAD SheetMetal #513 | a curved face joined to a straight edge: unfold error | fail | contour flange with an arc next to a straight flange |
| FreeCAD SheetMetal #25 | L-shaped plate, two bends on the inner corner; unfolding starting from the bend face at the inner corner fails, from the other works | order | flat pattern of the same part with each base face chosen as fixed: same flat size |
| FreeCAD SheetMetal #361 | hinge shapes (rolled edges) do not unfold | fail | Hem (Rolled type, when we have it) unfolds |
| Inventor forum (flat pattern errors) | Cut inside Unfold / Refold: the bend zone changes shape and the flat pattern fails; users make the flat in the first part and derive | fail | our Cut Across Bend journal (`tests/data/sheet_cut_across_journal.jsonl`) + a cut crossing two bends |
| Inventor forum | the sheet's rule and the unfold rule disagree (style error) | fail | a part whose rule changes thickness after flanges: every flange rebuilds, flat size follows BA |
| Inventor / SolidWorks forums | parts converted from solids (shelled lofts, imported bodies) and compound curves do not flatten | fail (correct) | Convert of a shelled loft: a clear refusal, not a wrong flat |
| SolidWorks forum ("cannot be flattened") | multibody sheet parts, mirrored linked parts, a late cut-extrude, curved panels | fail | flat of each body of a two-body sheet part |

## 5. Assemblies (FreeCAD Assembly, Inventor, SolidWorks)

| Source | Case | Symptom | Our test idea |
|---|---|---|---|
| Inventor help + Arkance KB, Inventor forum | **Angle at 0° / 180° flips**: the directed angle can turn the wrong way there; advice: never drive to or through 0°, use an explicit reference vector | silent | ✅ found in ours too (7.5e-5 rad off, one DOF left) and fixed 2026-10-28 (`angles_at_the_ends_of_their_range` passes) |
| FreeCAD #30585 | a revolute drive of π (or 3.14159) moves only a quarter turn, 3.1418 is fine | silent | drive a joint by π, 2π, 3.14159: the angle reached equals the value |
| FreeCAD #32477 | Fixed joint with opposite connector axes (face to face) settles rotated by exactly 180° | silent | Rigid joint of two faces face to face: the turn about the normal is the one asked |
| FreeCAD #16057, #17625 | cylindrical joint fails unless the parts start nearly aligned; or depending on which part is selected first | fail, order | ✅ starts far away covered (`components_far_from_their_solution`); add: Insert with a / b swapped gives the same placement |
| FreeCAD #30536 | closed loop (crank – rod – piston): the last joint solves only after dragging the rod near its place | fail | four-bar / slider-crank loop closed from a far start |
| FreeCAD #20377, #31986 | a solver that does not converge leaves the parts more broken after each try (grows to 1e9) | silent | ✅ `inconsistent_constraints_fail_without_flying_away`; add: solve twice after a failure, nothing moves further |
| FreeCAD #33557 | several solutions: each recompute picks another (same geometry, different values), noisy diffs | order | solve twice: bit-identical transforms (rule 6: deterministic saves) |
| FreeCAD #31349 | a failed solve only in the console; the user goes on unaware | silent | the GUI path shows every non-converged solve (asm_try already refuses) |
| SolidWorks help / forum | changing a distance makes another mate fail; redundant mates make "over defined" warnings even when consistent | fail | ✅ consistent redundancy covered; add: change the offset of one of two redundant mates — reported as a conflict, not solved halfway |

## 6. Surfaces

| Source | Case | Symptom | Our test idea |
|---|---|---|---|
| Inventor help (Stitch: Maximum Tolerance, Find Remaining Free Edges) | Stitch bridges gaps up to the tolerance and lists the free edges left with their gaps | — | ✅ ours ignores the tolerance (`stitch_leaves_gaps_over_the_tolerance`, ignored); add the free-edge list when it is fixed |
| FreeCAD #17735 | Fill Boundary Curves: "curves are disjoint" for curves that meet | fail | Boundary Patch of 4 edges whose ends meet within 1e-6 (not exactly) |
| FreeCAD #29872 | surface tools ignore the placement of the bodies they use | silent | Patch / Trim of a moved body (Move Bodies first) |
| OCCT #1534, #1543 (seams), ours | tools crossing a periodic face at its seam | fail, silent | ✅ ours: Trim of a tube along its seam plane (`trim_a_curved_wall_along_its_seams`, ignored); add the same for Split and Cut |

## 7. Loft and sweep

| Source | Case | Symptom | Our test idea |
|---|---|---|---|
| FreeCAD #21376 | a loft twisted by more than ~42–45° between sections flips its edges over | silent | loft of squares turned 0°, 30°, 44°, 46°, 89°: side faces never cross (volume follows the twist) |
| FreeCAD #19658, #32215 | loft twisted 180° from the winding order / start vertex of wires (also from STEP wires) | silent | loft of a circle and a square drawn clockwise vs counter-clockwise, start vertex moved: same volume |
| SolidWorks (GoEngineer), Onshape tech tips | loft connectors / vertex counts decide the twist; mismatched counts make the program guess | silent | sections with 3 / 4 / 6 vertices: no self-intersection (`check_folds`) |
| FreeCAD #21832 | fuse of a polar array of helical blades with a swept shaft fails | fail | sweep along a helix + circular pattern of 3 + Join |

## 8. Drawings (hidden lines)

| Source | Case | Symptom | Our test idea |
|---|---|---|---|
| FreeCAD #30101 | two holes with filleted edges at the same height on opposite faces, different sizes: the longer hidden line is not clipped by the smaller hole | silent | plate with two coaxial holes of different Ø from both sides, rims filleted: hidden lines clipped |
| FreeCAD #19442 | requests for a better hidden line removal (long-standing quality gap) | — | — |
| Inventor forum ("Edges missing from drawing", spring) | small parts removed too eagerly; the tangent edges option only adds edges between tangent faces | silent | ✅ coincident hidden lines drawn once (`drawing_hard_cases`); add: a spring / coil front view keeps every turn |
| Inventor forum (sheet metal bends) | tangent edges of curved flanges add many lines | — | tangent edges on/off on a flange with bends: bend lines only once per bend |
| ours | revision cloud round a self-crossing outline | silent | ✅ found and fixed 2026-10-28 (`revision_cloud_round_a_figure_eight` passes) |

## 9. Sketch solver (FreeCAD Sketcher)

| Source | Case | Symptom | Our test idea |
|---|---|---|---|
| FreeCAD #26924, #17579, #10190, #5763, #6709 | adding the last dimension (or changing one) flips a closed profile to the other side or crosses its lines, even "fully constrained" | silent | L profile, every dimension changed by ±50 % one after another: no edge crosses another, orientation kept |
| FreeCAD #20898 | a point on an axis and on a circle reported fully constrained, though two solutions exist | silent | DOF report for two-solution sets (point on line ∩ circle): our analysis may say 0 — it is right only locally; check the solver keeps the nearer solution |
| FreeCAD #14769 | a polar pattern "explodes" the sketch | silent | circular sketch pattern of 12 with the centre on a constrained point: residual 0, nothing moves |
| FreeCAD #13941, #10708 | flips through redundant constraints; directed distances asked for | — | (Inventor has signed driven dimensions; our `Angle { supplement }` is the same idea) |

## Sources

FreeCAD (GitHub issues, text only): #30101, #19442, #33557, #31349, #30536, #16057, #17625, #32477, #30585,
#20377, #31986, #20898, #26924, #17579, #10190, #5763, #6709, #14769, #13941, #10708, #21376, #19658, #32215,
#21832, #17735, #29872, #27252 — <https://github.com/FreeCAD/FreeCAD/issues>.
FreeCAD SheetMetal add-on: #537, #538, #513, #361, #25 — <https://github.com/shaise/FreeCAD_SheetMetal/issues>.
OCCT: #1659, #1629, #1543, #1534, #1643, #1687, #1610, #245, #1041, #1676, #724, #1541, #1639, #1177, #1568,
#1571, #1569, #1570, #1427, #737, #1371, #1665, #1606 — <https://github.com/Open-Cascade-SAS/OCCT/issues>.

SolidWorks: help "Zero Thickness Geometry" (help.solidworks.com, `HIDE_NON_MANIFOLD`), GoEngineer
"SOLIDWORKS Zero-thickness Error", Hawk Ridge "What is Zero Thickness Geometry?", help "Fillet – Corner"
(`HIDD_FILLET_MGR_CORNER`), forum "Recreating setback corner fillets", help "Mates: FAQ" and "Techniques for
Fixing Mate Problems", forum "Distance mates in 2015 broke?", "flipping angle mates", GoEngineer "SOLIDWORKS
Lofts FAQ" and "Managing SOLIDWORKS Loft Connectors", forum threads "Why won't my sheet metal part flatten",
"multibody Sheetmetal wont flatten", "Cannot flatten sheetmetal tube".

Inventor: Help "To Place Angle Constraints in Assemblies" (`GUID-AD4679FD-EE9B-47C0-9E32-04B990BDD29E`),
"To Create Edge Fillets" (`GUID-590A8760-FFBB-41C8-8169-6B7002207884`), "Stitch dialog box"
(`GUID-E2058559-1450-4EE4-AA41-0777E437A4B3`), API `KnitFeature.MaximumTolerance`; Arkance KB "Angle
constraint Flipping 180 degrees in Inventor"; forums.autodesk.com Inventor Forum threads "Can't shell or
thicken", "Trouble with shell feature at desired thickness", "Shell Radii Problem", "Loft operation won't
allow me to shell a part", "Sheetmetal flat pattern error", "Sheet metal part won't unfold or make correct
flat pattern", "Sheet metal style error", "Edges Missing from Drawing", "Sheet metal bend part hide tangent
edge", "Explicit Vector Angle Constraint Issue".

Onshape: forum threads on "Boolean … did not regenerate properly" and loft twist; tech tip "Control loft twist".
