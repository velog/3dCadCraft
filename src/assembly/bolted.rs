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

use serde::{Deserialize, Serialize};

use super::relation::{Geom, GeomRef};
use crate::kernel::{FaceId, Point3, Real, Transform3, Vector3};
use crate::part::bolt_hole::BoltHoleOp;
use crate::part::hole::FastenerFit;
use crate::part::standards::{ANSI_UNIFIED, ISO_METRIC, Standards};

/// Through All or Blind (spec 6.2 §3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
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
                // A vertex is a frame on its face: its point.
                Geom::Point { p } | Geom::Frame { p, .. } => p,
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

/// How many bolts one connection places from its reference (spec 6.2 §3.2c).
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub enum Spread {
    /// One bolt per reference.
    #[default]
    One,
    /// Linear as a pattern: `count[0]` bolts `spacing[0]` apart along Linear
    /// Edge 1 (away from Edge 2), `count[1]` along Edge 2 (away from Edge 1).
    Grid {
        /// Bolts in each direction (at least 1).
        count: [u32; 2],
        /// Their spacing in each direction.
        spacing: [Real; 2],
    },
    /// Circular: `count` bolts on a circle of `radius` about the reference's
    /// centre, the first at `angle` (radians, from the start plane's local X,
    /// counter-clockwise seen from outside the material), spread evenly round
    /// the circle when `span` is a full turn, else the first and the last
    /// `span` apart.
    Circle {
        /// Bolts.
        count: u32,
        /// Bolt circle radius.
        radius: Real,
        /// The first bolt's angle.
        angle: Real,
        /// Full turn or the first-to-last angle.
        span: Real,
    },
}

/// The start plane's local X (the world X projected onto it; the world Y on a
/// plane square to X): the zero of a bolt circle's angle.
pub fn plane_x(n: &Vector3) -> Vector3 {
    let x = Vector3::x() - n * n.dot(&Vector3::x());
    if x.norm() > 1e-3 {
        x.normalize()
    } else {
        (Vector3::y() - n * n.dot(&Vector3::y())).normalize()
    }
}

/// Every bolt's entry of `reference` spread by `spread` (the first is
/// [`entry`]'s). Linear's grid steps along its edges, away from the other
/// edge; a circle goes round the reference's centre.
pub fn entries(start: &Geom, reference: &Reference, spread: &Spread) -> Result<Vec<Axis>, String> {
    let first = entry(start, reference)?;
    let Geom::Plane { n, .. } = *start else {
        return Err("the start plane must be a planar face or a work plane".into());
    };
    match (*spread, reference) {
        (Spread::One, _) => Ok(vec![first]),
        (Spread::Grid { count, spacing }, Reference::Linear { e1, e2, .. }) => {
            let dir = |g: &Geom| match *g {
                Geom::Axis { d, .. } => Ok((d - n * n.dot(&d)).normalize()),
                _ => Err("Linear needs two straight edges".to_string()),
            };
            let (mut u1, mut u2) = (dir(e1)?, dir(e2)?);
            // Away from the other edge: towards where the first bolt is from
            // the other edge's line.
            let away = |u: Vector3, g: &Geom| match *g {
                Geom::Axis { p, .. } if (first.at - p).dot(&u) < 0.0 => -u,
                _ => u,
            };
            u1 = away(u1, e2);
            u2 = away(u2, e1);
            if count[0] == 0 || count[1] == 0 {
                return Err("a count must be 1 or more".into());
            }
            let mut v = Vec::new();
            for j in 0..count[1] {
                for i in 0..count[0] {
                    let at = first.at
                        + u1 * (spacing[0] * f64::from(i))
                        + u2 * (spacing[1] * f64::from(j));
                    v.push(Axis { at, ..first });
                }
            }
            Ok(v)
        }
        (Spread::Grid { .. }, _) => Err("a grid needs the Linear placement".into()),
        (
            Spread::Circle {
                count,
                radius,
                angle,
                span,
            },
            _,
        ) => {
            if count == 0 {
                return Err("a count must be 1 or more".into());
            }
            if radius <= 1e-6 {
                return Err("the bolt circle needs a radius".into());
            }
            let x = plane_x(&n);
            let y = n.cross(&x);
            let full = (span.abs() - std::f64::consts::TAU).abs() < 1e-6;
            let step = if full {
                std::f64::consts::TAU / f64::from(count)
            } else if count > 1 {
                span / f64::from(count - 1)
            } else {
                0.0
            };
            Ok((0..count)
                .map(|k| {
                    let a = angle + step * f64::from(k);
                    Axis {
                        at: first.at + (x * a.cos() + y * a.sin()) * radius,
                        ..first
                    }
                })
                .collect())
        }
    }
}

/// Every bolt's axis: [`entries`] of each reference, all ending on the
/// termination (parallel planes: the same length for each).
pub fn axes(
    start: &Geom,
    references: &[Reference],
    spread: &Spread,
    end: &Geom,
) -> Result<Vec<Axis>, String> {
    let mut v = Vec::new();
    for r in references {
        let a = axis(start, r, end)?;
        for e in entries(start, r, spread)? {
            v.push(Axis {
                length: a.length,
                ..e
            });
        }
    }
    if v.is_empty() {
        return Err("pick the reference".into());
    }
    for (i, a) in v.iter().enumerate() {
        if v[..i].iter().any(|b| (b.at - a.at).norm() < 1e-3) {
            return Err("two bolts would stand on the same axis".into());
        }
    }
    Ok(v)
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

/// Whether the axis itself runs through material of plate `p` (of component
/// `parts[p.part]`): `false` means the plate already has a hole there, which
/// the connection does not cut again.
pub fn solid_on_axis(parts: &[(Triangles<'_>, Transform3)], axis: &Axis, p: &Plate) -> bool {
    let Some((tris, place)) = parts.get(p.part) else {
        return false;
    };
    let inv = place.inverse();
    let eps = 1e-3;
    intervals(p.part, crossings(tris, &(inv * axis.at), &(inv * axis.dir)))
        .iter()
        .any(|q| q.from < p.to - eps && q.to > p.from + eps)
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Placement {
    /// Two edges and two distances.
    Linear,
    /// The centre of a circular edge.
    Concentric,
    /// Points.
    OnPoint,
    /// An existing hole.
    ByHole,
    /// A bolt circle about a circular reference (ours, for flanges).
    Circular,
}

impl Placement {
    /// From the panel's index.
    pub fn from_index(i: usize) -> Self {
        match i {
            0 => Placement::Linear,
            1 => Placement::Concentric,
            2 => Placement::OnPoint,
            3 => Placement::ByHole,
            _ => Placement::Circular,
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
        // Circular: a rim or a boss's wall is the bolt circle's centre.
        ("start", C::CircularEdge) if placement == Placement::Circular => {
            keep(vec!["start*", "circle"])
        }
        ("start", C::RoundFace) if placement == Placement::Circular => keep(vec!["circle"]),
        ("circle", C::RoundFace) if placement == Placement::Circular => keep(vec!["circle"]),
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

// ---- the connection --------------------------------------------------------

/// What a fastener is in the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    /// The bolt or screw (one, first on the head side).
    Bolt,
    /// A washer.
    Washer,
    /// A nut.
    Nut,
}

impl Role {
    /// The role of a library category (`Fasteners/Bolts/Hex Head`).
    pub fn of(category: &str) -> Option<Role> {
        let c = category.strip_prefix("Fasteners/")?;
        if c.starts_with("Bolts") {
            Some(Role::Bolt)
        } else if c.starts_with("Washers") {
            Some(Role::Washer)
        } else if c.starts_with("Nuts") {
            Some(Role::Nut)
        } else {
            None
        }
    }

    /// Its name in the panel.
    pub fn title(self) -> &'static str {
        match self {
            Role::Bolt => "Bolt",
            Role::Washer => "Washer",
            Role::Nut => "Nut",
        }
    }
}

/// One fastener of a connection: an ordinary occurrence of a library member.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoltFastener {
    /// Its occurrence.
    pub occurrence: u32,
    /// Which bolt of the connection (0 = the first; spec 6.2 §3.2c).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub instance: u32,
    /// On the nut side (below the termination), else the head side.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub nut_side: bool,
    /// What it is.
    pub role: Role,
    /// The library family's name (the member follows from the size).
    pub family: String,
    /// The family's standard.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub standard: String,
}

/// A bolted connection (spec 6.2 §4, `AssemblyDocument::bolts`, format
/// revision 66): its inputs, so Edit opens them again, and what it made —
/// the fasteners (in stack order: the head side from the head down, then
/// the nut side from the plates down) and the holes (bodies of the tools
/// part, spec 3.10 §3.8).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoltedConnection {
    /// Never reused within the assembly.
    pub id: u32,
    /// Browser name (`Bolted Connection 1`).
    pub name: String,
    /// Through All or Blind.
    #[serde(default)]
    pub kind: BoltKind,
    /// How the axis is placed.
    pub placement: Placement,
    /// The start plane.
    pub start: GeomRef,
    /// The placement's references: two edges (Linear), the circle, the
    /// points or the hole.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<GeomRef>,
    /// Linear's distances from the two edges.
    #[serde(default)]
    pub distances: [Real; 2],
    /// How many bolts the references give (a grid, a bolt circle).
    #[serde(default, skip_serializing_if = "is_one")]
    pub spread: Spread,
    /// The clearance holes' fit.
    #[serde(default, skip_serializing_if = "is_normal")]
    pub fit: FastenerFit,
    /// The bolt length the user chose (`None`: the shortest that works).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<Real>,
    /// The termination (Through All) or the blind start plane.
    pub end: GeomRef,
    /// The termination was found along the axis (not picked).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub end_auto: bool,
    /// The thread type (`ISO Metric profile`).
    pub thread: String,
    /// The size (`M8`).
    pub size: String,
    /// The fasteners, in stack order.
    pub fasteners: Vec<BoltFastener>,
    /// The holes: feature ids (= body names) in the tools part (each bolt's
    /// axis, which its fasteners mate to; they cut nothing).
    pub holes: Vec<String>,
    /// The Hole features written into the plates' own parts: the part and
    /// the feature's id (spec 6.2 §4, owner 2026-11-07).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drilled: Vec<(crate::document::DocRef, String)>,
    /// The fasteners may turn about their axis (Lock Rotation off). Off by
    /// default: each fastener's axis Mate holds its turn (owner 2026-11-08).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub free_turn: bool,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

fn is_one(s: &Spread) -> bool {
    *s == Spread::One
}

fn is_normal(f: &FastenerFit) -> bool {
    *f == FastenerFit::Normal
}

/// What a hole of a connection cuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoleUse {
    /// The clearance hole through the plates (all but a blind one's last).
    Clearance,
    /// A blind connection's tapped hole in its last component.
    Tapped,
}

/// Where the fasteners go along the axis and which bolt length suits.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// Head side, in stack order (the bolt first): each seat's height above
    /// the start plane.
    pub head: Vec<Real>,
    /// Nut side, from the plates down: each seat's depth below the
    /// termination.
    pub nut: Vec<Real>,
    /// The shortest bolt (below its seat) that works.
    pub min_length: Real,
    /// The longest one (a blind hole's bottom), if any.
    pub max_length: Option<Real>,
}

/// The seats of a stack: `head` and `nut` are the thicknesses of each
/// fastener along the axis in stack order (the bolt's own entry is ignored;
/// `sink` is how far its seat sinks below the next one, a countersunk head).
/// `grip` is the stack of plates (Through All) or the plates above the blind
/// one; `last` that one's thickness; `major` the thread's diameter.
pub fn layout(
    kind: BoltKind,
    grip: Real,
    last: Real,
    major: Real,
    head: &[Real],
    sink: Real,
    nut: &[Real],
) -> Layout {
    // Head side: the last row sits on the plate, the ones above on it.
    let mut heights = vec![0.0; head.len()];
    let mut h = 0.0;
    for k in (1..head.len()).rev() {
        heights[k] = h;
        h += head[k];
    }
    if let Some(b) = heights.first_mut() {
        *b = h - sink;
    }
    let mut depths = Vec::with_capacity(nut.len());
    let mut t = 0.0;
    for x in nut {
        depths.push(t);
        t += x;
    }
    let under_head = h - sink + grip;
    match kind {
        // Some thread past the nut (as Smart Drop's stack).
        BoltKind::ThroughAll => Layout {
            head: heights,
            nut: depths,
            min_length: under_head + t + 0.3 * major,
            max_length: None,
        },
        // At least one diameter into the tapped part, never through it.
        BoltKind::Blind => Layout {
            head: heights,
            nut: Vec::new(),
            min_length: under_head + major.min(last),
            max_length: Some(under_head + last),
        },
    }
}

/// The shortest of `lengths` from `min` up to `max`.
pub fn choose_length(lengths: &[Real], min: Real, max: Option<Real>) -> Option<usize> {
    lengths
        .iter()
        .enumerate()
        .filter(|(_, l)| **l >= min - 1e-6 && max.is_none_or(|m| **l <= m + 1e-6))
        .min_by(|a, b| a.1.total_cmp(b.1))
        .map(|(k, _)| k)
}

/// The tools of a connection along `axis` (assembly coordinates): Through
/// All one clearance cylinder through the grip; Blind the clearance down to
/// the blind plane (`axis.length`) and the tap drill `depth` into the last
/// component. Each starts and ends 1 mm in the air (or in a component it does
/// not cut), so no face of a tool is flush with a plate.
pub fn hole_ops(
    axis: &Axis,
    kind: BoltKind,
    h: &HoleSizes,
    depth: Real,
) -> Vec<(BoltHoleOp, HoleUse)> {
    let op = |diameter: Real, from: Real, to: Real| BoltHoleOp {
        origin: [axis.at.x, axis.at.y, axis.at.z],
        dir: [axis.dir.x, axis.dir.y, axis.dir.z],
        diameter,
        from,
        to,
    };
    let air = 1.0;
    let clearance = (op(h.clearance, -air, axis.length + air), HoleUse::Clearance);
    match kind {
        BoltKind::ThroughAll => vec![clearance],
        BoltKind::Blind => vec![
            clearance,
            (
                op(h.tap_drill, axis.length - air, axis.length + depth),
                HoleUse::Tapped,
            ),
        ],
    }
}

/// How deep a blind connection's tapped hole goes: half a diameter past the
/// end of a `bolt` long bolt, never past the component (`last`).
pub fn tapped_depth(bolt: Real, layout: &Layout, grip: Real, major: Real, last: Real) -> Real {
    let into = bolt - (layout.head.first().copied().unwrap_or(0.0) + grip);
    (into + 0.5 * major).min(last)
}

// ---- making and removing -------------------------------------------------------

/// One fastener to place: its component, browser name, side and seat
/// (distance from the plate along the axis; head side above the start
/// plane, nut side below the termination).
#[derive(Clone, Debug, PartialEq)]
pub struct Placing {
    /// The member's document (linked file or a part of the project).
    pub component: crate::document::DocRef,
    /// Browser name (`ISO 4017 M8 x 30`).
    pub name: String,
    /// Below the termination.
    pub nut_side: bool,
    /// The seat's distance from its plate.
    pub offset: Real,
}

/// One bolt of a connection: its axis and its holes, each with the
/// occurrence paths it cuts (a plate already drilled there is left out; the
/// clearance hole is made even when it cuts nothing: the fasteners mate to
/// its wall).
#[derive(Clone, Debug)]
pub struct Instance {
    /// The axis.
    pub axis: Axis,
    /// The clearance hole first, then a blind connection's tapped one.
    pub holes: Vec<(BoltHoleOp, Vec<Vec<u32>>)>,
}

impl Instance {
    /// A bolt of `axis` with the holes of [`hole_ops`]: the clearance hole
    /// cuts `clearance`, the tapped one `tapped`.
    pub fn new(
        axis: Axis,
        holes: Vec<(BoltHoleOp, HoleUse)>,
        clearance: &[Vec<u32>],
        tapped: &[Vec<u32>],
    ) -> Self {
        Instance {
            axis,
            holes: holes
                .into_iter()
                .map(|(op, used)| {
                    let paths = match used {
                        HoleUse::Clearance => clearance.to_vec(),
                        HoleUse::Tapped => tapped.to_vec(),
                    };
                    (op, paths)
                })
                .collect(),
        }
    }
}

/// A Hole feature a connection writes into a plate's part: on `face` (the
/// face the bolt enters by, part coordinates), one centre per bolt.
#[derive(Clone, Debug, PartialEq)]
pub struct Drill {
    /// The plate's part.
    pub component: crate::document::DocRef,
    /// Its face the holes start on.
    pub face: String,
    /// The centres on that face (part coordinates).
    pub centers: Vec<Point3>,
    /// Clearance (Through All) or tapped (a depth).
    pub spec: crate::part::hole::HoleSpec,
}

/// A clearance hole of `diameter` through the part.
pub fn clearance_spec(diameter: Real) -> crate::part::hole::HoleSpec {
    let mut s = crate::part::hole::HoleSpec::simple(diameter, 1.0);
    s.hole_type = crate::part::hole::HoleType::Clearance;
    s.termination = crate::part::hole::Termination::ThroughAll;
    s
}

/// A tapped hole of size `size` of thread type `thread`, `depth` deep (a
/// cosmetic thread over its whole depth).
pub fn tapped_spec(
    st: &Standards,
    thread: &str,
    size: &str,
    depth: Real,
) -> Option<crate::part::hole::HoleSpec> {
    let designation = st
        .thread_table(thread)?
        .of_size(size)
        .next()?
        .designation
        .clone();
    let t = crate::part::hole::ThreadSpec::from_table(st, thread, &designation, true)?;
    Some(crate::part::hole::HoleSpec::tapped(t, depth))
}

/// Writes `d` into `part` as one Hole feature (its own sketch of centre
/// points on the face). Returns the feature's id; on failure the part is as
/// it was.
pub fn drill(part: &mut crate::part::PartDocument, d: &Drill) -> Result<String, String> {
    use crate::part::hole::{HoleCenter, HoleFeature};
    let before = part.clone();
    let res = (|| {
        let (i, ids) = part
            .create_hole_sketch(&d.face, &d.centers)
            .map_err(|e| e.to_string())?;
        let mut def = HoleFeature::new(d.spec.clone());
        def.sketch = part.sketches[i].id.clone();
        def.centers = ids.into_iter().map(HoleCenter::Point).collect();
        part.add_hole(def).map_err(|e| e.to_string())
    })();
    if res.is_err() {
        *part = before;
    }
    res
}

/// Removes Hole feature `id` (and its sketch) from `part`; later features
/// that used its faces stay (and get health), as after a Delete.
pub fn undrill(part: &mut crate::part::PartDocument, id: &str) -> Result<(), String> {
    let Some(k) = part.features.iter().position(|f| f.id() == id) else {
        return Ok(());
    };
    part.delete_features(
        k,
        crate::part::history::DeleteOptions {
            consumed_sketches: true,
            dependents: false,
        },
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// What [`make`] needs: each bolt's axis and holes, the planes the seats lie
/// on (assembly coordinates, any sense) and the fasteners of one stack in
/// stack order (placed on every bolt). `record` carries the inputs and the
/// stack's fasteners once; its id, name, holes and the fasteners of every
/// bolt (with their occurrences) are filled in.
#[derive(Clone, Debug)]
pub struct Making {
    /// Each bolt: its axis and holes.
    pub instances: Vec<Instance>,
    /// The Hole features for the plates' parts of this project (a linked
    /// part file is drilled by the caller, which lists it in
    /// `record.drilled`).
    pub drills: Vec<Drill>,
    /// The start plane (point, normal).
    pub start_plane: (Point3, Vector3),
    /// The termination (point, normal).
    pub end_plane: (Point3, Vector3),
    /// The fasteners, in the order of `record.fasteners`.
    pub fasteners: Vec<Placing>,
    /// The stored inputs.
    pub record: BoltedConnection,
}

/// Makes a connection in assembly `asm_id` of `project`: the tools part
/// (made when there is none) gets one body per hole, cutting the plates of
/// `m`; each fastener is placed on its seat with a Mate of its Z axis to the
/// clearance hole's axis and a Flush / Mate of its XY plane to its seat's
/// plane at its offset. Returns the connection's id. The caller solves (the
/// placements are already where the relationships hold) and on failure puts
/// the project back.
pub fn make(
    project: &mut crate::document::Project,
    asm_id: crate::document::DocumentId,
    m: Making,
) -> Result<u32, String> {
    // All or nothing (rule 5).
    let mut trial = project.clone();
    let id = make_in(&mut trial, asm_id, m)?;
    *project = trial;
    Ok(id)
}

fn make_in(
    project: &mut crate::document::Project,
    asm_id: crate::document::DocumentId,
    mut m: Making,
) -> Result<u32, String> {
    use super::relation::{Constraint, ConstraintKind, OriginItem};
    use crate::document::DocumentData;
    if m.instances.is_empty() {
        return Err("no bolt to place".into());
    }
    if m.fasteners.len() != m.record.fasteners.len() {
        return Err("the stack and its record differ".into());
    }
    // The holes in the plates' own parts.
    for d in &m.drills {
        if d.component.library.is_some() {
            return Err("a linked part is drilled by the caller".into());
        }
        let Some(DocumentData::Part(part)) = project
            .document_mut(d.component.document)
            .map(|x| &mut x.data)
        else {
            return Err("a plate's part is missing".into());
        };
        let id = drill(part, d).map_err(|e| format!("the hole in the plate: {e}"))?;
        m.record.drilled.push((d.component, id));
    }
    let (tools_occ, tools_doc) = super::features::ensure_tools(project, asm_id)?;
    // Each bolt's hole ids, in the order of its holes.
    let mut ids: Vec<Vec<String>> = Vec::new();
    {
        let Some(DocumentData::Part(tp)) = project.document_mut(tools_doc).map(|d| &mut d.data)
        else {
            return Err("the assembly's tools part is missing".into());
        };
        if tp.exact_bodies.0.is_empty() && !tp.features.is_empty() {
            tp.regenerate().map_err(|e| e.to_string())?;
        }
        for inst in &m.instances {
            let mut v = Vec::new();
            for (op, _) in &inst.holes {
                let f = crate::part::BodyOpFeature::new(crate::part::BodyOp::BoltHole(op.clone()));
                v.push(tp.add_body_op(f).map_err(|e| format!("the hole: {e}"))?);
            }
            ids.push(v);
        }
    }
    let Some(DocumentData::Assembly(a)) = project.document_mut(asm_id).map(|d| &mut d.data) else {
        return Err("not an assembly".into());
    };
    if let Some(f) = a.features.as_mut() {
        for (inst, v) in m.instances.iter().zip(&ids) {
            for ((_, paths), id) in inst.holes.iter().zip(v) {
                f.participants.insert(id.clone(), paths.clone());
            }
        }
    }
    let stack = std::mem::take(&mut m.record.fasteners);
    let mut placed = Vec::new();
    for (i, (inst, v)) in m.instances.iter().zip(&ids).enumerate() {
        let ax = inst.axis;
        let r = inst.holes.first().map_or(1.0, |(op, _)| op.diameter / 2.0);
        let side = if ax.dir.x.abs() < 0.9 {
            ax.dir.cross(&Vector3::x())
        } else {
            ax.dir.cross(&Vector3::y())
        }
        .normalize();
        let wall_at = ax.at + ax.dir * (ax.length / 2.0) + side * r;
        let axis_ref = GeomRef::Face {
            occurrence: tools_occ,
            sub: Vec::new(),
            name: format!("{}:Wall", v.first().ok_or("no hole was made")?),
            hint: [wall_at.x, wall_at.y, wall_at.z],
        };
        let bottom = ax.at + ax.dir * ax.length;
        for (p, template) in m.fasteners.iter().zip(&stack) {
            // Out of the material: up from the start plane, down from the end.
            let (z, seat, plane, (q, n)) = if p.nut_side {
                (
                    ax.dir,
                    bottom + ax.dir * p.offset,
                    m.record.end.clone(),
                    m.end_plane,
                )
            } else {
                (
                    -ax.dir,
                    ax.at - ax.dir * p.offset,
                    m.record.start.clone(),
                    m.start_plane,
                )
            };
            let t = crate::components::fit::seat_transform(seat, z, 0.0, false);
            let id = a.place(p.component, &p.name, t, false);
            let origin = |item: OriginItem| GeomRef::Origin {
                occurrence: id,
                sub: Vec::new(),
                item,
            };
            a.add_constraint(Constraint::new(
                ConstraintKind::Mate,
                axis_ref.clone(),
                origin(OriginItem::ZAxis),
            ));
            let mut c = Constraint::new(
                if z.dot(&n) > 0.0 {
                    ConstraintKind::Flush
                } else {
                    ConstraintKind::Mate
                },
                plane,
                origin(OriginItem::XyPlane),
            );
            // Along the plane's own normal; `0.0 +` keeps "-0" out of the panel.
            c.value = 0.0 + (seat - q).dot(&n);
            a.add_constraint(c);
            placed.push(BoltFastener {
                occurrence: id,
                instance: i as u32,
                ..template.clone()
            });
        }
    }
    // An Edit keeps the connection's id and name (its record says them).
    let id = if m.record.id != 0 && !a.bolts.iter().any(|b| b.id == m.record.id) {
        m.record.id
    } else {
        a.bolts.iter().map(|b| b.id).max().unwrap_or(0) + 1
    };
    m.record.id = id;
    if m.record.name.is_empty() {
        m.record.name = format!("Bolted Connection {id}");
    }
    m.record.holes = ids.into_iter().flatten().collect();
    m.record.fasteners = placed;
    a.bolts.push(m.record);
    Ok(id)
}

/// Removes connection `id` from assembly `asm_id`: its fasteners (and their
/// relationships), its holes in the tools part and its record.
pub fn remove(
    project: &mut crate::document::Project,
    asm_id: crate::document::DocumentId,
    id: u32,
) -> Result<(), String> {
    let mut trial = project.clone();
    remove_in(&mut trial, asm_id, id)?;
    *project = trial;
    Ok(())
}

fn remove_in(
    project: &mut crate::document::Project,
    asm_id: crate::document::DocumentId,
    id: u32,
) -> Result<(), String> {
    use crate::document::DocumentData;
    let Some(DocumentData::Assembly(a)) = project.document_mut(asm_id).map(|d| &mut d.data) else {
        return Err("not an assembly".into());
    };
    let k = a
        .bolts
        .iter()
        .position(|b| b.id == id)
        .ok_or("the connection no longer exists")?;
    let b = a.bolts.remove(k);
    for f in &b.fasteners {
        a.remove(f.occurrence);
    }
    let doc = a.features.as_mut().map(|f| {
        for h in &b.holes {
            f.participants.remove(h);
        }
        f.occurrence
    });
    let doc = doc.and_then(|o| a.find(o)).map(|o| o.component.document);
    if let Some(doc) = doc
        && let Some(DocumentData::Part(tp)) = project.document_mut(doc).map(|d| &mut d.data)
    {
        for h in &b.holes {
            if let Some(i) = tp.features.iter().position(|f| f.id() == h) {
                tp.remove_feature(i);
            }
        }
        tp.regenerate().map_err(|e| e.to_string())?;
    }
    // The holes in the plates' parts of this project (linked ones: the caller).
    for (c, id) in b.drilled.iter().filter(|(c, _)| c.library.is_none()) {
        if let Some(DocumentData::Part(part)) =
            project.document_mut(c.document).map(|d| &mut d.data)
        {
            undrill(part, id)?;
        }
    }
    Ok(())
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

    /// Use case 14: Linear as a pattern — 3 x 2 bolts stepping along each
    /// edge away from the other one, on whichever corner the start face was
    /// picked; all end on the termination.
    #[test]
    fn use_case_linear_grid_steps_away_from_the_edges() {
        let start = plane([30.0, 30.0, 40.0], [0.0, 0.0, 1.0]);
        let end = plane([0.0, 0.0, 0.0], [0.0, 0.0, -1.0]);
        let edge = |d: [Real; 3]| Geom::Axis {
            p: Point3::new(0.0, 0.0, 40.0),
            d: Vector3::from(d),
        };
        let grid = Spread::Grid {
            count: [3, 2],
            spacing: [20.0, 25.0],
        };
        for (toward, sx) in [(30.0, 1.0), (-30.0, -1.0)] {
            let r = Reference::Linear {
                e1: edge([1.0, 0.0, 0.0]),
                e2: edge([0.0, -1.0, 0.0]),
                d1: 12.0,
                d2: 15.0,
                toward: Point3::new(toward, 30.0, 40.0),
            };
            let v = axes(&start, &[r], &grid, &end).unwrap();
            assert_eq!(v.len(), 6);
            let want = [
                (15.0, 12.0),
                (35.0, 12.0),
                (55.0, 12.0),
                (15.0, 37.0),
                (35.0, 37.0),
                (55.0, 37.0),
            ];
            for (a, (x, y)) in v.iter().zip(want) {
                assert!(
                    (a.at - Point3::new(sx * x, y, 40.0)).norm() < 1e-9,
                    "{:?}",
                    a.at
                );
                assert!((a.length - 40.0).abs() < 1e-9);
            }
        }
        let bad = Spread::Grid {
            count: [0, 1],
            spacing: [1.0, 1.0],
        };
        let r = Reference::Through(Geom::Point {
            p: Point3::new(1.0, 1.0, 40.0),
        });
        assert!(entries(&start, &r, &bad).is_err(), "a grid needs Linear");
    }

    /// Use case 15: a flange's bolt circle — 6 bolts evenly round, or 3 over
    /// 90°; the angle counts from the plane's X; several references give a
    /// bolt each and two on one axis are refused.
    #[test]
    fn use_case_bolt_circle_and_several_references() {
        let start = plane([0.0, 0.0, 10.0], [0.0, 0.0, 1.0]);
        let end = plane([0.0, 0.0, 0.0], [0.0, 0.0, -1.0]);
        let c = Reference::Through(Geom::Circle {
            c: Point3::new(100.0, 0.0, 10.0),
            n: Vector3::z(),
            r: 20.0,
        });
        let six = Spread::Circle {
            count: 6,
            radius: 40.0,
            angle: 0.0,
            span: std::f64::consts::TAU,
        };
        let v = axes(&start, std::slice::from_ref(&c), &six, &end).unwrap();
        assert_eq!(v.len(), 6);
        assert!((v[0].at - Point3::new(140.0, 0.0, 10.0)).norm() < 1e-9);
        assert!((v[1].at - Point3::new(120.0, 40.0 * 0.75_f64.sqrt(), 10.0)).norm() < 1e-9);
        assert!((v[3].at - Point3::new(60.0, 0.0, 10.0)).norm() < 1e-9);
        let three = Spread::Circle {
            count: 3,
            radius: 40.0,
            angle: std::f64::consts::FRAC_PI_2,
            span: std::f64::consts::FRAC_PI_2,
        };
        let v = axes(&start, std::slice::from_ref(&c), &three, &end).unwrap();
        assert!((v[0].at - Point3::new(100.0, 40.0, 10.0)).norm() < 1e-9);
        assert!((v[2].at - Point3::new(60.0, 0.0, 10.0)).norm() < 1e-9);
        // The X of a plane square to the world X is the world Y.
        assert!((plane_x(&Vector3::x()) - Vector3::y()).norm() < 1e-12);
        // One bolt per picked point; the same point twice is refused.
        let p = |x: Real| {
            Reference::Through(Geom::Point {
                p: Point3::new(x, 5.0, 3.0),
            })
        };
        let v = axes(&start, &[p(0.0), p(30.0)], &Spread::One, &end).unwrap();
        assert_eq!(v.len(), 2);
        assert!((v[1].at - Point3::new(30.0, 5.0, 10.0)).norm() < 1e-9);
        assert!(axes(&start, &[p(0.0), p(0.0)], &Spread::One, &end).is_err());
        assert!(axes(&start, &[], &Spread::One, &end).is_err());
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
        // The drilled plate is not cut again; the others are.
        let solid: Vec<bool> = s.iter().map(|p| solid_on_axis(&parts, &axis, p)).collect();
        assert_eq!(solid, vec![false, true, true]);
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

    // ---- use cases: making the connection -----------------------------------

    use crate::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Project};

    /// A library member as a part of the project (tests never write files).
    fn member(
        project: &mut Project,
        family: &str,
        size: &str,
    ) -> (DocRef, crate::components::samples::Kind) {
        let s = crate::components::samples::all()
            .into_iter()
            .find(|s| s.family == family && s.size == size)
            .unwrap_or_else(|| panic!("no {family} {size}"));
        let p = s.part().unwrap();
        (
            DocRef::local(project.add_document(&s.size, DocumentData::Part(p))),
            s.kind,
        )
    }

    fn asm(p: &Project, id: DocumentId) -> &AssemblyDocument {
        match p.document(id).map(|d| &d.data) {
            Some(DocumentData::Assembly(a)) => a,
            _ => panic!("no assembly"),
        }
    }

    /// The geometry of `r` in its occurrence's part (parts of the project only).
    fn local(p: &Project, a: &AssemblyDocument, r: &GeomRef) -> Result<Geom, String> {
        if r.occurrence() == super::super::relation::ASSEMBLY {
            return super::super::relation::resolve_local(&crate::kernel::BRep::default(), r);
        }
        let o = a.find(r.occurrence()).ok_or("no occurrence")?;
        let Some(DocumentData::Part(part)) = p.document(o.component.document).map(|d| &d.data)
        else {
            return Err("no part".into());
        };
        super::super::relation::resolve_in(part, r)
    }

    fn solve_in(p: &mut Project, id: DocumentId) -> super::super::solve::Solved {
        let a = asm(p, id).clone();
        let s = super::super::solve::solve(&a, &|r| local(p, &a, r), None);
        if let Some(DocumentData::Assembly(a)) = p.document_mut(id).map(|d| &mut d.data) {
            for (o, t) in &s.transforms {
                if let Some(x) = a.find_mut(*o) {
                    x.transform = *t;
                }
            }
        }
        s
    }

    fn face(occurrence: u32, name: &str, hint: [Real; 3]) -> GeomRef {
        GeomRef::Face {
            occurrence,
            sub: Vec::new(),
            name: name.into(),
            hint,
        }
    }

    fn thick(k: crate::components::samples::Kind) -> Real {
        use crate::components::samples::Kind;
        match k {
            Kind::Washer(_, _, h) | Kind::HexNut(_, _, h) => h,
            _ => panic!("{k:?}"),
        }
    }

    /// Two 10 mm plates (z 0..10, 10..20) and an empty assembly of them.
    fn two_plates() -> (Project, DocumentId, [u32; 2]) {
        let mut project = Project::new("Bolted");
        let plate = PartDocument::with_box("Box1", [120.0, 80.0, 10.0]).unwrap();
        let doc = project.add_document("Plate", DocumentData::Part(plate));
        let mut a = AssemblyDocument::default();
        let lower = a.place(DocRef::local(doc), "Plate", Transform3::identity(), true);
        let upper = a.place(
            DocRef::local(doc),
            "Plate",
            Transform3::translation(0.0, 0.0, 10.0),
            true,
        );
        let id = project.add_document("Top", DocumentData::Assembly(a));
        (project, id, [lower, upper])
    }

    fn record(
        kind: BoltKind,
        start: GeomRef,
        end: GeomRef,
        roles: &[(Role, bool)],
    ) -> BoltedConnection {
        BoltedConnection {
            id: 0,
            name: String::new(),
            kind,
            placement: Placement::OnPoint,
            start,
            references: Vec::new(),
            distances: [10.0, 10.0],
            spread: Spread::One,
            fit: FastenerFit::Normal,
            length: None,
            end,
            end_auto: true,
            thread: ISO_METRIC.into(),
            size: "M8".into(),
            fasteners: roles
                .iter()
                .map(|(role, nut_side)| BoltFastener {
                    occurrence: 0,
                    instance: 0,
                    nut_side: *nut_side,
                    role: *role,
                    family: String::new(),
                    standard: "ISO".into(),
                })
                .collect(),
            holes: Vec::new(),
            drilled: Vec::new(),
            free_turn: false,
        }
    }

    fn origin_of(p: &Project, id: DocumentId, occ: u32) -> (Point3, Vector3) {
        let t = asm(p, id).find(occ).unwrap().transform;
        (t * Point3::origin(), t * Vector3::z())
    }

    fn close(a: Point3, b: [Real; 3]) -> bool {
        (a - Point3::from(b)).norm() < 1e-4
    }

    /// Use case 12: M8 Through All through two plates with a washer under the
    /// head, a washer and a nut: the holes cut both plates, the bolt is the
    /// shortest that leaves thread past the nut, every seat holds after a
    /// solve, a save and a load, and Delete takes it all away again.
    #[test]
    fn use_case_through_all_made_solved_saved_and_deleted() {
        let (mut p, id, [lower, upper]) = two_plates();
        let st = Standards::global();
        let h = hole_sizes(st, ISO_METRIC, "M8", FastenerFit::Normal).unwrap();
        let (washer, wk) = member(&mut p, "ISO 7089", "8");
        let (nut, nk) = member(&mut p, "ISO 4032", "M8");
        let (wt, nt) = (thick(wk), thick(nk));
        let lay = layout(
            BoltKind::ThroughAll,
            20.0,
            0.0,
            8.0,
            &[0.0, wt],
            0.0,
            &[wt, nt],
        );
        let lengths = [16.0, 20.0, 25.0, 30.0, 35.0, 40.0, 50.0];
        let k = choose_length(&lengths, lay.min_length, lay.max_length).unwrap();
        assert_eq!(
            lengths[k], 35.0,
            "1.6 + 20 + 1.6 + 6.8 + 2.4 = 32.4 needs 35"
        );
        let (bolt, _) = member(&mut p, "ISO 4017", "M8 x 35");
        let axis = Axis {
            at: Point3::new(60.0, 40.0, 20.0),
            dir: -Vector3::z(),
            length: 20.0,
        };
        let start = face(upper, "Box1:Top", [60.0, 40.0, 10.0]);
        let end = face(lower, "Box1:Bottom", [60.0, 40.0, 0.0]);
        let fasteners = vec![
            Placing {
                component: bolt,
                name: "ISO 4017 M8 x 35".into(),
                nut_side: false,
                offset: lay.head[0],
            },
            Placing {
                component: washer,
                name: "ISO 7089 8".into(),
                nut_side: false,
                offset: lay.head[1],
            },
            Placing {
                component: washer,
                name: "ISO 7089 8".into(),
                nut_side: true,
                offset: lay.nut[0],
            },
            Placing {
                component: nut,
                name: "ISO 4032 M8".into(),
                nut_side: true,
                offset: lay.nut[1],
            },
        ];
        let roles = [
            (Role::Bolt, false),
            (Role::Washer, false),
            (Role::Washer, true),
            (Role::Nut, true),
        ];
        let m = Making {
            drills: Vec::new(),
            instances: vec![Instance::new(
                axis,
                hole_ops(&axis, BoltKind::ThroughAll, &h, 0.0),
                &[vec![upper], vec![lower]],
                &[],
            )],
            start_plane: (Point3::new(0.0, 0.0, 20.0), Vector3::z()),
            end_plane: (Point3::new(0.0, 0.0, 0.0), -Vector3::z()),
            fasteners,
            record: record(BoltKind::ThroughAll, start, end, &roles),
        };
        let before = p.clone();
        let made = make(&mut p, id, m.clone()).unwrap();
        assert_eq!(made, 1);
        let s = solve_in(&mut p, id);
        assert!(s.converged, "{:?}", s.sick);
        let b = asm(&p, id).bolts[0].clone();
        let at = |k: usize| origin_of(&p, id, b.fasteners[k].occurrence);
        // The bolt's seat on the washer, the washer on the plate; below, the
        // washer on the bottom and the nut on it, both facing down.
        assert!(close(at(0).0, [60.0, 40.0, 21.6]), "{:?}", at(0));
        assert!(close(at(1).0, [60.0, 40.0, 20.0]));
        assert!(close(at(2).0, [60.0, 40.0, 0.0]));
        assert!(close(at(3).0, [60.0, 40.0, -1.6]));
        assert!((at(0).1 - Vector3::z()).norm() < 1e-6);
        assert!((at(3).1 + Vector3::z()).norm() < 1e-6);
        // The hole cuts both plates: Ø9 x 10 out of each.
        let a = asm(&p, id).clone();
        let f = a.features.clone().unwrap();
        assert_eq!(f.participants[&b.holes[0]], vec![vec![upper], vec![lower]]);
        let tools_doc = a.find(f.occurrence).unwrap().component.document;
        let Some(DocumentData::Part(tools)) = p.document(tools_doc).map(|d| &d.data) else {
            panic!()
        };
        let plate_doc = a.find(lower).unwrap().component.document;
        let Some(DocumentData::Part(plate)) = p.document(plate_doc).map(|d| &d.data) else {
            panic!()
        };
        let cut = super::super::features::cut(
            plate,
            &a.find(lower).unwrap().transform,
            &super::super::features::tools_for(&a, tools, &[lower]),
        )
        .unwrap()
        .unwrap();
        let v0 = plate.exact_bodies.0[0].measure().unwrap().0;
        let v1 = cut.exact_bodies.0[0].measure().unwrap().0;
        let hole = std::f64::consts::PI * 4.5 * 4.5 * 10.0;
        assert!(
            ((v0 - v1) - hole).abs() < hole * 3e-3,
            "{} vs {hole}",
            v0 - v1
        );
        // Save and load: the connection and its relationships come back.
        let dir = std::env::temp_dir().join(format!("bolted_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("bolted.3dcadcraft");
        p.save(&file).unwrap();
        let mut q = Project::load(&file).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(asm(&q, id).bolts, asm(&p, id).bolts);
        let s = solve_in(&mut q, id);
        assert!(s.converged, "after load: {:?}", s.sick);
        assert!(close(
            origin_of(&q, id, b.fasteners[0].occurrence).0,
            [60.0, 40.0, 21.6]
        ));
        // Delete: back to the plates alone (the empty tools part stays).
        remove(&mut p, id, made).unwrap();
        let a = asm(&p, id);
        assert!(a.bolts.is_empty());
        assert_eq!(a.occurrences.len(), asm(&before, id).occurrences.len() + 1);
        assert!(a.constraints.is_empty());
        assert!(a.features.as_ref().unwrap().participants.is_empty());
        // A connection that cannot be made changes nothing.
        let mut bad = m;
        bad.instances[0].holes[0].0.diameter = 0.0;
        let snapshot = format!("{:?}", asm(&p, id));
        assert!(make(&mut p, id, bad).is_err());
        assert_eq!(format!("{:?}", asm(&p, id)), snapshot);
        assert!(remove(&mut p, id, 99).is_err());
    }

    /// Use case 17: the holes go into the plate's own part (owner
    /// 2026-11-07): one Hole feature with a centre per bolt, Through All Ø9
    /// from the face the bolt enters by; a tapped M8 hole 12 deep with its
    /// thread; Delete takes them out again. The face names of the plate
    /// stay.
    #[test]
    fn use_case_holes_written_into_the_plate() {
        let vol = |p: &PartDocument| -> Real {
            p.exact_bodies
                .0
                .iter()
                .filter_map(|s| s.volume_at(1e-3))
                .sum()
        };
        let mut plate = PartDocument::with_box("Box1", [120.0, 80.0, 10.0]).unwrap();
        let v0 = vol(&plate);
        let d = Drill {
            component: DocRef::local(DocumentId::new_random()),
            face: "Box1:Top".into(),
            centers: vec![
                Point3::new(20.0, 20.0, 10.0),
                Point3::new(100.0, 20.0, 10.0),
            ],
            spec: clearance_spec(9.0),
        };
        let id = drill(&mut plate, &d).unwrap();
        let cut = v0 - vol(&plate);
        let want = 2.0 * std::f64::consts::PI * 4.5 * 4.5 * 10.0;
        assert!((cut - want).abs() < 0.01 * want, "{cut} vs {want}");
        assert_eq!(plate.holes().len(), 2);
        assert!(
            plate
                .body
                .faces()
                .any(|(_, f)| f.name.as_deref() == Some("Box1:Top"))
        );
        // Tapped: the tap drill 12 mm deep with an M8 thread.
        let st = Standards::global();
        let spec = tapped_spec(st, ISO_METRIC, "M8", 12.0).unwrap();
        assert_eq!(spec.tapped_thread().map(|t| t.major), Some(8.0));
        let mut block = PartDocument::with_box("Box1", [120.0, 80.0, 40.0]).unwrap();
        let b0 = vol(&block);
        let t = Drill {
            component: DocRef::local(DocumentId::new_random()),
            face: "Box1:Top".into(),
            centers: vec![Point3::new(30.0, 20.0, 40.0)],
            spec,
        };
        drill(&mut block, &t).unwrap();
        let r = spec_radius(&t.spec);
        let cut = b0 - vol(&block);
        assert!(cut > std::f64::consts::PI * r * r * 12.0 * 0.98, "{cut}");
        assert!(
            cut < std::f64::consts::PI * r * r * 12.0 * 1.2,
            "{cut} (with the drill point)"
        );
        // A face the part does not have: refused, the part unchanged.
        let before = format!("{:?}", plate.features.len());
        let bad = Drill {
            face: "Box1:Nowhere".into(),
            ..d.clone()
        };
        assert!(drill(&mut plate, &bad).is_err());
        assert_eq!(format!("{:?}", plate.features.len()), before);
        undrill(&mut plate, &id).unwrap();
        assert!((vol(&plate) - v0).abs() < 1e-6 * v0);
        assert!(plate.holes().is_empty());
    }

    fn spec_radius(s: &crate::part::hole::HoleSpec) -> Real {
        s.diameter / 2.0
    }

    /// Use case 16: one connection, two bolts (a grid of 2): each bolt gets
    /// its own hole and its own stack held on that hole's axis; the second
    /// bolt's upper plate already has a hole there, so its hole cuts only the
    /// lower plate. Delete removes both bolts and both holes.
    #[test]
    fn use_case_two_bolts_in_one_connection() {
        let (mut p, id, [lower, upper]) = two_plates();
        let h = hole_sizes(Standards::global(), ISO_METRIC, "M8", FastenerFit::Normal).unwrap();
        let (bolt, _) = member(&mut p, "ISO 4017", "M8 x 30");
        let (nut, nk) = member(&mut p, "ISO 4032", "M8");
        let lay = layout(
            BoltKind::ThroughAll,
            20.0,
            0.0,
            8.0,
            &[0.0],
            0.0,
            &[thick(nk)],
        );
        let start = face(upper, "Box1:Top", [30.0, 40.0, 10.0]);
        let end = face(lower, "Box1:Bottom", [30.0, 40.0, 0.0]);
        let at = |x: Real| Axis {
            at: Point3::new(x, 40.0, 20.0),
            dir: -Vector3::z(),
            length: 20.0,
        };
        let inst = |x: Real, cut: &[Vec<u32>]| {
            Instance::new(
                at(x),
                hole_ops(&at(x), BoltKind::ThroughAll, &h, 0.0),
                cut,
                &[],
            )
        };
        let m = Making {
            drills: Vec::new(),
            instances: vec![
                inst(30.0, &[vec![upper], vec![lower]]),
                inst(90.0, &[vec![lower]]),
            ],
            start_plane: (Point3::new(0.0, 0.0, 20.0), Vector3::z()),
            end_plane: (Point3::origin(), -Vector3::z()),
            fasteners: vec![
                Placing {
                    component: bolt,
                    name: "ISO 4017 M8 x 30".into(),
                    nut_side: false,
                    offset: lay.head[0],
                },
                Placing {
                    component: nut,
                    name: "ISO 4032 M8".into(),
                    nut_side: true,
                    offset: lay.nut[0],
                },
            ],
            record: record(
                BoltKind::ThroughAll,
                start,
                end,
                &[(Role::Bolt, false), (Role::Nut, true)],
            ),
        };
        let made = make(&mut p, id, m).unwrap();
        let s = solve_in(&mut p, id);
        assert!(s.converged, "{:?}", s.sick);
        let a = asm(&p, id).clone();
        let b = &a.bolts[0];
        assert_eq!(b.holes.len(), 2);
        assert_eq!(b.fasteners.len(), 4);
        assert_eq!(
            b.fasteners.iter().map(|f| f.instance).collect::<Vec<_>>(),
            vec![0, 0, 1, 1]
        );
        assert_eq!(a.constraints.len(), 8);
        // Each stack on its own axis: the bolts' heads at z 20, the nuts at 0.
        let o = |k: usize| origin_of(&p, id, b.fasteners[k].occurrence).0;
        assert!(close(o(0), [30.0, 40.0, 20.0]), "{:?}", o(0));
        assert!(close(o(2), [90.0, 40.0, 20.0]), "{:?}", o(2));
        assert!(close(o(3), [90.0, 40.0, 0.0]), "{:?}", o(3));
        let f = a.features.clone().unwrap();
        assert_eq!(f.participants[&b.holes[0]], vec![vec![upper], vec![lower]]);
        assert_eq!(f.participants[&b.holes[1]], vec![vec![lower]]);
        remove(&mut p, id, made).unwrap();
        let a = asm(&p, id);
        assert!(a.bolts.is_empty() && a.constraints.is_empty());
        assert!(a.features.as_ref().unwrap().participants.is_empty());
    }

    /// Use case 13: a 10 mm plate on a 40 mm block, Blind: the clearance hole
    /// cuts only the plate, the tapped hole only the block, half a diameter
    /// past the screw's end; a screw that would reach through the block is
    /// never chosen.
    #[test]
    fn use_case_blind_into_the_block() {
        let mut project = Project::new("Blind");
        let plate = PartDocument::with_box("Box1", [120.0, 80.0, 10.0]).unwrap();
        let block = PartDocument::with_box("Box1", [120.0, 80.0, 40.0]).unwrap();
        let pd = project.add_document("Plate", DocumentData::Part(plate));
        let bd = project.add_document("Block", DocumentData::Part(block));
        let mut a = AssemblyDocument::default();
        let blk = a.place(DocRef::local(bd), "Block", Transform3::identity(), true);
        let top = a.place(
            DocRef::local(pd),
            "Plate",
            Transform3::translation(0.0, 0.0, 40.0),
            true,
        );
        let id = project.add_document("Top", DocumentData::Assembly(a));
        let h = hole_sizes(Standards::global(), ISO_METRIC, "M8", FastenerFit::Normal).unwrap();
        let (washer, wk) = member(&mut project, "ISO 7089", "8");
        let wt = thick(wk);
        let lay = layout(BoltKind::Blind, 10.0, 40.0, 8.0, &[0.0, wt], 0.0, &[]);
        assert!((lay.min_length - 19.6).abs() < 1e-9);
        assert_eq!(lay.max_length, Some(51.6));
        let lengths = [16.0, 20.0, 25.0, 60.0, 80.0];
        let k = choose_length(&lengths, lay.min_length, lay.max_length).unwrap();
        assert_eq!(lengths[k], 20.0);
        assert_eq!(
            choose_length(&[16.0, 60.0], lay.min_length, lay.max_length),
            None
        );
        let depth = tapped_depth(20.0, &lay, 10.0, 8.0, 40.0);
        assert!((depth - 12.4).abs() < 1e-9, "8.4 into the block + 4");
        let (bolt, _) = member(&mut project, "ISO 4017", "M8 x 20");
        let axis = Axis {
            at: Point3::new(30.0, 20.0, 50.0),
            dir: -Vector3::z(),
            length: 10.0,
        };
        let holes = hole_ops(&axis, BoltKind::Blind, &h, depth);
        assert_eq!(holes.len(), 2);
        assert_eq!(holes[1].1, HoleUse::Tapped);
        assert!((holes[1].0.diameter - 6.75).abs() < 1e-9);
        let m = Making {
            drills: Vec::new(),
            instances: vec![Instance::new(axis, holes, &[vec![top]], &[vec![blk]])],
            start_plane: (Point3::new(0.0, 0.0, 50.0), Vector3::z()),
            end_plane: (Point3::new(0.0, 0.0, 40.0), Vector3::z()),
            fasteners: vec![
                Placing {
                    component: bolt,
                    name: "ISO 4017 M8 x 20".into(),
                    nut_side: false,
                    offset: lay.head[0],
                },
                Placing {
                    component: washer,
                    name: "ISO 7089 8".into(),
                    nut_side: false,
                    offset: lay.head[1],
                },
            ],
            record: record(
                BoltKind::Blind,
                face(top, "Box1:Top", [30.0, 20.0, 10.0]),
                face(blk, "Box1:Top", [30.0, 20.0, 40.0]),
                &[(Role::Bolt, false), (Role::Washer, false)],
            ),
        };
        make(&mut project, id, m).unwrap();
        let s = solve_in(&mut project, id);
        assert!(s.converged, "{:?}", s.sick);
        let a = asm(&project, id).clone();
        let b = &a.bolts[0];
        assert!(close(
            origin_of(&project, id, b.fasteners[0].occurrence).0,
            [30.0, 20.0, 51.6]
        ));
        let f = a.features.as_ref().unwrap();
        assert_eq!(f.participants[&b.holes[0]], vec![vec![top]]);
        assert_eq!(f.participants[&b.holes[1]], vec![vec![blk]]);
        let tools_doc = a.find(f.occurrence).unwrap().component.document;
        let Some(DocumentData::Part(tools)) = project.document(tools_doc).map(|d| &d.data) else {
            panic!()
        };
        let Some(DocumentData::Part(block)) = project.document(bd).map(|d| &d.data) else {
            panic!()
        };
        let cut = super::super::features::cut(
            block,
            &Transform3::identity(),
            &super::super::features::tools_for(&a, tools, &[blk]),
        )
        .unwrap()
        .unwrap();
        let removed = block.exact_bodies.0[0].measure().unwrap().0
            - cut.exact_bodies.0[0].measure().unwrap().0;
        let want = std::f64::consts::PI * 3.375 * 3.375 * 12.4;
        assert!((removed - want).abs() < want * 3e-3, "{removed} vs {want}");
    }
}
