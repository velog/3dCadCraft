//! Blends of edges that run tangentially into the rim of an earlier round (the owner's
//! part `ChanferFillet`, 2026-11-10): a ball R30 about (0, 30, 0) with a channel cut
//! through it along Y (x −3.5..5, z 15..35), the wall x = −3.5 rounded R4 where it meets
//! the ball. The round ends on the channel's floor z = 15 in an arc that runs tangent
//! into the floor's edge along the wall (both concave) at one end, and leaves the
//! floor's edge along the ball (convex) in the same direction at the other, so that the
//! floor closes to a cusp there.
//!
//! Expected values are Inventor 2027's on the same part (R4, then r1): volume
//! 108122.147 after the R4 round; a fillet r1 of the floor arc alone rolls on along the
//! floor's whole concave chain (arc, wall edge, the other arc): 108131.984, the same as
//! picking all three.

mod common;
use common::*;

/// The ball with its channel, the wall's top edge rounded R4.
fn rounded_channel() -> PartDocument {
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut p.sketches[i].sketch;
    let (a, c, b) = (
        sk.add_point(Point2::new(0.0, 0.0)),
        sk.add_point(Point2::new(0.0, 30.0)),
        sk.add_point(Point2::new(0.0, 60.0)),
    );
    sk.add_arc(c, a, b).unwrap();
    sk.add_line_between(b, a).unwrap();
    let r = sk.regions();
    p.add_revolve(
        i,
        RevolveFeature::new(region_profiles(&r, |_| true), AxisRef::Origin(1)),
    )
    .unwrap();
    // XZ: sketch x = X, sketch y = Z.
    let j = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XZ), &opts())
        .unwrap();
    p.sketches[j]
        .sketch
        .add_rectangle_two_point(Point2::new(-3.5, 15.0), Point2::new(5.0, 35.0))
        .unwrap();
    cut(&mut p, j, None, |_| true);
    let wall_top = edges_where(&p, |h| (h[0] + 3.5).abs() < 1e-6 && h[2] > 20.0);
    assert_eq!(wall_top.len(), 1, "the wall's top edge: {wall_top:?}");
    p.add_blend(BlendFeature::fillet(wall_top, 4.0)).unwrap();
    p
}

/// The floor's edges at z = 15 whose hint `keep` takes.
fn floor_edges(p: &PartDocument, keep: impl Fn([Real; 3]) -> bool) -> Vec<EdgeRef> {
    edges_where(p, |h| (h[2] - 15.0).abs() < 1e-6 && keep(h))
}

/// The arc where the R4 round ends on the floor, at the low-y end of the channel.
fn floor_arc(p: &PartDocument) -> Vec<EdgeRef> {
    let arc = floor_edges(p, |h| h[0] < -3.6 && h[1] < 30.0);
    assert_eq!(arc.len(), 1, "the floor arc: {arc:?}");
    arc
}

#[test]
fn the_round_matches_inventor() {
    let p = rounded_channel();
    check(&p, 108_122.147, 2e-5);
}

/// The floor arc alone: SMLib cannot end a blend where its edge runs on tangentially
/// (into the wall's floor edge), so the blend follows the tangent chain, as Inventor's
/// does (picked alone it rounds the arc, the wall edge and the other arc).
#[test]
fn a_fillet_of_the_rounds_floor_arc_rolls_on_along_its_tangent_chain() {
    let mut p = rounded_channel();
    let before = volume(&p);
    let arc = floor_arc(&p);
    p.add_blend(BlendFeature::fillet(arc, 1.0)).unwrap();
    let added = volume(&p) - before;
    // Inventor: 108131.984 − 108122.147 = 9.837 (a concave round adds material).
    check(&p, 108_131.984, 2e-5);
    assert!((added - 9.837).abs() < 0.05, "added {added}");
    // The same as picking the whole chain.
    let mut q = rounded_channel();
    let chain = floor_edges(&q, |h| h[0] < -3.4 && h[1] > 0.0);
    assert_eq!(chain.len(), 3, "arc, wall edge, arc: {chain:?}");
    q.add_blend(BlendFeature::fillet(chain, 1.0)).unwrap();
    assert!(
        (volume(&q) - volume(&p)).abs() < 1e-3,
        "{} vs {}",
        volume(&q),
        volume(&p)
    );
}

/// Not built yet (solidcore): blends that reach the floor's cusp, where the convex
/// edge along the ball and the concave arc of the round leave one vertex along one
/// tangent. Inventor 2027's values on this part, r1 after the R4: fillet of the ball's
/// floor edge alone 108126.238 (chamfer 108115.152), with the arc 108131.162, the
/// floor's whole outline (both ball edges, both arcs, the wall edge) 108130.383; a
/// chamfer of the concave chain 108147.657. Each of them is refused with a reason and
/// leaves the part as it was.
#[test]
#[ignore = "solidcore: no corner where a convex and a concave blend leave a vertex along one tangent over a curved seam (#2256), nor a single blend ending at such a cusp"]
fn blends_reaching_the_floors_cusp_match_inventor() {
    let base = rounded_channel();
    let ball_edge = floor_edges(&base, |h| h[0].abs() < 4.9 && h[1] < 6.0);
    assert_eq!(
        ball_edge.len(),
        1,
        "the floor's edge along the ball: {ball_edge:?}"
    );
    let arc = floor_arc(&base);
    let cases: [(&str, Vec<EdgeRef>, bool, Real); 4] = [
        ("ball edge fillet", ball_edge.clone(), true, 108_126.238),
        ("ball edge chamfer", ball_edge.clone(), false, 108_115.152),
        (
            "ball edge + arc fillet",
            [ball_edge, arc.clone()].concat(),
            true,
            108_131.162,
        ),
        ("arc chamfer", arc, false, 108_147.657),
    ];
    for (what, edges, round, want) in cases {
        let mut p = base.clone();
        let def = if round {
            BlendFeature::fillet(edges, 1.0)
        } else {
            BlendFeature::chamfer(edges, 1.0)
        };
        p.add_blend(def).unwrap_or_else(|e| panic!("{what}: {e}"));
        check(&p, want, 2e-5);
    }
}
