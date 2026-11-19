//! The relationships of the machine elements the Design tab generates (spec
//! 6.2-design-calculators §4a), GUI-free: a generated shaft, gear, key, bearing
//! or spring is placed on its axis and start plane by ordinary relationships —
//! a Mate of its Z axis with the picked axis (the turn about it stays free) and
//! its XY plane flush with the start plane — so it follows the parts it sits on.
//! A gear pair gets a Rotation motion link between its two axis Mates whose
//! ratio is the teeth's (the gears turn as they mesh); a key is held in the
//! shaft's frame (its groove's place) by three plane relationships.
//!
//! Every relationship is created *at the place the component already stands*:
//! its offset is measured there, Mate or Flush follows from the normals, so the
//! first solve moves nothing.

use super::motion::{LinkKind, MotionLink};
use super::relation::{self, ASSEMBLY, Constraint, ConstraintKind, Geom, GeomRef, OriginItem};
use super::solve::Solved;
use super::{flexible, tree};
use crate::document::{AssemblyDocument, DocumentData, DocumentId, Occurrence, Project};
use crate::kernel::{Real, Transform3};
use crate::part::work::{UcsDef, WorkDef, WorkFeature};

thread_local! {
    /// The linked files the references may reach (library parts, part files),
    /// set by [`with_linked`] for the calls inside it.
    static LINKED: std::cell::RefCell<std::rc::Rc<tree::Linked>> =
        std::cell::RefCell::new(std::rc::Rc::new(tree::Linked::new()));
}

/// Runs `f` with `linked` as the files references into linked components
/// read (the app's loaded links); outside it only the project's own parts.
pub fn with_linked<R>(linked: std::rc::Rc<tree::Linked>, f: impl FnOnce() -> R) -> R {
    let old = LINKED.with(|l| std::mem::replace(&mut *l.borrow_mut(), linked));
    let r = f();
    LINKED.with(|l| *l.borrow_mut() = old);
    r
}

/// An origin item of occurrence `occ` (or of the assembly, [`ASSEMBLY`]).
pub fn origin(occ: u32, item: OriginItem) -> GeomRef {
    GeomRef::Origin {
        occurrence: occ,
        sub: Vec::new(),
        item,
    }
}

/// Geometry of `r` in assembly coordinates (the components as they stand).
pub fn world(project: &Project, asm: &AssemblyDocument, r: &GeomRef) -> Result<Geom, String> {
    if r.occurrence() == ASSEMBLY {
        return match r {
            GeomRef::Origin { item, .. } => Ok(item.geom()),
            _ => Err("the assembly has only its origin".into()),
        };
    }
    let o = asm
        .find(r.occurrence())
        .ok_or("the component no longer exists")?;
    Ok(local(project, asm, r)?.moved(&o.transform))
}

/// Geometry of `r` in its top occurrence's coordinates (what the solver reads).
fn local(project: &Project, asm: &AssemblyDocument, r: &GeomRef) -> Result<Geom, String> {
    if r.occurrence() == ASSEMBLY {
        return world(project, asm, r);
    }
    // Every part's origin is the same: no need to find the part (a linked
    // library part is not in the project).
    if let GeomRef::Origin { item, sub, .. } = r
        && sub.is_empty()
    {
        return Ok(item.geom());
    }
    let linked = LINKED.with(|l| l.borrow().clone());
    let n = tree::nested(project, &linked, asm, r.occurrence(), r.sub())?;
    let part = tree::part_of(project, &linked, n.component).ok_or("not a part")?;
    let g = relation::resolve_in(part, r)?;
    Ok(if r.sub().is_empty() {
        g
    } else {
        g.moved(&n.at)
    })
}

/// Solves assembly `asm` of `project` as the app does (references through the
/// tree, the assembly's own origin included).
pub fn solve(project: &Project, asm: &AssemblyDocument) -> Solved {
    let linked = LINKED.with(|l| l.borrow().clone());
    let l = |r: &GeomRef| local(project, asm, r);
    let sub_of = |o: &Occurrence| {
        tree::assembly_of(project, &linked, o.component)
            .map(|s| tree::rebased(s, o.component.library))
    };
    flexible::solve(asm, &l, None, &sub_of)
}

/// The value relationship `c` reads with the components as they stand: an
/// offset, or the turn of an axis Mate / Rotational joint.
pub fn value(project: &Project, asm: &AssemblyDocument, c: &Constraint) -> Option<Real> {
    let a = world(project, asm, &c.a).ok()?;
    let b = world(project, asm, &c.b).ok()?;
    let (a, b) = (
        relation::for_kind(c.kind, &c.a, a),
        relation::for_kind(c.kind, &c.b, b),
    );
    if relation::is_axis_mate(c.kind, &a, &b) || c.kind.is_joint() {
        // Frames made in the part's own coordinates, then moved (as the solver
        // does): a frame made from the moved axis would lose the part's turn.
        let frame = |r: &GeomRef| -> Option<Geom> {
            let g = relation::for_kind(c.kind, r, local(project, asm, r).ok()?).as_frame();
            Some(match r.occurrence() {
                ASSEMBLY => g,
                o => g.moved(&asm.find(o)?.transform),
            })
        };
        return relation::measure(c, &frame(&c.a)?, &frame(&c.b)?);
    }
    relation::measure(c, &a, &b)
}

/// Adds a relationship between `a` and `b` holding them as they stand: planes
/// Flush when their normals point the same way, else Mate, at the measured
/// offset; axes Mated (the turn free). Returns its id.
pub fn hold(
    project: &Project,
    asm: &mut AssemblyDocument,
    a: GeomRef,
    b: GeomRef,
) -> Result<u32, String> {
    let (ga, gb) = (world(project, asm, &a)?, world(project, asm, &b)?);
    let kind = match (ga, gb) {
        (Geom::Plane { n: na, .. }, Geom::Plane { n: nb, .. }) => {
            if na.dot(&nb) >= 0.0 {
                ConstraintKind::Flush
            } else {
                ConstraintKind::Mate
            }
        }
        _ => ConstraintKind::Mate,
    };
    let mut c = Constraint::new(kind, a, b);
    if !(ga.is_axis_like() && gb.is_axis_like()) {
        c.value = value(project, asm, &c).ok_or("the relationship cannot be measured")?;
    }
    Ok(asm.add_constraint(c))
}

/// What holds a generated component on its seat: the Mate of its Z axis with
/// the seat's axis (its turn is the value a motion link drives) and the
/// relationship of its XY plane with the start plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seated {
    /// The axis Mate.
    pub axis: u32,
    /// The start plane's relationship.
    pub start: u32,
}

/// Seats occurrence `occ` (Z = its axis, XY = its start) on `axis` and `start`
/// as it stands (rule: the first solve moves nothing).
pub fn seat(
    project: &Project,
    asm: &mut AssemblyDocument,
    occ: u32,
    axis: GeomRef,
    start: GeomRef,
) -> Result<Seated, String> {
    let ax = hold(project, asm, origin(occ, OriginItem::ZAxis), axis)?;
    let st = hold(project, asm, origin(occ, OriginItem::XyPlane), start)?;
    // Held by its relationships: it turns about its axis.
    if let Some(o) = asm.find_mut(occ) {
        o.grounded = false;
    }
    Ok(Seated {
        axis: ax,
        start: st,
    })
}

/// A user coordinate system in the assembly (in its Assembly Features tools
/// part, made when missing) at `at`: its Z axis and XY plane, a seat for a
/// component that has nothing picked to stand on.
pub fn ucs_seat(
    project: &mut Project,
    asm_id: DocumentId,
    at: &Transform3,
) -> Result<(GeomRef, GeomRef), String> {
    let (occ, doc) = super::features::ensure_tools(project, asm_id)?;
    let def = ucs_def(at);
    let Some(DocumentData::Part(p)) = project.document_mut(doc).map(|d| &mut d.data) else {
        return Err("the assembly's features part is missing".into());
    };
    let mut w = WorkFeature::new(def);
    // A seat, not something to look at (the browser still lists it).
    w.visible = false;
    let id = p.add_work(w).map_err(|e| e.to_string())?;
    p.regenerate().map_err(|e| e.to_string())?;
    let w = |s: &str| GeomRef::Work {
        occurrence: occ,
        sub: Vec::new(),
        id: format!("{id}:{s}"),
    };
    Ok((w("Z"), w("XY")))
}

/// The Rotation motion link of a gear pair: the driven gear's axis Mate turns
/// `−z1 / z2` times the driver's (the other way round when their axes point
/// opposite ways), offset so the teeth mesh where they stand. Returns its id.
pub fn gear_link(
    project: &Project,
    asm: &mut AssemblyDocument,
    driver: u32,
    driven: u32,
    teeth: [u32; 2],
) -> Result<u32, String> {
    let c1 = asm
        .constraint(driver)
        .ok_or("the first gear's Mate is gone")?;
    let c2 = asm
        .constraint(driven)
        .ok_or("the second gear's Mate is gone")?;
    let dir = |c: &Constraint| -> Result<crate::kernel::Vector3, String> {
        Ok(world(project, asm, &c.a)?.direction())
    };
    let same = dir(c1)?.dot(&dir(c2)?) >= 0.0;
    let ratio = f64::from(teeth[0]) / f64::from(teeth[1].max(1)) * if same { -1.0 } else { 1.0 };
    let v1 = value(project, asm, c1).ok_or("the first gear's turn cannot be read")?;
    let v2 = value(project, asm, c2).ok_or("the second gear's turn cannot be read")?;
    let id = asm.motion.next_id();
    asm.motion.links.push(MotionLink {
        id,
        driver,
        driven,
        kind: LinkKind::Rotation,
        ratio,
        offset: v2 - ratio * v1,
        cam: None,
        suppressed: false,
        last: None,
    });
    Ok(id)
}

/// Gear 2's place turned about its own axis so its teeth mesh with gear 1's
/// (each gear has a tooth centred on its +X): seen along gear 1's axis with
/// gear 1's X at angle 0 and the centre line at φ, gear 2's X stands at
/// φ + π + π / z2 + φ z1 / z2 (a space of gear 2 faces gear 1's tooth on the
/// line, then the pair rolled back by φ).
pub fn mesh_turn(t1: &Transform3, t2: &Transform3, teeth: [u32; 2]) -> Transform3 {
    use crate::kernel::{Point3, Vector3};
    let n = t1 * Vector3::z();
    let e1 = t1 * Vector3::x();
    let e2 = n.cross(&e1);
    let ang = |v: Vector3| v.dot(&e2).atan2(v.dot(&e1));
    let (c1, c2) = (t1 * Point3::origin(), t2 * Point3::origin());
    let (z1, z2) = (f64::from(teeth[0]), f64::from(teeth[1].max(1)));
    let phi = ang(c2 - c1);
    let pi = std::f64::consts::PI;
    let target = phi + pi + pi / z2 + phi * z1 / z2;
    let delta = target - ang(t2 * Vector3::x());
    let s = if (t2 * Vector3::z()).dot(&n) >= 0.0 {
        1.0
    } else {
        -1.0
    };
    t2 * Transform3::new(Vector3::zeros(), Vector3::z() * (s * delta))
}

/// The UCS of the Assembly Features part an axis Mate seats its component on
/// (`ucs_seat`): the UCS's id.
pub fn ucs_of(asm: &AssemblyDocument, mate: u32) -> Option<String> {
    let tools = asm.features.as_ref()?.occurrence;
    match &asm.constraint(mate)?.b {
        GeomRef::Work { occurrence, id, .. } if *occurrence == tools => {
            id.strip_suffix(":Z").map(str::to_owned)
        }
        _ => None,
    }
}

/// Moves UCS `ucs` of the Assembly Features part of `asm_id` to `at`.
pub fn move_ucs(
    project: &mut Project,
    asm_id: DocumentId,
    ucs: &str,
    at: &Transform3,
) -> Result<(), String> {
    let (_, doc) = super::features::ensure_tools(project, asm_id)?;
    let def = ucs_def(at);
    let Some(DocumentData::Part(p)) = project.document_mut(doc).map(|d| &mut d.data) else {
        return Err("the assembly's features part is missing".into());
    };
    let Some(f) = p.features.iter_mut().find_map(|f| match f {
        crate::part::Feature::Work(w) if w.id == ucs => Some(w),
        _ => None,
    }) else {
        return Err(format!("the coordinate system {ucs} is gone"));
    };
    f.def = def;
    p.regenerate().map_err(|e| e.to_string())
}

/// A UCS work feature's definition placing it at `at`.
pub fn ucs_def_of(at: &Transform3) -> WorkDef {
    ucs_def(at)
}

fn ucs_def(at: &Transform3) -> WorkDef {
    let r = at.rotation.to_rotation_matrix();
    let m = r.matrix();
    // Its axes are the columns of Rz·Ry·Rx (`UcsDef::Offsets`).
    let ry = (-m[(2, 0)]).clamp(-1.0, 1.0).asin();
    let rx = m[(2, 1)].atan2(m[(2, 2)]);
    let rz = m[(1, 0)].atan2(m[(0, 0)]);
    let t = at.translation.vector;
    WorkDef::Ucs(UcsDef::Offsets {
        x: t.x,
        y: t.y,
        z: t.z,
        rx,
        ry,
        rz,
    })
}

/// Moves every motion link driving or driven by relationship `from` onto
/// relationship `to` (a gear keyed to its shaft: the shaft's own turn drives
/// the pair), its ratio's sign following the two axes and its offset measured
/// where the components stand.
pub fn move_links(
    project: &Project,
    asm: &mut AssemblyDocument,
    from: u32,
    to: u32,
) -> Result<(), String> {
    let dir = |asm: &AssemblyDocument, id: u32| -> Result<crate::kernel::Vector3, String> {
        let c = asm.constraint(id).ok_or("a relationship is gone")?;
        Ok(world(project, asm, &c.a)?.direction())
    };
    let flip = dir(asm, from)?.dot(&dir(asm, to)?) < 0.0;
    let links: Vec<usize> = (0..asm.motion.links.len())
        .filter(|&i| asm.motion.links[i].driver == from || asm.motion.links[i].driven == from)
        .collect();
    for i in links {
        let mut l = asm.motion.links[i].clone();
        if l.driver == from {
            l.driver = to;
        } else {
            l.driven = to;
        }
        if flip {
            l.ratio = -l.ratio;
        }
        let v1 = asm
            .constraint(l.driver)
            .and_then(|c| value(project, asm, c))
            .ok_or("a turn cannot be read")?;
        let v2 = asm
            .constraint(l.driven)
            .and_then(|c| value(project, asm, c))
            .ok_or("a turn cannot be read")?;
        l.offset = v2 - l.ratio * v1;
        l.last = None;
        asm.motion.links[i] = l;
    }
    Ok(())
}

/// The axis Mate of a seated occurrence: the Mate whose first selection is its
/// Z axis.
pub fn axis_mate(asm: &AssemblyDocument, occ: u32) -> Option<u32> {
    asm.constraints
        .iter()
        .find(|c| c.kind == ConstraintKind::Mate && c.a == origin(occ, OriginItem::ZAxis))
        .map(|c| c.id)
}

/// Holds `key` in the frame of `shaft` (its groove's place) as it stands: the
/// key's XZ and YZ planes with the shaft's (they must be parallel: the shaft's
/// axis is its own Z) and its XY plane with the start plane. `None` when the
/// planes are not parallel (the key is placed without relationships).
pub fn hold_key(
    project: &Project,
    asm: &mut AssemblyDocument,
    key: u32,
    shaft: u32,
    start: GeomRef,
) -> Result<Option<[u32; 3]>, String> {
    let pairs = [OriginItem::XzPlane, OriginItem::YzPlane];
    for item in pairs {
        let a = world(project, asm, &origin(shaft, item))?;
        let b = world(project, asm, &origin(key, item))?;
        if a.direction().cross(&b.direction()).norm() > 1e-6 {
            return Ok(None);
        }
    }
    let x = hold(
        project,
        asm,
        origin(shaft, OriginItem::XzPlane),
        origin(key, OriginItem::XzPlane),
    )?;
    let y = hold(
        project,
        asm,
        origin(shaft, OriginItem::YzPlane),
        origin(key, OriginItem::YzPlane),
    )?;
    let z = hold(project, asm, origin(key, OriginItem::XyPlane), start)?;
    Ok(Some([x, y, z]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::gear::GearPair;
    use crate::design::gear_model::gear_part;
    use crate::document::DocRef;
    use crate::kernel::Vector3;
    use std::f64::consts::PI;

    fn asm_of(p: &Project, id: DocumentId) -> &AssemblyDocument {
        match p.document(id).map(|d| &d.data) {
            Some(DocumentData::Assembly(a)) => a,
            _ => panic!("no assembly"),
        }
    }

    fn asm_mut(p: &mut Project, id: DocumentId) -> &mut AssemblyDocument {
        match p.document_mut(id).map(|d| &mut d.data) {
            Some(DocumentData::Assembly(a)) => a,
            _ => panic!("no assembly"),
        }
    }

    /// The angle about world Z of an occurrence's X axis.
    fn angle(p: &Project, id: DocumentId, occ: u32) -> f64 {
        let t = asm_of(p, id).find(occ).unwrap().transform;
        let x = t * Vector3::x();
        x.y.atan2(x.x)
    }

    fn solve_apply(p: &mut Project, id: DocumentId) {
        let s = solve(p, asm_of(p, id));
        assert!(s.converged, "residual {}", s.residual);
        flexible::apply(&s, asm_mut(p, id));
    }

    /// The turn that meshes gear 2 reproduces the plain layout (gear 2 on +X
    /// of gear 1, turned π + π / z2) and rolls with the pair: gear 1 turned
    /// by δ gives gear 2 −δ z1 / z2 more.
    #[test]
    fn mesh_turn_rolls_with_the_pair() {
        let t1 = Transform3::identity();
        let t2 = Transform3::new(Vector3::new(60.0, 0.0, 0.0), Vector3::zeros());
        let m = mesh_turn(&t1, &t2, [20, 40]);
        let x = m * Vector3::x();
        let want = PI + PI / 40.0;
        assert!((x.y.atan2(x.x) - (want - 2.0 * PI)).abs() < 1e-9);
        let d = 0.2;
        let t1 = Transform3::new(Vector3::zeros(), Vector3::z() * d);
        let m = mesh_turn(&t1, &t2, [20, 40]);
        let x = m * Vector3::x();
        let got = x.y.atan2(x.x).rem_euclid(2.0 * PI);
        let want = (PI + PI / 40.0 - d * 0.5).rem_euclid(2.0 * PI);
        assert!((got - want).abs() < 1e-9, "{got} {want}");
    }

    /// Two gears seated on UCSs of the assembly (nothing picked) at their
    /// centre distance: the first solve moves nothing; turning gear 1 by θ
    /// (its Mate's value held) and following the link turns gear 2 by
    /// −θ z1 / z2 about its own axis, which stays at the centre distance.
    #[test]
    fn a_gear_pair_meshes_and_turns_by_the_ratio() {
        let pair = GearPair {
            module: 2.0,
            teeth: [18, 30],
            face: [10.0, 10.0],
            ..GearPair::default()
        };
        let g = pair.geometry().unwrap();
        let mut p = Project::new("Test");
        let a = p.add_document("Gears", DocumentData::Assembly(AssemblyDocument::default()));
        let mut occ = [0u32; 2];
        let places = [
            Transform3::identity(),
            Transform3::new(
                Vector3::new(g.center_distance, 0.0, 0.0),
                Vector3::z() * (PI + PI / 30.0),
            ),
        ];
        for k in 0..2 {
            let mut part = gear_part(&pair, &g, k, 8.0).unwrap();
            part.regenerate().unwrap();
            let d = p.add_document(format!("Gear{k}"), DocumentData::Part(part));
            occ[k] = asm_mut(&mut p, a).place(DocRef::local(d), "Gear", places[k], false);
        }
        let mut mates = [0u32; 2];
        for k in 0..2 {
            let (ax, st) = ucs_seat(&mut p, a, &places[k]).unwrap();
            let q = p.clone();
            let s = seat(&q, asm_mut(&mut p, a), occ[k], ax, st).unwrap();
            mates[k] = s.axis;
        }
        let q = p.clone();
        let link = gear_link(&q, asm_mut(&mut p, a), mates[0], mates[1], pair.teeth).unwrap();
        let l = asm_of(&p, a).motion.links[0].clone();
        assert_eq!(l.id, link);
        assert!((l.ratio + 18.0 / 30.0).abs() < 1e-12, "{}", l.ratio);
        solve_apply(&mut p, a);
        let before = [angle(&p, a, occ[0]), angle(&p, a, occ[1])];
        // Following the link where the gears stand moves nothing (the offset
        // keeps them meshing).
        {
            let asm = asm_of(&p, a);
            let v = value(&p, asm, asm.constraint(mates[0]).unwrap()).unwrap();
            let w = value(&p, asm, asm.constraint(mates[1]).unwrap()).unwrap();
            assert!(
                (l.driven_value(v) - w).abs() < 1e-9,
                "{} vs {w}",
                l.driven_value(v)
            );
        }
        assert!(
            (before[1] - (PI / 30.0 - PI)).abs() < 1e-6,
            "gear 2 at {}",
            before[1]
        );
        let c2 = asm_of(&p, a).find(occ[1]).unwrap().transform * crate::kernel::Point3::origin();
        assert!((c2.x - g.center_distance).abs() < 1e-6 && c2.y.abs() < 1e-6);
        // Gear 1 turns by θ: its Mate's value held there.
        let theta = 0.3;
        let v1 = {
            let c = asm_of(&p, a).constraint(mates[0]).unwrap().clone();
            value(&p, asm_of(&p, a), &c).unwrap()
        };
        {
            let c = asm_mut(&mut p, a).constraint_mut(mates[0]).unwrap();
            c.locked = true;
            c.value = v1 + theta;
        }
        solve_apply(&mut p, a);
        let d1 = angle(&p, a, occ[0]) - before[0];
        assert!((d1.abs() - theta).abs() < 1e-6, "gear 1 turned {d1}");
        // The link: gear 2's Mate follows.
        let v = {
            let asm = asm_of(&p, a);
            value(&p, asm, asm.constraint(mates[0]).unwrap()).unwrap()
        };
        let c = asm_mut(&mut p, a).constraint_mut(mates[1]).unwrap();
        c.locked = true;
        c.value = l.driven_value(v);
        solve_apply(&mut p, a);
        let d2 = angle(&p, a, occ[1]) - before[1];
        let want = -d1 * 18.0 / 30.0;
        assert!((d2 - want).abs() < 1e-6, "gear 2 turned {d2}, want {want}");
        let c2 = asm_of(&p, a).find(occ[1]).unwrap().transform * crate::kernel::Point3::origin();
        assert!((c2.x - g.center_distance).abs() < 1e-6 && c2.y.abs() < 1e-6);
    }

    /// A key held in a shaft's frame follows the shaft when it turns and
    /// moves (three plane relationships, measured where they stood).
    #[test]
    fn a_key_follows_its_shaft() {
        let mut p = Project::new("Test");
        let a = p.add_document("Asm", DocumentData::Assembly(AssemblyDocument::default()));
        let shaft = crate::part::PartDocument::with_box("Box1", [10.0, 10.0, 60.0]).unwrap();
        let key = crate::part::PartDocument::with_box("Box1", [4.0, 6.0, 20.0]).unwrap();
        let ds = p.add_document("Shaft", DocumentData::Part(shaft));
        let dk = p.add_document("Key", DocumentData::Part(key));
        let t0 = Transform3::new(Vector3::new(5.0, 2.0, 0.0), Vector3::zeros());
        let off = Transform3::new(Vector3::new(9.0, 0.0, 12.0), Vector3::zeros());
        let s_occ = asm_mut(&mut p, a).place(DocRef::local(ds), "Shaft", t0, true);
        let k_occ = asm_mut(&mut p, a).place(DocRef::local(dk), "Key", t0 * off, false);
        let q = p.clone();
        let start = origin(s_occ, OriginItem::XyPlane);
        let ids = hold_key(&q, asm_mut(&mut p, a), k_occ, s_occ, start).unwrap();
        assert!(ids.is_some());
        solve_apply(&mut p, a);
        let t1 = Transform3::new(Vector3::new(-3.0, 7.0, 4.0), Vector3::z() * 0.5);
        asm_mut(&mut p, a).find_mut(s_occ).unwrap().transform = t1;
        solve_apply(&mut p, a);
        let got = asm_of(&p, a).find(k_occ).unwrap().transform;
        let want = t1 * off;
        for q in [
            crate::kernel::Point3::origin(),
            crate::kernel::Point3::new(1.0, 2.0, 3.0),
        ] {
            assert!(
                (got * q - want * q).norm() < 1e-5,
                "{} vs {}",
                got * q,
                want * q
            );
        }
    }
}
