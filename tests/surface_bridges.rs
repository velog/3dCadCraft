//! Loft between surfaces and Stitch on the owner's spline surfaces (`docs/spec/5.4a-surface-workflow.md`
//! L1, L3, S2): bridges between trimmed edges, between whole boundaries (Output Surface / Solid), and
//! Stitch closing the owner's hand-made lofts. Heavier than `surface_tools` (the surfaces are
//! B-splines trimmed by a curved wall), so it runs with the medium tests.

use cadcraft3d::kernel::Real;
use cadcraft3d::part::surface_op::{StitchOp, SurfEdge};
use cadcraft3d::part::{BodyOp, BodyOpFeature, Output, PartDocument};

/// Area of the surface bodies (display triangles).
fn sheet_area(part: &PartDocument) -> Real {
    part.exact_bodies
        .0
        .iter()
        .filter(|s| s.is_open())
        .filter_map(|s| s.triangles().ok())
        .flatten()
        .map(|[a, b, c]| (b - a).cross(&(c - a)).norm() / 2.0)
        .sum()
}

fn surfaces(part: &PartDocument) -> usize {
    part.exact_bodies.0.iter().filter(|s| s.is_open()).count()
}

fn close(got: Real, want: Real, rel: Real) {
    assert!(
        (got - want).abs() <= rel * want.abs(),
        "{got} vs {want} (rel {rel})"
    );
}

fn op(p: &mut PartDocument, o: BodyOp) -> String {
    let id = p.add_body_op(BodyOpFeature::new(o)).unwrap();
    assert!(p.exact, "exact kernel");
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    id
}

fn solid_volume(p: &PartDocument) -> Real {
    p.exact_bodies
        .0
        .iter()
        .filter(|s| !s.is_open())
        .filter_map(|s| s.volume_at(1e-4))
        .sum()
}

/// The owner's test part (2026-10-29): two spline surfaces trimmed by a curved wall; a Loft
/// between their trimmed edges (which lie on the curved wall, not in a plane) bridges them.
#[test]
fn loft_bridges_the_owners_trimmed_surfaces() {
    use cadcraft3d::document::{DocumentData, Project};
    use cadcraft3d::kernel::{Point3, TessellationOptions};
    use cadcraft3d::part::{LoftFeature, LoftSection};
    let pr = Project::load(std::path::Path::new(
        "tests/data/surface_test_owner.3dcadcraft",
    ))
    .unwrap();
    let mut p = pr
        .documents()
        .iter()
        .find_map(|d| match &d.data {
            DocumentData::Part(p) => Some(p.clone()),
            _ => None,
        })
        .unwrap();
    p.regenerate().unwrap();
    let before = (surfaces(&p), sheet_area(&p));
    // The free edge of each trimmed surface nearest the wall's middle.
    let wall = p
        .exact_bodies
        .0
        .iter()
        .find(|s| s.name.as_deref() == Some("Extrusion3"))
        .unwrap();
    let tris = wall.triangles().unwrap();
    let c = Point3::from(
        tris.iter()
            .fold(nalgebra::Vector3::zeros(), |a, t| a + t[0].coords)
            / tris.len() as Real,
    );
    let opts = TessellationOptions::default();
    let free_edge = |prefix: &str| {
        p.body
            .edges()
            .filter_map(|(e, _)| {
                let faces = p.body.edge_faces(e);
                let name = p.body.face(*faces.first()?)?.name.clone()?;
                (faces.len() == 1 && name.starts_with(prefix)).then_some(())?;
                let pts = p.body.sample_edge(e, &opts).ok()?;
                let m = pts[pts.len() / 2];
                Some((name, m))
            })
            .min_by(|a, b| (a.1 - c).norm().total_cmp(&(b.1 - c).norm()))
            .map(|(face, m)| SurfEdge {
                face,
                hint: [m.x, m.y, m.z],
            })
            .unwrap()
    };
    let a = free_edge("Extrusion2:");
    let b = free_edge("Extrusion4:");
    let mut def = LoftFeature::new(vec![
        LoftSection::Edges(vec![a]),
        LoftSection::Edges(vec![b]),
    ]);
    def.output = Output::Surface;
    p.add_loft(def).unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    assert_eq!(surfaces(&p), before.0 + 1);
    // The bridge spans the gap between the surfaces (about 35 mm) along the wall's curve.
    let added = sheet_area(&p) - before.1;
    assert!(added > 1000.0, "bridge area {added}");
}

/// The whole free boundaries of two surfaces as loft sections (the owner's Surface_test_3 before
/// its hole): one bridge per pair of sides (split at the corners), all of them meshed (the one
/// along the trimmed wavy edges once had no triangles); they share the surfaces' edges, so Stitch
/// closes a solid, and Output Solid makes it in one feature.
#[test]
fn loft_between_two_whole_surfaces() {
    use cadcraft3d::document::{DocumentData, Project};
    use cadcraft3d::kernel::TessellationOptions;
    use cadcraft3d::part::{LoftFeature, LoftSection};
    let pr = Project::load(std::path::Path::new(
        "tests/data/surface_test3_owner.3dcadcraft",
    ))
    .unwrap();
    let mut p = pr
        .documents()
        .iter()
        .find_map(|d| match &d.data {
            DocumentData::Part(p) => Some(p.clone()),
            _ => None,
        })
        .unwrap();
    // Up to the second trim (before the hole and the owner's lofts).
    p.features.truncate(5);
    p.eop = None;
    p.regenerate().unwrap();
    let opts = TessellationOptions::default();
    let boundary = |p: &PartDocument, body: &str| -> Vec<SurfEdge> {
        let mut out = Vec::new();
        for f in cadcraft3d::part::select::faces_of_body(&p.body, body) {
            let face = p.body.face(f).unwrap();
            for lp in std::iter::once(&face.outer).chain(&face.inner) {
                for ce in &lp.coedges {
                    if p.body.edge_faces(ce.edge).len() != 1 {
                        continue;
                    }
                    let pts = p.body.sample_edge(ce.edge, &opts).unwrap();
                    let m = cadcraft3d::part::select::polyline_middle(&pts).unwrap();
                    out.push(SurfEdge {
                        face: face.name.clone().unwrap(),
                        hint: [m.x, m.y, m.z],
                    });
                }
            }
        }
        out
    };
    let sections = vec![
        LoftSection::Edges(boundary(&p, "Extrusion2")),
        LoftSection::Edges(boundary(&p, "Extrusion4")),
    ];
    let base = p.clone();
    // Output Surface: one body of four bridges (one per pair of sides), every one meshed.
    let mut q = p.clone();
    let mut def = LoftFeature::new(sections.clone());
    def.output = Output::Surface;
    q.add_loft(def).unwrap();
    assert!(q.feature_health.is_empty(), "{:?}", q.feature_health);
    let bridges = q
        .exact_bodies
        .0
        .iter()
        .find(|s| s.name.as_deref() == Some("Loft1"))
        .unwrap();
    assert!(bridges.is_open(), "the bridges are a surface body");
    let b = cadcraft3d::kernel::exact::to_brep(std::slice::from_ref(bridges)).unwrap();
    let tris: Vec<usize> = b
        .faces()
        .map(|(_, f)| f.mesh.as_ref().map_or(0, |m| m.triangles.len()))
        .collect();
    assert_eq!(tris.len(), 4, "{tris:?}");
    assert!(
        tris.iter().all(|&n| n > 0),
        "a bridge was not meshed: {tris:?}"
    );
    // Stitch closes the two surfaces and the bridges into a solid.
    op(
        &mut q,
        BodyOp::Stitch(StitchOp {
            surfaces: vec!["Extrusion2".into(), "Extrusion4".into(), "Loft1".into()],
            tolerance: 0.001,
            keep_surface: false,
        }),
    );
    let stitched = solid_volume(&q);
    // Output Solid: the same solid in one feature; the two surfaces are used up.
    let mut def = LoftFeature::new(sections.clone());
    def.output = Output::Solid;
    p.add_loft(def).unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    let v = solid_volume(&p);
    // Between the surfaces: about 35 mm high over about 28 000 mm².
    assert!(v > 850_000.0 && v < 1_000_000.0, "{v}");
    close(stitched, v, 1e-6);
    assert!(
        p.exact_bodies
            .0
            .iter()
            .all(|s| !matches!(s.name.as_deref(), Some("Extrusion2" | "Extrusion4"))),
        "the sewn surfaces stay"
    );
    // Every face of the solid is meshed and faces out.
    let solid = p.exact_bodies.0.iter().find(|s| !s.is_open()).unwrap();
    let b = cadcraft3d::kernel::exact::to_brep(std::slice::from_ref(solid)).unwrap();
    assert!(
        b.faces()
            .all(|(_, f)| f.mesh.as_ref().is_some_and(|m| !m.triangles.is_empty())),
        "a face of the solid has no triangles"
    );
    let inside = |q| cadcraft3d::kernel::exact::contains(solid, q);
    let tris = solid.triangles().unwrap();
    for [a, b, c] in tris.iter().step_by(tris.len() / 30 + 1) {
        if let Some(n) = (b - a).cross(&(c - a)).try_normalize(1e-18) {
            let m = cadcraft3d::kernel::Point3::from((a.coords + b.coords + c.coords) / 3.0);
            assert!(
                !(inside(m + n * 1e-2) && !inside(m - n * 1e-2)),
                "a face faces in at {m:?}"
            );
        }
    }
    // Tangent to both surfaces (spec 5.4a F6): the bridges leave each surface going on past its
    // edges (here outwards and down / up) and turn; the corners' curves are shared by the
    // neighbouring bridges, so the solid still closes, bulging out (more volume than G0).
    use cadcraft3d::part::{ConditionKind, LoftCondition};
    let tangent = LoftCondition {
        kind: ConditionKind::Tangent,
        ..Default::default()
    };
    let mut t = base;
    let mut def = LoftFeature::new(sections);
    def.output = Output::Solid;
    def.conditions = vec![tangent, tangent];
    t.add_loft(def).unwrap();
    assert!(t.feature_health.is_empty(), "{:?}", t.feature_health);
    let vt = solid_volume(&t);
    assert!(vt > v * 1.01 && vt < v * 1.5, "{vt} vs G0 {v}");
}

/// The owner's own 8 lofts between pieces of the two surfaces' edges (Surface_test_3, before its
/// hole), then Stitch: the edges are split at different places on the two surfaces, so a long
/// edge meets several pieces (T-junctions) and two curves along one edge are parametrised
/// differently — Stitch cuts the long edges there and matches edges by geometry, and closes a
/// solid of the same volume as Loft between the two whole surfaces.
#[test]
fn stitch_closes_the_owners_eight_lofts() {
    use cadcraft3d::document::{DocumentData, Project};
    let pr = Project::load(std::path::Path::new(
        "tests/data/surface_test3_owner.3dcadcraft",
    ))
    .unwrap();
    let mut p = pr
        .documents()
        .iter()
        .find_map(|d| match &d.data {
            DocumentData::Part(p) => Some(p.clone()),
            _ => None,
        })
        .unwrap();
    // The two trimmed surfaces and the owner's Loft1..8 (without the hole and the old Stitch).
    let mut f: Vec<_> = p.features[..5].to_vec();
    f.extend(p.features[7..15].iter().cloned());
    p.features = f;
    p.eop = None;
    p.regenerate().unwrap();
    assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
    let surfaces: Vec<String> = [
        "Extrusion2",
        "Extrusion4",
        "Loft1",
        "Loft2",
        "Loft3",
        "Loft4",
        "Loft5",
        "Loft6",
        "Loft7",
        "Loft8",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    op(
        &mut p,
        BodyOp::Stitch(StitchOp {
            surfaces,
            tolerance: 0.001,
            keep_surface: false,
        }),
    );
    let v = solid_volume(&p);
    // Loft between the two whole surfaces gives ≈ 933 900 mm³ (its bridges are ruled the same
    // way, between other pieces): the same solid within 0.1 %.
    assert!(v > 930_000.0 && v < 938_000.0, "{v}");
}

/// Boundary Patch of the circular hole the owner cut out of a spline surface (Surface_test_3,
/// TrimSurface3): the kernel's fit of the loop folds over its best-fit plane on the wave, so the
/// four-sided loop is filled from its four arcs instead; the patch is meshed and about the area
/// of the hole (a disc of r ≈ 42 on a slope).
#[test]
fn patch_of_a_hole_in_a_wavy_surface() {
    use cadcraft3d::document::{DocumentData, Project};
    use cadcraft3d::kernel::TessellationOptions;
    use cadcraft3d::part::surface_op::PatchOp;
    let pr = Project::load(std::path::Path::new(
        "tests/data/surface_test3_owner.3dcadcraft",
    ))
    .unwrap();
    let mut p = pr
        .documents()
        .iter()
        .find_map(|d| match &d.data {
            DocumentData::Part(p) => Some(p.clone()),
            _ => None,
        })
        .unwrap();
    // Up to the hole (TrimSurface3), before the owner's lofts.
    p.features.truncate(7);
    p.eop = None;
    p.regenerate().unwrap();
    let before = sheet_area(&p);
    // The hole's edges: the free edges of Extrusion2 away from its outer rim.
    let opts = TessellationOptions::default();
    let mut hole = Vec::new();
    for (e, _) in p.body.edges() {
        let faces = p.body.edge_faces(e);
        let [f] = faces[..] else { continue };
        let name = p
            .body
            .face(f)
            .and_then(|x| x.name.clone())
            .unwrap_or_default();
        if !name.starts_with("Extrusion2:") {
            continue;
        }
        let pts = p.body.sample_edge(e, &opts).unwrap();
        let m = cadcraft3d::part::select::polyline_middle(&pts).unwrap();
        if (m.x + 25.0).hypot(m.y) < 46.0 && m.y.abs() < 99.0 && m.x > -90.0 && m.x < 60.0 {
            hole.push(SurfEdge {
                face: name,
                hint: [m.x, m.y, m.z],
            });
        }
    }
    assert_eq!(hole.len(), 4, "the hole's arcs");
    // Tangent (G1) on a round hole is refused for now (a four-sided patch bends at its corners
    // there), and nothing changes.
    {
        let mut q = p.clone();
        let n = q.features.len();
        assert!(
            q.add_body_op(BodyOpFeature::new(BodyOp::Patch(PatchOp {
                edges: hole.clone(),
                sketch: None,
                profile: None,
                solid: false,
                tangent: Some(1.0),
            })))
            .is_err()
        );
        assert_eq!(q.features.len(), n);
    }
    op(
        &mut p,
        BodyOp::Patch(PatchOp {
            edges: hole,
            sketch: None,
            profile: None,
            solid: false,
            tangent: None,
        }),
    );
    let patch = p
        .exact_bodies
        .0
        .iter()
        .find(|s| s.name.as_deref() == Some("BoundaryPatch1"))
        .unwrap();
    let b = cadcraft3d::kernel::exact::to_brep(std::slice::from_ref(patch)).unwrap();
    assert!(
        b.faces()
            .all(|(_, f)| f.mesh.as_ref().is_some_and(|m| !m.triangles.is_empty())),
        "the patch has no triangles"
    );
    let added = sheet_area(&p) - before;
    assert!(added > 4_000.0 && added < 9_000.0, "patch area {added}");
    // Stitched into the holed surface, the patch closes the hole: one surface, whose free
    // edges are the outer rim only (the 4 arcs of the hole are shared now).
    let free = |p: &PartDocument| {
        p.body
            .edges()
            .filter(|(e, _)| p.body.edge_faces(*e).len() == 1)
            .count()
    };
    let n = free(&p);
    let t = std::time::Instant::now();
    op(
        &mut p,
        BodyOp::Stitch(cadcraft3d::part::surface_op::StitchOp {
            surfaces: vec!["Extrusion2".into(), "BoundaryPatch1".into()],
            tolerance: 0.01,
            keep_surface: false,
        }),
    );
    let took = t.elapsed();
    let st = p
        .exact_bodies
        .0
        .iter()
        .find(|s| s.name.as_deref() == Some("Stitch1"))
        .unwrap();
    assert!(st.is_open(), "the outer rim stays open");
    assert_eq!(
        free(&p),
        n - 8,
        "the hole's 4 arcs are no longer free edges"
    );
    // It took 7 s while the conversion asked the kernel whether points lie inside a surface.
    // (Skipped like the other budgets on a busy machine: ~1.5 s alone.)
    assert!(
        took.as_secs_f64() < 5.0 || std::env::var_os("CADCRAFT3D_NO_BUDGETS").is_some(),
        "stitch took {took:?}"
    );
}
