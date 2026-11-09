//! Fields of drawing texts (spec 4.13 §3.2, spec 4.9 §3.4): a text is a list
//! of [`Piece`]s, plain text or a live value — a Property of the drawing or
//! of the model of the sheet's first view, a value of the sheet, or a
//! prompted entry filled in where the text is placed. Fields are evaluated
//! whenever the sheet is drawn; a field without a value shows `#?`.
//!
//! In a text box fields are written as tokens: `<Designer>` (a Property of
//! the drawing), `<Model: Part Number>`, `<Sheet Number>`, `<?Order>` (a
//! prompted entry). [`parse`] and [`tokens`] turn one into the other.

use serde::{Deserialize, Serialize};

use super::{DrawingDocument, scale_text};
use crate::document::iprops::{IProperties, STANDARD};

/// A value of the sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SheetField {
    /// The sheet's name (`Sheet:1`).
    Name,
    /// Its number among the counted sheets.
    Number,
    /// The number of counted sheets.
    Count,
    /// Its paper size (`A3`).
    Size,
    /// Its revision (Edit Sheet).
    Revision,
    /// The scale of its first view.
    Scale,
}

impl SheetField {
    /// Every value, in the Insert Field list's order.
    pub const ALL: [SheetField; 6] = [
        SheetField::Name,
        SheetField::Number,
        SheetField::Count,
        SheetField::Size,
        SheetField::Revision,
        SheetField::Scale,
    ];

    /// Its token's name.
    pub fn label(self) -> &'static str {
        match self {
            SheetField::Name => "Sheet Name",
            SheetField::Number => "Sheet Number",
            SheetField::Count => "Number of Sheets",
            SheetField::Size => "Sheet Size",
            SheetField::Revision => "Sheet Revision",
            SheetField::Scale => "Scale",
        }
    }
}

/// Where a field's value comes from.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Field {
    /// A Property of the drawing: a standard key (`designer`) or a custom
    /// property's name.
    Drawing(String),
    /// A Property of the model of the sheet's first view.
    Model(String),
    /// The material of that model.
    Material,
    /// A value of the sheet.
    Sheet(SheetField),
    /// A parameter of that model, by name (format revision 70).
    Parameter(String),
    /// A physical property of that model.
    Physical(PhysicalField),
}

/// The physical properties a text can read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PhysicalField {
    /// Mass (g).
    Mass,
    /// Volume (mm³).
    Volume,
    /// Surface area (mm²).
    Area,
}

impl PhysicalField {
    /// Every value, in the Insert Field list's order.
    pub const ALL: [PhysicalField; 3] = [Self::Mass, Self::Volume, Self::Area];

    /// Its token's name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Mass => "Mass",
            Self::Volume => "Volume",
            Self::Area => "Area",
        }
    }
}

/// One piece of a text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Piece {
    /// Plain text.
    Text(String),
    /// A live value.
    Field(Field),
    /// A prompted entry: its title; the value belongs to where the text is
    /// placed.
    Prompt(String),
}

/// The name of a Property key in tokens: the standard label, else the key.
fn prop_label(key: &str) -> &str {
    STANDARD
        .iter()
        .find(|(k, _, _)| *k == key)
        .map_or(key, |(_, _, l)| l)
}

/// The key of a Property named `label` in a token.
fn prop_key(label: &str) -> String {
    STANDARD
        .iter()
        .find(|(_, _, l)| l.eq_ignore_ascii_case(label))
        .map_or_else(|| label.to_string(), |(k, _, _)| (*k).to_string())
}

impl Field {
    /// The field's token without its brackets (`Model: Part Number`).
    pub fn name(&self) -> String {
        match self {
            Field::Drawing(k) => prop_label(k).to_string(),
            Field::Model(k) => format!("Model: {}", prop_label(k)),
            Field::Material => "Model: Material".to_string(),
            Field::Sheet(s) => s.label().to_string(),
            Field::Parameter(n) => format!("Parameter: {n}"),
            Field::Physical(p) => format!("Physical: {}", p.label()),
        }
    }

    /// The field named `name` (a token's inside).
    fn from_name(name: &str) -> Option<Field> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        if let Some(s) = SheetField::ALL
            .into_iter()
            .find(|s| s.label().eq_ignore_ascii_case(name))
        {
            return Some(Field::Sheet(s));
        }
        if let Some(n) = name.strip_prefix("Parameter:") {
            let n = n.trim();
            return (!n.is_empty()).then(|| Field::Parameter(n.to_string()));
        }
        if let Some(n) = name.strip_prefix("Physical:") {
            let n = n.trim();
            return PhysicalField::ALL
                .into_iter()
                .find(|p| p.label().eq_ignore_ascii_case(n))
                .map(Field::Physical);
        }
        if let Some(m) = name
            .strip_prefix("Model:")
            .or_else(|| name.strip_prefix("model:"))
        {
            let m = m.trim();
            if m.is_empty() {
                return None;
            }
            if m.eq_ignore_ascii_case("Material") {
                return Some(Field::Material);
            }
            return Some(Field::Model(prop_key(m)));
        }
        if let Some(d) = name.strip_prefix("Drawing:") {
            let d = d.trim();
            return (!d.is_empty()).then(|| Field::Drawing(prop_key(d)));
        }
        STANDARD
            .iter()
            .any(|(_, _, l)| l.eq_ignore_ascii_case(name))
            .then(|| Field::Drawing(prop_key(name)))
    }

    /// The fields the Insert Field list offers: the drawing's standard
    /// Properties, the model's, its material and the sheet's values.
    pub fn offered() -> Vec<Field> {
        let mut v: Vec<Field> = STANDARD
            .iter()
            .map(|(k, _, _)| Field::Drawing((*k).to_string()))
            .collect();
        v.extend(
            STANDARD
                .iter()
                .map(|(k, _, _)| Field::Model((*k).to_string())),
        );
        v.push(Field::Material);
        v.extend(SheetField::ALL.into_iter().map(Field::Sheet));
        v
    }
}

/// The text box form of `pieces`: fields as `<…>` tokens, prompts `<?…>`.
pub fn tokens(pieces: &[Piece]) -> String {
    let mut s = String::new();
    for p in pieces {
        match p {
            Piece::Text(t) => s.push_str(t),
            Piece::Field(f) => {
                s.push('<');
                s.push_str(&f.name());
                s.push('>');
            }
            Piece::Prompt(t) => {
                s.push_str("<?");
                s.push_str(t);
                s.push('>');
            }
        }
    }
    s
}

/// The pieces of a text box's text: `<…>` naming a field, and `<?…>`, become
/// fields and prompts; anything else stays text (a `<` of its own too).
pub fn parse(text: &str) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        let (before, from) = rest.split_at(open);
        plain.push_str(before);
        let Some(close) = from.find('>') else {
            plain.push_str(from);
            rest = "";
            break;
        };
        let inner = &from[1..close];
        let piece = if let Some(t) = inner.strip_prefix('?') {
            let t = t.trim();
            (!t.is_empty()).then(|| Piece::Prompt(t.to_string()))
        } else {
            Field::from_name(inner).map(Piece::Field)
        };
        match piece {
            Some(p) => {
                if !plain.is_empty() {
                    out.push(Piece::Text(std::mem::take(&mut plain)));
                }
                out.push(p);
            }
            None => plain.push_str(&from[..=close]),
        }
        rest = &from[close + 1..];
    }
    plain.push_str(rest);
    if !plain.is_empty() {
        out.push(Piece::Text(plain));
    }
    out
}

/// The prompted entries of `pieces`, in order, each once.
pub fn prompts(pieces: &[Piece]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for p in pieces {
        if let Piece::Prompt(t) = p
            && !v.contains(t)
        {
            v.push(t.clone());
        }
    }
    v
}

/// Property `key` of `p`: a standard one (empty when not filled in), else a
/// custom property by name (`None` when there is no such property).
pub fn property(p: &IProperties, key: &str) -> Option<String> {
    if STANDARD.iter().any(|(k, _, _)| *k == key) {
        return Some(p.get(key).to_string());
    }
    p.custom
        .iter()
        .find(|c| c.name == key)
        .map(|c| c.value.text())
}

/// A value of sheet `i` of `doc`.
pub fn sheet_value(doc: &DrawingDocument, i: usize, f: SheetField) -> Option<String> {
    let s = doc.sheets.get(i)?;
    Some(match f {
        SheetField::Name => s.name.clone(),
        SheetField::Number => doc.sheet_number(i).0.to_string(),
        SheetField::Count => doc.sheet_number(i).1.to_string(),
        SheetField::Size => s.size.label(),
        SheetField::Revision => s.revision.clone(),
        SheetField::Scale => {
            let v = s.views.first()?;
            scale_text(doc.effective(s, v.id)?.0)
        }
    })
}

/// The text of `pieces` on sheet `i`: `props` gives the Properties (drawing,
/// model, material), the sheet's values come from `doc`, `prompt` the
/// prompted entries' values (empty until filled in). A field with no value
/// shows `#?`.
pub fn expand(
    pieces: &[Piece],
    doc: &DrawingDocument,
    i: usize,
    props: &dyn Fn(&Field) -> Option<String>,
    prompt: &dyn Fn(&str) -> Option<String>,
) -> String {
    let mut s = String::new();
    for p in pieces {
        match p {
            Piece::Text(t) => s.push_str(t),
            Piece::Field(Field::Sheet(f)) => {
                s.push_str(sheet_value(doc, i, *f).as_deref().unwrap_or("#?"))
            }
            Piece::Field(f) => s.push_str(props(f).as_deref().unwrap_or("#?")),
            Piece::Prompt(t) => s.push_str(&prompt(t).unwrap_or_default()),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_and_pieces_round_trip() {
        let text = "Drawn: <Designer>, part <Model: Part Number> (<Model: Material>) \
                    sheet <Sheet Number>/<Number of Sheets> order <?Order No> a<b <x>";
        let p = parse(text);
        assert_eq!(p[1], Piece::Field(Field::Drawing("designer".into())));
        assert_eq!(p[3], Piece::Field(Field::Model("part_number".into())));
        assert_eq!(p[5], Piece::Field(Field::Material));
        assert_eq!(p[7], Piece::Field(Field::Sheet(SheetField::Number)));
        assert_eq!(p[9], Piece::Field(Field::Sheet(SheetField::Count)));
        assert_eq!(p[11], Piece::Prompt("Order No".into()));
        // Unknown tokens and a lone `<` stay text.
        assert_eq!(p[12], Piece::Text(" a<b <x>".into()));
        assert_eq!(tokens(&p), text);
        assert_eq!(prompts(&p), vec!["Order No".to_string()]);
        // Custom properties by prefix; serde round trip.
        let c = parse("<Drawing: Order> <Model: Colour>");
        assert_eq!(c[0], Piece::Field(Field::Drawing("Order".into())));
        assert_eq!(c[2], Piece::Field(Field::Model("Colour".into())));
        let json = serde_json::to_string(&p).unwrap_or_default();
        let back: Vec<Piece> = serde_json::from_str(&json).unwrap_or_default();
        assert_eq!(back, p);
    }

    #[test]
    fn expand_fills_fields_sheets_and_prompts() {
        let mut d = DrawingDocument::default();
        d.new_sheet();
        let p = parse(
            "<Designer> <Model: Title> <Sheet Number>/<Number of Sheets> <Sheet Size> [<?Order>]",
        );
        let props = |f: &Field| match f {
            Field::Drawing(k) if k == "designer" => Some("M. R.".to_string()),
            _ => None,
        };
        let s = expand(&p, &d, 1, &props, &|t| {
            (t == "Order").then(|| "42".to_string())
        });
        assert_eq!(s, "M. R. #? 2/2 A3 [42]");
        let empty = expand(&p, &d, 0, &props, &|_| None);
        assert_eq!(empty, "M. R. #? 1/2 A3 []");
        // No view: no scale.
        let sc = expand(&parse("<Scale>"), &d, 0, &props, &|_| None);
        assert_eq!(sc, "#?");
    }

    #[test]
    fn sketch_texts_are_drawn_with_their_fields() {
        use crate::drawing::sheet::{Anchor, Prim};
        use crate::drawing::view_sketch::{DrawingText, TextAnchor, ViewSketch};
        let mut d = DrawingDocument::default();
        let id = d
            .add_sketch(None, ViewSketch::default())
            .unwrap_or_default();
        let text = |s: &str, anchor| DrawingText {
            at: [100.0, 50.0],
            height: 3.5,
            anchor,
            angle: 0.0,
            pieces: parse(s),
        };
        assert!(d.set_sketch_texts(
            id,
            vec![
                text(
                    "Sheet <Sheet Number> of <Number of Sheets>",
                    TextAnchor::Left
                ),
                text("<Designer>", TextAnchor::Center),
                text("R", TextAnchor::Right),
                text("<?Order>", TextAnchor::Left),
            ],
        ));
        let props = |f: &Field| (*f == Field::Drawing("designer".into())).then(|| "MR".to_string());
        let view = |_: u32| None;
        let prims = crate::drawing::render::sketch_prims_with(&d, 0, &view, None, &props);
        let texts: Vec<(String, [f64; 2], Anchor)> = prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text {
                    text, at, anchor, ..
                } => Some((text.clone(), *at, *anchor)),
                _ => None,
            })
            .collect();
        // The empty prompt draws nothing.
        assert_eq!(texts.len(), 3, "{texts:?}");
        assert_eq!(
            texts[0],
            ("Sheet 1 of 1".to_string(), [100.0, 50.0], Anchor::Left)
        );
        assert_eq!(texts[1], ("MR".to_string(), [100.0, 50.0], Anchor::Center));
        // Right: its end on the point.
        assert!(
            texts[2].1[0] < 100.0 && texts[2].1[0] > 95.0,
            "{:?}",
            texts[2]
        );
        // Saved and read back.
        let json = serde_json::to_string(&d).unwrap_or_default();
        let back: DrawingDocument = serde_json::from_str(&json).unwrap_or_default();
        assert_eq!(back.sheets[0].sketches[0].texts.len(), 4);
    }
}
