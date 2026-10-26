//! Sheet sizes, the border and the title block as 2D primitives in sheet
//! millimetres (the same list feeds the screen and, later, PDF / DXF).

use serde::{Deserialize, Serialize};

use super::{DrawingDocument, Sheet, scale_text};

/// Paper sizes (ISO 216 and ANSI Y14.1).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SheetSize {
    /// ISO A4 297 × 210.
    A4,
    /// ISO A3 420 × 297.
    A3,
    /// ISO A2 594 × 420.
    A2,
    /// ISO A1 841 × 594.
    A1,
    /// ISO A0 1189 × 841.
    A0,
    /// ANSI A 11 × 8.5 in.
    AnsiA,
    /// ANSI B 17 × 11 in.
    AnsiB,
    /// ANSI C 22 × 17 in.
    AnsiC,
    /// ANSI D 34 × 22 in.
    AnsiD,
    /// ANSI E 44 × 34 in.
    AnsiE,
    /// Any size (landscape width × height, mm).
    Custom {
        /// Width.
        width: f64,
        /// Height.
        height: f64,
    },
}

impl SheetSize {
    /// The named sizes, in the Edit Sheet list.
    pub const NAMED: [SheetSize; 10] = [
        SheetSize::A4,
        SheetSize::A3,
        SheetSize::A2,
        SheetSize::A1,
        SheetSize::A0,
        SheetSize::AnsiA,
        SheetSize::AnsiB,
        SheetSize::AnsiC,
        SheetSize::AnsiD,
        SheetSize::AnsiE,
    ];

    /// Display name.
    pub fn label(self) -> String {
        match self {
            SheetSize::A4 => "A4".into(),
            SheetSize::A3 => "A3".into(),
            SheetSize::A2 => "A2".into(),
            SheetSize::A1 => "A1".into(),
            SheetSize::A0 => "A0".into(),
            SheetSize::AnsiA => "A (ANSI)".into(),
            SheetSize::AnsiB => "B (ANSI)".into(),
            SheetSize::AnsiC => "C (ANSI)".into(),
            SheetSize::AnsiD => "D (ANSI)".into(),
            SheetSize::AnsiE => "E (ANSI)".into(),
            SheetSize::Custom { .. } => "Custom Size".into(),
        }
    }

    /// Landscape width and height in mm.
    pub fn landscape(self) -> (f64, f64) {
        let inch = 25.4;
        match self {
            SheetSize::A4 => (297.0, 210.0),
            SheetSize::A3 => (420.0, 297.0),
            SheetSize::A2 => (594.0, 420.0),
            SheetSize::A1 => (841.0, 594.0),
            SheetSize::A0 => (1189.0, 841.0),
            SheetSize::AnsiA => (11.0 * inch, 8.5 * inch),
            SheetSize::AnsiB => (17.0 * inch, 11.0 * inch),
            SheetSize::AnsiC => (22.0 * inch, 17.0 * inch),
            SheetSize::AnsiD => (34.0 * inch, 22.0 * inch),
            SheetSize::AnsiE => (44.0 * inch, 34.0 * inch),
            SheetSize::Custom { width, height } => (width.max(height), width.min(height)),
        }
    }

    /// Width and height in `orientation`.
    pub fn dims(self, orientation: Orientation) -> (f64, f64) {
        let (w, h) = self.landscape();
        match orientation {
            Orientation::Landscape => (w, h),
            Orientation::Portrait => (h, w),
        }
    }
}

/// Sheet orientation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    /// Wider than high (the default).
    #[default]
    Landscape,
    /// Higher than wide.
    Portrait,
}

/// Line weights of the sheet format (ISO 128: thin 0.25, thick 0.5 / 0.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
    /// 0.25 mm.
    Thin,
    /// 0.5 mm.
    Medium,
    /// 0.7 mm.
    Thick,
}

impl Weight {
    /// Width on paper (mm).
    pub fn mm(self) -> f64 {
        match self {
            Weight::Thin => 0.25,
            Weight::Medium => 0.5,
            Weight::Thick => 0.7,
        }
    }
}

/// Horizontal anchor of a text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// The point is the text's left end.
    Left,
    /// The point is the text's middle.
    Center,
}

/// One drawn thing of the sheet format.
#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    /// A straight line.
    Line {
        /// Start (mm).
        a: [f64; 2],
        /// End (mm).
        b: [f64; 2],
        /// Weight.
        weight: Weight,
    },
    /// A text line; `at` is its baseline at the anchor.
    Text {
        /// Position (mm).
        at: [f64; 2],
        /// Cap height (mm).
        height: f64,
        /// Contents.
        text: String,
        /// Anchor.
        anchor: Anchor,
        /// Rotation about `at` (radians, counter-clockwise).
        angle: f64,
    },
    /// A filled polygon (arrowheads).
    Fill {
        /// Corners (mm).
        pts: Vec<[f64; 2]>,
    },
    /// A polyline of a view (visible solid, hidden dashed).
    Path {
        /// Points (mm).
        pts: Vec<[f64; 2]>,
        /// Width on paper (mm).
        width: f64,
        /// Hidden-line dashes.
        dashed: bool,
    },
    /// A thin centre line (long dash, dot).
    Center {
        /// Start (mm).
        a: [f64; 2],
        /// End (mm).
        b: [f64; 2],
    },
}

/// Fields of the title block: iProperties of the drawing or of the model of
/// its first view, and values of the sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleField {
    /// Model: Title (else Description, else the document name).
    Title,
    /// Model: Part Number.
    PartNumber,
    /// Drawing: Designer (else Author).
    Designer,
    /// Drawing: Checked By.
    CheckedBy,
    /// Drawing: Creation Date.
    Date,
    /// Model: material.
    Material,
    /// Drawing: Company.
    Company,
    /// Model: Revision Number (else the sheet's revision).
    Revision,
}

impl TitleField {
    /// Every field, for the block and tests.
    pub const ALL: [TitleField; 8] = [
        TitleField::Title,
        TitleField::PartNumber,
        TitleField::Designer,
        TitleField::CheckedBy,
        TitleField::Date,
        TitleField::Material,
        TitleField::Company,
        TitleField::Revision,
    ];

    /// Caption above the value.
    pub fn caption(self) -> &'static str {
        match self {
            TitleField::Title => "TITLE",
            TitleField::PartNumber => "PART NUMBER",
            TitleField::Designer => "DESIGNED BY",
            TitleField::CheckedBy => "CHECKED BY",
            TitleField::Date => "DATE",
            TitleField::Material => "MATERIAL",
            TitleField::Company => "COMPANY",
            TitleField::Revision => "REV",
        }
    }
}

/// Margins of the frame (ISO 5457: 20 mm filing margin on the left).
pub const MARGIN: f64 = 10.0;
/// Left (filing) margin.
pub const FILING_MARGIN: f64 = 20.0;
/// Title block width (ISO 7200: 180 mm).
pub const TITLE_W: f64 = 180.0;
/// Title block height.
pub const TITLE_H: f64 = 36.0;

/// The inner frame (lower left, upper right) of a sheet.
pub fn frame(sheet: &Sheet) -> ([f64; 2], [f64; 2]) {
    let (w, h) = sheet.dims();
    ([FILING_MARGIN, MARGIN], [w - MARGIN, h - MARGIN])
}

/// The title block's rectangle (lower left, upper right), when shown.
pub fn title_rect(sheet: &Sheet) -> Option<([f64; 2], [f64; 2])> {
    if !sheet.title_block {
        return None;
    }
    let (lo, hi) = frame(sheet);
    let w = TITLE_W.min(hi[0] - lo[0]);
    Some(([hi[0] - w, lo[1]], [hi[0], lo[1] + TITLE_H]))
}

/// The border and title block of sheet `i` of `doc`; `value` gives a field's
/// text.
pub fn format_prims(
    doc: &DrawingDocument,
    i: usize,
    value: &dyn Fn(TitleField) -> String,
) -> Vec<Prim> {
    let Some(sheet) = doc.sheets.get(i) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let line = |out: &mut Vec<Prim>, a: [f64; 2], b: [f64; 2], weight| {
        out.push(Prim::Line { a, b, weight });
    };
    let text = |out: &mut Vec<Prim>, at: [f64; 2], height: f64, t: &str, anchor| {
        if !t.is_empty() {
            out.push(Prim::Text {
                at,
                height,
                text: t.to_string(),
                anchor,
                angle: 0.0,
            });
        }
    };
    let rect = |out: &mut Vec<Prim>, lo: [f64; 2], hi: [f64; 2], weight| {
        line(out, lo, [hi[0], lo[1]], weight);
        line(out, [hi[0], lo[1]], hi, weight);
        line(out, hi, [lo[0], hi[1]], weight);
        line(out, [lo[0], hi[1]], lo, weight);
    };
    let (w, h) = sheet.dims();
    if sheet.border {
        let (lo, hi) = frame(sheet);
        rect(&mut out, lo, hi, Weight::Thick);
        // Zones: numbers along the top and bottom, letters down the sides
        // (ISO 5457: 1 at the left, A at the top; ANSI: from the lower right).
        let ansi = doc.standard.kind == super::StandardKind::Ansi;
        let nx = ((hi[0] - lo[0]) / 50.0).round().max(2.0) as usize;
        let ny = ((hi[1] - lo[1]) / 50.0).round().max(2.0) as usize;
        let zx = (hi[0] - lo[0]) / nx as f64;
        let zy = (hi[1] - lo[1]) / ny as f64;
        let tick = 5.0f64.min(MARGIN * 0.5);
        for k in 0..nx {
            let x = lo[0] + zx * k as f64;
            if k > 0 {
                line(&mut out, [x, lo[1]], [x, lo[1] - tick], Weight::Thin);
                line(&mut out, [x, hi[1]], [x, hi[1] + tick], Weight::Thin);
            }
            let mid = x + zx * 0.5;
            let n = format!("{}", if ansi { nx - k } else { k + 1 });
            text(
                &mut out,
                [mid, lo[1] - MARGIN * 0.75],
                3.5,
                &n,
                Anchor::Center,
            );
            text(
                &mut out,
                [mid, hi[1] + MARGIN * 0.25],
                3.5,
                &n,
                Anchor::Center,
            );
        }
        for k in 0..ny {
            let y = lo[1] + zy * k as f64;
            if k > 0 {
                line(&mut out, [lo[0], y], [lo[0] - tick, y], Weight::Thin);
                line(&mut out, [hi[0], y], [hi[0] + tick, y], Weight::Thin);
            }
            let j = if ansi { k } else { ny - 1 - k };
            let letter = (b'A' + (j as u8).min(25)) as char;
            let s = letter.to_string();
            let mid = y + zy * 0.5 - 1.75;
            text(
                &mut out,
                [lo[0] - MARGIN * 0.5, mid],
                3.5,
                &s,
                Anchor::Center,
            );
            text(
                &mut out,
                [hi[0] + MARGIN * 0.5, mid],
                3.5,
                &s,
                Anchor::Center,
            );
        }
        // Centring marks (ISO 5457).
        for (a, b) in [
            ([w * 0.5, 0.0], [w * 0.5, lo[1]]),
            ([w * 0.5, h], [w * 0.5, hi[1]]),
            ([0.0, h * 0.5], [lo[0], h * 0.5]),
            ([w, h * 0.5], [hi[0], h * 0.5]),
        ] {
            line(&mut out, a, b, Weight::Thick);
        }
    }
    if let Some((lo, hi)) = title_rect(sheet) {
        rect(&mut out, lo, hi, Weight::Thick);
        // Rows: (caption, value) cells; the bottom row is the title.
        let (x0, x1, y0) = (lo[0], hi[0], lo[1]);
        let row = TITLE_H / 4.0;
        let cell = |out: &mut Vec<Prim>, a: f64, b: f64, y: f64, cap: &str, v: &str, big: bool| {
            line(out, [a, y], [a, y + row], Weight::Thin);
            text(out, [a + 1.5, y + row - 3.2], 1.8, cap, Anchor::Left);
            let hgt: f64 = if big { 5.0 } else { 3.5 };
            let at = [a + (b - a) * 0.5, y + 1.6];
            text(out, at, hgt.min(row - 4.0), v, Anchor::Center);
        };
        for k in 1..4 {
            let y = y0 + row * k as f64;
            line(&mut out, [x0, y], [x1, y], Weight::Thin);
        }
        let (n, m) = doc.sheet_number(i);
        let scale = sheet
            .views
            .first()
            .and_then(|v| doc.effective(sheet, v.id))
            .map(|(s, _)| scale_text(s))
            .unwrap_or_default();
        let w3 = (x1 - x0) / 3.0;
        let get = |f| value(f);
        // Top row: company / designer / checked by.
        let y = y0 + row * 3.0;
        cell(
            &mut out,
            x0,
            x0 + w3,
            y,
            "COMPANY",
            &get(TitleField::Company),
            false,
        );
        cell(
            &mut out,
            x0 + w3,
            x0 + 2.0 * w3,
            y,
            "DESIGNED BY",
            &get(TitleField::Designer),
            false,
        );
        cell(
            &mut out,
            x0 + 2.0 * w3,
            x1,
            y,
            "CHECKED BY",
            &get(TitleField::CheckedBy),
            false,
        );
        // Second row: material / date / scale.
        let y = y0 + row * 2.0;
        cell(
            &mut out,
            x0,
            x0 + w3,
            y,
            "MATERIAL",
            &get(TitleField::Material),
            false,
        );
        cell(
            &mut out,
            x0 + w3,
            x0 + 2.0 * w3,
            y,
            "DATE",
            &get(TitleField::Date),
            false,
        );
        cell(&mut out, x0 + 2.0 * w3, x1, y, "SCALE", &scale, false);
        // Third row: part number / revision / sheet.
        let y = y0 + row;
        let rev = {
            let r = get(TitleField::Revision);
            if r.is_empty() {
                sheet.revision.clone()
            } else {
                r
            }
        };
        let sheet_text = if sheet.exclude_from_count {
            String::new()
        } else {
            format!("{n} / {m}")
        };
        cell(
            &mut out,
            x0,
            x0 + 2.0 * w3,
            y,
            "PART NUMBER",
            &get(TitleField::PartNumber),
            false,
        );
        cell(
            &mut out,
            x0 + 2.0 * w3,
            x0 + 2.5 * w3,
            y,
            "REV",
            &rev,
            false,
        );
        cell(&mut out, x0 + 2.5 * w3, x1, y, "SHEET", &sheet_text, false);
        // Bottom row: the title over the whole width.
        cell(&mut out, x0, x1, y0, "TITLE", &get(TitleField::Title), true);
    }
    out
}
