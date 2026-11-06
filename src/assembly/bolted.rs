//! Bolted Connection (spec `docs/spec/6.2-bolted-connection.md`): a bolt,
//! washers and a nut placed as one object through holes drilled in the
//! components between a start plane and a termination. GUI-free; the panel
//! is `gui/bolted.rs`.
//!
//! What the panel needs so far: the hole each plate gets for a thread size
//! (clearance holes from the standards' clearance table, the tap drill of a
//! blind connection's last component), the axis from the picks, the plate
//! stack along it (which components the bolt passes and where the
//! termination is when none is picked) and where a click goes (the smart
//! selectors).

use super::relation::Geom;
use crate::kernel::{FaceId, Point3, Real, Transform3, Vector3};
use crate::part::hole::FastenerFit;
use crate::part::standards::{ANSI_UNIFIED, ISO_METRIC, Standards};

/// Through All or Blind (spec 6.2 §3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BoltKind {
    /// Clearance holes through every component; a nut on the far side.
    #[default]
    ThroughAll,
    /// Clearance holes down to the blind start plane, a tapped hole in the
    /// last component; no nut.
    Blind,
}

/// The holes of one thread size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoleSizes {
    /// Nominal (major) diameter of the thread.
    pub major: Real,
    /// Pitch of the coarse thread.
    pub pitch: Real,
    /// Clearance hole diameter (the fit asked for).
    pub clearance: Real,
    /// Tap drill diameter (blind connection's last component).
    pub tap_drill: Real,
}

/// The clearance table's standard used for a thread type.
pub fn clearance_standard(thread: &str) -> &'static str {
    if thread == ANSI_UNIFIED {
        "ASME B18.2.8"
    } else {
        "ISO 273"
    }
}

/// The holes for size `size` (`M8`, `1/4`) of thread type `thread`, `fit`
/// clearance (Normal unless the hole row says otherwise). The first row of
/// the size is the coarse thread.
pub fn hole_sizes(st: &Standards, thread: &str, size: &str, fit: FastenerFit) -> Option<HoleSizes> {
    let table = st.thread_table(thread)?;
    let row = table.of_size(size).next()?;
    let clearance = st
        .clearance(clearance_standard(thread), size)
        .map(|c| c.diameter(fit))?;
    Some(HoleSizes {
        major: row.major,
        pitch: row.pitch,
        clearance,
        tap_drill: row.tap_drill,
    })
}

/// The thread types a bolted connection offers, in order.
pub fn thread_types(st: &Standards) -> Vec<&str> {
    [ISO_METRIC, ANSI_UNIFIED]
        .into_iter()
        .filter(|t| st.thread_table(t).is_some())
        .collect()
}

/// The sizes of thread type `thread` that have a clearance hole (the only
/// ones a connection can drill).
pub fn sizes(st: &Standards, thread: &str) -> Vec<String> {
    let Some(table) = st.thread_table(thread) else {
        return Vec::new();
    };
    table
        .sizes()
        .into_iter()
        .filter(|s| st.clearance(clearance_standard(thread), s).is_some())
        .map(str::to_owned)
        .collect()
}

/// Where the bolt's axis is on the start plane (spec 6.2 §3.2).
#[derive(Clone, Debug, PartialEq)]
pub enum Reference {
    /// Linear: at `d1`, `d2` from two straight edges, on the side of the
    /// edges where `toward` lies (the point the start plane was picked at).
    Linear {
        /// The first edge (an `Axis`).
        e1: Geom,
        /// The second edge.
        e2: Geom,
        /// Distance from the first edge.
        d1: Real,
        /// Distance from the second edge.
        d2: Real,
        /// A point of the start face inside the corner.
        toward: Point3,
    },
    /// Concentric, On Point, By Hole: through this geometry (a circle's
    /// centre, a point, a hole's axis), projected onto the start plane.
    Through(Geom),
}

/// The connection's axis: where it enters the start plane, the direction
/// into the material and the distance to the termination (the grip of a
/// through connection, the clearance depth of a blind one).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    /// On the start plane.
    pub at: Point3,
    /// Unit direction into the material (against the start face's normal).
    pub dir: Vector3,
    /// From the start plane to the termination along `dir`.
    pub length: Real,
}

const PARALLEL: Real = 1e-6;

/// The axis from the start plane, the reference and the termination (all in
/// one coordinate system). Errors are the panel's messages.
pub fn axis(start: &Geom, reference: &Reference, end: &Geom) -> Result<Axis, String> {
    let Geom::Plane { n, .. } = *start else {
        return Err("the start plane must be a planar face or a work plane".into());
    };
    let Geom::Plane { p: q, n: m } = *end else {
        return Err("the termination must be a planar face or a work plane".into());
    };
    if n.cross(&m).norm() > 1e-4 {
        return Err("the termination must be parallel to the start plane".into());
    }
    let a = entry(start, reference)?;
    let length = (a.at - q).dot(&n);
    if length <= 1e-6 {
        return Err("the termination lies behind the start plane".into());
    }
    Ok(Axis { length, ..a })
}

/// Where the axis enters the start plane and its direction, before the
/// termination is known (`length` 0): the automatic termination is looked
/// for from here.
pub fn entry(start: &Geom, reference: &Reference) -> Result<Axis, String> {
    let Geom::Plane { p, n } = *start else {
        return Err("the start plane must be a planar face or a work plane".into());
    };
    let dir = -n;
    let at = match reference {
        Reference::Through(g) => {
            let c = match *g {
                Geom::Circle { c, .. } => c,
                Geom::Point { p } => p,
                Geom::Axis { p, d } | Geom::Cylinder { p, d, .. } => {
                    if d.cross(&n).norm() > 1e-4 {
                        return Err("the hole must be square to the start plane".into());
                    }
                    p
                }
                _ => return Err("the reference gives no centre".into()),
            };
            c - n * (c - p).dot(&n)
        }
        Reference::Linear {
            e1,
            e2,
            d1,
            d2,
            toward,
        } => {
            let line = |g: &Geom| match *g {
                Geom::Axis { p, d } => Ok((p, d)),
                _ => Err("Linear needs two straight edges".to_string()),
            };
            let ((p1, u1), (p2, u2)) = (line(e1)?, line(e2)?);
            // In-plane normals of the edges, pointing to the picked side.
            let side = |pi: Point3, u: Vector3| -> Result<Vector3, String> {
                let m = n.cross(&u);
                if m.norm() < PARALLEL {
                    return Err("an edge is square to the start plane".into());
                }
                let m = m.normalize();
                Ok(if (toward - pi).dot(&m) < 0.0 { -m } else { m })
            };
            let (m1, m2) = (side(p1, u1)?, side(p2, u2)?);
            if m1.cross(&m2).norm() < 1e-4 {
                return Err("the two edges are parallel".into());
            }
            // (x - p1)·m1 = d1, (x - p2)·m2 = d2, (x - p)·n = 0.
            let a = nalgebra::Matrix3::from_rows(&[m1.transpose(), m2.transpose(), n.transpose()]);
            let b = Vector3::new(
                d1 + m1.dot(&p1.coords),
                d2 + m2.dot(&p2.coords),
                n.dot(&p.coords),
            );
            let x = a.try_inverse().ok_or("the two edges are parallel")? * b;
            Point3::from(x)
        }
    };
    Ok(Axis {
        at,
        dir,
        length: 0.0,
    })
}

// ---- the plate stack along the axis --------------------------------------

/// Where the line `o + t·d` crosses `tris` (part coordinates): every face
/// crossed, at which `t` (any sign) and whether the line enters the
/// material there (the triangle faces against `d`).
pub fn crossings(
    tris: &[(FaceId, [Point3; 3])],
    o: &Point3,
    d: &Vector3,
) -> Vec<(FaceId, Real, bool)> {
    let mut out = Vec::new();
    for (f, [a, b, c]) in tris {
        let (e1, e2) = (b - a, c - a);
        let p = d.cross(&e2);
        let det = e1.dot(&p);
        if det.abs() < 1e-14 {
            continue;
        }
        let inv = 1.0 / det;
        let s = o - a;
        let u = s.dot(&p) * inv;
        if !(-1e-9..=1.0 + 1e-9).contains(&u) {
            continue;
        }
        let q = s.cross(&e1);
        let v = d.dot(&q) * inv;
        if v < -1e-9 || u + v > 1.0 + 1e-9 {
            continue;
        }
        let t = e2.dot(&q) * inv;
        let entering = e1.cross(&e2).dot(d) < 0.0;
        out.push((*f, t, entering));
    }
    out
}

/// A component's picking triangles (face, corners; part coordinates).
pub type Triangles<'a> = &'a [(FaceId, [Point3; 3])];

/// One stretch of material of a component along the axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plate {
    /// Which component (an index the caller gave).
    pub part: usize,
    /// Where the material starts along the axis.
    pub from: Real,
    /// Where it ends.
    pub to: Real,
    /// The face the axis enters by.
    pub entry: FaceId,
    /// The face it leaves by.
    pub exit: FaceId,
}

/// The stretches of material from one component's crossings (a triangle
/// edge hit twice counts once).
pub fn intervals(part: usize, mut cross: Vec<(FaceId, Real, bool)>) -> Vec<Plate> {
    cross.sort_by(|a, b| a.1.total_cmp(&b.1));
    cross.dedup_by(|b, a| (a.1 - b.1).abs() < 1e-7 && a.2 == b.2);
    let mut out = Vec::new();
    let mut open: Option<(FaceId, Real)> = None;
    for (f, t, entering) in cross {
        match (entering, open) {
            (true, None) => open = Some((f, t)),
            (false, Some((entry, from))) => {
                if t - from > 1e-6 {
                    out.push(Plate {
                        part,
                        from,
                        to: t,
                        entry,
                        exit: f,
                    });
                }
                open = None;
            }
            _ => {}
        }
    }
    out
}

/// The plates a bolt clamps: the one starting at the start plane (`t = 0`),
/// then each one starting where the last ended; a gap over `tol` ends the
/// stack (the start plane and the termination bound the plates).
pub fn stack(plates: &[Plate], tol: Real) -> Vec<Plate> {
    let mut out: Vec<Plate> = Vec::new();
    let mut at = 0.0;
    loop {
        let next = plates
            .iter()
            .filter(|p| (p.from - at).abs() <= tol && !out.contains(p))
            .max_by(|a, b| a.to.total_cmp(&b.to))
            .copied();
        match next {
            Some(p) if p.to > at + 1e-6 => {
                at = p.to;
                out.push(p);
            }
            _ => return out,
        }
    }
}

/// The plate stack round an axis through `parts` (each a component's
/// triangles and placement): lines parallel to the axis at `probe` from it
/// (the bolt passes through a hole, so the axis itself may meet nothing);
/// the longest stack any of them finds. `t` counts from the start plane.
pub fn stack_along(
    parts: &[(Triangles<'_>, Transform3)],
    axis: &Axis,
    probe: Real,
    tol: Real,
) -> Vec<Plate> {
    let u = if axis.dir.x.abs() < 0.9 {
        axis.dir.cross(&Vector3::x())
    } else {
        axis.dir.cross(&Vector3::y())
    }
    .normalize();
    let v = axis.dir.cross(&u);
    let mut best: Vec<Plate> = Vec::new();
    let len = |s: &[Plate]| s.last().map_or(0.0, |p| p.to);
    for k in 0..8 {
        let ang = std::f64::consts::TAU * f64::from(k) / 8.0;
        let o = axis.at + (u * ang.cos() + v * ang.sin()) * probe;
        let mut plates = Vec::new();
        for (i, (tris, place)) in parts.iter().enumerate() {
            let inv = place.inverse();
            // Rigid placements: the line parameter stays a distance.
            plates.extend(intervals(i, crossings(tris, &(inv * o), &(inv * axis.dir))));
        }
        let s = stack(&plates, tol);
        if len(&s) > len(&best) + 1e-9 {
            best = s;
        }
    }
    best
}

/// The termination a stack gives when none is picked: the last plate's exit
/// face (Through All), or the last plate's entry face, where its threaded
/// hole starts (Blind: two plates at least). The plate's index, the face
/// and its distance from the start plane.
pub fn auto_end(stack: &[Plate], kind: BoltKind) -> Option<(usize, FaceId, Real)> {
    let last = stack.last()?;
    match kind {
        BoltKind::ThroughAll => Some((last.part, last.exit, last.to)),
        BoltKind::Blind if stack.len() >= 2 => Some((last.part, last.entry, last.from)),
        BoltKind::Blind => None,
    }
}

// ---- smart selectors ------------------------------------------------------

/// What the user clicked, as far as the routing cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clicked {
    /// A planar face.
    PlanarFace,
    /// A round face (a hole's or a shaft's wall).
    RoundFace,
    /// A circular edge.
    CircularEdge,
    /// A straight edge.
    StraightEdge,
    /// A point (work point, vertex).
    Point,
}

/// The four placements, in the panel's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Two edges and two distances.
    Linear,
    /// The centre of a circular edge.
    Concentric,
    /// Points.
    OnPoint,
    /// An existing hole.
    ByHole,
}

impl Placement {
    /// From the panel's index.
    pub fn from_index(i: usize) -> Self {
        match i {
            0 => Placement::Linear,
            1 => Placement::Concentric,
            2 => Placement::OnPoint,
            _ => Placement::ByHole,
        }
    }

    /// The panel's index.
    pub fn index(self) -> usize {
        self as usize
    }
}

/// Where a click goes (spec 6.2 §3.2, smart selectors).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    /// The placement to switch to (`None`: keep it).
    pub placement: Option<Placement>,
    /// What the click fills, in order. `"start*"`: the planar face the clicked
    /// edge bounds becomes the start plane; every other key takes the item itself.
    pub keys: Vec<&'static str>,
}

/// Where a click of `what` goes while selector `active` waits, in `placement`
/// (`edge1_set`: Linear Edge 1 is filled). `None`: refused (rule 19).
///
/// - Start Plane and a circular edge: its planar face is the start plane and
///   the edge the reference: Concentric (a hole's rim), or the hole in By Hole.
/// - Start Plane and a round face: By Hole, the start plane stays asked for.
/// - Start Plane and a straight edge in Linear: its face and the first edge.
/// - Circular Reference and a hole's wall: By Hole. Point and a circular edge:
///   Concentric (its centre).
pub fn route(placement: Placement, active: &str, what: Clicked, edge1_set: bool) -> Option<Route> {
    use Clicked as C;
    let keep = |keys: Vec<&'static str>| {
        Some(Route {
            placement: None,
            keys,
        })
    };
    let to = |p: Placement, keys: Vec<&'static str>| {
        Some(Route {
            placement: (p != placement).then_some(p),
            keys,
        })
    };
    match (active, what) {
        ("start", C::PlanarFace) => keep(vec!["start"]),
        ("start", C::CircularEdge) if placement == Placement::ByHole => {
            keep(vec!["start*", "hole"])
        }
        ("start", C::CircularEdge) => to(Placement::Concentric, vec!["start*", "circle"]),
        ("start", C::RoundFace) => to(Placement::ByHole, vec!["hole"]),
        ("start", C::StraightEdge) if placement == Placement::Linear => {
            keep(vec!["start*", if edge1_set { "edge2" } else { "edge1" }])
        }
        ("edge1", C::StraightEdge) => keep(vec!["edge1"]),
        ("edge2", C::StraightEdge) => keep(vec!["edge2"]),
        ("circle", C::CircularEdge) => keep(vec!["circle"]),
        ("circle", C::RoundFace) => to(Placement::ByHole, vec!["hole"]),
        ("hole", C::RoundFace | C::CircularEdge) => keep(vec!["hole"]),
        ("point", C::Point) => keep(vec!["point"]),
        ("point", C::CircularEdge) => to(Placement::Concentric, vec!["circle"]),
        ("end", C::PlanarFace) => keep(vec!["end"]),
        ("blind", C::PlanarFace) => keep(vec!["blind"]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_holes_follow_iso_273_and_the_tap_drill() {
        let st = Standards::builtin();
        let m8 = hole_sizes(&st, ISO_METRIC, "M8", FastenerFit::Normal).unwrap();
        assert_eq!(
            (m8.major, m8.pitch, m8.clearance, m8.tap_drill),
            (8.0, 1.25, 9.0, 6.75)
        );
        let m12 = hole_sizes(&st, ISO_METRIC, "M12", FastenerFit::Close).unwrap();
        assert_eq!((m12.clearance, m12.tap_drill), (13.0, 10.25));
        let m6 = hole_sizes(&st, ISO_METRIC, "M6", FastenerFit::Loose).unwrap();
        assert_eq!(m6.clearance, 7.0);
    }

    fn plane(p: [Real; 3], n: [Real; 3]) -> Geom {
        Geom::Plane {
            p: Point3::from(p),
            n: Vector3::from(n),
        }
    }

    #[test]
    fn the_axis_stands_at_the_distances_from_two_edges() {
        // A 40 mm stack of plates, top at z = 40; edges along X and Y through the origin.
        let start = plane([30.0, 30.0, 40.0], [0.0, 0.0, 1.0]);
        let end = plane([0.0, 0.0, 0.0], [0.0, 0.0, -1.0]);
        let edge = |d: [Real; 3]| Geom::Axis {
            p: Point3::new(0.0, 0.0, 40.0),
            d: Vector3::from(d),
        };
        let r = Reference::Linear {
            e1: edge([1.0, 0.0, 0.0]),
            e2: edge([0.0, 1.0, 0.0]),
            d1: 12.0,
            d2: 15.0,
            toward: Point3::new(30.0, 30.0, 40.0),
        };
        let a = axis(&start, &r, &end).unwrap();
        assert!(
            (a.at - Point3::new(15.0, 12.0, 40.0)).norm() < 1e-9,
            "{a:?}"
        );
        assert!((a.dir - Vector3::new(0.0, 0.0, -1.0)).norm() < 1e-12);
        assert!((a.length - 40.0).abs() < 1e-9);
        // The other corner: the same distances on the picked side.
        let r = Reference::Linear {
            e1: edge([1.0, 0.0, 0.0]),
            e2: edge([0.0, 1.0, 0.0]),
            d1: 12.0,
            d2: 15.0,
            toward: Point3::new(-30.0, 30.0, 40.0),
        };
        let a = axis(&start, &r, &end).unwrap();
        assert!(
            (a.at - Point3::new(-15.0, 12.0, 40.0)).norm() < 1e-9,
            "{a:?}"
        );
    }

    #[test]
    fn a_centre_is_projected_and_bad_terminations_are_refused() {
        let start = plane([0.0, 0.0, 10.0], [0.0, 0.0, 1.0]);
        let c = Reference::Through(Geom::Circle {
            c: Point3::new(5.0, 6.0, 0.0),
            n: Vector3::z(),
            r: 4.5,
        });
        let a = axis(&start, &c, &plane([0.0, 0.0, -5.0], [0.0, 0.0, -1.0])).unwrap();
        assert!(
            (a.at - Point3::new(5.0, 6.0, 10.0)).norm() < 1e-12 && (a.length - 15.0).abs() < 1e-12
        );
        let tilted = plane([0.0, 0.0, 0.0], [0.0, 0.6, -0.8]);
        assert!(axis(&start, &c, &tilted).unwrap_err().contains("parallel"));
        let above = plane([0.0, 0.0, 20.0], [0.0, 0.0, 1.0]);
        assert!(axis(&start, &c, &above).unwrap_err().contains("behind"));
    }

    // ---- use cases: the plate stack -------------------------------------

    use crate::assembly::PickMesh;
    use crate::part::PartDocument;

    /// A box part's picking triangles and its name lookup.
    fn plate(size: [Real; 3]) -> (PickMesh, PartDocument) {
        let p = PartDocument::with_box("Box1", size).unwrap();
        (PickMesh::of(&p), p)
    }

    /// A plate with one Ø`d` through hole at (x, y).
    fn holed(size: [Real; 3], at: (Real, Real), d: Real) -> (PickMesh, PartDocument) {
        use crate::part::hole::{HoleCenter, HoleFeature, HoleSpec, Termination};
        let mut p = PartDocument::with_box("Box1", size).unwrap();
        let (i, ids) = p
            .create_hole_sketch("Box1:Top", &[Point3::new(at.0, at.1, size[2])])
            .unwrap();
        let mut spec = HoleSpec::simple(d, size[2]);
        spec.termination = Termination::ThroughAll;
        let mut def = HoleFeature::new(spec);
        def.sketch = p.sketches[i].id.clone();
        def.centers = ids.into_iter().map(HoleCenter::Point).collect();
        p.add_hole(def).unwrap();
        (PickMesh::of(&p), p)
    }

    fn face_name(p: &PartDocument, f: FaceId) -> String {
        p.body
            .face(f)
            .and_then(|x| x.name.clone())
            .unwrap_or_default()
    }

    fn down_at(x: Real, y: Real, z: Real) -> Axis {
        Axis {
            at: Point3::new(x, y, z),
            dir: -Vector3::z(),
            length: 0.0,
        }
    }

    /// Use case 1: two 10 mm plates, Through All: both clamped, the grip is
    /// 20 mm and the termination found is the lower plate's bottom.
    #[test]
    fn use_case_two_plates_through_all() {
        let (m, p) = plate([120.0, 80.0, 10.0]);
        let parts = [
            (m.triangles(), Transform3::identity()),
            (m.triangles(), Transform3::translation(0.0, 0.0, 10.0)),
        ];
        let s = stack_along(&parts, &down_at(60.0, 40.0, 20.0), 6.0, 0.05);
        assert_eq!(s.iter().map(|x| x.part).collect::<Vec<_>>(), vec![1, 0]);
        let (part, face, t) = auto_end(&s, BoltKind::ThroughAll).unwrap();
        assert_eq!((part, face_name(&p, face).as_str()), (0, "Box1:Bottom"));
        assert!((t - 20.0).abs() < 1e-9);
    }

    /// Use case 2: three plates, the top one already drilled at the axis
    /// (Concentric on its hole): the lines beside the axis still see it.
    #[test]
    fn use_case_three_plates_the_top_one_drilled() {
        let (m, _) = plate([120.0, 80.0, 10.0]);
        let (h, _) = holed([120.0, 80.0, 10.0], (15.0, 15.0), 9.0);
        let parts = [
            (m.triangles(), Transform3::identity()),
            (m.triangles(), Transform3::translation(0.0, 0.0, 10.0)),
            (h.triangles(), Transform3::translation(0.0, 0.0, 20.0)),
        ];
        let axis = down_at(15.0, 15.0, 30.0);
        // On the axis itself the top plate is a hole: the stack would start nowhere.
        let on_axis = stack_along(&parts, &axis, 0.0, 0.05);
        assert!(on_axis.is_empty(), "{on_axis:?}");
        let s = stack_along(&parts, &axis, 6.0, 0.05);
        assert_eq!(s.iter().map(|x| x.part).collect::<Vec<_>>(), vec![2, 1, 0]);
        assert!((s[2].to - 30.0).abs() < 1e-9);
    }

    /// Use case 3: Blind, a 10 mm plate on a 40 mm block: the threaded hole
    /// starts on the block's top, 10 mm below the start plane.
    #[test]
    fn use_case_blind_into_a_thick_block() {
        let (m, _) = plate([120.0, 80.0, 10.0]);
        let (b, bp) = plate([120.0, 80.0, 40.0]);
        let parts = [
            (b.triangles(), Transform3::identity()),
            (m.triangles(), Transform3::translation(0.0, 0.0, 40.0)),
        ];
        let s = stack_along(&parts, &down_at(30.0, 30.0, 50.0), 6.0, 0.05);
        let (part, face, t) = auto_end(&s, BoltKind::Blind).unwrap();
        assert_eq!((part, face_name(&bp, face).as_str()), (0, "Box1:Top"));
        assert!((t - 10.0).abs() < 1e-9);
        // One plate alone cannot be blind.
        assert_eq!(auto_end(&s[..1], BoltKind::Blind), None);
    }

    /// Use case 4: a third plate 5 mm below the others is not clamped (the
    /// gap ends the stack), and a stack that does not start at the start
    /// plane is none.
    #[test]
    fn use_case_a_gap_ends_the_stack() {
        let (m, _) = plate([120.0, 80.0, 10.0]);
        let parts = [
            (m.triangles(), Transform3::translation(0.0, 0.0, -15.0)),
            (m.triangles(), Transform3::identity()),
            (m.triangles(), Transform3::translation(0.0, 0.0, 10.0)),
        ];
        let s = stack_along(&parts, &down_at(60.0, 40.0, 20.0), 6.0, 0.05);
        assert_eq!(s.len(), 2);
        assert!((s[1].to - 20.0).abs() < 1e-9);
        let above = stack_along(&parts, &down_at(60.0, 40.0, 25.0), 6.0, 0.05);
        assert!(above.is_empty());
    }

    /// Use case 5: the axis close to a plate's edge (Linear, 4 mm from it):
    /// some lines miss the plate, the stack is still found; plates of
    /// different sizes, offset sideways.
    #[test]
    fn use_case_near_an_edge_and_offset_plates() {
        let (a, _) = plate([120.0, 80.0, 10.0]);
        let (b, _) = plate([60.0, 30.0, 8.0]);
        let parts = [
            (a.triangles(), Transform3::identity()),
            (b.triangles(), Transform3::translation(0.0, 0.0, 10.0)),
        ];
        let s = stack_along(&parts, &down_at(4.0, 4.0, 18.0), 6.0, 0.05);
        assert_eq!(s.len(), 2);
        assert!((s[1].to - 18.0).abs() < 1e-9);
    }

    /// Use case 6: the placements give the same axis from different picks:
    /// By Hole (the hole's wall, a cylinder), Concentric (its rim), On Point.
    #[test]
    fn use_case_every_placement_finds_the_hole() {
        let start = plane([0.0, 0.0, 20.0], [0.0, 0.0, 1.0]);
        let end = plane([0.0, 0.0, 0.0], [0.0, 0.0, -1.0]);
        let c = Point3::new(15.0, 15.0, 20.0);
        let refs = [
            Reference::Through(Geom::Cylinder {
                p: Point3::new(15.0, 15.0, 7.0),
                d: -Vector3::z(),
                r: 4.5,
            }),
            Reference::Through(Geom::Circle {
                c,
                n: Vector3::z(),
                r: 4.5,
            }),
            Reference::Through(Geom::Point {
                p: Point3::new(15.0, 15.0, 3.0),
            }),
        ];
        for r in &refs {
            let a = axis(&start, r, &end).unwrap();
            assert!(
                (a.at - c).norm() < 1e-9 && (a.length - 20.0).abs() < 1e-9,
                "{r:?}"
            );
        }
        // An inclined hole cannot carry a bolt square to the plate.
        let tilted = Reference::Through(Geom::Axis {
            p: c,
            d: Vector3::new(0.0, 0.6, 0.8),
        });
        assert!(axis(&start, &tilted, &end).unwrap_err().contains("square"));
        // Two parallel edges give no corner.
        let e = |y: Real| Geom::Axis {
            p: Point3::new(0.0, y, 20.0),
            d: Vector3::x(),
        };
        let lin = Reference::Linear {
            e1: e(0.0),
            e2: e(80.0),
            d1: 10.0,
            d2: 10.0,
            toward: c,
        };
        assert!(axis(&start, &lin, &end).unwrap_err().contains("parallel"));
    }

    /// Use case 7: the smart selectors — a hole's rim clicked for the start
    /// plane gives the start plane, the circle and Concentric at once; its
    /// wall gives By Hole; wrong kinds are refused.
    #[test]
    fn use_case_smart_selectors() {
        use Clicked as C;
        use Placement as P;
        let r = route(P::Linear, "start", C::CircularEdge, false).unwrap();
        assert_eq!(r.placement, Some(P::Concentric));
        assert_eq!(r.keys, vec!["start*", "circle"]);
        let r = route(P::ByHole, "start", C::CircularEdge, false).unwrap();
        assert_eq!((r.placement, r.keys), (None, vec!["start*", "hole"]));
        assert_eq!(
            route(P::Concentric, "start", C::RoundFace, false)
                .unwrap()
                .placement,
            Some(P::ByHole)
        );
        let r = route(P::Linear, "start", C::StraightEdge, true).unwrap();
        assert_eq!(r.keys, vec!["start*", "edge2"]);
        assert_eq!(route(P::Concentric, "start", C::StraightEdge, false), None);
        assert_eq!(route(P::Linear, "end", C::CircularEdge, false), None);
        assert_eq!(route(P::Concentric, "circle", C::PlanarFace, false), None);
        assert_eq!(
            route(P::OnPoint, "point", C::CircularEdge, false)
                .unwrap()
                .placement,
            Some(P::Concentric)
        );
        for p in [P::Linear, P::Concentric, P::OnPoint, P::ByHole] {
            assert_eq!(P::from_index(p.index()), p);
        }
    }

    #[test]
    fn every_offered_size_has_its_holes() {
        let st = Standards::builtin();
        for t in thread_types(&st) {
            let all = sizes(&st, t);
            assert!(all.len() > 5, "{t}: {all:?}");
            for s in &all {
                let h = hole_sizes(&st, t, s, FastenerFit::Normal).unwrap();
                assert!(
                    h.clearance > h.major && h.tap_drill < h.major,
                    "{t} {s}: {h:?}"
                );
            }
        }
        assert!(sizes(&st, ISO_METRIC).iter().any(|s| s == "M8"));
    }
}
