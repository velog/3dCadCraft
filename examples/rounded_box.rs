//! The fillet/chamfer acceptance part: a 40×30×20 box with r = 5 fillets on
//! the vertical and bottom edges (spherical corners) and a 2 mm chamfer
//! around the whole top loop (straight parts and fillet arcs).
//!
//! ```sh
//! cargo run --example rounded_box -- /tmp/rounded.3dcadcraft
//! cargo run -- /tmp/rounded.3dcadcraft
//! ```

use std::path::PathBuf;

use cadcraft3d::document::{DocumentData, Project};
use cadcraft3d::part::blend::{edge_ref, tangent_chain};
use cadcraft3d::part::{BlendFeature, EdgeRef, PartDocument};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path: PathBuf = std::env::args_os()
        .nth(1)
        .map_or("rounded_box.3dcadcraft".into(), Into::into);
    let (a, b, h, r) = (40.0, 30.0, 20.0, 5.0);
    let mut part = PartDocument::with_box("Box1", [a, b, h])?;
    let f = |x: &str, y: &str, hint: [f64; 3]| EdgeRef {
        faces: [format!("Box1:{x}"), format!("Box1:{y}")],
        hint,
    };
    part.add_blend(BlendFeature::fillet(
        vec![
            f("Front", "Right", [a, 0.0, h / 2.0]),
            f("Right", "Back", [a, b, h / 2.0]),
            f("Back", "Left", [0.0, b, h / 2.0]),
            f("Left", "Front", [0.0, 0.0, h / 2.0]),
            f("Bottom", "Front", [a / 2.0, 0.0, 0.0]),
            f("Bottom", "Right", [a, b / 2.0, 0.0]),
            f("Bottom", "Back", [a / 2.0, b, 0.0]),
            f("Bottom", "Left", [0.0, b / 2.0, 0.0]),
        ],
        r,
    ))?;
    let top = part
        .body
        .edges()
        .find(|(e, _)| {
            edge_ref(&part.body, *e).is_some_and(|x| {
                x.faces.contains(&"Box1:Top".to_owned())
                    && x.faces.contains(&"Box1:Front".to_owned())
            })
        })
        .map(|(e, _)| e)
        .ok_or("top edge")?;
    part.add_blend(BlendFeature::chamfer(tangent_chain(&part.body, top), 2.0))?;
    let mut project = Project::new("RoundedBox");
    project.add_document("Part1", DocumentData::Part(part));
    project.save(&path)?;
    println!("saved {}", path.display());
    Ok(())
}
