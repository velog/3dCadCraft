//! Reading a tool's JSON arguments, with errors in the agent's terms.

use serde_json::Value;

use super::session::ApiError;
use crate::kernel::{Point3, Real};
use crate::sketch::Point2;

/// The arguments of one call and the command they belong to (for error texts).
pub struct Args<'a> {
    /// The command's name (`Extrude`).
    pub cmd: &'a str,
    /// The arguments object.
    pub v: &'a Value,
}

impl<'a> Args<'a> {
    /// Arguments `v` of command `cmd`.
    pub fn new(cmd: &'a str, v: &'a Value) -> Self {
        Self { cmd, v }
    }

    /// An error of this command.
    pub fn err(&self, reason: impl Into<String>) -> ApiError {
        ApiError::new(self.cmd, reason)
    }

    /// The raw value of `key`, if given (`null` counts as not given).
    pub fn get(&self, key: &str) -> Option<&'a Value> {
        self.v.get(key).filter(|x| !x.is_null())
    }

    /// A required text.
    pub fn str(&self, key: &str) -> Result<&'a str, ApiError> {
        self.opt_str(key)?
            .ok_or_else(|| self.err(format!("'{key}' is required")))
    }

    /// An optional text.
    pub fn opt_str(&self, key: &str) -> Result<Option<&'a str>, ApiError> {
        match self.get(key) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.as_str())),
            Some(_) => Err(self.err(format!("'{key}' must be a text"))),
        }
    }

    /// A required number.
    pub fn num(&self, key: &str) -> Result<Real, ApiError> {
        self.opt_num(key)?
            .ok_or_else(|| self.err(format!("'{key}' is required")))
    }

    /// An optional number.
    pub fn opt_num(&self, key: &str) -> Result<Option<Real>, ApiError> {
        match self.get(key) {
            None => Ok(None),
            Some(x) => x
                .as_f64()
                .filter(|f| f.is_finite())
                .map(Some)
                .ok_or_else(|| self.err(format!("'{key}' must be a number"))),
        }
    }

    /// An optional flag (default false).
    pub fn flag(&self, key: &str) -> Result<bool, ApiError> {
        match self.get(key) {
            None => Ok(false),
            Some(Value::Bool(b)) => Ok(*b),
            Some(_) => Err(self.err(format!("'{key}' must be true or false"))),
        }
    }

    /// An optional count (a whole number ≥ 0).
    pub fn opt_count(&self, key: &str) -> Result<Option<u32>, ApiError> {
        match self.get(key) {
            None => Ok(None),
            Some(x) => x
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .map(Some)
                .ok_or_else(|| self.err(format!("'{key}' must be a whole number"))),
        }
    }

    /// A list of texts (missing = empty).
    pub fn strs(&self, key: &str) -> Result<Vec<String>, ApiError> {
        match self.get(key) {
            None => Ok(Vec::new()),
            Some(Value::Array(a)) => a
                .iter()
                .map(|x| {
                    x.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| self.err(format!("'{key}' must list texts")))
                })
                .collect(),
            Some(Value::String(s)) => Ok(vec![s.clone()]),
            Some(_) => Err(self.err(format!("'{key}' must be a list of texts"))),
        }
    }

    /// One of `choices` (case-insensitive), or `default` when not given.
    pub fn choice(
        &self,
        key: &str,
        choices: &[&'static str],
        default: &'static str,
    ) -> Result<&'static str, ApiError> {
        let Some(s) = self.opt_str(key)? else {
            return Ok(default);
        };
        choices
            .iter()
            .find(|c| c.eq_ignore_ascii_case(s))
            .copied()
            .ok_or_else(|| self.err(format!("'{key}' must be one of {}", choices.join(", "))))
    }
}

/// `[x, y]` as a 2D point.
pub fn point2(a: &Args, v: &Value, what: &str) -> Result<Point2, ApiError> {
    let xs = numbers(a, v, what)?;
    match xs.as_slice() {
        [x, y] => Ok(Point2::new(*x, *y)),
        _ => Err(a.err(format!("'{what}' must be [x, y] (mm)"))),
    }
}

/// `[x, y, z]` as a 3D point.
pub fn point3(a: &Args, v: &Value, what: &str) -> Result<Point3, ApiError> {
    let xs = numbers(a, v, what)?;
    match xs.as_slice() {
        [x, y, z] => Ok(Point3::new(*x, *y, *z)),
        _ => Err(a.err(format!("'{what}' must be [x, y, z] (mm)"))),
    }
}

fn numbers(a: &Args, v: &Value, what: &str) -> Result<Vec<Real>, ApiError> {
    v.as_array()
        .ok_or_else(|| a.err(format!("'{what}' must be a list of numbers")))?
        .iter()
        .map(|x| {
            x.as_f64()
                .filter(|f| f.is_finite())
                .ok_or_else(|| a.err(format!("'{what}' must be a list of numbers")))
        })
        .collect()
}

/// A number rounded for answers (agents read text: 1e-6 mm is plenty).
pub fn round(x: Real) -> Real {
    // `+ 0.0` turns -0.0 into 0.0
    (x * 1e6).round() / 1e6 + 0.0
}

/// A point as a rounded `[x, y, z]`.
pub fn p3(p: &Point3) -> [Real; 3] {
    [round(p.x), round(p.y), round(p.z)]
}
