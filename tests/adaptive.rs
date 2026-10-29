//! Adaptivity (spec 3.10 §3.4, stage B) on cases from real assemblies: a
//! plate cut to fit between two walls, a spacer filling the gap between two
//! plates (and following it when the gap shrinks), a shelf taking both its
//! length and thickness from its frame, and the refusals — a size fixed by a
//! dimension, a part that is not adaptive.

use cadcraft3d::assembly::adapt::{self, Unknown};
use cadcraft3d::assembly::relation::{self, Constraint, ConstraintKind, Geom, GeomRef};
use cadcraft3d::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Project};
use cadcraft3d::kernel::{Point3, Real, Transform3};
use cadcraft3d::part::{
    ExtrudeFeature, OriginPlane, PartDocument, ProfileRef, SketchOptions, SketchPlacement,
};
use cadcraft3d::sketch::{Constraint as SkCon, Point2};

fn opts() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

/// A `w` × `d` rectangle on XY with its corner at the origin, extruded `h`.
/// `fix_w` / `fix_d`: whether the width / depth is dimensioned (else free:
/// the far side slides). The extrusion is `Extrusion1` (adaptive when
/// `adaptive_h`); the sketch is adaptive.
fn block(w: Real, d: Real, h: Real, fix_w: bool, fix_d: bool, adaptive_h: bool) -> PartDocument {
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    let sh = sk
        .add_rectangle_two_point(Point2::new(0.0, 0.0), Point2::new(w, d))
        .unwrap();
    let p = &sh.points;
    sk.set_fixed(p[0], true).unwrap();
    if fix_w {
        sk.add_constraint(SkCon::HorizontalDistance {
            a: p[0],
            b: p[1],
            value: w,
        })
        .unwrap();
    }
    if fix_d {
        sk.add_constraint(SkCon::VerticalDistance {
            a: p[1],
            b: p[2],
            value: d,
        })
        .unwrap();
    }
    let rg = sk.regions();
    let id = part.sketches[i].id.clone();
    let f = part
        .add_extrude(
            i,
            ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], h),
        )
        .unwrap();
    part.set_adaptive(&id, true);
    if adaptive_h {
        part.set_adaptive(&f, true);
    }
    part
}

/// A Ø`2r` disc at the origin extruded `h`, its distance adaptive.
fn spacer(r: Real, h: Real) -> PartDocument {
    let mut part = PartDocument::default();
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut part.sketches[i].sketch;
    let centre = sk.add_point(Point2::new(0.0, 0.0));
    sk.set_fixed(centre, true).unwrap();
    let c = sk.add_circle_at(centre, r).unwrap();
    sk.add_constraint(SkCon::Diameter {
        circle: c,
        value: 2.0 * r,
    })
    .unwrap();
    let rg = sk.regions();
    let f = part
        .add_extrude(
            i,
            ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], h),
        )
        .unwrap();
    part.set_adaptive(&f, true);
    part
}

/// The extent of the part's display points along axis `k`.
fn extent(part: &PartDocument, k: usize) -> (Real, Real) {
    let mut lo = Real::INFINITY;
    let mut hi = Real::NEG_INFINITY;
    for (_, f) in part.body.faces() {
        for q in f.mesh.iter().flat_map(|m| &m.positions) {
            lo = lo.min(q[k]);
            hi = hi.max(q[k]);
        }
    }
    (lo, hi)
}

/// The planar face of `part` whose points all have coordinate `v` on axis
/// `k`, as a selection of occurrence `o`.
fn face(part: &PartDocument, o: u32, k: usize, v: Real) -> GeomRef {
    let (name, hint) = part
        .body
        .faces()
        .find_map(|(_, f)| {
            let m = f.mesh.as_ref()?;
            if m.positions.is_empty() || !m.positions.iter().all(|q| (q[k] - v).abs() < 1e-6) {
                return None;
            }
            let n = m.positions.len() as Real;
            let c = m
                .positions
                .iter()
                .fold(Point3::origin(), |a, q| a + q.coords / n);
            Some((f.name.clone()?, [c.x, c.y, c.z]))
        })
        .unwrap_or_else(|| panic!("no face at axis {k} = {v}"));
    GeomRef::Face {
        occurrence: o,
        sub: Vec::new(),
        name,
        hint,
    }
}

fn mate(a: GeomRef, b: GeomRef) -> Constraint {
    Constraint::new(ConstraintKind::Mate, a, b)
}

/// The project's parts by occurrence, and the assembly.
struct Case {
    project: Project,
    asm: AssemblyDocument,
}

impl Case {
    fn new() -> Self {
        Self {
            project: Project::new("T"),
            asm: AssemblyDocument::default(),
        }
    }

    fn place(&mut self, part: PartDocument, name: &str, at: Transform3, grounded: bool) -> u32 {
        let d = self.project.add_document(name, DocumentData::Part(part));
        self.asm.place(DocRef::local(d), name, at, grounded)
    }

    fn doc(&self, o: u32) -> DocumentId {
        self.asm.find(o).unwrap().component.document
    }

    fn part(&self, o: u32) -> &PartDocument {
        match self.project.document(self.doc(o)).map(|d| &d.data) {
            Some(DocumentData::Part(p)) => p,
            _ => panic!("no part"),
        }
    }

    fn local(&self) -> impl Fn(&GeomRef) -> Result<Geom, String> + '_ {
        move |r: &GeomRef| relation::resolve_in(self.part(r.occurrence()), r)
    }

    /// Adapts `o`; on success writes the part and the placements back.
    fn adapt(&mut self, o: u32) -> Result<Option<Vec<Real>>, String> {
        let base = self.part(o).clone();
        let r = adapt::adapt(&self.asm, o, &base, &self.local())?;
        let Some(a) = r else { return Ok(None) };
        let d = self.doc(o);
        if let Some(DocumentData::Part(p)) = self.project.document_mut(d).map(|d| &mut d.data) {
            *p = a.part;
        }
        for (id, t) in a.solved.transforms {
            self.asm.find_mut(id).unwrap().transform = t;
        }
        Ok(Some(a.x))
    }
}

/// Two 10 mm walls with 100 mm between them (x 10..110).
fn walls(c: &mut Case) -> (u32, u32) {
    let wall = || PartDocument::with_box("Box1", [10.0, 50.0, 50.0]).unwrap();
    let a = c.place(wall(), "WallA", Transform3::identity(), true);
    let b = c.place(
        wall(),
        "WallB",
        Transform3::translation(110.0, 0.0, 0.0),
        true,
    );
    (a, b)
}

#[test]
fn a_plate_is_cut_to_fit_between_two_walls() {
    let mut c = Case::new();
    let (a, b) = walls(&mut c);
    // 80 long (free), 30 deep (dimensioned), 5 thick.
    let plate = block(80.0, 30.0, 5.0, false, true, false);
    let unknowns = adapt::unknowns(&plate);
    assert!(
        unknowns.iter().all(|u| matches!(u, Unknown::Point { .. })) && !unknowns.is_empty(),
        "{unknowns:?}"
    );
    let left = face(&plate, 0, 0, 0.0);
    let right = face(&plate, 0, 0, 80.0);
    let p = c.place(
        plate,
        "Plate",
        Transform3::translation(20.0, 5.0, 10.0),
        false,
    );
    let wa = c.part(a).clone();
    let wb = c.part(b).clone();
    let with = |g: GeomRef, o: u32| match g {
        GeomRef::Face { name, hint, .. } => GeomRef::Face {
            occurrence: o,
            sub: Vec::new(),
            name,
            hint,
        },
        g => g,
    };
    c.asm
        .add_constraint(mate(with(left, p), face(&wa, a, 0, 10.0)));
    c.asm
        .add_constraint(mate(with(right, p), face(&wb, b, 0, 0.0)));
    let x = c.adapt(p).unwrap().expect("adapted");
    assert!(x.iter().any(|v| v.abs() > 1.0), "{x:?}");
    let (lo, hi) = extent(c.part(p), 0);
    assert!(((hi - lo) - 100.0).abs() < 1e-4, "length {}", hi - lo);
    // The depth kept its dimension, the thickness its distance.
    let (lo, hi) = extent(c.part(p), 1);
    assert!(((hi - lo) - 30.0).abs() < 1e-6);
    let (lo, hi) = extent(c.part(p), 2);
    assert!(((hi - lo) - 5.0).abs() < 1e-6);
    // Solved again: the rigid solve now holds, nothing more changes.
    assert!(c.adapt(p).unwrap().is_none());
    // The adapted part regenerates the same from its own data (rule 8).
    let mut alone = c.part(p).clone();
    alone.regenerate().unwrap();
    let (lo, hi) = extent(&alone, 0);
    assert!(((hi - lo) - 100.0).abs() < 1e-4);
}

/// A base plate (z 0..10) and a cover (z `gap + 10` up), both grounded, and
/// a Ø10 spacer of height 5 between them, mated to both.
fn stack(gap: Real) -> (Case, u32, u32) {
    let mut c = Case::new();
    let slab = || PartDocument::with_box("Box1", [60.0, 60.0, 10.0]).unwrap();
    let base = c.place(slab(), "Base", Transform3::identity(), true);
    let cover = c.place(
        slab(),
        "Cover",
        Transform3::translation(0.0, 0.0, 10.0 + gap),
        true,
    );
    let s = spacer(5.0, 5.0);
    let bottom = face(&s, 0, 2, 0.0);
    let top = face(&s, 0, 2, 5.0);
    let sp = c.place(
        s,
        "Spacer",
        Transform3::translation(30.0, 30.0, 40.0),
        false,
    );
    let slab_part = c.part(base).clone();
    let re = |g: GeomRef| match g {
        GeomRef::Face { name, hint, .. } => GeomRef::Face {
            occurrence: sp,
            sub: Vec::new(),
            name,
            hint,
        },
        g => g,
    };
    c.asm
        .add_constraint(mate(re(bottom), face(&slab_part, base, 2, 10.0)));
    c.asm
        .add_constraint(mate(re(top), face(&slab_part, cover, 2, 0.0)));
    (c, sp, cover)
}

#[test]
fn a_spacer_fills_the_gap_between_two_plates() {
    let (mut c, sp, _) = stack(12.0);
    let x = c.adapt(sp).unwrap().expect("adapted");
    assert_eq!(x.len(), 1, "only the distance: {x:?}");
    assert!((x[0] - 7.0).abs() < 1e-4, "{x:?}");
    let (lo, hi) = extent(c.part(sp), 2);
    assert!(((hi - lo) - 12.0).abs() < 1e-4, "height {}", hi - lo);
    // The spacer stands on the base.
    let t = c.asm.find(sp).unwrap().transform;
    assert!(((t * Point3::new(0.0, 0.0, lo)).z - 10.0).abs() < 1e-4);
}

#[test]
fn a_spacer_follows_a_gap_that_shrinks() {
    let (mut c, sp, cover) = stack(12.0);
    c.adapt(sp).unwrap().expect("adapted");
    // The cover comes down to a 7 mm gap: the spacer shortens.
    c.asm.find_mut(cover).unwrap().transform = Transform3::translation(0.0, 0.0, 17.0);
    c.adapt(sp).unwrap().expect("adapted again");
    let (lo, hi) = extent(c.part(sp), 2);
    assert!(((hi - lo) - 7.0).abs() < 1e-4, "height {}", hi - lo);
}

#[test]
fn a_shelf_takes_its_length_and_thickness_from_the_frame() {
    // Walls 100 apart, a floor stop at z 0..10 and a ceiling stop at
    // z 18..28: the shelf (80 long, 5 thick) becomes 100 × 8.
    let mut c = Case::new();
    let (a, b) = walls(&mut c);
    let stop = || PartDocument::with_box("Box1", [120.0, 50.0, 10.0]).unwrap();
    let floor = c.place(
        stop(),
        "Floor",
        Transform3::translation(0.0, 60.0, 0.0),
        true,
    );
    let ceiling = c.place(
        stop(),
        "Ceiling",
        Transform3::translation(0.0, 60.0, 18.0),
        true,
    );
    let shelf = block(80.0, 30.0, 5.0, false, true, true);
    let faces = [
        face(&shelf, 0, 0, 0.0),
        face(&shelf, 0, 0, 80.0),
        face(&shelf, 0, 2, 0.0),
        face(&shelf, 0, 2, 5.0),
    ];
    let s = c.place(
        shelf,
        "Shelf",
        Transform3::translation(20.0, 5.0, 30.0),
        false,
    );
    let re = |g: &GeomRef| match g {
        GeomRef::Face { name, hint, .. } => GeomRef::Face {
            occurrence: s,
            sub: Vec::new(),
            name: name.clone(),
            hint: *hint,
        },
        g => g.clone(),
    };
    let (wa, wb, st) = (c.part(a).clone(), c.part(b).clone(), c.part(floor).clone());
    c.asm
        .add_constraint(mate(re(&faces[0]), face(&wa, a, 0, 10.0)));
    c.asm
        .add_constraint(mate(re(&faces[1]), face(&wb, b, 0, 0.0)));
    // The stops lie beside the shelf (y 60..110): it rests on the floor's
    // plane and the ceiling's plane rests on it.
    c.asm
        .add_constraint(mate(re(&faces[2]), face(&st, floor, 2, 10.0)));
    c.asm
        .add_constraint(mate(re(&faces[3]), face(&st, ceiling, 2, 0.0)));
    c.adapt(s).unwrap().expect("adapted");
    let p = c.part(s);
    let (x0, x1) = extent(p, 0);
    let (z0, z1) = extent(p, 2);
    assert!(((x1 - x0) - 100.0).abs() < 1e-4, "length {}", x1 - x0);
    assert!(((z1 - z0) - 8.0).abs() < 1e-4, "thickness {}", z1 - z0);
}

#[test]
fn a_dimensioned_size_does_not_adapt_and_nothing_changes() {
    // The plate's length is dimensioned (80): it cannot reach both walls.
    let mut c = Case::new();
    let (a, b) = walls(&mut c);
    let plate = block(80.0, 30.0, 5.0, true, true, false);
    assert!(adapt::unknowns(&plate).is_empty());
    let left = face(&plate, 0, 0, 0.0);
    let right = face(&plate, 0, 0, 80.0);
    let p = c.place(
        plate,
        "Plate",
        Transform3::translation(20.0, 5.0, 10.0),
        false,
    );
    let re = |g: GeomRef| match g {
        GeomRef::Face { name, hint, .. } => GeomRef::Face {
            occurrence: p,
            sub: Vec::new(),
            name,
            hint,
        },
        g => g,
    };
    let (wa, wb) = (c.part(a).clone(), c.part(b).clone());
    c.asm.add_constraint(mate(re(left), face(&wa, a, 0, 10.0)));
    c.asm.add_constraint(mate(re(right), face(&wb, b, 0, 0.0)));
    assert!(c.adapt(p).unwrap().is_none());
    let (lo, hi) = extent(c.part(p), 0);
    assert!(((hi - lo) - 80.0).abs() < 1e-6);
}

#[test]
fn a_part_with_nothing_adaptive_keeps_its_size() {
    let (mut c, sp, _) = stack(12.0);
    let d = c.doc(sp);
    if let Some(DocumentData::Part(p)) = c.project.document_mut(d).map(|d| &mut d.data) {
        p.adaptive.clear();
    }
    assert!(c.adapt(sp).unwrap().is_none());
    let (lo, hi) = extent(c.part(sp), 2);
    assert!(((hi - lo) - 5.0).abs() < 1e-6);
}

#[test]
fn a_spacer_cannot_shrink_to_nothing() {
    // The cover lies on the base (no gap): no height reaches it.
    let (mut c, sp, cover) = stack(12.0);
    c.asm.find_mut(cover).unwrap().transform = Transform3::translation(0.0, 0.0, 10.0);
    let before = extent(c.part(sp), 2);
    assert!(c.adapt(sp).is_err());
    assert_eq!(extent(c.part(sp), 2), before);
}

/// The round face of `part` named after feature `id` (a hole's or a pin's
/// wall), as a selection of occurrence `o`.
fn round(part: &PartDocument, o: u32, id: &str) -> GeomRef {
    let (name, hint) = part
        .body
        .faces()
        .find_map(|(_, f)| {
            let n = f.name.clone()?;
            let m = f.mesh.as_ref()?;
            let q = m.positions.first()?;
            let flat = (0..3).any(|k| m.positions.iter().all(|p| (p[k] - q[k]).abs() < 1e-6));
            (n.starts_with(&format!("{id}:")) && !flat).then(|| (n, [q.x, q.y, q.z]))
        })
        .unwrap_or_else(|| panic!("no round face of {id}"));
    GeomRef::Face {
        occurrence: o,
        sub: Vec::new(),
        name,
        hint,
    }
}

#[test]
fn a_mounting_hole_moves_onto_its_stud() {
    use cadcraft3d::part::{Extent, Operation};
    // A grounded 100 × 60 × 8 plate with a Ø8 hole sketched at (40, 20),
    // its centre free; a grounded Ø8 stud standing at (63, 31).
    let mut plate = block(100.0, 60.0, 8.0, true, true, false);
    plate.adaptive.clear();
    let j = plate
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .unwrap();
    let sk = &mut plate.sketches[j].sketch;
    let c = sk.add_circle(Point2::new(40.0, 20.0), 4.0).unwrap();
    sk.add_constraint(SkCon::Diameter {
        circle: c,
        value: 8.0,
    })
    .unwrap();
    let rg = sk.regions();
    let mut cut = ExtrudeFeature::new(vec![ProfileRef::of(&rg[0], rg[0].interior_point())], 1.0);
    cut.operation = Operation::Cut;
    cut.extent = Extent::ThroughAll;
    let hole = plate.add_extrude(j, cut).unwrap();
    let sid = plate.sketches[j].id.clone();
    plate.set_adaptive(&sid, true);
    let before = cadcraft3d::kernel::Real::from(adapt::unknowns(&plate).len() as u32);
    assert_eq!(before, 2.0, "the centre moves in x and y");
    let mut stud = spacer(4.0, 30.0);
    stud.adaptive.clear();
    let stud_id = stud.features[0].id().to_owned();
    let mut c2 = Case::new();
    let wall = round(&plate, 0, &hole);
    let p = c2.place(plate, "Plate", Transform3::identity(), true);
    let pin = round(&stud, 0, &stud_id);
    let s = c2.place(
        stud,
        "Stud",
        Transform3::translation(63.0, 31.0, -10.0),
        true,
    );
    let re = |g: GeomRef, o: u32| match g {
        GeomRef::Face { name, hint, .. } => GeomRef::Face {
            occurrence: o,
            sub: Vec::new(),
            name,
            hint,
        },
        g => g,
    };
    c2.asm.add_constraint(mate(re(wall, p), re(pin, s)));
    c2.adapt(p).unwrap().expect("adapted");
    // The hole's wall now stands round (63, 31).
    let part = c2.part(p);
    let w = part
        .body
        .faces()
        .find(|(_, f)| {
            f.name
                .as_deref()
                .is_some_and(|n| n.starts_with(&format!("{hole}:")))
                && {
                    let m = f.mesh.as_ref().unwrap();
                    m.positions.iter().any(|q| (q.z - 4.0).abs() > 3.0)
                }
        })
        .map(|(_, f)| f.mesh.clone().unwrap())
        .expect("the hole's wall");
    for q in &w.positions {
        let r = ((q.x - 63.0).powi(2) + (q.y - 31.0).powi(2)).sqrt();
        assert!((r - 4.0).abs() < 1e-3, "{q:?} is {r} from the stud's axis");
    }
}

#[test]
fn a_plate_fits_between_walls_standing_at_an_angle() {
    // The same plate between walls turned 30° about Z and lifted: the result
    // must not depend on where the pair stands.
    let mut c = Case::new();
    let turn = Transform3::new(
        cadcraft3d::kernel::Vector3::new(15.0, -40.0, 7.0),
        cadcraft3d::kernel::Vector3::z() * 30f64.to_radians(),
    );
    let wall = || PartDocument::with_box("Box1", [10.0, 50.0, 50.0]).unwrap();
    let a = c.place(wall(), "WallA", turn, true);
    let b = c.place(
        wall(),
        "WallB",
        turn * Transform3::translation(127.5, 0.0, 0.0),
        true,
    );
    let plate = block(80.0, 30.0, 5.0, false, true, false);
    let left = face(&plate, 0, 0, 0.0);
    let right = face(&plate, 0, 0, 80.0);
    let p = c.place(
        plate,
        "Plate",
        turn * Transform3::translation(20.0, 5.0, 10.0),
        false,
    );
    let re = |g: GeomRef| match g {
        GeomRef::Face { name, hint, .. } => GeomRef::Face {
            occurrence: p,
            sub: Vec::new(),
            name,
            hint,
        },
        g => g,
    };
    let (wa, wb) = (c.part(a).clone(), c.part(b).clone());
    c.asm.add_constraint(mate(re(left), face(&wa, a, 0, 10.0)));
    c.asm.add_constraint(mate(re(right), face(&wb, b, 0, 0.0)));
    c.adapt(p).unwrap().expect("adapted");
    let (lo, hi) = extent(c.part(p), 0);
    assert!(((hi - lo) - 117.5).abs() < 1e-4, "length {}", hi - lo);
}
