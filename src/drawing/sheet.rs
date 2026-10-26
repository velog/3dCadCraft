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
    /// A centre line (long dash, dot).
    Center {
        /// Start (mm).
        a: [f64; 2],
        /// End (mm).
        b: [f64; 2],
        /// Width on paper (mm).
        width: f64,
        /// Long dash, dot; else continuous (the layer's line type).
        chain: bool,
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
    /// Drawing: Eng. Approved By.
    ApprovedBy,
    /// Drawing: Eng. Approved Date.
    ApprovedDate,
}

impl TitleField {
    /// Every field, for the block and tests.
    pub const ALL: [TitleField; 10] = [
        TitleField::Title,
        TitleField::PartNumber,
        TitleField::Designer,
        TitleField::CheckedBy,
        TitleField::Date,
        TitleField::Material,
        TitleField::Company,
        TitleField::Revision,
        TitleField::ApprovedBy,
        TitleField::ApprovedDate,
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
            TitleField::ApprovedBy => "APPROVED BY",
            TitleField::ApprovedDate => "DATE",
        }
    }
}

/// The frame's margin (Inventor 2027's ISO template: 10 mm on every side,
/// 20 mm on A0 and A1; read through its API, see `tools/inventor/`).
pub fn margin(sheet: &Sheet) -> f64 {
    if matches!(sheet.size, SheetSize::A0 | SheetSize::A1) {
        20.0
    } else {
        10.0
    }
}
/// Title block width (Inventor's ISO title block: 170 × 30 mm).
pub const TITLE_W: f64 = 170.0;
/// Title block height.
pub const TITLE_H: f64 = 30.0;
/// Zones of the default border (Inventor's ISO template: 6 × 4 on every
/// size, labelled from the bottom right).
pub const ZONES: (usize, usize) = (6, 4);

/// The inner frame (lower left, upper right) of a sheet.
pub fn frame(sheet: &Sheet) -> ([f64; 2], [f64; 2]) {
    let (w, h) = sheet.dims();
    let m = margin(sheet);
    ([m, m], [w - m, h - m])
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
        // Zones (Inventor's default border): numbers along the top and
        // bottom counted from the right, letters up the sides from the bottom.
        let m = margin(sheet);
        let (nx, ny) = ZONES;
        let zx = (hi[0] - lo[0]) / nx as f64;
        let zy = (hi[1] - lo[1]) / ny as f64;
        for k in 0..nx {
            let x = lo[0] + zx * k as f64;
            if k > 0 {
                line(&mut out, [x, lo[1]], [x, lo[1] - m], Weight::Thin);
                line(&mut out, [x, hi[1]], [x, hi[1] + m], Weight::Thin);
            }
            let mid = x + zx * 0.5;
            let n = format!("{}", nx - k);
            text(
                &mut out,
                [mid, lo[1] - m * 0.5 - 1.75],
                3.5,
                &n,
                Anchor::Center,
            );
            text(
                &mut out,
                [mid, hi[1] + m * 0.5 - 1.75],
                3.5,
                &n,
                Anchor::Center,
            );
        }
        for k in 0..ny {
            let y = lo[1] + zy * k as f64;
            if k > 0 {
                line(&mut out, [lo[0], y], [lo[0] - m, y], Weight::Thin);
                line(&mut out, [hi[0], y], [hi[0] + m, y], Weight::Thin);
            }
            let letter = (b'A' + (k as u8).min(25)) as char;
            let s = letter.to_string();
            let mid = y + zy * 0.5 - 1.75;
            text(&mut out, [lo[0] - m * 0.5, mid], 3.5, &s, Anchor::Center);
            text(&mut out, [hi[0] + m * 0.5, mid], 3.5, &s, Anchor::Center);
        }
        // Centring marks: a line from the paper's edge to the frame and an
        // open triangle pointing into the drawing area.
        let t = (m * 0.5).min(5.0);
        for (edge, apex, inward) in [
            ([w * 0.5, 0.0], [w * 0.5, lo[1]], [0.0, 1.0]),
            ([w * 0.5, h], [w * 0.5, hi[1]], [0.0, -1.0]),
            ([0.0, h * 0.5], [lo[0], h * 0.5], [1.0, 0.0]),
            ([w, h * 0.5], [hi[0], h * 0.5], [-1.0, 0.0]),
        ] {
            line(&mut out, edge, apex, Weight::Thin);
            let base = [apex[0] - inward[0] * t, apex[1] - inward[1] * t];
            let side = [inward[1] * t * 0.6, -inward[0] * t * 0.6];
            let (l, r) = (
                [base[0] + side[0], base[1] + side[1]],
                [base[0] - side[0], base[1] - side[1]],
            );
            line(&mut out, apex, l, Weight::Thin);
            line(&mut out, l, r, Weight::Thin);
            line(&mut out, r, apex, Weight::Thin);
        }
    }
    if let Some((lo, hi)) = title_rect(sheet) {
        // Inventor's ISO title block (170 × 30 mm, three 10 mm rows): the
        // first row holds who and when, the left of rows 2–3 the company, the
        // last row the title, edition and sheet. The part number, scale and
        // material sit in row 2, which Inventor leaves empty.
        rect(&mut out, lo, hi, Weight::Thick);
        let (x0, y0) = (lo[0], lo[1]);
        let k = (hi[0] - lo[0]) / TITLE_W;
        let x = |mm: f64| x0 + mm * k;
        let row = TITLE_H / 3.0;
        let (y1, y2) = (y0 + row, y0 + 2.0 * row);
        line(&mut out, [x0, y2], [hi[0], y2], Weight::Thin);
        line(&mut out, [x(74.8), y1], [hi[0], y1], Weight::Thin);
        line(&mut out, [x(74.8), y0], [x(74.8), y2], Weight::Thin);
        let get = |f| value(f);
        // A caption at the top left of a cell and its value under it.
        let cell = |out: &mut Vec<Prim>, a: f64, b: f64, y: f64, cap: &str, v: &str, big: bool| {
            line(out, [a, y], [a, y + row], Weight::Thin);
            text(out, [a + 1.0, y + row - 2.6], 1.8, cap, Anchor::Left);
            let hgt: f64 = if big { 5.0 } else { 3.5 };
            if big {
                text(out, [(a + b) * 0.5, y + 2.0], hgt, v, Anchor::Center);
            } else {
                text(out, [a + 1.0, y + 1.6], hgt, v, Anchor::Left);
            }
        };
        let (n, m) = doc.sheet_number(i);
        let scale = sheet
            .views
            .first()
            .and_then(|v| doc.effective(sheet, v.id))
            .map(|(s, _)| scale_text(s))
            .unwrap_or_default();
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
        // Row 1.
        cell(
            &mut out,
            x(0.0),
            x(31.8),
            y2,
            "Designed by",
            &get(TitleField::Designer),
            false,
        );
        cell(
            &mut out,
            x(31.8),
            x(56.5),
            y2,
            "Checked by",
            &get(TitleField::CheckedBy),
            false,
        );
        cell(
            &mut out,
            x(56.5),
            x(97.3),
            y2,
            "Approved by",
            &get(TitleField::ApprovedBy),
            false,
        );
        cell(
            &mut out,
            x(97.3),
            x(118.2),
            y2,
            "Date",
            &get(TitleField::ApprovedDate),
            false,
        );
        cell(
            &mut out,
            x(118.2),
            x(149.4),
            y2,
            "Date",
            &get(TitleField::Date),
            false,
        );
        cell(&mut out, x(149.4), x(170.0), y2, "", "", false);
        // Rows 2–3, left: the company.
        line(&mut out, [x0, y0], [x0, y2], Weight::Thin);
        text(&mut out, [x0 + 1.0, y2 - 2.6], 1.8, "Company", Anchor::Left);
        text(
            &mut out,
            [x(37.4), y0 + 7.0],
            5.0,
            &get(TitleField::Company),
            Anchor::Center,
        );
        // Row 2, right: part number, scale, material.
        cell(
            &mut out,
            x(74.8),
            x(118.2),
            y1,
            "Part number",
            &get(TitleField::PartNumber),
            false,
        );
        cell(&mut out, x(118.2), x(134.5), y1, "Scale", &scale, false);
        cell(
            &mut out,
            x(134.5),
            x(170.0),
            y1,
            "Material",
            &get(TitleField::Material),
            false,
        );
        // Row 3, right: title, edition, sheet.
        cell(
            &mut out,
            x(74.8),
            x(134.5),
            y0,
            "",
            &get(TitleField::Title),
            true,
        );
        cell(&mut out, x(134.5), x(149.4), y0, "Edition", &rev, false);
        cell(
            &mut out,
            x(149.4),
            x(170.0),
            y0,
            "Sheet",
            &sheet_text,
            false,
        );
    }
    out
}
