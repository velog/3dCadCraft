//! Drawing resources (spec 4.13): the user's borders, title blocks and
//! sketched symbols. The parameters of a zone border are kept per sheet
//! (`Sheet::border_params`; `None` = the standard's default border); the
//! user's title blocks are definitions in [`Resources`], placed on a sheet
//! by name (`Sheet::title_def`; `None` = the standard's title block).
//! The user's borders are definitions in sheet mm as drawn (`BorderDef`),
//! placed by name (`Sheet::border_def`); a title block sits on a border's
//! connection point.

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
    /// Borders, in browser order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub borders: Vec<BorderDef>,
    /// Sketched symbols, in browser order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<Definition>,
}

/// A border definition (spec 4.13 §3.3): a sketch in sheet mm as it was
/// drawn; a zone border also draws its zones (`zones`), a custom border
/// only the sketch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BorderDef {
    /// Its sketch, texts and connection points.
    pub def: Definition,
    /// The default parameters of a zone border (`None`: a custom border).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zones: Option<ZoneParams>,
}

impl Resources {
    /// No definitions.
    pub fn is_empty(&self) -> bool {
        self.title_blocks.is_empty() && self.borders.is_empty() && self.symbols.is_empty()
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
    /// Connection points (sheet mm): where a title block's corner attaches
    /// (a border's), where a leader ends (a symbol's).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connections: Vec<[f64; 2]>,
    /// A symbol's Insertion Point (`None`: the middle of its box).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insertion: Option<[f64; 2]>,
}

impl Definition {
    /// A definition of `sketch` and `texts` without connection points.
    pub fn new(name: impl Into<String>, sketch: ViewSketch, texts: Vec<DrawingText>) -> Self {
        Self {
            name: name.into(),
            sketch,
            texts,
            connections: Vec::new(),
            insertion: None,
        }
    }

    /// The point a symbol is placed, scaled and turned about: its Insertion
    /// Point, else the middle of its box.
    pub fn anchor(&self) -> Option<[f64; 2]> {
        self.insertion.or_else(|| {
            self.extent()
                .map(|(lo, hi)| [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0])
        })
    }

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
        for c in &mut self.connections {
            *c = [c[0] + d[0], c[1] + d[1]];
        }
        if let Some(c) = &mut self.insertion {
            *c = [c[0] + d[0], c[1] + d[1]];
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

/// The border definition sheet `sheet` uses, when it is the user's.
pub fn border_of<'a>(doc: &'a DrawingDocument, sheet: &Sheet) -> Option<&'a BorderDef> {
    if !sheet.border {
        return None;
    }
    let p = sheet.border_def.as_ref()?;
    doc.resources.borders.iter().find(|d| d.def.name == p.name)
}

/// Where a title block attaches on sheet `sheet` and which of its corners
/// goes there (`[right, top]`): the border's connection point farthest from
/// the sheet's centre in the lower right quarter (else the farthest of all,
/// the block's corner on that point's side); a custom border without one:
/// its lower right corner; else the frame's lower right corner.
pub fn title_anchor(doc: &DrawingDocument, sheet: &Sheet) -> ([f64; 2], [bool; 2]) {
    let (flo, fhi) = super::sheet::frame(sheet);
    let corner = ([fhi[0], flo[1]], [true, false]);
    let Some(b) = border_of(doc, sheet) else {
        return corner;
    };
    let (w, h) = sheet.dims();
    let c = [w * 0.5, h * 0.5];
    let far = |pts: &mut dyn Iterator<Item = [f64; 2]>| {
        pts.max_by(|a, b| {
            let d = |q: &[f64; 2]| (q[0] - c[0]).hypot(q[1] - c[1]);
            d(a).total_cmp(&d(b))
        })
    };
    let cs = &b.def.connections;
    let best = far(&mut cs.iter().copied().filter(|q| q[0] >= c[0] && q[1] <= c[1]))
        .or_else(|| far(&mut cs.iter().copied()));
    if let Some(q) = best {
        return (q, [q[0] >= c[0], q[1] > c[1]]);
    }
    if b.zones.is_none()
        && let Some((lo, hi)) = b.def.extent()
    {
        return ([hi[0], lo[1]], [true, false]);
    }
    corner
}

/// How far a title block definition is moved onto sheet `sheet` of `doc`:
/// its corner on the anchor (Inventor anchors the block's outermost corner
/// on the border's connection point or corner).
pub fn title_offset(doc: &DrawingDocument, def: &Definition, sheet: &Sheet) -> Option<[f64; 2]> {
    let (lo, hi) = def.extent()?;
    let (at, [right, top]) = title_anchor(doc, sheet);
    let x = if right { hi[0] } else { lo[0] };
    let y = if top { hi[1] } else { lo[1] };
    Some([at[0] - x, at[1] - y])
}

/// A definition's curves and texts as primitives, moved by `off`, at the
/// border layer's weight; fields from `props`, the sheet and `placed`.
fn def_prims(
    doc: &DrawingDocument,
    i: usize,
    def: &Definition,
    off: [f64; 2],
    placed: Option<&PlacedTitle>,
    props: &dyn Fn(&Field) -> Option<String>,
) -> Vec<Prim> {
    let weight = doc.styles.layer(&doc.styles.defaults.border_layer).weight;
    def_prims_at(doc, i, def, off, weight, placed, props)
}

/// [`def_prims`] at line weight `weight`.
fn def_prims_at(
    doc: &DrawingDocument,
    i: usize,
    def: &Definition,
    off: [f64; 2],
    weight: f64,
    placed: Option<&PlacedTitle>,
    props: &dyn Fn(&Field) -> Option<String>,
) -> Vec<Prim> {
    let mv = |q: [f64; 2]| [q[0] + off[0], q[1] + off[1]];
    let mut out = Vec::new();
    for (pts, look) in def.as_sketch().curves() {
        out.push(Prim::Path {
            pts: pts.into_iter().map(mv).collect(),
            width: weight,
            dashed: look == super::view_sketch::CurveLook::Chain,
        });
    }
    for t in &def.texts {
        let prompt = |title: &str| placed.and_then(|p| p.prompt(title));
        let text = fields::expand(&t.pieces, doc, i, props, &prompt);
        out.extend(super::render::text_prim(&text, t, mv(t.at)));
    }
    out
}

/// The user's border of sheet `i` as primitives (empty when the sheet shows
/// the default border): its sketch where it was drawn, fields filled in.
/// A zone border's zones are drawn by `sheet::format_prims`.
pub fn border_def_prims(
    doc: &DrawingDocument,
    i: usize,
    props: &dyn Fn(&Field) -> Option<String>,
) -> Vec<Prim> {
    let Some(sheet) = doc.sheets.get(i) else {
        return Vec::new();
    };
    let Some(b) = border_of(doc, sheet) else {
        return Vec::new();
    };
    def_prims(doc, i, &b.def, [0.0, 0.0], sheet.border_def.as_ref(), props)
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
    let Some(off) = title_offset(doc, def, sheet) else {
        return Vec::new();
    };
    def_prims(doc, i, def, off, sheet.title_def.as_ref(), props)
}

/// A sketched symbol placed on a sheet (spec 4.13 §3.4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacedSymbol {
    /// The definition's name.
    pub name: String,
    /// Where its anchor is (sheet mm).
    pub at: [f64; 2],
    /// Its scale.
    #[serde(default = "one")]
    pub scale: f64,
    /// Its rotation about the anchor (radians, counter-clockwise).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotate: f64,
    /// Static: no scale and rotate grips on the sheet.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fixed: bool,
    /// The leader's points from its tip (sheet mm); empty: no leader.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub leader: Vec<[f64; 2]>,
    /// The leader is kept but not drawn (Visible off).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub leader_hidden: bool,
    /// The prompted entries' values of this instance (entry title, value).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompts: Vec<(String, String)>,
}

fn one() -> f64 {
    1.0
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

/// What a symbol definition looks like on a sheet, about its anchor, before
/// the instance's scale and rotation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SymbolArt {
    /// Curves and texts, fields filled in.
    pub prims: Vec<Prim>,
    /// Connection points.
    pub connections: Vec<[f64; 2]>,
}

/// The art of symbol `s` on sheet `i` (`None`: no such definition, or it is
/// empty).
pub fn symbol_art(
    doc: &DrawingDocument,
    i: usize,
    s: &PlacedSymbol,
    props: &dyn Fn(&Field) -> Option<String>,
) -> Option<SymbolArt> {
    let def = doc.resources.symbols.iter().find(|d| d.name == s.name)?;
    let a = def.anchor()?;
    let placed = PlacedTitle {
        name: s.name.clone(),
        prompts: s.prompts.clone(),
    };
    let weight = doc.styles.layer(&doc.styles.defaults.symbol_layer).weight;
    Some(SymbolArt {
        prims: def_prims_at(doc, i, def, [-a[0], -a[1]], weight, Some(&placed), props),
        connections: def
            .connections
            .iter()
            .map(|c| [c[0] - a[0], c[1] - a[1]])
            .collect(),
    })
}

impl PlacedSymbol {
    /// A point of its art on the sheet.
    pub fn to_sheet(&self, q: [f64; 2]) -> [f64; 2] {
        let (sn, c) = self.rotate.sin_cos();
        let (x, y) = (q[0] * self.scale, q[1] * self.scale);
        [self.at[0] + x * c - y * sn, self.at[1] + x * sn + y * c]
    }

    /// `art` placed on the sheet.
    pub fn place(&self, art: &[Prim]) -> Vec<Prim> {
        art.iter()
            .map(|p| match p {
                Prim::Line { a, b, weight } => Prim::Line {
                    a: self.to_sheet(*a),
                    b: self.to_sheet(*b),
                    weight: *weight,
                },
                Prim::Path { pts, width, dashed } => Prim::Path {
                    pts: pts.iter().map(|q| self.to_sheet(*q)).collect(),
                    width: *width,
                    dashed: *dashed,
                },
                Prim::Fill { pts } => Prim::Fill {
                    pts: pts.iter().map(|q| self.to_sheet(*q)).collect(),
                },
                Prim::Text {
                    at,
                    height,
                    text,
                    anchor,
                    angle,
                } => Prim::Text {
                    at: self.to_sheet(*at),
                    height: height * self.scale,
                    text: text.clone(),
                    anchor: *anchor,
                    angle: angle + self.rotate,
                },
                other => other.clone(),
            })
            .collect()
    }

    /// Where the leader meets the symbol: the connection point nearest the
    /// leader's last point before it, else the anchor.
    pub fn leader_end(&self, connections: &[[f64; 2]]) -> [f64; 2] {
        let from = self.leader.last().copied().unwrap_or(self.at);
        connections
            .iter()
            .map(|c| self.to_sheet(*c))
            .min_by(|a, b| {
                let d = |q: &[f64; 2]| (q[0] - from[0]).hypot(q[1] - from[1]);
                d(a).total_cmp(&d(b))
            })
            .unwrap_or(self.at)
    }
}

impl DrawingDocument {
    /// Adds symbol definition `def` (Save Sketched Symbol); refused when its
    /// name is empty or taken.
    pub fn add_symbol(&mut self, def: Definition) -> Result<(), String> {
        let name = def.name.trim().to_string();
        if name.is_empty() {
            return Err("Define Sketched Symbol: the symbol needs a name".into());
        }
        if self.resources.symbols.iter().any(|d| d.name == name) {
            return Err(format!(
                "Define Sketched Symbol: a symbol named {name} exists"
            ));
        }
        self.resources.symbols.push(Definition { name, ..def });
        Ok(())
    }

    /// Replaces symbol `name`'s definition (Edit Definition): every instance
    /// changes; prompted values of entries that left it are dropped.
    pub fn update_symbol(&mut self, name: &str, def: Definition) -> bool {
        let Some(d) = self.resources.symbols.iter_mut().find(|d| d.name == name) else {
            return false;
        };
        *d = Definition {
            name: name.to_string(),
            ..def
        };
        let kept = d.prompts();
        for s in &mut self.sheets {
            for a in &mut s.annotations {
                if let super::AnnotationKind::Symbol(p) = &mut a.kind
                    && p.name == name
                {
                    p.prompts.retain(|(t, _)| kept.contains(t));
                }
            }
        }
        true
    }

    /// Deletes symbol `name`; refused while an instance of it is on a sheet.
    pub fn delete_symbol(&mut self, name: &str) -> Result<(), String> {
        let used = self.sheets.iter().find(|s| {
            s.annotations
                .iter()
                .any(|a| matches!(&a.kind, super::AnnotationKind::Symbol(p) if p.name == name))
        });
        if let Some(s) = used {
            return Err(format!("Delete: {} uses symbol {name}", s.name));
        }
        let before = self.resources.symbols.len();
        self.resources.symbols.retain(|d| d.name != name);
        if self.resources.symbols.len() == before {
            return Err(format!("Delete: no symbol named {name}"));
        }
        Ok(())
    }

    /// A free symbol name (`Symbol1`, `Symbol2` …).
    pub fn next_symbol_name(&self) -> String {
        (1..)
            .map(|k| format!("Symbol{k}"))
            .find(|n| !self.resources.symbols.iter().any(|d| &d.name == n))
            .unwrap_or_default()
    }

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

    /// Adds border definition `b` (Save Border); refused when its name is
    /// empty or taken.
    pub fn add_border(&mut self, mut b: BorderDef) -> Result<(), String> {
        let name = b.def.name.trim().to_string();
        if name.is_empty() {
            return Err("Define Border: the border needs a name".into());
        }
        if name == DEFAULT_BORDER || self.resources.borders.iter().any(|d| d.def.name == name) {
            return Err(format!("Define Border: a border named {name} exists"));
        }
        b.def.name = name;
        self.resources.borders.push(b);
        Ok(())
    }

    /// Replaces the sketch, texts and connection points of border `name`
    /// (Edit): every sheet using it changes; prompted values of entries
    /// that left it are dropped.
    pub fn update_border(&mut self, name: &str, def: Definition) -> bool {
        let Some(b) = self
            .resources
            .borders
            .iter_mut()
            .find(|d| d.def.name == name)
        else {
            return false;
        };
        b.def = Definition {
            name: name.to_string(),
            ..def
        };
        let kept = b.def.prompts();
        for s in &mut self.sheets {
            if let Some(p) = s.border_def.as_mut().filter(|p| p.name == name) {
                p.prompts.retain(|(t, _)| kept.contains(t));
            }
        }
        true
    }

    /// Puts border `name` on sheet `i` (`None`: the default border), with
    /// the prompted values `prompts`; a zone border brings its parameters
    /// (`params`: the sheet's own, from Insert's Edit Drawing Border
    /// Parameters, else the definition's). False when there is no such
    /// sheet or definition. Inventor keeps one border per sheet: the old
    /// one goes.
    pub fn set_border(
        &mut self,
        i: usize,
        name: Option<&str>,
        prompts: Vec<(String, String)>,
        params: Option<ZoneParams>,
    ) -> bool {
        let zones = match name {
            Some(n) => match self.resources.borders.iter().find(|d| d.def.name == n) {
                Some(b) => b.zones.clone(),
                None => return false,
            },
            None => None,
        };
        let Some(s) = self.sheets.get_mut(i) else {
            return false;
        };
        s.border = true;
        s.border_def = name.map(|n| PlacedTitle {
            name: n.to_string(),
            prompts,
        });
        if name.is_some() {
            s.border_params = params.or(zones).filter(|p| *p != ZoneParams::default());
        }
        true
    }

    /// Deletes border definition `name`; refused while a sheet uses it.
    pub fn delete_border(&mut self, name: &str) -> Result<(), String> {
        if let Some(s) = self
            .sheets
            .iter()
            .find(|s| s.border_def.as_ref().is_some_and(|p| p.name == name))
        {
            return Err(format!("Delete: {} uses border {name}", s.name));
        }
        let before = self.resources.borders.len();
        self.resources.borders.retain(|d| d.def.name != name);
        if self.resources.borders.len() == before {
            return Err(format!("Delete: no border named {name}"));
        }
        Ok(())
    }

    /// A free border name (`Border1`, `Border2` …).
    pub fn next_border_name(&self) -> String {
        (1..)
            .map(|k| format!("Border{k}"))
            .find(|n| !self.resources.borders.iter().any(|d| &d.def.name == n))
            .unwrap_or_default()
    }

    /// A free title block name (`Title Block1`, `Title Block2` …).
    pub fn next_title_block_name(&self) -> String {
        (1..)
            .map(|k| format!("Title Block{k}"))
            .find(|n| !self.resources.title_blocks.iter().any(|d| &d.name == n))
            .unwrap_or_default()
    }
}

/// The built-in border's name in Drawing Resources.
pub const DEFAULT_BORDER: &str = "Default Border";

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
            connections: Vec::new(),
            insertion: None,
        }
    }

    /// A 20 × 10 box at (10, 10) with a prompted text, its Insertion Point
    /// on the lower left corner and a connection point on the right side.
    fn symbol() -> Definition {
        use crate::drawing::view_sketch::TextAnchor;
        let mut sk = crate::sketch::Sketch::default();
        let p = |x, y| crate::sketch::Point2::new(x, y);
        let a = sk.add_point(p(10.0, 10.0));
        let b = sk.add_point(p(30.0, 10.0));
        let c = sk.add_point(p(30.0, 20.0));
        let d = sk.add_point(p(10.0, 20.0));
        for (u, v) in [(a, b), (b, c), (c, d), (d, a)] {
            let _ = sk.add_line_between(u, v);
        }
        let mut def = Definition::new(
            "Mark",
            ViewSketch(sk),
            vec![DrawingText {
                at: [12.0, 12.0],
                height: 3.5,
                anchor: TextAnchor::Left,
                angle: 0.0,
                pieces: fields::parse("<?Code>"),
            }],
        );
        def.insertion = Some([10.0, 10.0]);
        def.connections = vec![[30.0, 15.0]];
        def
    }

    fn placed(at: [f64; 2]) -> PlacedSymbol {
        PlacedSymbol {
            name: "Mark".into(),
            at,
            scale: 1.0,
            rotate: 0.0,
            fixed: false,
            leader: Vec::new(),
            leader_hidden: false,
            prompts: vec![("Code".into(), "A1".into())],
        }
    }

    fn near(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9
    }

    #[test]
    fn a_symbol_is_placed_scaled_and_turned_about_its_insertion_point() {
        use crate::drawing::{AnnotationKind, sheet::Prim};
        let mut d = DrawingDocument::default();
        assert!(d.add_symbol(symbol()).is_ok());
        assert!(d.add_symbol(symbol()).is_err(), "names are unique");
        assert_eq!(d.next_symbol_name(), "Symbol1");
        let mut s = placed([100.0, 100.0]);
        s.scale = 2.0;
        s.rotate = std::f64::consts::FRAC_PI_2;
        let art = symbol_art(&d, 0, &s, &|_| None).unwrap_or_default();
        let prims = s.place(&art.prims);
        // The box's far corner (20, 10) from the anchor: doubled and turned.
        let far = prims.iter().any(
            |p| matches!(p, Prim::Path { pts, .. } if pts.iter().any(|q| near(*q, [80.0, 140.0]))),
        );
        assert!(far, "{prims:?}");
        // The instance's own value, at twice the height, turned with it.
        assert!(prims.iter().any(|p| matches!(
            p,
            Prim::Text { text, height, angle, .. }
                if text == "A1" && *height == 7.0
                    && (*angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12
        )));
        // The leader ends on the connection point, its arrow on the tip.
        s.leader = vec![[60.0, 60.0]];
        assert!(near(s.leader_end(&art.connections), [90.0, 140.0]));
        // Without an Insertion Point: the middle of the box.
        let mut mid = symbol();
        mid.insertion = None;
        assert_eq!(mid.anchor(), Some([20.0, 15.0]));
        // In use: no Delete; a definition change keeps only live entries.
        assert!(d.add_annotation(AnnotationKind::Symbol(s)).is_some());
        assert!(d.delete_symbol("Mark").is_err());
        let mut edited = symbol();
        edited.texts.clear();
        assert!(d.update_symbol("Mark", edited));
        assert!(matches!(
            &d.sheets[0].annotations[0].kind,
            AnnotationKind::Symbol(p) if p.prompts.is_empty()
        ));
    }

    #[test]
    fn a_symbol_draws_through_the_tables_and_round_trips() {
        use crate::drawing::{AnnotationKind, annot, parts::Tables, sheet::Prim};
        let mut d = DrawingDocument::default();
        assert!(d.add_symbol(symbol()).is_ok());
        let mut s = placed([50.0, 60.0]);
        s.leader = vec![[20.0, 20.0]];
        let kind = AnnotationKind::Symbol(s.clone());
        assert!(d.add_annotation(kind.clone()).is_some());
        let tables = Tables::build(&d, 0, &|_, _, _| None);
        let look = d.styles.look();
        let shown = annot::resolve_in(&kind, &|_| None, &look, &tables);
        let Ok(shown) = shown else {
            panic!("{shown:?}");
        };
        // Four box lines, the leader's line to the connection point, its
        // arrow.
        let paths = |p: &&Prim| matches!(p, Prim::Path { .. });
        assert_eq!(shown.prims.iter().filter(paths).count(), 4);
        assert!(shown.prims.iter().any(|p| matches!(
            p,
            Prim::Line { a, b, .. } if near(*a, [20.0, 20.0]) && near(*b, [70.0, 65.0])
        )));
        // Visible off: the leader stays in the data, not on the sheet.
        s.leader_hidden = true;
        let hidden = AnnotationKind::Symbol(s);
        let lines = annot::resolve_in(&hidden, &|_| None, &look, &tables).map(|r| {
            r.prims
                .iter()
                .filter(|p| matches!(p, Prim::Line { .. }))
                .count()
        });
        assert_eq!(lines, Ok(0));
        // A definition that is gone: an orphan, not a crash.
        let gone = AnnotationKind::Symbol(placed([0.0, 0.0]));
        let empty = Tables::default();
        assert!(annot::resolve_in(&gone, &|_| None, &look, &empty).is_err());
        let json = serde_json::to_string(&d).unwrap_or_default();
        let back: DrawingDocument = serde_json::from_str(&json).unwrap_or_default();
        assert_eq!(back.resources, d.resources);
        assert_eq!(back.sheets[0].annotations, d.sheets[0].annotations);
    }

    /// A custom border: a rectangle 5 mm in from an A3 sheet's edges and a
    /// text with a prompted entry.
    fn custom_border() -> BorderDef {
        use crate::drawing::view_sketch::TextAnchor;
        let mut sk = crate::sketch::Sketch::default();
        let p = |x, y| crate::sketch::Point2::new(x, y);
        let a = sk.add_point(p(5.0, 5.0));
        let b = sk.add_point(p(415.0, 5.0));
        let c = sk.add_point(p(415.0, 292.0));
        let d = sk.add_point(p(5.0, 292.0));
        for (u, v) in [(a, b), (b, c), (c, d), (d, a)] {
            let _ = sk.add_line_between(u, v);
        }
        let mut def = Definition::new(
            "Frame",
            ViewSketch(sk),
            vec![DrawingText {
                at: [10.0, 285.0],
                height: 3.5,
                anchor: TextAnchor::Left,
                angle: 0.0,
                pieces: fields::parse("Project <?Project>"),
            }],
        );
        def.connections = vec![[395.0, 25.0], [210.0, 148.0]];
        BorderDef { def, zones: None }
    }

    #[test]
    fn a_custom_border_replaces_the_frame_and_holds_the_title_block() {
        use crate::drawing::sheet::Prim;
        let mut d = DrawingDocument::default();
        assert!(d.add_border(custom_border()).is_ok());
        assert!(d.add_border(custom_border()).is_err(), "names are unique");
        let mut named = custom_border();
        named.def.name = DEFAULT_BORDER.into();
        assert!(d.add_border(named).is_err(), "the built-in name is taken");
        assert_eq!(d.next_border_name(), "Border1");
        assert!(!d.set_border(0, Some("Nope"), Vec::new(), None));
        assert!(d.set_border(
            0,
            Some("Frame"),
            vec![("Project".into(), "X7".into())],
            None
        ));
        // The default frame and zones are gone: only the title block's lines
        // remain in the format.
        d.sheets[0].title_block = false;
        let format = crate::drawing::sheet::format_prims(&d, 0, &|_| String::new());
        assert!(format.is_empty(), "{format:?}");
        let prims = border_def_prims(&d, 0, &|_| None);
        assert_eq!(
            prims
                .iter()
                .filter(|p| matches!(p, Prim::Path { .. }))
                .count(),
            4
        );
        assert!(prims.iter().any(
            |p| matches!(p, Prim::Text { text, at, .. } if text == "Project X7" && *at == [10.0, 285.0])
        ));
        // The title block's lower right corner on the connection point
        // farthest out in the lower right quarter: the standard's block
        // (170 × 30) and the user's.
        d.sheets[0].title_block = true;
        let rect = crate::drawing::sheet::title_rect_in(&d, &d.sheets[0]);
        assert_eq!(rect, Some(([225.0, 25.0], [395.0, 55.0])));
        assert!(d.add_title_block(block().normalised()).is_ok());
        assert!(d.set_title_block(0, Some("Company"), Vec::new()));
        let rect = crate::drawing::sheet::title_rect_in(&d, &d.sheets[0]);
        assert_eq!(rect, Some(([295.0, 25.0], [395.0, 45.0])));
        // Without connection points: the border's own lower right corner.
        let mut def = custom_border().def;
        def.connections.clear();
        assert!(d.update_border("Frame", def));
        assert!(
            d.sheets[0]
                .border_def
                .as_ref()
                .is_some_and(|p| p.prompts.len() == 1),
            "Project is still asked"
        );
        let rect = crate::drawing::sheet::title_rect_in(&d, &d.sheets[0]);
        assert_eq!(rect, Some(([315.0, 5.0], [415.0, 25.0])));
        // A single point in the upper left: the block's upper left corner.
        let mut def = custom_border().def;
        def.connections = vec![[20.0, 280.0]];
        assert!(d.update_border("Frame", def));
        let rect = crate::drawing::sheet::title_rect_in(&d, &d.sheets[0]);
        assert_eq!(rect, Some(([20.0, 260.0], [120.0, 280.0])));
        // In use: no Delete. Back to the default border, then Delete.
        assert!(d.delete_border("Frame").is_err());
        assert!(d.set_border(0, None, Vec::new(), None));
        assert!(d.sheets[0].border_def.is_none());
        let rect = crate::drawing::sheet::title_rect_in(&d, &d.sheets[0]);
        assert_eq!(rect, Some(([310.0, 10.0], [410.0, 30.0])));
        assert!(d.delete_border("Frame").is_ok());
        assert!(d.delete_border("Frame").is_err());
    }

    #[test]
    fn a_zone_border_brings_its_parameters_and_round_trips() {
        let mut d = DrawingDocument::default();
        let mut b = custom_border();
        b.def.name = "Zones".into();
        b.zones = Some(ZoneParams {
            horizontal: 8,
            margins: Some([5.0, 5.0, 20.0, 5.0]),
            ..ZoneParams::default()
        });
        assert!(d.add_border(b.clone()).is_ok());
        assert!(d.set_border(0, Some("Zones"), Vec::new(), None));
        assert_eq!(d.sheets[0].border_params, b.zones);
        // The zones are drawn under the sketch.
        let format = crate::drawing::sheet::format_prims(&d, 0, &|_| String::new());
        assert!(
            format.iter().any(
                |p| matches!(p, crate::drawing::sheet::Prim::Text { text, .. } if text == "8")
            )
        );
        // Insert with parameters: the sheet's own win.
        let own = ZoneParams {
            vertical: 6,
            ..ZoneParams::default()
        };
        assert!(d.set_border(0, Some("Zones"), Vec::new(), Some(own.clone())));
        assert_eq!(d.sheets[0].border_params, Some(own));
        let json = serde_json::to_string(&d).unwrap_or_default();
        let back: DrawingDocument = serde_json::from_str(&json).unwrap_or_default();
        assert_eq!(back.resources, d.resources);
        assert_eq!(back.sheets[0].border_def, d.sheets[0].border_def);
        // An old drawing (no resources) reads as before.
        let old: crate::drawing::Sheet = serde_json::from_str(
            &serde_json::to_string(&crate::drawing::Sheet::new(
                "Sheet:1",
                crate::drawing::sheet::SheetSize::A3,
                crate::drawing::sheet::Orientation::Landscape,
            ))
            .unwrap_or_default(),
        )
        .unwrap_or_else(|_| d.sheets[0].clone());
        assert!(old.border_def.is_none());
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
