//! Work features of an assembly (`docs/spec/3.10-top-down.md` T0): they
//! live in the assembly's hidden tools part (placed at the origin, so its
//! coordinates are the assembly's). A work plane offset from a component's
//! face is a cross-part reference that follows the component, and other
//! components can be constrained to it.

use cadcraft3d::assembly::context;
use cadcraft3d::assembly::features;
use cadcraft3d::assembly::mechanisms;
use cadcraft3d::assembly::relation::{Constraint, ConstraintKind, GeomRef, OriginItem};
use cadcraft3d::assembly::tree::Linked;
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Project};
use cadcraft3d::kernel::{Point3, Real, Transform3};
use cadcraft3d::part::blend::edge_ref;
use cadcraft3d::part::context::{ContextRef, ContextSource};
use cadcraft3d::part::{
    LineRef, PartDocument, PlaneRef, PointRef, WorkAxisDef, WorkDef, WorkFeature, WorkPlaneDef,
    WorkPointDef,
};

struct Case {
    project: Project,
    asm: DocumentId,
    tools: DocumentId,
    tools_occ: u32,
    base: u32,
    block: u32,
}

/// A grounded 100 × 60 × 20 base (top face at z 20) and a free 20 × 20 × 10
/// block above it; the tools part made.
fn case() -> Case {
    let mut project = Project::new("WorkFeatures");
    let base_doc = project.add_document(
        "Base",
        DocumentData::Part(PartDocument::with_box("Box1", [100.0, 60.0, 20.0]).unwrap()),
    );
    let block_doc = project.add_document(
        "Block",
        DocumentData::Part(PartDocument::with_box("Box1", [20.0, 20.0, 10.0]).unwrap()),
    );
    let mut a = AssemblyDocument::default();
    let base = a.place(
        DocRef::local(base_doc),
        "Base",
        Transform3::identity(),
        true,
    );
    let block = a.place(
        DocRef::local(block_doc),
        "Block",
        Transform3::translation(10.0, 10.0, 50.0),
        false,
    );
    let asm = project.add_document("Top", DocumentData::Assembly(a));
    let (tools_occ, tools) = features::ensure_tools(&mut project, asm).unwrap();
    Case {
        project,
        asm,
        tools,
        tools_occ,
        base,
        block,
    }
}

fn assembly(p: &Project, id: DocumentId) -> &AssemblyDocument {
    match &p.document(id).unwrap().data {
        DocumentData::Assembly(a) => a,
        _ => panic!("not an assembly"),
    }
}

fn assembly_mut(p: &mut Project, id: DocumentId) -> &mut AssemblyDocument {
    match &mut p.document_mut(id).unwrap().data {
        DocumentData::Assembly(a) => a,
        _ => panic!("not an assembly"),
    }
}

fn part(p: &Project, id: DocumentId) -> &PartDocument {
    match &p.document(id).unwrap().data {
        DocumentData::Part(x) => x,
        _ => panic!("not a part"),
    }
}

fn part_mut(p: &mut Project, id: DocumentId) -> &mut PartDocument {
    match &mut p.document_mut(id).unwrap().data {
        DocumentData::Part(x) => x,
        _ => panic!("not a part"),
    }
}

/// Work Plane in the assembly: 15 mm above the base's top face (what the
/// app's command makes from a click on the face). Returns its id.
fn plane_above_base(c: &mut Case) -> String {
    let source = ContextSource {
        vertex: None,
        assembly: c.asm,
        target: vec![c.tools_occ],
        path: vec![c.base],
        face: "Box1:Top".into(),
        edge: None,
        hint: [50.0, 30.0, 20.0],
        label: "Base:1 face".into(),
    };
    let linked = Linked::new();
    let geom = context::current(&c.project, &linked, assembly(&c.project, c.asm), &source).unwrap();
    let tp = part_mut(&mut c.project, c.tools);
    let id = tp.add_context(ContextRef {
        id: String::new(),
        source,
        geom,
    });
    tp.add_work(WorkFeature::new(WorkDef::Plane(WorkPlaneDef::Offset {
        base: PlaneRef::Context(id),
        distance: 15.0,
    })))
    .unwrap()
}

fn plane_z(c: &Case, id: &str) -> Real {
    let p = &part(&c.project, c.tools).work.planes[id];
    assert!(p.normal.z.abs() > 1.0 - 1e-9, "{p:?}");
    p.origin.z
}

#[test]
fn a_work_plane_on_a_components_face() {
    let mut c = case();
    let id = plane_above_base(&mut c);
    assert!((plane_z(&c, &id) - 35.0).abs() < 1e-9);
    // It is saved with the tools part and computed again after a load.
    let dir = std::env::temp_dir().join("cadcraft_asm_work_features");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("plane.3dcadcraft");
    c.project.save(&file).unwrap();
    let mut back = Project::load(&file).unwrap();
    back.rebuild_unmeshed_parts();
    let p = &part(&back, c.tools).work.planes[&id];
    assert!((p.origin.z - 35.0).abs() < 1e-9, "{p:?}");
}

#[test]
fn the_work_plane_follows_the_component() {
    let mut c = case();
    let id = plane_above_base(&mut c);
    let base = c.base;
    if let Some(o) = assembly_mut(&mut c.project, c.asm).find_mut(base) {
        o.transform = Transform3::translation(0.0, 0.0, 5.0);
    }
    let r = context::update(&mut c.project, &Linked::new(), c.asm);
    assert!(r.sick.is_empty(), "{:?}", r.sick);
    assert!(r.changed.contains(&c.tools), "{:?}", r.changed);
    assert!((plane_z(&c, &id) - 40.0).abs() < 1e-9);
}

#[test]
fn a_component_flush_with_the_assemblys_work_plane() {
    let mut c = case();
    let id = plane_above_base(&mut c);
    let mut k = Constraint::new(
        ConstraintKind::Flush,
        GeomRef::Origin {
            occurrence: c.block,
            sub: Vec::new(),
            item: OriginItem::XyPlane,
        },
        GeomRef::Work {
            occurrence: c.tools_occ,
            sub: Vec::new(),
            id,
        },
    );
    k.id = 1;
    k.name = "Flush:1".into();
    assembly_mut(&mut c.project, c.asm).constraints.push(k);
    let a = assembly(&c.project, c.asm).clone();
    let s = mechanisms::solve_in(&c.project, &a, None);
    assert!(s.converged, "residual {} sick {:?}", s.residual, s.sick);
    let (_, t) = s
        .transforms
        .iter()
        .find(|(o, _)| *o == c.block)
        .expect("the block moves");
    assert!((t.translation.z - 35.0).abs() < 1e-6, "{t:?}");
}

/// The base's top edge along X at y 0, as a work axis of the assembly.
fn axis_on_base_edge(c: &mut Case) -> String {
    let base_doc = assembly(&c.project, c.asm).find(c.base).unwrap().component;
    let body = &part(&c.project, base_doc.document).body;
    let e = body
        .edges()
        .find(|(_, e)| {
            let (a, b) = (
                body.vertex(e.start).unwrap().point,
                body.vertex(e.end).unwrap().point,
            );
            (a.y.abs() + b.y.abs()) < 1e-9
                && (a.z - 20.0).abs() + (b.z - 20.0).abs() < 1e-9
                && (a.x - b.x).abs() > 1.0
        })
        .map(|(id, _)| id)
        .expect("the top front edge");
    let edge = edge_ref(body, e).expect("its reference");
    let source = ContextSource {
        vertex: None,
        assembly: c.asm,
        target: vec![c.tools_occ],
        path: vec![c.base],
        face: edge.faces[0].clone(),
        hint: edge.hint,
        edge: Some(edge),
        label: "Base:1 edge".into(),
    };
    let geom = context::current(
        &c.project,
        &Linked::new(),
        assembly(&c.project, c.asm),
        &source,
    )
    .unwrap();
    let tp = part_mut(&mut c.project, c.tools);
    let id = tp.add_context(ContextRef {
        id: String::new(),
        source,
        geom,
    });
    tp.add_work(WorkFeature::new(WorkDef::Axis(WorkAxisDef::OnLine {
        line: LineRef::Context(id),
    })))
    .unwrap()
}

/// The base's corner (100, 60, 20) as a work point of the assembly.
fn point_on_base_vertex(c: &mut Case) -> String {
    let base_doc = assembly(&c.project, c.asm).find(c.base).unwrap().component;
    let body = &part(&c.project, base_doc.document).body;
    let name = body
        .vertices()
        .find(|(_, v)| {
            (v.point.x - 100.0).abs() + (v.point.y - 60.0).abs() + (v.point.z - 20.0).abs() < 1e-9
        })
        .and_then(|(_, v)| v.name.clone())
        .expect("the named corner");
    let source = ContextSource {
        vertex: Some(name.clone()),
        assembly: c.asm,
        target: vec![c.tools_occ],
        path: vec![c.base],
        face: name,
        edge: None,
        hint: [100.0, 60.0, 20.0],
        label: "Base:1 vertex".into(),
    };
    let geom = context::current(
        &c.project,
        &Linked::new(),
        assembly(&c.project, c.asm),
        &source,
    )
    .unwrap();
    let tp = part_mut(&mut c.project, c.tools);
    let id = tp.add_context(ContextRef {
        id: String::new(),
        source,
        geom,
    });
    tp.add_work(WorkFeature::new(WorkDef::Point(WorkPointDef::OnPoint {
        point: PointRef::Context(id),
    })))
    .unwrap()
}

fn move_base_up(c: &mut Case, z: Real) {
    let base = c.base;
    if let Some(o) = assembly_mut(&mut c.project, c.asm).find_mut(base) {
        o.transform = Transform3::translation(0.0, 0.0, z);
    }
    let r = context::update(&mut c.project, &Linked::new(), c.asm);
    assert!(r.sick.is_empty(), "{:?}", r.sick);
    assert!(r.changed.contains(&c.tools), "{:?}", r.changed);
}

#[test]
fn a_work_axis_along_a_components_edge_follows_it() {
    let mut c = case();
    let id = axis_on_base_edge(&mut c);
    let (p, d) = part(&c.project, c.tools).work.axes[&id];
    assert!(d.x.abs() > 1.0 - 1e-9, "{d:?}");
    assert!(p.y.abs() < 1e-9 && (p.z - 20.0).abs() < 1e-9, "{p:?}");
    move_base_up(&mut c, 5.0);
    let (p, _) = part(&c.project, c.tools).work.axes[&id];
    assert!((p.z - 25.0).abs() < 1e-9, "{p:?}");
    // Saved with the tools part, computed again after a load.
    let dir = std::env::temp_dir().join("cadcraft_asm_work_features");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("axis.3dcadcraft");
    c.project.save(&file).unwrap();
    let mut back = Project::load(&file).unwrap();
    back.rebuild_unmeshed_parts();
    let (p, _) = part(&back, c.tools).work.axes[&id];
    assert!((p.z - 25.0).abs() < 1e-9, "{p:?}");
}

#[test]
fn a_work_point_on_a_components_vertex_follows_it() {
    let mut c = case();
    let id = point_on_base_vertex(&mut c);
    let p = part(&c.project, c.tools).work.points[&id];
    assert!((p - Point3::new(100.0, 60.0, 20.0)).norm() < 1e-9, "{p:?}");
    move_base_up(&mut c, 7.0);
    let p = part(&c.project, c.tools).work.points[&id];
    assert!((p - Point3::new(100.0, 60.0, 27.0)).norm() < 1e-9, "{p:?}");
    let dir = std::env::temp_dir().join("cadcraft_asm_work_features");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("point.3dcadcraft");
    c.project.save(&file).unwrap();
    let mut back = Project::load(&file).unwrap();
    back.rebuild_unmeshed_parts();
    let p = part(&back, c.tools).work.points[&id];
    assert!((p - Point3::new(100.0, 60.0, 27.0)).norm() < 1e-9, "{p:?}");
}

#[test]
fn a_lost_vertex_is_reported_and_the_copy_stays() {
    let mut c = case();
    let id = point_on_base_vertex(&mut c);
    let tp = part_mut(&mut c.project, c.tools);
    let k = tp
        .context
        .iter()
        .position(|x| x.source.vertex.is_some())
        .unwrap();
    tp.context[k].source.vertex = Some("Box1:NoSuchVertex".into());
    let r = context::update(&mut c.project, &Linked::new(), c.asm);
    assert_eq!(r.sick.len(), 1, "{:?}", r.sick);
    let p = part(&c.project, c.tools).work.points[&id];
    assert!((p - Point3::new(100.0, 60.0, 20.0)).norm() < 1e-9, "{p:?}");
}

#[test]
fn a_component_mated_to_the_assemblys_work_point() {
    let mut c = case();
    let id = point_on_base_vertex(&mut c);
    let mut k = Constraint::new(
        ConstraintKind::Mate,
        GeomRef::Origin {
            occurrence: c.block,
            sub: Vec::new(),
            item: OriginItem::Center,
        },
        GeomRef::Work {
            occurrence: c.tools_occ,
            sub: Vec::new(),
            id,
        },
    );
    k.id = 1;
    k.name = "Mate:1".into();
    assembly_mut(&mut c.project, c.asm).constraints.push(k);
    let a = assembly(&c.project, c.asm).clone();
    let s = mechanisms::solve_in(&c.project, &a, None);
    assert!(s.converged, "residual {} sick {:?}", s.residual, s.sick);
    let (_, t) = s
        .transforms
        .iter()
        .find(|(o, _)| *o == c.block)
        .expect("the block moves");
    let at = t.translation.vector;
    assert!(
        (at - nalgebra::Vector3::new(100.0, 60.0, 20.0)).norm() < 1e-6,
        "{t:?}"
    );
}
