//! The compression spring's ends and model (spec 6.2-design-calculators §3.5a,
//! after Inventor's Compression Spring Component Generator): each end is
//! closed, closed and ground, open, or open and ground, with its number of
//! closed (end) coils and transition coils. Closed coils lie on each other
//! (pitch = the wire), the pitch changes linearly over the transition coils to
//! the active pitch, and a ground end is flat square to the axis, so the
//! spring stands on a plane.
//!
//! The model is built directly as the wire's surface (a tube of rings in the
//! meridian planes along the centre line), the ground ends pressed flat onto
//! their planes, and embedded in the part as a mesh body: a coil whose pitch
//! changes is swept along its centre line by the kernel, which took about
//! 15 s, and grinding it would need a Boolean on that body. No Boolean here.
//!
//! The rings stand in the meridian planes, so the wire's volume is the
//! section's area times the path of its centre round the axis, π r² · R · Θ
//! (Pappus for a screw motion: the rise does not change the volume).

use std::f64::consts::{PI, TAU};

/// A triangle mesh: positions and counter-clockwise triangles.
pub type Mesh = (Vec<[f64; 3]>, Vec<[u32; 3]>);

/// How one end of the spring is made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EndType {
    /// Closed coils lying on each other, not ground.
    Closed,
    /// Closed coils, ground flat square to the axis.
    #[default]
    ClosedGround,
    /// The wire ends in the active pitch.
    Open,
    /// Open, ground flat.
    OpenGround,
}

impl EndType {
    /// Every type, in the panel's order.
    pub const ALL: [EndType; 4] = [
        EndType::Closed,
        EndType::ClosedGround,
        EndType::Open,
        EndType::OpenGround,
    ];

    /// Its name on the panel.
    pub fn label(self) -> &'static str {
        match self {
            EndType::Closed => "Closed",
            EndType::ClosedGround => "Closed and Ground",
            EndType::Open => "Open",
            EndType::OpenGround => "Open and Ground",
        }
    }

    /// Whether the end is ground flat.
    pub fn ground(self) -> bool {
        matches!(self, EndType::ClosedGround | EndType::OpenGround)
    }

    /// Whether it has closed coils.
    pub fn closed(self) -> bool {
        matches!(self, EndType::Closed | EndType::ClosedGround)
    }
}

/// One end: its type, closed coils and transition coils.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct End {
    /// The type.
    pub kind: EndType,
    /// Closed (end) coils, at the wire's pitch (ignored for open ends).
    pub closed_coils: f64,
    /// Transition coils from the closed pitch to the active one.
    pub transition: f64,
}

impl Default for End {
    fn default() -> Self {
        End {
            kind: EndType::ClosedGround,
            closed_coils: 1.0,
            transition: 0.5,
        }
    }
}

impl End {
    /// The closed coils that count (none for open ends).
    pub fn closed_count(&self) -> f64 {
        if self.kind.closed() {
            self.closed_coils.max(0.0)
        } else {
            0.0
        }
    }

    /// Its coils that do not spring: the closed coils and half the transition
    /// (the transition's pitch rises from closed to active).
    pub fn inactive(&self) -> f64 {
        self.closed_count() + self.transition.max(0.0) / 2.0
    }

    /// Coils it adds to the active ones.
    pub fn coils(&self) -> f64 {
        self.closed_count() + self.transition.max(0.0)
    }
}

/// A compression spring with its ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringModel {
    /// Wire diameter d.
    pub wire: f64,
    /// Mean coil diameter D.
    pub mean_diameter: f64,
    /// Active coils n (at the active pitch).
    pub active: f64,
    /// Free length L0 (ground ends: between the ground planes; else the wire's
    /// extreme points).
    pub free_length: f64,
    /// The start (z = 0) and end.
    pub ends: [End; 2],
}

impl SpringModel {
    /// Every coil: active, transition and closed, n_t.
    pub fn total_coils(&self) -> f64 {
        self.active + self.ends[0].coils() + self.ends[1].coils()
    }

    /// The coils that spring for the rate: the active ones and half of each
    /// transition, n_t minus the inactive coils.
    pub fn springing_coils(&self) -> f64 {
        self.total_coils() - self.ends[0].inactive() - self.ends[1].inactive()
    }

    /// Solid length: every coil on the next. A ground end loses half a wire
    /// (the usual workshop rule: closed and ground ends d·n_t, closed ends
    /// d·(n_t + 1)).
    pub fn solid_length(&self) -> f64 {
        let ground = self.ends.iter().filter(|e| e.kind.ground()).count() as f64;
        self.wire * (self.total_coils() + 1.0 - ground / 2.0)
    }

    /// The rise of the centre line over the closed and transition coils of
    /// both ends at active pitch `p` (closed coils rise d a turn, a transition
    /// its mean of d and p).
    fn rise(&self, p: f64) -> f64 {
        let d = self.wire;
        self.ends
            .iter()
            .map(|e| e.closed_count() * d + e.transition.max(0.0) * (d + p) / 2.0)
            .sum::<f64>()
            + self.active * p
    }

    /// The centre line's height at its start: a plain end stands on its wire
    /// (d / 2), a ground one has its first point on the plane (ground through
    /// half the wire at the tip).
    fn start_z(&self, e: &End) -> f64 {
        if e.kind.ground() {
            0.0
        } else {
            self.wire / 2.0
        }
    }

    /// The active pitch that gives the free length.
    pub fn pitch(&self) -> Result<f64, String> {
        let d = self.wire;
        let (s, e) = (self.start_z(&self.ends[0]), self.start_z(&self.ends[1]));
        // L0 = s + rise(p) + e; rise is linear in p.
        let r0 = self.rise(0.0);
        let slope = self.rise(1.0) - r0;
        if slope <= 0.0 {
            return Err("the spring has no coils that rise".into());
        }
        let p = (self.free_length - s - e - r0) / slope;
        if p <= d * 1.05 {
            return Err(format!(
                "the active coils touch at a free length of {:.2} mm (pitch {p:.2} mm, wire Ø{d})",
                self.free_length
            ));
        }
        Ok(p)
    }

    /// Checks the sizes.
    pub fn validate(&self) -> Result<(), String> {
        let d = self.wire;
        if d.is_nan() || d <= 0.0 {
            return Err("the wire diameter must be positive".into());
        }
        if self.mean_diameter.is_nan() || self.mean_diameter <= d {
            return Err("the outer diameter must be more than twice the wire".into());
        }
        if self.active.is_nan() || self.active < 1.0 {
            return Err("the spring needs at least one active coil".into());
        }
        for e in &self.ends {
            if e.closed_coils < 0.0 || e.transition < 0.0 {
                return Err("closed and transition coils cannot be negative".into());
            }
        }
        self.pitch().map(|_| ())
    }

    /// The centre line's height at turn angle `t` (radians from the start).
    pub fn centre_z(&self, t: f64, p: f64) -> f64 {
        let d = self.wire;
        let [a, b] = self.ends;
        // Pieces in turns: closed, transition, active, transition, closed.
        let pieces = [
            (a.closed_count(), d, d),
            (a.transition.max(0.0), d, p),
            (self.active, p, p),
            (b.transition.max(0.0), p, d),
            (b.closed_count(), d, d),
        ];
        let mut z = self.start_z(&a);
        let mut left = t / TAU;
        for (turns, p0, p1) in pieces {
            if turns <= 0.0 {
                continue;
            }
            let u = left.min(turns);
            // The pitch changes linearly along the piece: the rise over u of
            // its turns is ∫ p(s) ds.
            z += u * p0 + (p1 - p0) * u * u / (2.0 * turns);
            left -= u;
            if left <= 0.0 {
                break;
            }
        }
        z
    }

    /// The wire's surface: rings of `around` points in the meridian planes,
    /// `per_turn` rings a turn, the tube closed by flat discs at the wire's
    /// ends; a ground end's points below its plane pressed onto it. Positions
    /// and counter-clockwise triangles (outward normals).
    pub fn mesh(&self, per_turn: usize, around: usize) -> Result<Mesh, String> {
        self.validate()?;
        let p = self.pitch()?;
        let (rm, r) = (self.mean_diameter / 2.0, self.wire / 2.0);
        let total = self.total_coils();
        let rings = ((total * per_turn as f64).ceil() as usize).max(2);
        let theta = total * TAU;
        let top = self.free_length;
        let (g0, g1) = (self.ends[0].kind.ground(), self.ends[1].kind.ground());
        let mut pos = Vec::with_capacity((rings + 1) * around + 2);
        for k in 0..=rings {
            let t = theta * k as f64 / rings as f64;
            let zc = self.centre_z(t, p);
            let (c, s) = (t.cos(), t.sin());
            for j in 0..around {
                let phi = TAU * j as f64 / around as f64;
                let rr = rm + r * phi.cos();
                let mut z = zc + r * phi.sin();
                if g0 && z < 0.0 {
                    z = 0.0;
                }
                if g1 && z > top {
                    z = top;
                }
                pos.push([rr * c, rr * s, z]);
            }
        }
        let mut tri = Vec::new();
        let at = |k: usize, j: usize| (k * around + j % around) as u32;
        for k in 0..rings {
            for j in 0..around {
                let (a, b, c, d) = (at(k, j), at(k + 1, j), at(k + 1, j + 1), at(k, j + 1));
                // The rings advance counter-clockwise about Z and φ runs
                // from outward (+R) up (+Z): this order faces outward
                // (the volume test checks the sign).
                tri.push([a, c, d]);
                tri.push([a, b, c]);
            }
        }
        // End discs: a fan about each end ring's centre.
        for (k, back) in [(0usize, true), (rings, false)] {
            let t = theta * k as f64 / rings as f64;
            let mut zc = self.centre_z(t, p);
            if g0 && k == 0 {
                zc = zc.max(0.0);
            }
            if g1 && k == rings {
                zc = zc.min(top);
            }
            let ci = pos.len() as u32;
            pos.push([rm * t.cos(), rm * t.sin(), zc]);
            for j in 0..around {
                let (a, b) = (at(k, j), at(k, j + 1));
                tri.push(if back { [ci, b, a] } else { [ci, a, b] });
            }
        }
        Ok((pos, tri))
    }

    /// The wire's volume, π r² · R · Θ (Pappus; before grinding).
    pub fn wire_volume(&self) -> f64 {
        let r = self.wire / 2.0;
        PI * r * r * (self.mean_diameter / 2.0) * self.total_coils() * TAU
    }
}

/// The mesh as OBJ text (one object `Spring`), for a part's embedded mesh body.
pub fn obj_text(pos: &[[f64; 3]], tri: &[[u32; 3]]) -> String {
    let mut out = String::from("o Spring\n");
    for v in pos {
        out += &format!("v {} {} {}\n", v[0], v[1], v[2]);
    }
    for t in tri {
        out += &format!("f {} {} {}\n", t[0] + 1, t[1] + 1, t[2] + 1);
    }
    out
}

/// The spring as a part: its mesh embedded as a mesh body (one Import
/// feature, `part::import`), no sketch, no Boolean.
pub fn spring_part(s: &SpringModel) -> Result<crate::part::PartDocument, String> {
    use crate::part::body_op::{BodyOp, BodyOpFeature};
    use crate::part::import::{ImportOp, mesh_text};
    let (pos, tri) = s.mesh(48, 16)?;
    let text = mesh_text(obj_text(&pos, &tri).as_bytes(), "spring.obj", 1.0)?;
    let mut part = crate::part::PartDocument::default();
    part.add_body_op(BodyOpFeature::new(BodyOp::Import(ImportOp::new(
        "Compression Spring",
        text,
    ))))
    .map_err(|e| e.to_string())?;
    Ok(part)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spring(kind: EndType) -> SpringModel {
        SpringModel {
            wire: 2.0,
            mean_diameter: 18.0,
            active: 6.0,
            free_length: 50.0,
            ends: [End {
                kind,
                closed_coils: 1.0,
                transition: 0.5,
            }; 2],
        }
    }

    /// The signed volume of a closed triangle mesh (tetrahedra from the origin).
    fn volume(pos: &[[f64; 3]], tri: &[[u32; 3]]) -> f64 {
        tri.iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| pos[i as usize]);
                (a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
                    + a[2] * (b[0] * c[1] - b[1] * c[0]))
                    / 6.0
            })
            .sum()
    }

    /// Every end type: the free length between the extreme points, the pitch
    /// running down to the wire over the transition into closed coils.
    #[test]
    fn free_length_and_pitch_by_end_type() {
        for kind in EndType::ALL {
            let s = spring(kind);
            let p = s.pitch().unwrap();
            let (pos, _) = s.mesh(48, 16).unwrap();
            let lo = pos.iter().map(|v| v[2]).fold(f64::INFINITY, f64::min);
            let hi = pos.iter().map(|v| v[2]).fold(f64::NEG_INFINITY, f64::max);
            assert!(
                lo.abs() < 0.02 && (hi - 50.0).abs() < 0.02,
                "{kind:?}: {lo} … {hi}"
            );
            if kind.closed() {
                // One closed turn rises one wire diameter: coils on each other.
                let z0 = s.centre_z(0.0, p);
                let z1 = s.centre_z(TAU, p);
                assert!((z1 - z0 - 2.0).abs() < 1e-9, "{kind:?}");
            }
        }
    }

    /// The tube's volume is the wire's, π r² R Θ, within the facets' loss
    /// (ends not ground); ground ends take a little off.
    #[test]
    fn volume_against_the_wire_length() {
        let s = spring(EndType::Closed);
        let (pos, tri) = s.mesh(96, 32).unwrap();
        let v = volume(&pos, &tri);
        let want = s.wire_volume();
        // A ring of 32 sides holds cos(π/32)·sin… of the circle's area: < 1 %.
        assert!(v > 0.0 && (v - want).abs() / want < 0.01, "{v} vs {want}");
        let g = spring(EndType::ClosedGround);
        let (pos, tri) = g.mesh(96, 32).unwrap();
        let vg = volume(&pos, &tri);
        assert!(vg < g.wire_volume() && vg > 0.9 * g.wire_volume(), "{vg}");
    }

    /// Ground ends are flat square to the axis: a ring of points at z = 0
    /// and at the free length, covering a footprint, none beyond.
    #[test]
    fn ground_ends_are_flat() {
        let s = spring(EndType::ClosedGround);
        let (pos, tri) = s.mesh(96, 32).unwrap();
        let flat_area = |z: f64| -> f64 {
            tri.iter()
                .filter(|t| t.iter().all(|&i| (pos[i as usize][2] - z).abs() < 1e-9))
                .map(|t| {
                    let [a, b, c] = t.map(|i| pos[i as usize]);
                    ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.0
                })
                .sum()
        };
        let (bottom, top) = (flat_area(0.0), flat_area(50.0));
        // At least a few square wire diameters of flat seat at each end.
        assert!(bottom > 4.0 && top > 4.0, "{bottom} {top}");
        assert!(pos.iter().all(|v| v[2] >= -1e-12 && v[2] <= 50.0 + 1e-12));
        // Not ground: nothing lies flat.
        let o = spring(EndType::Closed);
        let (pos, tri) = o.mesh(96, 32).unwrap();
        let flat = tri
            .iter()
            .filter(|t| t.iter().all(|&i| pos[i as usize][2].abs() < 1e-9))
            .count();
        assert_eq!(flat, 0);
    }

    /// Solid length: closed and ground ends d·n_t, closed ends d·(n_t + 1);
    /// the springing coils count half of each transition.
    #[test]
    fn solid_length_and_coils() {
        let g = spring(EndType::ClosedGround);
        assert!((g.total_coils() - 9.0).abs() < 1e-12);
        assert!((g.solid_length() - 2.0 * 9.0).abs() < 1e-12);
        let c = spring(EndType::Closed);
        assert!((c.solid_length() - 2.0 * 10.0).abs() < 1e-12);
        assert!((g.springing_coils() - 6.5).abs() < 1e-12);
        let o = spring(EndType::Open);
        assert!((o.total_coils() - 7.0).abs() < 1e-12);
        // Too short for its coils: refused.
        let mut short = g;
        short.free_length = 15.0;
        assert!(short.validate().is_err());
    }
}
