//! What Smart Drop decides, GUI-free (`docs/spec/6.3-content-center.md` §3.5,
//! §6.5): which member of a family suits a hole or a shaft, and where it goes.
//! A library part's axis is Z and its seat the XY plane, so every size of a
//! family lands alike and a later Change Size keeps its relationships.

use crate::kernel::{Point3, Real, Transform3, Vector3};

use super::samples::Kind;

/// How a member takes its place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fit {
    /// Goes into a hole (bolt, screw, pin): its diameter, its length if it
    /// has one, and how far its seat sinks below the face (a countersunk head).
    Into {
        /// Nominal diameter.
        d: Real,
        /// Length below the seat.
        length: Option<Real>,
        /// The seat's depth below the face.
        sink: Real,
    },
    /// Goes over a shaft or a shank (nut, washer, ring).
    Over {
        /// Its bore.
        bore: Real,
    },
    /// A bearing: on a shaft by its bore, in a housing by its outside.
    Bearing {
        /// Its bore.
        bore: Real,
        /// Its outside diameter.
        outside: Real,
    },
    /// Stands on the end of a cylinder of its own diameter and continues it (a
    /// weld neck flange on its pipe, a blind flange on a flange's rim).
    Butt {
        /// The diameter it meets.
        diameter: Real,
    },
    /// Stands on the floor of a slot, its foot across the slot (a T-slot nut).
    Slot {
        /// The foot's width.
        foot: Real,
    },
    /// Placed by hand only (keys, shapes, O-rings).
    Manual,
}

impl Fit {
    /// The fit of a built-in component.
    pub fn of(kind: &Kind) -> Fit {
        let into = |d: Real, l: Real| Fit::Into {
            d,
            length: Some(l),
            sink: 0.0,
        };
        match *kind {
            Kind::HexBolt(d, l, ..)
            | Kind::PartThreadBolt(d, l, ..)
            | Kind::SocketScrew(d, l, ..)
            | Kind::ButtonHead(d, l, ..)
            | Kind::SetScrew(d, l, ..)
            | Kind::FlangeBolt(d, l, ..)
            | Kind::Rivet(d, l, ..)
            | Kind::RoundHeadScrew(d, l, ..)
            | Kind::FlatHeadScrew(d, l, ..)
            | Kind::Rod(d, l)
            | Kind::Pin(d, l)
            | Kind::TaperPin(d, l)
            | Kind::EyeBolt(d, l, ..)
            | Kind::THead(d, l, ..)
            | Kind::SquareHead(d, l, ..) => into(d, l),
            // Its shoulder fills the hole, the thread runs on below it.
            Kind::Shoulder(_, l, ds, ..) => into(ds, l),
            Kind::Countersunk(d, l, _, k, _) | Kind::ConeRivet(d, l, _, k) => Fit::Into {
                d,
                length: Some(l),
                sink: k,
            },
            Kind::HexNut(d, ..)
            | Kind::FlangeNut(d, ..)
            | Kind::CapNut(d, ..)
            | Kind::SquareNut(d, ..)
            | Kind::WingNut(d, ..) => Fit::Over { bore: d },
            Kind::Washer(d1, ..)
            | Kind::Circlip(d1, ..)
            | Kind::SplitRing(d1, ..)
            | Kind::DiscSpring(d1, ..)
            | Kind::SquareWasher(d1, ..) => Fit::Over { bore: d1 },
            Kind::Bearing(d, od, _) => Fit::Bearing {
                bore: d,
                outside: od,
            },
            Kind::PipeElbow(d, ..)
            | Kind::PipeTee(d, ..)
            | Kind::PipeReducer(d, ..)
            | Kind::PipeCap(d, ..) => Fit::Butt { diameter: d },
            Kind::Grease(d, l1, ..) | Kind::Plug(d, _, _, l1, ..) => into(d, l1),
            Kind::BallKnob(_, _, d, _) => Fit::Over { bore: d },
            Kind::Cup(od, _, t) => Fit::Over { bore: od - 2.0 * t },
            Kind::Valve(d, ..) => Fit::Butt { diameter: d },
            Kind::Gear(_, _, _, d)
            | Kind::Sprocket(_, _, _, _, d)
            | Kind::Pulley(_, _, _, _, _, d) => Fit::Over { bore: d },
            Kind::Spring(od, d, ..) => Fit::Over { bore: od - 2.0 * d },
            Kind::Block(..) | Kind::Handle(..) | Kind::Hinge(..) => Fit::Manual,
            Kind::Flange(f) => match f.neck {
                Some(n) => Fit::Butt { diameter: n.pipe },
                None if f.bore > 0.0 => Fit::Over { bore: f.bore },
                None => Fit::Butt {
                    diameter: f.outside,
                },
            },
            Kind::TNut(_, _, e, ..) => Fit::Slot { foot: e },
            Kind::BallNut(d0, ..) => Fit::Over { bore: d0 },
            Kind::BallScrew(d, l) => into(d, l),
            Kind::ThreadedPin(d, l, ..) => into(d, l),
            Kind::Key(..)
            | Kind::Woodruff(..)
            | Kind::ORing(..)
            | Kind::Shape(..)
            | Kind::Cylinder(..) => Fit::Manual,
        }
    }

    /// Can it be dropped at all?
    pub fn droppable(self) -> bool {
        self != Fit::Manual
    }
}

/// What the cursor is on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    /// A hole (the material around it) or a shaft (the material inside).
    pub hole: bool,
    /// The diameter of the picked circle or cylinder.
    pub diameter: Real,
    /// A tapped hole's or a modelled thread's nominal diameter: exact, where
    /// `diameter` is only a drill.
    pub thread: Option<Real>,
    /// Length of the hole or shaft along its axis.
    pub depth: Real,
    /// The hole ends inside the material.
    pub blind: bool,
}

/// A clearance hole is at most this much wider than its screw (a coarse
/// clearance hole is ~1.12 d at M8, 1.2 d below).
const CLEARANCE: Real = 1.3;
/// Two diameters taken as one.
const SAME: Real = 0.05;
/// A pipe's outside diameter may differ this much from its flange's (series differ).
const BUTT: Real = 0.02;
/// A slot's floor is at most this much wider than the nut's foot (DIN 650 leaves room).
const SLOT: Real = 1.35;

/// How well a member's diameter suits the target: `None` does not fit, a
/// smaller number fits better.
fn score(fit: Fit, t: &Target) -> Option<Real> {
    match fit {
        Fit::Manual => None,
        Fit::Into { d, .. } => {
            if !t.hole {
                return None;
            }
            if let Some(thread) = t.thread {
                return ((d - thread).abs() < SAME).then_some(0.0);
            }
            // The widest screw that still passes through.
            (d <= t.diameter + SAME && d * CLEARANCE >= t.diameter).then_some(t.diameter - d)
        }
        Fit::Over { bore } => {
            let at = t.thread.unwrap_or(t.diameter);
            // A shank is matched by its nominal diameter, a hole's rim (no
            // thread data) by the nearest.
            let gap = (bore - at).abs();
            (gap <= (0.2 * at).max(SAME)).then_some(gap)
        }
        // The target's `diameter` is the floor's width: the widest foot that still goes in.
        Fit::Slot { foot } => (!t.hole && foot <= t.diameter + SAME && foot * SLOT >= t.diameter)
            .then_some(t.diameter - foot),
        Fit::Butt { diameter } => {
            let gap = (diameter - t.diameter).abs();
            (!t.hole && gap <= BUTT * diameter).then_some(gap)
        }
        Fit::Bearing { bore, outside } => {
            let d = if t.hole { outside } else { bore };
            ((d - t.diameter).abs() < SAME).then_some(0.0)
        }
    }
}

/// The member of a family that suits `t`: the nearest diameter, then the
/// length (through a hole the shortest that passes, in a blind hole the
/// longest that stays inside). `None`: no size fits.
pub fn choose(members: &[Fit], t: &Target) -> Option<usize> {
    let best = members
        .iter()
        .filter_map(|m| score(*m, t))
        .fold(Real::INFINITY, Real::min);
    if !best.is_finite() {
        return None;
    }
    let of_size: Vec<usize> = (0..members.len())
        .filter(|&i| score(members[i], t).is_some_and(|s| s <= best + 1e-9))
        .collect();
    let length = |i: usize| match members[i] {
        Fit::Into { length, .. } => length,
        _ => None,
    };
    let pick = |want: &dyn Fn(Real) -> bool, smallest: bool| {
        of_size
            .iter()
            .copied()
            .filter(|&i| length(i).is_some_and(want))
            .min_by(|&a, &b| {
                let (la, lb) = (length(a).unwrap_or(0.0), length(b).unwrap_or(0.0));
                if smallest {
                    la.total_cmp(&lb)
                } else {
                    lb.total_cmp(&la)
                }
            })
    };
    let chosen = if t.blind {
        pick(&|l| l <= t.depth + SAME, false).or_else(|| pick(&|_| true, true))
    } else {
        pick(&|l| l >= t.depth - SAME, true).or_else(|| pick(&|_| true, false))
    };
    chosen.or(of_size.first().copied())
}

/// A washer under the head, one under the nut, and the nut (Bolted Connection):
/// which members they are and their thicknesses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stack {
    /// The washer's family and member.
    pub washer: (usize, usize),
    /// The washer's thickness.
    pub washer_h: Real,
    /// The nut's family and member.
    pub nut: (usize, usize),
    /// The nut's height.
    pub nut_m: Real,
}

impl Stack {
    /// The length a bolt needs through `grip`: the stack and a little thread
    /// past the nut.
    pub fn length_for(&self, grip: Real, d: Real) -> Real {
        grip + 2.0 * self.washer_h + self.nut_m + 0.3 * d
    }
}

/// Does a part picked onto a rim run from there into the target, rather than
/// stand on the face? A bearing enters its housing or slides onto its shaft, a
/// ring slides onto a shaft. On a plane picked for the seat everything sits on it.
pub fn goes_in(fit: Fit, t: &Target, from_rim: bool) -> bool {
    from_rim
        && match fit {
            Fit::Bearing { .. } => true,
            Fit::Over { .. } => !t.hole,
            Fit::Into { .. } | Fit::Butt { .. } | Fit::Slot { .. } | Fit::Manual => false,
        }
}

/// The floor of a slot, from its face's points and normal: the middle line and the width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotFloor {
    /// A point of the middle line, on the floor.
    pub middle: Point3,
    /// Along the slot.
    pub along: Vector3,
    /// Across it (in the floor).
    pub across: Vector3,
    /// Its width.
    pub width: Real,
    /// Its length.
    pub length: Real,
}

/// `None`: the points do not make a long floor. A slot is longer than it is wide; the directions
/// come from the point spread, so a slot at any angle is found.
pub fn slot_floor(points: &[Point3], normal: Vector3) -> Option<SlotFloor> {
    let n = normal.try_normalize(1e-12)?;
    let helper = if n.x.abs() < 0.9 {
        Vector3::x()
    } else {
        Vector3::y()
    };
    let (u, v) = (n.cross(&helper).normalize(), {
        let u = n.cross(&helper).normalize();
        n.cross(&u)
    });
    if points.len() < 3 {
        return None;
    }
    let centre = points.iter().fold(Vector3::zeros(), |s, p| s + p.coords) / points.len() as Real;
    let flat: Vec<(Real, Real)> = points
        .iter()
        .map(|p| {
            let q = p.coords - centre;
            (q.dot(&u), q.dot(&v))
        })
        .collect();
    // The outline's corners, as extremes in many directions: a mesh may hold thousands of points.
    let mut corners: Vec<(Real, Real)> = Vec::new();
    for k in 0..48 {
        let a = Real::from(k) * std::f64::consts::TAU / 48.0;
        let (c, s) = (a.cos(), a.sin());
        let far = flat
            .iter()
            .copied()
            .max_by(|p, q| (p.0 * c + p.1 * s).total_cmp(&(q.0 * c + q.1 * s)));
        if let Some(f) = far
            && !corners.contains(&f)
        {
            corners.push(f);
        }
    }
    // The smallest rectangle round them has a side along two of them (the mean of a mesh's points
    // leans towards where it has more of them: a floor came out 3 mm too wide).
    let extent = |dir: (Real, Real)| {
        corners
            .iter()
            .fold((Real::INFINITY, Real::NEG_INFINITY), |(lo, hi), p| {
                let s = p.0 * dir.0 + p.1 * dir.1;
                (lo.min(s), hi.max(s))
            })
    };
    let mut best: Option<(Real, (Real, Real))> = None;
    for (i, a) in corners.iter().enumerate() {
        for b in corners.iter().skip(i + 1) {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len = dx.hypot(dy);
            if len < 1e-9 {
                continue;
            }
            let d = (dx / len, dy / len);
            let (s0, s1) = extent(d);
            let (t0, t1) = extent((-d.1, d.0));
            let area = (s1 - s0) * (t1 - t0);
            if best.is_none_or(|(b, _)| area < b - 1e-9) {
                best = Some((area, d));
            }
        }
    }
    let (_, d) = best?;
    let (s0, s1) = extent(d);
    let (t0, t1) = extent((-d.1, d.0));
    // The long side runs along the slot.
    let (dir, (a0, a1), (b0, b1)) = if s1 - s0 >= t1 - t0 {
        (d, (s0, s1), (t0, t1))
    } else {
        ((-d.1, d.0), (t0, t1), (-s1, -s0))
    };
    let along = u * dir.0 + v * dir.1;
    let across = n.cross(&along);
    let (length, width) = (a1 - a0, b1 - b0);
    if width <= 0.0 || length < 1.2 * width {
        return None;
    }
    Some(SlotFloor {
        middle: Point3::from(centre + along * (a0 + a1) / 2.0 + across * (b0 + b1) / 2.0),
        along,
        across,
        width,
        length,
    })
}

/// The slot whose floor is the face `name` of `p` (part coordinates): the floor, and the long wall
/// beside it facing the slot: its name, a point on it, the direction from it to the middle. Asked
/// once per hover of the face, a walk over the part's faces.
pub fn slot_in(
    p: &crate::part::PartDocument,
    name: &str,
    hint: Point3,
    normal: Vector3,
) -> Option<(SlotFloor, String, Point3, Vector3)> {
    use crate::kernel::Surface;
    let near = |f: &crate::kernel::Face| {
        f.mesh.as_ref().map_or(Real::INFINITY, |m| {
            m.positions
                .iter()
                .map(|q| (q - hint).norm())
                .fold(Real::INFINITY, Real::min)
        })
    };
    let (_, floor_face) = p
        .body
        .faces()
        .filter(|(_, f)| f.name.as_deref() == Some(name))
        .min_by(|a, b| near(a.1).total_cmp(&near(b.1)))?;
    let floor = slot_floor(&floor_face.mesh.as_ref()?.positions, normal)?;
    p.body.faces().find_map(|(_, f)| {
        let Some(Surface::Plane(pl)) = p.body.surface(f.surface) else {
            return None;
        };
        if pl.normal.dot(&floor.across).abs() < 0.999 {
            return None;
        }
        let m = f.mesh.as_ref()?;
        // Half the width from the middle line, and rising above the floor: a slot's wall. The top
        // beside a slot's neck has walls too, going down. No normal is read: those of an exact
        // face's triangles may come out inside-out.
        let gap = (floor.middle - pl.origin).dot(&pl.normal);
        if (gap.abs() - floor.width / 2.0).abs() > 1e-3 {
            return None;
        }
        let on = m.positions.iter().find(|q| {
            (*q - floor.middle).dot(&normal) > 1e-6
                && (*q - floor.middle).dot(&floor.along).abs() <= floor.length / 2.0 + 1e-6
        })?;
        let out = pl.normal.into_inner() * gap.signum();
        Some((floor, f.name.clone()?, *on, out))
    })
}

/// Where a part goes on a slot's floor: the seat on `center`, +Z along `normal`, X across the slot.
pub fn slot_transform(center: Point3, normal: Vector3, across: Vector3) -> Transform3 {
    let z = normal.try_normalize(1e-12).unwrap_or_else(Vector3::z);
    let x = (across - z * across.dot(&z))
        .try_normalize(1e-12)
        .unwrap_or_else(Vector3::x);
    let y = z.cross(&x);
    let turn =
        nalgebra::Rotation3::from_matrix_unchecked(nalgebra::Matrix3::from_columns(&[x, y, z]));
    Transform3::from_parts(
        nalgebra::Translation3::new(center.x, center.y, center.z),
        nalgebra::UnitQuaternion::from_rotation_matrix(&turn),
    )
}

/// Where a part goes that lands by a circular edge: its centre on the target's,
/// its face against the target's (`aligned`: side by side instead).
pub fn landing(
    center: Point3,
    normal: Vector3,
    target: Point3,
    target_normal: Vector3,
    aligned: bool,
) -> Transform3 {
    seat_transform(target, target_normal, 0.0, !aligned)
        * seat_transform(center, normal, 0.0, false).inverse()
}

/// Where a library part goes: the seat on `center`, its +Z along `normal`
/// (the face's outward normal), `sink` below the face; `flip` turns it over.
pub fn seat_transform(center: Point3, normal: Vector3, sink: Real, flip: bool) -> Transform3 {
    let n = normal.try_normalize(1e-12).unwrap_or_else(Vector3::z);
    let z = if flip { -n } else { n };
    let helper = if z.x.abs() < 0.9 {
        Vector3::x()
    } else {
        Vector3::y()
    };
    let y = z.cross(&helper).normalize();
    let x = y.cross(&z);
    let at = center - n * sink;
    let turn =
        nalgebra::Rotation3::from_matrix_unchecked(nalgebra::Matrix3::from_columns(&[x, y, z]));
    Transform3::from_parts(
        nalgebra::Translation3::new(at.x, at.y, at.z),
        nalgebra::UnitQuaternion::from_rotation_matrix(&turn),
    )
}
