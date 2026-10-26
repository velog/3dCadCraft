use super::hlr::{part_meshes, view_lines};
use super::sheet::{Prim, format_prims};
use super::*;
use crate::document::{DocumentData, DocumentId, Project};
use crate::part::{OriginPlane, PartDocument, PrimitiveFeature, PrimitiveKind, SketchPlacement};
use crate::sketch::Point2;

fn model() -> DocRef {
    DocRef::local(DocumentId::new_random())
}

fn close(a: f64, b: f64, tol: f64, what: &str) {
    assert!((a - b).abs() <= tol, "{what}: {a} vs {b}");
}

fn vclose(a: Vector3, b: Vector3, what: &str) {
    assert!((a - b).norm() < 1e-9, "{what}: {a:?} vs {b:?}");
}

/// Sheet sizes: ISO 216 and ANSI, both orientations.
#[test]
fn sheet_sizes() {
    assert_eq!(SheetSize::A3.dims(Orientation::Landscape), (420.0, 297.0));
    assert_eq!(SheetSize::A3.dims(Orientation::Portrait), (297.0, 420.0));
    assert_eq!(SheetSize::A0.dims(Orientation::Landscape), (1189.0, 841.0));
    let (w, h) = SheetSize::AnsiB.dims(Orientation::Landscape);
    close(w, 431.8, 1e-9, "ANSI B width");
    close(h, 279.4, 1e-9, "ANSI B height");
    let c = SheetSize::Custom {
        width: 100.0,
        height: 300.0,
    };
    assert_eq!(c.dims(Orientation::Landscape), (300.0, 100.0));
    // Every named size is landscape and ISO halves keep √2.
    for s in SheetSize::NAMED {
        let (w, h) = s.landscape();
        assert!(w > h, "{s:?}");
    }
    for s in [
        SheetSize::A4,
        SheetSize::A3,
        SheetSize::A2,
        SheetSize::A1,
        SheetSize::A0,
    ] {
        let (w, h) = s.landscape();
        close(w / h, 2f64.sqrt(), 0.01, "ISO ratio");
    }
}

/// Standard frames are right-handed, unit and look the right way.
#[test]
fn standard_frames() {
    for o in ViewOrientation::ALL {
        let f = o.frame();
        close(f.right.norm(), 1.0, 1e-12, "unit");
        close(f.up.norm(), 1.0, 1e-12, "unit");
        vclose(f.right.cross(&f.up), f.toward, o.label());
    }
    let f = ViewOrientation::Front.frame();
    vclose(f.right, Vector3::x(), "front right");
    vclose(f.up, Vector3::z(), "front up");
    let t = ViewOrientation::Top.frame();
    vclose(t.right, Vector3::x(), "top right");
    vclose(t.up, Vector3::y(), "top up");
}

/// Projected frames follow the projection type: third angle puts the view
/// of the right side on the right, first angle the view of the left side.
#[test]
fn projected_frames() {
    let front = ViewOrientation::Front.frame();
    let third = |d| front.projected(d, Projection::ThirdAngle);
    let first = |d| front.projected(d, Projection::FirstAngle);
    vclose(
        third(ProjectDir::Right).toward,
        Vector3::x(),
        "3rd right = right side",
    );
    vclose(third(ProjectDir::Up).toward, Vector3::z(), "3rd up = top");
    vclose(
        third(ProjectDir::Up).up,
        Vector3::y(),
        "top view: back is up",
    );
    vclose(
        first(ProjectDir::Right).toward,
        -Vector3::x(),
        "1st right = left side",
    );
    vclose(
        first(ProjectDir::Down).toward,
        Vector3::z(),
        "1st below = top",
    );
    // A view from the top is the same picture wherever it is placed.
    vclose(first(ProjectDir::Down).up, Vector3::y(), "1st top view up");
    vclose(
        third(ProjectDir::Up).right,
        first(ProjectDir::Down).right,
        "same top view",
    );
    for d in [ProjectDir::Left, ProjectDir::Down, ProjectDir::Iso(1, 1)] {
        let f = third(d);
        vclose(f.right.cross(&f.up), f.toward, "right-handed");
    }
    assert_eq!(ProjectDir::from_offset(100.0, 5.0), Some(ProjectDir::Right));
    assert_eq!(ProjectDir::from_offset(-3.0, -80.0), Some(ProjectDir::Down));
    assert_eq!(
        ProjectDir::from_offset(50.0, 60.0),
        Some(ProjectDir::Iso(1, 1))
    );
    assert_eq!(ProjectDir::from_offset(0.0, 0.0), None);
}

/// Views: ids, identifiers, alignment of projected views, inherited scale,
/// moving a parent moves aligned children, delete takes dependents.
#[test]
fn views_place_move_delete() {
    let mut d = DrawingDocument::default();
    assert_eq!(d.standard.projection, Projection::FirstAngle, "ISO default");
    let m = model();
    let b = d
        .add_base_view(
            m,
            ViewOrientation::Front,
            [150.0, 180.0],
            0.5,
            ViewStyle::HiddenRemoved,
        )
        .unwrap();
    let r = d.add_projected_view(b, [300.0, 170.0]).unwrap();
    let t = d.add_projected_view(b, [160.0, 60.0]).unwrap();
    let iso = d.add_projected_view(b, [300.0, 60.0]).unwrap();
    let sheet = d.sheet().unwrap().clone();
    assert_eq!(
        sheet.view(r).unwrap().position,
        [300.0, 180.0],
        "aligned row"
    );
    assert_eq!(
        sheet.view(t).unwrap().position,
        [150.0, 60.0],
        "aligned column"
    );
    assert_eq!(
        sheet.view(iso).unwrap().position,
        [300.0, 60.0],
        "iso is free"
    );
    assert_eq!(sheet.view(r).unwrap().identifier, "VIEW2");
    assert_eq!(d.effective(&sheet, r).unwrap().0, 0.5, "scale from base");
    assert_eq!(d.model_of(&sheet, iso), Some(m));
    // Moving the base drags its aligned children along.
    assert!(d.move_view(b, [170.0, 200.0]));
    let s = d.sheet().unwrap();
    assert_eq!(s.view(r).unwrap().position[1], 200.0);
    assert_eq!(s.view(t).unwrap().position[0], 170.0);
    // A projected view slides only along its axis.
    d.move_view(r, [320.0, 10.0]);
    assert_eq!(d.sheet().unwrap().view(r).unwrap().position, [320.0, 200.0]);
    // Scale of the base reaches the children.
    d.sheet_mut().unwrap().view_mut(b).unwrap().scale = 2.0;
    let s = d.sheet().unwrap().clone();
    assert_eq!(d.effective(&s, t).unwrap().0, 2.0);
    // Delete the base: every child goes with it; ids are not reused.
    let mut gone = d.delete_view(b);
    gone.sort();
    assert_eq!(gone, vec![b, r, t, iso]);
    assert!(d.sheet().unwrap().views.is_empty());
    let again = d
        .add_base_view(m, ViewOrientation::Top, [0.0, 0.0], 1.0, ViewStyle::Shaded)
        .unwrap();
    assert!(again > iso);
    assert!(d.delete_view(999).is_empty());
}

/// Sheets: New Sheet names, delete keeps one, numbering skips excluded.
#[test]
fn sheets() {
    let mut d = DrawingDocument::new(DrawingStandard::of(StandardKind::Ansi));
    assert_eq!(d.standard.projection, Projection::ThirdAngle);
    assert_eq!(d.sheets[0].size, SheetSize::AnsiB);
    assert_eq!(d.new_sheet(), 1);
    assert_eq!(d.sheets[1].name, "Sheet:2");
    d.new_sheet();
    d.sheets[1].exclude_from_count = true;
    assert_eq!(d.sheet_number(2), (2, 2));
    assert_eq!(d.sheet_number(1), (0, 2));
    assert!(d.delete_sheet(0));
    assert_eq!(d.active_sheet, 1);
    assert!(d.delete_sheet(0));
    assert!(!d.delete_sheet(0), "the last sheet stays");
    assert_eq!(d.new_sheet(), 1);
    assert_eq!(d.sheets[1].name, "Sheet:1");
}

/// Save / load round trip inside a project, references and Pack remap.
#[test]
fn round_trip_and_references() {
    let mut p = Project::new("P");
    let part = p.add_document("Part1", DocumentData::Part(PartDocument::default()));
    let mut d = DrawingDocument::default();
    let b = d
        .add_base_view(
            DocRef::local(part),
            ViewOrientation::IsoTopRight,
            [100.0, 100.0],
            1.0,
            ViewStyle::HiddenShown,
        )
        .unwrap();
    d.add_projected_view(b, [100.0, 30.0]).unwrap();
    let id = p.add_document("Drawing1", DocumentData::Drawing(d.clone()));
    p.validate().unwrap();
    let doc = p.document(id).unwrap();
    assert_eq!(doc.data.kind(), crate::document::DocumentKind::Drawing);
    assert_eq!(doc.data.references(), vec![DocRef::local(part)]);
    let json = serde_json::to_string(&doc.data).unwrap();
    let back: DocumentData = serde_json::from_str(&json).unwrap();
    let DocumentData::Drawing(e) = back else {
        panic!("not a drawing");
    };
    assert_eq!(e.sheets[0].views, d.sheets[0].views);
    assert_eq!(e.standard, d.standard);
    assert_eq!(
        serde_json::to_string(&DocumentData::Drawing(e)).unwrap(),
        json
    );
}

/// The border and title block: frame inside the sheet, fields filled.
#[test]
fn sheet_format() {
    let d = DrawingDocument::default();
    let prims = format_prims(&d, 0, &|f| match f {
        TitleField::Title => "BRACKET".into(),
        TitleField::PartNumber => "P-100".into(),
        _ => String::new(),
    });
    let texts: Vec<&str> = prims
        .iter()
        .filter_map(|p| match p {
            Prim::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    for want in ["BRACKET", "P-100", "TITLE", "SCALE", "1 / 1", "A", "1"] {
        assert!(texts.contains(&want), "{want} in {texts:?}");
    }
    let (w, h) = d.sheets[0].dims();
    for p in &prims {
        if let Prim::Line { a, b, .. } = p {
            for q in [a, b] {
                assert!(q[0] >= -1e-9 && q[0] <= w + 1e-9 && q[1] >= -1e-9 && q[1] <= h + 1e-9);
            }
        }
    }
    assert_eq!(scale_text(0.5), "1 : 2");
    assert_eq!(scale_text(2.0), "2 : 1");
    assert_eq!(scale_text(1.0), "1 : 1");
    assert_eq!(fit_scale((200.0, 100.0), (420.0, 297.0), 0.4), 0.5);
}

fn primitive(kind: PrimitiveKind, c: Point2, q: Point2, h: f64) -> PartDocument {
    let mut part = PartDocument::default();
    let f = PrimitiveFeature::extrude(&part, h);
    crate::part::primitive::add_primitive(
        &mut part,
        kind,
        SketchPlacement::on_origin_plane(OriginPlane::XY),
        &[c, q],
        f,
    )
    .unwrap();
    part
}

fn length(lines: &[Vec<[f64; 2]>]) -> f64 {
    lines
        .iter()
        .flat_map(|l| l.windows(2))
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .sum()
}

/// A 40 × 20 × 10 box: the front view is its 40 × 10 outline with nothing
/// hidden; the iso view hides the three back edges (30 + 10 + 20 long).
#[test]
fn hlr_box() {
    let part = primitive(
        PrimitiveKind::Box,
        Point2::new(0.0, 0.0),
        Point2::new(20.0, 10.0),
        10.0,
    );
    let meshes = part_meshes(&part);
    let front = view_lines(&meshes, &ViewOrientation::Front.frame());
    let (w, h) = front.size();
    close(w, 40.0, 1e-6, "front width");
    close(h, 10.0, 1e-6, "front height");
    close(length(&front.hidden), 0.0, 1e-6, "front: nothing hidden");
    // The outline (each edge seen twice: front and back edges coincide).
    assert!(length(&front.visible) >= 2.0 * (40.0 + 10.0) - 1e-6);
    let iso = view_lines(&meshes, &ViewOrientation::IsoTopRight.frame());
    // Hidden: the three edges at the back lower corner, foreshortened by
    // the isometric projection (√(2/3) of their length).
    let k = (2.0f64 / 3.0).sqrt();
    close(
        length(&iso.hidden),
        (40.0 + 20.0 + 10.0) * k,
        0.5,
        "iso hidden",
    );
    close(
        length(&iso.visible),
        (2.0 * 40.0 + 2.0 * 20.0 + 2.0 * 10.0 + 40.0 + 20.0 + 10.0) * k,
        0.5,
        "iso visible",
    );
}

/// A cylinder r 10, h 30 standing on XY: the front view is a 20 × 30
/// rectangle whose sides are silhouettes; the top view a circle.
#[test]
fn hlr_cylinder_silhouettes() {
    let part = primitive(
        PrimitiveKind::Cylinder,
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
        30.0,
    );
    let meshes = part_meshes(&part);
    let front = view_lines(&meshes, &ViewOrientation::Front.frame());
    let (w, h) = front.size();
    close(w, 20.0, 0.05, "front width");
    close(h, 30.0, 1e-6, "front height");
    // Two silhouette sides of 30 + top and bottom (front and back halves
    // of each circle overlap: 2 × 20 each, the back half hidden by nothing
    // but drawn on top of the front one).
    let vis = length(&front.visible);
    assert!(vis >= 2.0 * 30.0 + 2.0 * 20.0 - 0.2, "visible {vis}");
    let top = view_lines(&meshes, &ViewOrientation::Top.frame());
    let (w, h) = top.size();
    close(w, 20.0, 0.05, "top width");
    close(h, 20.0, 0.05, "top height");
    // The bottom circle lies under the top one: not drawn hidden.
    close(
        length(&top.hidden),
        0.0,
        1e-6,
        "no hidden line on a visible one",
    );
    close(
        length(&top.visible),
        2.0 * std::f64::consts::PI * 10.0,
        0.3,
        "one circle",
    );
}

/// A plate with a through hole, seen from the front: the hole's two walls
/// are hidden lines of the plate's height.
#[test]
fn hlr_hidden_hole() {
    let mut part = primitive(
        PrimitiveKind::Box,
        Point2::new(0.0, 0.0),
        Point2::new(30.0, 20.0),
        10.0,
    );
    let mut e = crate::part::ExtrudeFeature::new(Vec::new(), 10.0);
    e.operation = crate::part::Operation::Cut;
    let f = PrimitiveFeature::Extrude(e);
    crate::part::primitive::add_primitive(
        &mut part,
        PrimitiveKind::Cylinder,
        SketchPlacement::on_origin_plane(OriginPlane::XY),
        &[Point2::new(0.0, 0.0), Point2::new(5.0, 0.0)],
        f,
    )
    .unwrap();
    let front = view_lines(&part_meshes(&part), &ViewOrientation::Front.frame());
    let hidden = length(&front.hidden);
    assert!(hidden >= 2.0 * 10.0 - 0.2, "hole walls hidden: {hidden}");
    let (w, _) = front.size();
    close(w, 60.0, 1e-6, "plate width");
}
