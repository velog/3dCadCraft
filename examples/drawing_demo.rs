//! Builds a part with holes and a drawing of it through the API — base,
//! projected, section and detail views, dimensions, centre marks,
//! hole notes, a note — saves the project and writes the sheet as a PDF.
//!
//! ```sh
//! cargo run --example drawing_demo -- /tmp/plate.3dcadcraft
//! cargo run -- /tmp/plate.3dcadcraft          # open it, then the drawing
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use cadcraft3d::document::{DocRef, DocumentData, Project};
use cadcraft3d::drawing::hlr::{self, ViewLines};
use cadcraft3d::drawing::{
    AnnotationKind, DimKind, Dimension, DrawingDocument, LinesJob, PointRef, PointRole,
    ViewOrientation, ViewStyle,
};
use cadcraft3d::kernel::Point3;
use cadcraft3d::part::{HoleCenter, HoleFeature, HoleSpec, PartDocument, SeatType, Termination};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path: PathBuf = std::env::args_os()
        .nth(1)
        .map_or("plate.3dcadcraft".into(), Into::into);

    // An 80 × 50 × 12 plate: two counterbored holes and a Ø16 hole.
    let mut part = PartDocument::with_box("Box1", [80.0, 50.0, 12.0])?;
    let pts = [Point3::new(15.0, 25.0, 12.0), Point3::new(65.0, 25.0, 12.0)];
    let (k, ids) = part.create_hole_sketch("Box1:Top", &pts)?;
    let mut s = HoleSpec::simple(6.6, 12.0);
    s.termination = Termination::ThroughAll;
    s.seat = SeatType::Counterbore;
    s.seat_diameter = 11.0;
    s.seat_depth = 6.4;
    let mut def = HoleFeature::new(s);
    def.sketch = part.sketches[k].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    part.add_hole(def)?;
    let (k, ids) = part.create_hole_sketch("Box1:Top", &[Point3::new(40.0, 25.0, 12.0)])?;
    let mut s = HoleSpec::simple(16.0, 12.0);
    s.termination = Termination::ThroughAll;
    let mut def = HoleFeature::new(s);
    def.sketch = part.sketches[k].id.clone();
    def.centers = ids.into_iter().map(HoleCenter::Point).collect();
    part.add_hole(def)?;

    let mut project = Project::new("PlateDrawing");
    let part_id = project.add_document("Plate", DocumentData::Part(part.clone()));
    let model = DocRef::local(part_id);

    // The views (A3, ISO first angle: the top view goes below the front).
    let mut d = DrawingDocument::default();
    let front = d
        .add_base_view(
            model,
            ViewOrientation::Front,
            [110.0, 215.0],
            1.5,
            ViewStyle::HiddenShown,
        )
        .ok_or("base view")?;
    let top = d
        .add_projected_view(front, [110.0, 120.0])
        .ok_or("top view")?;
    let side = d
        .add_projected_view(front, [300.0, 215.0])
        .ok_or("side view")?;
    // Section A-A along the holes' row in the top view (view plane = x, y).
    let sec = d
        .add_section_view(top, [-5.0, 25.0], [85.0, 25.0], [110.0, 60.0])
        .ok_or("section view")?;
    let det = d
        .add_detail_view(front, [15.0, 9.0], 7.0, [310.0, 120.0], 4.0)
        .ok_or("detail view")?;

    // Lines of every view, to find the edges the annotations attach to.
    let pm = hlr::part_model(&part);
    let sheet = d.sheet().ok_or("sheet")?.clone();
    let mut lines: std::collections::HashMap<u32, Arc<ViewLines>> = Default::default();
    for v in &sheet.views {
        if let Some(LinesJob::Model { frame, aligned, .. }) = d.lines_job(&sheet, v.id) {
            lines.insert(
                v.id,
                Arc::new(hlr::view_lines_with(&pm, &frame, aligned.as_ref())),
            );
        }
    }
    if let Some(LinesJob::Detail {
        parent,
        center,
        radius,
    }) = d.lines_job(&sheet, det)
        && let Some(p) = lines.get(&parent).cloned()
    {
        lines.insert(det, Arc::new(hlr::clip_circle(&p, center, radius)));
    }
    let top_lines = lines.get(&top).ok_or("top lines")?.clone();
    let circles: Vec<(String, [f64; 2], f64, bool)> = top_lines
        .edges
        .iter()
        .filter(|e| e.visible)
        .filter_map(|e| Some((e.name.clone()?, e.circle?.0, e.circle?.1, e.note.is_some())))
        .collect();
    // Centre marks on the visible hole circles, notes on the first of each size.
    let mut noted = Vec::new();
    for (name, c, r, hole) in &circles {
        d.add_annotation(AnnotationKind::CenterMark {
            view: top,
            edge: name.clone(),
        });
        let size = (r * 10.0).round() as i64;
        if *hole && !noted.contains(&size) {
            noted.push(size);
            d.add_annotation(AnnotationKind::HoleNote {
                view: top,
                edge: name.clone(),
                text: [(c[0] - 40.0) * 1.5 + 35.0, 48.0 + noted.len() as f64 * 6.0],
            });
        }
    }
    // Overall length of the plate on the top view: the longest straight edge.
    if let Some(e) = top_lines
        .edges
        .iter()
        .filter(|e| e.straight && e.visible && e.name.is_some())
        .max_by(|a, b| (a.b[0] - a.a[0]).abs().total_cmp(&(b.b[0] - b.a[0]).abs()))
    {
        let name = e.name.clone().unwrap_or_default();
        d.add_annotation(AnnotationKind::Dimension(Dimension {
            dim: DimKind::Horizontal,
            a: PointRef {
                view: top,
                edge: name.clone(),
                role: PointRole::Start,
            },
            b: Some(PointRef {
                view: top,
                edge: name,
                role: PointRole::End,
            }),
            text: [0.0, -50.0],
            precision: 2,
            format: Default::default(),
        }));
    }
    d.add_annotation(AnnotationKind::Text {
        at: [30.0, 30.0],
        text: "NOTES:\n1. Break all sharp edges.\n2. Забележка: без чапаци.".into(),
        height: 3.5,
    });
    let _ = (side, sec);
    let drawing_id = project.add_document("Drawing1", DocumentData::Drawing(d.clone()));
    let _ = drawing_id;
    project.save(&path)?;
    println!("saved {}", path.display());

    // The sheet as a PDF (texts need a font: Arial on Windows when present).
    let font = [
        "C:/Windows/Fonts/arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ]
    .iter()
    .find_map(|f| std::fs::read(f).ok());
    let prims = cadcraft3d::drawing::render::sheet_list(
        &d,
        0,
        &|_| "PLATE".into(),
        &|id| lines.get(&id).cloned(),
        &Default::default(),
    );
    let pdf = cadcraft3d::drawing::pdf::write(
        &[cadcraft3d::drawing::pdf::Page {
            size: sheet.dims(),
            prims,
        }],
        font,
        Default::default(),
    )?;
    let pdf_path = path.with_extension("pdf");
    std::fs::write(&pdf_path, pdf)?;
    println!("wrote {}", pdf_path.display());
    Ok(())
}
