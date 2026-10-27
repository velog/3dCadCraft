//! Delete Face (`docs/spec/2.14-delete-face.md`): Heal brings a part back
//! to what it was before a fillet, chamfer or hole; without Heal the body
//! opens into a surface body; Lump or Void deletes a whole shell. Every
//! refusal leaves the part as it was.

mod common;
use common::*;

use cadcraft3d::kernel::Real;
use cadcraft3d::part::body_op::{DeleteFaceOp, DeleteSelect};
use cadcraft3d::part::{
    BlendFeature, BodyOp, BodyOpFeature, EdgeRef, FaceRef, PartDocument, ShellFeature,
};

fn solid_volume(p: &PartDocument) -> Real {
    p.exact_bodies
        .0
        .iter()
        .filter(|s| !s.is_open())
        .filter_map(|s| s.volume_at(1e-3))
        .sum()
}

fn surfaces(p: &PartDocument) -> usize {
    p.exact_bodies.0.iter().filter(|s| s.is_open()).count()
}

fn solids(p: &PartDocument) -> usize {
    p.exact_bodies.0.iter().filter(|s| !s.is_open()).count()
}

fn near(v: Real, want: Real, what: &str) {
    assert!(
        (v - want).abs() <= 1e-4 * want.abs().max(1.0),
        "{what}: {v:.4} vs {want:.4}"
    );
}

fn healthy(p: &PartDocument) {
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    for (s, sol) in p.body.solids() {
        if !sol.open {
            p.body.validate_solid(s).unwrap();
        }
    }
}

/// The faces of the part whose name starts with `prefix`, one reference each
/// (a point inside the face: its first triangle's centre).
fn faces_of(p: &PartDocument, prefix: &str) -> Vec<FaceRef> {
    p.body
        .faces()
        .filter(|(_, f)| f.name.as_deref().is_some_and(|n| n.starts_with(prefix)))
        .filter_map(|(id, _)| cadcraft3d::part::shell::face_ref(&p.body, id, None))
        .collect()
}

fn delete(
    p: &mut PartDocument,
    faces: Vec<FaceRef>,
    select: DeleteSelect,
    heal: bool,
) -> Result<String, String> {
    let mut d = DeleteFaceOp::new();
    d.faces = faces;
    d.select = select;
    d.heal = heal;
    p.add_body_op(BodyOpFeature::new(BodyOp::DeleteFace(d)))
        .map_err(|e| e.to_string())
}

const BOX: [Real; 3] = [80.0, 60.0, 20.0];

fn plain_box() -> PartDocument {
    PartDocument::with_box("Box1", BOX).unwrap()
}

fn top_front() -> EdgeRef {
    EdgeRef {
        faces: ["Box1:Top".into(), "Box1:Front".into()],
        hint: [40.0, 0.0, 20.0],
    }
}

#[test]
fn heal_takes_a_fillet_or_a_chamfer_off() {
    let whole = BOX[0] * BOX[1] * BOX[2];
    for (blend, prefix) in [
        (BlendFeature::fillet(vec![top_front()], 5.0), "Fillet1"),
        (BlendFeature::chamfer(vec![top_front()], 4.0), "Chamfer1"),
    ] {
        let mut p = plain_box();
        p.add_blend(blend).unwrap();
        assert!(solid_volume(&p) < whole - 1.0);
        let faces = faces_of(&p, prefix);
        assert!(!faces.is_empty(), "{prefix}: no faces");
        let id = delete(&mut p, faces, DeleteSelect::Faces, true).unwrap();
        assert_eq!(id, "DeleteFace1");
        healthy(&p);
        near(solid_volume(&p), whole, prefix);
        assert_eq!(p.body.faces().count(), 6, "{prefix}: a box again");
        // The healed neighbours keep their names.
        for n in ["Box1:Top", "Box1:Front"] {
            assert!(
                p.body.faces().any(|(_, f)| f.name.as_deref() == Some(n)),
                "{prefix}: {n} lost"
            );
        }
    }
}

#[test]
fn without_heal_the_body_opens() {
    let mut p = plain_box();
    let top = FaceRef {
        face: "Box1:Top".into(),
        hint: [40.0, 30.0, 20.0],
    };
    delete(&mut p, vec![top], DeleteSelect::Faces, false).unwrap();
    healthy(&p);
    assert_eq!(solids(&p), 0);
    assert_eq!(surfaces(&p), 1);
    assert_eq!(p.body.faces().count(), 5);
    assert!(
        !p.body
            .faces()
            .any(|(_, f)| f.name.as_deref() == Some("Box1:Top"))
    );
}

#[test]
fn a_void_fills_with_material() {
    // A closed shell (no face removed) leaves a void inside the box.
    let whole = BOX[0] * BOX[1] * BOX[2];
    let mut p = plain_box();
    p.add_shell(ShellFeature::new(2.0)).unwrap();
    let hollow = solid_volume(&p);
    let inner = (BOX[0] - 4.0) * (BOX[1] - 4.0) * (BOX[2] - 4.0);
    near(hollow, whole - inner, "hollow box");
    let void = faces_of(&p, "Shell1");
    assert!(!void.is_empty(), "no void faces");
    delete(
        &mut p,
        vec![void[0].clone()],
        DeleteSelect::LumpOrVoid,
        false,
    )
    .unwrap();
    healthy(&p);
    near(solid_volume(&p), whole, "void deleted");
}

#[test]
fn refusals_change_nothing() {
    let mut p = plain_box();
    let top = FaceRef {
        face: "Box1:Top".into(),
        hint: [40.0, 30.0, 20.0],
    };
    let cases: [(Vec<FaceRef>, DeleteSelect, bool, &str); 3] = [
        // A body of one lump and no void.
        (
            vec![top.clone()],
            DeleteSelect::LumpOrVoid,
            false,
            "one lump",
        ),
        // Nothing picked.
        (Vec::new(), DeleteSelect::Faces, true, "select the faces"),
        // A face that is not there.
        (
            vec![FaceRef {
                face: "Box9:Top".into(),
                hint: [0.0; 3],
            }],
            DeleteSelect::Faces,
            true,
            "no longer exists",
        ),
    ];
    for (faces, select, heal, text) in cases {
        let before = p.features.len();
        let e = delete(&mut p, faces, select, heal).unwrap_err();
        assert!(e.contains(text), "{e}");
        assert_eq!(p.features.len(), before);
        near(solid_volume(&p), BOX[0] * BOX[1] * BOX[2], text);
    }
    // Heal with a curved neighbour: a fillet's neighbour is the round.
    let mut q = plain_box();
    q.add_blend(BlendFeature::fillet(vec![top_front()], 5.0))
        .unwrap();
    let before = q.features.len();
    let v = solid_volume(&q);
    let e = delete(&mut q, vec![top], DeleteSelect::Faces, true).unwrap_err();
    assert!(e.contains("curved"), "{e}");
    assert_eq!(q.features.len(), before);
    near(solid_volume(&q), v, "unchanged after a refusal");
}

#[test]
fn save_and_load_keep_it() {
    let mut p = plain_box();
    p.add_blend(BlendFeature::fillet(vec![top_front()], 5.0))
        .unwrap();
    let faces = faces_of(&p, "Fillet1");
    delete(&mut p, faces, DeleteSelect::Faces, true).unwrap();
    let text = serde_json::to_string(&p.features).unwrap();
    assert!(text.contains("\"DeleteFace\""), "{text}");
    let back: Vec<cadcraft3d::part::Feature> = serde_json::from_str(&text).unwrap();
    assert_eq!(back, p.features);
}

/// Whether region `r` of sketch `j` lies in the model's box x0..x1 × y0..y1
/// (its interior point; the sketch also holds the plate's projected outline).
fn inside(
    p: &PartDocument,
    j: usize,
    x: (Real, Real),
    y: (Real, Real),
) -> impl Fn(&cadcraft3d::sketch::Region) -> bool + use<> {
    let plane = p.sketches[j].sketch.plane.clone();
    move |r| {
        let ip = r.interior_point();
        let q = plane.point_at(ip.x, ip.y);
        q.x > x.0 && q.x < x.1 && q.y > y.0 && q.y < y.1
    }
}

/// A plate 80 × 60 × 20 (Extrusion1).
fn base() -> PartDocument {
    plate(80.0, 60.0, 20.0)
}

fn top_name(p: &PartDocument) -> String {
    top(p, 20.0)
}

#[test]
fn heal_takes_a_hole_a_pocket_or_a_boss_off() {
    let whole = 80.0 * 60.0 * 20.0;
    // A hole through all, a pocket 5 deep, a boss 10 high.
    type Make = fn(&mut PartDocument, usize);
    let cases: [(&str, Make); 3] = [
        ("hole", |p, j| {
            let pick = inside(p, j, (22.0, 38.0), (22.0, 38.0));
            cut(p, j, None, pick)
        }),
        ("pocket", |p, j| {
            let pick = inside(p, j, (20.0, 50.0), (20.0, 40.0));
            cut(p, j, Some(5.0), pick)
        }),
        ("boss", |p, j| {
            let pick = inside(p, j, (20.0, 50.0), (20.0, 40.0));
            boss(p, j, 10.0, pick)
        }),
    ];
    for (what, make) in cases {
        let mut p = base();
        let t = top_name(&p);
        let j = sketch_on(&mut p, &t, |s, at| {
            if what == "hole" {
                s.add_circle(at(30.0, 30.0, 20.0), 8.0).unwrap();
            } else {
                s.add_rectangle_two_point(at(20.0, 20.0, 20.0), at(50.0, 40.0, 20.0))
                    .unwrap();
            }
        });
        make(&mut p, j);
        let v = solid_volume(&p);
        assert!(
            (v - whole).abs() > 1.0,
            "{what}: the feature changed nothing"
        );
        let faces = faces_of(&p, "Extrusion2");
        assert!(!faces.is_empty(), "{what}: no faces of the feature");
        delete(&mut p, faces, DeleteSelect::Faces, true).unwrap_or_else(|e| panic!("{what}: {e}"));
        healthy(&p);
        near(solid_volume(&p), whole, what);
        assert_eq!(p.body.faces().count(), 6, "{what}: a plate again");
    }
}

#[test]
fn faces_of_a_surface_body_go_one_after_another() {
    // The box opened (no Heal), then a side of the surface deleted too.
    let mut p = plain_box();
    let top = FaceRef {
        face: "Box1:Top".into(),
        hint: [40.0, 30.0, 20.0],
    };
    delete(&mut p, vec![top], DeleteSelect::Faces, false).unwrap();
    let front = FaceRef {
        face: "Box1:Front".into(),
        hint: [40.0, 0.0, 10.0],
    };
    delete(&mut p, vec![front], DeleteSelect::Faces, false).unwrap();
    healthy(&p);
    assert_eq!((solids(&p), surfaces(&p)), (0, 1));
    assert_eq!(p.body.faces().count(), 4);
}

#[test]
fn a_lump_of_a_body_goes() {
    // A slot cut across the whole plate leaves two lumps in one body.
    let mut p = base();
    let t = top_name(&p);
    let j = sketch_on(&mut p, &t, |s, at| {
        s.add_rectangle_two_point(at(30.0, -5.0, 20.0), at(40.0, 65.0, 20.0))
            .unwrap();
    });
    let pick = inside(&p, j, (30.0, 40.0), (0.0, 60.0));
    cut(&mut p, j, None, pick);
    let pieces = solid_volume(&p);
    near(pieces, 70.0 * 60.0 * 20.0, "two lumps");
    // A face of the right-hand lump (x > 40).
    let right = p
        .body
        .faces()
        .filter(|(_, f)| {
            f.mesh
                .as_ref()
                .is_some_and(|m| m.positions.iter().all(|q| q.x > 40.0 - 1e-6))
        })
        .find_map(|(id, _)| cadcraft3d::part::shell::face_ref(&p.body, id, None))
        .expect("a face of the right lump");
    delete(&mut p, vec![right], DeleteSelect::LumpOrVoid, false).unwrap();
    healthy(&p);
    near(solid_volume(&p), 30.0 * 60.0 * 20.0, "left lump");
}
