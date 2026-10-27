//! Chamfer Note (spec 4.7 §3.3, format revision 33): a leader note on a
//! chamfer seen edge-on in a view, written from two straight edges of that
//! view — the chamfer's edge and a reference edge that touches or crosses
//! it. Like Inventor, the values are measured against the reference:
//! `<DISTANCE1>` is the chamfer's length along the reference edge,
//! `<DISTANCE2>` its length square to it and `<ANGLE>` the acute angle
//! between the two edges — so the same chamfer reads `2 X 30°` from one
//! side and `1,15 X 60°` from the other. Both edges are kept by persistent
//! name (rule 2): the note follows the model and is orphaned when one goes.

use serde::{Deserialize, Serialize};

use super::hlr::ViewLines;
use super::style::DimensionStyle;

/// The format Inventor's ISO dimension style writes (`2,00 X 30,00°`).
pub const DEFAULT_FORMAT: &str = "<DISTANCE1> X <ANGLE>";

/// The tokens a format may hold, with what each inserts.
pub const TOKENS: [(&str, &str); 3] = [
    ("<DISTANCE1>", "Distance 1: along the reference edge"),
    ("<DISTANCE2>", "Distance 2: square to the reference edge"),
    ("<ANGLE>", "Angle to the reference edge"),
];

/// A chamfer note.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChamferNote {
    /// The view.
    pub view: u32,
    /// The chamfer's edge (persistent name).
    pub chamfer: String,
    /// The reference edge (persistent name).
    pub reference: String,
    /// Where the leader ends on the chamfer edge (0 = its start, 1 = its
    /// end; Inventor starts at the middle).
    #[serde(default = "half")]
    pub along: f64,
    /// The text's position relative to the view's position (sheet mm).
    pub text: [f64; 2],
    /// The note's own text with tokens (`None`: [`DEFAULT_FORMAT`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

fn half() -> f64 {
    0.5
}

/// The measured values of a chamfer note (view-plane mm, degrees).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Values {
    /// Along the reference edge.
    pub distance1: f64,
    /// Square to the reference edge.
    pub distance2: f64,
    /// Acute angle between the edges (degrees).
    pub angle: f64,
}

/// Why two edges cannot make a chamfer note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// An edge is a point in this view or not straight.
    NotStraight,
    /// The edges are parallel.
    Parallel,
    /// The edges are square to each other.
    Perpendicular,
    /// The edges neither touch nor cross.
    Apart,
}

impl Refused {
    /// The status-bar message.
    pub fn message(self) -> &'static str {
        match self {
            Self::NotStraight => "the edge is not a straight line in this view",
            Self::Parallel => "the reference edge is parallel to the chamfer",
            Self::Perpendicular => "the reference edge is square to the chamfer",
            Self::Apart => "the reference edge must touch or cross the chamfer",
        }
    }
}

/// The values of a chamfer `c` measured from the reference `r` (segments
/// in the view plane).
pub fn values(c: [[f64; 2]; 2], r: [[f64; 2]; 2]) -> Result<Values, Refused> {
    let dc = sub(c[1], c[0]);
    let dr = sub(r[1], r[0]);
    let (lc, lr) = (norm(dc), norm(dr));
    if lc < 1e-9 || lr < 1e-9 {
        return Err(Refused::NotStraight);
    }
    let cos = (dot(dc, dr) / (lc * lr)).abs().min(1.0);
    let sin = (cross(dc, dr) / (lc * lr)).abs().min(1.0);
    // 0.01° either way counts as parallel / square.
    if sin < 1.75e-4 {
        return Err(Refused::Parallel);
    }
    if cos < 1.75e-4 {
        return Err(Refused::Perpendicular);
    }
    let tol = 1e-4 * lc.max(lr) + 1e-6;
    if !segments_meet(c, r, tol) {
        return Err(Refused::Apart);
    }
    Ok(Values {
        distance1: lc * cos,
        distance2: lc * sin,
        angle: sin.atan2(cos).to_degrees(),
    })
}

/// The note's text: the format with its tokens replaced by the values in
/// the dimension style's precision and decimal marker. Like Inventor's
/// notes (and our bend notes), the angle takes the *linear* precision:
/// ISO writes `30,00°` here while its angular dimensions read `30°`.
pub fn text(format: &str, v: Values, d: &DimensionStyle) -> String {
    let num = |x: f64, precision: u8| {
        let mut t = format!("{:.*}", usize::from(precision), x);
        if !d.trailing_zeros && t.contains('.') {
            t = t.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        if d.decimal_comma {
            t = t.replace('.', ",");
        }
        t
    };
    format
        .replace("<DISTANCE1>", &num(v.distance1, d.precision))
        .replace("<DISTANCE2>", &num(v.distance2, d.precision))
        .replace("<ANGLE>", &format!("{}°", num(v.angle, d.precision)))
}

impl ChamferNote {
    /// A note of `chamfer` from `reference` in `view`, its text at `text`
    /// (relative to the view's position).
    pub fn new(view: u32, chamfer: &str, reference: &str, text: [f64; 2]) -> Self {
        Self {
            view,
            chamfer: chamfer.to_string(),
            reference: reference.to_string(),
            along: 0.5,
            text,
            format: None,
        }
    }

    /// The format in use.
    pub fn format(&self) -> &str {
        self.format.as_deref().unwrap_or(DEFAULT_FORMAT)
    }

    /// The chamfer segment and the values, from the view's lines; `Err`
    /// names the missing edge (or why the edges no longer fit).
    pub fn measure(&self, lines: &ViewLines) -> Result<([[f64; 2]; 2], Values), String> {
        let seg = |name: &str| {
            let e = lines.edge(name).ok_or_else(|| name.to_string())?;
            if !e.straight || e.circle.is_some() {
                return Err(name.to_string());
            }
            Ok([e.a, e.b])
        };
        let c = seg(&self.chamfer)?;
        let r = seg(&self.reference)?;
        let v = values(c, r).map_err(|e| e.message().to_string())?;
        Ok((c, v))
    }
}

/// Do two segments touch or cross (within `tol`)?
fn segments_meet(a: [[f64; 2]; 2], b: [[f64; 2]; 2], tol: f64) -> bool {
    let d = |p: [f64; 2], s: [[f64; 2]; 2]| {
        let v = sub(s[1], s[0]);
        let t = (dot(sub(p, s[0]), v) / dot(v, v)).clamp(0.0, 1.0);
        norm(sub(p, [s[0][0] + v[0] * t, s[0][1] + v[1] * t]))
    };
    if d(a[0], b) <= tol || d(a[1], b) <= tol || d(b[0], a) <= tol || d(b[1], a) <= tol {
        return true;
    }
    let side = |p: [f64; 2], s: [[f64; 2]; 2]| cross(sub(s[1], s[0]), sub(p, s[0]));
    side(a[0], b) * side(a[1], b) < 0.0 && side(b[0], a) * side(b[1], a) < 0.0
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn norm(a: [f64; 2]) -> f64 {
    a[0].hypot(a[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iso() -> DimensionStyle {
        super::super::style::AnnotationLook::default().dim
    }

    // Inventor's reference: a 40 x 30 block, a 2 mm x 30° chamfer at a
    // corner (legs 2 along the top edge, 1,1547 down the side) and an equal
    // 2 mm chamfer at another.
    const CHAMFER: [[f64; 2]; 2] = [[38.0, 30.0], [40.0, 28.845_299]];
    const TOP: [[f64; 2]; 2] = [[38.0, 30.0], [0.0, 30.0]];
    const SIDE: [[f64; 2]; 2] = [[40.0, 0.0], [40.0, 28.845_299]];

    #[test]
    fn the_reference_decides_distance_and_angle() {
        let d = iso();
        let top = values(CHAMFER, TOP).unwrap();
        assert_eq!(text(DEFAULT_FORMAT, top, &d), "2,00 X 30,00°");
        let side = values(CHAMFER, SIDE).unwrap();
        assert_eq!(text(DEFAULT_FORMAT, side, &d), "1,15 X 60,00°");
        let equal = values([[2.0, 0.0], [0.0, 2.0]], [[2.0, 0.0], [40.0, 0.0]]).unwrap();
        assert_eq!(text(DEFAULT_FORMAT, equal, &d), "2,00 X 45,00°");
        assert_eq!(text("<DISTANCE1> X <DISTANCE2>", side, &d), "1,15 X 2,00");
    }

    #[test]
    fn parallel_square_and_distant_references_are_refused() {
        let far = [[100.0, 0.0], [100.0, 10.0]];
        assert_eq!(values(CHAMFER, far), Err(Refused::Apart));
        let par = [[0.0, 0.0], [2.0, -1.154_7]];
        assert_eq!(values(CHAMFER, par), Err(Refused::Parallel));
        let sq = [[39.0, 29.0], [40.155, 31.0]];
        assert_eq!(values(CHAMFER, sq), Err(Refused::Perpendicular));
        assert_eq!(values([[0.0; 2], [0.0; 2]], TOP), Err(Refused::NotStraight));
    }

    #[test]
    fn a_crossing_reference_counts() {
        // A reference line drawn across the chamfer's middle.
        let across = [[39.0, 35.0], [39.0, 20.0]];
        let v = values(CHAMFER, across).unwrap();
        assert!((v.angle - 60.0).abs() < 1e-3);
    }

    #[test]
    fn round_trip_keeps_the_format_off_when_default() {
        let n = ChamferNote::new(3, "Chamfer1:E1", "Box1:E2", [10.0, 5.0]);
        let json = serde_json::to_string(&n).unwrap();
        assert!(!json.contains("format"));
        let back: ChamferNote = serde_json::from_str(&json).unwrap();
        assert_eq!(back, n);
    }
}
