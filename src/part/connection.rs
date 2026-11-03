//! Connections (`docs/spec/6.3-content-center.md` §3.5): named geometry a part
//! offers for being placed. A connection is the part's own data (rule 8) and
//! names its edge by persistent names (rule 2), so it survives regeneration.

use serde::{Deserialize, Serialize};

use crate::kernel::Real;
use crate::part::blend::EdgeRef;

/// How a connection takes its place.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionRole {
    /// It goes into a hole.
    #[default]
    Into,
    /// It goes over a shaft.
    Over,
}

impl ConnectionRole {
    /// Its name in the interface.
    pub fn label(self) -> &'static str {
        match self {
            ConnectionRole::Into => "Goes into a hole",
            ConnectionRole::Over => "Goes over a shaft",
        }
    }
}

/// One named connection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Connection {
    /// Its name, unique in the part.
    pub name: String,
    /// How it takes its place.
    #[serde(default)]
    pub role: ConnectionRole,
    /// The circular edge that lands on the target; `None`: the part's Z axis
    /// and XY plane (the library's parts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<EdgeRef>,
    /// The diameter a target must have, in mm.
    pub diameter: Real,
    /// The family table's column that gives each member's diameter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diameter_column: Option<String>,
}

impl Connection {
    /// A library part's: its axis and seat are the part's origin.
    pub fn seat(role: ConnectionRole, diameter: Real) -> Self {
        Connection {
            name: "Seat".into(),
            role,
            edge: None,
            diameter,
            diameter_column: None,
        }
    }
}

/// A name not yet used in `list`: `Connection1`, `Connection2`...
pub fn next_name(list: &[Connection]) -> String {
    (1..)
        .map(|k| format!("Connection{k}"))
        .find(|n| !list.iter().any(|c| &c.name == n))
        .unwrap_or_default()
}
