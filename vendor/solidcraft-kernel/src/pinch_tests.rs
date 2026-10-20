//! Outlines that touch themselves: a circle tangent to a side, a corner on an edge, corners
//! meeting. Joins over such a contact fuse into one solid, cuts that leave pieces meeting only
//! along a line give separate shells, and near-tangent numbers from the sketch solver (a
//! circle 1e-13 mm past the side) behave like exact ones.

use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};

use crate::*;

fn prism(outer: Loop2, z0: f64, z1: f64) -> Body {
    extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], z0, z1).unwrap().pop().unwrap()
}

fn poly(pts: &[[f64; 2]]) -> Loop2 {
    Loop2::polygon(&pts.iter().map(|p| Vec2::new(p[0], p[1])).collect::<Vec<_>>())
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop2 {
    poly(&[[x0, y0], [x1, y0], [x1, y1], [x0, y1]])
}

fn disc(cx: f64, cy: f64, r: f64) -> Loop2 {
    Loop2::circle(Vec2::new(cx, cy), r)
}

fn assert_solid(r: &Body, volume: f64, rel: f64, shells: usize, what: &str) {
    let v = r.tessellate(1e-4).unwrap().measure().volume;
    assert!((v - volume).abs() <= volume * rel, "{what}: volume {v} vs {volume}");
    assert_eq!(r.shell_count(), shells, "{what}: shells");
    assert!(r.validity().is_empty(), "{what}: {:?}", r.validity());
}

/// The reported case: a disc on a 31 × 29 block, tangent to its back side and overhanging the
/// other three, joined exactly, whether the tangency is exact or the solver's (1.3e-13 past).
/// The solver's numbers used to fail every route: a split line traced on the disc's face ended
/// one chord's sag short of the rim and was dropped, so the contact faces never matched.
#[test]
fn disc_tangent_to_a_side_and_overhanging_joins() {
    let block = prism(rect(0., 0., 31., 29.), 0., 10.);
    for (cy, r) in [(4.0, 25.0), (4.392830461839724, 24.607169538160402), (4.0, 25.0 + 1e-9), (4.0, 25.0 - 1e-9)] {
        let top = prism(disc(18.5, cy, r), 10., 20.);
        let want = 8990.0 + std::f64::consts::PI * r * r * 10.0;
        for (a, b) in [(&block, &top), (&top, &block)] {
            let (res, d) = capture_boolean_diagnostics(|| boolean(a, b, BoolOp::Union));
            let res = res.unwrap().unwrap();
            assert_solid(&res, want, 1e-5, 1, &format!("disc at {cy}, r {r}"));
            assert!(!d.events.iter().any(|e| e.strategy == BooleanStrategy::ToolNudge), "{d:?}");
        }
    }
}

/// A wall splitting a disc's face where the rim crosses it between two chords of the trace
/// mesh: the split happens however the mesh falls.
#[test]
fn split_line_ending_past_the_last_chord_still_splits() {
    let top = prism(disc(18.5, 4.392830461839724, 24.607169538160402), 10., 20.);
    let bottom =
        (0..top.face_count()).find(|i| matches!(face_surface(&top, *i), Some(FaceSurface::Plane { normal, .. }) if normal.z < -0.9)).unwrap();
    let wall = SplitTool::Plane(Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z });
    let (split, _) = split_faces_with_map(&top, &[bottom], &wall).unwrap();
    assert_eq!(split.face_count(), top.face_count() + 1);
}

/// Lines only: a triangle with its apex on the square's back edge, its base overhanging, and a
/// diamond with its corners on the square's edge midpoints, each standing on the square.
#[test]
fn corner_on_an_edge_joins() {
    let square = prism(rect(0., 0., 20., 20.), 0., 10.);
    let triangle = prism(poly(&[[-5., -5.], [25., -5.], [10., 20.]]), 10., 20.);
    let diamond = prism(poly(&[[10., 0.], [20., 10.], [10., 20.], [0., 10.]]), 10., 20.);
    for (tool, want) in [(&triangle, 4000.0 + 3750.0), (&diamond, 4000.0 + 2000.0)] {
        let res = boolean(&square, tool, BoolOp::Union).unwrap().unwrap();
        assert_solid(&res, want, 1e-9, 1, "corner on an edge");
    }
}

/// A cut leaving pieces that meet only along a line gives one shell per piece, not one shell
/// pinched along that line: a diamond cut through a square leaves four corner posts, a disc
/// tangent to the block's back side two.
#[test]
fn cut_leaving_pieces_meeting_along_a_line_gives_separate_shells() {
    let square = prism(rect(0., 0., 20., 20.), 0., 10.);
    let diamond = prism(poly(&[[10., 0.], [20., 10.], [10., 20.], [0., 10.]]), -1., 11.);
    let res = boolean(&square, &diamond, BoolOp::Cut).unwrap().unwrap();
    assert_solid(&res, 2000.0, 1e-9, 4, "diamond through a square");
    let block = prism(rect(0., 0., 31., 29.), 0., 10.);
    let tool = prism(disc(18.5, 4., 25.), -1., 11.);
    let res = boolean(&block, &tool, BoolOp::Cut).unwrap().unwrap();
    // Material left: the block's area outside the disc, as two corner posts 10 mm high.
    let n = 400_000;
    let dx = 31.0 / n as f64;
    let outside: f64 = (0..n)
        .map(|i| {
            let x = (i as f64 + 0.5) * dx - 18.5;
            let h = (625.0 - x * x).max(0.0).sqrt();
            (29.0 - (4.0 + h).min(29.0)).max(0.0) * dx
        })
        .sum();
    assert_solid(&res, outside * 10.0, 1e-4, 2, "disc tangent to the back side, cut through");
}

/// A hole tangent to a side from inside is one piece (the material goes round the hole), and a
/// disc standing on a larger one, tangent to its rim from inside, joins.
#[test]
fn hole_touching_the_outline_stays_one_piece() {
    let block = prism(rect(0., 0., 31., 29.), 0., 10.);
    let hole = prism(disc(5., 14.5, 5.), -1., 11.);
    let res = boolean(&block, &hole, BoolOp::Cut).unwrap().unwrap();
    assert_solid(&res, (899.0 - std::f64::consts::PI * 25.0) * 10.0, 1e-5, 1, "hole tangent to a side");
    let small = prism(disc(-2., 0., 10.), 0., 10.);
    let big = prism(disc(0., 0., 12.), 10., 20.);
    let res = boolean(&small, &big, BoolOp::Union).unwrap().unwrap();
    assert_solid(&res, std::f64::consts::PI * (100.0 + 144.0) * 10.0, 1e-5, 1, "disc tangent inside a larger one");
}
