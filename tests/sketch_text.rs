//! Sketch text extruded (spec 1.11). The expected areas are Inventor's behaviour with the same
//! fonts; a machine without them (CI on Linux) skips those cases, the fixture in `tests/compat`
//! proves a saved text builds without its font.
//!
//! Inventor's outlines are hinted by the system, ours are the font file's: single letters differ by
//! up to 2 % (x in Arial 19.87 mm2 here 19.49, T 21.06 / 21.03), hence the tolerance.

use cadcraft3d::kernel::Real;
use cadcraft3d::part::{
    ExtrudeFeature, Operation, OriginPlane, PartDocument, PlaneRef, ProfileRef, SketchOptions,
    SketchPlacement,
};
use cadcraft3d::sketch::fonts::{LoadedFont, installed};
use cadcraft3d::sketch::text::{TextAlign, TextSpec};
use nalgebra::Point2;

fn part_with(spec: TextSpec) -> Option<PartDocument> {
    if !installed(&spec.font) {
        return None;
    }
    let font = LoadedFont::find(&spec.font, spec.bold, spec.italic).unwrap();
    let mut p = PartDocument::default();
    let i = p
        .create_sketch(
            SketchPlacement {
                plane: PlaneRef::Origin(OriginPlane::XY),
                ..Default::default()
            },
            &SketchOptions {
                auto_project_origin: false,
                auto_reference_edges: false,
                ..Default::default()
            },
        )
        .unwrap();
    p.sketches[i]
        .sketch
        .add_text(Point2::origin(), 0.0, spec, &font)
        .unwrap();
    let r: Vec<ProfileRef> = p.sketches[i]
        .sketch
        .regions()
        .iter()
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect();
    let mut ex = ExtrudeFeature::new(r, 1.0);
    ex.operation = Operation::NewSolid;
    p.add_extrude(i, ex).unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    Some(p)
}

fn volume(p: &PartDocument) -> Real {
    p.exact_bodies
        .0
        .iter()
        .filter_map(|s| s.volume_at(1e-3))
        .sum()
}

#[test]
fn extruded_text_has_the_fonts_area() {
    // (text, font, size, bold, italic, stretch) → Inventor's volume at 1 mm.
    let cases: &[(&str, &str, Real, bool, bool, Real, Real)] = &[
        ("H", "Arial", 10.0, false, false, 1.0, 32.6475),
        ("O", "Arial", 10.0, false, false, 1.0, 34.7141),
        ("HH", "Arial", 10.0, false, false, 1.0, 65.2950),
        ("AV", "Arial", 10.0, false, false, 1.0, 54.8497),
        ("H", "Arial", 20.0, false, false, 1.0, 130.5901),
        ("H", "Arial", 10.0, true, false, 1.0, 47.0898),
        ("H", "Arial", 10.0, false, true, 1.0, 32.5976),
        ("H", "Times New Roman", 10.0, false, false, 1.0, 35.5127),
        ("\u{416}", "Arial", 10.0, false, false, 1.0, 47.8523),
        ("Text", "Arial", 10.0, false, false, 1.0, 80.9859),
        ("H", "Arial", 10.0, false, false, 0.5, 16.3148),
        ("H\nH", "Arial", 10.0, false, false, 1.0, 65.2950),
    ];
    for (text, font, size, bold, italic, stretch, want) in cases {
        let mut spec = TextSpec::new(*text, *font, *size);
        (spec.bold, spec.italic, spec.stretch) = (*bold, *italic, *stretch);
        let Some(p) = part_with(spec) else {
            eprintln!("skipped: {font} is not installed");
            continue;
        };
        let v = volume(&p);
        assert!(
            (v - want).abs() < want * 1e-2,
            "{text:?} {font} {size} bold {bold} italic {italic}: {v} vs {want}"
        );
    }
}

#[test]
fn the_box_stands_where_inventors_does() {
    let spec = TextSpec::new("H", "Arial", 10.0);
    let Some(p) = part_with(spec) else {
        return;
    };
    let (lo, hi) = p.body.bounding_box().unwrap();
    // Inventor: x 1.1280..8.9634, y -10..0.
    for (got, want) in [(lo.x, 1.1280), (hi.x, 8.9634), (lo.y, -10.0), (hi.y, 0.0)] {
        assert!((got - want).abs() < 0.03, "{got} vs {want}");
    }
    // Two lines: the second baseline 25.737 below the top, no kerning in AV.
    let Some(p) = part_with(TextSpec::new("H\nH", "Arial", 10.0)) else {
        return;
    };
    let (lo, _) = p.body.bounding_box().unwrap();
    assert!((lo.y + 25.737).abs() < 1e-3, "{}", lo.y);
    let mut c = TextSpec::new("H", "Arial", 10.0);
    c.align = TextAlign::Center;
    let Some(p) = part_with(c) else {
        return;
    };
    let (lo, hi) = p.body.bounding_box().unwrap();
    assert!((lo.x + hi.x).abs() < 0.2, "centred: {} {}", lo.x, hi.x);
}

#[test]
fn a_save_keeps_the_outlines_without_the_font() {
    let Some(p) = part_with(TextSpec::new("Ab", "Arial", 10.0)) else {
        return;
    };
    let v = volume(&p);
    let json = serde_json::to_string(&p).unwrap();
    // The font's name changed to one that does not exist: the text does not need it.
    let json = json.replace("\"Arial\"", "\"No Such Font\"");
    let mut q: PartDocument = serde_json::from_str(&json).unwrap();
    q.regenerate().unwrap();
    assert!((volume(&q) - v).abs() < 1e-6);
}

/// Letters over a face's edge stay exact (B-spline walls sent this to the faceted kernel).
#[test]
fn letters_over_the_edge_stay_exact() {
    if !installed("Arial") {
        return;
    }
    let font = LoadedFont::find("Arial", false, false).unwrap();
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let mut p = PartDocument::with_box("Box1", [100.0, 60.0, 20.0]).unwrap();
    let i = p
        .create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)
        .unwrap();
    let plane = p.sketches[i].sketch.plane.clone();
    let at = plane.project(&nalgebra::Point3::new(8.0, 6.0, 20.0));
    p.sketches[i]
        .sketch
        .add_text(at, 0.0, TextSpec::new("Ab1", "Arial", 12.0), &font)
        .unwrap();
    let r: Vec<ProfileRef> = p.sketches[i]
        .sketch
        .regions()
        .iter()
        .map(|r| ProfileRef::of(r, r.interior_point()))
        .collect();
    let mut ex = ExtrudeFeature::new(r, 10.0);
    ex.operation = Operation::Join;
    p.add_extrude(i, ex).unwrap();
    assert!(p.exact, "the part went faceted");
    assert!(p.feature_health.is_empty());
    assert!(volume(&p) > 120000.0);
}

/// Emboss takes a text like any other profile.
#[test]
fn a_text_is_embossed_and_engraved() {
    use cadcraft3d::part::emboss::{EmbossKind, EmbossOp};
    use cadcraft3d::part::{BodyOp, BodyOpFeature};
    if !installed("Arial") {
        return;
    }
    let font = LoadedFont::find("Arial", false, false).unwrap();
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    // The letter's area at 1 mm depth, from Inventor above: H at 10 mm is 32.6475 mm2.
    for (style, want) in [
        (EmbossKind::EmbossFromFace, 120000.0 + 2.0 * 32.6475),
        (EmbossKind::EngraveFromFace, 120000.0 - 2.0 * 32.6475),
    ] {
        let mut p = PartDocument::with_box("Box1", [100.0, 60.0, 20.0]).unwrap();
        let i = p
            .create_sketch(SketchPlacement::on_face("Box1:Top"), &opts)
            .unwrap();
        let plane = p.sketches[i].sketch.plane.clone();
        let at = plane.project(&nalgebra::Point3::new(40.0, 40.0, 20.0));
        p.sketches[i]
            .sketch
            .add_text(at, 0.0, TextSpec::new("H", "Arial", 10.0), &font)
            .unwrap();
        let mut op = EmbossOp::new(p.sketches[i].id.clone(), 2.0);
        op.style = style;
        p.add_body_op(BodyOpFeature::new(BodyOp::Emboss(op)))
            .unwrap();
        let v = volume(&p);
        assert!((v - want).abs() < 0.2, "{style:?}: {v} vs {want}");
    }
}
