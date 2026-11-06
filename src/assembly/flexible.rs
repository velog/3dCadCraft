//! Flexible subassemblies (`docs/spec/3.1a-subassemblies.md`, S5): an
//! occurrence of a subassembly marked [`Occurrence::flexible`] lets the
//! subassembly's free components move in the parent, held by the
//! subassembly's own relationships and by the parent's relationships to
//! them. Inventor keeps the flexible state in the parent only; so do we
//! (`Occurrence::flex`: each component's place inside that occurrence).
//!
//! The solve works on a flattened copy of the parent: every free component
//! of a flexible subassembly becomes a stand-in occurrence of the parent
//! (placed at `occurrence · place inside`), the subassembly's relationships
//! come along onto the stand-ins, and its grounded components (and its
//! origin) move with the subassembly's own occurrence. The solver needs no
//! change; the stand-ins' results are written back as places inside.
//!
//! One level: a subassembly inside a flexible one moves as one body (its
//! own components rigid in it), and a pattern's copies inside stay where
//! the subassembly put them.

use std::collections::{HashMap, HashSet};

use super::relation::{self, Geom, GeomRef};
use super::solve::{self, Drag, Solved};
use crate::document::{AssemblyDocument, FlexPlace, Occurrence};
use crate::kernel::Transform3;

/// First id of a stand-in occurrence or relationship (real ids are small
/// counters, never this large).
const STAND_IN: u32 = 1 << 30;

/// A stand-in: the flexible occurrence and the component inside it.
#[derive(Clone, Copy, Debug)]
struct StandIn {
    holder: u32,
    child: u32,
    /// The component's place inside the holder the geometry was read at.
    place: Transform3,
}

/// The flattened parent.
struct Flat {
    asm: AssemblyDocument,
    stand_ins: HashMap<u32, StandIn>,
    /// Flexible occurrences that took part.
    holders: HashSet<u32>,
}

/// Whether any occurrence of `asm` is flexible.
pub fn has_flexible(asm: &AssemblyDocument) -> bool {
    asm.occurrences.iter().any(|o| o.flexible && !o.suppressed)
}

/// The components of `sub` that move in a flexible occurrence of it: not
/// grounded, not suppressed, not a pattern's copy.
fn moving(sub: &AssemblyDocument) -> Vec<&Occurrence> {
    sub.occurrences
        .iter()
        .filter(|o| !o.grounded && !o.suppressed && !sub.is_element(o.id))
        .collect()
}

fn flatten(
    asm: &AssemblyDocument,
    sub_of: &dyn Fn(&Occurrence) -> Option<AssemblyDocument>,
) -> Flat {
    let mut flat = asm.clone();
    let mut stand_ins = HashMap::new();
    let mut by_child: HashMap<(u32, u32), u32> = HashMap::new();
    let mut holders = HashSet::new();
    let mut next_occ = STAND_IN;
    let mut next_con = STAND_IN;
    for f in asm
        .occurrences
        .iter()
        .filter(|o| o.flexible && !o.suppressed)
    {
        let Some(sub) = sub_of(f) else {
            continue;
        };
        holders.insert(f.id);
        let free: HashSet<u32> = moving(&sub).iter().map(|o| o.id).collect();
        for c in moving(&sub) {
            let place = f.child_transform(c);
            let mut o = c.clone();
            o.id = next_occ;
            o.transform = f.transform * place;
            o.grounded = false;
            o.flexible = false;
            o.flex.clear();
            flat.occurrences.push(o);
            stand_ins.insert(
                next_occ,
                StandIn {
                    holder: f.id,
                    child: c.id,
                    place,
                },
            );
            by_child.insert((f.id, c.id), next_occ);
            next_occ += 1;
        }
        // The subassembly's relationships, onto the stand-ins (grounded
        // components and the subassembly's origin: the holder itself).
        let inner = |r: &GeomRef| -> GeomRef {
            let mut r = r.clone();
            let c = r.occurrence();
            if c == relation::ASSEMBLY {
                *r.occurrence_mut() = f.id;
                r.sub_mut().clear();
            } else if free.contains(&c) {
                *r.occurrence_mut() = by_child[&(f.id, c)];
            } else {
                *r.occurrence_mut() = f.id;
                r.sub_mut().insert(0, c);
            }
            r
        };
        for c in sub.constraints.iter().filter(|c| !c.suppressed) {
            let mut k = c.clone();
            k.id = next_con;
            next_con += 1;
            k.a = inner(&c.a);
            k.b = inner(&c.b);
            k.plane = c.plane.as_ref().map(inner);
            flat.constraints.push(k);
        }
    }
    // The parent's relationships to a moving component inside: onto its
    // stand-in.
    let outer = |r: &mut GeomRef| {
        if let Some(&c) = r.sub().first()
            && let Some(&id) = by_child.get(&(r.occurrence(), c))
        {
            *r.occurrence_mut() = id;
            r.sub_mut().remove(0);
        }
    };
    for k in flat.constraints.iter_mut().filter(|k| k.id < STAND_IN) {
        outer(&mut k.a);
        outer(&mut k.b);
        if let Some(p) = k.plane.as_mut() {
            outer(p);
        }
    }
    Flat {
        asm: flat,
        stand_ins,
        holders,
    }
}

/// Solves `asm` as [`solve::solve`] does, with its flexible occurrences'
/// components moving. `sub_of` gives the subassembly a flexible occurrence
/// places (in the top file's terms); `local` reads geometry as for
/// `solve::solve`. The result's `flex` holds the components' new places.
pub fn solve(
    asm: &AssemblyDocument,
    local: &dyn Fn(&GeomRef) -> Result<Geom, String>,
    drag: Option<&Drag>,
    sub_of: &dyn Fn(&Occurrence) -> Option<AssemblyDocument>,
) -> Solved {
    if !has_flexible(asm) {
        return solve::solve(asm, local, drag.filter(|d| d.inside.is_none()));
    }
    let flat = flatten(asm, sub_of);
    if flat.stand_ins.is_empty() && flat.asm.constraints.len() == asm.constraints.len() {
        return solve::solve(asm, local, drag.filter(|d| d.inside.is_none()));
    }
    let stand_ins = &flat.stand_ins;
    let holders = &flat.holders;
    let flat_local = |r: &GeomRef| -> Result<Geom, String> {
        let id = r.occurrence();
        // The subassembly's own origin: the holder's frame.
        if holders.contains(&id)
            && r.sub().is_empty()
            && let GeomRef::Origin { item, .. } = r
        {
            return Ok(item.geom());
        }
        if let Some(s) = stand_ins.get(&id) {
            let mut back = r.clone();
            *back.occurrence_mut() = s.holder;
            back.sub_mut().insert(0, s.child);
            // Read in the holder's frame, back into the component's own.
            return Ok(local(&back)?.moved(&s.place.inverse()));
        }
        local(r)
    };
    // A drag of a component inside a flexible occurrence pulls its stand-in.
    let inner_drag = drag.and_then(|d| {
        let child = d.inside?;
        let (&id, _) = stand_ins
            .iter()
            .find(|(_, s)| s.holder == d.occurrence && s.child == child)?;
        Some(Drag {
            occurrence: id,
            inside: None,
            ..*d
        })
    });
    let drag = inner_drag.as_ref().or(drag.filter(|d| d.inside.is_none()));
    let mut s = solve::solve(&flat.asm, &flat_local, drag);
    let new_place = |id: u32| {
        s.transforms
            .iter()
            .find(|(k, _)| *k == id)
            .map(|(_, t)| *t)
            .or_else(|| asm.find(id).map(|o| o.transform))
            .unwrap_or_else(Transform3::identity)
    };
    let mut flex = Vec::new();
    for (id, t) in &s.transforms {
        if let Some(st) = stand_ins.get(id) {
            let holder = new_place(st.holder);
            flex.push((
                st.holder,
                FlexPlace {
                    child: st.child,
                    transform: holder.inverse() * t,
                },
            ));
        }
    }
    s.flex = flex;
    s.transforms.retain(|(id, _)| *id < STAND_IN);
    s.dof.retain(|id, _| *id < STAND_IN);
    s.free.retain(|id, _| *id < STAND_IN);
    // A subassembly's own relationship that cannot be met is named on its
    // holder (its id belongs to the subassembly, not to this assembly).
    let mut sick = Vec::new();
    for (id, why) in std::mem::take(&mut s.sick) {
        if id < STAND_IN {
            sick.push((id, why));
        }
    }
    s.sick = sick;
    s
}

/// Writes solve result `s` into `asm`: the moved components' placements and
/// the flexible components' places inside their occurrences.
pub fn apply(s: &Solved, asm: &mut AssemblyDocument) {
    for (id, t) in &s.transforms {
        if let Some(o) = asm.find_mut(*id) {
            o.transform = *t;
        }
    }
    for (holder, place) in &s.flex {
        if let Some(o) = asm.find_mut(*holder) {
            match o.flex.iter_mut().find(|f| f.child == place.child) {
                Some(f) => f.transform = place.transform,
                None => o.flex.push(*place),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::relation::{Constraint, ConstraintKind, OriginItem};
    use crate::assembly::tree;
    use crate::document::{DocRef, DocumentData, Project};
    use crate::kernel::{Real, Vector3};
    use crate::part::PartDocument;

    /// Box1 10 × 10 × 10 parts. Sub: a grounded Base and a free Slider whose
    /// XY plane is flush with Base's (it slides in X and Y, turns about Z).
    /// Top: Sub placed once (grounded) and a grounded Stop 50 mm along X.
    fn project() -> (
        Project,
        crate::document::DocumentId,
        crate::document::DocumentId,
    ) {
        let mut p = Project::new("T");
        let part = PartDocument::with_box("Box1", [10.0, 10.0, 10.0]).unwrap();
        let bx = p.add_document("Box", DocumentData::Part(part));
        let mut sub = AssemblyDocument::default();
        let base = sub.place(DocRef::local(bx), "Base", Transform3::identity(), true);
        let slider = sub.place(
            DocRef::local(bx),
            "Slider",
            Transform3::translation(20.0, 0.0, 0.0),
            false,
        );
        let origin = |occurrence, item| GeomRef::Origin {
            occurrence,
            sub: Vec::new(),
            item,
        };
        for (a, b, item) in [
            (base, slider, OriginItem::XyPlane),
            (base, slider, OriginItem::XzPlane),
        ] {
            let mut c = Constraint::new(ConstraintKind::Flush, origin(a, item), origin(b, item));
            c.id = sub.constraints.len() as u32 + 1;
            sub.constraints.push(c);
        }
        let sid = p.add_document("Sub", DocumentData::Assembly(sub));
        let mut top = AssemblyDocument::default();
        top.place(DocRef::local(sid), "Sub", Transform3::identity(), true);
        top.place(
            DocRef::local(bx),
            "Stop",
            Transform3::translation(50.0, 0.0, 0.0),
            true,
        );
        let tid = p.add_document("Top", DocumentData::Assembly(top));
        (p, sid, tid)
    }

    fn top(p: &Project, id: crate::document::DocumentId) -> AssemblyDocument {
        match &p.document(id).unwrap().data {
            DocumentData::Assembly(a) => a.clone(),
            _ => panic!("not an assembly"),
        }
    }

    /// A Mate of the Slider's YZ plane (inside Sub:1) with the Stop's at 0:
    /// the Slider moves 30 mm onto it only when Sub:1 is flexible.
    fn mate_to_stop(a: &mut AssemblyDocument) {
        let mut c = Constraint::new(
            ConstraintKind::Flush,
            GeomRef::Origin {
                occurrence: 1,
                sub: vec![2],
                item: OriginItem::YzPlane,
            },
            GeomRef::Origin {
                occurrence: 2,
                sub: Vec::new(),
                item: OriginItem::YzPlane,
            },
        );
        c.id = a.constraints.len() as u32 + 1;
        a.constraints.push(c);
    }

    fn solve_in(p: &Project, a: &AssemblyDocument) -> Solved {
        let linked = tree::Linked::new();
        let local = |r: &GeomRef| -> Result<Geom, String> {
            let n = tree::nested(p, &linked, a, r.occurrence(), r.sub())?;
            let part = tree::part_of(p, &linked, n.component).ok_or("no part")?;
            let g = relation::resolve_in(part, r)?;
            Ok(if r.sub().is_empty() {
                g
            } else {
                g.moved(&n.at)
            })
        };
        let sub_of = |o: &Occurrence| tree::assembly_of(p, &linked, o.component).cloned();
        solve(a, &local, None, &sub_of)
    }

    fn slider_x(p: &Project, a: &AssemblyDocument) -> Real {
        let linked = tree::Linked::new();
        let n = tree::nested(p, &linked, a, 1, &[2]).unwrap();
        let world = a.find(1).unwrap().transform * n.at;
        world.translation.vector.x
    }

    #[test]
    fn a_rigid_subassembly_cannot_follow_a_mate_inside_it() {
        let (p, _, tid) = project();
        let mut a = top(&p, tid);
        mate_to_stop(&mut a);
        // Sub:1 is grounded: the Slider cannot reach the Stop.
        let s = solve_in(&p, &a);
        assert!(!s.converged);
        assert!(s.flex.is_empty());
    }

    #[test]
    fn a_flexible_subassembly_lets_its_slider_move() {
        let (p, sid, tid) = project();
        let mut a = top(&p, tid);
        mate_to_stop(&mut a);
        a.find_mut(1).unwrap().flexible = true;
        let s = solve_in(&p, &a);
        assert!(s.converged, "residual {}", s.residual);
        assert_eq!(s.flex.len(), 1);
        apply(&s, &mut a);
        assert!(
            (slider_x(&p, &a) - 50.0).abs() < 1e-6,
            "{}",
            slider_x(&p, &a)
        );
        // Its own relationships still hold (it slid, it did not climb).
        let o = a.find(1).unwrap();
        let t = o.flex[0].transform.translation.vector;
        assert!(t.z.abs() < 1e-6 && t.y.abs() < 1e-6, "{t:?}");
        // The subassembly itself is unchanged: another placement of it, or
        // the subassembly opened alone, keeps the Slider at x 20.
        let sub = top(&p, sid);
        assert!((sub.find(2).unwrap().transform.translation.vector.x - 20.0).abs() < 1e-12);
        // Flexible off: it stands where the subassembly puts it again.
        a.find_mut(1).unwrap().flexible = false;
        assert!((slider_x(&p, &a) - 20.0).abs() < 1e-9);
    }

    #[test]
    fn two_flexible_placements_stand_apart() {
        let (p, _, tid) = project();
        let mut a = top(&p, tid);
        // A second Sub 100 mm up, flexible too, with no relationship: its
        // Slider stays; the first one goes to the Stop.
        let second = a.place(
            a.find(1).unwrap().component,
            "Sub",
            Transform3::new(Vector3::new(0.0, 0.0, 100.0), Vector3::zeros()),
            true,
        );
        mate_to_stop(&mut a);
        for id in [1, second] {
            a.find_mut(id).unwrap().flexible = true;
        }
        let s = solve_in(&p, &a);
        assert!(s.converged);
        apply(&s, &mut a);
        assert!((slider_x(&p, &a) - 50.0).abs() < 1e-6);
        let linked = tree::Linked::new();
        let n = tree::nested(&p, &linked, &a, second, &[2]).unwrap();
        assert!((n.at.translation.vector.x - 20.0).abs() < 1e-6);
    }
}
