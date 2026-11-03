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
            Kind::Flange(f) => match f.neck {
                Some(n) => Fit::Butt { diameter: n.pipe },
                None if f.bore > 0.0 => Fit::Over { bore: f.bore },
                None => Fit::Butt {
                    diameter: f.outside,
                },
            },
            // A T-slot nut goes into a slot: its placement comes with the slot rules.
            Kind::Key(..)
            | Kind::Woodruff(..)
            | Kind::ORing(..)
            | Kind::TNut(..)
            | Kind::Shape(..) => Fit::Manual,
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
            Fit::Into { .. } | Fit::Butt { .. } | Fit::Manual => false,
        }
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
