//! Writes the cases for checking Smart Drop in the app (spec 6.3 §3.5): an assembly
//! with a plate whose Hole feature has four holes, a pipe of DN 100 and a part with
//! its own connection, and a published family with a connection.
//!
//! ```sh
//! cargo run --example library_cases -- out_dir library_dir
//! CADCRAFT3D_LIBRARY_DIR=library_dir cargo run -- out_dir/cases.3dcadcraft
//! ```

use std::path::PathBuf;

use cadcraft3d::components::catalog::publish;
use cadcraft3d::document::iprops::IProperties;
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, Project};
use cadcraft3d::kernel::{Curve, Point3, Transform3};
use cadcraft3d::part::blend::{EdgeRef, edge_ref};
use cadcraft3d::part::connection::{Connection, ConnectionRole};
use cadcraft3d::part::expr::Unit;
use cadcraft3d::part::family::{Column, FamilyTable, Target};
use cadcraft3d::part::hole::{HoleCenter, HoleFeature, HoleSpec, Termination};
use cadcraft3d::part::{
    Extent, ExtentDirection, ExtrudeFeature, Operation, OriginPlane, PartDocument, ProfileRef,
    SketchOptions, SketchPlacement,
};
use cadcraft3d::sketch::Point2;

type Fallible<T> = Result<T, Box<dyn std::error::Error>>;

fn options() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

/// 200 × 120 × 12 with Ø9 holes through it, one Hole feature.
fn plate() -> Fallible<PartDocument> {
    const TOP: &str = "Box1:Top";
    let mut p = PartDocument::with_box("Box1", [200.0, 120.0, 12.0])?;
    let centers: Vec<Point3> = [(30.0, 30.0), (170.0, 30.0), (170.0, 90.0), (30.0, 90.0)]
        .iter()
        .map(|&(x, y)| Point3::new(x, y, 12.0))
        .collect();
    let (i, ids) = p.create_hole_sketch(TOP, &centers)?;
    let mut spec = HoleSpec::simple(9.0, 12.0);
    spec.termination = Termination::ThroughAll;
    let mut def = HoleFeature::new(spec);
    def.sketch = p.sketches[i].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    p.add_hole(def)?;
    Ok(p)
}

/// A tube: `od` and `id`, `length` along +Z.
fn pipe(od: f64, id: f64, length: f64) -> Fallible<PartDocument> {
    let mut p = PartDocument {
        exact_only: true,
        ..Default::default()
    };
    let i = p.create_sketch(
        SketchPlacement::on_origin_plane(OriginPlane::XY),
        &options(),
    )?;
    let s = &mut p.sketches[i].sketch;
    s.add_circle(Point2::new(0.0, 0.0), od / 2.0)?;
    s.add_circle(Point2::new(0.0, 0.0), id / 2.0)?;
    let q = Point2::new((od + id) / 4.0, 0.0);
    let r = s.regions();
    let region = r.iter().find(|r| r.contains(q)).ok_or("no ring")?;
    let mut e = ExtrudeFeature::new(vec![ProfileRef::of(region, q)], length);
    if s.plane.normal.z < 0.0 {
        e.direction = ExtentDirection::Flipped;
    }
    p.add_extrude(i, e)?;
    Ok(p)
}

/// The circular edge of `radius` at height `z`, as a connection's reference.
fn rim(p: &PartDocument, radius: f64, z: f64) -> Fallible<EdgeRef> {
    let found = p.body.edges().find_map(|(id, e)| {
        let curve = p.body.curve(e.curve)?;
        let Curve::Circle(c) = curve else {
            return None;
        };
        let at = (c.radius - radius).abs() < 1e-6 && (c.center.z - z).abs() < 1e-6;
        at.then(|| edge_ref(&p.body, id)).flatten()
    });
    Ok(found.ok_or("no such rim")?)
}

/// A square plate with a pin: goes into a Ø10 hole, its own connection on the
/// pin's root. Not round, so Lock Rotation and Flip show.
fn pin_block() -> Fallible<PartDocument> {
    let mut p = PartDocument::with_box("Box1", [30.0, 30.0, 6.0])?;
    let i = p.create_sketch(SketchPlacement::on_face("Box1:Top"), &options())?;
    let c = p.sketches[i]
        .sketch
        .plane
        .project(&Point3::new(15.0, 15.0, 6.0));
    p.sketches[i].sketch.add_circle(c, 4.5)?;
    let r = p.sketches[i].sketch.regions();
    let region = r.first().ok_or("no circle")?;
    let mut e = ExtrudeFeature::new(vec![ProfileRef::of(region, region.interior_point())], 20.0);
    e.operation = Operation::Join;
    e.extent = Extent::Distance;
    p.add_extrude(i, e)?;
    let edge = rim(&p, 4.5, 6.0)?;
    p.connections.push(Connection {
        name: "Connection1".into(),
        role: ConnectionRole::Into,
        edge: Some(edge),
        diameter: 9.0,
        diameter_column: None,
    });
    Ok(p)
}

/// A family of collars with a connection: bore on a table column, droppable
/// over a shaft. The thickness comes from the table.
fn collar_family() -> Fallible<PartDocument> {
    let mut p = PartDocument::with_box("Box1", [150.0, 150.0, 30.0])?;
    p.rename_parameter("d2", "thick")?;
    let i = p.create_sketch(SketchPlacement::on_face("Box1:Top"), &options())?;
    let c = p.sketches[i]
        .sketch
        .plane
        .project(&Point3::new(75.0, 75.0, 30.0));
    p.sketches[i].sketch.add_circle(c, 57.25)?;
    let r = p.sketches[i].sketch.regions();
    let region = r.first().ok_or("no circle")?;
    let mut e = ExtrudeFeature::new(vec![ProfileRef::of(region, region.interior_point())], 1.0);
    e.operation = Operation::Cut;
    e.extent = Extent::ThroughAll;
    e.direction = ExtentDirection::Flipped;
    p.add_extrude(i, e)?;
    let edge = rim(&p, 57.25, 0.0)?;
    let mut column = Column::new("L", Unit::Mm, Target::Parameter("thick".into()));
    column.key = 1;
    let od = Column::new("OD", Unit::Mm, Target::Property("stock_number".into()));
    p.family = Some(FamilyTable {
        id: "collar-family".into(),
        columns: vec![column, od],
        rows: vec![
            vec!["30".into(), "114.3".into()],
            vec!["60".into(), "114.3".into()],
        ],
        default_row: 0,
    });
    p.connections.push(Connection {
        name: "Connection1".into(),
        role: ConnectionRole::Over,
        edge: Some(edge),
        diameter: 114.3,
        diameter_column: Some("OD".into()),
    });
    Ok(p)
}

fn main() -> Fallible<()> {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let out = args.next().ok_or("out_dir")?;
    let library = args.next().ok_or("library_dir")?;
    std::fs::create_dir_all(&out)?;
    std::fs::create_dir_all(&library)?;

    publish(
        &library,
        "Own",
        "Collars",
        &collar_family()?,
        &IProperties::default(),
    )?;

    let mut project = Project::new("Cases");
    let plate = project.add_document("Plate", DocumentData::Part(plate()?));
    let tube = project.add_document("Pipe DN100", DocumentData::Part(pipe(114.3, 107.1, 300.0)?));
    project.add_document("Pin Block", DocumentData::Part(pin_block()?));
    let mut a = AssemblyDocument::default();
    a.place(DocRef::local(plate), "Plate", Transform3::identity(), true);
    a.place(
        DocRef::local(tube),
        "Pipe",
        Transform3::translation(350.0, 60.0, 0.0),
        true,
    );
    project.add_document("Cases", DocumentData::Assembly(a));
    project.save(&out.join("cases.3dcadcraft"))?;
    Ok(())
}
