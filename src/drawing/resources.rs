//! Drawing resources (spec 4.13): the user's borders, title blocks and
//! sketched symbols. The parameters of a zone border are kept per sheet
//! (`Sheet::border_params`; `None` = the standard's default border); the
//! user's title blocks are definitions in [`Resources`], placed on a sheet
//! by name (`Sheet::title_def`; `None` = the standard's title block).

use serde::{Deserialize, Serialize};

use super::fields::{self, Field};
use super::sheet::Prim;
use super::view_sketch::{DrawingSketch, DrawingText, ViewSketch};
use super::{DrawingDocument, Sheet};

/// The definitions of a drawing (Drawing Resources).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Resources {
    /// Title blocks, in browser order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub title_blocks: Vec<Definition>,
}

impl Resources {
    /// No definitions.
    pub fn is_empty(&self) -> bool {
        self.title_blocks.is_empty()
    }
}

/// A definition: a sketch with texts, in sheet mm as it was drawn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    /// Its name, unique in its folder.
    pub name: String,
    /// Lines, arcs, circles.
    #[serde(default)]
    pub sketch: ViewSketch,
    /// Texts and fields.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub texts: Vec<DrawingText>,
}

impl Definition {
    /// As a drawing sketch (to draw it and take its extent).
    fn as_sketch(&self) -> DrawingSketch {
        DrawingSketch {
            id: 0,
            name: self.name.clone(),
            visible: true,
            sketch: self.sketch.clone(),
            texts: self.texts.clone(),
        }
    }

    /// The box of its curves (lower left, upper right); a definition of
    /// texts only: the box of its texts. Texts do not widen a drawn block:
    /// a field's value is not the length of its name.
    pub fn extent(&self) -> Option<([f64; 2], [f64; 2])> {
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        let mut grow = |q: [f64; 2]| {
            for k in 0..2 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        };
        let curves = self.as_sketch().curves();
        let drawn = !curves.is_empty();
        for (pts, _) in curves {
            pts.into_iter().for_each(&mut grow);
        }
        for t in self.texts.iter().filter(|_| !drawn) {
            let s = fields::tokens(&t.pieces);
            if let Some(Prim::Text {
                at, height, angle, ..
            }) = super::render::text_prim(&s, t, t.at)
            {
                let w = super::rich::text_advance(&s, height);
                let (sn, c) = angle.sin_cos();
                for (u, v) in [(0.0, 0.0), (w, 0.0), (0.0, height), (w, height)] {
                    grow([at[0] + u * c - v * sn, at[1] + u * sn + v * c]);
                }
            }
        }
        (lo[0] <= hi[0] && lo[1] <= hi[1]).then_some((lo, hi))
    }

    /// Moves it by `d`.
    pub fn translate(&mut self, d: [f64; 2]) {
        self.sketch.translate(d);
        for t in &mut self.texts {
            t.at = [t.at[0] + d[0], t.at[1] + d[1]];
        }
    }

    /// Moved so that its lower right corner is the origin (how a title
    /// block is kept: its corner goes on the frame's).
    pub fn normalised(mut self) -> Self {
        if let Some((lo, hi)) = self.extent() {
            self.translate([-hi[0], -lo[1]]);
        }
        self
    }

    /// Its prompted entries, in order.
    pub fn prompts(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for t in &self.texts {
            for p in fields::prompts(&t.pieces) {
                if !v.contains(&p) {
                    v.push(p);
                }
            }
        }
        v
    }
}

/// A title block definition placed on a sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacedTitle {
    /// The definition's name.
    pub name: String,
    /// The prompted entries' values on this sheet (entry title, value).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompts: Vec<(String, String)>,
}

impl PlacedTitle {
    /// The value of prompted entry `title` (`None`: not filled in).
    pub fn prompt(&self, title: &str) -> Option<String> {
        self.prompts
            .iter()
            .find(|(t, _)| t == title)
            .map(|(_, v)| v.clone())
    }
}

/// The title block definition sheet `sheet` uses, when it is the user's.
pub fn title_of<'a>(doc: &'a DrawingDocument, sheet: &Sheet) -> Option<&'a Definition> {
    if !sheet.title_block {
        return None;
    }
    let p = sheet.title_def.as_ref()?;
    doc.resources.title_blocks.iter().find(|d| d.name == p.name)
}

/// How far a definition is moved onto sheet `sheet`: its lower right corner
/// on the frame's (Inventor anchors the block's outermost corner on the
/// border's corner).
pub fn title_offset(def: &Definition, sheet: &Sheet) -> Option<[f64; 2]> {
    let (lo, hi) = def.extent()?;
    let (flo, fhi) = super::sheet::frame(sheet);
    Some([fhi[0] - hi[0], flo[1] - lo[1]])
}

/// The user's title block of sheet `i` as primitives (empty when the sheet
/// shows the standard's): its curves and texts moved onto the frame's
/// corner, fields filled from `props`, the sheet and its prompted values.
pub fn title_prims(
    doc: &DrawingDocument,
    i: usize,
    props: &dyn Fn(&Field) -> Option<String>,
) -> Vec<Prim> {
    let Some(sheet) = doc.sheets.get(i) else {
        return Vec::new();
    };
    let Some(def) = title_of(doc, sheet) else {
        return Vec::new();
    };
    let Some(off) = title_offset(def, sheet) else {
        return Vec::new();
    };
    let placed = sheet.title_def.clone();
    let mv = |q: [f64; 2]| [q[0] + off[0], q[1] + off[1]];
    let weight = doc.styles.layer(&doc.styles.defaults.border_layer).weight;
    let mut out = Vec::new();
    for (pts, look) in def.as_sketch().curves() {
        out.push(Prim::Path {
            pts: pts.into_iter().map(mv).collect(),
            width: weight,
            dashed: look == super::view_sketch::CurveLook::Chain,
        });
    }
    for t in &def.texts {
        let prompt = |title: &str| placed.as_ref().and_then(|p| p.prompt(title));
        let text = fields::expand(&t.pieces, doc, i, props, &prompt);
        out.extend(super::render::text_prim(&text, t, mv(t.at)));
    }
    out
}

impl DrawingDocument {
    /// Adds title block definition `def` (Save Title Block); refused when
    /// its name is empty or taken.
    pub fn add_title_block(&mut self, def: Definition) -> Result<(), String> {
        let name = def.name.trim().to_string();
        if name.is_empty() {
            return Err("Define Title Block: the title block needs a name".into());
        }
        if self.resources.title_blocks.iter().any(|d| d.name == name) {
            return Err(format!(
                "Define Title Block: a title block named {name} exists"
            ));
        }
        self.resources.title_blocks.push(Definition { name, ..def });
        Ok(())
    }

    /// Replaces the sketch and texts of title block `name` (Edit
    /// Definition): every sheet using it changes; prompted values of
    /// entries that left it are dropped.
    pub fn update_title_block(
        &mut self,
        name: &str,
        sketch: ViewSketch,
        texts: Vec<DrawingText>,
    ) -> bool {
        let Some(d) = self
            .resources
            .title_blocks
            .iter_mut()
            .find(|d| d.name == name)
        else {
            return false;
        };
        d.sketch = sketch;
        d.texts = texts;
        let kept = d.prompts();
        for s in &mut self.sheets {
            if let Some(p) = s.title_def.as_mut().filter(|p| p.name == name) {
                p.prompts.retain(|(t, _)| kept.contains(t));
            }
        }
        true
    }

    /// Puts title block `name` on sheet `i` (`None`: the standard's), with
    /// the prompted values `prompts`; false when there is no such sheet or
    /// definition.
    pub fn set_title_block(
        &mut self,
        i: usize,
        name: Option<&str>,
        prompts: Vec<(String, String)>,
    ) -> bool {
        if let Some(n) = name
            && !self.resources.title_blocks.iter().any(|d| d.name == n)
        {
            return false;
        }
        let Some(s) = self.sheets.get_mut(i) else {
            return false;
        };
        s.title_block = true;
        s.title_def = name.map(|n| PlacedTitle {
            name: n.to_string(),
            prompts,
        });
        true
    }

    /// Deletes title block definition `name`; refused while a sheet uses
    /// it (naming the sheet).
    pub fn delete_title_block(&mut self, name: &str) -> Result<(), String> {
        if let Some(s) = self
            .sheets
            .iter()
            .find(|s| s.title_def.as_ref().is_some_and(|p| p.name == name))
        {
            return Err(format!("Delete: {} uses title block {name}", s.name));
        }
        let before = self.resources.title_blocks.len();
        self.resources.title_blocks.retain(|d| d.name != name);
        if self.resources.title_blocks.len() == before {
            return Err(format!("Delete: no title block named {name}"));
        }
        Ok(())
    }

    /// A free title block name (`Title Block1`, `Title Block2` …).
    pub fn next_title_block_name(&self) -> String {
        (1..)
            .map(|k| format!("Title Block{k}"))
            .find(|n| !self.resources.title_blocks.iter().any(|d| &d.name == n))
            .unwrap_or_default()
    }
}

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

    fn block() -> Definition {
        use crate::drawing::view_sketch::TextAnchor;
        let mut sk = crate::sketch::Sketch::default();
        let p = |x, y| crate::sketch::Point2::new(x, y);
        let a = sk.add_point(p(0.0, 0.0));
        let b = sk.add_point(p(100.0, 0.0));
        let c = sk.add_point(p(100.0, 20.0));
        let d = sk.add_point(p(0.0, 20.0));
        for (u, v) in [(a, b), (b, c), (c, d), (d, a)] {
            let _ = sk.add_line_between(u, v);
        }
        Definition {
            name: "Company".into(),
            sketch: ViewSketch(sk),
            texts: vec![DrawingText {
                at: [5.0, 5.0],
                height: 3.5,
                anchor: TextAnchor::Left,
                angle: 0.0,
                pieces: fields::parse("<Model: Part Number> / <?Order> / <Sheet Number>"),
            }],
        }
    }

    #[test]
    fn a_definition_is_kept_with_its_lower_right_corner_at_the_origin() {
        let mut b = block();
        b.translate([50.0, 70.0]);
        let n = b.normalised();
        assert_eq!(n.extent(), Some(([-100.0, 0.0], [0.0, 20.0])));
        assert_eq!(n.texts[0].at, [-95.0, 5.0]);
        assert_eq!(n.prompts(), vec!["Order".to_string()]);
    }

    #[test]
    fn a_title_block_sits_on_the_frame_corner_with_its_fields() {
        use crate::drawing::sheet::Prim;
        let mut d = DrawingDocument::default();
        assert!(d.add_title_block(block()).is_ok());
        assert!(d.add_title_block(block()).is_err(), "names are unique");
        assert_eq!(d.next_title_block_name(), "Title Block1");
        assert!(!d.set_title_block(0, Some("Nope"), Vec::new()));
        assert!(d.set_title_block(0, Some("Company"), vec![("Order".into(), "42".into())]));
        let props =
            |f: &Field| (*f == Field::Model("part_number".into())).then(|| "P-1".to_string());
        let prims = title_prims(&d, 0, &props);
        // A3 landscape, 10 mm margins: the block's lower right at (410, 10).
        let pts: Vec<[f64; 2]> = prims
            .iter()
            .filter_map(|p| match p {
                Prim::Path { pts, .. } => Some(pts.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        assert!(!pts.is_empty());
        let max_x = pts.iter().map(|q| q[0]).fold(f64::MIN, f64::max);
        let min_x = pts.iter().map(|q| q[0]).fold(f64::MAX, f64::min);
        let min_y = pts.iter().map(|q| q[1]).fold(f64::MAX, f64::min);
        let max_y = pts.iter().map(|q| q[1]).fold(f64::MIN, f64::max);
        assert!((max_x - 410.0).abs() < 1e-6 && (min_y - 10.0).abs() < 1e-6);
        assert!((min_x - 310.0).abs() < 1e-6 && (max_y - 30.0).abs() < 1e-6);
        let text = prims.iter().find_map(|p| match p {
            Prim::Text { text, at, .. } => Some((text.clone(), *at)),
            _ => None,
        });
        assert_eq!(text, Some(("P-1 / 42 / 1".to_string(), [315.0, 15.0])));
        // The frame's corner rectangle is the user's block; the standard's
        // block is not drawn.
        let rect = crate::drawing::sheet::title_rect_in(&d, &d.sheets[0]);
        assert_eq!(rect, Some(([310.0, 10.0], [410.0, 30.0])));
        let builtin = crate::drawing::sheet::format_prims(&d, 0, &|_| String::new());
        assert!(
            !builtin
                .iter()
                .any(|p| matches!(p, Prim::Text { text, .. } if text == "Designed by"))
        );
        // In use: no Delete; editing drops prompts that left it.
        assert!(d.delete_title_block("Company").is_err());
        assert!(d.update_title_block("Company", ViewSketch::default(), Vec::new()));
        assert!(
            d.sheets[0]
                .title_def
                .as_ref()
                .is_some_and(|p| p.prompts.is_empty())
        );
        assert!(d.set_title_block(0, None, Vec::new()));
        assert!(d.delete_title_block("Company").is_ok());
        // Saved and read back.
        assert!(d.add_title_block(block()).is_ok());
        assert!(d.set_title_block(0, Some("Company"), Vec::new()));
        let json = serde_json::to_string(&d).unwrap_or_default();
        let back: DrawingDocument = serde_json::from_str(&json).unwrap_or_default();
        assert_eq!(back.resources, d.resources);
        assert_eq!(back.sheets[0].title_def, d.sheets[0].title_def);
    }
}
