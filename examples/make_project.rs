//! Builds a small project through the public API and saves it in both forms.
//!
//! ```sh
//! cargo run --example make_project -- /tmp/bracket.3dcadcraft
//! cargo run -- /tmp/bracket.3dcadcraft          # open it in the GUI
//! ```

use std::path::PathBuf;

use cadcraft3d::document::{
    DocRef, DocumentData, Occurrence, PartDocument, PartSketch, Project, SaveOptions,
};
use cadcraft3d::kernel::{BRep, Plane, Point3, Transform3, Vector3};
use cadcraft3d::sketch::{Constraint, Point2, Sketch};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path: PathBuf = std::env::args_os()
        .nth(1)
        .map_or("bracket.3dcadcraft".into(), Into::into);

    // Body: a 50 × 30 × 10 mm plate.
    let mut body = BRep::new();
    body.make_box(Point3::origin(), Point3::new(50.0, 30.0, 10.0))?;

    // Sketch on the top face: a fully dimensioned 30 × 12 rectangle.
    let top = Plane::new(Point3::new(10.0, 9.0, 10.0), Vector3::z(), Vector3::x())?;
    let mut sketch = Sketch::new(top);
    let c = [
        sketch.add_fixed_point(Point2::new(0.0, 0.0)),
        sketch.add_point(Point2::new(28.0, 1.0)),
        sketch.add_point(Point2::new(31.0, 13.0)),
        sketch.add_point(Point2::new(-1.0, 11.0)),
    ];
    let mut lines = Vec::new();
    for i in 0..4 {
        lines.push(sketch.add_line_between(c[i], c[(i + 1) % 4])?);
    }
    sketch.add_constraint(Constraint::Horizontal { line: lines[0] })?;
    sketch.add_constraint(Constraint::Vertical { line: lines[1] })?;
    sketch.add_constraint(Constraint::Horizontal { line: lines[2] })?;
    sketch.add_constraint(Constraint::Vertical { line: lines[3] })?;
    sketch.add_constraint(Constraint::Distance {
        a: c[0],
        b: c[1],
        value: 30.0,
    })?;
    sketch.add_constraint(Constraint::Distance {
        a: c[1],
        b: c[2],
        value: 12.0,
    })?;
    let report = sketch.solve()?;
    println!("sketch: {:?}, DOF {}", report.status, report.dof);

    let mut project = Project::new("Bracket");
    let plate = project.add_document(
        "Plate",
        DocumentData::Part(PartDocument {
            sketches: vec![PartSketch::new("Sketch1", sketch)],
            body,
            ..Default::default()
        }),
    );
    // The same part placed twice: stored once, referenced by ID.
    let place = |name: &str, z: f64, grounded| Occurrence {
        name: name.into(),
        component: DocRef::local(plate),
        transform: Transform3::translation(0.0, 0.0, z),
        grounded,
    };
    project.add_document(
        "Stack",
        DocumentData::Assembly(cadcraft3d::document::AssemblyDocument {
            occurrences: vec![place("Plate:1", 0.0, true), place("Plate:2", 10.0, false)],
        }),
    );

    project.save(&path)?;
    let folder = path.with_extension("src.3dcadcraft");
    project.save_folder(&folder, &SaveOptions::default())?;
    println!("saved {} and {}", path.display(), folder.display());
    Ok(())
}
