//! Frame member sections (`docs/spec/6.1-frame-generator.md`, profile
//! library 6.3 CC3), GUI-free: the section table (`data/profiles/profiles.csv`,
//! built in, plus a user file overriding sizes by family + size), each
//! shape's outline (lines and arcs, its bounding box centred on the origin),
//! the section properties in closed form over the outline, and the member
//! part — the section sketched on XY and extruded along +Z by the length.
//!
//! A member part holds everything it needs (rule 8): the numbers are copied
//! into its sketch, so it regenerates the same without the library.

use crate::document::iprops::{CustomProperty, IProperties, PropValue};
use crate::kernel::{Point3, Real, Vector3};
use crate::part::PlaneRef;
use crate::part::split::{SplitFeature, SplitMethod, SplitToolRef};
use crate::part::work::{UcsDef, WorkDef, WorkFeature};
use crate::part::{
    ExtrudeFeature, OriginPlane, PartDocument, PartError, ProfileRef, SketchOptions,
    SketchPlacement,
};
use crate::sketch::{Point2, Sketch, Vector2};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// The built-in table.
const BUILTIN: &str = include_str!("../../data/profiles/profiles.csv");

/// The custom property holding a member's length (Inventor's `G_L`).
pub const LENGTH_PROPERTY: &str = "G_L";

/// A member length as it is stored in [`LENGTH_PROPERTY`]: to 0.001 mm. The
/// value is read off a display mesh, so it carries noise (399.99999998).
pub fn stored_length(l: Real) -> Real {
    (l * 1000.0).round() / 1000.0
}

/// The custom properties with the cut of a member's start and end
/// (Inventor's `CUTDETAIL<n>`; a square end has none).
pub const CUT_PROPERTIES: [&str; 2] = ["CUTDETAIL1", "CUTDETAIL2"];

/// The kind of section: which template draws it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// Square hollow section (rounded corners).
    Shs,
    /// Rectangular hollow section.
    Rhs,
    /// Circular hollow section.
    Chs,
    /// Angle (L), root and toe radii.
    Angle,
    /// I / H beam with parallel flanges and root radii.
    IBeam,
    /// Flat bar.
    Flat,
    /// Round bar.
    Bar,
    /// Channel (U) with parallel flanges: web on the left, root and toe radii.
    Channel,
    /// T section, flange on top, stem down.
    Tee,
    /// Channel with tapered flanges (UPN, 8 % slope): web on the left.
    ChannelTapered,
    /// I beam with tapered flanges (IPN, 14 % slope).
    IBeamTapered,
    /// Square bar (side `h`).
    SquareBar,
    /// Hexagon bar (`h` across the flats, flats top and bottom).
    HexBar,
    /// Cold-formed Z (`h`, flanges `b` to the web's outer face, wall `t`,
    /// inner bend radius `r1`): top flange to the right.
    ZedFormed,
    /// Cold-formed lipped channel (`h`, `b`, wall `t`, lips `tf`, inner bend
    /// radius `r1`): web on the left.
    ChannelFormed,
    /// The user's own outline (`Section::custom`, spec 6.1 §3.11).
    Custom,
    /// Aluminium T-slot framing on a grid (spec 6.1 §3.11a): `h` x `b`, a
    /// slot in the middle of every grid cell's face (opening `tw`, lip `t`,
    /// groove depth `tf` from the face), a core bore `r1` (diameter) in every
    /// cell, outer corner radius `r2`, grid `grid`. The chamber under the lips
    /// and the cavities between cells are sized so the section's area is the
    /// published one (`area`): a maker's inner webs are not modelled.
    TSlot,
}

impl Shape {
    fn parse(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "shs" => Shape::Shs,
            "rhs" => Shape::Rhs,
            "chs" => Shape::Chs,
            "angle" => Shape::Angle,
            "ibeam" => Shape::IBeam,
            "flat" => Shape::Flat,
            "bar" => Shape::Bar,
            "channel" => Shape::Channel,
            "tee" => Shape::Tee,
            "upn" => Shape::ChannelTapered,
            "ipn" => Shape::IBeamTapered,
            "sqbar" => Shape::SquareBar,
            "hexbar" => Shape::HexBar,
            "zcf" => Shape::ZedFormed,
            "ccf" => Shape::ChannelFormed,
            "custom" => Shape::Custom,
            "tslot" => Shape::TSlot,
            _ => return None,
        })
    }
}

/// One size of a family.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    /// Family (`Square hollow section (EN 10219)`).
    pub family: String,
    /// Standard (`EN`).
    pub standard: String,
    /// Which template.
    pub shape: Shape,
    /// Designation within the family (`40x40x3`, `IPE 200`).
    pub size: String,
    /// Height / outside diameter.
    pub h: Real,
    /// Width.
    pub b: Real,
    /// Wall / leg / bar thickness.
    pub t: Real,
    /// Web thickness (beams).
    pub tw: Real,
    /// Flange thickness (beams).
    pub tf: Real,
    /// Outer radius (hollow) / root radius (angle, beam).
    pub r1: Real,
    /// Inner radius (hollow) / toe radius (angle).
    pub r2: Real,
    /// The outline of a [`Shape::Custom`] section (bounding box centred on
    /// the origin); empty for the templates.
    pub custom: Vec<Loop>,
    /// Grid of a [`Shape::TSlot`] section (slot spacing), mm; 0 for others.
    pub grid: Real,
    /// The published area of a [`Shape::TSlot`] section, mm²; 0 = none.
    pub area: Real,
    /// The published chamber width of a thin-walled [`Shape::TSlot`] section
    /// (its maker's catalogue); 0 = the chamber is sized to the area instead.
    pub chamber: Real,
    /// Depth from the face to where a thin-walled section's chamber walls start
    /// to slope towards its floor (where the corner blocks' legs end).
    pub undercut: Real,
    /// The maker's published second moment about the section's x axis, mm⁴; 0 = none.
    /// [`Section::properties`] gives it, and a walled T-slot outline is fitted to it.
    pub ix_published: Real,
    /// The same about the y axis.
    pub iy_published: Real,
    /// The maker's published torsion constant, mm⁴; 0 = none ([`Section::torsion`]).
    pub it_published: Real,
}

/// What a T-slot section's published area makes of its inner shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TSlotFit {
    /// Width of the chamber under the lips.
    pub chamber: Real,
    /// Half diagonal of the square cavity on each boundary between two cells.
    pub between: Real,
    /// Half side of the square cavity where four cells meet.
    pub node: Real,
}

/// What a thin-walled T-slot section's published area and second moments make of
/// its outline ([`Section::walled_fit`]), mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WalledFit {
    /// Thickness of the walls, the hooks at the chambers and the webs.
    pub wall: Real,
    /// Side of the solid block in each corner of a cell.
    pub block: Real,
    /// Half side of the core round each bore.
    pub core: Real,
}

/// The thinnest web a T-slot section keeps between two of its cavities, mm.
const TSLOT_WEB: Real = 1.2;

/// A closed loop of the outline: corners with their fillet radius (0 =
/// sharp), counter-clockwise for the outside, clockwise for a hole; or a
/// circle; or a path of lines and arcs (a user's outline).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Loop {
    /// Corners and fillet radii.
    Corners(Vec<(Point2, Real)>),
    /// A whole circle (centre, radius); `hole` = a hole.
    Circle(Point2, Real, bool),
    /// Vertices in loop order; each runs to the next by a line, or by an
    /// arc about the given centre (`true` = counter-clockwise).
    Path(Vec<(Point2, Option<(Point2, bool)>)>),
}

/// Section properties (mm units): area, centroid, second moments about
/// the centroid's axes (x horizontal, y vertical), section moduli and radii
/// of gyration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionProps {
    /// Area, mm².
    pub area: Real,
    /// Centroid in section coordinates (the bounding box is centred on 0).
    pub centroid: Point2,
    /// Second moment about the horizontal axis through the centroid, mm⁴.
    pub ix: Real,
    /// Second moment about the vertical axis through the centroid, mm⁴.
    pub iy: Real,
    /// Elastic section modulus about x, mm³ (`ix` / farthest fibre).
    pub wx: Real,
    /// Elastic section modulus about y, mm³.
    pub wy: Real,
}

impl SectionProps {
    /// Radius of gyration about x, mm.
    pub fn rx(&self) -> Real {
        (self.ix / self.area).sqrt()
    }
    /// Radius of gyration about y, mm.
    pub fn ry(&self) -> Real {
        (self.iy / self.area).sqrt()
    }
}

impl Section {
    /// The designation the BOM shows: `<size>` (with the standard's family
    /// prefix where the size has none: `SHS 40x40x3`).
    pub fn stock_number(&self) -> String {
        let prefix = match self.shape {
            Shape::Shs => "SHS ",
            Shape::Rhs => "RHS ",
            Shape::Chs => "CHS ",
            Shape::Flat => "FL ",
            Shape::Bar => "RD ",
            Shape::Angle
            | Shape::IBeam
            | Shape::Channel
            | Shape::Tee
            | Shape::Custom
            | Shape::TSlot
            | Shape::ChannelTapered
            | Shape::IBeamTapered
            | Shape::ZedFormed
            | Shape::ChannelFormed => "",
            Shape::SquareBar => "SQ ",
            Shape::HexBar => "HEX ",
        };
        format!("{prefix}{}", self.size)
    }

    /// Width and height of the bounding box (section x, y).
    pub fn extent(&self) -> (Real, Real) {
        match self.shape {
            Shape::Chs | Shape::Bar | Shape::SquareBar => (self.h, self.h),
            Shape::Flat => (self.b, self.t),
            Shape::HexBar => (2.0 * self.h / 3f64.sqrt(), self.h),
            Shape::ZedFormed => (2.0 * self.b - self.t, self.h),
            _ => (self.b, self.h),
        }
    }

    /// The outline, bounding box centred on the origin.
    pub fn loops(&self) -> Vec<Loop> {
        let (w, h) = self.extent();
        let (x0, y0, x1, y1) = (-w / 2.0, -h / 2.0, w / 2.0, h / 2.0);
        let p = Point2::new;
        let rect = |x0: Real, y0: Real, x1: Real, y1: Real, r: Real, hole: bool| {
            let mut c = vec![
                (p(x0, y0), r),
                (p(x1, y0), r),
                (p(x1, y1), r),
                (p(x0, y1), r),
            ];
            if hole {
                c.reverse();
            }
            Loop::Corners(c)
        };
        match self.shape {
            Shape::Shs | Shape::Rhs => {
                let t = self.t;
                vec![
                    rect(x0, y0, x1, y1, self.r1, false),
                    rect(x0 + t, y0 + t, x1 - t, y1 - t, self.r2, true),
                ]
            }
            Shape::Chs => vec![
                Loop::Circle(Point2::origin(), self.h / 2.0, false),
                Loop::Circle(Point2::origin(), self.h / 2.0 - self.t, true),
            ],
            Shape::Bar => vec![Loop::Circle(Point2::origin(), self.h / 2.0, false)],
            Shape::Flat => vec![rect(x0, y0, x1, y1, 0.0, false)],
            Shape::Angle => {
                // Heel at the bottom left, legs along +x and +y.
                let t = self.t;
                vec![Loop::Corners(vec![
                    (p(x0, y0), 0.0),
                    (p(x1, y0), 0.0),
                    (p(x1, y0 + t), self.r2),
                    (p(x0 + t, y0 + t), self.r1),
                    (p(x0 + t, y1), self.r2),
                    (p(x0, y1), 0.0),
                ])]
            }
            Shape::Channel => {
                let (tw, tf) = (self.tw, self.tf);
                vec![Loop::Corners(vec![
                    (p(x0, y0), 0.0),
                    (p(x1, y0), 0.0),
                    (p(x1, y0 + tf), self.r2),
                    (p(x0 + tw, y0 + tf), self.r1),
                    (p(x0 + tw, y1 - tf), self.r1),
                    (p(x1, y1 - tf), self.r2),
                    (p(x1, y1), 0.0),
                    (p(x0, y1), 0.0),
                ])]
            }
            Shape::Tee => {
                let tw = self.tee_web() / 2.0;
                let tf = self.tee_flange();
                vec![Loop::Corners(vec![
                    (p(-tw, y0), 0.0),
                    (p(tw, y0), 0.0),
                    (p(tw, y1 - tf), self.r1),
                    (p(x1, y1 - tf), self.r2),
                    (p(x1, y1), 0.0),
                    (p(x0, y1), 0.0),
                    (p(x0, y1 - tf), self.r2),
                    (p(-tw, y1 - tf), self.r1),
                ])]
            }
            Shape::Custom => self.custom.clone(),
            Shape::TSlot => self.tslot_loops(),
            Shape::SquareBar => vec![rect(x0, y0, x1, y1, 0.0, false)],
            Shape::HexBar => {
                let r = self.h / 3f64.sqrt();
                vec![Loop::Corners(
                    (0..6)
                        .map(|k| {
                            let a = k as Real * std::f64::consts::FRAC_PI_3;
                            (p(r * a.cos(), r * a.sin()), 0.0)
                        })
                        .collect(),
                )]
            }
            Shape::ChannelFormed => {
                // Constant wall t, inner bends r1, outer r1 + t; lips tf.
                let (t, c, ri) = (self.t, self.tf, self.r1);
                let ro = ri + t;
                let mut cs = vec![(p(x0, y0), ro), (p(x1, y0), if c > t { ro } else { 0.0 })];
                if c > t {
                    cs.extend([
                        (p(x1, y0 + c), 0.0),
                        (p(x1 - t, y0 + c), 0.0),
                        (p(x1 - t, y0 + t), ri),
                    ]);
                } else {
                    cs.push((p(x1, y0 + t), 0.0));
                }
                cs.push((p(x0 + t, y0 + t), ri));
                cs.push((p(x0 + t, y1 - t), ri));
                if c > t {
                    cs.extend([
                        (p(x1 - t, y1 - t), ri),
                        (p(x1 - t, y1 - c), 0.0),
                        (p(x1, y1 - c), 0.0),
                        (p(x1, y1), ro),
                    ]);
                } else {
                    cs.extend([(p(x1, y1 - t), 0.0), (p(x1, y1), 0.0)]);
                }
                cs.push((p(x0, y1), ro));
                vec![Loop::Corners(cs)]
            }
            Shape::ZedFormed => {
                // Web x from xw0 to xw0 + t; top flange to the right of the
                // web's left face, bottom flange to the left of its right face.
                let (t, ri) = (self.t, self.r1);
                let ro = ri + t;
                let xw0 = x0 + self.b - t;
                let xw1 = xw0 + t;
                vec![Loop::Corners(vec![
                    (p(x0, y0), 0.0),
                    (p(xw1, y0), ro),
                    (p(xw1, y1 - t), ri),
                    (p(x1, y1 - t), 0.0),
                    (p(x1, y1), 0.0),
                    (p(xw0, y1), ro),
                    (p(xw0, y0 + t), ri),
                    (p(x0, y0 + t), 0.0),
                ])]
            }
            Shape::ChannelTapered => {
                // Flange thickness tf at the middle of the free flange, 8 %
                // thicker towards the web (EN 10365 / DIN 1026-1).
                let (tw, k) = (self.tw, 0.08);
                let free = self.b - tw;
                let t = |u: Real| self.tf + k * (u - free / 2.0);
                vec![Loop::Corners(vec![
                    (p(x0, y0), 0.0),
                    (p(x1, y0), 0.0),
                    (p(x1, y0 + t(0.0)), self.r2),
                    (p(x0 + tw, y0 + t(free)), self.r1),
                    (p(x0 + tw, y1 - t(free)), self.r1),
                    (p(x1, y1 - t(0.0)), self.r2),
                    (p(x1, y1), 0.0),
                    (p(x0, y1), 0.0),
                ])]
            }
            Shape::IBeamTapered => {
                // Flange thickness tf at b/4 from the flange's edge, 14 %
                // thicker towards the web (EN 10365 / DIN 1025-1).
                let (tw, k) = (self.tw / 2.0, 0.14);
                let free = self.b / 2.0 - tw;
                let t = |u: Real| self.tf + k * (u - self.b / 4.0);
                let (r1, r2) = (self.r1, self.r2);
                vec![Loop::Corners(vec![
                    (p(x0, y0), 0.0),
                    (p(x1, y0), 0.0),
                    (p(x1, y0 + t(0.0)), r2),
                    (p(tw, y0 + t(free)), r1),
                    (p(tw, y1 - t(free)), r1),
                    (p(x1, y1 - t(0.0)), r2),
                    (p(x1, y1), 0.0),
                    (p(x0, y1), 0.0),
                    (p(x0, y1 - t(0.0)), r2),
                    (p(-tw, y1 - t(free)), r1),
                    (p(-tw, y0 + t(free)), r1),
                    (p(x0, y0 + t(0.0)), r2),
                ])]
            }
            Shape::IBeam => {
                let (tw, tf, r) = (self.tw / 2.0, self.tf, self.r1);
                vec![Loop::Corners(vec![
                    (p(x0, y0), 0.0),
                    (p(x1, y0), 0.0),
                    (p(x1, y0 + tf), 0.0),
                    (p(tw, y0 + tf), r),
                    (p(tw, y1 - tf), r),
                    (p(x1, y1 - tf), 0.0),
                    (p(x1, y1), 0.0),
                    (p(x0, y1), 0.0),
                    (p(x0, y1 - tf), 0.0),
                    (p(-tw, y1 - tf), r),
                    (p(-tw, y0 + tf), r),
                    (p(x0, y0 + tf), 0.0),
                ])]
            }
        }
    }

    /// The outer outline as points (arcs sampled), section coordinates.
    pub fn outline(&self) -> Vec<Point2> {
        self.loops()
            .first()
            .map(|l| polygon(l, 96))
            .unwrap_or_default()
    }

    /// A point inside the material (to choose the profile region).
    pub fn material_point(&self) -> Point2 {
        let (w, _) = self.extent();
        match self.shape {
            Shape::Shs | Shape::Rhs => Point2::new(w / 2.0 - self.t / 2.0, 0.0),
            Shape::Chs => Point2::new(self.h / 2.0 - self.t / 2.0, 0.0),
            Shape::Angle => Point2::new(-w / 2.0 + self.t / 2.0, 0.0),
            Shape::IBeam => Point2::new(0.0, 0.0),
            Shape::Flat | Shape::Bar => Point2::origin(),
            Shape::Channel | Shape::ChannelTapered => Point2::new(-w / 2.0 + self.tw / 2.0, 0.0),
            Shape::IBeamTapered | Shape::SquareBar | Shape::HexBar => Point2::new(0.0, 0.0),
            Shape::ChannelFormed => Point2::new(-w / 2.0 + self.t / 2.0, 0.0),
            Shape::ZedFormed => Point2::new(-w / 2.0 + self.b - self.t / 2.0, 0.0),
            Shape::Tee => Point2::new(0.0, self.extent().1 / 2.0 - self.tee_flange() / 2.0),
            Shape::Custom => self.inside_point().unwrap_or_else(Point2::origin),
            Shape::TSlot => {
                let (w, h) = self.extent();
                let m = self.tslot_grid();
                if self.chamber > 0.0 {
                    // In the outer wall between the corner and the chamber.
                    let along = (m / 2.0 - self.chamber / 2.0) / 2.0;
                    Point2::new(-w / 2.0 + along, -h / 2.0 + self.walled_fit().wall / 2.0)
                } else {
                    let along = (m / 2.0 - self.tw / 2.0) / 2.0;
                    Point2::new(-w / 2.0 + along, -h / 2.0 + self.t / 2.0)
                }
            }
        }
    }

    /// A T-slot section's grid (the smaller side when the table gives none).
    fn tslot_grid(&self) -> Real {
        if self.grid > 0.0 {
            self.grid
        } else {
            self.h.min(self.b)
        }
    }

    /// Cells of a T-slot section along x and y.
    fn tslot_cells(&self) -> (usize, usize) {
        let m = self.tslot_grid();
        let n = |l: Real| ((l / m).round() as usize).max(1);
        (n(self.b), n(self.h))
    }

    /// The chamber and cavities of a T-slot section that give its published
    /// area: the chamber's width is scanned between its narrowest (floor as
    /// wide as the opening) and its widest (a web of [`TSLOT_WEB`] left at the
    /// corners), the cavities between cells take what is left, each up to the
    /// web it must keep. Without a published area the chamber is the middle
    /// of its range and there are no cavities.
    pub fn tslot_fit(&self) -> TSlotFit {
        use std::f64::consts::{PI, SQRT_2};
        let (m, w, t, dd, core) = (self.tslot_grid(), self.tw, self.t, self.tf, self.r1);
        let (nx, ny) = self.tslot_cells();
        let (nxr, nyr) = (nx as Real, ny as Real);
        let hh = (dd - t).max(0.0);
        let slots = 2.0 * (nxr + nyr);
        let lo = w + 2.0 * hh;
        let hi = (m - 2.0 * t - SQRT_2 * TSLOT_WEB).max(lo);
        if self.area <= 0.0 {
            return TSlotFit {
                chamber: (lo + hi) / 2.0,
                between: 0.0,
                node: 0.0,
            };
        }
        let between_n = (nxr - 1.0) * nyr + (nyr - 1.0) * nxr;
        let node_n = (nxr - 1.0) * (nyr - 1.0);
        let model = |c: Real| -> (Real, TSlotFit) {
            let solid = self.b * self.h
                - (4.0 - PI) * self.r2 * self.r2
                - slots * (w * t + (c - hh) * hh)
                - nxr * nyr * PI * core * core / 4.0;
            let mut left = solid - self.area;
            let (mut between, mut node) = (0.0, 0.0);
            if between_n > 0.0 {
                let max = (m - c / 2.0 - t - SQRT_2 * TSLOT_WEB)
                    .min(m / 2.0 - core / 2.0 - TSLOT_WEB)
                    .max(0.0);
                between = (left.max(0.0) / (2.0 * between_n)).sqrt().min(max);
                left -= 2.0 * between_n * between * between;
            }
            if node_n > 0.0 {
                let max = (m / 2.0 - between - TSLOT_WEB).max(0.0);
                node = (left.max(0.0) / (4.0 * node_n)).sqrt().min(max);
                left -= 4.0 * node_n * node * node;
            }
            (
                left.abs(),
                TSlotFit {
                    chamber: c,
                    between,
                    node,
                },
            )
        };
        let steps = 2000;
        (0..=steps)
            .map(|k| model(lo + (hi - lo) * k as Real / steps as Real))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map_or(
                TSlotFit {
                    chamber: lo,
                    between: 0.0,
                    node: 0.0,
                },
                |r| r.1,
            )
    }

    /// The thin-walled T-slot section (a published chamber, spec 6.1 §3.11a), drawn
    /// as these profiles are built: walls round the slots ending in hooks at the
    /// chambers, a solid block in each corner, diagonal webs from the corners to a
    /// core round the bore, every slot open down to the core. In each cell a face
    /// with a slot has its opening, lips (`t`), chamber (`chamber` wide, upright
    /// walls down to `undercut`), then the space under the walls between two webs,
    /// down to the core. A face between two cells has the same space closed under a
    /// plate two walls thick. The wall (= web) thickness, the corner blocks and the
    /// core are the ones that give the published area and second moments
    /// ([`Section::walled_fit`]). One outside loop, the bores and the closed spaces:
    /// a 90x180 is 8 bores and 20 spaces, no longer 72 cavities.
    fn walled_loops(&self) -> Vec<Loop> {
        self.walled_shape(self.walled_fit())
    }

    /// The loops of a walled section of the given wall, blocks and core.
    fn walled_shape(&self, fit: WalledFit) -> Vec<Loop> {
        let (x0, y0) = (-self.b / 2.0, -self.h / 2.0);
        let m = self.tslot_grid();
        let (nx, ny) = self.tslot_cells();
        let notch = self.walled_notch(fit, true);
        let closed = self.walled_notch(fit, false);
        let mut outer: Vec<(Point2, Real)> = Vec::new();
        let sides = [
            (Point2::new(x0, y0), Vector2::new(1.0, 0.0), nx),
            (Point2::new(-x0, y0), Vector2::new(0.0, 1.0), ny),
            (Point2::new(-x0, -y0), Vector2::new(-1.0, 0.0), nx),
            (Point2::new(x0, -y0), Vector2::new(0.0, -1.0), ny),
        ];
        for (start, d, cells) in sides {
            outer.push((start, self.r2));
            // Into the material: left of the direction of a CCW loop.
            let n = Vector2::new(-d.y, d.x);
            for k in 0..cells {
                let mid = start + d * (m * (k as Real + 0.5));
                for (a, z) in &notch {
                    outer.push((mid + d * *a + n * *z, 0.0));
                }
            }
        }
        let mut loops = vec![Loop::Corners(outer)];
        for j in 0..ny {
            for i in 0..nx {
                let c = Point2::new(x0 + m * (i as Real + 0.5), y0 + m * (j as Real + 0.5));
                loops.push(Loop::Circle(c, self.r1 / 2.0, true));
            }
        }
        // The closed spaces on both sides of each face between two cells: the
        // face's middle `mid`, `n` into the cell.
        let mut hole = |mid: Point2, n: Vector2| {
            let d = Vector2::new(n.y, -n.x);
            let mut pts: Vec<Point2> = closed.iter().map(|(a, z)| mid + d * *a + n * *z).collect();
            // Holes run clockwise.
            if polygon_area(&pts) > 0.0 {
                pts.reverse();
            }
            loops.push(Loop::Corners(pts.into_iter().map(|p| (p, 0.0)).collect()));
        };
        for j in 0..ny {
            for i in 1..nx {
                let mid = Point2::new(x0 + m * i as Real, y0 + m * (j as Real + 0.5));
                hole(mid, Vector2::new(1.0, 0.0));
                hole(mid, Vector2::new(-1.0, 0.0));
            }
        }
        for j in 1..ny {
            for i in 0..nx {
                let mid = Point2::new(x0 + m * (i as Real + 0.5), y0 + m * j as Real);
                hole(mid, Vector2::new(0.0, 1.0));
                hole(mid, Vector2::new(0.0, -1.0));
            }
        }
        loops
    }

    /// One face's slot as (along the face, depth into the material), from one end
    /// of the opening to the other (`open`); or the closed space under a face
    /// between two cells (`!open`, no corner blocks there).
    fn walled_notch(&self, fit: WalledFit, open: bool) -> Vec<(Real, Real)> {
        let m = self.tslot_grid();
        let (w, t, c, u) = (self.tw, self.t, self.chamber, self.undercut);
        let WalledFit {
            wall: k,
            block: s,
            core: q,
        } = fit;
        // A web's edge is `g` off its diagonal along a face.
        let g = k / std::f64::consts::SQRT_2;
        let mut half: Vec<(Real, Real)> = Vec::new();
        if open {
            half.extend([(w / 2.0, 0.0), (w / 2.0, t), (c / 2.0, t), (c / 2.0, u)]);
            if k < u {
                // The hook at the chamber's side.
                half.extend([(c / 2.0 + k, u), (c / 2.0 + k, k)]);
            }
        }
        if open && s - g > k {
            // The corner block reaches below the wall; the web leaves its side.
            half.extend([(m / 2.0 - s, k), (m / 2.0 - s, s - g)]);
        } else {
            half.push((m / 2.0 - k - g, k));
        }
        half.push((q - g, m / 2.0 - q));
        // In from the near end of the opening, across, and out at the far end.
        let mut out: Vec<(Real, Real)> = half.iter().map(|&(a, z)| (-a, z)).collect();
        out.extend(half.iter().rev());
        out
    }

    /// Area and second moments about the section's axes through its middle of a
    /// walled section's polygons, without sampling: the fit evaluates it often.
    fn walled_moments(&self, fit: WalledFit) -> (Real, Real, Real) {
        let (mut a, mut ixx, mut iyy) = (0.0, 0.0, 0.0);
        for l in self.walled_shape(fit) {
            match l {
                Loop::Circle(c, r, _) => {
                    let ar = std::f64::consts::PI * r * r;
                    a -= ar;
                    ixx -= ar * (r * r / 4.0 + c.y * c.y);
                    iyy -= ar * (r * r / 4.0 + c.x * c.x);
                }
                Loop::Corners(cs) => {
                    let n = cs.len();
                    for i in 0..n {
                        let (p, q) = (cs[i].0, cs[(i + 1) % n].0);
                        let k = p.x * q.y - q.x * p.y;
                        a += k / 2.0;
                        ixx += (p.y * p.y + p.y * q.y + q.y * q.y) * k / 12.0;
                        iyy += (p.x * p.x + p.x * q.x + q.x * q.x) * k / 12.0;
                    }
                }
                Loop::Path(_) => {}
            }
        }
        // The four rounded corners of the outside.
        let r = self.r2;
        let lost = (1.0 - std::f64::consts::FRAC_PI_4) * r * r * 4.0;
        let (cx, cy) = (self.b / 2.0 - 0.22 * r, self.h / 2.0 - 0.22 * r);
        (a - lost, ixx - lost * cy * cy, iyy - lost * cx * cx)
    }

    /// The wall, corner blocks and core of a walled section. For a wall and a core,
    /// the blocks that give the published area (bisection: the area grows with
    /// them); among walls from 0.8 mm to the lips' thickness and cores from the
    /// bore's wall to the chamber's, the pair whose second moments are nearest the
    /// published ones (the strong axis counts most), searched on a grid and then
    /// on a finer one round the best. Without published moments: walls as thick as
    /// the lips, the core in the middle of its range. Kept per section: the search
    /// takes some 10^4 evaluations of the outline.
    pub fn walled_fit(&self) -> WalledFit {
        type Cache = std::sync::Mutex<std::collections::HashMap<String, WalledFit>>;
        static FITS: std::sync::OnceLock<Cache> = std::sync::OnceLock::new();
        let key = format!(
            "{:?}",
            (
                self.h,
                self.b,
                self.t,
                self.tw,
                self.r1,
                self.r2,
                self.grid,
                self.area,
                self.chamber,
                self.undercut,
                self.ix_published,
                self.iy_published
            )
        );
        let fits = FITS.get_or_init(Default::default);
        if let Some(f) = fits.lock().ok().and_then(|f| f.get(&key).copied()) {
            return f;
        }
        let fit = self.walled_search();
        if let Ok(mut f) = fits.lock() {
            f.insert(key, fit);
        }
        fit
    }

    fn walled_search(&self) -> WalledFit {
        let m = self.tslot_grid();
        let (c, u, t) = (self.chamber, self.undercut, self.t);
        let (k_lo, k_hi) = (0.8, t.max(0.8));
        let (q_lo, q_hi) = (
            self.r1 / 2.0 + 1.5,
            (m / 2.0 - u - 1.5).max(self.r1 / 2.0 + 1.5),
        );
        let blocks = |k: Real, q: Real| -> WalledFit {
            let s_lo = k;
            let s_hi = (m / 2.0 - c / 2.0 - k - TSLOT_WEB).max(s_lo);
            let at = |s: Real| WalledFit {
                wall: k,
                block: s,
                core: q,
            };
            if self.area <= 0.0 {
                return at(s_lo);
            }
            let (mut lo, mut hi) = (s_lo, s_hi);
            for _ in 0..30 {
                let mid = (lo + hi) / 2.0;
                if self.walled_moments(at(mid)).0 < self.area {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            at((lo + hi) / 2.0)
        };
        let ((strong, weak), about_y) = if self.b > self.h {
            ((self.iy_published, self.ix_published), true)
        } else {
            ((self.ix_published, self.iy_published), false)
        };
        if strong <= 0.0 || self.area <= 0.0 {
            return blocks(k_hi, (q_lo + q_hi) / 2.0);
        }
        // The miss of a candidate; none where the area cannot be met.
        let miss = |f: WalledFit| -> Option<Real> {
            let (a, ix, iy) = self.walled_moments(f);
            if (a - self.area).abs() > 2e-3 * self.area {
                return None;
            }
            let (s, w) = if about_y { (iy, ix) } else { (ix, iy) };
            let mut e = (s / strong - 1.0).abs();
            if weak > 0.0 {
                e += 0.3 * (w / weak - 1.0).abs();
            }
            Some(e)
        };
        let mut best: Option<(Real, WalledFit)> = None;
        let search = |best: &mut Option<(Real, WalledFit)>,
                      (k0, k1): (Real, Real),
                      (q0, q1): (Real, Real),
                      n: usize| {
            for i in 0..=n {
                for j in 0..=n {
                    let k = k0 + (k1 - k0) * i as Real / n as Real;
                    let q = q0 + (q1 - q0) * j as Real / n as Real;
                    let f = blocks(k, q);
                    if let Some(e) = miss(f)
                        && best.is_none_or(|b| e < b.0)
                    {
                        *best = Some((e, f));
                    }
                }
            }
        };
        search(&mut best, (k_lo, k_hi), (q_lo, q_hi), 8);
        if let Some((_, f)) = best {
            let (dk, dq) = ((k_hi - k_lo) / 8.0, (q_hi - q_lo) / 8.0);
            search(
                &mut best,
                ((f.wall - dk).max(k_lo), (f.wall + dk).min(k_hi)),
                ((f.core - dq).max(q_lo), (f.core + dq).min(q_hi)),
                6,
            );
        }
        best.map_or_else(|| blocks(k_lo, q_lo), |b| b.1)
    }

    /// The outline of a T-slot section: the outside with a slot in the middle
    /// of every cell's face (opening, lips, a chamber whose walls run at 45°
    /// down to its floor), counter-clockwise; core bores and cavities as holes.
    fn tslot_loops(&self) -> Vec<Loop> {
        if self.chamber > 0.0 {
            return self.walled_loops();
        }
        let (x0, y0) = (-self.b / 2.0, -self.h / 2.0);
        let (m, w, t, dd) = (self.tslot_grid(), self.tw, self.t, self.tf);
        let (nx, ny) = self.tslot_cells();
        let fit = self.tslot_fit();
        let (c, hh) = (fit.chamber, (dd - t).max(0.0));
        let f = c - 2.0 * hh;
        // Along the side (in the direction of travel) and into the material.
        let notch = [
            (-w / 2.0, 0.0),
            (-w / 2.0, t),
            (-c / 2.0, t),
            (-f / 2.0, dd),
            (f / 2.0, dd),
            (c / 2.0, t),
            (w / 2.0, t),
            (w / 2.0, 0.0),
        ];
        let mut outer: Vec<(Point2, Real)> = Vec::new();
        let sides = [
            (Point2::new(x0, y0), Vector2::new(1.0, 0.0), nx),
            (Point2::new(-x0, y0), Vector2::new(0.0, 1.0), ny),
            (Point2::new(-x0, -y0), Vector2::new(-1.0, 0.0), nx),
            (Point2::new(x0, -y0), Vector2::new(0.0, -1.0), ny),
        ];
        for (start, d, cells) in sides {
            outer.push((start, self.r2));
            // Into the material: left of the direction of a CCW loop.
            let n = Vector2::new(-d.y, d.x);
            for k in 0..cells {
                let mid = start + d * (m * (k as Real + 0.5));
                for (a, z) in notch {
                    outer.push((mid + d * a + n * z, 0.0));
                }
            }
        }
        let mut loops = vec![Loop::Corners(outer)];
        let centre = |i: usize, j: usize| {
            Point2::new(x0 + m * (i as Real + 0.5), y0 + m * (j as Real + 0.5))
        };
        for j in 0..ny {
            for i in 0..nx {
                loops.push(Loop::Circle(centre(i, j), self.r1 / 2.0, true));
            }
        }
        // Holes run clockwise.
        let diamond = |q: Point2, r: Real| {
            Loop::Corners(vec![
                (Point2::new(q.x + r, q.y), 0.0),
                (Point2::new(q.x, q.y - r), 0.0),
                (Point2::new(q.x - r, q.y), 0.0),
                (Point2::new(q.x, q.y + r), 0.0),
            ])
        };
        if fit.between > 1e-3 {
            for j in 0..ny {
                for i in 1..nx {
                    loops.push(diamond(
                        centre(i, j) - Vector2::new(m / 2.0, 0.0),
                        fit.between,
                    ));
                }
            }
            for j in 1..ny {
                for i in 0..nx {
                    loops.push(diamond(
                        centre(i, j) - Vector2::new(0.0, m / 2.0),
                        fit.between,
                    ));
                }
            }
        }
        if fit.node > 1e-3 {
            let r = fit.node;
            for j in 1..ny {
                for i in 1..nx {
                    let q = Point2::new(x0 + m * i as Real, y0 + m * j as Real);
                    loops.push(Loop::Corners(vec![
                        (Point2::new(q.x + r, q.y + r), 0.0),
                        (Point2::new(q.x + r, q.y - r), 0.0),
                        (Point2::new(q.x - r, q.y - r), 0.0),
                        (Point2::new(q.x - r, q.y + r), 0.0),
                    ]));
                }
            }
        }
        loops
    }

    /// A T section's stem thickness (EN 10055 gives one `t` for both).
    fn tee_web(&self) -> Real {
        if self.tw > 0.0 { self.tw } else { self.t }
    }

    /// A T section's flange thickness.
    fn tee_flange(&self) -> Real {
        if self.tf > 0.0 { self.tf } else { self.t }
    }

    /// A point inside a custom outline's material (outside its holes): the
    /// first of a fine grid over the bounding box.
    fn inside_point(&self) -> Option<Point2> {
        let polys: Vec<Vec<Point2>> = self.custom.iter().map(|l| polygon(l, 360)).collect();
        let (w, h) = self.extent();
        let n = 64;
        for j in 0..n {
            for i in 0..n {
                let q = Point2::new(
                    -w / 2.0 + w * (i as Real + 0.5) / n as Real,
                    -h / 2.0 + h * (j as Real + 0.5) / n as Real,
                );
                // Inside an odd number of loops = in the material.
                let inside = polys.iter().filter(|p| contains(p, q)).count() % 2 == 1;
                let clear = polys.iter().all(|p| edge_distance(p, q) > 1e-3 * (w + h));
                if inside && clear {
                    return Some(q);
                }
            }
        }
        None
    }

    /// A section of the user's own outline (spec 6.1 §3.11): a closed
    /// region of a sketch (lines, arcs, circles; holes allowed), moved so
    /// its bounding box is centred on the origin.
    pub fn custom(
        standard: &str,
        family: &str,
        size: &str,
        region: &crate::sketch::Region,
    ) -> Result<Section, String> {
        use crate::sketch::ProfileCurve;
        let to_loop = |l: &crate::sketch::ProfileLoop, hole: bool| -> Result<Loop, String> {
            if let [e] = l.edges.as_slice()
                && let ProfileCurve::Arc { c, r, .. } = e.curve
            {
                return Ok(Loop::Circle(c, r, hole));
            }
            l.edges
                .iter()
                .map(|e| match &e.curve {
                    ProfileCurve::Line { a, .. } => Ok((*a, None)),
                    ProfileCurve::Arc { c, sweep, .. } => {
                        Ok((e.curve.point(0.0), Some((*c, *sweep > 0.0))))
                    }
                    ProfileCurve::Spline { .. } => {
                        Err("splines cannot be part of a frame profile yet".to_owned())
                    }
                })
                .collect::<Result<Vec<_>, String>>()
                .map(Loop::Path)
        };
        if family.trim().is_empty() || size.trim().is_empty() {
            return Err("give the profile a family and a size".into());
        }
        let mut loops = vec![to_loop(&region.outer, false)?];
        for h in &region.holes {
            loops.push(to_loop(h, true)?);
        }
        let pts = polygon(&loops[0], 360);
        let mut lo = Point2::new(Real::MAX, Real::MAX);
        let mut hi = Point2::new(Real::MIN, Real::MIN);
        for q in &pts {
            lo = Point2::new(lo.x.min(q.x), lo.y.min(q.y));
            hi = Point2::new(hi.x.max(q.x), hi.y.max(q.y));
        }
        let (b, h) = (hi.x - lo.x, hi.y - lo.y);
        if !(b > 1e-6 && h > 1e-6) {
            return Err("the region has no size".into());
        }
        let shift = Vector2::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
        let mv = |q: Point2| q - shift;
        let loops = loops
            .into_iter()
            .map(|l| match l {
                Loop::Circle(c, r, hole) => Loop::Circle(mv(c), r, hole),
                Loop::Corners(v) => Loop::Corners(v.into_iter().map(|(q, r)| (mv(q), r)).collect()),
                Loop::Path(v) => Loop::Path(
                    v.into_iter()
                        .map(|(q, a)| (mv(q), a.map(|(c, ccw)| (mv(c), ccw))))
                        .collect(),
                ),
            })
            .collect();
        Ok(Section {
            family: family.trim().to_owned(),
            standard: standard.trim().to_owned(),
            shape: Shape::Custom,
            size: size.trim().to_owned(),
            h,
            b,
            t: 0.0,
            tw: 0.0,
            tf: 0.0,
            r1: 0.0,
            r2: 0.0,
            custom: loops,
            grid: 0.0,
            area: 0.0,
            chamber: 0.0,
            undercut: 0.0,
            ix_published: 0.0,
            iy_published: 0.0,
            it_published: 0.0,
        })
    }

    /// The section properties: the outline's ([`Section::outline_properties`]),
    /// with the maker's published second moments where the table gives them (a
    /// T-slot catalogue's numbers are its real section's; ours is a model of it).
    pub fn properties(&self) -> SectionProps {
        let mut p = self.outline_properties();
        let (w, h) = self.extent();
        if self.ix_published > 0.0 {
            p.ix = self.ix_published;
            p.wx = p.ix / (h / 2.0 + p.centroid.y.abs());
        }
        if self.iy_published > 0.0 {
            p.iy = self.iy_published;
            p.wy = p.iy / (w / 2.0 + p.centroid.x.abs());
        }
        p
    }

    /// The section properties of the outline itself (arcs integrated exactly
    /// enough by fine sampling).
    pub fn outline_properties(&self) -> SectionProps {
        // Green's theorem over each loop's polygon (holes run clockwise, so
        // their terms subtract).
        let (mut a, mut sx, mut sy, mut sxx, mut syy) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for l in self.loops() {
            let pts = polygon(&l, 2000);
            for i in 0..pts.len() {
                let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
                let c = p.x * q.y - q.x * p.y;
                a += c / 2.0;
                sx += (p.x + q.x) * c / 6.0;
                sy += (p.y + q.y) * c / 6.0;
                // ∫ y² dA and ∫ x² dA.
                syy += (p.y * p.y + p.y * q.y + q.y * q.y) * c / 12.0;
                sxx += (p.x * p.x + p.x * q.x + q.x * q.x) * c / 12.0;
            }
        }
        let c = Point2::new(sx / a, sy / a);
        let ix = syy - a * c.y * c.y;
        let iy = sxx - a * c.x * c.x;
        let (w, h) = self.extent();
        let ymax = (h / 2.0 - c.y).max(h / 2.0 + c.y);
        let xmax = (w / 2.0 - c.x).max(w / 2.0 + c.x);
        SectionProps {
            area: a,
            centroid: c,
            ix,
            iy,
            wx: ix / ymax,
            wy: iy / xmax,
        }
    }

    /// Torsion constant (mm⁴), torsional modulus (mm³) and the shear areas
    /// along the section's horizontal and vertical axes (mm²): the maker's
    /// published constant where the table gives one, else EN 10219's
    /// formulas for hollow sections, thin-wall sums for open ones,
    /// Saint-Venant's approximation for a solid outline of ours.
    pub fn torsion(&self) -> (Real, Real, Real, Real) {
        use std::f64::consts::PI;
        let p = self.properties();
        let a = p.area;
        let (h, b, t, tw, tf) = (self.h, self.b, self.t, self.tw, self.tf);
        let solid = |a: Real, ip: Real, r: Real| {
            let j = a.powi(4) / (40.0 * ip);
            (j, j / r, 5.0 / 6.0 * a, 5.0 / 6.0 * a)
        };
        let ip = p.ix + p.iy;
        if self.it_published > 0.0 {
            let (w, d) = self.extent();
            let j = self.it_published;
            return (j, j / (w.max(d) / 2.0), 5.0 / 6.0 * a, 5.0 / 6.0 * a);
        }
        match self.shape {
            Shape::Shs | Shape::Rhs => {
                let rc = (self.r1 + self.r2) / 2.0;
                let hp = 2.0 * ((b - t) + (h - t)) - 2.0 * rc * (4.0 - PI);
                let ah = (b - t) * (h - t) - rc * rc * (4.0 - PI);
                let k = 2.0 * ah * t / hp;
                let j = t.powi(3) * hp / 3.0 + 2.0 * k * ah;
                (j, j / (t + k / t), a * b / (b + h), a * h / (b + h))
            }
            Shape::Chs => {
                let d = h - 2.0 * t;
                let j = PI / 32.0 * (h.powi(4) - d.powi(4));
                (j, 2.0 * j / h, 2.0 * a / PI, 2.0 * a / PI)
            }
            Shape::Bar => (
                PI * h.powi(4) / 32.0,
                PI * h.powi(3) / 16.0,
                0.9 * a,
                0.9 * a,
            ),
            Shape::Flat | Shape::SquareBar => {
                let (w, d) = self.extent();
                let (l, s) = (w.max(d), w.min(d));
                let j =
                    l * s.powi(3) * (1.0 / 3.0 - 0.21 * (s / l) * (1.0 - (s / l).powi(4) / 12.0));
                (
                    j,
                    l * l * s * s / (3.0 * l + 1.8 * s),
                    5.0 / 6.0 * a,
                    5.0 / 6.0 * a,
                )
            }
            Shape::HexBar => solid(a, ip, h / 2.0),
            // A T-slot profile without a published constant: the outline's
            // Saint-Venant value, like a custom section (high for its open slots).
            Shape::Custom | Shape::TSlot => {
                let (w, d) = self.extent();
                solid(a, ip, w.max(d) / 2.0)
            }
            Shape::IBeam | Shape::IBeamTapered => {
                let j = (2.0 * b * tf.powi(3) + (h - 2.0 * tf) * tw.powi(3)) / 3.0;
                (j, j / tf.max(tw), 2.0 * b * tf, tw * (h - 2.0 * tf))
            }
            Shape::Channel | Shape::ChannelTapered => {
                let j = (2.0 * b * tf.powi(3) + (h - 2.0 * tf) * tw.powi(3)) / 3.0;
                (j, j / tf.max(tw), 2.0 * b * tf, tw * (h - 2.0 * tf))
            }
            Shape::Tee => {
                let j = (b * tf.powi(3) + (h - tf) * tw.powi(3)) / 3.0;
                (j, j / tf.max(tw), b * tf, tw * h)
            }
            Shape::Angle => {
                let j = (h + b - t) * t.powi(3) / 3.0;
                (j, j / t, b * t, h * t)
            }
            Shape::ZedFormed => {
                let j = (h + 2.0 * b) * t.powi(3) / 3.0;
                (j, j / t, 2.0 * b * t, h * t)
            }
            Shape::ChannelFormed => {
                let j = (h + 2.0 * b + 2.0 * tf) * t.powi(3) / 3.0;
                (j, j / t, 2.0 * b * t, h * t)
            }
        }
    }

    /// Draws the outline into `sk` (sketch coordinates = section
    /// coordinates). Corners with a radius become tangent arcs; points are
    /// shared along each loop.
    pub fn draw(&self, sk: &mut Sketch) -> Result<(), PartError> {
        for l in self.loops() {
            draw_loop(sk, l)?;
        }
        Ok(())
    }

    /// The outer loop of the outline grown by `by` (≥ 0) on every side:
    /// Notch's Basic Profile. `None` for a user's outline with arcs and a
    /// positive offset (not yet).
    pub fn notch_outline(&self, by: Real) -> Option<Loop> {
        let outer = self.loops().into_iter().next()?;
        if by.abs() < 1e-12 {
            return Some(outer);
        }
        match outer {
            Loop::Circle(c, r, h) => Some(Loop::Circle(c, r + by, h)),
            Loop::Corners(cs) => Some(Loop::Corners(grow_corners(&cs, by))),
            Loop::Path(v) if v.iter().all(|(_, a)| a.is_none()) => {
                let cs: Vec<(Point2, Real)> = v.iter().map(|(q, _)| (*q, 0.0)).collect();
                Some(Loop::Corners(grow_corners(&cs, by)))
            }
            Loop::Path(_) => None,
        }
    }
}

/// The signed area of a polygon (counter-clockwise positive).
fn polygon_area(p: &[Point2]) -> Real {
    let n = p.len();
    (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<Real>()
        / 2.0
}

/// Draws one loop into `sk` (sketch coordinates = the loop's).
pub fn draw_loop(sk: &mut Sketch, l: Loop) -> Result<(), PartError> {
    let err = |e: crate::sketch::SketchError| PartError::Feature(e.to_string());
    {
        {
            match l {
                Loop::Circle(c, r, _) => {
                    let _ = sk.add_circle(c, r).map_err(err)?;
                }
                Loop::Path(v) => {
                    let ids: Vec<_> = v.iter().map(|(q, _)| sk.add_point(*q)).collect();
                    let n = v.len();
                    for i in 0..n {
                        let (a, b) = (ids[i], ids[(i + 1) % n]);
                        match v[i].1 {
                            Some((c, ccw)) => {
                                let c = sk.add_point(c);
                                if ccw {
                                    sk.add_arc(c, a, b).map_err(err)?;
                                } else {
                                    sk.add_arc(c, b, a).map_err(err)?;
                                }
                            }
                            None => {
                                sk.add_line_between(a, b).map_err(err)?;
                            }
                        }
                    }
                }
                Loop::Corners(corners) => {
                    // Each corner: where it starts and ends (tangent points),
                    // and its arc centre when rounded.
                    let n = corners.len();
                    let mut ends = Vec::with_capacity(n);
                    for i in 0..n {
                        let (prev, (p, r), next) = (
                            corners[(i + n - 1) % n].0,
                            corners[i],
                            corners[(i + 1) % n].0,
                        );
                        ends.push(fillet(prev, p, next, r));
                    }
                    let ids: Vec<_> = ends
                        .iter()
                        .map(|e| match e {
                            Corner::Sharp(p) => {
                                let id = sk.add_point(*p);
                                (id, id, None)
                            }
                            Corner::Round { a, b, c, ccw } => {
                                let (ia, ib, ic) =
                                    (sk.add_point(*a), sk.add_point(*b), sk.add_point(*c));
                                (ia, ib, Some((ic, *ccw)))
                            }
                        })
                        .collect();
                    for i in 0..n {
                        let (a, b, arc) = ids[i];
                        if let Some((c, ccw)) = arc {
                            if ccw {
                                sk.add_arc(c, a, b).map_err(err)?;
                            } else {
                                sk.add_arc(c, b, a).map_err(err)?;
                            }
                        }
                        let next = ids[(i + 1) % n].0;
                        sk.add_line_between(b, next).map_err(err)?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// A counter-clockwise loop of filleted corners offset outwards by `by`:
/// every edge moves out, convex corners round by `r + by`, concave ones by
/// `r - by` (sharp below 0).
fn grow_corners(cs: &[(Point2, Real)], by: Real) -> Vec<(Point2, Real)> {
    let n = cs.len();
    (0..n)
        .map(|i| {
            let (prev, (p, r), next) = (cs[(i + n - 1) % n].0, cs[i], cs[(i + 1) % n].0);
            let (din, dout) = ((p - prev).normalize(), (next - p).normalize());
            // Outward normals of a CCW loop: to the right of the direction.
            let (nin, nout) = (Vector2::new(din.y, -din.x), Vector2::new(dout.y, -dout.x));
            let convex = din.perp(&dout) > 0.0;
            // The corner of the two moved lines: p + by·(nin + nout)/(1 + cos).
            let cos = nin.dot(&nout);
            let q = if cos > -0.999 {
                p + (nin + nout) * (by / (1.0 + cos))
            } else {
                p + nin * by
            };
            let r = if convex { r + by } else { (r - by).max(0.0) };
            (q, r)
        })
        .collect()
}

impl Section {
    /// A member part: this section extruded by `length` along +Z from its
    /// XY plane, with the member's Properties (Stock Number, Description,
    /// `G_L`) and `material` when the library knows it.
    pub fn member_part(
        &self,
        length: Real,
        material: Option<&str>,
    ) -> Result<PartDocument, PartError> {
        self.member_part_cut(length, [0.0, 0.0], &[], &[], material)
    }

    /// A member part with end treatments: the section extruded from
    /// `-ext[0]` to `length + ext[1]` along +Z (past the skeleton's ends,
    /// so a cut has material to take), then each plane of `cuts` trims what
    /// lies on its normal's side (a work plane + Trim Solid per cut, so the
    /// part keeps its cuts without the frame — rule 8).
    pub fn member_part_cut(
        &self,
        length: Real,
        ext: [Real; 2],
        cuts: &[EndCut],
        notches: &[NotchTool],
        material: Option<&str>,
    ) -> Result<PartDocument, PartError> {
        if !(length.is_finite() && length > 1e-6) {
            return Err(PartError::Feature("the member has no length".into()));
        }
        let mut part = PartDocument {
            exact_only: true,
            ..Default::default()
        };
        let opts = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        // Cuts square to the member are its extrusion's ends (no Trim Solid:
        // each was ~0.4 s in the kernel for a rounded tube).
        let (mut z0, mut z1) = (-ext[0], length + ext[1]);
        let mut slanted = Vec::new();
        for c in cuts {
            let n = c.normal.try_normalize(1e-12);
            match n {
                Some(n) if n.x.abs() < 1e-9 && n.y.abs() < 1e-9 => {
                    if n.z > 0.0 {
                        z1 = z1.min(c.point.z);
                    } else {
                        z0 = z0.max(c.point.z);
                    }
                }
                _ => slanted.push(*c),
            }
        }
        if z1 - z0 < 1e-6 {
            return Err(PartError::Feature(
                "the cuts leave nothing of the member".into(),
            ));
        }
        // Slanted ends (a mitre): the section between its two end planes in
        // one kernel call, no Trim Solid. Which end a plane cuts follows from
        // the side it keeps, never from where it lies: a start cut past the
        // member's middle taken for an end cut left the short piece as well.
        let plane = |c: &EndCut| {
            [
                [c.point.x, c.point.y, c.point.z],
                [c.normal.x, c.normal.y, c.normal.z],
            ]
        };
        let (starts, ends): (Vec<&EndCut>, Vec<&EndCut>) =
            slanted.iter().partition(|c| c.normal.z < 0.0);
        if !slanted.is_empty() && starts.len() <= 1 && ends.len() <= 1 {
            let square = |z: Real| [[0.0, 0.0, z], [0.0, 0.0, 1.0]];
            let op = PrismOp {
                loops: self.loops(),
                start: starts.first().map_or(square(z0), |c| plane(c)),
                end: ends.first().map_or(square(z1), |c| plane(c)),
            };
            part.add_body_op(crate::part::BodyOpFeature::new(crate::part::BodyOp::Prism(
                op,
            )))?;
            self.add_notches(&mut part, notches, &opts)?;
            if let Some(m) = material {
                let _ = part.set_material(Some(m), crate::materials::with_user());
            }
            return Ok(part);
        }
        let start_inside = z0 > 1e-9 || z1 < 1e-9;
        let (i, seed) = if start_inside {
            // The section on a plane at z0 (points mapped through 3D, so the
            // part's coordinates stay the section's).
            let w = part.add_work(hidden(WorkDef::Plane(
                crate::part::work::WorkPlaneDef::Offset {
                    base: PlaneRef::Origin(OriginPlane::XY),
                    distance: z0,
                },
            )))?;
            let placement = SketchPlacement {
                plane: PlaneRef::Work(w),
                ..Default::default()
            };
            let i = part.create_sketch(placement, &opts)?;
            let pl = part.sketches[i].sketch.plane.clone();
            for l in self.loops() {
                let t = NotchTool {
                    origin: [0.0, 0.0, z0],
                    x: [1.0, 0.0, 0.0],
                    y: [0.0, 1.0, 0.0],
                    length: 0.0,
                    outline: l,
                };
                draw_loop(&mut part.sketches[i].sketch, t.outline_in(&pl))?;
            }
            let m = self.material_point();
            let q = pl.project(&Point3::new(m.x, m.y, z0));
            (i, Point2::new(q.x, q.y))
        } else {
            let i = part.create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)?;
            self.draw(&mut part.sketches[i].sketch)?;
            (i, self.material_point())
        };
        let regions = part.sketches[i].sketch.regions();
        let r = regions
            .iter()
            .find(|r| r.contains(seed))
            .ok_or_else(|| PartError::Feature("the section has no closed profile".into()))?;
        let (dist, back) = if start_inside {
            (z1 - z0, 0.0)
        } else {
            (z1, -z0)
        };
        let mut e = ExtrudeFeature::new(vec![ProfileRef::of(r, seed)], dist);
        if start_inside && part.sketches[i].sketch.plane.normal.z < 0.0 {
            e.direction = crate::part::ExtentDirection::Flipped;
        }
        if back > 1e-9 {
            e.direction = crate::part::ExtentDirection::Asymmetric;
            e.distance_b = back;
        }
        part.add_extrude(i, e)?;
        for c in &slanted {
            let n = c
                .normal
                .try_normalize(1e-12)
                .ok_or_else(|| PartError::Feature("an end cut has no direction".into()))?;
            // A UCS whose XY plane is the cut (its Z = the normal: Ry·Rx·ẑ).
            let w = part.add_work(hidden(WorkDef::Ucs(UcsDef::Offsets {
                x: c.point.x,
                y: c.point.y,
                z: c.point.z,
                rx: (-n.y).clamp(-1.0, 1.0).asin(),
                ry: n.x.atan2(n.z),
                rz: 0.0,
            })))?;
            let mut sp = SplitFeature::new(SplitMethod::TrimSolid);
            sp.tool = Some(SplitToolRef::Plane(PlaneRef::Work(format!("{w}:XY"))));
            part.add_split(sp)?;
        }
        self.add_notches(&mut part, notches, &opts)?;
        if let Some(m) = material {
            let _ = part.set_material(Some(m), crate::materials::with_user());
        }
        Ok(part)
    }

    /// A curved member's part (spec 6.1 Merge Frame Members, arcs and
    /// splines): this section on the XY plane swept along `path`, which
    /// starts in that plane going +Z and lies in one plane. The path is
    /// copied into the part as a sketch (rule 8).
    pub fn member_part_path(
        &self,
        path: &[MemberPiece],
        material: Option<&str>,
    ) -> Result<PartDocument, PartError> {
        let (origin, n) = path_plane(path).ok_or_else(|| {
            PartError::Feature("the curves of a member must lie in one plane".into())
        })?;
        self.fits_bends(path, n)?;
        let mut part = PartDocument {
            exact_only: true,
            ..Default::default()
        };
        let opts = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let i = part.create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts)?;
        self.draw(&mut part.sketches[i].sketch)?;
        let w = part.add_work(hidden(WorkDef::Ucs(UcsDef::Offsets {
            x: origin.x,
            y: origin.y,
            z: origin.z,
            rx: (-n.y).clamp(-1.0, 1.0).asin(),
            ry: n.x.atan2(n.z),
            rz: 0.0,
        })))?;
        let placement = SketchPlacement {
            plane: PlaneRef::Work(format!("{w}:XY")),
            ..Default::default()
        };
        let k = part.create_sketch(placement, &opts)?;
        let bad = |e: crate::sketch::SketchError| PartError::Feature(e.to_string());
        let curves = {
            let sk = &mut part.sketches[k].sketch;
            let pl = sk.plane.clone();
            let flat = |p: &[Real; 3]| {
                let q = pl.project(&Point3::new(p[0], p[1], p[2]));
                Point2::new(q.x, q.y)
            };
            let mut curves = Vec::new();
            let mut last = None;
            for piece in path {
                let from = match last {
                    Some(id) => id,
                    None => sk.add_point(flat(&piece.first())),
                };
                match piece {
                    MemberPiece::Line { b, .. } => {
                        let to = sk.add_point(flat(b));
                        curves.push(sk.add_line_between(from, to).map_err(bad)?);
                        last = Some(to);
                    }
                    MemberPiece::Arc {
                        center,
                        axis,
                        angle,
                        ..
                    } => {
                        let to = sk.add_point(flat(&piece.last()));
                        let c = sk.add_point(flat(center));
                        let up = Vector3::new(axis[0], axis[1], axis[2]).dot(&pl.normal);
                        curves.push(if up * angle > 0.0 {
                            sk.add_arc(c, from, to).map_err(bad)?
                        } else {
                            sk.add_arc(c, to, from).map_err(bad)?
                        });
                        last = Some(to);
                    }
                    MemberPiece::Points(v) => {
                        let mut ids = vec![from];
                        let step = (v.len().saturating_sub(1) / SPLINE_POINTS).max(1);
                        for (j, p) in v.iter().enumerate().skip(1) {
                            if j % step == 0 || j + 1 == v.len() {
                                ids.push(sk.add_point(flat(p)));
                            }
                        }
                        last = ids.last().copied();
                        curves.push(sk.add_interp_spline(ids).map_err(bad)?);
                    }
                }
            }
            curves
        };
        let seed = self.material_point();
        let regions = part.sketches[i].sketch.regions();
        let r = regions
            .iter()
            .find(|r| r.contains(seed))
            .ok_or_else(|| PartError::Feature("the section has no closed profile".into()))?;
        let def = crate::part::sweep::SweepFeature::new(
            vec![ProfileRef::of(r, seed)],
            crate::part::sweep::PathRef::Sketch {
                sketch: part.sketches[k].id.clone(),
                curves,
            },
        );
        part.add_sweep(i, def)?;
        part.sketches[i].visible = false;
        part.sketches[k].visible = false;
        if let Some(m) = material {
            let _ = part.set_material(Some(m), crate::materials::with_user());
        }
        Ok(part)
    }

    /// Refuses a section reaching past the centre of an arc of `path`
    /// (plane normal `n`; the section stands square to the path at its
    /// start, which goes +Z): the sweep would fold through itself.
    fn fits_bends(&self, path: &[MemberPiece], n: Vector3) -> Result<(), PartError> {
        let start = path.first().map_or([0.0; 3], MemberPiece::first);
        let left = n.cross(&Vector3::z());
        let (mut lo, mut hi) = (Real::INFINITY, Real::NEG_INFINITY);
        for v in self.outline() {
            let w = (v.x - start[0]) * left.x + (v.y - start[1]) * left.y;
            lo = lo.min(w);
            hi = hi.max(w);
        }
        for p in path {
            if let MemberPiece::Arc {
                a,
                center,
                axis,
                angle,
            } = p
            {
                let r = (0..3)
                    .map(|k| (a[k] - center[k]).powi(2))
                    .sum::<Real>()
                    .sqrt();
                let up = axis[0] * n.x + axis[1] * n.y + axis[2] * n.z;
                let reach = if up * angle > 0.0 { hi } else { -lo };
                if reach >= r - 1e-6 {
                    return Err(PartError::Feature(format!(
                        "the section does not fit a bend of radius {r:.1} mm"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Notch cuts of a member part: each prism a sketch on a work UCS and an
    /// Extrude Cut.
    fn add_notches(
        &self,
        part: &mut PartDocument,
        notches: &[NotchTool],
        opts: &SketchOptions,
    ) -> Result<(), PartError> {
        for t in notches {
            let v = |a: &[Real; 3]| Vector3::new(a[0], a[1], a[2]);
            let n = v(&t.x)
                .cross(&v(&t.y))
                .try_normalize(1e-12)
                .ok_or_else(|| PartError::Feature("a notch has no direction".into()))?;
            let w = part.add_work(hidden(WorkDef::Ucs(UcsDef::Offsets {
                x: t.origin[0],
                y: t.origin[1],
                z: t.origin[2],
                rx: (-n.y).clamp(-1.0, 1.0).asin(),
                ry: n.x.atan2(n.z),
                rz: 0.0,
            })))?;
            let placement = SketchPlacement {
                plane: PlaneRef::Work(format!("{w}:XY")),
                ..Default::default()
            };
            let k = part.create_sketch(placement, opts)?;
            let pl = part.sketches[k].sketch.plane.clone();
            draw_loop(&mut part.sketches[k].sketch, t.outline_in(&pl))?;
            let regions = part.sketches[k].sketch.regions();
            let r = regions
                .first()
                .ok_or_else(|| PartError::Feature("the notch outline is not closed".into()))?;
            let seed = r.interior_point();
            let mut e = ExtrudeFeature::new(vec![ProfileRef::of(r, seed)], t.length);
            e.operation = crate::part::Operation::Cut;
            if n.dot(&pl.normal) < 0.0 {
                e.direction = crate::part::ExtentDirection::Flipped;
            }
            part.add_extrude(k, e)?;
        }
        Ok(())
    }

    /// The Properties a member of this section and `length` carries.
    pub fn member_properties(&self, length: Real) -> IProperties {
        let mut p = IProperties::default();
        p.set("stock_number", &self.stock_number());
        p.set("description", &format!("{} {}", self.family, self.size));
        p.custom.push(CustomProperty {
            name: LENGTH_PROPERTY.into(),
            value: PropValue::Number(stored_length(length)),
        });
        p
    }
}

/// How the corners of an end cap's plate are finished.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapCorners {
    /// Rounded like the section's.
    #[default]
    Filleted,
    /// A straight cut across each rounded corner.
    Chamfered,
    /// Sharp.
    Sharp,
}

impl Section {
    /// The section of an end cap's plate of this member section: its outline
    /// (`internal`: the inside of a hollow section), `inset` smaller all
    /// round. On a failure the reason, for the panel's status.
    pub fn cap_section(
        &self,
        internal: bool,
        corners: CapCorners,
        inset: Real,
    ) -> Result<Section, String> {
        if !(inset.is_finite() && inset >= 0.0) {
            return Err("the edge offset must be zero or more".into());
        }
        let loops = self.loops();
        let source = if internal {
            loops
                .get(1)
                .ok_or("an Internal cap needs a hollow section")?
        } else {
            loops.first().ok_or("the section has no outline")?
        };
        let outline = match source {
            Loop::Circle(c, r, _) => {
                if *r - inset <= 1e-6 {
                    return Err("the edge offset leaves no plate".into());
                }
                Loop::Circle(*c, *r - inset, false)
            }
            Loop::Corners(v) => {
                let mut v = v.clone();
                // A hole runs clockwise; a plate's outline counter-clockwise.
                if internal {
                    v.reverse();
                }
                Loop::Corners(cap_corners(&v, corners, inset)?)
            }
            Loop::Path(_) if inset > 1e-9 => {
                return Err("a custom section's cap takes an Edge Offset of 0".into());
            }
            Loop::Path(_) => source.clone(),
        };
        let mut s = self.clone();
        s.shape = Shape::Custom;
        s.custom = vec![outline];
        Ok(s)
    }
}

/// The corners of a counter-clockwise outline moved `inset` inwards, each
/// rounded, cut or sharp as asked.
fn cap_corners(
    v: &[(Point2, Real)],
    corners: CapCorners,
    inset: Real,
) -> Result<Vec<(Point2, Real)>, String> {
    let n = v.len();
    if n < 3 {
        return Err("the section has no outline".into());
    }
    let mut out = Vec::with_capacity(n * 2);
    for k in 0..n {
        let (a, (p, r), b) = (v[(k + n - 1) % n].0, v[k], v[(k + 1) % n].0);
        let (Some(u), Some(w)) = ((p - a).try_normalize(1e-9), (b - p).try_normalize(1e-9)) else {
            return Err("the section's outline has a zero-length side".into());
        };
        // Inward normals of the two sides (left of a counter-clockwise run).
        let (nu, nw) = (Vector2::new(-u.y, u.x), Vector2::new(-w.y, w.x));
        let denom = 1.0 + nu.dot(&nw);
        if denom < 1e-6 {
            return Err("the section's outline folds back on itself".into());
        }
        let q = p + (nu + nw) * (inset / denom);
        let convex = u.perp(&w) > 0.0;
        let left = (r - inset).max(0.0);
        match corners {
            CapCorners::Filleted => out.push((q, if convex { left } else { r + inset })),
            CapCorners::Sharp => out.push((q, 0.0)),
            CapCorners::Chamfered if convex && left > 1e-9 => {
                // The tangent points of the arc that was there.
                let half = (1.0 - u.dot(&w)).max(0.0).sqrt() / (1.0 + u.dot(&w)).max(1e-9).sqrt();
                let d = left * half;
                out.push((q - u * d, 0.0));
                out.push((q + w * d, 0.0));
            }
            CapCorners::Chamfered => out.push((q, 0.0)),
        }
    }
    // Each side must still run its old way: a larger inset turns it round.
    let shrunk = |k: usize| out[k].0 - out[(k + out.len() - 1) % out.len()].0;
    let flipped = match corners {
        // The chamfer's extra corners do not line up with the old sides.
        CapCorners::Chamfered => false,
        _ => (0..n).any(|k| {
            let old = v[k].0 - v[(k + n - 1) % n].0;
            shrunk(k).dot(&old) <= 1e-9
        }),
    };
    if flipped || signed_area(&out) <= 1e-9 {
        return Err("the edge offset leaves no plate".into());
    }
    Ok(out)
}

/// Twice the signed area of a corner list, over 2.
fn signed_area(v: &[(Point2, Real)]) -> Real {
    let n = v.len();
    (0..n)
        .map(|k| {
            let (p, q) = (v[k].0, v[(k + 1) % n].0);
            p.x * q.y - q.x * p.y
        })
        .sum::<Real>()
        / 2.0
}

/// A work feature of a member part, not shown (the part's construction).
pub(crate) fn hidden(def: WorkDef) -> WorkFeature {
    let mut w = WorkFeature::new(def);
    w.visible = false;
    w
}

/// One piece of a curved member's path, in the member part's coordinates
/// (kept in `assembly::frame::Shape` to tell when the part must be rebuilt).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MemberPiece {
    /// Straight from `a` to `b`.
    Line {
        /// Start.
        a: [Real; 3],
        /// End.
        b: [Real; 3],
    },
    /// From `a` about the axis through `center` (right hand) by `angle`.
    Arc {
        /// Start.
        a: [Real; 3],
        /// Centre.
        center: [Real; 3],
        /// Unit axis.
        axis: [Real; 3],
        /// Signed angle (radians).
        angle: Real,
    },
    /// A free-form curve by its points.
    Points(Vec<[Real; 3]>),
}

/// How many spans a free-form path piece keeps in the member's sketch.
const SPLINE_POINTS: usize = 32;

impl MemberPiece {
    /// Its first point.
    pub fn first(&self) -> [Real; 3] {
        match self {
            MemberPiece::Line { a, .. } | MemberPiece::Arc { a, .. } => *a,
            MemberPiece::Points(v) => v.first().copied().unwrap_or([0.0; 3]),
        }
    }

    /// Its last point.
    pub fn last(&self) -> [Real; 3] {
        match self {
            MemberPiece::Line { b, .. } => *b,
            MemberPiece::Arc {
                a,
                center,
                axis,
                angle,
            } => {
                let c = Point3::new(center[0], center[1], center[2]);
                let k = nalgebra::Unit::new_normalize(Vector3::new(axis[0], axis[1], axis[2]));
                let q = c + nalgebra::Rotation3::from_axis_angle(&k, *angle)
                    * (Point3::new(a[0], a[1], a[2]) - c);
                [q.x, q.y, q.z]
            }
            MemberPiece::Points(v) => v.last().copied().unwrap_or([0.0; 3]),
        }
    }

    /// Its numbers, to compare two paths.
    pub fn numbers(&self) -> Vec<Real> {
        match self {
            MemberPiece::Line { a, b } => a.iter().chain(b).copied().collect(),
            MemberPiece::Arc {
                a,
                center,
                axis,
                angle,
            } => a
                .iter()
                .chain(center)
                .chain(axis)
                .copied()
                .chain([*angle])
                .collect(),
            MemberPiece::Points(v) => v.iter().flatten().copied().collect(),
        }
    }
}

/// A point and the unit normal of the plane `path` lies in, or `None` when
/// it leaves every plane. A path along one line takes any plane through it.
pub fn path_plane(path: &[MemberPiece]) -> Option<(Point3, Vector3)> {
    let p3 = |p: [Real; 3]| Point3::new(p[0], p[1], p[2]);
    let mut pts = Vec::new();
    let mut normal = None;
    for piece in path {
        pts.push(p3(piece.first()));
        pts.push(p3(piece.last()));
        match piece {
            MemberPiece::Arc { axis, center, .. } => {
                normal = normal.or(Some(Vector3::new(axis[0], axis[1], axis[2]).normalize()));
                pts.push(p3(*center));
            }
            MemberPiece::Points(v) => pts.extend(v.iter().map(|p| p3(*p))),
            MemberPiece::Line { .. } => {}
        }
    }
    let o = *pts.first()?;
    let size = pts.iter().map(|p| (p - o).norm()).fold(0.0, Real::max);
    let n = match normal {
        Some(n) => n,
        None => {
            // The widest triangle from the start and its farthest point.
            let far = *pts
                .iter()
                .max_by(|a, b| (*a - o).norm().total_cmp(&(*b - o).norm()))?;
            let d = far - o;
            let best = pts
                .iter()
                .map(|p| d.cross(&(p - o)))
                .max_by(|a, b| a.norm().total_cmp(&b.norm()))?;
            match best.try_normalize(1e-9 * size.max(1.0).powi(2)) {
                Some(n) => n,
                None => {
                    let d = d.try_normalize(1e-12)?;
                    let other = if d.x.abs() < 0.9 {
                        Vector3::x()
                    } else {
                        Vector3::y()
                    };
                    d.cross(&other).normalize()
                }
            }
        }
    };
    pts.iter()
        .all(|p| (p - o).dot(&n).abs() <= 1e-5 * size.max(1.0))
        .then_some((o, n))
}

/// A planar end cut of a member, in its part's coordinates: the material
/// on the side `normal` points to is removed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EndCut {
    /// A point of the plane.
    pub point: Point3,
    /// Towards the removed side.
    pub normal: Vector3,
}

/// A frame member's body (`BodyOp::Prism`, format revision 57): the
/// section's loops (part XY; outer first, counter-clockwise, then the holes)
/// pushed along +Z onto a start and an end plane. A mitred member in one
/// kernel call: an extrusion with a Trim Solid per slanted end took ~0.4 s
/// an end (the kernel traces the section; meshing its spline edges).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrismOp {
    /// The section (copied into the part, rule 8).
    pub loops: Vec<Loop>,
    /// Start plane: a point and a normal (either side).
    pub start: [[Real; 3]; 2],
    /// End plane.
    pub end: [[Real; 3]; 2],
}

/// A segment of a section outline: a line or a circle arc (`sweep`
/// radians from angle `from`, positive counter-clockwise).
#[derive(Clone, Copy, Debug)]
enum Seg {
    Line(Point2, Point2),
    Arc {
        c: Point2,
        r: Real,
        from: Real,
        sweep: Real,
    },
}

/// The loop's segments in its own direction, arcs at most a quarter turn.
fn segments(l: &Loop) -> Vec<Seg> {
    use std::f64::consts::{FRAC_PI_2, TAU};
    let mut out = Vec::new();
    let arc = |c: Point2, r: Real, from: Real, sweep: Real, out: &mut Vec<Seg>| {
        let k = (sweep.abs() / FRAC_PI_2 - 1e-9).ceil().max(1.0) as usize;
        for i in 0..k {
            out.push(Seg::Arc {
                c,
                r,
                from: from + sweep * i as Real / k as Real,
                sweep: sweep / k as Real,
            });
        }
    };
    let angle = |c: Point2, p: Point2| (p.y - c.y).atan2(p.x - c.x);
    let turn = |from: Real, to: Real, ccw: bool| {
        let mut d = to - from;
        if ccw {
            while d <= 0.0 {
                d += TAU;
            }
        } else {
            while d >= 0.0 {
                d -= TAU;
            }
        }
        d
    };
    match l {
        Loop::Circle(c, r, hole) => arc(*c, *r, 0.0, if *hole { -TAU } else { TAU }, &mut out),
        Loop::Path(v) => {
            let n = v.len();
            for i in 0..n {
                let (a, b) = (v[i].0, v[(i + 1) % n].0);
                match v[i].1 {
                    Some((c, ccw)) => {
                        let r = (a - c).norm();
                        arc(
                            c,
                            r,
                            angle(c, a),
                            turn(angle(c, a), angle(c, b), ccw),
                            &mut out,
                        );
                    }
                    None => out.push(Seg::Line(a, b)),
                }
            }
        }
        Loop::Corners(cs) => {
            let n = cs.len();
            let ends: Vec<Corner> = (0..n)
                .map(|i| fillet(cs[(i + n - 1) % n].0, cs[i].0, cs[(i + 1) % n].0, cs[i].1))
                .collect();
            let span = |e: &Corner| match *e {
                Corner::Sharp(p) => (p, p),
                Corner::Round { a, b, .. } => (a, b),
            };
            for i in 0..n {
                if let Corner::Round { a, b, c, ccw } = ends[i] {
                    let r = (a - c).norm();
                    arc(
                        c,
                        r,
                        angle(c, a),
                        turn(angle(c, a), angle(c, b), ccw),
                        &mut out,
                    );
                }
                let from = span(&ends[i]).1;
                let to = span(&ends[(i + 1) % n]).0;
                if (to - from).norm() > 1e-12 {
                    out.push(Seg::Line(from, to));
                }
            }
        }
    }
    out
}

impl PrismOp {
    /// Refuses a prism that cannot be built.
    pub fn validate(&self) -> Result<(), String> {
        if self.loops.is_empty() {
            return Err("the member has no section".into());
        }
        for p in [self.start, self.end] {
            let n = Vector3::new(p[1][0], p[1][1], p[1][2]);
            if n.z.abs() < 0.05 * n.norm() {
                return Err("an end plane runs along the member".into());
            }
        }
        Ok(())
    }

    /// The plane of `p` with its normal along +Z, its x axis over the
    /// section's x, and the point of it over section point (x, y).
    fn plane(p: &[[Real; 3]; 2]) -> Result<(crate::kernel::Plane, Vector3, Point3), String> {
        let q = Point3::new(p[0][0], p[0][1], p[0][2]);
        let mut n = Vector3::new(p[1][0], p[1][1], p[1][2]);
        if n.z < 0.0 {
            n = -n;
        }
        let z0 = q.z + (n.x * q.x + n.y * q.y) / n.z;
        let o = Point3::new(0.0, 0.0, z0);
        let x = Vector3::new(n.z, 0.0, -n.x);
        let pl = crate::kernel::Plane::new(o, n, x).map_err(|e| e.to_string())?;
        Ok((pl, n, o))
    }

    /// The sections on both planes in their own coordinates (the loops'
    /// arcs as conics there), and the two planes (for the kernel's
    /// `prism_between`).
    #[allow(clippy::type_complexity)]
    pub fn sections(
        &self,
    ) -> Result<
        (
            Vec<(
                Vec<crate::kernel::exact::PrismSeg>,
                Vec<crate::kernel::exact::PrismSeg>,
            )>,
            crate::kernel::Plane,
            crate::kernel::Plane,
        ),
        String,
    > {
        use crate::kernel::exact::PrismSeg;
        self.validate()?;
        let (p0, n0, o0) = Self::plane(&self.start)?;
        let (p1, n1, o1) = Self::plane(&self.end)?;
        // Section point (x, y) pushed along +Z onto a plane, in its 2D
        // coordinates (an affine map: lines stay lines, arcs become conics).
        let on = |pl: &crate::kernel::Plane, n: &Vector3, o: &Point3, q: Point2| {
            let z = o.z - (n.x * q.x + n.y * q.y) / n.z;
            let p = pl.project(&Point3::new(q.x, q.y, z));
            Point2::new(p.x, p.y)
        };
        let map = |pl: &crate::kernel::Plane, n: &Vector3, o: &Point3, s: &Seg| match *s {
            Seg::Line(a, b) => PrismSeg::Line(on(pl, n, o, a), on(pl, n, o, b)),
            Seg::Arc { c, r, from, sweep } => {
                let h = sweep / 2.0;
                let at = |t: Real| Point2::new(c.x + r * t.cos(), c.y + r * t.sin());
                let m = from + h;
                let apex = Point2::new(c.x + r / h.cos() * m.cos(), c.y + r / h.cos() * m.sin());
                PrismSeg::Conic(
                    on(pl, n, o, at(from)),
                    on(pl, n, o, apex),
                    on(pl, n, o, at(from + sweep)),
                    h.cos(),
                )
            }
        };
        let loops = self
            .loops
            .iter()
            .map(|l| {
                let segs = segments(l);
                (
                    segs.iter().map(|s| map(&p0, &n0, &o0, s)).collect(),
                    segs.iter().map(|s| map(&p1, &n1, &o1, s)).collect(),
                )
            })
            .collect();
        Ok((loops, p0, p1))
    }
}

/// A notch of a member (Notch, Basic Profile), in its part's coordinates:
/// the prism of `outline` (in the plane through `origin` spanned by `x`,
/// `y`) along `x × y` over `length` is removed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NotchTool {
    /// The prism's start point (the notching member's section origin).
    pub origin: [Real; 3],
    /// The outline's horizontal axis.
    pub x: [Real; 3],
    /// The outline's vertical axis.
    pub y: [Real; 3],
    /// Prism length along `x × y`.
    pub length: Real,
    /// The notching member's outer outline (offset applied).
    pub outline: Loop,
}

impl NotchTool {
    /// Same prism within 1e-7 mm.
    pub fn same(&self, o: &NotchTool) -> bool {
        let near = |a: &[Real; 3], b: &[Real; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-7);
        near(&self.origin, &o.origin)
            && near(&self.x, &o.x)
            && near(&self.y, &o.y)
            && (self.length - o.length).abs() < 1e-7
            && self.outline == o.outline
    }

    /// The outline mapped into the plane `pl` of a sketch through the
    /// tool's start (points through 3D; a mirrored frame turns the arcs).
    fn outline_in(&self, pl: &crate::kernel::Plane) -> Loop {
        let v = |a: &[Real; 3]| Vector3::new(a[0], a[1], a[2]);
        let (o, x, y) = (Point3::from(v(&self.origin)), v(&self.x), v(&self.y));
        let map = |q: Point2| {
            let p = pl.project(&(o + x * q.x + y * q.y));
            Point2::new(p.x, p.y)
        };
        let same_turn = x.cross(&y).dot(&pl.normal) > 0.0;
        match &self.outline {
            Loop::Circle(c, r, h) => Loop::Circle(map(*c), *r, *h),
            Loop::Corners(cs) => {
                let mut cs: Vec<_> = cs.iter().map(|(q, r)| (map(*q), *r)).collect();
                if !same_turn {
                    cs.reverse();
                }
                Loop::Corners(cs)
            }
            Loop::Path(ps) => Loop::Path(
                ps.iter()
                    .map(|(q, a)| (map(*q), a.map(|(c, ccw)| (map(c), ccw == same_turn))))
                    .collect(),
            ),
        }
    }
}

/// A corner of an outline after filleting.
#[derive(Clone, Copy, Debug)]
enum Corner {
    Sharp(Point2),
    /// From `a` to `b` round centre `c`; `ccw` = the arc runs
    /// counter-clockwise from `a` to `b` (a convex corner of a CCW loop).
    Round {
        a: Point2,
        b: Point2,
        c: Point2,
        ccw: bool,
    },
}

/// The fillet of radius `r` at corner `p` between the edges from `prev`
/// and to `next`.
fn fillet(prev: Point2, p: Point2, next: Point2, r: Real) -> Corner {
    let (u, v) = ((prev - p).normalize(), (next - p).normalize());
    let cos = u.dot(&v).clamp(-1.0, 1.0);
    let half = cos.acos() / 2.0;
    if r <= 1e-9 || half < 1e-6 {
        return Corner::Sharp(p);
    }
    let along = r / half.tan();
    let a = p + u * along;
    let b = p + v * along;
    let bis: Vector2 = (u + v).normalize();
    let c = p + bis * (r / half.sin());
    // Turning left (CCW) from the incoming edge to the outgoing one.
    let din = p - prev;
    let left = din.perp(&(next - p)) > 0.0;
    Corner::Round { a, b, c, ccw: left }
}

/// The loop as a polygon (arcs in `per_turn` steps a full turn), in the
/// loop's own direction.
fn polygon(l: &Loop, per_turn: usize) -> Vec<Point2> {
    use std::f64::consts::TAU;
    match l {
        Loop::Circle(c, r, hole) => {
            let mut v: Vec<Point2> = (0..per_turn)
                .map(|k| {
                    let t = TAU * k as Real / per_turn as Real;
                    c + Vector2::new(t.cos(), t.sin()) * *r
                })
                .collect();
            if *hole {
                v.reverse();
            }
            v
        }
        Loop::Corners(corners) => {
            let n = corners.len();
            let mut out = Vec::new();
            for i in 0..n {
                let (prev, (p, r), next) = (
                    corners[(i + n - 1) % n].0,
                    corners[i],
                    corners[(i + 1) % n].0,
                );
                match fillet(prev, p, next, r) {
                    Corner::Sharp(q) => out.push(q),
                    Corner::Round { a, b, c, ccw } => {
                        let rad = (a - c).norm();
                        let a0 = (a.y - c.y).atan2(a.x - c.x);
                        let a1 = (b.y - c.y).atan2(b.x - c.x);
                        let mut sweep = a1 - a0;
                        if ccw && sweep < 0.0 {
                            sweep += TAU;
                        }
                        if !ccw && sweep > 0.0 {
                            sweep -= TAU;
                        }
                        let steps =
                            ((sweep.abs() / TAU) * per_turn as Real).ceil().max(2.0) as usize;
                        for k in 0..=steps {
                            let t = a0 + sweep * k as Real / steps as Real;
                            out.push(c + Vector2::new(t.cos(), t.sin()) * rad);
                        }
                    }
                }
            }
            out
        }
        Loop::Path(v) => {
            let n = v.len();
            let mut out = Vec::new();
            for i in 0..n {
                let (a, arc) = v[i];
                out.push(a);
                if let Some((c, ccw)) = arc {
                    let b = v[(i + 1) % n].0;
                    let rad = (a - c).norm();
                    let a0 = (a.y - c.y).atan2(a.x - c.x);
                    let a1 = (b.y - c.y).atan2(b.x - c.x);
                    let mut sweep = a1 - a0;
                    if ccw && sweep <= 0.0 {
                        sweep += TAU;
                    }
                    if !ccw && sweep >= 0.0 {
                        sweep -= TAU;
                    }
                    let steps = ((sweep.abs() / TAU) * per_turn as Real).ceil().max(2.0) as usize;
                    for k in 1..steps {
                        let t = a0 + sweep * k as Real / steps as Real;
                        out.push(c + Vector2::new(t.cos(), t.sin()) * rad);
                    }
                }
            }
            out
        }
    }
}

/// Is `q` inside the closed polygon `p` (even-odd)?
fn contains(p: &[Point2], q: Point2) -> bool {
    let mut inside = false;
    let n = p.len();
    for i in 0..n {
        let (a, b) = (p[i], p[(i + 1) % n]);
        if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// Distance from `q` to the nearest edge of the closed polygon `p`.
fn edge_distance(p: &[Point2], q: Point2) -> Real {
    let n = p.len();
    (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            let d = b - a;
            let t = ((q - a).dot(&d) / d.norm_squared().max(1e-300)).clamp(0.0, 1.0);
            (a + d * t - q).norm()
        })
        .fold(Real::MAX, Real::min)
}

/// The user's profile folder (`%APPDATA%\3dcadcraft\profiles`; tests and
/// scripted runs move it with `CADCRAFT3D_PROFILES_DIR`).
pub fn user_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("CADCRAFT3D_PROFILES_DIR") {
        return Some(PathBuf::from(d));
    }
    crate::materials::user_dir().and_then(|d| d.parent().map(|p| p.join("profiles")))
}

/// One family of the user's own outlines, as stored in its JSON file.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct UserFamily {
    standard: String,
    family: String,
    sizes: Vec<UserSize>,
}

/// One size of a [`UserFamily`].
#[derive(Clone, Debug, Serialize, Deserialize)]
struct UserSize {
    size: String,
    width: Real,
    height: Real,
    loops: Vec<Loop>,
}

/// The library the commands use: built in + the user's folder, read once
/// and again on [`Library::reload`].
static CURRENT: RwLock<Option<Arc<Library>>> = RwLock::new(None);

/// The section table: built in, then a user file's sizes over it.
#[derive(Clone, Debug, Default)]
pub struct Library {
    /// Every size, in file order.
    pub sections: Vec<Section>,
}

impl Library {
    /// The built-in table with the user's folder over it (spec 6.1
    /// §3.11): what Insert Frame and Change list.
    pub fn current() -> Arc<Library> {
        if let Ok(g) = CURRENT.read()
            && let Some(l) = g.as_ref()
        {
            return l.clone();
        }
        Library::reload().0
    }

    /// Reads the user's folder again (Refresh Categories); returns the new
    /// library and what could not be read.
    pub fn reload() -> (Arc<Library>, Vec<String>) {
        let (lib, bad) = match user_dir() {
            Some(d) => Library::with_user(&d),
            None => (Library::builtin().clone(), Vec::new()),
        };
        let lib = Arc::new(lib);
        if let Ok(mut g) = CURRENT.write() {
            *g = Some(lib.clone());
        }
        (lib, bad)
    }

    /// The built-in table, then every `*.csv` (the built-in columns) and
    /// `*.json` (outlines saved by Author Frame Profile) of folder `dir`;
    /// the same family + size replaces a size. Returns what could not be
    /// read (`file: line`).
    pub fn with_user(dir: &Path) -> (Library, Vec<String>) {
        let mut lib = Library::builtin().clone();
        let mut bad = Vec::new();
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|r| r.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default();
        files.sort();
        for f in files {
            let name = f
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let Ok(text) = std::fs::read_to_string(&f) else {
                continue;
            };
            match f.extension().and_then(|e| e.to_str()) {
                Some("csv") => {
                    let (l, b) = Library::parse(&text);
                    for s in l.sections.into_iter().filter(|s| s.shape != Shape::Custom) {
                        lib.add(s);
                    }
                    bad.extend(b.into_iter().map(|line| format!("{name}: {line}")));
                }
                Some("json") => match serde_json::from_str::<UserFamily>(&text) {
                    Ok(u) => {
                        for z in u.sizes {
                            lib.add(Section {
                                family: u.family.clone(),
                                standard: u.standard.clone(),
                                shape: Shape::Custom,
                                size: z.size,
                                h: z.height,
                                b: z.width,
                                t: 0.0,
                                tw: 0.0,
                                tf: 0.0,
                                r1: 0.0,
                                r2: 0.0,
                                custom: z.loops,
                                grid: 0.0,
                                area: 0.0,
                                chamber: 0.0,
                                undercut: 0.0,
                                ix_published: 0.0,
                                iy_published: 0.0,
                                it_published: 0.0,
                            });
                        }
                    }
                    Err(e) => bad.push(format!("{name}: {e}")),
                },
                _ => {}
            }
        }
        (lib, bad)
    }

    /// Saves custom section `s` into its family's JSON file in `dir` (the
    /// same size replaced); returns the file.
    pub fn save_custom(dir: &Path, s: &Section) -> std::io::Result<PathBuf> {
        let safe: String = s
            .family
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{safe}.json"));
        let mut fam = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<UserFamily>(&t).ok())
            .unwrap_or_default();
        fam.standard = s.standard.clone();
        fam.family = s.family.clone();
        fam.sizes.retain(|z| z.size != s.size);
        fam.sizes.push(UserSize {
            size: s.size.clone(),
            width: s.b,
            height: s.h,
            loops: s.custom.clone(),
        });
        let text = serde_json::to_string_pretty(&fam).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &path)?;
        Ok(path)
    }

    /// The built-in table.
    pub fn builtin() -> &'static Library {
        static LIB: std::sync::OnceLock<Library> = std::sync::OnceLock::new();
        LIB.get_or_init(|| Library::parse(BUILTIN).0)
    }

    /// Reads a table; returns it and the lines it could not read.
    pub fn parse(text: &str) -> (Library, Vec<String>) {
        let mut lib = Library::default();
        let mut bad = Vec::new();
        let num = |s: Option<&str>| -> Real {
            s.map(str::trim)
                .filter(|s| !s.is_empty())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0)
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("family,") {
                continue;
            }
            let f: Vec<&str> = line.split(',').collect();
            let Some(shape) = f.get(2).and_then(|s| Shape::parse(s)) else {
                bad.push(line.to_owned());
                continue;
            };
            lib.add(Section {
                family: f[0].trim().to_owned(),
                standard: f.get(1).map_or("", |s| s.trim()).to_owned(),
                shape,
                size: f.get(3).map_or("", |s| s.trim()).to_owned(),
                h: num(f.get(4).copied()),
                b: num(f.get(5).copied()),
                t: num(f.get(6).copied()),
                tw: num(f.get(7).copied()),
                tf: num(f.get(8).copied()),
                r1: num(f.get(9).copied()),
                r2: num(f.get(10).copied()),
                custom: Vec::new(),
                grid: num(f.get(11).copied()),
                area: num(f.get(12).copied()),
                chamber: num(f.get(13).copied()),
                undercut: num(f.get(14).copied()),
                // Published in cm⁴.
                ix_published: num(f.get(15).copied()) * 1e4,
                iy_published: num(f.get(16).copied()) * 1e4,
                it_published: num(f.get(17).copied()) * 1e4,
            });
        }
        (lib, bad)
    }

    /// Adds a size, replacing one of the same family and size.
    pub fn add(&mut self, s: Section) {
        match self
            .sections
            .iter_mut()
            .find(|x| x.family == s.family && x.size == s.size)
        {
            Some(x) => *x = s,
            None => self.sections.push(s),
        }
    }

    /// The standards, in table order.
    pub fn standards(&self) -> Vec<&str> {
        let mut v: Vec<&str> = Vec::new();
        for s in &self.sections {
            if !v.contains(&s.standard.as_str()) {
                v.push(&s.standard);
            }
        }
        v
    }

    /// The families of `standard`, in table order.
    pub fn families(&self, standard: &str) -> Vec<&str> {
        let mut v: Vec<&str> = Vec::new();
        for s in self.sections.iter().filter(|s| s.standard == standard) {
            if !v.contains(&s.family.as_str()) {
                v.push(&s.family);
            }
        }
        v
    }

    /// The family named `family`, with the table's lifetime.
    pub fn families_named(&self, family: &str) -> Option<&str> {
        self.sections
            .iter()
            .find(|s| s.family == family)
            .map(|s| s.family.as_str())
    }

    /// The sizes of a family, in table order.
    pub fn sizes(&self, family: &str) -> Vec<&str> {
        self.sections
            .iter()
            .filter(|s| s.family == family)
            .map(|s| s.size.as_str())
            .collect()
    }

    /// One size.
    pub fn find(&self, family: &str, size: &str) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| s.family == family && s.size == size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sec(family: &str, size: &str) -> Section {
        Library::builtin()
            .find(family, size)
            .unwrap_or_else(|| panic!("{family} {size}"))
            .clone()
    }

    fn close(got: Real, want: Real, rel: Real, what: &str) {
        assert!(
            (got - want).abs() <= rel * want.abs(),
            "{what}: {got} vs {want}"
        );
    }

    /// EN 10219-2 tabulates It = 15.7 cm⁴ for SHS 40x40x3; a round bar is
    /// πd⁴/32 exactly.
    #[test]
    fn torsion_constants() {
        let s = sec("Square hollow section (EN 10219)", "40x40x3");
        close(s.torsion().0, 15.7e4, 0.01, "SHS It");
        let lib = Library::builtin();
        let chs = lib
            .standards()
            .iter()
            .flat_map(|st| lib.families(st))
            .find(|f| f.contains("Circular hollow"))
            .and_then(|f| lib.sizes(f).first().and_then(|z| lib.find(f, z)))
            .unwrap();
        let d = chs.h - 2.0 * chs.t;
        let want = std::f64::consts::PI / 32.0 * (chs.h.powi(4) - d.powi(4));
        close(chs.torsion().0, want, 1e-12, "CHS");
    }

    #[test]
    fn the_table_reads_every_line() {
        let (lib, bad) = Library::parse(BUILTIN);
        assert!(bad.is_empty(), "{bad:?}");
        assert!(lib.sections.len() > 50);
        assert_eq!(lib.standards(), vec!["EN", "ANSI", "Other"]);
        assert!(lib.families("EN").contains(&"IPE beam (EN 10365)"));
        assert!(lib.sizes("IPE beam (EN 10365)").contains(&"IPE 200"));
    }

    /// Areas and second moments against the standards' tables.
    #[test]
    fn section_properties_match_the_standards_tables() {
        let ipe = sec("IPE beam (EN 10365)", "IPE 200").properties();
        close(ipe.area, 2848.0, 0.002, "IPE 200 A");
        close(ipe.ix, 1943e4, 0.003, "IPE 200 Iy (strong axis)");
        close(ipe.iy, 142.4e4, 0.005, "IPE 200 Iz (weak axis)");
        close(ipe.wx, 194.3e3, 0.003, "IPE 200 Wel,y");
        let shs = sec("Square hollow section (EN 10219)", "40x40x3").properties();
        close(shs.area, 421.0, 0.003, "SHS 40x40x3 A");
        close(shs.ix, 9.32e4, 0.01, "SHS 40x40x3 I");
        let l = sec("Equal angle (EN 10056-1)", "L 50x50x5").properties();
        close(l.area, 480.0, 0.003, "L 50x50x5 A");
        close(l.ix, 11.0e4, 0.01, "L 50x50x5 Iy");
        // Centroid 14.0 mm from the heel (e).
        close(l.centroid.y + 25.0, 14.0, 0.01, "L 50x50x5 e");
        let chs = sec("Circular hollow section (EN 10219)", "48.3x3.2").properties();
        close(chs.area, 453.4, 0.003, "CHS 48.3x3.2 A");
        let fl = sec("Flat bar (EN 10058)", "40x5").properties();
        close(fl.area, 200.0, 1e-9, "FL 40x5 A");
        close(fl.ix, 40.0 * 125.0 / 12.0, 1e-9, "FL 40x5 Ix");
    }

    /// A member is the section times its length; the sketch has one region
    /// of material (a tube's inside is not in it).
    #[test]
    fn a_member_part_is_the_section_extruded_by_its_length() {
        for (f, s) in [
            ("Square hollow section (EN 10219)", "40x40x3"),
            ("IPE beam (EN 10365)", "IPE 200"),
            ("Equal angle (EN 10056-1)", "L 50x50x5"),
            ("Circular hollow section (EN 10219)", "48.3x3.2"),
            ("Round bar (EN 10060)", "D20"),
            ("UPE channel (EN 10365)", "UPE 120"),
            ("T section (EN 10055)", "T 50"),
            ("Unequal angle (EN 10056-1)", "L 60x40x5"),
        ] {
            let sec = sec(f, s);
            let part = sec.member_part(500.0, Some("Steel S235")).unwrap();
            assert!(
                part.feature_health.is_empty(),
                "{s}: {:?}",
                part.feature_health
            );
            let v: Real = part
                .exact_bodies
                .0
                .iter()
                .filter_map(|b| b.volume_at(1e-3))
                .sum();
            close(v, sec.properties().area * 500.0, 0.003, s);
            assert_eq!(part.material.as_deref(), Some("Steel S235"));
        }
    }

    /// A 45° cut at each end of a flat bar (a mitred piece of a square
    /// frame corner): the part is the trapezoid between the cut planes.
    #[test]
    fn end_cuts_trim_the_member() {
        let s = sec("Flat bar (EN 10058)", "40x5");
        // Section x across 40 mm; cuts in the x-z plane at 45°: the
        // outer edge (x = +20) is 500 + 2·20 long, the inner one 500 − 2·20.
        let r = std::f64::consts::FRAC_1_SQRT_2;
        let cuts = [
            EndCut {
                point: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(r, 0.0, -r),
            },
            EndCut {
                point: Point3::new(0.0, 0.0, 500.0),
                normal: Vector3::new(r, 0.0, r),
            },
        ];
        let part = s
            .member_part_cut(500.0, [30.0, 30.0], &cuts, &[], None)
            .unwrap();
        assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
        let v: Real = part
            .exact_bodies
            .0
            .iter()
            .filter_map(|b| b.volume_at(1e-3))
            .sum();
        // Mean length 500 (the trapezoid's mid-line) × 40 × 5.
        close(v, 500.0 * 200.0, 0.002, "mitred flat");
        let (lo, hi) = part.body.bounding_box().unwrap();
        close(hi.z - lo.z, 540.0, 1e-3, "outer length");
    }

    #[test]
    fn a_member_without_length_is_refused() {
        let s = sec("Flat bar (EN 10058)", "40x5");
        assert!(s.member_part(0.0, None).is_err());
    }

    /// The added families against the standards' tables (areas, cm²).
    #[test]
    fn channels_tees_and_unequal_angles_match_their_tables() {
        for (f, s, a) in [
            ("UPE channel (EN 10365)", "UPE 200", 2900.0),
            ("HEM beam (EN 10365)", "HEM 200", 13130.0),
            ("T section (EN 10055)", "T 50", 566.0),
            ("Unequal angle (EN 10056-1)", "L 60x40x5", 479.0),
        ] {
            close(sec(f, s).properties().area, a, 0.02, s);
        }
        // An unequal angle's centroid lies towards the long leg's side.
        let p = sec("Unequal angle (EN 10056-1)", "L 60x40x5").properties();
        assert!(p.centroid.x < 0.0 && p.centroid.y < 0.0);
    }

    /// A member mitred at 45° at both ends is one prism (no Trim Solid):
    /// volume A × mean length, Start / End caps on the planes, quickly.
    /// A slanted start cut past the member's middle keeps the far piece only:
    /// taken for an end cut (by where it lies) it left the short piece as well.
    #[test]
    fn a_slanted_cut_past_the_middle_keeps_one_piece() {
        let s = sec("Square hollow section (EN 10219)", "40x40x3");
        let (w, _) = s.extent();
        for (point, n, keep) in [
            // Start side kept off: material above the plane.
            (240.0, Vector3::new(0.5, 0.0, -1.0), 160.0),
            (200.0, Vector3::new(0.5, 0.0, -1.0), 200.0),
            // End side kept off, before the middle: material below it.
            (160.0, Vector3::new(0.5, 0.0, 1.0), 160.0),
        ] {
            let cuts = [EndCut {
                point: Point3::new(0.0, 0.0, point),
                normal: n.normalize(),
            }];
            let part = s
                .member_part_cut(400.0, [w + 1.0, w + 1.0], &cuts, &[], None)
                .unwrap();
            assert!(part.feature_health.is_empty(), "{:?}", part.feature_health);
            assert_eq!(part.exact_bodies.0.len(), 1, "one body at {point}");
            let v: Real = part
                .exact_bodies
                .0
                .iter()
                .filter_map(|b| b.volume_at(1e-3))
                .sum();
            let (area, _) = (s.properties().area, 0);
            // Measured along the axis: the slant is symmetric about the point
            // where the plane crosses the axis (+ the extension past the far end).
            let far = w + 1.0;
            let want = area * (keep + far);
            assert!((v - want).abs() / want < 0.02, "{point}: {v} vs {want}");
        }
    }

    #[test]
    fn a_mitred_member_is_one_prism() {
        for (f, size) in [
            ("Square hollow section (EN 10219)", "40x40x3"),
            ("Circular hollow section (EN 10219)", "48.3x3.2"),
            ("IPN beam (EN 10365)", "IPN 100"),
        ] {
            let s = sec(f, size);
            let (w, _) = s.extent();
            let n0 = Vector3::new(1.0, 0.0, -1.0).normalize();
            let n1 = Vector3::new(1.0, 0.0, 1.0).normalize();
            let cuts = [
                EndCut {
                    point: Point3::origin(),
                    normal: n0,
                },
                EndCut {
                    point: Point3::new(0.0, 0.0, 400.0),
                    normal: n1,
                },
            ];
            let t0 = std::time::Instant::now();
            let part = s
                .member_part_cut(400.0, [w + 1.0, w + 1.0], &cuts, &[], None)
                .unwrap();
            let ms = t0.elapsed().as_secs_f64() * 1e3;
            assert!(
                part.feature_health.is_empty(),
                "{size}: {:?}",
                part.feature_health
            );
            assert!(
                matches!(part.features[0], crate::part::Feature::BodyOp(_)),
                "{size}: not a prism"
            );
            let v: Real = part
                .exact_bodies
                .0
                .iter()
                .filter_map(|b| b.volume_at(1e-3))
                .sum();
            close(v, s.properties().area * 400.0, 0.002, size);
            let names: Vec<String> = part
                .body
                .faces()
                .filter_map(|(_, f)| f.name.clone())
                .collect();
            assert!(
                names.iter().any(|n| n.ends_with(":Start"))
                    && names.iter().any(|n| n.ends_with(":End")),
                "{size}: {names:?}"
            );
            eprintln!("{size}: mitred member {ms:.0} ms");
        }
    }

    /// The bars, cold-formed sections and ANSI families: areas of their
    /// tables (bars exact, ANSI within 3 %, cold-formed = wall × the centre
    /// line), and a member of each builds whole.
    #[test]
    fn more_families_match_their_tables() {
        let in2 = 25.4 * 25.4;
        for (f, s, a, tol) in [
            ("Square bar (EN 10059)", "20", 400.0, 1e-6),
            (
                "Hexagon bar (EN 10061)",
                "24",
                3f64.sqrt() / 2.0 * 576.0,
                1e-6,
            ),
            (
                "Cold-formed lipped channel (EN 10162)",
                "C100x50x15x2",
                434.0,
                0.02,
            ),
            ("Cold-formed Z (EN 10162)", "Z100x50x2", 387.0, 0.02),
            ("W shape (ASTM A6)", "W8x31", 9.13 * in2, 0.03),
            ("W shape (ASTM A6)", "W14x22", 6.49 * in2, 0.03),
            ("Pipe Schedule 40 (ASME B36.10)", "NPS 2", 1.07 * in2, 0.02),
            ("Equal angle (AISC)", "L4x4x1/2", 3.75 * in2, 0.03),
            ("Square HSS (ASTM A500)", "HSS4x4x1/4", 3.37 * in2, 0.03),
        ] {
            let sec = sec(f, s);
            close(sec.properties().area, a, tol, s);
            let part = sec.member_part(100.0, None).unwrap();
            let v: Real = part
                .exact_bodies
                .0
                .iter()
                .filter_map(|b| b.volume_at(1e-3))
                .sum();
            close(v, sec.properties().area * 100.0, 0.003, s);
        }
        // The ANSI standard lists its families.
        let lib = Library::builtin();
        assert!(lib.standards().contains(&"ANSI"));
    }

    /// T-slot framing (spec 6.1 §3.11a): every size's area is the published
    /// one within 1 %, a walled (Bosch Rexroth) outline has the published second
    /// moments within 2 % (one cell) or 12 % / 17 % (several),
    /// the properties give the published moments and torsion constant, the
    /// chamber's floor is at least as wide as the opening, the loops stay more
    /// than 0.7 mm apart, and a member builds with the area's volume.
    #[test]
    fn t_slot_sections_have_their_published_area() {
        let lib = Library::builtin();
        let rows: Vec<&Section> = lib
            .sections
            .iter()
            .filter(|s| s.shape == Shape::TSlot)
            .collect();
        assert_eq!(rows.len(), 22);
        for s in &rows {
            let p = s.outline_properties();
            println!(
                "{} {}: A {:.0} (published {:.0}) Ix {:.2} ({:.2}) Iy {:.2} ({:.2}) cm4, fit {:?}",
                s.family,
                s.size,
                p.area,
                s.area,
                p.ix / 1e4,
                s.ix_published / 1e4,
                p.iy / 1e4,
                s.iy_published / 1e4,
                (s.chamber > 0.0).then(|| s.walled_fit())
            );
        }
        for s in rows {
            let p = s.outline_properties();
            close(p.area, s.area, 0.01, &s.size);
            if s.chamber > 0.0 {
                let ((strong, weak), (ps, pw)) = if s.b > s.h {
                    ((p.iy, p.ix), (s.iy_published, s.ix_published))
                } else {
                    ((p.ix, p.iy), (s.ix_published, s.iy_published))
                };
                // One cell: the moments within 2 %; several: within 12 % / 17 %
                // (their inside is a guess of ours, see spec 6.1 §3.11a).
                let one = s.tslot_cells() == (1, 1);
                close(strong, ps, if one { 0.02 } else { 0.12 }, &s.size);
                close(weak, pw, if one { 0.02 } else { 0.17 }, &s.size);
                let q = s.properties();
                assert_eq!((q.ix, q.iy), (s.ix_published, s.iy_published));
                assert_eq!(s.torsion().0, s.it_published);
            } else {
                let fit = s.tslot_fit();
                assert!(
                    fit.chamber - 2.0 * (s.tf - s.t) >= s.tw - 1e-9,
                    "{}",
                    s.size
                );
            }
            let polys: Vec<Vec<Point2>> = s.loops().iter().map(|l| polygon(l, 96)).collect();
            // Every hole lies inside the outside and clear of the other loops.
            for (i, a) in polys.iter().enumerate().skip(1) {
                for q in a {
                    assert!(contains(&polys[0], *q), "{} hole {i}", s.size);
                    for (j, b) in polys.iter().enumerate() {
                        if j != i {
                            assert!(edge_distance(b, *q) > 0.7, "{} loops {i} {j}", s.size);
                        }
                    }
                }
            }
            let part = s.member_part(100.0, Some("Aluminium 6063-T66")).unwrap();
            let v: Real = part
                .exact_bodies
                .0
                .iter()
                .filter_map(|b| b.volume_at(1e-3))
                .sum();
            close(v, p.area * 100.0, 0.005, &s.size);
        }
    }

    /// Tapered flanges (UPN 8 %, IPN 14 %): areas and Ix of the EN 10365
    /// tables within 2 % / 3 %, and a member of each builds whole.
    #[test]
    fn upn_and_ipn_match_their_tables() {
        for (f, s, a, ix) in [
            ("UPN channel (EN 10365)", "UPN 100", 1350.0, 206e4),
            ("UPN channel (EN 10365)", "UPN 200", 3220.0, 1910e4),
            ("IPN beam (EN 10365)", "IPN 100", 1060.0, 171e4),
            ("IPN beam (EN 10365)", "IPN 200", 3340.0, 2140e4),
        ] {
            let p = sec(f, s).properties();
            close(p.area, a, 0.02, s);
            close(p.ix, ix, 0.03, s);
            let part = sec(f, s).member_part(500.0, None).unwrap();
            let v: Real = part
                .exact_bodies
                .0
                .iter()
                .filter_map(|b| b.volume_at(1e-3))
                .sum();
            close(v, p.area * 500.0, 0.002, s);
        }
    }

    /// A D-shaped outline (a 40 mm line closed by a half circle) with a
    /// Ø10 hole as the user's profile: area, member volume, save / load.
    #[test]
    fn a_profile_from_a_sketch_region() {
        use crate::sketch::Sketch;
        let mut sk = Sketch::new(crate::kernel::Plane::xy());
        let a = sk.add_point(Point2::new(100.0, 0.0));
        let b = sk.add_point(Point2::new(140.0, 0.0));
        let c = sk.add_point(Point2::new(120.0, 0.0));
        sk.add_line_between(a, b).unwrap();
        sk.add_arc(c, b, a).unwrap();
        sk.add_circle(Point2::new(120.0, 8.0), 5.0).unwrap();
        let regions = sk.regions();
        let r = regions
            .iter()
            .find(|r| r.contains(Point2::new(120.0, 2.0)))
            .unwrap();
        let s = Section::custom("Own", "D profile", "D40", r).unwrap();
        assert_eq!(s.shape, Shape::Custom);
        assert!((s.b - 40.0).abs() < 1e-6 && (s.h - 20.0).abs() < 1e-6);
        let want = std::f64::consts::PI * (400.0 / 2.0 - 25.0);
        close(s.properties().area, want, 0.002, "D40");
        let part = s.member_part(100.0, None).unwrap();
        let v: Real = part
            .exact_bodies
            .0
            .iter()
            .filter_map(|b| b.volume_at(1e-3))
            .sum();
        close(v, want * 100.0, 0.005, "D40 volume");
        // Saved into the user folder and read back with the built-in table.
        let dir = std::env::temp_dir().join(format!("cc3d-profiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Library::save_custom(&dir, &s).unwrap();
        std::fs::write(
            dir.join("mine.csv"),
            "family,standard,shape,size,h,b,t,tw,tf,r1,r2
Flat bar (EN 10058),EN,flat,45x7,,45,7,,,,
",
        )
        .unwrap();
        let (lib, bad) = Library::with_user(&dir);
        assert!(bad.is_empty(), "{bad:?}");
        assert!(lib.standards().contains(&"Own"));
        let back = lib.find("D profile", "D40").unwrap();
        assert_eq!(back, &s);
        assert!(lib.find("Flat bar (EN 10058)", "45x7").is_some());
        assert!(lib.find("IPE beam (EN 10365)", "IPE 200").is_some());
        // A profile without a family is refused.
        assert!(Section::custom("Own", "", "x", r).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
