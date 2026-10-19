//! Dimension parameters: the name (`d0`), equation and tolerance of each
//! dimension (Inventor: every dimension is a model parameter), and how its
//! text is shown (Value / Name / Expression / Tolerance).

use serde::{Deserialize, Serialize};

use super::{Constraint, ConstraintId, Sketch};
use crate::kernel::Real;

/// Tolerance type (Inventor's Tolerance dialog).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TolKind {
    /// No tolerance (document default).
    #[default]
    Default,
    /// ±a.
    Symmetric,
    /// +a / −b.
    Deviation,
    /// Upper and lower limits.
    Limits,
    /// Maximum only.
    Max,
    /// Minimum only.
    Min,
    /// Basic (theoretically exact, boxed).
    Basic,
}

impl TolKind {
    /// All kinds, in dialog order.
    pub const ALL: [TolKind; 7] = [
        TolKind::Default,
        TolKind::Symmetric,
        TolKind::Deviation,
        TolKind::Limits,
        TolKind::Max,
        TolKind::Min,
        TolKind::Basic,
    ];

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            TolKind::Default => "Default",
            TolKind::Symmetric => "Symmetric",
            TolKind::Deviation => "Deviation",
            TolKind::Limits => "Limits",
            TolKind::Max => "Max",
            TolKind::Min => "Min",
            TolKind::Basic => "Basic",
        }
    }
}

/// Which size the model is built at (Inventor's evaluated size).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvalSize {
    /// The nominal value.
    #[default]
    Nominal,
    /// Nominal + upper deviation.
    Upper,
    /// Middle of the tolerance zone.
    Median,
    /// Nominal + lower deviation.
    Lower,
}

impl EvalSize {
    /// All, in dialog order.
    pub const ALL: [EvalSize; 4] = [
        EvalSize::Upper,
        EvalSize::Median,
        EvalSize::Nominal,
        EvalSize::Lower,
    ];

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            EvalSize::Nominal => "Nominal",
            EvalSize::Upper => "Upper",
            EvalSize::Median => "Median",
            EvalSize::Lower => "Lower",
        }
    }
}

/// A tolerance: deviations from the nominal (internal units, signed:
/// `upper ≥ lower`), and the evaluated size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Tolerance {
    /// Type.
    pub kind: TolKind,
    /// Upper deviation (e.g. +0.1).
    pub upper: Real,
    /// Lower deviation (e.g. −0.05).
    pub lower: Real,
    /// Which size the model uses.
    #[serde(default)]
    pub eval: EvalSize,
}

impl Tolerance {
    /// The effective (upper, lower) deviations of the type.
    pub fn deviations(&self) -> (Real, Real) {
        match self.kind {
            TolKind::Default | TolKind::Basic => (0.0, 0.0),
            TolKind::Symmetric => (self.upper.abs(), -self.upper.abs()),
            TolKind::Deviation | TolKind::Limits => (self.upper, self.lower),
            TolKind::Max => (0.0, self.lower.min(0.0)),
            TolKind::Min => (self.upper.max(0.0), 0.0),
        }
    }

    /// Model value − nominal for the evaluated size.
    pub fn offset(&self) -> Real {
        let (u, l) = self.deviations();
        match self.eval {
            EvalSize::Nominal => 0.0,
            EvalSize::Upper => u,
            EvalSize::Lower => l,
            EvalSize::Median => (u + l) / 2.0,
        }
    }

    /// Text after (or around) the value, in display units.
    pub fn format(
        &self,
        nominal: Real,
        to_display: impl Fn(Real) -> Real,
        digits: usize,
    ) -> String {
        let f = |v: Real| format!("{:.*}", digits, to_display(v));
        let (u, l) = self.deviations();
        match self.kind {
            TolKind::Default => f(nominal),
            TolKind::Basic => format!("[{}]", f(nominal)),
            TolKind::Symmetric => format!("{} ±{}", f(nominal), f(u)),
            TolKind::Deviation => format!("{} +{}/{}", f(nominal), f(u), f(l)),
            TolKind::Limits => format!("{}/{}", f(nominal + u), f(nominal + l)),
            TolKind::Max => format!("{} max", f(nominal)),
            TolKind::Min => format!("{} min", f(nominal)),
        }
    }
}

/// The parameter side of a dimension.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DimParam {
    /// Parameter name (`d0`); empty until the part names it.
    #[serde(default)]
    pub name: String,
    /// Equation, when the value comes from one (`d0 / 2`); `None` = a
    /// plain value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expr: Option<String>,
    /// Tolerance.
    #[serde(default)]
    pub tol: Tolerance,
    /// Comment (Parameters table).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
}

/// How dimension texts are shown (Inventor's status-bar choice).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DimDisplay {
    /// The value (`fx:` before values from equations).
    #[default]
    Value,
    /// The parameter name.
    Name,
    /// `name = equation`.
    Expression,
    /// The value with its tolerance.
    Tolerance,
}

impl DimDisplay {
    /// All modes.
    pub const ALL: [DimDisplay; 4] = [
        DimDisplay::Value,
        DimDisplay::Name,
        DimDisplay::Expression,
        DimDisplay::Tolerance,
    ];

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            DimDisplay::Value => "Value",
            DimDisplay::Name => "Name",
            DimDisplay::Expression => "Expression",
            DimDisplay::Tolerance => "Tolerance",
        }
    }
}

impl Sketch {
    /// The parameter data of dimension `id` (default if none).
    pub fn dim_param(&self, id: ConstraintId) -> DimParam {
        self.dim_params.get(id).cloned().unwrap_or_default()
    }

    /// Mutable parameter data of dimension `id` (created on demand).
    pub fn dim_param_mut(&mut self, id: ConstraintId) -> Option<&mut DimParam> {
        self.constraints.get(id).filter(|c| c.is_dimension())?;
        if !self.dim_params.contains_key(id) {
            self.dim_params.insert(id, DimParam::default());
        }
        self.dim_params.get_mut(id)
    }

    /// The nominal value of dimension `id` (its model value minus the
    /// tolerance offset of the evaluated size).
    pub fn dimension_nominal(&self, id: ConstraintId) -> Option<Real> {
        let v = self.constraint(id)?.value()?;
        Some(v - self.dim_param(id).tol.offset())
    }

    /// Sets the nominal value; the model value follows the tolerance.
    pub fn set_dimension_nominal(
        &mut self,
        id: ConstraintId,
        nominal: Real,
    ) -> Result<(), super::SketchError> {
        let off = self.dim_param(id).tol.offset();
        let v = self
            .constraint(id)
            .and_then(Constraint::value)
            .unwrap_or(nominal);
        // Keep the sign of signed dimensions (angles).
        let target = if v < 0.0 && nominal > 0.0 {
            -(nominal + off)
        } else {
            nominal + off
        };
        self.set_dimension(id, target)
    }

    /// Changes the tolerance; the model value moves with the evaluated size.
    pub fn set_dimension_tolerance(
        &mut self,
        id: ConstraintId,
        tol: Tolerance,
    ) -> Result<(), super::SketchError> {
        let nominal = self
            .dimension_nominal(id)
            .ok_or(super::SketchError::UnknownConstraint)?;
        let before = self.dim_param(id);
        self.dim_param_mut(id)
            .ok_or(super::SketchError::UnknownConstraint)?
            .tol = tol;
        if let Err(e) = self.set_dimension_nominal(id, nominal) {
            if let Some(p) = self.dim_param_mut(id) {
                *p = before;
            }
            return Err(e);
        }
        Ok(())
    }

    /// The text of dimension `id` in display mode `mode`.
    pub fn dimension_text(&self, id: ConstraintId, mode: DimDisplay) -> String {
        let Some(c) = self.constraint(id) else {
            return String::new();
        };
        let angle = matches!(c, Constraint::Angle { .. });
        let deg = |v: Real| if angle { v.to_degrees().abs() } else { v };
        let (digits, unit) = if angle { (1, "°") } else { (2, "") };
        let driven = self.is_driven(id);
        let value = if driven {
            self.measure(id).unwrap_or_default()
        } else {
            self.dimension_nominal(id).unwrap_or_default()
        };
        let p = self.dim_param(id);
        let prefix = match c {
            Constraint::Diameter { .. } | Constraint::DiameterToAxis { .. } => "Ø",
            Constraint::Radius { .. } => "R",
            _ => "",
        };
        let plain = format!("{prefix}{:.*}{unit}", digits, deg(value));
        let text = match mode {
            DimDisplay::Value => {
                if p.expr.is_some() {
                    format!("fx:{plain}")
                } else {
                    plain
                }
            }
            DimDisplay::Name if !p.name.is_empty() => p.name.clone(),
            DimDisplay::Expression if !p.name.is_empty() => match &p.expr {
                Some(e) => format!("{} = {e}", p.name),
                None => format!(
                    "{} = {:.*}{}",
                    p.name,
                    digits,
                    deg(value),
                    if angle { " deg" } else { " mm" }
                ),
            },
            DimDisplay::Tolerance => format!(
                "{prefix}{}{unit}",
                p.tol
                    .format(value, |v| if angle { v.to_degrees() } else { v }, digits)
            ),
            _ => plain,
        };
        if driven { format!("({text})") } else { text }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::Point2;

    #[test]
    fn tolerance_drives_the_model_value_and_shows() {
        let mut s = Sketch::default();
        let a = s.add_point(Point2::new(0.0, 0.0));
        let b = s.add_point(Point2::new(25.0, 0.0));
        let d = s
            .add_constraint(Constraint::Distance { a, b, value: 25.0 })
            .unwrap();
        let tol = Tolerance {
            kind: TolKind::Deviation,
            upper: 0.1,
            lower: -0.05,
            eval: EvalSize::Upper,
        };
        s.set_dimension_tolerance(d, tol).unwrap();
        assert!((s.constraint(d).unwrap().value().unwrap() - 25.1).abs() < 1e-12);
        assert!((s.dimension_nominal(d).unwrap() - 25.0).abs() < 1e-12);
        assert_eq!(
            s.dimension_text(d, DimDisplay::Tolerance),
            "25.00 +0.10/-0.05"
        );
        s.dim_param_mut(d).unwrap().name = "d0".into();
        assert_eq!(s.dimension_text(d, DimDisplay::Name), "d0");
        assert_eq!(s.dimension_text(d, DimDisplay::Expression), "d0 = 25.00 mm");
        s.set_dimension_nominal(d, 30.0).unwrap();
        assert!((s.constraint(d).unwrap().value().unwrap() - 30.1).abs() < 1e-12);
        let mut lim = tol;
        lim.kind = TolKind::Limits;
        lim.eval = EvalSize::Median;
        s.set_dimension_tolerance(d, lim).unwrap();
        assert!((s.constraint(d).unwrap().value().unwrap() - 30.025).abs() < 1e-12);
        assert_eq!(s.dimension_text(d, DimDisplay::Tolerance), "30.10/29.95");
    }
}
