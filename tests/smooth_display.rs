//! One visible face / edge from several kernel pieces (`kernel::smooth`):
//! a cylinder of the exact kernel comes in pieces; the seams between them
//! are not drawn, a piece's smooth group is the whole wall, and an arc of
//! a circle is grouped with the other arcs of that circle.

use cadcraft3d::kernel::{Curve, Point3, Real, Surface};
use cadcraft3d::part::{
    ExtrudeFeature, Operation, PartDocument, ProfileRef, SketchOptions, SketchPlacement,
};

fn boss(r: Real, h: Real) -> PartDocument {
    let mut p = PartDocument::with_box("Box1", [60.0, 40.0, 10.0]).unwrap();
    let o = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let j = p
        .create_sketch(SketchPlacement::on_face("Box1:Top"), &o)
        .unwrap();
    let c = p.sketches[j]
        .sketch
        .plane
        .project(&Point3::new(30.0, 20.0, 10.0));
    p.sketches[j].sketch.add_circle(c, r).unwrap();
    let rg = p.sketches[j].sketch.regions();
    let mut ex = ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], h);
    ex.operation = Operation::Join;
    p.add_extrude(j, ex).unwrap();
    p
}

#[test]
fn cylinder_pieces_are_one_face_and_one_circle() {
    for (r, h) in [(5.0, 8.0), (2.0, 3.0), (12.0, 20.0)] {
        let p = boss(r, h);
        assert!(p.exact);
        let b = &p.body;
        let walls: Vec<_> = b
            .faces()
            .filter(|(_, f)| {
                matches!(
                    b.surface(f.facet_of.unwrap_or(f.surface)),
                    Some(Surface::Cylinder(_))
                )
            })
            .map(|(id, _)| id)
            .collect();
        assert!(!walls.is_empty());
        // Every piece's group is the whole wall.
        for w in &walls {
            let mut g = b.smooth_group(*w);
            g.sort();
            let mut all = walls.clone();
            all.sort();
            assert_eq!(g, all, "r {r}");
        }
        // No visible edge lies between two wall pieces (no seam lines).
        let walls_of = |e| b.edge_faces(e).iter().filter(|f| walls.contains(f)).count();
        for e in b.visible_edges() {
            assert!(
                walls_of(e) < 2,
                "a seam between wall pieces is drawn (r {r})"
            );
        }
        // The top circle: its arcs form one edge of length 2πr.
        let top: Vec<_> = b
            .edges()
            .filter(|(_, e)| {
                matches!(b.curve(e.curve), Some(Curve::Circle(c)) if (c.center.z - (10.0 + h)).abs() < 1e-6)
            })
            .map(|(id, _)| id)
            .collect();
        assert!(!top.is_empty());
        let mut g = b.edge_group(top[0]);
        g.sort();
        let mut all = top.clone();
        all.sort();
        assert_eq!(g, all, "every arc of the top circle (r {r})");
        // The box's planar faces stay separate faces.
        let topf = b.find_face("Box1:Top").unwrap();
        assert_eq!(b.smooth_group(topf), vec![topf]);
    }
}
