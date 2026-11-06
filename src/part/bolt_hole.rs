//! A bolted connection's hole as a body (`BodyOp::BoltHole`, format revision
//! 66, spec 6.2 §4): a cylinder about any axis in the part's coordinates.
//! Bolted Connection puts one into the assembly's tools part (spec 3.10
//! §3.8), where it cuts the components it lists; its wall `<id>:Wall` is the
//! axis the fasteners are mated to.

use serde::{Deserialize, Serialize};

use crate::kernel::{Point3, Real, Vector3};

/// A cylinder from `from` to `to` along `dir` from `origin`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoltHoleOp {
    /// A point of the axis (the entry on the start plane).
    pub origin: [Real; 3],
    /// The axis' direction (unit, into the material).
    pub dir: [Real; 3],
    /// The hole's diameter.
    pub diameter: Real,
    /// Where it starts along `dir` (negative: in the air before the start plane).
    pub from: Real,
    /// Where it ends.
    pub to: Real,
}

impl BoltHoleOp {
    /// Refuses a hole without a size or a direction.
    pub fn validate(&self) -> Result<(), String> {
        let d = Vector3::from(self.dir);
        if !(self.diameter > 0.0 && self.to - self.from > 1e-6) || d.norm() < 1e-9 {
            return Err("the bolt hole needs a diameter, a length and a direction".into());
        }
        Ok(())
    }

    /// The feature's text in the browser.
    pub fn describe(&self) -> String {
        format!(
            "Bolt hole Ø{:.2} x {:.2} mm",
            self.diameter,
            self.to - self.from
        )
    }

    /// The axis: a point and the unit direction.
    pub fn axis(&self) -> (Point3, Vector3) {
        let d = Vector3::from(self.dir);
        (Point3::from(self.origin), d / d.norm().max(1e-12))
    }

    /// The body (faces `<id>:Start`, `<id>:Wall`, `<id>:End`).
    pub fn build(&self, id: &str) -> Result<crate::kernel::exact::ExactSolid, String> {
        self.validate()?;
        let (o, d) = self.axis();
        let r = self.diameter / 2.0;
        let profile = [
            [0.0, self.from],
            [r, self.from],
            [r, self.to],
            [0.0, self.to],
        ];
        crate::kernel::exact::revolved_tool(
            o,
            d,
            0.0,
            &profile,
            &["Start", "Wall", "End", "Axis"],
            id,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tilted_hole_is_a_cylinder_of_its_size() {
        let op = BoltHoleOp {
            origin: [10.0, 5.0, 3.0],
            dir: [0.0, 0.6, -0.8],
            diameter: 9.0,
            from: -1.0,
            to: 21.0,
        };
        let s = op.build("BoltHole1").expect("built");
        let v = s.measure().expect("measured").0;
        let want = std::f64::consts::PI * 4.5 * 4.5 * 22.0;
        assert!((v - want).abs() < want * 2e-3, "{v} vs {want}");
        assert!(s.face_names.iter().flatten().any(|n| n == "BoltHole1:Wall"));
        let mut bad = op.clone();
        bad.diameter = 0.0;
        assert!(bad.validate().is_err());
    }
}
