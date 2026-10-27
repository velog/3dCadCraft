use std::sync::Arc;

use super::hlr::{part_model, view_lines};
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
    for want in [
        "BRACKET",
        "P-100",
        "Designed by",
        "Scale",
        "Sheet",
        "1 / 1",
        "A",
        "1",
        "6",
    ] {
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
    let meshes = part_model(&part);
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
    let meshes = part_model(&part);
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
    let front = view_lines(&part_model(&part), &ViewOrientation::Front.frame());
    let hidden = length(&front.hidden);
    assert!(hidden >= 2.0 * 10.0 - 0.2, "hole walls hidden: {hidden}");
    let (w, _) = front.size();
    close(w, 60.0, 1e-6, "plate width");
}

/// Lines of a part in a standard view, with the view's placement.
fn lines_of(part: &PartDocument, o: ViewOrientation) -> super::hlr::ViewLines {
    view_lines(&part_model(part), &o.frame())
}

/// Annotations attach to edges by name: a horizontal dimension over the
/// box's 40 mm front edge, a diameter, a centre mark and a centreline on a
/// cylinder seen from the top; a missing edge orphans, never panics.
#[test]
fn annotations_resolve_by_edge_name() {
    use super::annot::{Placement, resolve};
    let boxed = primitive(
        PrimitiveKind::Box,
        Point2::new(0.0, 0.0),
        Point2::new(20.0, 10.0),
        10.0,
    );
    let front = lines_of(&boxed, ViewOrientation::Front);
    // A visible straight edge 40 long in x.
    let e = front
        .edges
        .iter()
        .find(|e| e.straight && e.visible && ((e.b[0] - e.a[0]).abs() - 40.0).abs() < 1e-6)
        .expect("a 40 mm edge");
    let name = e.name.clone().expect("named edge");
    let pl = Placement {
        position: [100.0, 100.0],
        scale: 2.0,
        center: front.center(),
    };
    let view = |v: u32| (v == 1).then_some((pl, &front));
    let dim = |dim: DimKind, b: Option<PointRef>| {
        AnnotationKind::Dimension(Dimension {
            dim,
            a: PointRef {
                view: 1,
                edge: name.clone(),
                role: PointRole::Start,
            },
            b,
            text: [0.0, 30.0],
            precision: 2,
            format: Default::default(),
        })
    };
    let end = Some(PointRef {
        view: 1,
        edge: name.clone(),
        role: PointRole::End,
    });
    let h = resolve(&dim(DimKind::Horizontal, end.clone()), &view).unwrap();
    close(h.value.unwrap(), 40.0, 1e-9, "horizontal value (model mm)");
    let v = resolve(&dim(DimKind::Vertical, end.clone()), &view).unwrap();
    close(
        v.value.unwrap(),
        0.0,
        1e-9,
        "vertical value of a horizontal edge",
    );
    let a = resolve(&dim(DimKind::Aligned, end), &view).unwrap();
    close(a.value.unwrap(), 40.0, 1e-9, "aligned value");
    assert!(
        h.prims
            .iter()
            .any(|p| matches!(p, super::sheet::Prim::Text { text, .. } if text == "40,00")),
        "value text"
    );
    assert_eq!(
        h.prims
            .iter()
            .filter(|p| matches!(p, super::sheet::Prim::Fill { .. }))
            .count(),
        2,
        "two arrows"
    );
    // Orphan: an edge that does not exist.
    let orphan = AnnotationKind::CenterMark {
        view: 1,
        edge: "Nope:E1".into(),
    };
    assert!(matches!(
        resolve(&orphan, &view),
        Err(super::annot::Orphan::Missing(_))
    ));
    assert!(matches!(
        resolve(&orphan, &|_| None),
        Err(super::annot::Orphan::NoView)
    ));

    let cyl = primitive(
        PrimitiveKind::Cylinder,
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
        30.0,
    );
    let top = lines_of(&cyl, ViewOrientation::Top);
    let circles: Vec<String> = top
        .edges
        .iter()
        .filter(|e| e.circle.is_some())
        .filter_map(|e| e.name.clone())
        .collect();
    assert!(
        !circles.is_empty(),
        "the cylinder's circles are seen face-on"
    );
    let pl = Placement {
        position: [50.0, 50.0],
        scale: 1.0,
        center: top.center(),
    };
    let view = |v: u32| (v == 2).then_some((pl, &top));
    let d = AnnotationKind::Dimension(Dimension {
        dim: DimKind::Diameter,
        a: PointRef {
            view: 2,
            edge: circles[0].clone(),
            role: PointRole::Center,
        },
        b: None,
        text: [20.0, 20.0],
        precision: 2,
        format: Default::default(),
    });
    let r = resolve(&d, &view).unwrap();
    close(r.value.unwrap(), 20.0, 1e-9, "diameter");
    assert!(
        r.prims
            .iter()
            .any(|p| matches!(p, super::sheet::Prim::Text { text, .. } if text == "Ø20,00"))
    );
    let cm = resolve(
        &AnnotationKind::CenterMark {
            view: 2,
            edge: circles[0].clone(),
        },
        &view,
    )
    .unwrap();
    // The cross (2 lines through the centre) and 4 centre-line pieces past
    // the circle by the overshoot (2.5 mm, Inventor's ISO).
    let crosses: Vec<_> = cm
        .prims
        .iter()
        .filter_map(|p| match p {
            super::sheet::Prim::Line { a, b, .. } => Some((*a, *b)),
            _ => None,
        })
        .collect();
    assert_eq!(crosses.len(), 2);
    for (a, b) in crosses {
        close((a[0] + b[0]) * 0.5, 50.0, 1e-6, "cross x");
        close((a[1] + b[1]) * 0.5, 50.0, 1e-6, "cross y");
    }
    let reach = cm
        .prims
        .iter()
        .filter_map(|p| match p {
            super::sheet::Prim::Center { a, b, .. } => Some(
                (a[0] - 50.0)
                    .hypot(a[1] - 50.0)
                    .max((b[0] - 50.0).hypot(b[1] - 50.0)),
            ),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(reach.len(), 4);
    for r in reach {
        close(r, 10.0 + 2.5, 1e-6, "past the circle by the overshoot");
    }
    assert_eq!(super::annot::format_value(12.5, 2), "12.5");
    assert_eq!(super::annot::format_value(20.0, 2), "20");
    assert_eq!(super::annot::format_value(1.005, 0), "1");
}

/// Annotations are stored, numbered, removed with their view and survive a
/// save round trip.
#[test]
fn annotations_in_the_document() {
    let mut d = DrawingDocument::default();
    let b = d
        .add_base_view(
            model(),
            ViewOrientation::Front,
            [100.0, 100.0],
            1.0,
            ViewStyle::HiddenRemoved,
        )
        .unwrap();
    let cm = d
        .add_annotation(AnnotationKind::CenterMark {
            view: b,
            edge: "Extrusion1:E1".into(),
        })
        .unwrap();
    let t = d
        .add_annotation(AnnotationKind::Text {
            at: [20.0, 20.0],
            text: "NOTE 1\nЗабележка".into(),
            height: 3.5,
        })
        .unwrap();
    assert!(t > cm);
    let json = serde_json::to_string(&d).unwrap();
    let back: DrawingDocument = serde_json::from_str(&json).unwrap();
    assert_eq!(back.sheets[0].annotations, d.sheets[0].annotations);
    d.delete_view(b);
    let left: Vec<u32> = d.sheets[0].annotations.iter().map(|a| a.id).collect();
    assert_eq!(
        left,
        vec![t],
        "the view's centre mark went with it, the text stays"
    );
    assert!(d.delete_annotation(t));
    assert!(!d.delete_annotation(t));
    let mut e = back;
    e.next_annotation_id = 0;
    e.fix();
    assert!(e.next_annotation_id > t, "ids are never reused after load");
}

/// The sheet's display list goes to PDF: a valid document, one page per
/// sheet.
#[test]
fn pdf_of_a_sheet() {
    let part = primitive(
        PrimitiveKind::Box,
        Point2::new(0.0, 0.0),
        Point2::new(20.0, 10.0),
        10.0,
    );
    let mut d = DrawingDocument::default();
    let b = d
        .add_base_view(
            model(),
            ViewOrientation::Front,
            [150.0, 180.0],
            2.0,
            ViewStyle::HiddenShown,
        )
        .unwrap();
    d.add_projected_view(b, [150.0, 80.0]).unwrap();
    d.new_sheet();
    let lines_front = Arc::new(lines_of(&part, ViewOrientation::Front));
    let lines_top = Arc::new(lines_of(&part, ViewOrientation::Bottom));
    let pages: Vec<super::pdf::Page> = (0..d.sheets.len())
        .map(|i| {
            let lines = |v: u32| {
                Some(if v == b {
                    lines_front.clone()
                } else {
                    lines_top.clone()
                })
            };
            super::pdf::Page {
                size: d.sheets[i].dims(),
                prims: super::render::sheet_list(
                    &d,
                    i,
                    &|_| "TITLE".into(),
                    &lines,
                    &Default::default(),
                ),
            }
        })
        .collect();
    assert!(
        pages[0]
            .prims
            .iter()
            .any(|p| matches!(p, super::sheet::Prim::Path { dashed: false, .. }))
    );
    let bytes = super::pdf::write(&pages, None, Default::default()).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Count 2"), "two pages");
}

fn plate_with_hole() -> PartDocument {
    // 60 × 40 × 10 centred on the origin, a Ø10 hole through the middle.
    let mut part = primitive(
        PrimitiveKind::Box,
        Point2::new(0.0, 0.0),
        Point2::new(30.0, 20.0),
        10.0,
    );
    let mut e = crate::part::ExtrudeFeature::new(Vec::new(), 10.0);
    e.operation = crate::part::Operation::Cut;
    crate::part::primitive::add_primitive(
        &mut part,
        PrimitiveKind::Cylinder,
        SketchPlacement::on_origin_plane(OriginPlane::XY),
        &[Point2::new(0.0, 0.0), Point2::new(5.0, 0.0)],
        crate::part::PrimitiveFeature::Extrude(e),
    )
    .unwrap();
    part
}

fn area(lp: &[[f64; 2]]) -> f64 {
    lp.windows(2)
        .map(|w| w[0][0] * w[1][1] - w[1][0] * w[0][1])
        .sum::<f64>()
        .abs()
        * 0.5
}

/// Section A-A through the hole of a plate seen from the top: two cut faces
/// of 15 × 10 each, hatched; the view looks square to the line and stays
/// aligned with the parent along it.
#[test]
fn section_view_of_a_plate() {
    let part = plate_with_hole();
    let mut d = DrawingDocument::default();
    let b = d
        .add_base_view(
            model(),
            ViewOrientation::Top,
            [150.0, 150.0],
            1.0,
            ViewStyle::HiddenRemoved,
        )
        .unwrap();
    // A vertical line through the hole (top view plane = x, y).
    let s = d
        .add_section_view(b, [0.0, -25.0], [0.0, 25.0], [250.0, 160.0])
        .unwrap();
    let sheet = d.sheet().unwrap().clone();
    let v = sheet.view(s).unwrap();
    assert_eq!(v.identifier, "A");
    assert_eq!(
        v.position,
        [250.0, 150.0],
        "slides square to the line from the parent"
    );
    assert_eq!(v.label_lines(1.0)[0], "SECTION A-A");
    let LinesJob::Model { frame, aligned, .. } = d.lines_job(&sheet, s).unwrap() else {
        panic!("a model job");
    };
    // The line (y) stays up on the sheet; the view looks along ±x.
    vclose(frame.up, Vector3::y(), "section up");
    assert!(frame.toward.x.abs() > 1.0 - 1e-9);
    let lines = super::hlr::view_lines_with(&part_model(&part), &frame, aligned.as_ref());
    assert_eq!(
        lines.section.len(),
        2,
        "two cut faces beside the hole: {:?}",
        lines.section.len()
    );
    let total: f64 = lines.section.iter().map(|l| area(l)).sum();
    close(total, 2.0 * 15.0 * 10.0, 0.5, "cut area");
    // Aligned: the parent's middle (y = 0) is the view's anchor along y.
    close(lines.center()[1], 0.0, 1e-6, "aligned along the line");
    let h = super::render::hatch(&lines.section, std::f64::consts::FRAC_PI_4, 3.0);
    assert!(h.len() > 5, "hatched");
    // The hole's walls are visible lines in the section (seen through the
    // cut), not hidden.
    assert!(lines.visible.len() > 4);
    // Moving the parent moves the section along the line only.
    d.move_view(b, [150.0, 170.0]);
    assert_eq!(d.sheet().unwrap().view(s).unwrap().position, [250.0, 170.0]);
    // The section line on the parent and the hatching are in the list.
    let ls = Arc::new(lines);
    let base_lines = Arc::new(lines_of(&part, ViewOrientation::Top));
    let list = super::render::sheet_list(
        &d,
        0,
        &|_| String::new(),
        &|id| {
            Some(if id == s {
                ls.clone()
            } else {
                base_lines.clone()
            })
        },
        &Default::default(),
    );
    assert!(
        list.iter()
            .any(|p| matches!(p, super::sheet::Prim::Center { .. })),
        "section line"
    );
    assert!(
        list.iter()
            .any(|p| matches!(p, super::sheet::Prim::Text { text, .. } if text == "SECTION A-A"))
    );
}

/// Detail and auxiliary views: the detail clips its parent's lines to the
/// fence and centres on it; an auxiliary view of a horizontal edge is the
/// view from above (as a projected one).
#[test]
fn detail_and_auxiliary_views() {
    let part = plate_with_hole();
    let mut d = DrawingDocument::default();
    let b = d
        .add_base_view(
            model(),
            ViewOrientation::Top,
            [150.0, 150.0],
            1.0,
            ViewStyle::HiddenRemoved,
        )
        .unwrap();
    let det = d
        .add_detail_view(b, [5.0, 0.0], 4.0, [300.0, 100.0], 4.0)
        .unwrap();
    let sheet = d.sheet().unwrap().clone();
    assert_eq!(sheet.view(det).unwrap().identifier, "A");
    assert_eq!(d.effective(&sheet, det).unwrap().0, 4.0, "own scale");
    let top = lines_of(&part, ViewOrientation::Top);
    let clipped = super::hlr::clip_circle(&top, [5.0, 0.0], 4.0);
    assert_eq!(clipped.center(), [5.0, 0.0]);
    assert!(!clipped.visible.is_empty(), "the hole's rim is inside");
    // A line crossing the fence with both ends outside is cut, not lost.
    let crossing = super::hlr::ViewLines {
        visible: vec![vec![[-50.0, 0.0], [50.0, 0.0]]],
        visible_src: vec![None],
        ..Default::default()
    };
    let c = super::hlr::clip_circle(&crossing, [0.0, 0.0], 5.0);
    assert_eq!(c.visible.len(), 1);
    let l = &c.visible[0];
    close(l[0][0], -5.0, 1e-6, "enters at the fence");
    close(l[l.len() - 1][0], 5.0, 1e-6, "leaves at the fence");
    for l in &clipped.visible {
        for q in l {
            assert!((q[0] - 5.0).hypot(q[1]) <= 4.0 + 1e-6);
        }
    }
    // Auxiliary of the front view's top edge (horizontal) = the top view.
    let mut d = DrawingDocument::default();
    let f = d
        .add_base_view(
            model(),
            ViewOrientation::Front,
            [150.0, 150.0],
            1.0,
            ViewStyle::HiddenRemoved,
        )
        .unwrap();
    let aux = d
        .add_auxiliary_view(f, [-30.0, 10.0], [30.0, 10.0], [150.0, 60.0])
        .unwrap();
    let p = d.add_projected_view(f, [150.0, 60.0]).unwrap();
    let sheet = d.sheet().unwrap().clone();
    let fa = d.frame_of(&sheet, aux).unwrap();
    let fp = d.frame_of(&sheet, p).unwrap();
    vclose(fa.toward, fp.toward, "aux = projected below");
    vclose(fa.up, fp.up, "same up");
    assert_eq!(
        d.sheet().unwrap().view(aux).unwrap().position,
        [150.0, 60.0]
    );
    // Deleting the parent takes the children and their annotations.
    let gone = d.delete_view(f);
    assert_eq!(gone.len(), 3);
}

/// Hole notes take their text from the edge's hole (so they follow the Hole
/// feature); a circle without a hole orphans the note. Leader texts draw an
/// arrow at their tip and stack their lines above the shoulder.
#[test]
fn hole_notes_and_leader_texts() {
    use super::annot::{Placement, resolve};
    use super::hlr::{ViewEdge, ViewLines};
    let mut lines = ViewLines {
        edges: vec![ViewEdge {
            name: Some("Hole1:Bore:1".into()),
            a: [5.0, 0.0],
            b: [5.0, 0.0],
            circle: Some(([0.0, 0.0], 5.0)),
            straight: false,
            visible: true,
            note: Some("Ø10 THRU\nM12x1.75 - 6H".into()),
            hole: Some("Hole1:1".into()),
            full_circle: true,
        }],
        min: [-30.0, -20.0],
        max: [30.0, 20.0],
        ..Default::default()
    };
    let pl = Placement {
        position: [100.0, 100.0],
        scale: 1.0,
        center: [0.0, 0.0],
    };
    let note = AnnotationKind::HoleNote {
        view: 1,
        edge: "Hole1:Bore:1".into(),
        text: [20.0, 15.0],
    };
    let s = {
        let view = |v: u32| (v == 1).then_some((pl, &lines));
        resolve(&note, &view).unwrap()
    };
    let texts: Vec<&str> = s
        .prims
        .iter()
        .filter_map(|p| match p {
            Prim::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        texts,
        vec!["M12x1.75 - 6H", "Ø10 THRU"],
        "bottom line first"
    );
    assert!(
        s.prims.iter().any(|p| matches!(p, Prim::Fill { .. })),
        "arrow at the rim"
    );
    lines.edges[0].note = None;
    let view = |v: u32| (v == 1).then_some((pl, &lines));
    assert!(matches!(
        resolve(&note, &view),
        Err(super::annot::Orphan::Missing(_))
    ));
    let lt = resolve(
        &AnnotationKind::LeaderText {
            tip: [50.0, 50.0],
            at: [70.0, 60.0],
            text: "WELD ALL ROUND".into(),
        },
        &view,
    )
    .unwrap();
    assert!(
        lt.prims
            .iter()
            .any(|p| matches!(p, Prim::Fill { pts } if pts[0] == [50.0, 50.0]))
    );
}

/// Styles drive the drawing: a dimension style with a comma, three decimals
/// kept, a prefix and open arrows changes the text and the arrows; new /
/// rename / delete keep the references; styles in use cannot be deleted.
#[test]
fn styles_drive_annotations() {
    use super::annot::{Placement, resolve_with};
    use super::style::{DrawingStyles, StyleType, Terminator};
    let boxed = primitive(
        PrimitiveKind::Box,
        Point2::new(0.0, 0.0),
        Point2::new(20.0, 10.0),
        10.0,
    );
    let front = lines_of(&boxed, ViewOrientation::Front);
    let e = front
        .edges
        .iter()
        .find(|e| e.straight && e.visible && ((e.b[0] - e.a[0]).abs() - 40.0).abs() < 1e-6)
        .expect("a 40 mm edge");
    let name = e.name.clone().unwrap_or_default();
    let pl = Placement {
        position: [100.0, 100.0],
        scale: 1.0,
        center: front.center(),
    };
    let view = |v: u32| (v == 1).then_some((pl, &front));
    let dim = AnnotationKind::Dimension(Dimension {
        dim: DimKind::Horizontal,
        a: PointRef {
            view: 1,
            edge: name.clone(),
            role: PointRole::Start,
        },
        b: Some(PointRef {
            view: 1,
            edge: name,
            role: PointRole::End,
        }),
        text: [0.0, 20.0],
        precision: 2,
        format: Default::default(),
    });
    let mut st = DrawingStyles::default();
    let texts = |st: &DrawingStyles| -> Vec<String> {
        resolve_with(&dim, &view, &st.look())
            .unwrap()
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        texts(&st),
        vec!["40,00".to_string()],
        "Inventor's ISO: comma, 2 decimals"
    );
    let d = &mut st.dimension[0];
    d.decimal_comma = true;
    d.trailing_zeros = true;
    d.precision = 3;
    d.prefix = "(".into();
    d.suffix = ")".into();
    d.terminator = Terminator::Open;
    assert_eq!(texts(&st), vec!["(40,000)".to_string()]);
    let s = resolve_with(&dim, &view, &st.look()).unwrap();
    assert!(
        !s.prims.iter().any(|p| matches!(p, Prim::Fill { .. })),
        "open arrows"
    );
    // New / rename / delete.
    let name = st.dimension[0].name.clone();
    let copy = st.duplicate(StyleType::Dimension, &name).unwrap();
    assert_eq!(st.dimension.len(), 2);
    assert!(
        st.delete(StyleType::Dimension, &name).is_err(),
        "in use by Object Defaults"
    );
    st.defaults.dimension = copy.clone();
    st.rename(StyleType::Dimension, &copy, "Shop").unwrap();
    assert_eq!(st.defaults.dimension, "Shop");
    st.delete(StyleType::Dimension, &name).unwrap();
    assert!(
        st.delete(StyleType::Dimension, "Shop").is_err(),
        "the last one stays"
    );
    // Renaming a text style follows into the dimension style that uses it.
    let t = st.text[0].name.clone();
    st.rename(StyleType::Text, &t, "Big").unwrap();
    assert_eq!(st.dimension[0].text_style, "Big");
    assert_eq!(st.look().dim_text.name, "Big");
    // ANSI: horizontal text, a decimal point.
    let ansi = DrawingStyles::of(StandardKind::Ansi);
    assert!(!ansi.dimension[0].decimal_comma);
    // Old drawings without styles read the ISO ones.
    let old: DrawingDocument = serde_json::from_str(
        r#"{"sheets":[{"name":"Sheet:1","size":"A3","orientation":"Landscape"}]}"#,
    )
    .unwrap();
    assert_eq!(old.styles, DrawingStyles::default());
}

/// Our ISO and ANSI defaults keep the reference values of
/// `tests/data/drawing_defaults.json`.
#[test]
fn iso_and_ansi_defaults() {
    use super::style::{DrawingStyles, LineType, TextOrientation};
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/drawing_defaults.json"),
    )
    .unwrap();
    let r: serde_json::Value = serde_json::from_str(&text).unwrap();
    let f = |v: &serde_json::Value| v.as_f64().unwrap();
    for (kind, key) in [(StandardKind::Iso, "ISO"), (StandardKind::Ansi, "ANSI")] {
        let st = DrawingStyles::of(kind);
        let x = &r[key];
        let d = &st.dimension[0];
        let rd = &x["dimension"];
        assert_eq!(
            d.decimal_comma,
            rd["decimal_comma"].as_bool().unwrap(),
            "{key} decimal"
        );
        assert_eq!(
            f64::from(d.precision),
            f(&rd["precision"]),
            "{key} precision"
        );
        assert_eq!(d.trailing_zeros, rd["trailing_zeros"].as_bool().unwrap());
        for (ours, name) in [
            (d.arrow_size, "arrow_size"),
            (d.arrow_height, "arrow_height"),
            (d.extension, "extension"),
            (d.origin_offset, "origin_offset"),
            (d.gap, "gap"),
        ] {
            close(ours, f(&rd[name]), 1e-9, &format!("{key} dimension {name}"));
        }
        assert_eq!(
            d.orientation == TextOrientation::Aligned,
            rd["aligned_text"].as_bool().unwrap(),
            "{key} text orientation"
        );
        close(
            st.text[0].height,
            f(&x["note_text"]["height"]),
            1e-9,
            "note text height",
        );
        let c = &st.center_mark[0];
        let rc = &x["center_mark"];
        close(c.mark, f(&rc["mark"]), 1e-9, "centre mark");
        close(c.gap, f(&rc["gap"]), 1e-9, "centre gap");
        close(c.overshoot, f(&rc["overshoot"]), 1e-9, "centre overshoot");
        assert_eq!(
            DrawingStandard::of(kind).projection == Projection::FirstAngle,
            x["first_angle"].as_bool().unwrap()
        );
    }
    let iso = DrawingStyles::of(StandardKind::Iso);
    let l = &r["ISO"]["layers"];
    for (name, key) in [
        ("Visible (ISO)", "visible"),
        ("Hidden (ISO)", "hidden"),
        ("Center Mark (ISO)", "center_mark"),
        ("Dimension (ISO)", "dimension"),
        ("Hatch (ISO)", "hatch"),
        ("Section Line (ISO)", "section_line"),
        ("Border (ISO)", "border"),
        ("Title (ISO)", "title"),
    ] {
        close(iso.layer(name).weight, f(&l[key]), 1e-9, name);
    }
    assert_eq!(
        iso.layer("Center Mark (ISO)").line_type,
        LineType::Continuous
    );
    // Border and title block.
    let b = &r["ISO"]["border"];
    let d = DrawingDocument::default();
    let (lo, _) = super::sheet::frame(&d.sheets[0]);
    close(lo[0], f(&b["margin"]), 1e-9, "A3 margin");
    assert_eq!(super::sheet::ZONES.0 as f64, f(&b["zones"][0]));
    assert_eq!(super::sheet::ZONES.1 as f64, f(&b["zones"][1]));
    close(
        super::sheet::TITLE_W,
        f(&r["ISO"]["title_block"]["width"]),
        1e-9,
        "title block width",
    );
    close(
        super::sheet::TITLE_H,
        f(&r["ISO"]["title_block"]["height"]),
        1e-9,
        "title block height",
    );
    // The same diameter text as Inventor's (Ø11,00).
    let look = iso.look();
    assert_eq!(
        format!("Ø{}", {
            let mut t = format!("{:.*}", usize::from(look.dim.precision), 11.0);
            if look.dim.decimal_comma {
                t = t.replace('.', ",");
            }
            t
        }),
        r["ISO"]["dimension"]["sample"].as_str().unwrap()
    );
}

/// An assembly view (a plate and three blocks): the parts list lists the
/// BOM, a balloon on a block shows the block's item number, and renumbering
/// the parts list by part number renumbers the balloon too.
#[test]
fn assembly_parts_list_and_balloons() {
    use super::annot::{Placement, resolve_in};
    use super::parts::{Balloon, PartsList, SortBy, Tables};
    use crate::assembly::bom::{self, ComponentInfo};
    use crate::document::AssemblyDocument;
    use crate::kernel::Transform3;
    let mut project = Project::new("Asm");
    let plate = project.add_document(
        "Plate",
        DocumentData::Part(PartDocument::with_box("Box1", [80.0, 50.0, 12.0]).unwrap()),
    );
    let block = project.add_document(
        "Block",
        DocumentData::Part(PartDocument::with_box("Box1", [10.0, 10.0, 30.0]).unwrap()),
    );
    let mut asm = AssemblyDocument::default();
    asm.place(DocRef::local(plate), "Plate", Transform3::identity(), true);
    for k in 0..3 {
        let t = Transform3::translation(10.0 + 25.0 * f64::from(k), 20.0, 12.0);
        asm.place(DocRef::local(block), "Block", t, false);
    }
    let a = project.add_document("Assembly1", DocumentData::Assembly(asm.clone()));
    let model_of = |id: DocumentId| match &project.document(id).unwrap().data {
        DocumentData::Part(p) => Arc::new(part_model(p)),
        _ => panic!("a part"),
    };
    let (mp, mb) = (model_of(plate), model_of(block));
    let instances: Vec<hlr::Instance> = asm
        .occurrences
        .iter()
        .map(|o| hlr::Instance {
            id: o.id,
            transform: o.transform,
            model: if o.component.document == plate {
                mp.clone()
            } else {
                mb.clone()
            },
        })
        .collect();
    let vm = hlr::assembly_model(&instances, hlr::Simplify::default());
    let lines = view_lines(&vm, &ViewOrientation::Front.frame());
    let mut d = DrawingDocument::default();
    let v = d
        .add_base_view(
            DocRef::local(a),
            ViewOrientation::Front,
            [120.0, 180.0],
            1.0,
            ViewStyle::HiddenShown,
        )
        .unwrap();
    let block_occ = asm.occurrences[1].id;
    let edge = lines
        .edges
        .iter()
        .filter_map(|e| e.name.clone())
        .find(|n| hlr::occurrence_of(n) == Some(block_occ))
        .expect("an edge of the first block");
    d.add_annotation(AnnotationKind::PartsList(PartsList::new(v, [405.0, 287.0])));
    let balloon = AnnotationKind::Balloon(Balloon {
        view: v,
        edge,
        along: 0.5,
        at: [-50.0, 40.0],
        shape: Default::default(),
        bom: Default::default(),
        text: None,
    });
    d.add_annotation(balloon.clone());
    let info = |r: DocRef| ComponentInfo {
        file_name: project.document(r.document).unwrap().name.clone(),
        ..Default::default()
    };
    let rows = |_: DocRef, k: super::parts::BomKind, merge: bool| {
        Some(bom::rows(&asm, k.view(), merge, &info))
    };
    let texts = |prims: &[Prim]| -> Vec<String> {
        prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    };
    let pl = Placement {
        position: [120.0, 180.0],
        scale: 1.0,
        center: lines.center(),
    };
    let view = |id: u32| (id == v).then_some((pl, &lines));
    let look = d.styles.look();
    let tables = Tables::build(&d, 0, &rows);
    let list = resolve_in(&d.sheets[0].annotations[0].kind, &view, &look, &tables).unwrap();
    let t = texts(&list.prims);
    assert!(
        t.contains(&"Plate".to_owned()) && t.contains(&"Block".to_owned()),
        "{t:?}"
    );
    assert!(t.contains(&"3".to_owned()), "three blocks: {t:?}");
    let b = resolve_in(&balloon, &view, &look, &tables).unwrap();
    assert_eq!(texts(&b.prims), ["2"]);
    // Sorted by part number and renumbered: Block becomes item 1.
    if let AnnotationKind::PartsList(p) = &mut d.sheets[0].annotations[0].kind {
        p.sort = Some(SortBy {
            column: "part_number".into(),
            ascending: true,
            renumber: true,
        });
    }
    let tables = Tables::build(&d, 0, &rows);
    let b = resolve_in(&balloon, &view, &look, &tables).unwrap();
    assert_eq!(texts(&b.prims), ["1"]);
    // A missing edge orphans the balloon.
    let mut gone = balloon.clone();
    if let AnnotationKind::Balloon(b) = &mut gone {
        b.edge = "o99/none".into();
    }
    assert!(resolve_in(&gone, &view, &look, &tables).is_err());
}

/// A 100 × 60 × 2 sheet with a 25 mm flange.
fn sheet_part() -> PartDocument {
    use crate::part::SketchOptions;
    let mut p = PartDocument::default();
    p.make_sheet_metal(None);
    let o = SketchOptions {
        auto_project_origin: false,
        ..Default::default()
    };
    let i = p
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
        .unwrap();
    p.sketches[i]
        .sketch
        .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(100.0, 60.0))
        .unwrap();
    let r = p.sketches[i].sketch.regions();
    let prof = vec![crate::part::ProfileRef::of(&r[0], r[0].interior_point())];
    let face = p.add_sheet_face(i, prof, false).unwrap();
    let (body, edge) = p
        .sheet_edge_at(crate::kernel::Point3::new(50.0, 0.0, 2.0))
        .unwrap();
    assert_eq!(body, face);
    let fl = crate::part::sheet_metal::SheetFlangeFeature::new(&body, vec![edge], 25.0);
    p.add_sheet_flange(fl).unwrap();
    p
}

/// A flat pattern view: the flat's outline seen on its top face, one bend
/// with its centre line and extents across the flat, and its bend note in
/// the dimension style (`UP 90,00° R2,00`).
#[test]
fn flat_pattern_view_and_bend_note() {
    use super::annot::{Placement, bend_note_text, resolve};
    let part = sheet_part();
    let flats = part.flat_patterns();
    let (a, b) = flats[0].size;
    let lines = hlr::flat_lines(&part, &ViewOrientation::Top.frame(), None).expect("a flat");
    let (w, h) = lines.size();
    let (long, short) = (w.max(h), w.min(h));
    close(long, a.max(b), 0.05, "flat length");
    close(short, a.min(b), 0.05, "flat width");
    assert_eq!(lines.bends.len(), 1);
    let bend = &lines.bends[0];
    let len = (bend.centre[1][0] - bend.centre[0][0]).hypot(bend.centre[1][1] - bend.centre[0][1]);
    close(len, 100.0, 0.05, "bend line length");
    close(bend.angle, 90.0, 1e-6, "bend angle");
    // The zone is the bend allowance wide, around the centre line.
    let z = &bend.zone;
    let width = (z[0][0][0] - z[1][0][0]).hypot(z[0][0][1] - z[1][0][1]);
    let ba = std::f64::consts::FRAC_PI_2 * (2.0 + 0.44 * 2.0);
    close(width, ba, 0.01, "bend zone");
    let st = super::style::DrawingStyles::default();
    let text = bend_note_text(bend, &st.dimension[0]);
    assert!(text.ends_with(" 90,00° R2,00"), "{text}");
    assert!(text.starts_with("UP") || text.starts_with("DOWN"));
    // The note annotation resolves on the bend, and is orphaned without it.
    let pl = Placement {
        position: [200.0, 150.0],
        scale: 1.0,
        center: lines.center(),
    };
    let view = |id: u32| (id == 1).then_some((pl, &lines));
    let note = AnnotationKind::BendNote {
        view: 1,
        bend: bend.middle(),
        offset: [0.0, 0.0],
    };
    let s = resolve(&note, &view).unwrap();
    assert!(matches!(&s.prims[0], Prim::Text { text: t, .. } if *t == text));
    let far = AnnotationKind::BendNote {
        view: 1,
        bend: [bend.middle()[0] + 500.0, 0.0],
        offset: [0.0, 0.0],
    };
    assert!(resolve(&far, &view).is_err());
    // Dragged a little along the line it stays on the line; dragged off
    // the bend it gets a leader whose tip lies on the centre line.
    let along = AnnotationKind::BendNote {
        view: 1,
        bend: bend.middle(),
        offset: [1.0, 1.0],
    };
    assert_eq!(resolve(&along, &view).unwrap().prims.len(), 1);
    let dragged = AnnotationKind::BendNote {
        view: 1,
        bend: bend.middle(),
        offset: [30.0, 40.0],
    };
    let s = resolve(&dragged, &view).unwrap();
    assert!(s.prims.len() > 2, "{:?}", s.prims);
    assert!(
        s.prims
            .iter()
            .any(|p| matches!(p, Prim::Text { angle, .. } if angle.abs() < 1e-12))
    );
    let (a, c) = (pl.to_sheet(bend.centre[0]), pl.to_sheet(bend.centre[1]));
    let on_line = |q: [f64; 2]| {
        let (d, e) = ([c[0] - a[0], c[1] - a[1]], [q[0] - a[0], q[1] - a[1]]);
        (d[0] * e[1] - d[1] * e[0]).abs() / d[0].hypot(d[1]) < 1e-6
    };
    assert!(
        s.prims
            .iter()
            .any(|p| matches!(p, Prim::Line { a, .. } if on_line(*a))),
        "{:?}",
        s.prims
    );
    // The bend layers draw the lines: chain centre line, two extents.
    let mut d = DrawingDocument::default();
    let v = d
        .add_base_view(
            model(),
            ViewOrientation::Top,
            [200.0, 150.0],
            1.0,
            ViewStyle::HiddenShown,
        )
        .unwrap();
    d.sheets[0].views[0].sheet_metal = SheetView::Flat;
    assert!(d.flat_of(&d.sheets[0], v));
    let extras = super::render::view_extras(&d, 0, &|id| (id == v).then_some((pl, &lines)));
    assert_eq!(
        extras
            .iter()
            .filter(|p| matches!(p, Prim::Center { chain: true, .. }))
            .count(),
        1
    );
    // Each kind of bend line can be switched off per view.
    let paths = |d: &DrawingDocument| {
        super::render::view_extras(d, 0, &|id| (id == v).then_some((pl, &lines)))
            .iter()
            .fold((0, 0), |(c, p), x| match x {
                Prim::Center { .. } => (c + 1, p),
                Prim::Path { .. } => (c, p + 1),
                _ => (c, p),
            })
    };
    let (c0, p0) = paths(&d);
    d.sheets[0].views[0].display.bend_extents = false;
    assert_eq!(paths(&d), (c0, p0 - 2));
    d.sheets[0].views[0].display.bend_centerlines = false;
    assert_eq!(paths(&d), (c0 - 1, p0 - 2));
}

/// Baseline, chain and ordinate sets over the box's front view: points of
/// its edges measured along x and y; repeated coordinates count once.
#[test]
fn dimension_sets_measure_from_the_origin() {
    use super::annot::{Placement, resolve, set_members};
    use super::dimset::{DimSet, SetDir, SetKind};
    use super::sheet::Prim;
    let boxed = primitive(
        PrimitiveKind::Box,
        Point2::new(0.0, 0.0),
        Point2::new(20.0, 10.0),
        10.0,
    );
    let front = lines_of(&boxed, ViewOrientation::Front);
    let pl = Placement {
        position: [100.0, 100.0],
        scale: 1.0,
        center: front.center(),
    };
    let view = |v: u32| (v == 1).then_some((pl, &front));
    // Every end point of the visible straight edges.
    let points: Vec<PointRef> = front
        .edges
        .iter()
        .filter(|e| e.straight && e.visible && e.name.is_some())
        .flat_map(|e| {
            [PointRole::Start, PointRole::End].map(|role| PointRef {
                view: 1,
                edge: e.name.clone().unwrap_or_default(),
                role,
            })
        })
        .collect();
    assert!(points.len() >= 4);
    let set = |kind, dir, at| DimSet {
        kind,
        dir,
        points: points.clone(),
        origin: 0,
        at,
    };
    let texts = |s: &DimSet| -> Vec<String> {
        resolve(&AnnotationKind::DimSet(s.clone()), &view)
            .unwrap()
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    };
    // Two x coordinates (40 apart), two y coordinates (10 apart).
    let b = set(SetKind::Baseline, SetDir::Horizontal, [0.0, 30.0]);
    assert_eq!(set_members(&b, &view, 6.0).unwrap().len(), 1);
    assert_eq!(texts(&b), ["40,00"]);
    let c = set(SetKind::Chain, SetDir::Vertical, [40.0, 0.0]);
    assert_eq!(texts(&c), ["10,00"]);
    let mut o = set(SetKind::Ordinate, SetDir::Horizontal, [0.0, 30.0]);
    let mut t = texts(&o);
    t.sort();
    assert_eq!(t, ["0,00", "40,00"]);
    assert!(set_members(&o, &view, 6.0).unwrap().is_empty());
    // A missing origin orphans the set.
    o.points[0].edge = "gone".into();
    assert!(resolve(&AnnotationKind::DimSet(o), &view).is_err());
    // Save / load.
    let a = AnnotationKind::DimSet(b);
    let back: AnnotationKind = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
    assert_eq!(back, a);
}

/// Arrange Dimensions over the box's front view: horizontal dimensions above
/// it go into rows Part Offset outside the view, Spacing apart, shortest
/// first, non-overlapping ones sharing a row; the text keeps its place
/// along the line.
#[test]
fn arrange_puts_dimensions_in_rows() {
    use super::annot::{Placement, dim_points};
    use super::arrange::arrange;
    let boxed = primitive(
        PrimitiveKind::Box,
        Point2::new(0.0, 0.0),
        Point2::new(20.0, 10.0),
        10.0,
    );
    let front = lines_of(&boxed, ViewOrientation::Front);
    let pl = Placement {
        position: [100.0, 100.0],
        scale: 1.0,
        center: front.center(),
    };
    let view = |v: u32| (v == 1).then_some((pl, &front));
    let e = front
        .edges
        .iter()
        .find(|e| e.straight && e.visible && ((e.b[0] - e.a[0]).abs() - 40.0).abs() < 1e-6)
        .expect("a 40 mm edge");
    let name = e.name.clone().unwrap_or_default();
    let p = |role| PointRef {
        view: 1,
        edge: name.clone(),
        role,
    };
    let dim = |text: [f64; 2]| Dimension {
        dim: DimKind::Horizontal,
        a: p(PointRole::Start),
        b: Some(p(PointRole::End)),
        text,
        precision: 2,
        format: Default::default(),
    };
    // Two overlapping 40 mm dimensions placed untidily above the view.
    let (d1, d2) = (dim([3.0, 40.0]), dim([-2.0, 13.0]));
    assert!(dim_points(&d1, &view).is_some());
    let top = pl.to_sheet(front.max)[1].max(pl.to_sheet(front.min)[1]);
    let r = arrange(&[(1, &d1), (2, &d2)], &view, 10.0, 6.0);
    assert_eq!(r.len(), 2);
    let y = |id: u32| {
        r.iter()
            .find(|x| x.0 == id)
            .map(|x| 100.0 + x.1[1])
            .unwrap()
    };
    let mut ys = [y(1), y(2)];
    ys.sort_by(f64::total_cmp);
    close(ys[0], top + 10.0, 1e-9, "first row at the part offset");
    close(ys[1], top + 16.0, 1e-9, "second row one spacing further");
    // The text keeps its place along the line.
    close(
        r.iter().find(|x| x.0 == 1).unwrap().1[0],
        3.0,
        1e-12,
        "text x",
    );
    // A dimension inside the view's box is left alone.
    let inside = dim([0.0, 0.0]);
    assert!(arrange(&[(3, &inside)], &view, 10.0, 6.0).is_empty());
}

/// Chamfer notes on a real part: a 40 × 30 × 20 block with an equal 2 mm
/// chamfer and a 2 mm × 30° chamfer on two upright edges, seen from the top.
/// The values follow the reference edge as in Inventor (`2,00 X 30,00°`
/// along the face the distance was taken on, `1,15 X 60,00°` along the
/// other), and the note is orphaned when the chamfer goes.
#[test]
fn chamfer_notes_measure_from_the_reference() {
    use super::annot::{Placement, resolve};
    use super::chamfer_note::ChamferNote;
    use crate::part::blend::{BlendFeature, BlendKind, ChamferMethod, EdgeRef};
    let mut part = PartDocument::with_box("Box1", [40.0, 30.0, 20.0]).unwrap();
    part.add_blend(BlendFeature::chamfer(
        vec![EdgeRef {
            faces: ["Box1:Left".into(), "Box1:Front".into()],
            hint: [0.0, 0.0, 10.0],
        }],
        2.0,
    ))
    .unwrap();
    let mut c = BlendFeature::chamfer(
        vec![EdgeRef {
            faces: ["Box1:Front".into(), "Box1:Right".into()],
            hint: [40.0, 0.0, 10.0],
        }],
        2.0,
    );
    if let BlendKind::Chamfer { method, angle, .. } = &mut c.kind {
        *method = ChamferMethod::DistanceAngle;
        *angle = 30f64.to_radians();
    }
    part.add_blend(c).unwrap();
    let lines = lines_of(&part, ViewOrientation::Top);
    let named: Vec<&super::hlr::ViewEdge> = lines
        .edges
        .iter()
        .filter(|e| e.visible && e.straight && e.circle.is_none() && e.name.is_some())
        .collect();
    let slanted =
        |e: &super::hlr::ViewEdge| (e.b[0] - e.a[0]).abs() > 1e-6 && (e.b[1] - e.a[1]).abs() > 1e-6;
    let pl = Placement {
        position: [100.0, 100.0],
        scale: 1.0,
        center: lines.center(),
    };
    let mut texts = Vec::new();
    for ch in named.iter().filter(|e| slanted(e)) {
        for r in named.iter().filter(|e| !slanted(e)) {
            let n = ChamferNote::new(
                1,
                ch.name.as_deref().unwrap(),
                r.name.as_deref().unwrap(),
                [20.0, 20.0],
            );
            let Ok((_, v)) = n.measure(&lines) else {
                continue;
            };
            let s = resolve(&AnnotationKind::ChamferNote(n), &|id: u32| {
                (id == 1).then_some((pl, &lines))
            })
            .unwrap();
            let text = s.prims.iter().find_map(|p| match p {
                Prim::Text { text, .. } => Some(text.clone()),
                _ => None,
            });
            assert!((s.value.unwrap() - v.distance1).abs() < 1e-9);
            texts.push(text.unwrap());
        }
    }
    texts.sort();
    texts.dedup();
    assert_eq!(texts, ["1,15 X 60,00°", "2,00 X 30,00°", "2,00 X 45,00°"]);
    // A missing edge orphans the note.
    let n = ChamferNote::new(1, "Nope:E1", "Box1:Top", [0.0, 0.0]);
    assert!(matches!(
        resolve(&AnnotationKind::ChamferNote(n), &|id: u32| (id == 1)
            .then_some((pl, &lines))),
        Err(super::annot::Orphan::Missing(_))
    ));
}
