//! Helical compression springs of round wire (spec 6.2-design-calculators
//! §3.5).
//!
//! Source: D. S. Kimball, J. H. Barr, *Elements of Machine Design*, 1909
//! (public domain), Chapter V §43 *Helical Springs*: the wire is taken in
//! torsion only (the direct stress of the pitch neglected, as the book does
//! "with ordinary proportions"), so for a load P on coils of radius r (to the
//! wire's centre), wire diameter d, n active coils and shear modulus G:
//!
//! - eq. (1)–(3): P = d⁴ G δ / (64 r³ n), δ = 64 P r³ n / (d⁴ G);
//! - eq. (4): stress p = 16 P r / (π d³);
//! - eq. (7): the deflection at stress p, δ = 4π n r² p / (G d) (the book's
//!   12.57);
//! - eq. (12): a safe stress for spring steel wire, 40 000 + 15 000 / d psi
//!   (d in inches), in MPa 275.8 + 2627 / d (d in mm);
//! - the solid height d·n (the book's "H = d n").
//!
//! The book has no curvature correction (Wahl's factor came in 1929): the
//! stress is the plain torsion stress, as the source gives it. Units: N, mm,
//! MPa.

use std::f64::consts::PI;

/// How the ends are made: how many coils are inactive and what they add to
/// the solid height. Closed ends lie flat on the seat with one coil each that
/// does not spring (the usual workshop allowance; the book flattens the ends
/// "to secure a better bearing", Fig. 30).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ends {
    /// Closed and ground: one inactive coil at each end, ground flat.
    #[default]
    ClosedGround,
    /// Closed, not ground.
    Closed,
    /// Open: every coil springs.
    Open,
}

impl Ends {
    /// Inactive coils of both ends together.
    pub fn inactive(self) -> f64 {
        match self {
            Ends::ClosedGround | Ends::Closed => 2.0,
            Ends::Open => 0.0,
        }
    }
}

/// A compression spring.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    /// Wire diameter d.
    pub wire: f64,
    /// Mean coil diameter D = 2r (outer diameter − d).
    pub mean_diameter: f64,
    /// Active coils n.
    pub active: f64,
    /// Free length.
    pub free_length: f64,
    /// Its ends.
    pub ends: Ends,
    /// Shear modulus G (the book's transverse modulus Es).
    pub g: f64,
}

/// The book's shear modulus of spring steel, 12 000 000 psi, in MPa.
pub const G_SPRING_STEEL: f64 = 82_737.0;

/// The book's safe stress for steel spring wire of diameter `d` mm, eq. (12):
/// 40 000 + 15 000 / d psi with d in inches.
pub fn safe_stress(d: f64) -> f64 {
    const PSI: f64 = 0.006_894_757;
    (40_000.0 + 15_000.0 * 25.4 / d) * PSI
}

impl Spring {
    /// The coil radius r to the wire's centre.
    pub fn radius(&self) -> f64 {
        self.mean_diameter / 2.0
    }

    /// The rate P / δ from eq. (1): d⁴ G / (64 r³ n).
    pub fn rate(&self) -> f64 {
        self.wire.powi(4) * self.g / (64.0 * self.radius().powi(3) * self.active)
    }

    /// Deflection under load `p`, eq. (2).
    pub fn deflection(&self, p: f64) -> f64 {
        p / self.rate()
    }

    /// Stress in the wire under load `p`, eq. (4).
    pub fn stress(&self, p: f64) -> f64 {
        16.0 * p * self.radius() / (PI * self.wire.powi(3))
    }

    /// The deflection at which the wire reaches stress `s`, eq. (7).
    pub fn deflection_at_stress(&self, s: f64) -> f64 {
        4.0 * PI * self.active * self.radius().powi(2) * s / (self.g * self.wire)
    }

    /// Solid height: every coil, active and inactive, wire on wire.
    pub fn solid_length(&self) -> f64 {
        self.wire * (self.active + self.ends.inactive())
    }

    /// The deflection until the spring is solid.
    pub fn travel(&self) -> f64 {
        self.free_length - self.solid_length()
    }

    /// Checks the sizes.
    pub fn validate(&self) -> Result<(), String> {
        if !(self.wire > 0.0 && self.mean_diameter > self.wire) {
            return Err("the coils must be wider than the wire".into());
        }
        if !(self.active > 0.0 && self.g > 0.0) {
            return Err("the spring needs active coils and a shear modulus".into());
        }
        if self.travel() <= 0.0 {
            return Err(format!(
                "the free length must exceed the solid length ({:.2} mm)",
                self.solid_length()
            ));
        }
        Ok(())
    }
}

/// Designs a spring for load `p` and deflection `delta` from a chosen wire `d`
/// and stress `s` (the book's procedure after eq. (4)): the coil radius from
/// eq. (4), r = π s d³ / (16 P), then the coils from eq. (3). Returns (r, n).
pub fn design(p: f64, delta: f64, d: f64, s: f64, g: f64) -> (f64, f64) {
    let r = PI * s * d.powi(3) / (16.0 * p);
    let n = d.powi(4) * g * delta / (64.0 * p * r.powi(3));
    (r, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs()
    }

    /// Kimball and Barr's example (§43), in their inches and pounds (the
    /// formulas hold in any consistent units): 1600 lb and 4 in, Es 11 000 000,
    /// d = 5/8 in. With r = 1.5 in, n = 19.4 and a stress of about 50 000 psi;
    /// wound on a 3 in arbor (r = 1.81 in), 60 500 psi and n = 11.1.
    #[test]
    fn example_of_kimball_and_barr() {
        let d: f64 = 5.0 / 8.0;
        let n_for = |r: f64| d.powi(4) * 11e6 * 4.0 / (64.0 * 1600.0 * r.powi(3));
        let s = Spring {
            wire: d,
            mean_diameter: 3.0,
            active: n_for(1.5),
            free_length: 20.0,
            ends: Ends::Open,
            g: 11e6,
        };
        assert!(close(s.active, 19.4, 0.003), "{}", s.active);
        assert!(close(s.deflection(1600.0), 4.0, 1e-12));
        assert!(
            close(s.stress(1600.0), 50_066.0, 1e-3),
            "{}",
            s.stress(1600.0)
        );
        let s2 = Spring {
            mean_diameter: 2.0 * 1.81,
            active: n_for(1.81),
            ..s
        };
        assert!(close(s2.active, 11.1, 0.005), "{}", s2.active);
        assert!(
            close(s2.stress(1600.0), 60_500.0, 0.003),
            "{}",
            s2.stress(1600.0)
        );
        // The book's procedure: r from the assumed 60 000 psi, eq. (4).
        let (r, n) = design(1600.0, 4.0, d, 60_000.0, 11e6);
        assert!(close(r, 1.80, 0.003), "{r}");
        assert!(n > 11.1 && n < 12.0, "{n}");
        // Eq. (7) is eq. (2) at the stress of eq. (4).
        let p = s.stress(1600.0);
        assert!(close(s.deflection_at_stress(p), 4.0, 1e-12));
    }

    /// Eq. (12): 40 000 + 15 000 / d psi; ¼ in wire 100 000 psi = 689 MPa.
    #[test]
    fn safe_stress_of_eq_12() {
        assert!(close(safe_stress(6.35), 100_000.0 * 0.006_894_757, 1e-9));
        let s = Spring {
            wire: 2.0,
            mean_diameter: 18.0,
            active: 6.0,
            free_length: 50.0,
            ends: Ends::ClosedGround,
            g: G_SPRING_STEEL,
        };
        assert!(s.validate().is_ok());
        assert!(close(s.solid_length(), 16.0, 1e-12));
        assert!(close(s.travel(), 34.0, 1e-12));
        let short = Spring {
            free_length: 15.0,
            ..s
        };
        assert!(short.validate().is_err());
    }
}
