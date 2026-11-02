//! An assembly to try the Motion and Contact tools (spec 3.1b): an off-centre wheel and pinion
//! on Rotational joints for Motion Links; an eccentric cam with a follower on a Slider for
//! Transitional; a block on a Slider and a stop for the Contact Solver.
//!
//! `cargo run --example motion_demo -- out.3dcadcraft`

use cadcraft3d::assembly::motion::{ContactSet, LinkKind, MotionLink};
use cadcraft3d::assembly::relation::{Constraint, ConstraintKind, GeomRef, OriginItem};
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, Project};
use cadcraft3d::kernel::{Real, Transform3, Vector3};
use cadcraft3d::part::{
    ExtrudeFeature, OriginPlane, PartDocument, ProfileRef, SketchOptions, SketchPlacement,
};
use cadcraft3d::sketch::Point2;

fn opts() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

/// A disc of radius `r`, its centre `off` along X from the part's origin (the turning axis),
/// `h` thick.
fn disc(r: Real, off: Real, h: Real) -> Result<PartDocument, Box<dyn std::error::Error>> {
    let mut part = PartDocument::default();
    let i = part.create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())?;
    part.sketches[i]
        .sketch
        .add_circle(Point2::new(off, 0.0), r)?;
    let regions = part.sketches[i].sketch.regions();
    part.add_extrude(
        i,
        ExtrudeFeature::new(
            vec![ProfileRef::of(&regions[0], regions[0].interior_point())],
            h,
        ),
    )?;
    Ok(part)
}

fn origin(occurrence: u32, item: OriginItem) -> GeomRef {
    GeomRef::Origin {
        sub: Vec::new(),
        occurrence,
        item,
    }
}

fn at(x: Real, y: Real, z: Real) -> Transform3 {
    Transform3::new(Vector3::new(x, y, z), Vector3::zeros())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "motion_demo.3dcadcraft".into());
    let mut project = Project::new("Motion demo");
    let mut doc = |name: &str, part: PartDocument| {
        DocRef::local(project.add_document(name, DocumentData::Part(part)))
    };
    let pin = doc("Pivot", PartDocument::with_box("Box1", [2.0, 2.0, 2.0])?);
    let wheel = doc("Wheel", disc(30.0, 6.0, 10.0)?);
    let pinion = doc("Pinion", disc(15.0, 4.0, 10.0)?);
    let cam = doc("Cam", disc(20.0, 8.0, 10.0)?);
    let follower = doc(
        "Follower",
        PartDocument::with_box("Box1", [10.0, 10.0, 40.0])?,
    );
    let block = doc("Block", PartDocument::with_box("Box1", [20.0, 20.0, 20.0])?);
    let stop = doc("Stop", PartDocument::with_box("Box1", [20.0, 20.0, 20.0])?);

    let mut a = AssemblyDocument::default();
    let pivot =
        |a: &mut AssemblyDocument, x: Real, y: Real| a.place(pin, "Pivot", at(x, y, 0.0), true);
    let wh = a.place(wheel, "Wheel", at(0.0, 0.0, 0.0), false);
    let pi = a.place(pinion, "Pinion", at(45.0, 0.0, 0.0), false);
    let ca = a.place(cam, "Cam", at(0.0, 100.0, 0.0), false);
    let fo = a.place(follower, "Follower", at(40.0, 95.0, 0.0), false);
    let bl = a.place(block, "Block", at(0.0, -100.0, 0.0), false);
    let st = a.place(stop, "Stop", at(80.0, -100.0, 0.0), true);
    let joint = |a: &mut AssemblyDocument, kind, p: u32, o: u32, item: OriginItem| {
        let mut c = Constraint::new(kind, origin(p, item), origin(o, item));
        c.aligned = true;
        a.add_constraint(c);
    };
    use ConstraintKind::{Rotational, Slider};
    let (p1, p2, p3) = (
        pivot(&mut a, 0.0, 0.0),
        pivot(&mut a, 45.0, 0.0),
        pivot(&mut a, 0.0, 100.0),
    );
    joint(&mut a, Rotational, p1, wh, OriginItem::ZAxis);
    joint(&mut a, Rotational, p2, pi, OriginItem::ZAxis);
    joint(&mut a, Rotational, p3, ca, OriginItem::ZAxis);
    let (p4, p5) = (pivot(&mut a, 0.0, 95.0), pivot(&mut a, 0.0, -100.0));
    joint(&mut a, Slider, p4, fo, OriginItem::XAxis);
    joint(&mut a, Slider, p5, bl, OriginItem::XAxis);

    // Ready to play: the wheel drives the block like a rack (200 mm a turn), the follower
    // rides on the cam, the block stops at the stop.
    let turn = a.constraints[0].id;
    let (ride, rack) = (a.constraints[3].id, a.constraints[4].id);
    a.motion.contact_enabled = true;
    a.motion.contact_sets.push(ContactSet {
        id: 1,
        name: "Block and Stop".into(),
        occurrences: vec![bl, st],
        suppressed: false,
    });
    a.motion.links.push(MotionLink {
        id: 2,
        driver: turn,
        driven: rack,
        kind: LinkKind::RotationTranslation,
        ratio: 200.0 / std::f64::consts::TAU,
        offset: 0.0,
        cam: None,
        suppressed: false,
        last: Some(0.0),
    });
    a.motion.links.push(MotionLink {
        id: 3,
        driver: 0,
        driven: ride,
        kind: LinkKind::Transitional,
        ratio: 0.0,
        offset: 0.0,
        cam: Some(ca),
        suppressed: false,
        last: None,
    });

    project.add_document("Assembly", DocumentData::Assembly(a));
    project.save(std::path::Path::new(&path))?;
    println!("saved {path}");
    Ok(())
}
