# Known geometry issues in FreeCAD (for our tests)

A review of the issues in [FreeCAD/FreeCAD](https://github.com/FreeCAD/FreeCAD/issues) as of 2026-10-19
(searched through the public GitHub API, only the text of the issues and their comments). The old tracker
`tracker.freecad.org` (Mantis) was moved to GitHub in February 2022 — those issues have numbers
~5500–6300 and the text "Issue imported from tracker.freecad.org", so they are included here. The forum
(`forum.freecad.org`) was not searched separately; the threads cited are those the issues themselves link to.

**Legal:** FreeCAD and OpenCASCADE (OCCT) are LGPL. We have not read any code, diffs or patches — only
the descriptions of the geometry and the symptom. The text below is our own retelling; the numbers come from the reports.

Marked:
- **OPEN**: not fixed;
- **FIXED**: closed as fixed (sometimes only with a newer OCCT);
- **REJECTED**: closed without a fix (not planned / no answer);
- **DUPL.**: closed as a duplicate;
- **OCC**: the cause is in the OCCT geometry kernel, not in FreeCAD.

The symptoms are: *failure* (an error, nothing is built), *silent wrong result* (no error, the geometry is
wrong), *invalid body* (Check Geometry fails), *crash*, *slow*.

## Thickness (Shell)

- [#25050](https://github.com/FreeCAD/FreeCAD/issues/25050) OPEN, silent: cylinder Ø40 × 10, Thickness with the top face open; with a thickness equal to the height (10) or more an invalid body comes out instead of an error (forum t=100912).
- [#5634](https://github.com/FreeCAD/FreeCAD/issues/5634) OPEN, OCC: cube 10×10×10, all edges filleted with r1, then Thickness 1 on a flat face → first a crash, today (OCC 7.8 / 8.1) invalid and wrong geometry. Fillet = the thickness.
- [#21472](https://github.com/FreeCAD/FreeCAD/issues/21472) FIXED (as "expected"): body with top edges r1, Thickness 1 on the bottom face. Thickness → Fillet works; Fillet → Thickness throws an exception, works at 0.99. Again r = t.
- [#5604](https://github.com/FreeCAD/FreeCAD/issues/5604) FIXED, OCC: cube 100, Thickness outward (the body grows), then a fillet of the inner corner → invalid body. In OCCT their test still fails.
- [#24132](https://github.com/FreeCAD/FreeCAD/issues/24132) OPEN, OCC: Thickness fails at conical corners (a fillet of a corner between drafted faces); in Part and in PartDesign.
- [#33012](https://github.com/FreeCAD/FreeCAD/issues/33012) OPEN: box with a dome (part of a sphere) on top; Thickness on the bottom face fails at every thickness, Arc and Intersection, inward and outward. With a cylinder instead of the sphere it works.
- [#19184](https://github.com/FreeCAD/FreeCAD/issues/19184) OPEN: hemisphere from a Revolution about Z, Thickness on the flat face fails; about X or Y it works; it also works on the "simple copy". The surface's seam lies in the plane of the open face. In a comment: Thickness after Draft gives an invalid body.
- [#27146](https://github.com/FreeCAD/FreeCAD/issues/27146) DUPL.: block 20×30×10 — Thickness on a straight Pad works, on a Pad with a 4° taper it does not.
- [#32020](https://github.com/FreeCAD/FreeCAD/issues/32020) FIXED: Pad tapered outward → Thickness works; with the same taper inward (narrowing) it fails.
- [#33710](https://github.com/FreeCAD/FreeCAD/issues/33710) / [#27755](https://github.com/FreeCAD/FreeCAD/issues/27755) OPEN: cone from a Pad of a circle with a taper. Thickness with the top or bottom base open fails; with both bases open it gives a tube, but the STL is open (the inner wall is missing). On a cone made another way it works. After a fillet on the cone the next Pocket fails.
- [#21614](https://github.com/FreeCAD/FreeCAD/issues/21614) FIXED (regression): rectangle 210×170 with rounded corners, Pad 50 with a 10° taper, Thickness 4 outward on the top face, Join = Intersection → the body is not hollowed.
- [#23765](https://github.com/FreeCAD/FreeCAD/issues/23765) OPEN: rectangle, Pad with a −2° taper, Thickness 2 outward, sketch on the side face, Pocket → "Null shape".
- [#32018](https://github.com/FreeCAD/FreeCAD/issues/32018) FIXED: three Pads stacked on one side → Thickness fails; with two it works; with a Pad on the other side it works.
- [#6196](https://github.com/FreeCAD/FreeCAD/issues/6196) REJECTED (fixed in OCCT #728): Arc/Tangent fails on mostly concave bodies; Intersection works but does not give what is wanted. Loft square → circle (a duct transition) fails; "inward" sometimes helps.
- [#6055](https://github.com/FreeCAD/FreeCAD/issues/6055) REJECTED: boat hull (Loft of 3 sections); if two lines in the middle section are replaced by nearly straight arcs, Thickness works up to 3.60 mm and fails from 3.70.
- [#15063](https://github.com/FreeCAD/FreeCAD/issues/15063) REJECTED: body from a Loft/Pad with chamfered neighbouring faces; Thickness 0.2 works, 0.3 does not — the wall would cover the opening. Proposal: keep the opening and trim the wall.
- [#24874](https://github.com/FreeCAD/FreeCAD/issues/24874) OPEN, OCC: holder with several pockets; Thickness < 1 works, 1 mm fails — the offset faces meet in one plane.
- [#11527](https://github.com/FreeCAD/FreeCAD/issues/11527) REJECTED: Thickness of a body with a hole with a conical bottom (drill point) → "command not done". Works if the point is not in the topology (a flat hole inside a larger conical one).
- [#5829](https://github.com/FreeCAD/FreeCAD/issues/5829) FIXED (OCC): Thickness on a particular face of a box fails ("case.fcstd").
- [#5622](https://github.com/FreeCAD/FreeCAD/issues/5622) OPEN, OCC, silent: Thickness on a Loft finishes without an error, but the body is unchanged and invalid.
- [#22031](https://github.com/FreeCAD/FreeCAD/issues/22031) OPEN: dashboard made of complex curves with fillets; Thickness on the bottom face fails.
- [#16206](https://github.com/FreeCAD/FreeCAD/issues/16206) OPEN, OCC: Thickness on a complex model fails.
- [#16968](https://github.com/FreeCAD/FreeCAD/issues/16968) OPEN, OCC: Thickness fails because of a check in the fillet builder that gets in the way of the seam correction; in another branch it works only inward.
- [#28569](https://github.com/FreeCAD/FreeCAD/issues/28569) FIXED (crash): tilted cylinder cut with a Polar Pattern; Thickness with 4 top faces and one bottom face → crash, later only a failure.
- [#21937](https://github.com/FreeCAD/FreeCAD/issues/21937) OPEN: two sketches touching at (0,0), one with a Pad upward, the other downward (bodies touching along an edge) → Thickness, Fillet and Chamfer fail with "Null input shape".
- [#27918](https://github.com/FreeCAD/FreeCAD/issues/27918) OPEN, silent: tube from two concentric circles, fillets on the top edges. When the sum of the radii exceeds the wall thickness, a body with zero thickness and a cavity is accepted.
- [#26587](https://github.com/FreeCAD/FreeCAD/issues/26587) / [#31936](https://github.com/FreeCAD/FreeCAD/issues/31936) OPEN: finished part with Thickness and many fillets; Pocket Through All gives broken geometry or a slanted wall instead of a straight slot.
- [#15323](https://github.com/FreeCAD/FreeCAD/issues/15323) OPEN: shell mirrored into a closed body; Pocket makes it solid again.
- [#27266](https://github.com/FreeCAD/FreeCAD/issues/27266) OPEN: plate with holes, fillets and chamfers on the opposite face; after Thickness the chamfer loses its face, yet Thickness succeeds.
- [#6885](https://github.com/FreeCAD/FreeCAD/issues/6885) / [#32490](https://github.com/FreeCAD/FreeCAD/issues/32490): the Pipe and Recto Verso modes are not implemented; the Intersection flag is an OCCT internal detail and users do not understand it.
- [#28189](https://github.com/FreeCAD/FreeCAD/issues/28189) OPEN: the message "BRep_API: command not done" does not say that the wall or chamfer swallows a neighbouring edge.

## Draft (face draft)

- [#9762](https://github.com/FreeCAD/FreeCAD/issues/9762) OPEN, OCC: Draft 60° on a face tangent to neighbouring faces (fillets) → invalid body. The wiki says Draft works only on faces without tangent neighbours — there is no check and no error.
- [#30610](https://github.com/FreeCAD/FreeCAD/issues/30610) OPEN: drip tray; Draft of a curved face between two other curved faces adds material instead of drafting, Check Geometry fails.
- [#28887](https://github.com/FreeCAD/FreeCAD/issues/28887) OPEN: truncated pyramid from a Pad of a square with a −45° taper; Draft on a side face fails at every angle and every neutral plane ("along taper angle too large").
- [#32777](https://github.com/FreeCAD/FreeCAD/issues/32777) OPEN: Draft with a given pull direction and angle 0 → no geometry; 0.01° works (forum t=107570).
- [#19532](https://github.com/FreeCAD/FreeCAD/issues/19532) FIXED: Draft does not accept a negative angle; it should equal "reversed direction" (needed for expressions).
- [#19528](https://github.com/FreeCAD/FreeCAD/issues/19528) FIXED: a sketch cannot be the neutral plane.
- [#26355](https://github.com/FreeCAD/FreeCAD/issues/26355) FIXED: direction from a sketch line — the whole sketch is shown instead of the edge.
- [#9234](https://github.com/FreeCAD/FreeCAD/issues/9234) REJECTED, crash: Draft ~30° on a transition face between two parts → crash while computing the tolerances.
- [#5749](https://github.com/FreeCAD/FreeCAD/issues/5749) OPEN: Draft on a body from a Loft does not work; on a cube or a cleaned-up body the angle comes out reversed.
- [#27250](https://github.com/FreeCAD/FreeCAD/issues/27250) OPEN, slow: body with four filleted sides, Draft → 100 % CPU (macOS).
- [#13217](https://github.com/FreeCAD/FreeCAD/issues/13217) OPEN: Draft, fillets and an angled Pad; Refine does not merge the fillet's faces.
- [#19184](https://github.com/FreeCAD/FreeCAD/issues/19184) (comment): Thickness after Draft → invalid body.
- [#33548](https://github.com/FreeCAD/FreeCAD/issues/33548) / [#32522](https://github.com/FreeCAD/FreeCAD/issues/32522): Step Draft and Parting Line Draft are missing; "Neutral Plane" and "Pull Direction" are not understood without hints.

## LinearPattern / PolarPattern / MultiTransform / Mirrored

- [#7791](https://github.com/FreeCAD/FreeCAD/issues/7791) FIXED: LinearPattern with 1 occurrence → "Recompute failed" (needed for a parametric count).
- [#18473](https://github.com/FreeCAD/FreeCAD/issues/18473) OPEN: count 0 → error; the Boolean with an empty body fails as well.
- [#5964](https://github.com/FreeCAD/FreeCAD/issues/5964) FIXED: patterns were refused by a wrong criterion ("the copy does not intersect the body"), although the copies are valid.
- [#5704](https://github.com/FreeCAD/FreeCAD/issues/5704) FIXED: Mirror of a feature that only touches the mirror plane: the contact is not counted as an intersection and the mirror is refused.
- [#5917](https://github.com/FreeCAD/FreeCAD/issues/5917) FIXED (OCC 7.7): Polar Pattern of a Groove on a revolved body; with some counts part of the copies are missing. Moving the arc centre 0.1 mm off the edge helps (coincident geometry).
- [#18720](https://github.com/FreeCAD/FreeCAD/issues/18720) OPEN: gear — Polar Pattern with 36 occurrences is fine, with 35 (and any other count) it is wrong. Fix: Refine = False on the helical base.
- [#27252](https://github.com/FreeCAD/FreeCAD/issues/27252) OPEN, OCC: Polar Pattern of a Pad with a negative taper: up to 5 occurrences works, from 6 it does not; with 3 occurrences it "succeeds" without a visible result; at 43 FreeCAD hangs. Counts 13 and 20 are also remembered.
- [#12099](https://github.com/FreeCAD/FreeCAD/issues/12099) OPEN: Pocket relative to a work plane, then Polar Pattern: 2 occurrences (no overlap) are fine, 6 overlapping cuts break the model. With a sketch on the same plane it works.
- [#30866](https://github.com/FreeCAD/FreeCAD/issues/30866) OPEN, silent: Polar Pattern of cuts — one is missing without an error, while the preview is right (the base is a Thickness).
- [#29375](https://github.com/FreeCAD/FreeCAD/issues/29375) FIXED, silent: Polar Pattern of a slot with 5 occurrences — shows 5, builds 2.
- [#26513](https://github.com/FreeCAD/FreeCAD/issues/26513) / [#26097](https://github.com/FreeCAD/FreeCAD/issues/26097): Polar Pattern of a Pocket in a dome — the copies remain only as outlines on the faces, without a cut.
- [#16931](https://github.com/FreeCAD/FreeCAD/issues/16931) / [#5925](https://github.com/FreeCAD/FreeCAD/issues/5925) FIXED: bearing cage — the pattern of cuts cuts only some of the copies, the union works.
- [#30971](https://github.com/FreeCAD/FreeCAD/issues/30971) OPEN: Polar Pattern of a Subtractive Pipe on a Revolution — with counts other than 2 and 4, at least 2 copies are wrong.
- [#15018](https://github.com/FreeCAD/FreeCAD/issues/15018) / [#17730](https://github.com/FreeCAD/FreeCAD/issues/17730) / [#18588](https://github.com/FreeCAD/FreeCAD/issues/18588) OPEN, OCC: Polar Pattern or Mirror of a subtractive helix — artefacts, parts without a cut.
- [#17463](https://github.com/FreeCAD/FreeCAD/issues/17463) FIXED: two-start internal thread (pitch 4) via a Polar Pattern of a helix: with an 8 mm cylinder it works, with 10 mm it does not.
- [#17392](https://github.com/FreeCAD/FreeCAD/issues/17392) OPEN, OCC: Additive Pipe along a curved path; LinearPattern 40 mm with 2 occurrences, Mirror about YZ or Polar 360° with 2 occurrences → wrong geometry. If the bend is added after the pattern, it works.
- [#6281](https://github.com/FreeCAD/FreeCAD/issues/6281) OPEN: LinearPattern of a flange from a Pipe along an ellipse does not work; along a circle it works.
- [#6285](https://github.com/FreeCAD/FreeCAD/issues/6285) / [#6210](https://github.com/FreeCAD/FreeCAD/issues/6210): Polar Pattern of a Loft (a column on a ring) does not work when the copies do not overlap.
- [#24994](https://github.com/FreeCAD/FreeCAD/issues/24994) FIXED, silent: elbow from an Additive Pipe with a volume of 1 402 248.2 mm³, mirrored twice; expected volume 5 608 992.8, got 1 402 248.2; the body is invalid, 4 circular edges remain after Refine.
- [#28396](https://github.com/FreeCAD/FreeCAD/issues/28396) REJECTED: two consecutive Mirrors in a MultiTransform; the original loses a feature.
- [#28399](https://github.com/FreeCAD/FreeCAD/issues/28399) REJECTED, silent: MultiTransform (LinearPattern twice) of a pin with a chamfer — the preview is right, the copies have no chamfer.
- [#28774](https://github.com/FreeCAD/FreeCAD/issues/28774) DUPL.: chamfer on edges of a mirrored Pad; Equal Distance is right, Two Distances and Distance+Angle ignore the mirrored copy.
- [#26815](https://github.com/FreeCAD/FreeCAD/issues/26815) OPEN: Mirror after a Fillet fails with an empty message if the plane is "Base XZ"; with "Horizontal sketch axis" (the same plane) it works; without the fillet it works.
- [#14629](https://github.com/FreeCAD/FreeCAD/issues/14629) FIXED, silent: Mirror of a body with a Pocket gives only the original half.
- [#15857](https://github.com/FreeCAD/FreeCAD/issues/15857) / [#23750](https://github.com/FreeCAD/FreeCAD/issues/23750) / [#15852](https://github.com/FreeCAD/FreeCAD/issues/15852): LinearPattern of a feature with holes breaks the body even with one occurrence; the hole is shown as a bump.
- [#30858](https://github.com/FreeCAD/FreeCAD/issues/30858) OPEN: pattern of a hole that crosses the edge of a chamfer → the holes close up, a neighbouring face disappears.
- [#19337](https://github.com/FreeCAD/FreeCAD/issues/19337) OPEN, crash: two rings of chamfered holes (Polar Pattern of a hole + chamfer); when the chamfers of the two rings overlap (~1 mm) → crash; 0.9 mm works.
- [#12914](https://github.com/FreeCAD/FreeCAD/issues/12914) FIXED: a suppressed feature is still copied by the pattern.
- [#15668](https://github.com/FreeCAD/FreeCAD/issues/15668) FIXED: the result depends on the order in which the features were selected for the pattern.
- [#23975](https://github.com/FreeCAD/FreeCAD/issues/23975) FIXED, crash: LinearPattern of a hole — 9 occurrences work, at 10 a crash.
- [#20699](https://github.com/FreeCAD/FreeCAD/issues/20699) OPEN: knurling — cylinder, helical cut, Polar Pattern, Mirror → does not work (there is a workaround).
- Speed: [#18205](https://github.com/FreeCAD/FreeCAD/issues/18205) OPEN — a torus with a cutout, Polar Pattern and two Mirrors: recompute 10 s – 2 min; [#29536](https://github.com/FreeCAD/FreeCAD/issues/29536) FIXED — Polar 8 → 9 occurrences freezes the system; [#25394](https://github.com/FreeCAD/FreeCAD/issues/25394) OPEN — a circular pattern hangs.

## Hole (countersink, counterbore, modeled thread, clearance)

- [#8777](https://github.com/FreeCAD/FreeCAD/issues/8777) FIXED (with a tooltip): countersink with depth 2 and angle 90° vs 45° — different depths. Conclusion: the depth is how far the top of the head is below the surface, not the depth of the cone.
- [#31527](https://github.com/FreeCAD/FreeCAD/issues/31527) OPEN: the countersink's Head Depth in the dialog is measured to the theoretical apex of the cone, while the drawing shows it to the shoulder; the measured value is smaller.
- [#5879](https://github.com/FreeCAD/FreeCAD/issues/5879) FIXED: the depth of a blind hole includes the 118° conical point; a "without the point" choice is needed (according to the standards).
- [#5894](https://github.com/FreeCAD/FreeCAD/issues/5894) OPEN (request): countersink with an extra counterbore on top; clearance above the head (Through All backward).
- [#23858](https://github.com/FreeCAD/FreeCAD/issues/23858) OPEN, silent: plate 5 mm, hole Ø10 with a 50° taper; at depth 5 it works, at 7 the cone closes before the bottom, the preview silently stops, OK gives "Could not revolve sketch".
- [#26816](https://github.com/FreeCAD/FreeCAD/issues/26816) FIXED: tapered hole (pipe thread, Through All, modeled thread) does not cut fully through the walls; at a small depth it works.
- [#15353](https://github.com/FreeCAD/FreeCAD/issues/15353) OPEN (regression): 4 threaded M3 holes in one sketch → "Removing splitter failed"; with one hole it works; without the thread it works. Fails when two holes are at diagonally symmetric points (±x, ±y). Cause: the clean-up (Refine) after the cut.
- [#18105](https://github.com/FreeCAD/FreeCAD/issues/18105) OPEN: after a hole with a modeled thread the next Pad fails ("Removing splitter failed").
- [#21325](https://github.com/FreeCAD/FreeCAD/issues/21325) DUPL.: the modeled thread leaves broken faces ("Invalid Tolerance Value"); then a Subtractive Pipe fails with "Base shape is not valid".
- [#19254](https://github.com/FreeCAD/FreeCAD/issues/19254) DUPL.: M10, blind hole 15 in a body of 25, modeled thread → the thread is not built; Through All or a depth > the body works.
- [#17846](https://github.com/FreeCAD/FreeCAD/issues/17846) / [#21097](https://github.com/FreeCAD/FreeCAD/issues/21097): the automatic Refine breaks threads and helical bodies; a Boolean with a threaded rod crashes.
- [#30661](https://github.com/FreeCAD/FreeCAD/issues/30661) OPEN, silent: thread from a Boolean with a tap, then a Pocket; at pitch 1.55 it works, at 1.53 and 1.52 the result is wrong without an error.
- [#11912](https://github.com/FreeCAD/FreeCAD/issues/11912) FIXED: the thread clearance accepts only positive values; a negative one shows in the preview, but OK resets it to 0.
- [#32048](https://github.com/FreeCAD/FreeCAD/issues/32048) REJECTED: M2 → drill 2.4, M3 → 3.4 (these are clearance holes, not tap drills) — confusion about which diameter is used.
- [#17825](https://github.com/FreeCAD/FreeCAD/issues/17825) / [#23703](https://github.com/FreeCAD/FreeCAD/issues/23703) OPEN: a hole on a cylindrical face gives "1.5 holes" (a phantom half at the top edge), does not go through both walls even with depth 200 at a distance of ~150. It should either work or refuse.
- [#30190](https://github.com/FreeCAD/FreeCAD/issues/30190) OPEN: a hole through two differently inclined faces works only if the depth equals exactly the cylinder; a longer tool does not cut.
- [#15570](https://github.com/FreeCAD/FreeCAD/issues/15570) REJECTED: Through All + Midplane cuts in one direction only.
- [#17335](https://github.com/FreeCAD/FreeCAD/issues/17335) FIXED: if the hole is the last feature, the STL does not contain the counterbore/countersink.
- [#11527](https://github.com/FreeCAD/FreeCAD/issues/11527): Thickness of a body with a conical hole point fails (see Thickness).
- [#30858](https://github.com/FreeCAD/FreeCAD/issues/30858), [#19337](https://github.com/FreeCAD/FreeCAD/issues/19337): patterns of holes crossing chamfers (see Pattern).

## Fillet

- [#33618](https://github.com/FreeCAD/FreeCAD/issues/33618) DUPL., old: a 20 mm wall, two fillets (or chamfers) of 10 each → "command not done"; 10 + 9.99 works.
- [#18556](https://github.com/FreeCAD/FreeCAD/issues/18556) OPEN: too large a chamfer/fillet on the top edge of a cylinder — old versions refused, 1.1 builds an invalid body. r = R (5 on R5) works now; on a cube it does not.
- [#27918](https://github.com/FreeCAD/FreeCAD/issues/27918) OPEN, silent: tube, fillets on the inner and outer top edges that overlap → zero thickness, a large bogus face above the model.
- [#19567](https://github.com/FreeCAD/FreeCAD/issues/19567) DUPL.: hollowed box, fillet along the edge above 10 mm → failure.
- [#5799](https://github.com/FreeCAD/FreeCAD/issues/5799) OPEN, OCC: torus cut in half and in half again; a fillet of an edge fails, while on a more complex body merged with it, it works.
- [#5665](https://github.com/FreeCAD/FreeCAD/issues/5665) OPEN, OCC: cube ∪ sphere, fillet of an edge next to the sphere → wrong result; still fails with OCCT 8.1.
- [#18383](https://github.com/FreeCAD/FreeCAD/issues/18383) OPEN: cylinder with a spherical dent (a Groove of a semicircle) on its side; fillet of the edge → failure. If the arc is outside the body, it works.
- [#27456](https://github.com/FreeCAD/FreeCAD/issues/27456) OPEN, OCC: a 3 mm fillet on an edge of a cube that ends exactly at the apex of the cylinder's generatrix → failure (StudyCADCAM exercise 150; Onshape does it).
- [#21665](https://github.com/FreeCAD/FreeCAD/issues/21665) OPEN, OCC: Pad with a −2.8° taper → the fillet works, −2.9° / −3° → it does not; far from other edges.
- [#32309](https://github.com/FreeCAD/FreeCAD/issues/32309) / [#31868](https://github.com/FreeCAD/FreeCAD/issues/31868) OPEN: an angle between the faces of 174.27° works, 174.28°…185.73° → "no suitable edges". Threshold 0.1 rad for "tangent". The same for an edge between two Lofts (with a ruled Loft it works).
- [#33432](https://github.com/FreeCAD/FreeCAD/issues/33432) OPEN: a fillet of several distant faces at once fails, one by one it works.
- [#17452](https://github.com/FreeCAD/FreeCAD/issues/17452) OPEN: rectangular Pad, fillet of an edge, then of a neighbouring edge in a new feature → all three edges at the common vertex get filleted; or "Tip shape is empty".
- [#5932](https://github.com/FreeCAD/FreeCAD/issues/5932) OPEN: a fillet fails on a body from a Revolution, while on the same body from a Pad it works.
- [#16654](https://github.com/FreeCAD/FreeCAD/issues/16654) OPEN: a fillet fails after Refine, without Refine it works (the same edges).
- [#20586](https://github.com/FreeCAD/FreeCAD/issues/20586) OPEN, OCC: fillet 0.01 along a face — one order of operations always works, another always fails.
- [#8549](https://github.com/FreeCAD/FreeCAD/issues/8549) OPEN: cylinder R25 × 10, fillet 1 → waves along the fillet (display, STEP and STL); present for R10…R64, absent from R65. Triangulation of a torus/fillet.
- [#29476](https://github.com/FreeCAD/FreeCAD/issues/29476) / [#30886](https://github.com/FreeCAD/FreeCAD/issues/30886) OPEN: a fillet/chamfer makes parts of neighbouring faces disappear (a triangle remains).
- [#26152](https://github.com/FreeCAD/FreeCAD/issues/26152) OPEN: a fillet > 1 mm of an edge meeting an intersecting cylindrical face bends inward.
- [#25916](https://github.com/FreeCAD/FreeCAD/issues/25916) / [#17845](https://github.com/FreeCAD/FreeCAD/issues/17845) / [#18179](https://github.com/FreeCAD/FreeCAD/issues/18179): the fillet spills past the edge, creates extra bodies; the 4th fillet on a ring takes minutes and deforms the model.
- [#14604](https://github.com/FreeCAD/FreeCAD/issues/14604) OPEN: a fillet on a circular extrusion stops halfway.
- [#19383](https://github.com/FreeCAD/FreeCAD/issues/19383) OPEN: dome with a flat top, a Pocket touching the flat top, fillet/chamfer of the pocket → broken.
- [#28992](https://github.com/FreeCAD/FreeCAD/issues/28992) OPEN: tube with a bevelled end, a fillet of the bevelled edge breaks the model; above 12 mm a failure.
- [#29234](https://github.com/FreeCAD/FreeCAD/issues/29234) OPEN: angled shaft; of 4 edges 2 get filleted (one of them wrongly), 2 → failure.
- [#24135](https://github.com/FreeCAD/FreeCAD/issues/24135) DUPL.: a fillet of an inclined corner fails as soon as it reaches another line.
- [#27839](https://github.com/FreeCAD/FreeCAD/issues/27839) OPEN: "hourglass" profile (outer angles < 180°) → sometimes non-manifold edges, the slicer complains.
- [#9871](https://github.com/FreeCAD/FreeCAD/issues/9871) FIXED: fillet 1 mm → failure, ≥ 5 works; the mirrored body — up to ~1.3.
- [#16644](https://github.com/FreeCAD/FreeCAD/issues/16644) FIXED, crash: the base circle reduced to R1.5 → the fillet becomes impossible → a crash instead of an error.
- [#17596](https://github.com/FreeCAD/FreeCAD/issues/17596) REJECTED: fillet 2 on the outer edge of a tube → "not possible"; the cause is an older fillet attached to the cylinder's seam (naming).
- [#32486](https://github.com/FreeCAD/FreeCAD/issues/32486) OPEN, OCC: a fillet of helical edges works for one, not for the other.

## Chamfer

- [#20750](https://github.com/FreeCAD/FreeCAD/issues/20750) OPEN: two adjacent Pads, chamfer 3 on the inner top edge → failure; only 3 + 2.9 on the short segment works.
- [#22921](https://github.com/FreeCAD/FreeCAD/issues/22921) / [#33092](https://github.com/FreeCAD/FreeCAD/issues/33092) / [#28828](https://github.com/FreeCAD/FreeCAD/issues/28828): "Use All Edges" with chamfer 5 (more than some faces allow) → crash; 1 mm works; on the bottom face with all edges → failure, manually selected ones work.
- [#30230](https://github.com/FreeCAD/FreeCAD/issues/30230) OPEN: Pad, fillets on the vertical corners, chamfer on the top edge → the top layer lifts and the body is hollow.
- [#26039](https://github.com/FreeCAD/FreeCAD/issues/26039) OPEN: chamfer of a slot next to a wall — one side is wrong, both at once → failure; the model is symmetric.
- [#26832](https://github.com/FreeCAD/FreeCAD/issues/26832) FIXED, crash: a 1 mm chamfer that goes below the edge of an inner shelf → crash.
- [#23818](https://github.com/FreeCAD/FreeCAD/issues/23818) OPEN, crash: chamfer of an edge connected to a fillet (the body is already invalid).
- [#17604](https://github.com/FreeCAD/FreeCAD/issues/17604) OPEN, OCC: a 2 mm wall along a B-spline; chamfer 0.1…1 on the inner top edge → failure, on the outer one it works.
- [#18556](https://github.com/FreeCAD/FreeCAD/issues/18556): a chamfer larger than the cylinder's radius → invalid body (see Fillet).
- [#28774](https://github.com/FreeCAD/FreeCAD/issues/28774): Two Distances / Distance+Angle on a mirrored Pad (see Pattern).
- [#29001](https://github.com/FreeCAD/FreeCAD/issues/29001) FIXED: two chamfers in one feature from 1.0 fail in 1.1 with the same edge indices.
- [#10523](https://github.com/FreeCAD/FreeCAD/issues/10523) REJECTED: 1 of 4, then 4 of 8 chamfers are not built.
- [#15102](https://github.com/FreeCAD/FreeCAD/issues/15102) FIXED, crash: chamfer size 3.9 → crash.
- [#14622](https://github.com/FreeCAD/FreeCAD/issues/14622) / [#18290](https://github.com/FreeCAD/FreeCAD/issues/18290) / [#11836](https://github.com/FreeCAD/FreeCAD/issues/11836): chamfers in three identical pockets or on a particular face → crash.
- [#17829](https://github.com/FreeCAD/FreeCAD/issues/17829) OPEN: internal thread from a helix, bevelled end via a Groove; mirrored at the other end → broken face.
- [#25206](https://github.com/FreeCAD/FreeCAD/issues/25206) OPEN: chamfer of the inner edges fails both when an edge and when a face is selected; the fillet by face works, but only up to 1.4 mm.

## Pad / Pocket with a taper (Taper)

- [#22391](https://github.com/FreeCAD/FreeCAD/issues/22391) / [#9708](https://github.com/FreeCAD/FreeCAD/issues/9708) / [#33427](https://github.com/FreeCAD/FreeCAD/issues/33427) FIXED in OCCT 8.0.2 / 8.1: a circle of Ø2 or smaller (exactly R1), Pad or Pocket with a negative taper → crash in the contour offset.
- [#8160](https://github.com/FreeCAD/FreeCAD/issues/8160) FIXED in OCCT 8.1: taper ~5° on a circle; size 2.01 works, 1.99 → a bogus "face disappears" error; the same for Pocket. Today the shape is wrong, although valid.
- [#28709](https://github.com/FreeCAD/FreeCAD/issues/28709) FIXED (regression): two nested rectangles (a box without a bottom) → the two contours taper in opposite directions.
- [#26772](https://github.com/FreeCAD/FreeCAD/issues/26772) FIXED (regression): the taper of the holes in a plate is reversed between 0.21 and 1.0 — old files change their shape.
- [#31625](https://github.com/FreeCAD/FreeCAD/issues/31625) FIXED: 4 pins (circles) in one sketch with a taper; one tapers the other way — its centre was taken from a projection of the angle (the contour's direction).
- [#32262](https://github.com/FreeCAD/FreeCAD/issues/32262) FIXED: Pocket from a circle on the bottom face, taper 20°; with "Two sided" the taper turns inward — it depends on whether the direction is +axis or −axis.
- [#27252](https://github.com/FreeCAD/FreeCAD/issues/27252): Polar Pattern of a Pad with a negative taper (see Pattern).
- [#21665](https://github.com/FreeCAD/FreeCAD/issues/21665): fillet of a Pad with a taper of −2.8° / −3° (see Fillet).
- [#27146](https://github.com/FreeCAD/FreeCAD/issues/27146), [#32020](https://github.com/FreeCAD/FreeCAD/issues/32020), [#33710](https://github.com/FreeCAD/FreeCAD/issues/33710), [#23765](https://github.com/FreeCAD/FreeCAD/issues/23765): Thickness after a tapered Pad (see Thickness).
- [#8176](https://github.com/FreeCAD/FreeCAD/issues/8176) OPEN, OCC: Pocket from two rectangles with a taper (a trapezoidal slot); the second such Pocket breaks the whole geometry.
- [#19785](https://github.com/FreeCAD/FreeCAD/issues/19785) / [#24171](https://github.com/FreeCAD/FreeCAD/issues/24171): the taper is missing for Through All, To Face and To Shape.
- [#25164](https://github.com/FreeCAD/FreeCAD/issues/25164) REJECTED: Pocket "up to face" with a limit coinciding with the edge of the Pad → a cut of zero height in the wall, an open model; the Pad was supposed to stop 10 µm earlier.
- [#23858](https://github.com/FreeCAD/FreeCAD/issues/23858): a hole with a 50° taper that closes before the depth (see Hole).

## Boolean operations (OCCT)

- [#17689](https://github.com/FreeCAD/FreeCAD/issues/17689) FIXED (regression): Common of a box, a cylinder and a sphere depends on the order; for n bodies (S1∧S2)∨(S1∧S3)… is computed instead of S1∧S2∧…∧Sn.
- [#23484](https://github.com/FreeCAD/FreeCAD/issues/23484) FIXED with OCCT 8.0: a sphere cut from a block with a hole does not cut; +0.1 mm on the radius or rotating the sphere helps — its seam lies in the plane of the face.
- [#5782](https://github.com/FreeCAD/FreeCAD/issues/5782) FIXED: cylinder ∪ sphere — in some positions the sphere disappears, although it intersects the cylinder.
- [#7338](https://github.com/FreeCAD/FreeCAD/issues/7338) FIXED: torus R10/r5 rotated −90°, + sphere r5 + box + cylinders; cutting the sphere after a union gives errors.
- [#17286](https://github.com/FreeCAD/FreeCAD/issues/17286) FIXED (regression): subtracting a truncated cone from a body → extra geometry; with a sphere or a cylinder (equal radii) there is none.
- [#32645](https://github.com/FreeCAD/FreeCAD/issues/32645) OPEN: tube from two circles; a rectangle tangent to the inner circle exactly at the seam, Pad (Join) → an invalid body without an error, every following feature fails. The Boolean is fine, the clean-up after it breaks it.
- [#29373](https://github.com/FreeCAD/FreeCAD/issues/29373) OPEN: two triangular prisms with a common face + a box coinciding with them in one plane but shorter; after the union the body is valid, after Refine there are self-intersections: 2 prisms → 2 errors, 3 → 4, 22 → 42.
- [#26119](https://github.com/FreeCAD/FreeCAD/issues/26119) OPEN: pattern with a spacing = the size (coincident faces); the rotations introduce an error of ~1e-14 and the Booleans fail without a tolerance (fuzzy).
- [#26327](https://github.com/FreeCAD/FreeCAD/issues/26327) OPEN: extruding a triangulated surface gives one body per triangle with coincident inner faces → the Booleans fail or are slow.
- [#17497](https://github.com/FreeCAD/FreeCAD/issues/17497) FIXED: cutting with a group of intersecting bodies → self-intersection only in the intersection zones.
- [#17705](https://github.com/FreeCAD/FreeCAD/issues/17705) OPEN, OCC, silent: Loft (B-spline sections) ∪ Revolution of a B-spline → half is missing (it cuts instead of uniting), the body passes the checks; the difference in the other direction works.
- [#25169](https://github.com/FreeCAD/FreeCAD/issues/25169) OPEN: two partly overlapping extrusions of splines → Common is empty; Fuse and Cut are wrong; if one fully covers the other, it works.
- [#25193](https://github.com/FreeCAD/FreeCAD/issues/25193) OPEN, OCC, silent: Common of two simple bodies → a "valid" body with volume 0 and an infinite bounding box.
- [#11620](https://github.com/FreeCAD/FreeCAD/issues/11620) OPEN: MultiFuse of many cylinders and spheres (a molecular model) often loses faces without an error; small random rotations and a tree-shaped union help.
- [#9751](https://github.com/FreeCAD/FreeCAD/issues/9751) / [#21832](https://github.com/FreeCAD/FreeCAD/issues/21832) OPEN: bodies along helices with a round section do not unite; pattern + Sweep → "Null shape".
- [#17338](https://github.com/FreeCAD/FreeCAD/issues/17338) / [#9666](https://github.com/FreeCAD/FreeCAD/issues/9666): the union of a thread with a cylinder fills the gaps; a difference with a tap/die loses the vertical faces.
- [#22721](https://github.com/FreeCAD/FreeCAD/issues/22721) OPEN: cutting an external thread in PartDesign → "multiple solids"; in Part it works.
- [#6095](https://github.com/FreeCAD/FreeCAD/issues/6095) / [#5949](https://github.com/FreeCAD/FreeCAD/issues/5949) REJECTED: vertical round pockets touching a horizontal one; after Refine "invalid curve on surface", missing faces.
- [#25899](https://github.com/FreeCAD/FreeCAD/issues/25899) OPEN, OCC: torus (70/30) with 5 bosses and a hexagonal socket 13; a Pocket through the curved face fails only at one particular exact alignment.
- [#18874](https://github.com/FreeCAD/FreeCAD/issues/18874) / [#16690](https://github.com/FreeCAD/FreeCAD/issues/16690) OPEN: an inclined Pocket "up to the first face" in a cylinder does not stop correctly, sometimes a reversed Boolean.
- [#20772](https://github.com/FreeCAD/FreeCAD/issues/20772) / [#26150](https://github.com/FreeCAD/FreeCAD/issues/26150) OPEN, silent: a simple rectangular Pocket in a complex body → an incomprehensible shape.
- [#25283](https://github.com/FreeCAD/FreeCAD/issues/25283) / [#12245](https://github.com/FreeCAD/FreeCAD/issues/12245) OPEN, silent: after deleting an earlier feature the cut is wrong, without an error; order-dependent damage.
- [#18047](https://github.com/FreeCAD/FreeCAD/issues/18047) OPEN: extrusion of a filled 2D offset (−2 mm into a hole) → every Boolean with a box fails.
- [#30143](https://github.com/FreeCAD/FreeCAD/issues/30143): Slice Apart loses one half, works only without Refine.

## Combinations of features

- **Fillet → Thickness, r = t**: #5634, #21472 (failure or invalid); **Thickness → Fillet**: #5604.
- **Taper → Thickness**: #27146, #32020 (only when narrowing), #33710 (cone), #23765 (+ a Pocket afterwards), #21614.
- **Draft → Thickness**: #19184; **Draft → Fillet / Refine**: #13217.
- **Fillet → Mirror**: #26815 (depends on how the plane is given); **Mirror → Chamfer**: #28774; **Mirror → Fillet** on a Pipe: #21003.
- **Pattern → Chamfer / Hole + Chamfer**: #28399 (copies without the chamfer), #19337 (overlapping chamfers → crash), #30858 (hole through a chamfer).
- **Thickness → Pocket / Polar Pattern**: #26587, #31936, #30866, #15323.
- **Shell/Thickness → Chamfer** (naming): #27266.
- **Threaded Hole → Pad / Pipe**: #18105, #21325; **Hole (conical point) → Thickness**: #11527.
- **Taper → Polar Pattern**: #27252; **Taper → Fillet**: #21665.
- **Pattern across a seam**: #5917 (Groove on a revolved body), #32645 (Pad tangent at the seam of a tube), #23484 (the sphere's seam in the plane of a face).
- **Refine after a Boolean** is a separate source of errors: #15353, #17846, #29373, #16654, #6095, #18720.

## Conclusions for us

1. **r = t and 10 + 10 = 20**: an exact equality between a radius, a thickness and a size is the most common cause. A clear decision is needed: either a valid body without zero-thickness walls, or a failure with a message (#5634, #21472, #33618, #27918, #25050).
2. **Silent wrong results** are more dangerous than failures: volume, number of copies and validity must be checked after every feature (#24994, #29375, #30866, #28399, #17705, #25193).
3. **Seams of revolved bodies, spheres and cylinders** in the plane of another face or at a point of tangency break the Booleans (#5917, #23484, #32645, #19184).
4. **The taper changes its direction** depending on the direction of the contour or of the axis (#28709, #31625, #32262, #26772): there must be one rule, tested for all cases.
5. **Merging faces (Refine) after a Boolean** must not be able to turn a valid body into an invalid one; if it does — a failure, and we return the body without merging.
6. **Crashes** come from too large chamfers, "Use All Edges" and deleting edges; for us: a guard and a message, without changing the state.

## Tests we should add

Volumes are in mm³; "valid" = the `BRep` passes the check (closed, oriented, Euler), with no faces of
zero area and no edges of zero length. "Failure" = an error `<Command>: <reason>`, the body and the
history are unchanged (rule 5).

### Thickness / Shell
1. Cylinder R20 × 10, Shell inward, top face open, t = 5 → V = 2875π ≈ 9032.08; t = 10 and t = 12 → failure "the thickness is ≥ the height" (#25050).
2. Cube 10³, all edges r1, Shell t = 1, top face open → failure "zero-thickness wall" (the opening's edge shrinks to a point); t = 0.99 → valid; t = 0.5 → valid, V = 975.587 − V(cavity), the cavity is the 0.5 inward offset (edges r0.5); the outer volume of the rounded cube is 8³ + 2·3·8² + 3π·8 + 4π/3 ≈ 975.587 (#5634, #21472).
3. Box 20×20×10, top edges r1, Shell t = 1 on the bottom face: the orders Fillet → Shell and Shell → Fillet give valid bodies; the volumes differ by the expected amount (inner edges r0 vs r1) (#21472).
4. Cube 100, Shell outward t = 10, top face open (outer 120×120×110, cavity = the cube) → V = 584 000; then a fillet r5 on one inner vertical corner → valid, V = 584 000 + (1 − π/4)·25·100 ≈ 584 536.5 (#5604).
5. Rectangle 20×30, Pad 10 with a taper of 4° (outward) and −4° (inward), Shell t = 1 with the top face open → valid; the volume by the prismatoid formula V = h/6·(A1 + 4Am + A2) for the outer body and the cavity (faces offset by t along the normal) (#27146, #32020).
6. Circle R20, Pad 20 with a −30° taper (truncated cone R20 → 8.453), Shell t = 1 with the top base open, then only the bottom one, then both → all three valid; with both — a closed tube, and the STL contains the inner wall (#33710).
7. Hemisphere R10 from a Revolution about Z, Shell t = 1 on the flat face → V = ⅔π(1000 − 729) ≈ 567.58; the same about X and Y gives the same volume (#19184).
8. Box 40×40×10 + hemisphere R15 on top in the centre (Join), Shell t = 2 with the bottom face open → valid, the volume = outer − cavity (the cavity is the offset body) (#33012).
9. Three Pads stacked (20×20×5, 10×10×5, 6×6×5), Shell t = 1 with the bottom face open → valid; adding a fourth Pad on the other side does not break the Shell (#32018).
10. Loft square 40 → circle Ø30 at height 40, Shell t = 2 with both bases open → valid, all faces of the Loft remain (#6196).
11. Tube R10/R20 × 10: fillets on the top inner and outer edges r6 + r6 (sum 12 > the wall 10) → failure; r5 + r5 = 10 → valid without a zero-thickness wall, or a failure — never a body with a cavity (#27918).
12. Plate 30×30×10 with a blind hole Ø6 × 5 with a 118° conical point, Shell t = 1 with the top face open → valid or a clear failure "the hole's point is thinner than the wall" (#11527).
13. Shell of a box 40×30×20, t = 2, then a Pocket Through All through the side face (a 10×5 window) → valid, the face count grows by 4; the body stays hollow (#26587, #15323, #23765).
14. Two bodies touching only along an edge (a Pad upward and a Pad downward from sketches touching at (0,0)), Shell → a clear failure "the body is not manifold", no crash (#21937).

### Draft
15. Cube 20³, Draft 5° on one side face, neutral plane = the bottom → V = 8000 − 0.5·20·20·tan5°·20 ≈ 7650.05; 7 faces → 6 faces (taper) (#9762).
16. Draft with a pull direction and angle 0 → the body is unchanged (same volume, same names), not empty; 0.01° → the volume decreases by ½·h²·tan(0.01°)·the width (#32777).
17. Draft −5° = Draft 5° with the direction reversed: equal volumes and identical faces (#19532).
18. Truncated pyramid: square 40, Pad 10 with a −45° taper (top 20×20); Draft +5° on one side face → valid, the volume by the prismatoid formula (#28887).
19. Cylinder R10 × 20, Draft 5° on the cylindrical face, neutral = the bottom → truncated cone R10 → 8.2502, V = π·20/3·(100 + 82.502 + 68.066) ≈ 5247.9 (#30610, SolidCraft #547).
20. Draft on a face tangent to a fillet r2 → a clear failure "the face is tangent to a neighbouring face" (until it is supported), no invalid body (#9762).
21. Box 40×20×20, Draft 3° on the four side faces, then a fillet r2 on the vertical edges → valid, the 4 fillet faces are cones (#13217).
22. The same, then Shell t = 1 with the top face open → valid (#19184).
23. Draft on a face of a Loft (square → smaller square) → valid or a clear failure; the direction of the draft must be the same as on a cube (#5749).
24. Speed: rectangle 40×20 with rounded corners R5, Pad 20, Draft 3° on all side faces → under the budget in `tests/perf.rs` (#27250).

### Pattern / Mirror
25. LinearPattern with count 1 → the body is unchanged, no error; count 0 → a failure with a message (#7791, #18473).
26. Plate 120×20×5, hole Ø5, LinearPattern 10 occurrences at 10 → V = 12 000 − 10·π·2.5²·5 ≈ 11 018.25; 50 occurrences (a long plate) under the budget (#23975).
27. Disc R30 × 5 + a tooth (Pad 4×4×5 at the edge), Polar Pattern N = 2…40: V = V_disc + N·V_tooth (as long as the teeth do not overlap), the face count grows linearly; no "magic" counts (#18720).
28. Polar Pattern of a Pad from a circle R3 with a −10° taper (a truncated cone) on a disc, N = 2…12 → all copies present, V = V_disc + N·V_cone (#27252).
29. Cylinder R30 × 10, pocket — a radial slot 4×30 from the centre outward, Polar Pattern 6 → the copies overlap at the centre; V = V_cyl − area(union)·10; check: equal to the volume from one Boolean with the united tool (#12099).
30. Polar Pattern of 5 cuts → exactly 5 holes: V = V_body − 5·V_cut, the face count matches the preview (#30866, #29375).
31. Revolved disc, a Groove with an arc whose centre is exactly on the disc's edge (coinciding with the seam), Polar Pattern N = 2…12 → all copies really cut (#5917).
32. Pad 10×10×10 touching YZ, Mirror about YZ → one body 20×10×10, V = 2000, 6 faces after merging (#5704).
33. Quarter elbow: circle r5 along an arc R30 / 90° (V = π·25·15π ≈ 3701.1), Mirror twice → a closed torus, V = 2π²·30·25 ≈ 14 804.4, no inner faces (#24994).
34. Pin Ø4 × 10 with a chamfer 1 on top, MultiTransform (LinearPattern 3 × LinearPattern 3) → 9 pins, each with a chamfer: V = 9·V_pin (#28399).
35. MultiTransform with two consecutive Mirrors (X and Y) → 4 copies; the original loses nothing, V = 4·V_feature (#28396).
36. Plate with a chamfer 2 along the edge, hole Ø4 crossing the chamfer, LinearPattern 4 → 4 holes, all crossing the chamfer, no face disappears (#30858).
37. Two rings of chamfered holes (Polar Pattern 8 of a hole + chamfer), the chamfers of neighbouring rings overlap → valid or a failure, never a crash (#19337).
38. Mirror after a fillet: the plane "origin XZ" and "a work plane on XZ" give identical bodies (#26815).
39. Speed: a torus with a cutout, Polar Pattern 12 and Mirror twice → under the budget; editing the count 8 → 9 does not freeze the UI (`preview_async`) (#18205, #29536).

### Hole
40. Plate 20×20×10, hole Ø6.6 with a counterbore Ø11 × 5.5, Through All → V = 4000 − π(3.3²·4.5 + 5.5²·5.5) ≈ 3323.37 (#5894).
41. Countersink Ø10 / 90° on a hole Ø5.5 → cone depth (D − d)/2 / tan(θ/2) = 2.25; at 60° → 3.897; the value in the dialog = the measured distance to the shoulder (#8777, #31527).
42. Blind hole Ø6 × 10 with a 118° conical point: with "the point is not included in the depth" the cylindrical part is 10; with "included" — 10 − 3/tan59° ≈ 8.197 (#5879).
43. Hole Ø10 with a 50° taper in a plate of 5: depth 5 → valid; depth 7 (the cone closes) → a clear failure already in the preview, no silent stop (#23858).
44. 4 threaded M3 holes at (±10, ±10) in one sketch → a valid body, 4 holes; a following Pad on the plate works (#15353, #18105).
45. M10 blind 15 in a body of 25 with a modeled thread → the thread is built for a blind hole too; the body is valid, a following Boolean works (#19254, #21325).
46. Hole Ø5 on a cylindrical face R20 (axis ⟂ to the cylinder's axis), Through All → exactly 2 holes, V = V_cyl − V(cylinder Ø5 ∩ cylinder R20); no "half" hole at all (#17825, #23703).
47. Hole through two differently inclined faces with a depth 2× larger than needed → cuts both (#30190).
48. Hole with a countersink as the last feature, export to STL → the STL volume = the body volume (±1 %) (#17335).

### Fillet
49. Box 20×40×20, fillets r10 on the two top long edges of the 20 face → a full round, V = 40·(400 − 2·(1 − π/4)·100) ≈ 14 283.2; r10 + r10.01 → failure (#33618).
50. Cylinder R5 × 20, fillet r5 on the top edge → a hemisphere, V = π·25·15 + ⅔π·125 ≈ 1439.9; r5.01 → failure, not an invalid body (#18556).
51. Cylinder R10 × 20 from a Revolution and the same from a Pad, fillet r2 on the top edge → the same V ≈ 6231.7 (Pappus: 2π·9.5533·0.8584) (#5932).
52. Pad with a taper from −2° to −4° in steps of 0.1°, fillet r2 on one edge → all succeed, the volume is monotonic (#21665).
53. Two faces at an angle of 170°…179.9° in steps of 0.5°, fillet r1 → succeeds, or a clear failure "the faces are nearly tangent" for all above the threshold, no jump at 174.28° (#32309, #31868).
54. Cube 20 ∪ sphere R8 centred at a vertex, fillet r1 on an edge next to the sphere → valid, the edge after regeneration is the same one (by name) (#5665).
55. Cylinder R20 × 20 with a spherical dent R6 on its side (Revolve cut), fillet r1 on the top edge → valid (#18383).
56. Box + cylinder, an edge whose fillet r3 ends exactly at the apex of the cylinder's generatrix → valid (#27456).
57. Fillet of 4 distant faces at once = fillets one by one: identical volumes and face counts (#33432).
58. Pad 20×20×10, fillet r2 on edge A, then r2 on the neighbouring edge B in a new feature → only A and B are filleted, the common vertex is valid (#17452).
59. Cylinder R25 × 10, fillet r1: the deviation of the triangulation from the torus ≤ the display tolerance along the whole length, the STL is closed (#8549).
60. "Hourglass" profile (outer angles < 180°), fillets on all vertical edges → STL without non-manifold edges (#27839).
61. The base circle is reduced until the fillet becomes impossible → feature health "Fillet: the radius is too large", no crash, the body before the fillet remains (#16644).

### Chamfer
62. L-body from two adjacent Pads (10×10×10 + 10×10×10), chamfer 3 on the top edge where they touch (with the short segment) → valid, the volume changes by ½·3²·L (the sign by the edge's convexity) (#20750).
63. Box 40×20×10, vertical edges r5, chamfer 1 along the top contour → V = 7785.4 − (0.5·80 + 2π·(5 − 1/3)·0.5) ≈ 7730.7, the body is not hollow (#30230).
64. "All edges" with chamfer 5 on a body with 4 mm walls → failure, no crash; chamfer 1 → valid (#22921, #33092).
65. A chamfer that would go below an inner edge (shelf 1 mm, chamfer 1.5) → failure or valid, no crash (#26832).
66. A 2 mm wall along a B-spline, chamfer 0.5 on the inner and on the outer top edge → both valid (#17604).
67. Mirrored Pad, chamfer Two Distances (1, 2) and Distance+Angle (1, 30°) on the edge between the original and the copy → the distances are kept on the correct sides (#28774).

### Taper
68. Circle Ø2 (R1), Pad 5 and Pocket 5 with a taper of −5° and −10° → valid, V = πh/3·(R² + Rr + r²) (≈ 9.839 and ≈ 5.929 for R1); a taper down to the apex (r = 0) → failure (#22391, #9708, #33427).
69. Circle R2.01 and R1.99, Pocket with a 5° taper → both valid, the volumes are continuous in R (#8160).
70. Frame 40×40 with a 30×30 opening (a box without a bottom), Pad 10 with a 5° taper → both contours follow one rule (the outer grows, the opening shrinks), V by the prismatoid formula; the same with Two sided (#28709).
71. 4 circles in one sketch (one with its centre from a projection, the others free, different traversal direction), Pad with a taper → 4 identical cones (#31625).
72. Pocket from a circle on the bottom and on the top face (direction +Z and −Z), taper 20°, One sided and Two sided → a positive angle always widens; the volumes match by symmetry (#32262).
73. Pocket "to face" whose limit coincides with the edge of the Pad → no zero-height face, the face count is as with a limit 10 µm off (#25164, SolidCraft #2963).

### Booleans
74. Common of a box 20³, a cylinder R8 × 30 and a sphere R12 in all 6 orders → the same volume; V(A∩B∩C) ≤ min(V) (#17689).
75. Block with a hole Ø16, a sphere R8.5 cut from the edge of the hole: the sphere's seam in the plane of the face and rotated by 37° → the same volume (#23484).
76. Cylinder ∪ sphere at 20 positions of the sphere along the axis → V(A∪B) = V(A) + V(B) − V(A∩B) for all (#5782, `tests/boolean_identity.rs`).
77. Tube R10/R15, a rectangle tangent to the inner circle exactly at the seam, Pad (Join) → valid, a following fillet works (#32645).
78. 2, 3 and 22 triangular prisms with common faces + a box coinciding with them in one plane and shorter → valid also after merging the faces (#29373).
79. 10 boxes 10³ at a spacing of 10 (coincident faces), Join → one box 100×10×10, V = 10 000, 6 faces after merging; the same after rotating the whole pattern by 33° (#26119).
80. Loft of B-spline sections ∪ Revolution of a B-spline, partly overlapping → V(A∪B) + V(A∩B) = V(A) + V(B) (#17705, #25169).

(80 recipes: the first 55 have priority; the rest — when the corresponding capability becomes ✅.)
