//! Drawing styles (`docs/spec/4.6-drawing-styles.md`): layers, dimension,
//! text, leader, centre mark, hatch and view annotation styles and the
//! standard's Object Defaults, with the defaults of each standard family.
//! The sheet's display list reads them (`render`, `annot`), so the screen
//! and paper follow every change.

use serde::{Deserialize, Serialize};

use super::StandardKind;

/// Every drawing style of a document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DrawingStyles {
    /// Layers (line type, weight, colour of object types).
    pub layers: Vec<LayerStyle>,
    /// Dimension styles.
    pub dimension: Vec<DimensionStyle>,
    /// Text styles.
    pub text: Vec<TextStyle>,
    /// Leader styles (hole notes, leader texts).
    pub leader: Vec<LeaderStyle>,
    /// Centre mark styles (centre marks, centrelines).
    pub center_mark: Vec<CenterMarkStyle>,
    /// Hatch styles (section views).
    pub hatch: Vec<HatchStyle>,
    /// View annotation styles (view labels, section / detail definitions).
    pub view_annotation: Vec<ViewAnnotationStyle>,
    /// Which style and layer each object type takes.
    pub defaults: ObjectDefaults,
}

impl Default for DrawingStyles {
    fn default() -> Self {
        Self::of(StandardKind::Iso)
    }
}

/// ISO 128 line types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineType {
    /// Solid.
    Continuous,
    /// Short dashes (hidden lines).
    Dashed,
    /// Long dash, dot (centre lines).
    Chain,
    /// Long dash, two dots.
    Phantom,
}

impl LineType {
    /// Every line type, for the editor.
    pub const ALL: [LineType; 4] = [
        LineType::Continuous,
        LineType::Dashed,
        LineType::Chain,
        LineType::Phantom,
    ];

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            LineType::Continuous => "Continuous",
            LineType::Dashed => "Dashed",
            LineType::Chain => "Chain",
            LineType::Phantom => "Phantom",
        }
    }

    /// Dash pattern on paper (mm: dash, gap, …); empty = solid.
    pub fn pattern(self) -> &'static [f64] {
        match self {
            LineType::Continuous => &[],
            LineType::Dashed => &[3.0, 1.5],
            LineType::Chain => &[12.0, 1.5, 1.5, 1.5],
            LineType::Phantom => &[12.0, 1.5, 1.5, 1.5, 1.5, 1.5],
        }
    }
}

/// One layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerStyle {
    /// Name.
    pub name: String,
    /// Shown.
    pub on: bool,
    /// Colour (sRGB).
    pub color: [u8; 3],
    /// Line type.
    pub line_type: LineType,
    /// Line weight on paper (mm).
    pub weight: f64,
    /// Printed / published.
    pub plot: bool,
}

/// Arrowheads and other dimension / leader terminators.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Terminator {
    /// Filled closed arrow (ISO default).
    Filled,
    /// Open arrow (two strokes).
    Open,
    /// Filled dot.
    Dot,
    /// Oblique stroke (architectural tick).
    Oblique,
    /// Nothing.
    None,
}

impl Terminator {
    /// Every terminator, for the editor.
    pub const ALL: [Terminator; 5] = [
        Terminator::Filled,
        Terminator::Open,
        Terminator::Dot,
        Terminator::Oblique,
        Terminator::None,
    ];

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            Terminator::Filled => "Filled",
            Terminator::Open => "Open",
            Terminator::Dot => "Dot",
            Terminator::Oblique => "Oblique",
            Terminator::None => "None",
        }
    }
}

/// How linear dimension text sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextOrientation {
    /// Along the dimension line (ISO).
    Aligned,
    /// Always horizontal (ANSI).
    Horizontal,
}

/// A dimension style.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DimensionStyle {
    /// Name.
    pub name: String,
    /// Linear precision (decimals).
    pub precision: u8,
    /// Decimal comma instead of a point.
    pub decimal_comma: bool,
    /// Keep trailing zeros up to the precision.
    pub trailing_zeros: bool,
    /// Angular precision (decimals).
    pub angular_precision: u8,
    /// Arrowhead.
    pub terminator: Terminator,
    /// Arrow length, Size X (mm).
    pub arrow_size: f64,
    /// Arrow width, Height Y (mm).
    pub arrow_height: f64,
    /// Extension lines pass the dimension line by (A, mm).
    pub extension: f64,
    /// Extension lines start this far from the object (B, mm).
    pub origin_offset: f64,
    /// Gap between text and dimension line (C, mm).
    pub gap: f64,
    /// Spacing of parallel dimensions (D, mm).
    pub spacing: f64,
    /// First dimension's distance from the part (E, mm).
    pub part_offset: f64,
    /// Text style of the value.
    pub text_style: String,
    /// Linear text orientation.
    pub orientation: TextOrientation,
    /// Text before every value.
    pub prefix: String,
    /// Text after every value.
    pub suffix: String,
    /// Ø before diameters.
    pub diameter_symbol: bool,
    /// Leader style of notes.
    pub leader_style: String,
}

impl Default for DimensionStyle {
    fn default() -> Self {
        Self {
            name: "Default (ISO)".into(),
            precision: 2,
            decimal_comma: true,
            trailing_zeros: true,
            angular_precision: 0,
            terminator: Terminator::Filled,
            arrow_size: 2.5,
            arrow_height: 1.0,
            extension: 3.175,
            origin_offset: 0.0,
            gap: 0.762,
            spacing: 6.0,
            part_offset: 10.0,
            text_style: "Note Text (ISO)".into(),
            orientation: TextOrientation::Aligned,
            prefix: String::new(),
            suffix: String::new(),
            diameter_symbol: true,
            leader_style: "Default (ISO)".into(),
        }
    }
}

/// A text style.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextStyle {
    /// Name.
    pub name: String,
    /// Font name (one font for now).
    pub font: String,
    /// Cap height on paper (mm).
    pub height: f64,
    /// Width scale (stretch).
    pub width_scale: f64,
    /// Line spacing (× height).
    pub line_spacing: f64,
    /// Colour (sRGB).
    pub color: [u8; 3],
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            name: "Note Text (ISO)".into(),
            font: "Ubuntu Light".into(),
            height: 3.5,
            width_scale: 1.0,
            line_spacing: 1.6,
            color: [0, 0, 0],
            bold: false,
            italic: false,
        }
    }
}

/// A leader style.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LeaderStyle {
    /// Name.
    pub name: String,
    /// Arrowhead.
    pub terminator: Terminator,
    /// Arrow length (mm).
    pub arrow_size: f64,
    /// Arrow width (mm).
    pub arrow_height: f64,
    /// Text style of leader texts and notes.
    pub text_style: String,
}

impl Default for LeaderStyle {
    fn default() -> Self {
        Self {
            name: "Default (ISO)".into(),
            terminator: Terminator::Filled,
            arrow_size: 2.5,
            arrow_height: 1.0,
            text_style: "Note Text (ISO)".into(),
        }
    }
}

/// A centre mark style (centre marks and centrelines).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CenterMarkStyle {
    /// Name.
    pub name: String,
    /// Size of the centre cross (mm).
    pub mark: f64,
    /// Gap between the cross and the extension lines (mm).
    pub gap: f64,
    /// Extension lines pass the feature by (mm).
    pub overshoot: f64,
}

impl Default for CenterMarkStyle {
    fn default() -> Self {
        Self {
            name: "Center Mark (ISO)".into(),
            mark: 5.0,
            gap: 1.5,
            overshoot: 2.5,
        }
    }
}

/// A hatch style.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HatchStyle {
    /// Name.
    pub name: String,
    /// Line angle (degrees).
    pub angle: f64,
    /// Distance between lines on paper (mm).
    pub spacing: f64,
    /// Cross-hatched (a second set at +90°).
    pub double: bool,
}

impl Default for HatchStyle {
    fn default() -> Self {
        Self {
            name: "ANSI 31 (ISO)".into(),
            angle: 45.0,
            spacing: 3.0,
            double: false,
        }
    }
}

/// A view annotation style.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewAnnotationStyle {
    /// Name.
    pub name: String,
    /// View label text height (mm).
    pub label_height: f64,
    /// Letters at section lines and detail fences (mm).
    pub identifier_height: f64,
}

impl Default for ViewAnnotationStyle {
    fn default() -> Self {
        Self {
            name: "View Annotation (ISO)".into(),
            label_height: 5.25,
            identifier_height: 5.0,
        }
    }
}

/// Which style and layer each object type takes (one per standard).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ObjectDefaults {
    /// Dimensions.
    pub dimension: String,
    /// Centre marks and centrelines.
    pub center_mark: String,
    /// Section hatching.
    pub hatch: String,
    /// Notes (Text).
    pub text: String,
    /// Leader texts and hole notes.
    pub leader: String,
    /// View labels and definitions.
    pub view_annotation: String,
    /// Layer of visible edges.
    pub visible_layer: String,
    /// Layer of hidden edges.
    pub hidden_layer: String,
    /// Layer of centre marks and centrelines.
    pub center_layer: String,
    /// Layer of section lines (on the parent view).
    #[serde(default = "section_layer")]
    pub section_layer: String,
    /// Layer of dimensions and notes.
    pub dimension_layer: String,
    /// Layer of hatching.
    pub hatch_layer: String,
    /// Layer of the border and title block.
    pub border_layer: String,
    /// Layer of the bend centre lines of flat pattern views.
    pub bend_center_layer: String,
    /// Layer of the bend extents of flat pattern views.
    pub bend_extent_layer: String,
}

impl Default for ObjectDefaults {
    fn default() -> Self {
        Self {
            dimension: "Default (ISO)".into(),
            center_mark: "Center Mark (ISO)".into(),
            hatch: "ANSI 31 (ISO)".into(),
            text: "Note Text (ISO)".into(),
            leader: "Default (ISO)".into(),
            view_annotation: "View Annotation (ISO)".into(),
            visible_layer: "Visible (ISO)".into(),
            hidden_layer: "Hidden (ISO)".into(),
            center_layer: "Center Mark (ISO)".into(),
            section_layer: "Section Line (ISO)".into(),
            dimension_layer: "Dimension (ISO)".into(),
            hatch_layer: "Hatch (ISO)".into(),
            border_layer: "Border (ISO)".into(),
            bend_center_layer: "Bend Centerline (ISO)".into(),
            bend_extent_layer: "Bend Extent (ISO)".into(),
        }
    }
}

/// The style types of the editor, in Inventor's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StyleType {
    /// Centre marks.
    CenterMark,
    /// Dimensions.
    Dimension,
    /// Hatching.
    Hatch,
    /// Layers.
    Layers,
    /// Leaders.
    Leader,
    /// Object Defaults.
    ObjectDefaults,
    /// Text.
    Text,
    /// View annotations.
    ViewAnnotation,
}

impl StyleType {
    /// The types the editor edits today.
    pub const ALL: [StyleType; 8] = [
        StyleType::CenterMark,
        StyleType::Dimension,
        StyleType::Hatch,
        StyleType::Layers,
        StyleType::Leader,
        StyleType::ObjectDefaults,
        StyleType::Text,
        StyleType::ViewAnnotation,
    ];

    /// Display name.
    pub fn label(self) -> &'static str {
        match self {
            StyleType::CenterMark => "Center Mark",
            StyleType::Dimension => "Dimension",
            StyleType::Hatch => "Hatch",
            StyleType::Layers => "Layers",
            StyleType::Leader => "Leader",
            StyleType::ObjectDefaults => "Object Defaults",
            StyleType::Text => "Text",
            StyleType::ViewAnnotation => "View Annotation",
        }
    }
}

/// Style types Inventor has that wait for their objects (tables, symbols).
pub const PLANNED_TYPES: [(&str, &str); 10] = [
    ("Balloon", "with balloons and parts lists (4.4)"),
    ("Datum Target", "with GD&T symbols (4.3)"),
    ("Feature Control Frame", "with GD&T symbols (4.3)"),
    ("Hole Table", "with hole tables (4.4)"),
    ("ID", "with ID symbols (4.3)"),
    ("Parts List", "with parts lists (4.4)"),
    ("Revision Table", "with revision tables (4.4)"),
    ("Surface Texture", "with surface texture symbols (4.3)"),
    ("Table", "with general tables (4.4)"),
    ("Weld Symbol", "with weld symbols (4.3)"),
];

fn layer(name: &str, color: [u8; 3], line_type: LineType, weight: f64) -> LayerStyle {
    LayerStyle {
        name: name.into(),
        on: true,
        color,
        line_type,
        weight,
        plot: true,
    }
}

impl DrawingStyles {
    /// The styles of a standard family (ISO 128 / 129 for ISO and DIN, ANSI
    /// Y14.5-like for ANSI: horizontal text, 0.13 in arrows).
    pub fn of(kind: StandardKind) -> Self {
        let tag = match kind {
            StandardKind::Iso => "ISO",
            StandardKind::Ansi => "ANSI",
            StandardKind::Din => "DIN",
        };
        let n = |base: &str| format!("{base} ({tag})");
        let black = [0, 0, 0];
        let mut s = Self {
            layers: vec![
                layer(&n("Visible"), black, LineType::Continuous, 0.5),
                layer(&n("Hidden"), black, LineType::Dashed, 0.35),
                layer(&n("Center Mark"), black, LineType::Continuous, 0.25),
                layer(&n("Dimension"), black, LineType::Continuous, 0.25),
                layer(&n("Hatch"), black, LineType::Continuous, 0.18),
                layer(&n("Section Line"), black, LineType::Chain, 0.7),
                layer(&n("Border"), black, LineType::Continuous, 0.7),
                layer(&n("Title"), black, LineType::Continuous, 0.5),
                layer(&n("Bend Centerline"), black, LineType::Chain, 0.25),
                layer(&n("Bend Extent"), black, LineType::Continuous, 0.5),
            ],
            dimension: vec![DimensionStyle {
                name: n("Default"),
                text_style: n("Note Text"),
                leader_style: n("Default"),
                ..DimensionStyle::default()
            }],
            text: vec![TextStyle {
                name: n("Note Text"),
                ..TextStyle::default()
            }],
            leader: vec![LeaderStyle {
                name: n("Default"),
                text_style: n("Note Text"),
                ..LeaderStyle::default()
            }],
            center_mark: vec![CenterMarkStyle {
                name: n("Center Mark"),
                ..CenterMarkStyle::default()
            }],
            hatch: vec![HatchStyle {
                name: n("ANSI 31"),
                ..HatchStyle::default()
            }],
            view_annotation: vec![ViewAnnotationStyle {
                name: n("View Annotation"),
                ..ViewAnnotationStyle::default()
            }],
            defaults: ObjectDefaults {
                dimension: n("Default"),
                center_mark: n("Center Mark"),
                hatch: n("ANSI 31"),
                text: n("Note Text"),
                leader: n("Default"),
                view_annotation: n("View Annotation"),
                visible_layer: n("Visible"),
                hidden_layer: n("Hidden"),
                center_layer: n("Center Mark"),
                section_layer: n("Section Line"),
                dimension_layer: n("Dimension"),
                hatch_layer: n("Hatch"),
                border_layer: n("Border"),
                bend_center_layer: n("Bend Centerline"),
                bend_extent_layer: n("Bend Extent"),
            },
        };
        if kind == StandardKind::Ansi {
            // Inventor 2027's "ANSI (mm)" template.
            let d = &mut s.dimension[0];
            d.orientation = TextOrientation::Horizontal;
            d.decimal_comma = false;
            d.origin_offset = 1.5875;
            d.gap = 0.8;
            s.text[0].height = 3.048;
            let c = &mut s.center_mark[0];
            c.mark = 6.35;
            c.gap = 2.9972;
            c.overshoot = 3.175;
            if let Some(l) = s
                .layers
                .iter_mut()
                .find(|l| l.name == "Bend Centerline (ANSI)")
            {
                l.weight = 0.254;
            }
        }
        s
    }

    /// The layers of bend centre lines and bend extents (chain 0.25 mm and
    /// continuous 0.5 mm in drawings made before they existed).
    pub fn bend_layers(&self) -> (LayerStyle, LayerStyle) {
        let d = &self.defaults;
        (
            self.layer_or(
                &d.bend_center_layer,
                layer(&d.bend_center_layer, [0, 0, 0], LineType::Chain, 0.25),
            ),
            self.layer_or(
                &d.bend_extent_layer,
                layer(&d.bend_extent_layer, [0, 0, 0], LineType::Continuous, 0.5),
            ),
        )
    }

    /// The layer named `name`, else the first.
    pub fn layer(&self, name: &str) -> LayerStyle {
        self.layers
            .iter()
            .find(|l| l.name == name)
            .or(self.layers.first())
            .cloned()
            .unwrap_or_else(|| layer(name, [0, 0, 0], LineType::Continuous, 0.25))
    }

    /// The layer named `name`, or `fallback` when the drawing has none of
    /// that name (files from before the layer existed).
    pub fn layer_or(&self, name: &str, fallback: LayerStyle) -> LayerStyle {
        self.layers
            .iter()
            .find(|l| l.name == name)
            .cloned()
            .unwrap_or(fallback)
    }

    /// The dimension style named `name`, else the first.
    pub fn dimension(&self, name: &str) -> DimensionStyle {
        find(&self.dimension, name, |s| &s.name)
    }

    /// The text style named `name`, else the first.
    pub fn text(&self, name: &str) -> TextStyle {
        find(&self.text, name, |s| &s.name)
    }

    /// The leader style named `name`, else the first.
    pub fn leader(&self, name: &str) -> LeaderStyle {
        find(&self.leader, name, |s| &s.name)
    }

    /// The centre mark style named `name`, else the first.
    pub fn center_mark(&self, name: &str) -> CenterMarkStyle {
        find(&self.center_mark, name, |s| &s.name)
    }

    /// The hatch style named `name`, else the first.
    pub fn hatch(&self, name: &str) -> HatchStyle {
        find(&self.hatch, name, |s| &s.name)
    }

    /// The view annotation style named `name`, else the first.
    pub fn view_annotation(&self, name: &str) -> ViewAnnotationStyle {
        find(&self.view_annotation, name, |s| &s.name)
    }

    /// The names of the styles of `t` (Object Defaults: its one entry).
    pub fn names(&self, t: StyleType) -> Vec<String> {
        match t {
            StyleType::CenterMark => self.center_mark.iter().map(|s| s.name.clone()).collect(),
            StyleType::Dimension => self.dimension.iter().map(|s| s.name.clone()).collect(),
            StyleType::Hatch => self.hatch.iter().map(|s| s.name.clone()).collect(),
            StyleType::Layers => self.layers.iter().map(|s| s.name.clone()).collect(),
            StyleType::Leader => self.leader.iter().map(|s| s.name.clone()).collect(),
            StyleType::ObjectDefaults => vec!["Object Defaults".into()],
            StyleType::Text => self.text.iter().map(|s| s.name.clone()).collect(),
            StyleType::ViewAnnotation => self
                .view_annotation
                .iter()
                .map(|s| s.name.clone())
                .collect(),
        }
    }

    /// Whether the Object Defaults or another style use style `name` of `t`
    /// (such styles cannot be deleted).
    pub fn in_use(&self, t: StyleType, name: &str) -> bool {
        let d = &self.defaults;
        match t {
            StyleType::CenterMark => d.center_mark == name,
            StyleType::Dimension => d.dimension == name,
            StyleType::Hatch => d.hatch == name,
            StyleType::Leader => {
                d.leader == name || self.dimension.iter().any(|s| s.leader_style == name)
            }
            StyleType::Text => {
                d.text == name
                    || self.dimension.iter().any(|s| s.text_style == name)
                    || self.leader.iter().any(|s| s.text_style == name)
            }
            StyleType::ViewAnnotation => d.view_annotation == name,
            StyleType::Layers => [
                &d.visible_layer,
                &d.hidden_layer,
                &d.center_layer,
                &d.section_layer,
                &d.dimension_layer,
                &d.hatch_layer,
                &d.border_layer,
            ]
            .iter()
            .any(|l| *l == name),
            StyleType::ObjectDefaults => true,
        }
    }

    /// New Style: a copy of style `name` of `t` under a free name; returns
    /// the new name.
    pub fn duplicate(&mut self, t: StyleType, name: &str) -> Option<String> {
        let taken = self.names(t);
        let new = (1..)
            .map(|k| {
                format!(
                    "Copy of {name}{}",
                    if k == 1 {
                        String::new()
                    } else {
                        format!(" {k}")
                    }
                )
            })
            .find(|n| !taken.contains(n))?;
        macro_rules! dup {
            ($v:expr) => {{
                let mut s = $v.iter().find(|s| s.name == name)?.clone();
                s.name = new.clone();
                $v.push(s);
            }};
        }
        match t {
            StyleType::CenterMark => dup!(self.center_mark),
            StyleType::Dimension => dup!(self.dimension),
            StyleType::Hatch => dup!(self.hatch),
            StyleType::Layers => dup!(self.layers),
            StyleType::Leader => dup!(self.leader),
            StyleType::Text => dup!(self.text),
            StyleType::ViewAnnotation => dup!(self.view_annotation),
            StyleType::ObjectDefaults => return None,
        }
        Some(new)
    }

    /// Deletes style `name` of `t` unless it is in use or the last one.
    pub fn delete(&mut self, t: StyleType, name: &str) -> Result<(), String> {
        if self.in_use(t, name) {
            return Err(format!("{name} is in use"));
        }
        if self.names(t).len() < 2 {
            return Err("the last style of a type stays".into());
        }
        match t {
            StyleType::CenterMark => self.center_mark.retain(|s| s.name != name),
            StyleType::Dimension => self.dimension.retain(|s| s.name != name),
            StyleType::Hatch => self.hatch.retain(|s| s.name != name),
            StyleType::Layers => self.layers.retain(|s| s.name != name),
            StyleType::Leader => self.leader.retain(|s| s.name != name),
            StyleType::Text => self.text.retain(|s| s.name != name),
            StyleType::ViewAnnotation => self.view_annotation.retain(|s| s.name != name),
            StyleType::ObjectDefaults => return Err("Object Defaults cannot be deleted".into()),
        }
        Ok(())
    }

    /// Renames style `old` of `t` and every reference to it.
    pub fn rename(&mut self, t: StyleType, old: &str, new: &str) -> Result<(), String> {
        let new = new.trim();
        if new.is_empty() {
            return Err("a style needs a name".into());
        }
        if self.names(t).iter().any(|n| n == new) {
            return Err(format!("{new} exists already"));
        }
        let fix = |s: &mut String| {
            if s == old {
                *s = new.to_string();
            }
        };
        let d = &mut self.defaults;
        match t {
            StyleType::CenterMark => {
                self.center_mark.iter_mut().for_each(|s| fix(&mut s.name));
                fix(&mut d.center_mark);
            }
            StyleType::Dimension => {
                self.dimension.iter_mut().for_each(|s| fix(&mut s.name));
                fix(&mut d.dimension);
            }
            StyleType::Hatch => {
                self.hatch.iter_mut().for_each(|s| fix(&mut s.name));
                fix(&mut d.hatch);
            }
            StyleType::Layers => {
                self.layers.iter_mut().for_each(|s| fix(&mut s.name));
                for l in [
                    &mut d.visible_layer,
                    &mut d.hidden_layer,
                    &mut d.center_layer,
                    &mut d.section_layer,
                    &mut d.dimension_layer,
                    &mut d.hatch_layer,
                    &mut d.border_layer,
                ] {
                    fix(l);
                }
            }
            StyleType::Leader => {
                self.leader.iter_mut().for_each(|s| fix(&mut s.name));
                fix(&mut d.leader);
                self.dimension
                    .iter_mut()
                    .for_each(|s| fix(&mut s.leader_style));
            }
            StyleType::Text => {
                self.text.iter_mut().for_each(|s| fix(&mut s.name));
                fix(&mut d.text);
                self.dimension
                    .iter_mut()
                    .for_each(|s| fix(&mut s.text_style));
                self.leader.iter_mut().for_each(|s| fix(&mut s.text_style));
            }
            StyleType::ViewAnnotation => {
                self.view_annotation
                    .iter_mut()
                    .for_each(|s| fix(&mut s.name));
                fix(&mut d.view_annotation);
            }
            StyleType::ObjectDefaults => return Err("Object Defaults has no name to change".into()),
        }
        Ok(())
    }
}

fn section_layer() -> String {
    "Section Line (ISO)".into()
}

fn find<T: Clone + Default>(v: &[T], name: &str, key: impl Fn(&T) -> &String) -> T {
    v.iter()
        .find(|s| key(s) == name)
        .or(v.first())
        .cloned()
        .unwrap_or_default()
}

/// What the annotations of a sheet take from the styles (the Object
/// Defaults' choices resolved).
#[derive(Clone, Debug, PartialEq)]
pub struct AnnotationLook {
    /// Dimension style.
    pub dim: DimensionStyle,
    /// Its text style.
    pub dim_text: TextStyle,
    /// Leader style.
    pub leader: LeaderStyle,
    /// Its text style.
    pub leader_text: TextStyle,
    /// Centre mark style.
    pub center: CenterMarkStyle,
    /// Line weight of dimensions and notes (mm).
    pub dim_weight: f64,
    /// Line weight of centre lines (mm).
    pub center_weight: f64,
    /// Centre marks and centrelines dash (their layer is not continuous).
    pub center_chain: bool,
}

impl Default for AnnotationLook {
    fn default() -> Self {
        DrawingStyles::default().look()
    }
}

impl DrawingStyles {
    /// The resolved look of annotations.
    pub fn look(&self) -> AnnotationLook {
        let d = &self.defaults;
        let dim = self.dimension(&d.dimension);
        let leader = self.leader(&d.leader);
        AnnotationLook {
            dim_text: self.text(&dim.text_style),
            leader_text: self.text(&leader.text_style),
            dim,
            leader,
            center: self.center_mark(&d.center_mark),
            dim_weight: self.layer(&d.dimension_layer).weight,
            center_weight: self.layer(&d.center_layer).weight,
            center_chain: self.layer(&d.center_layer).line_type != LineType::Continuous,
        }
    }
}
