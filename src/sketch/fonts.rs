//! The installed fonts, for sketch text (`docs/spec/1.11-sketch-text.md`): found by family and
//! face in the system's font folders, read with `ttf-parser`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ttf_parser::{Face, GlyphId, OutlineBuilder, name_id};

use super::text::{Glyph, GlyphSource, TextSeg};
use crate::kernel::Real;

/// A face of a family: bold, italic → its file and index in it.
type Faces = BTreeMap<(bool, bool), (PathBuf, u32)>;

fn font_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if cfg!(windows) {
        let root = std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into());
        v.push(Path::new(&root).join("Fonts"));
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            v.push(
                Path::new(&l)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts"),
            );
        }
    } else {
        v.push("/usr/share/fonts".into());
        v.push("/usr/local/share/fonts".into());
        v.push("/Library/Fonts".into());
        v.push("/System/Library/Fonts".into());
        if let Some(h) = std::env::var_os("HOME") {
            v.push(Path::new(&h).join(".fonts"));
            v.push(Path::new(&h).join(".local/share/fonts"));
            v.push(Path::new(&h).join("Library/Fonts"));
        }
    }
    v
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if depth < 4 {
                walk(&p, depth + 1, out);
            }
        } else if p
            .extension()
            .and_then(|x| x.to_str())
            .is_some_and(|x| matches!(x.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc"))
        {
            out.push(p);
        }
    }
}

fn family_of(face: &Face) -> Option<String> {
    let read = |id: u16| {
        face.names()
            .into_iter()
            .filter(|n| n.name_id == id && n.is_unicode())
            .find_map(|n| n.to_string())
    };
    // The typographic family groups weights Windows lists apart; the plain one otherwise.
    read(name_id::FAMILY)
        .or_else(|| read(name_id::TYPOGRAPHIC_FAMILY))
        .filter(|s| !s.is_empty())
}

fn scan() -> BTreeMap<String, Faces> {
    let mut files = Vec::new();
    for d in font_dirs() {
        walk(&d, 0, &mut files);
    }
    let mut out: BTreeMap<String, Faces> = BTreeMap::new();
    for path in files {
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        let n = ttf_parser::fonts_in_collection(&data).unwrap_or(1);
        for index in 0..n {
            let Ok(face) = Face::parse(&data, index) else {
                continue;
            };
            let Some(family) = family_of(&face) else {
                continue;
            };
            if family.starts_with('.') {
                continue;
            }
            out.entry(family)
                .or_default()
                .entry((face.is_bold(), face.is_italic()))
                .or_insert_with(|| (path.clone(), index));
        }
    }
    out
}

fn all() -> &'static BTreeMap<String, Faces> {
    static FONTS: OnceLock<BTreeMap<String, Faces>> = OnceLock::new();
    FONTS.get_or_init(scan)
}

/// The installed families, sorted (the first call reads the font folders).
pub fn families() -> Vec<String> {
    all().keys().cloned().collect()
}

/// Whether `family` is installed.
pub fn installed(family: &str) -> bool {
    all().contains_key(family)
}

/// A font ready to give glyphs.
pub struct LoadedFont {
    data: Vec<u8>,
    index: u32,
    /// Height of a capital letter, in font units.
    cap: Real,
    /// A face without its own italic is slanted (bold cannot be faked well, it stays regular).
    slant: Real,
}

impl LoadedFont {
    /// A font from the bytes of its file.
    pub fn from_bytes(data: Vec<u8>, index: u32) -> Result<Self, String> {
        let face =
            Face::parse(&data, index).map_err(|e| format!("the font is not readable: {e}"))?;
        let box_of = |c: char| {
            face.glyph_index(c)
                .and_then(|g| face.glyph_bounding_box(g))
                .map(|b| Real::from(b.y_max))
        };
        let cap = box_of('H')
            .or_else(|| face.capital_height().map(Real::from))
            .or_else(|| box_of('x'))
            .unwrap_or_else(|| Real::from(face.units_per_em()) * 0.7);
        if cap <= 0.0 {
            return Err("the font has no usable capital height".into());
        }
        Ok(Self {
            data,
            index,
            cap,
            slant: 0.0,
        })
    }

    /// The installed `family`, the closest face to bold / italic.
    pub fn find(family: &str, bold: bool, italic: bool) -> Result<Self, String> {
        let faces = all()
            .get(family)
            .ok_or_else(|| format!("the font {family} is not installed"))?;
        let want = [
            (bold, italic),
            (false, italic),
            (bold, false),
            (false, false),
        ];
        let found = want
            .iter()
            .find_map(|k| faces.get(k).map(|v| (*k, v)))
            .or_else(|| faces.iter().next().map(|(k, v)| (*k, v)));
        let Some((key, (path, index))) = found else {
            return Err(format!("the font {family} has no usable face"));
        };
        let data =
            std::fs::read(path).map_err(|e| format!("the font {family} cannot be read: {e}"))?;
        let mut f = Self::from_bytes(data, *index)?;
        if italic && !key.1 {
            f.slant = 0.2;
        }
        Ok(f)
    }
}

/// Collects one glyph's outline.
struct Outline {
    k: Real,
    slant: Real,
    loops: Glyph,
    cur: Vec<TextSeg>,
    start: [Real; 2],
    at: [Real; 2],
}

impl Outline {
    fn p(&self, x: f32, y: f32) -> [Real; 2] {
        let (x, y) = (Real::from(x) * self.k, Real::from(y) * self.k);
        [x + self.slant * y, y]
    }

    fn end_loop(&mut self) {
        if self.cur.is_empty() {
            return;
        }
        let d = (self.at[0] - self.start[0]).hypot(self.at[1] - self.start[1]);
        if d > 1e-12 {
            self.cur.push(TextSeg::Line([self.at, self.start]));
        }
        self.loops.push(std::mem::take(&mut self.cur));
    }
}

impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.end_loop();
        self.start = self.p(x, y);
        self.at = self.start;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let b = self.p(x, y);
        if (b[0] - self.at[0]).hypot(b[1] - self.at[1]) > 1e-12 {
            self.cur.push(TextSeg::Line([self.at, b]));
            self.at = b;
        }
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        // Raised to a cubic: the same curve.
        let (a, c, b) = (self.at, self.p(x1, y1), self.p(x, y));
        let third = |p: [Real; 2], q: [Real; 2]| {
            [
                p[0] + (q[0] - p[0]) * 2.0 / 3.0,
                p[1] + (q[1] - p[1]) * 2.0 / 3.0,
            ]
        };
        self.cur
            .push(TextSeg::Cubic([a, third(a, c), third(b, c), b]));
        self.at = b;
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let b = self.p(x, y);
        self.cur
            .push(TextSeg::Cubic([self.at, self.p(x1, y1), self.p(x2, y2), b]));
        self.at = b;
    }

    fn close(&mut self) {
        self.end_loop();
        self.at = self.start;
    }
}

impl GlyphSource for LoadedFont {
    fn glyph(&self, c: char) -> Option<(Real, Glyph)> {
        let face = Face::parse(&self.data, self.index).ok()?;
        let k = 1.0 / self.cap;
        let g: GlyphId = face.glyph_index(c)?;
        let advance = Real::from(face.glyph_hor_advance(g)?) * k;
        let mut o = Outline {
            k,
            slant: self.slant,
            loops: Vec::new(),
            cur: Vec::new(),
            start: [0.0, 0.0],
            at: [0.0, 0.0],
        };
        // A space has no outline: that is not an error.
        let _ = face.outline_glyph(g, &mut o);
        o.end_loop();
        Some((advance, o.loops))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_font_is_an_error() {
        assert!(LoadedFont::find("No Such Font 123", false, false).is_err());
        assert!(LoadedFont::from_bytes(vec![1, 2, 3], 0).is_err());
    }
}
