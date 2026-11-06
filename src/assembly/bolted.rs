//! Bolted Connection (spec `docs/spec/6.2-bolted-connection.md`): a bolt,
//! washers and a nut placed as one object through holes drilled in the
//! components between a start plane and a termination. GUI-free; the panel
//! is `gui/bolted.rs`.
//!
//! This first part holds what the panel needs: the hole each plate gets for
//! a thread size (clearance holes from the standards' clearance table, the
//! tap drill of a blind connection's last component).

use super::relation::Geom;
use crate::kernel::{Point3, Real, Vector3};
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
    let Geom::Plane { p, n } = *start else {
        return Err("the start plane must be a planar face or a work plane".into());
    };
    let Geom::Plane { p: q, n: m } = *end else {
        return Err("the termination must be a planar face or a work plane".into());
    };
    if n.cross(&m).norm() > 1e-4 {
        return Err("the termination must be parallel to the start plane".into());
    }
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
    let length = (at - q).dot(&n);
    if length <= 1e-6 {
        return Err("the termination lies behind the start plane".into());
    }
    Ok(Axis { at, dir, length })
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
