//! Drawing resources (spec 4.13): the user's borders, title blocks and
//! sketched symbols. First part: the parameters of a zone border, kept per
//! sheet (`Sheet::border_params`; `None` = the standard's default border).

use serde::{Deserialize, Serialize};

/// How the zones along one side are labelled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneLabel {
    /// 1, 2, 3 …
    #[default]
    Numeric,
    /// A, B, C … (then AA, AB …).
    Alphabetical,
    /// No labels.
    None,
}

impl ZoneLabel {
    /// Every choice, in the panel's order.
    pub const ALL: [ZoneLabel; 3] = [ZoneLabel::Numeric, ZoneLabel::Alphabetical, ZoneLabel::None];

    /// Name in the panel.
    pub fn label(self) -> &'static str {
        match self {
            ZoneLabel::Numeric => "Numeric",
            ZoneLabel::Alphabetical => "Alphabetical",
            ZoneLabel::None => "None",
        }
    }

    /// The label of zone `k` (counted from 0), or `None`.
    pub fn text(self, k: usize) -> Option<String> {
        match self {
            ZoneLabel::Numeric => Some(format!("{}", k + 1)),
            ZoneLabel::Alphabetical => Some(letters(k)),
            ZoneLabel::None => None,
        }
    }
}

/// A, B, … Z, AA, AB … for 0, 1, … 25, 26, 27 …
fn letters(mut k: usize) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (k % 26) as u8);
        if k < 26 {
            break;
        }
        k = k / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

/// The corner the zone labels are counted from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LabelFrom {
    /// Numbers from the right, letters from the bottom (ISO default).
    #[default]
    BottomRight,
    /// Numbers from the left, letters from the top.
    TopLeft,
}

/// What separates two zones in the margin.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Delimit {
    /// A short line across the margin.
    #[default]
    Line,
    /// A filled arrowhead pointing at the frame.
    Arrowhead,
}

/// Parameters of a border (Inventor's Default Drawing Border Parameters).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ZoneParams {
    /// Zones drawn (Zone Border); `false` = a plain frame (Border).
    pub zones: bool,
    /// Zones along the top and bottom.
    pub horizontal: usize,
    /// Their labels.
    pub h_label: ZoneLabel,
    /// Zones up the sides.
    pub vertical: usize,
    /// Their labels.
    pub v_label: ZoneLabel,
    /// The corner the labels count from.
    pub from: LabelFrom,
    /// Line or arrowhead between zones.
    pub delimit: Delimit,
    /// Centring marks on the four sides.
    pub center_marks: bool,
    /// Sheet margins top, bottom, left, right (mm); `None` = the standard's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub margins: Option<[f64; 4]>,
}

impl Default for ZoneParams {
    fn default() -> Self {
        Self {
            zones: true,
            horizontal: super::sheet::ZONES.0,
            h_label: ZoneLabel::Numeric,
            vertical: super::sheet::ZONES.1,
            v_label: ZoneLabel::Alphabetical,
            from: LabelFrom::BottomRight,
            delimit: Delimit::Line,
            center_marks: true,
            margins: None,
        }
    }
}

impl ZoneParams {
    /// The parameters with the counts held to what can be drawn (1–100).
    pub fn clamped(&self) -> Self {
        let mut p = self.clone();
        p.horizontal = p.horizontal.clamp(1, 100);
        p.vertical = p.vertical.clamp(1, 100);
        if let Some(m) = &mut p.margins {
            for v in m.iter_mut() {
                *v = if v.is_finite() {
                    v.clamp(0.0, 200.0)
                } else {
                    10.0
                };
            }
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_go_past_z() {
        assert_eq!(letters(0), "A");
        assert_eq!(letters(25), "Z");
        assert_eq!(letters(26), "AA");
        assert_eq!(letters(27), "AB");
        assert_eq!(letters(52), "BA");
    }

    #[test]
    fn params_round_trip_and_old_sheets_keep_the_default() {
        let p = ZoneParams {
            horizontal: 8,
            h_label: ZoneLabel::Alphabetical,
            from: LabelFrom::TopLeft,
            delimit: Delimit::Arrowhead,
            margins: Some([5.0, 10.0, 20.0, 5.0]),
            ..ZoneParams::default()
        };
        let text = serde_json::to_string(&p).unwrap_or_default();
        let back: ZoneParams = serde_json::from_str(&text).unwrap_or_default();
        assert_eq!(back, p);
        let partial: ZoneParams = serde_json::from_str("{\"horizontal\":3}").unwrap_or_default();
        assert_eq!(partial.horizontal, 3);
        assert_eq!(partial.vertical, 4);
        assert!(partial.center_marks);
    }

    fn border(params: Option<ZoneParams>) -> Vec<crate::drawing::sheet::Prim> {
        let mut d = crate::drawing::DrawingDocument::default();
        d.sheets[0].title_block = false;
        d.sheets[0].border_params = params;
        crate::drawing::sheet::format_prims(&d, 0, &|_| String::new())
    }

    fn texts(p: &[crate::drawing::sheet::Prim]) -> Vec<(String, [f64; 2])> {
        p.iter()
            .filter_map(|p| match p {
                crate::drawing::sheet::Prim::Text { text, at, .. } => Some((text.clone(), *at)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_default_parameters_draw_the_default_border() {
        use crate::drawing::sheet::Prim;
        let none = border(None);
        let default = border(Some(ZoneParams::default()));
        assert_eq!(none, default);
        // Frame 4, zone lines 2·5 + 2·3, centring marks 4 × 4.
        let lines = none
            .iter()
            .filter(|p| matches!(p, Prim::Line { .. }))
            .count();
        assert_eq!(lines, 4 + 10 + 6 + 16);
        let t = texts(&none);
        assert_eq!(t.len(), 2 * 6 + 2 * 4);
        // A3 landscape, 10 mm margins: zone 6 at the bottom left, A at the bottom.
        let six = t
            .iter()
            .find(|(s, _)| s == "6")
            .map(|x| x.1)
            .unwrap_or_default();
        assert!(six[0] < 100.0 && six[1] < 10.0, "{six:?}");
        let a = t
            .iter()
            .find(|(s, _)| s == "A")
            .map(|x| x.1)
            .unwrap_or_default();
        assert!(a[1] < 100.0, "{a:?}");
    }

    #[test]
    fn counts_labels_corner_and_margins_follow_the_parameters() {
        use crate::drawing::sheet::Prim;
        let p = ZoneParams {
            horizontal: 8,
            vertical: 3,
            h_label: ZoneLabel::Alphabetical,
            v_label: ZoneLabel::Numeric,
            from: LabelFrom::TopLeft,
            margins: Some([5.0, 15.0, 20.0, 5.0]),
            ..ZoneParams::default()
        };
        let prims = border(Some(p.clone()));
        let t = texts(&prims);
        assert_eq!(t.len(), 2 * 8 + 2 * 3);
        // Top/Left: A is the leftmost column, 1 the top row.
        let a = t
            .iter()
            .filter(|(s, _)| s == "A")
            .map(|x| x.1[0])
            .fold(f64::MAX, f64::min);
        let h = t
            .iter()
            .filter(|(s, _)| s == "H")
            .map(|x| x.1[0])
            .fold(f64::MAX, f64::min);
        assert!(a < h);
        let one = t
            .iter()
            .filter(|(s, _)| s == "1")
            .map(|x| x.1[1])
            .fold(f64::MAX, f64::min);
        let three = t
            .iter()
            .filter(|(s, _)| s == "3")
            .map(|x| x.1[1])
            .fold(f64::MAX, f64::min);
        assert!(one > three);
        // The frame follows the margins: A3 is 420 × 297.
        let mut d = crate::drawing::DrawingDocument::default();
        d.sheets[0].border_params = Some(p);
        let (lo, hi) = crate::drawing::sheet::frame(&d.sheets[0]);
        assert_eq!((lo, hi), ([20.0, 15.0], [415.0, 292.0]));
        // A plain border without centring marks is the frame alone.
        let plain = border(Some(ZoneParams {
            zones: false,
            center_marks: false,
            ..ZoneParams::default()
        }));
        assert_eq!(plain.len(), 4);
        // Arrowheads instead of lines between zones.
        let arrows = border(Some(ZoneParams {
            delimit: Delimit::Arrowhead,
            ..ZoneParams::default()
        }));
        let fills = arrows
            .iter()
            .filter(|p| matches!(p, Prim::Fill { .. }))
            .count();
        assert_eq!(fills, 10 + 6);
        // No labels.
        let bare = border(Some(ZoneParams {
            h_label: ZoneLabel::None,
            v_label: ZoneLabel::None,
            ..ZoneParams::default()
        }));
        assert!(texts(&bare).is_empty());
    }
}
