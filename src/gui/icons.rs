//! Command icons, drawn as vectors (our own design; nothing copied).
//!
//! Every icon is described on a 16 × 16 design grid (y down) with a few
//! primitives and two colours: `ink` (neutral outline) and `accent` (the
//! geometry the command creates or acts on); `hot` marks points and `warn`
//! removed parts. Vectors scale cleanly to any size and follow the theme,
//! and disabled commands get dimmed colours.

use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2};

/// Icon colours.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub ink: Color32,
    pub accent: Color32,
    pub hot: Color32,
    pub warn: Color32,
    pub ok: Color32,
}

impl Palette {
    /// Colours for an enabled command on the dark theme.
    pub const ENABLED: Palette = Palette {
        ink: Color32::from_rgb(205, 210, 218),
        accent: Color32::from_rgb(80, 160, 255),
        hot: Color32::from_rgb(255, 160, 50),
        warn: Color32::from_rgb(235, 80, 70),
        ok: Color32::from_rgb(90, 200, 110),
    };
    /// Colours for a disabled (or planned) command.
    pub const DISABLED: Palette = Palette {
        ink: Color32::from_rgb(95, 98, 104),
        accent: Color32::from_rgb(70, 90, 115),
        hot: Color32::from_rgb(110, 95, 75),
        warn: Color32::from_rgb(110, 80, 78),
        ok: Color32::from_rgb(80, 105, 85),
    };
}

/// All command icons.
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    // sketch: create
    Select,
    Line,
    CircleCenter,
    CircleTangent,
    ArcThree,
    ArcTangent,
    ArcCenter,
    Rect2,
    Rect3,
    RectCenter,
    RectCenter3,
    SlotCenter,
    SlotOverall,
    SlotPoint,
    SlotArc,
    Spline,
    Ellipse,
    Point,
    Polygon,
    Fillet2d,
    Chamfer2d,
    Text,
    // sketch: modify
    Move,
    Copy,
    Rotate,
    Trim,
    Extend,
    Split,
    Offset,
    Scale,
    Stretch,
    // sketch: pattern
    PatternRect,
    PatternCirc,
    PatternPoints,
    SplineInterp,
    PatternPath,
    Mirror,
    // sketch: constrain
    Dimension,
    Coincident,
    Collinear,
    Concentric,
    Fix,
    Parallel,
    Perpendicular,
    Horizontal,
    Vertical,
    Tangent,
    Smooth,
    Symmetric,
    Equal,
    Relax,
    Dof,
    ShowConstraints,
    CoordSystem,
    // sketch: format / exit
    Construction,
    Centerline,
    Normal,
    Demo,
    Finish,
    // 3D model
    StartSketch,
    Extrude,
    Revolve,
    Sweep,
    Loft,
    Coil,
    Rib,
    Emboss,
    Hole,
    SlotFeature,
    FilletEdge,
    ChamferEdge,
    Shell,
    Draft,
    Thread,
    Combine,
    SplitBody,
    WorkPlane,
    WorkAxis,
    WorkPoint,
    Ucs,
    Box,
    /// Primitive cylinder.
    Cylinder,
    /// Primitive sphere.
    Sphere,
    /// Primitive torus.
    Torus,
    // inspect / tools / view
    Measure,
    Mass,
    Validate,
    Interference,
    Curvature,
    Options,
    Settings,
    Parameters,
    /// Rebuild All (two arrows round a part).
    Rebuild,
    Rules,
    Assistant,
    Undo,
    Redo,
    Home,
    LookAt,
    ViewFront,
    ViewTop,
    ViewRight,
    VisualStyle,
    /// Material (a ball with a density band).
    Material,
    /// Appearance (a ball with colour patches).
    Appearance,
    /// Adjust appearance (a brush on a face).
    AdjustLook,
    /// Clear appearance overrides.
    ClearLook,
    /// Styles Editor (stacked sheets).
    Styles,
    /// Purge (a bin).
    Purge,
    Section,
    Ortho,
    // browser
    Part,
    Origin,
    Sketch,
    Project,
    // assembly
    Assembly,
    Folder,
    SheetMetal,
    // sheet metal commands (one each)
    SmFace,
    SmFlange,
    SmFlatPattern,
    SmContour,
    SmHem,
    SmFold,
    SmBend,
    SmContourRoll,
    SmLoftedFlange,
    SmRip,
    SmCut,
    SmCornerSeam,
    SmCornerRound,
    SmCornerChamfer,
    SmPunch,
    SmUnfold,
    SmRefold,
    SmExportFlat,
    Drawing,
    // drawings
    BaseView,
    ProjectedView,
    AuxView,
    SectionView,
    DetailView,
    BreakView,
    BreakOut,
    SliceView,
    CropView,
    DraftView,
    NewSheet,
    CenterMark,
    CenterlineMark,
    Balloon,
    PartsList,
    HoleNote,
    SurfaceSymbol,
    Datum,
    ControlFrame,
    Place,
    FreeMove,
    FreeRotate,
    Ground,
    // property panel choices
    DirDefault,
    DirFlipped,
    DirSymmetric,
    DirAsymmetric,
    OpJoin,
    OpCut,
    OpIntersect,
    OpNewSolid,
    OutSolid,
    OutSurface,
    // windows
    /// User Interface (a window with side panels).
    UiPanels,
    /// Clean Screen (an empty frame with the model).
    CleanScreen,
    /// Full Screen (corner brackets).
    FullScreen,
    /// Reset UI Layout (a frame with a turning arrow).
    ResetLayout,
    // documents of a design file (spec 0.6b): a sheet with a sign
    /// New Part in this file.
    DocNewPart,
    /// New Assembly in this file.
    DocNewAssembly,
    /// New Drawing in this file.
    DocNewDrawing,
    /// New Presentation in this file.
    DocNewPresentation,
    /// Insert Document (a sheet coming in).
    DocInsert,
    /// Link File (a chain link).
    DocLink,
    /// Duplicate Document (two sheets).
    DocDuplicate,
    /// Rename Document (a text cursor).
    DocRename,
    /// Delete Document (a cross).
    DocDelete,
    /// The Documents window (a list of sheets).
    Documents,
    /// Save to Separate File (a sheet going out).
    DocExtract,
    /// Embed (a file into the sheet).
    DocEmbed,
    /// Embed All / Pack (sheets into a box).
    DocPack,
    /// Separate All / Unpack (sheets out of a box).
    DocUnpack,
}

/// Pen on the 16 × 16 design grid mapped into a screen rect.
struct Pen<'a> {
    p: &'a Painter,
    r: Rect,
    w: f32,
}

impl Pen<'_> {
    fn pt(&self, x: f32, y: f32) -> Pos2 {
        let s = self.r.width() / 16.0;
        pos2(self.r.left() + x * s, self.r.top() + y * s)
    }
    fn scale(&self) -> f32 {
        self.r.width() / 16.0
    }
    fn line(&self, pts: &[(f32, f32)], c: Color32) {
        let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| self.pt(x, y)).collect();
        self.p.add(Shape::line(pts, Stroke::new(self.w, c)));
    }
    fn closed(&self, pts: &[(f32, f32)], c: Color32) {
        let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| self.pt(x, y)).collect();
        self.p.add(Shape::closed_line(pts, Stroke::new(self.w, c)));
    }
    fn fill(&self, pts: &[(f32, f32)], c: Color32) {
        let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| self.pt(x, y)).collect();
        self.p.add(Shape::convex_polygon(pts, c, Stroke::NONE));
    }
    fn dashed(&self, a: (f32, f32), b: (f32, f32), c: Color32) {
        let pts = [self.pt(a.0, a.1), self.pt(b.0, b.1)];
        let s = self.scale();
        self.p.extend(Shape::dashed_line(
            &pts,
            Stroke::new(self.w * 0.8, c),
            1.6 * s,
            1.2 * s,
        ));
    }
    fn circle(&self, x: f32, y: f32, r: f32, c: Color32) {
        self.p
            .circle_stroke(self.pt(x, y), r * self.scale(), Stroke::new(self.w, c));
    }
    fn disc(&self, x: f32, y: f32, r: f32, c: Color32) {
        self.p.circle_filled(self.pt(x, y), r * self.scale(), c);
    }
    /// Arc from `a0` to `a1` degrees (counter-clockwise on screen = y up).
    fn arc(&self, x: f32, y: f32, r: f32, a0: f32, a1: f32, c: Color32) {
        let n = (((a1 - a0).abs() / 10.0).ceil() as usize).max(2);
        let pts: Vec<(f32, f32)> = (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
                (x + r * a.cos(), y - r * a.sin())
            })
            .collect();
        self.line(&pts, c);
    }
    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32, c: Color32) {
        self.closed(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)], c);
    }
    fn text(&self, x: f32, y: f32, t: &str, size: f32, c: Color32) {
        self.p.text(
            self.pt(x, y),
            eframe::egui::Align2::CENTER_CENTER,
            t,
            eframe::egui::FontId::proportional(size * self.scale()),
            c,
        );
    }
    /// Arrow head at `tip` pointing along `dir`.
    fn head(&self, tip: (f32, f32), dir: (f32, f32), c: Color32) {
        let l = (dir.0 * dir.0 + dir.1 * dir.1).sqrt().max(1e-3);
        let (dx, dy) = (dir.0 / l * 2.2, dir.1 / l * 2.2);
        let (nx, ny) = (-dy * 0.6, dx * 0.6);
        self.fill(
            &[
                tip,
                (tip.0 - dx + nx, tip.1 - dy + ny),
                (tip.0 - dx - nx, tip.1 - dy - ny),
            ],
            c,
        );
    }
    /// A small isometric block (for 3D features): top, front, side.
    /// `b` = `[x, y, width, height, depth]` of the front face and depth.
    fn block(&self, b: [f32; 5], c: Color32, fill: Color32) {
        let [x, y, w, h, d] = b;
        let (dx, dy) = (d * 0.8, -d * 0.5);
        let front = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
        let top = [(x, y), (x + dx, y + dy), (x + w + dx, y + dy), (x + w, y)];
        let side = [
            (x + w, y),
            (x + w + dx, y + dy),
            (x + w + dx, y + h + dy),
            (x + w, y + h),
        ];
        for face in [front, top, side] {
            self.fill(&face, fill);
            self.closed(&face, c);
        }
    }
}

/// Draws `icon` into `rect` (square) with palette `k`.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, k: &Palette) {
    let pen = Pen {
        p: painter,
        r: rect,
        w: (rect.width() / 16.0 * 1.15).max(1.0),
    };
    let (ink, acc, hot, warn, ok) = (k.ink, k.accent, k.hot, k.warn, k.ok);
    let tint = |c: Color32, a: u8| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
    let soft = tint(acc, 70);
    let p = &pen;
    use Icon::*;
    match icon {
        Select => {
            p.fill(&[(4.0, 2.0), (4.0, 13.0), (7.0, 10.0)], ink);
            p.fill(&[(4.0, 2.0), (7.0, 10.0), (12.0, 9.5)], ink);
            p.line(&[(7.0, 10.0), (9.5, 14.5)], ink);
        }
        Line => {
            p.line(&[(2.5, 13.0), (13.5, 3.0)], acc);
            p.disc(2.5, 13.0, 1.3, hot);
            p.disc(13.5, 3.0, 1.3, hot);
        }
        CircleCenter => {
            p.circle(8.0, 8.0, 6.0, acc);
            p.disc(8.0, 8.0, 1.2, hot);
            p.dashed((8.0, 8.0), (12.2, 3.8), ink);
        }
        CircleTangent => {
            p.line(&[(1.0, 14.0), (15.0, 14.0)], ink);
            p.line(&[(1.5, 2.0), (8.0, 15.0)], ink);
            p.circle(8.6, 9.2, 4.6, acc);
        }
        ArcThree => {
            p.arc(8.0, 11.0, 6.0, 0.0, 180.0, acc);
            for (x, y) in [(2.0, 11.0), (8.0, 5.0), (14.0, 11.0)] {
                p.disc(x, y, 1.2, hot);
            }
        }
        ArcTangent => {
            p.line(&[(1.0, 13.0), (7.0, 13.0)], ink);
            p.arc(7.0, 7.5, 5.5, -90.0, 90.0, acc);
            p.disc(7.0, 13.0, 1.2, hot);
        }
        ArcCenter => {
            p.arc(6.0, 10.0, 7.0, 0.0, 90.0, acc);
            p.dashed((6.0, 10.0), (13.0, 10.0), ink);
            p.dashed((6.0, 10.0), (6.0, 3.0), ink);
            p.disc(6.0, 10.0, 1.2, hot);
        }
        Rect2 => {
            p.rect(2.5, 4.0, 13.5, 12.5, acc);
            p.disc(2.5, 12.5, 1.3, hot);
            p.disc(13.5, 4.0, 1.3, hot);
        }
        Rect3 => {
            p.closed(&[(2.0, 10.0), (9.0, 3.0), (14.0, 8.0), (7.0, 15.0)], acc);
            for (x, y) in [(2.0, 10.0), (9.0, 3.0), (14.0, 8.0)] {
                p.disc(x, y, 1.2, hot);
            }
        }
        RectCenter => {
            p.rect(2.5, 4.0, 13.5, 12.5, acc);
            p.dashed((2.5, 12.5), (13.5, 4.0), ink);
            p.disc(8.0, 8.25, 1.3, hot);
            p.disc(13.5, 4.0, 1.2, hot);
        }
        RectCenter3 => {
            p.closed(&[(2.0, 10.0), (9.0, 3.0), (14.0, 8.0), (7.0, 15.0)], acc);
            p.disc(8.0, 9.0, 1.3, hot);
            p.dashed((8.0, 9.0), (11.5, 5.5), ink);
        }
        SlotCenter | SlotOverall | SlotPoint => {
            p.line(&[(5.0, 5.0), (11.0, 5.0)], acc);
            p.line(&[(5.0, 11.0), (11.0, 11.0)], acc);
            p.arc(5.0, 8.0, 3.0, 90.0, 270.0, acc);
            p.arc(11.0, 8.0, 3.0, -90.0, 90.0, acc);
            match icon {
                SlotCenter => {
                    p.disc(5.0, 8.0, 1.1, hot);
                    p.disc(11.0, 8.0, 1.1, hot);
                }
                SlotOverall => {
                    p.disc(2.0, 8.0, 1.1, hot);
                    p.disc(14.0, 8.0, 1.1, hot);
                }
                _ => p.disc(8.0, 8.0, 1.2, hot),
            }
        }
        SlotArc => {
            p.arc(8.0, 14.0, 10.0, 35.0, 145.0, acc);
            p.arc(8.0, 14.0, 5.0, 35.0, 145.0, acc);
            p.arc(8.0, 14.0, 7.5, 60.0, 120.0, ink);
            p.disc(8.0, 14.0, 1.1, hot);
        }
        Spline => {
            let pts: Vec<(f32, f32)> = (0..=24)
                .map(|i| {
                    let t = i as f32 / 24.0;
                    (
                        2.0 + 12.0 * t,
                        9.0 - 5.0 * (t * std::f32::consts::TAU).sin(),
                    )
                })
                .collect();
            p.line(&pts, acc);
            p.dashed((2.0, 9.0), (5.0, 2.5), ink);
            p.dashed((11.0, 15.5), (14.0, 9.0), ink);
            p.disc(5.0, 2.5, 1.0, hot);
            p.disc(11.0, 15.5, 1.0, hot);
        }
        Ellipse => {
            let pts: Vec<(f32, f32)> = (0..=36)
                .map(|i| {
                    let a = i as f32 / 36.0 * std::f32::consts::TAU;
                    (8.0 + 6.5 * a.cos(), 8.0 + 4.0 * a.sin())
                })
                .collect();
            p.line(&pts, acc);
            p.disc(8.0, 8.0, 1.1, hot);
        }
        Point => {
            p.line(&[(8.0, 3.0), (8.0, 13.0)], ink);
            p.line(&[(3.0, 8.0), (13.0, 8.0)], ink);
            p.disc(8.0, 8.0, 2.0, hot);
        }
        Polygon => {
            let pts: Vec<(f32, f32)> = (0..6)
                .map(|i| {
                    let a = (i as f32 * 60.0 + 30.0).to_radians();
                    (8.0 + 6.0 * a.cos(), 8.0 + 6.0 * a.sin())
                })
                .collect();
            p.closed(&pts, acc);
            p.disc(8.0, 8.0, 1.1, hot);
        }
        Fillet2d => {
            p.line(&[(2.0, 14.0), (2.0, 8.0)], ink);
            p.line(&[(8.0, 2.0), (14.0, 2.0)], ink);
            p.arc(8.0, 8.0, 6.0, 90.0, 180.0, acc);
            p.dashed((2.0, 8.0), (2.0, 2.0), tint(ink, 120));
            p.dashed((2.0, 2.0), (8.0, 2.0), tint(ink, 120));
        }
        Chamfer2d => {
            p.line(&[(2.0, 14.0), (2.0, 8.0)], ink);
            p.line(&[(8.0, 2.0), (14.0, 2.0)], ink);
            p.line(&[(2.0, 8.0), (8.0, 2.0)], acc);
            p.dashed((2.0, 8.0), (2.0, 2.0), tint(ink, 120));
            p.dashed((2.0, 2.0), (8.0, 2.0), tint(ink, 120));
        }
        Text => {
            p.text(8.0, 8.5, "A", 13.0, acc);
            p.line(&[(2.0, 14.5), (14.0, 14.5)], ink);
        }
        Move => {
            p.rect(2.0, 8.0, 7.0, 13.0, tint(ink, 140));
            p.rect(9.0, 3.0, 14.0, 8.0, acc);
            p.line(&[(5.0, 9.0), (10.5, 5.5)], hot);
            p.head((11.0, 5.0), (1.0, -0.6), hot);
        }
        Copy => {
            p.rect(2.0, 8.0, 7.0, 13.0, acc);
            p.rect(9.0, 3.0, 14.0, 8.0, acc);
            p.text(11.5, 13.0, "+", 7.0, hot);
        }
        Rotate => {
            p.closed(
                &[(8.0, 8.0), (14.0, 8.0), (14.0, 11.0), (8.0, 11.0)],
                tint(ink, 140),
            );
            p.closed(&[(8.0, 8.0), (12.2, 3.8), (14.3, 5.9), (10.1, 10.1)], acc);
            p.arc(8.0, 8.0, 5.0, 95.0, 175.0, hot);
            p.disc(8.0, 8.0, 1.2, hot);
        }
        Trim => {
            p.line(&[(8.0, 1.5), (8.0, 14.5)], ink);
            p.line(&[(1.5, 8.0), (8.0, 8.0)], acc);
            p.dashed((8.0, 8.0), (14.5, 8.0), warn);
            p.line(&[(10.5, 5.5), (13.5, 10.5)], warn);
            p.line(&[(13.5, 5.5), (10.5, 10.5)], warn);
        }
        Extend => {
            p.line(&[(14.0, 1.5), (14.0, 14.5)], ink);
            p.line(&[(2.0, 8.0), (7.0, 8.0)], acc);
            p.dashed((7.0, 8.0), (13.0, 8.0), acc);
            p.head((13.8, 8.0), (1.0, 0.0), hot);
        }
        Split => {
            p.line(&[(8.0, 1.5), (8.0, 14.5)], ink);
            p.line(&[(1.5, 8.0), (7.0, 8.0)], acc);
            p.line(&[(9.0, 8.0), (14.5, 8.0)], acc);
            p.disc(8.0, 8.0, 1.4, hot);
        }
        Offset => {
            p.line(&[(2.0, 14.0), (2.0, 5.0), (11.0, 5.0)], ink);
            p.line(&[(5.0, 14.0), (5.0, 8.0), (14.0, 8.0)], acc);
            p.head((5.0, 8.0), (1.0, 1.0), hot);
        }
        Scale => {
            p.rect(2.0, 9.0, 7.0, 14.0, tint(ink, 140));
            p.rect(2.0, 2.0, 14.0, 14.0, acc);
            p.line(&[(7.0, 9.0), (12.0, 4.0)], hot);
            p.head((12.5, 3.5), (1.0, -1.0), hot);
        }
        Stretch => {
            p.line(&[(2.0, 4.0), (9.0, 4.0)], ink);
            p.line(&[(2.0, 12.0), (9.0, 12.0)], ink);
            p.line(&[(2.0, 4.0), (2.0, 12.0)], ink);
            p.line(&[(9.0, 4.0), (14.0, 4.0)], acc);
            p.line(&[(9.0, 12.0), (14.0, 12.0)], acc);
            p.line(&[(14.0, 4.0), (14.0, 12.0)], acc);
            p.head((15.0, 8.0), (1.0, 0.0), hot);
        }
        PatternRect => {
            for (x, y) in [(2.0, 2.0), (9.0, 2.0), (2.0, 9.0), (9.0, 9.0)] {
                p.rect(
                    x,
                    y,
                    x + 5.0,
                    y + 5.0,
                    if (x, y) == (2.0, 9.0) { ink } else { acc },
                );
            }
        }
        PatternPath => {
            p.arc(8.0, 16.0, 12.0, 200.0, 340.0, ink);
            for (x, y) in [(2.8, 10.0), (8.0, 4.2), (13.2, 10.0)] {
                p.rect(x - 2.0, y - 2.0, x + 2.0, y + 2.0, acc);
            }
        }
        SplineInterp => {
            let path: Vec<(f32, f32)> = (0..=20)
                .map(|i| {
                    let t = i as f32 / 20.0;
                    (
                        2.0 + 12.0 * t,
                        9.0 - 5.0 * (t * std::f32::consts::TAU * 0.9).sin(),
                    )
                })
                .collect();
            p.line(&path, acc);
            for (x, y) in [(2.0, 9.0), (6.0, 4.4), (10.5, 12.0), (14.0, 9.6)] {
                p.disc(x, y, 1.3, hot);
            }
        }
        PatternPoints => {
            p.rect(1.5, 1.5, 6.0, 6.0, ink);
            for (x, y) in [(12.0, 4.0), (5.0, 12.0), (13.0, 13.0)] {
                p.disc(x, y, 1.4, hot);
                p.rect(x - 2.2, y - 2.2, x + 2.2, y + 2.2, acc);
            }
        }
        PatternCirc => {
            for i in 0..6 {
                let a = (i as f32 * 60.0).to_radians();
                p.circle(
                    8.0 + 5.0 * a.cos(),
                    8.0 + 5.0 * a.sin(),
                    1.8,
                    if i == 0 { ink } else { acc },
                );
            }
            p.disc(8.0, 8.0, 1.1, hot);
        }
        Mirror => {
            p.dashed((8.0, 1.0), (8.0, 15.0), hot);
            p.closed(&[(2.0, 4.0), (6.0, 7.0), (2.0, 12.0)], ink);
            p.closed(&[(14.0, 4.0), (10.0, 7.0), (14.0, 12.0)], acc);
        }
        Dimension => {
            p.line(&[(2.0, 4.0), (2.0, 13.0)], ink);
            p.line(&[(14.0, 4.0), (14.0, 13.0)], ink);
            p.line(&[(3.5, 10.0), (12.5, 10.0)], acc);
            p.head((2.2, 10.0), (-1.0, 0.0), acc);
            p.head((13.8, 10.0), (1.0, 0.0), acc);
            p.text(8.0, 6.0, "D", 7.5, hot);
        }
        Coincident => {
            p.line(&[(1.5, 13.0), (8.0, 8.0)], acc);
            p.line(&[(8.0, 8.0), (14.5, 12.0)], ink);
            p.disc(8.0, 8.0, 2.0, hot);
        }
        Collinear => {
            p.line(&[(1.0, 12.0), (7.0, 8.0)], acc);
            p.line(&[(9.0, 6.7), (15.0, 2.7)], acc);
            p.dashed((7.0, 8.0), (9.0, 6.7), ink);
        }
        Concentric => {
            p.circle(8.0, 8.0, 6.0, acc);
            p.circle(8.0, 8.0, 3.0, ink);
            p.disc(8.0, 8.0, 1.1, hot);
        }
        Fix => {
            p.line(&[(2.0, 12.0), (14.0, 12.0)], acc);
            p.fill(&[(8.0, 12.0), (5.5, 4.5), (10.5, 4.5)], hot);
            p.line(&[(4.0, 4.5), (12.0, 4.5)], ink);
        }
        Parallel => {
            p.line(&[(2.0, 12.0), (10.0, 3.0)], acc);
            p.line(&[(6.0, 14.0), (14.0, 5.0)], acc);
        }
        Perpendicular => {
            p.line(&[(2.0, 13.0), (14.0, 13.0)], acc);
            p.line(&[(6.0, 13.0), (6.0, 2.0)], acc);
            p.line(&[(6.0, 10.0), (9.0, 10.0), (9.0, 13.0)], ink);
        }
        Horizontal => {
            p.line(&[(2.0, 8.0), (14.0, 8.0)], acc);
            p.text(8.0, 4.0, "H", 6.5, ink);
        }
        Vertical => {
            p.line(&[(8.0, 2.0), (8.0, 14.0)], acc);
            p.text(12.0, 8.0, "V", 6.5, ink);
        }
        Tangent => {
            p.circle(8.0, 9.5, 4.5, ink);
            p.line(&[(1.0, 5.0), (15.0, 5.0)], acc);
            p.disc(8.0, 5.0, 1.2, hot);
        }
        Smooth => {
            p.arc(4.0, 12.0, 6.0, 0.0, 90.0, ink);
            p.arc(12.0, 12.0, 6.0, 90.0, 180.0, acc);
            p.text(12.5, 13.0, "G2", 5.0, hot);
        }
        Symmetric => {
            p.dashed((8.0, 1.0), (8.0, 15.0), hot);
            p.line(&[(2.0, 4.0), (5.0, 12.0)], acc);
            p.line(&[(14.0, 4.0), (11.0, 12.0)], acc);
        }
        Equal => {
            p.line(&[(3.0, 6.0), (13.0, 6.0)], acc);
            p.line(&[(3.0, 10.0), (13.0, 10.0)], acc);
        }
        Relax => {
            p.line(
                &[
                    (2.0, 8.0),
                    (4.0, 5.0),
                    (6.0, 11.0),
                    (8.0, 5.0),
                    (10.0, 11.0),
                    (12.0, 5.0),
                    (14.0, 8.0),
                ],
                acc,
            );
            p.disc(14.0, 8.0, 1.2, hot);
        }
        Dof => {
            p.line(&[(8.0, 2.5), (8.0, 13.5)], acc);
            p.line(&[(2.5, 8.0), (13.5, 8.0)], acc);
            for (t, d) in [
                ((8.0, 2.0), (0.0, -1.0)),
                ((8.0, 14.0), (0.0, 1.0)),
                ((2.0, 8.0), (-1.0, 0.0)),
                ((14.0, 8.0), (1.0, 0.0)),
            ] {
                p.head(t, d, acc);
            }
            p.disc(8.0, 8.0, 1.3, hot);
        }
        ShowConstraints => {
            p.rect(2.0, 3.0, 9.0, 9.0, ink);
            p.text(5.5, 6.0, "H", 5.5, acc);
            p.rect(7.0, 8.0, 14.0, 14.0, ink);
            p.text(10.5, 11.0, "//", 5.0, acc);
        }
        CoordSystem => {
            p.line(&[(3.0, 13.0), (13.0, 13.0)], warn);
            p.head((14.0, 13.0), (1.0, 0.0), warn);
            p.line(&[(3.0, 13.0), (3.0, 3.0)], ok);
            p.head((3.0, 2.0), (0.0, -1.0), ok);
            p.disc(3.0, 13.0, 1.3, hot);
        }
        Construction => {
            p.dashed((2.0, 13.0), (14.0, 3.0), acc);
            p.disc(2.0, 13.0, 1.1, hot);
        }
        Centerline => {
            let pts = [
                (2.0, 13.0),
                (5.6, 10.0),
                (6.8, 9.0),
                (7.4, 8.5),
                (8.6, 7.5),
                (12.2, 4.5),
            ];
            p.line(&pts[0..2], acc);
            p.line(&pts[2..4], acc);
            p.line(&pts[4..6], acc);
        }
        Normal => {
            p.line(&[(2.0, 13.0), (14.0, 3.0)], acc);
        }
        Demo => {
            p.rect(2.0, 2.0, 14.0, 14.0, ink);
            p.circle(8.0, 8.0, 3.5, acc);
            p.text(8.0, 8.0, "?", 6.0, hot);
        }
        Finish => {
            p.circle(8.0, 8.0, 6.5, ok);
            p.line(&[(4.5, 8.0), (7.0, 10.8), (11.8, 5.0)], ok);
        }
        StartSketch => {
            p.closed(&[(1.0, 11.0), (7.0, 6.0), (15.0, 6.0), (9.0, 11.0)], ink);
            p.line(&[(4.0, 10.0), (11.0, 7.0)], acc);
            p.line(
                &[
                    (11.0, 1.0),
                    (14.0, 4.0),
                    (8.0, 10.0),
                    (6.0, 10.5),
                    (6.5, 8.5),
                    (11.0, 1.0),
                ],
                hot,
            );
        }
        Extrude => {
            p.block([2.0, 7.0, 9.0, 6.0, 5.0], ink, soft);
            p.line(&[(8.0, 6.0), (8.0, 1.5)], hot);
            p.head((8.0, 1.0), (0.0, -1.0), hot);
        }
        Revolve => {
            p.dashed((3.0, 1.0), (3.0, 15.0), hot);
            p.arc(3.0, 8.0, 10.0, -50.0, 50.0, acc);
            p.closed(&[(5.0, 4.0), (10.0, 4.0), (10.0, 12.0), (5.0, 12.0)], ink);
        }
        Sweep => {
            let path: Vec<(f32, f32)> = (0..=16)
                .map(|i| {
                    let t = i as f32 / 16.0;
                    (2.0 + 12.0 * t, 12.0 - 8.0 * t * t)
                })
                .collect();
            p.line(&path, hot);
            p.circle(2.5, 12.0, 2.2, acc);
            p.circle(13.5, 4.0, 2.2, acc);
        }
        Loft => {
            p.rect(1.5, 9.0, 7.5, 14.0, acc);
            p.circle(12.0, 5.0, 3.0, acc);
            p.line(&[(7.5, 9.0), (10.0, 2.8)], ink);
            p.line(&[(7.5, 14.0), (14.0, 7.5)], ink);
        }
        Coil => {
            for i in 0..4 {
                let y = 3.0 + i as f32 * 3.2;
                p.arc(8.0, y + 1.6, 5.0, 200.0, 340.0, acc);
            }
            p.dashed((8.0, 1.0), (8.0, 15.0), hot);
        }
        Rib => {
            p.line(&[(2.0, 14.0), (2.0, 2.0)], ink);
            p.line(&[(2.0, 14.0), (14.0, 14.0)], ink);
            p.fill(&[(2.0, 5.0), (11.0, 14.0), (2.0, 14.0)], soft);
            p.line(&[(2.0, 5.0), (11.0, 14.0)], acc);
        }
        Emboss => {
            p.rect(1.5, 4.0, 14.5, 13.0, ink);
            p.text(8.0, 8.5, "E", 9.0, acc);
        }
        Hole => {
            p.block([2.0, 7.0, 10.0, 6.0, 4.0], ink, tint(ink, 40));
            let pts: Vec<(f32, f32)> = (0..=24)
                .map(|i| {
                    let a = i as f32 / 24.0 * std::f32::consts::TAU;
                    (8.6 + 2.6 * a.cos(), 5.6 + 1.2 * a.sin())
                })
                .collect();
            p.line(&pts, acc);
        }
        SlotFeature => {
            p.block([1.5, 7.0, 11.0, 6.0, 4.0], ink, tint(ink, 40));
            p.line(&[(5.0, 4.6), (10.0, 4.6)], acc);
            p.line(&[(5.0, 6.6), (10.0, 6.6)], acc);
        }
        FilletEdge => {
            p.line(&[(2.0, 14.0), (2.0, 8.0)], ink);
            p.line(&[(8.0, 2.0), (14.0, 2.0)], ink);
            p.fill(
                &[
                    (2.0, 14.0),
                    (2.0, 8.0),
                    (3.0, 5.0),
                    (5.0, 3.0),
                    (8.0, 2.0),
                    (14.0, 2.0),
                    (14.0, 14.0),
                ],
                soft,
            );
            p.arc(8.0, 8.0, 6.0, 90.0, 180.0, acc);
        }
        ChamferEdge => {
            p.fill(
                &[
                    (2.0, 14.0),
                    (2.0, 8.0),
                    (8.0, 2.0),
                    (14.0, 2.0),
                    (14.0, 14.0),
                ],
                soft,
            );
            p.line(&[(2.0, 14.0), (2.0, 8.0), (8.0, 2.0), (14.0, 2.0)], ink);
            p.line(&[(2.0, 8.0), (8.0, 2.0)], acc);
        }
        Shell => {
            p.block([2.0, 6.0, 10.0, 7.0, 4.0], ink, soft);
            p.closed(&[(3.6, 5.6), (5.0, 4.1), (12.4, 4.1), (11.0, 5.6)], acc);
        }
        Draft => {
            p.line(&[(1.0, 14.0), (15.0, 14.0)], ink);
            p.fill(&[(4.0, 14.0), (6.0, 3.0), (10.0, 3.0), (12.0, 14.0)], soft);
            p.closed(&[(4.0, 14.0), (6.0, 3.0), (10.0, 3.0), (12.0, 14.0)], acc);
        }
        Thread => {
            p.rect(5.0, 2.0, 11.0, 14.0, ink);
            for i in 0..5 {
                let y = 3.5 + i as f32 * 2.3;
                p.line(&[(5.0, y + 1.2), (11.0, y)], acc);
            }
        }
        Combine => {
            p.circle(6.0, 8.0, 4.5, acc);
            p.circle(10.0, 8.0, 4.5, ink);
        }
        SplitBody => {
            p.block([2.0, 7.0, 10.0, 6.0, 4.0], ink, tint(ink, 40));
            p.dashed((8.0, 1.0), (7.0, 15.0), warn);
        }
        WorkPlane => {
            p.fill(
                &[(1.0, 11.0), (6.0, 5.0), (15.0, 5.0), (10.0, 11.0)],
                tint(hot, 60),
            );
            p.closed(&[(1.0, 11.0), (6.0, 5.0), (15.0, 5.0), (10.0, 11.0)], hot);
        }
        WorkAxis => {
            p.line(&[(2.0, 14.0), (14.0, 2.0)], hot);
            p.disc(8.0, 8.0, 1.2, hot);
        }
        WorkPoint => {
            p.disc(8.0, 8.0, 2.4, hot);
            p.circle(8.0, 8.0, 4.5, ink);
        }
        Ucs => {
            p.line(&[(4.0, 12.0), (14.0, 12.0)], warn);
            p.line(&[(4.0, 12.0), (4.0, 2.0)], ok);
            p.line(&[(4.0, 12.0), (0.5, 15.5)], acc);
        }
        Box => {
            p.block([2.0, 6.0, 9.0, 8.0, 5.0], ink, soft);
        }
        Cylinder | Sphere | Torus => {
            // An ellipse around (x, y) with half axes a, b.
            let ellipse = |x: f32, y: f32, a: f32, b: f32| -> Vec<(f32, f32)> {
                (0..24)
                    .map(|k| {
                        let t = k as f32 / 24.0 * std::f32::consts::TAU;
                        (x + a * t.cos(), y + b * t.sin())
                    })
                    .collect()
            };
            match icon {
                Cylinder => {
                    p.fill(&[(3.0, 4.0), (13.0, 4.0), (13.0, 12.0), (3.0, 12.0)], soft);
                    p.line(&[(3.0, 4.0), (3.0, 12.0)], ink);
                    p.line(&[(13.0, 4.0), (13.0, 12.0)], ink);
                    p.closed(&ellipse(8.0, 4.0, 5.0, 2.0), acc);
                    p.line(&ellipse(8.0, 12.0, 5.0, 2.0)[..13], ink);
                }
                Sphere => {
                    p.fill(&ellipse(8.0, 8.0, 6.0, 6.0), soft);
                    p.circle(8.0, 8.0, 6.0, ink);
                    p.closed(&ellipse(8.0, 8.0, 6.0, 2.0), acc);
                }
                _ => {
                    p.fill(&ellipse(8.0, 8.0, 7.0, 4.0), soft);
                    p.closed(&ellipse(8.0, 8.0, 7.0, 4.0), ink);
                    p.closed(&ellipse(8.0, 7.5, 2.5, 1.2), acc);
                }
            }
        }
        Measure => {
            p.closed(&[(1.0, 10.0), (11.0, 2.0), (15.0, 6.0), (5.0, 14.0)], ink);
            for i in 0..5 {
                let t = 0.15 + i as f32 * 0.17;
                let (x, y) = (1.0 + 10.0 * t, 10.0 - 8.0 * t);
                p.line(&[(x, y), (x + 1.5, y + 1.8)], acc);
            }
        }
        Mass => {
            p.fill(&[(3.0, 14.0), (5.0, 6.0), (11.0, 6.0), (13.0, 14.0)], soft);
            p.closed(&[(3.0, 14.0), (5.0, 6.0), (11.0, 6.0), (13.0, 14.0)], acc);
            p.circle(8.0, 4.0, 2.0, ink);
        }
        Validate => {
            p.block([2.0, 7.0, 8.0, 6.0, 4.0], ink, tint(ink, 40));
            p.line(&[(9.0, 11.0), (11.0, 13.5), (15.0, 8.0)], ok);
        }
        Interference => {
            p.rect(2.0, 2.0, 10.0, 10.0, ink);
            p.rect(6.0, 6.0, 14.0, 14.0, ink);
            p.fill(&[(6.0, 6.0), (10.0, 6.0), (10.0, 10.0), (6.0, 10.0)], warn);
        }
        Curvature => {
            p.arc(8.0, 16.0, 11.0, 30.0, 150.0, acc);
            for i in 0..5 {
                let a = (45.0 + i as f32 * 22.5f32).to_radians();
                let (x, y) = (8.0 + 11.0 * a.cos(), 16.0 - 11.0 * a.sin());
                p.line(&[(x, y), (x + 3.0 * a.cos(), y - 3.0 * a.sin())], hot);
            }
        }
        Options => {
            for (y, x) in [(4.0, 10.0), (8.0, 5.0), (12.0, 9.0)] {
                p.line(&[(2.0, y), (14.0, y)], ink);
                p.disc(x, y, 1.6, acc);
            }
        }
        Settings => {
            p.rect(3.0, 2.0, 13.0, 14.0, ink);
            for y in [5.0, 8.0, 11.0] {
                p.line(&[(5.0, y), (11.0, y)], acc);
            }
        }
        Parameters => {
            p.text(8.0, 8.0, "fx", 10.0, acc);
        }
        Rules => {
            p.rect(2.0, 2.0, 14.0, 14.0, ink);
            p.text(8.0, 8.0, "{ }", 7.0, acc);
        }
        Assistant => {
            p.circle(8.0, 7.0, 5.0, acc);
            p.fill(&[(5.0, 11.0), (4.0, 15.0), (8.0, 12.0)], acc);
            p.disc(6.0, 7.0, 0.9, hot);
            p.disc(10.0, 7.0, 0.9, hot);
        }
        Rebuild => {
            p.rect(5.5, 5.5, 10.5, 10.5, ink);
            p.arc(8.0, 8.0, 6.5, 20.0, 160.0, acc);
            p.head((2.4, 6.0), (0.0, 1.0), acc);
            p.arc(8.0, 8.0, 6.5, 200.0, 340.0, acc);
            p.head((13.6, 10.0), (0.0, -1.0), acc);
        }
        Undo => {
            p.arc(9.0, 9.0, 4.5, -90.0, 180.0, acc);
            p.head((4.5, 10.5), (0.0, 1.0), acc);
        }
        Redo => {
            p.arc(7.0, 9.0, 4.5, 0.0, 270.0, acc);
            p.head((11.5, 10.5), (0.0, 1.0), acc);
        }
        Home => {
            p.closed(&[(2.0, 8.0), (8.0, 2.5), (14.0, 8.0)], acc);
            p.rect(4.0, 8.0, 12.0, 14.0, ink);
        }
        LookAt => {
            p.closed(
                &[
                    (1.0, 8.0),
                    (5.0, 4.0),
                    (11.0, 4.0),
                    (15.0, 8.0),
                    (11.0, 12.0),
                    (5.0, 12.0),
                ],
                ink,
            );
            p.circle(8.0, 8.0, 2.6, acc);
            p.disc(8.0, 8.0, 1.0, acc);
        }
        ViewFront | ViewTop | ViewRight => {
            p.block([3.0, 6.0, 8.0, 8.0, 4.0], ink, Color32::TRANSPARENT);
            let face: [(f32, f32); 4] = match icon {
                ViewFront => [(3.0, 6.0), (11.0, 6.0), (11.0, 14.0), (3.0, 14.0)],
                ViewTop => [(3.0, 6.0), (6.2, 4.0), (14.2, 4.0), (11.0, 6.0)],
                _ => [(11.0, 6.0), (14.2, 4.0), (14.2, 12.0), (11.0, 14.0)],
            };
            p.fill(&face, tint(acc, 150));
        }
        VisualStyle => {
            p.circle(8.0, 8.0, 6.0, ink);
            p.fill(
                &[
                    (8.0, 2.0),
                    (12.2, 3.8),
                    (14.0, 8.0),
                    (12.2, 12.2),
                    (8.0, 14.0),
                ],
                soft,
            );
        }
        Material => {
            p.disc(8.0, 8.0, 6.0, tint(ink, 60));
            p.circle(8.0, 8.0, 6.0, ink);
            p.arc(8.0, 13.0, 6.5, 40.0, 140.0, acc);
            p.disc(6.0, 5.5, 1.2, ink);
        }
        Appearance => {
            p.circle(8.0, 8.0, 6.0, ink);
            p.disc(5.5, 6.0, 1.7, warn);
            p.disc(10.5, 6.0, 1.7, acc);
            p.disc(8.0, 10.8, 1.7, ok);
        }
        AdjustLook => {
            p.fill(&[(2.0, 10.0), (9.0, 13.0), (14.0, 10.0), (7.0, 7.0)], soft);
            p.closed(&[(2.0, 10.0), (9.0, 13.0), (14.0, 10.0), (7.0, 7.0)], ink);
            p.line(&[(9.0, 9.0), (13.5, 1.5)], hot);
            p.disc(9.0, 9.0, 1.4, acc);
        }
        ClearLook => {
            p.circle(7.0, 8.0, 5.5, ink);
            p.disc(5.0, 6.5, 1.4, acc);
            p.line(&[(10.0, 10.0), (15.0, 15.0)], warn);
            p.line(&[(15.0, 10.0), (10.0, 15.0)], warn);
        }
        Styles => {
            p.rect(5.0, 2.0, 14.0, 11.0, ink);
            p.fill(&[(2.0, 5.0), (11.0, 5.0), (11.0, 14.0), (2.0, 14.0)], soft);
            p.rect(2.0, 5.0, 11.0, 14.0, acc);
            p.line(&[(4.0, 9.0), (9.0, 9.0)], acc);
        }
        Purge => {
            p.line(&[(3.0, 4.0), (13.0, 4.0)], ink);
            p.line(&[(6.5, 4.0), (6.5, 2.5), (9.5, 2.5), (9.5, 4.0)], ink);
            p.closed(&[(4.0, 5.0), (12.0, 5.0), (11.0, 14.0), (5.0, 14.0)], ink);
            p.line(&[(8.0, 7.0), (8.0, 12.0)], warn);
        }
        Section => {
            p.block([2.0, 6.0, 9.0, 8.0, 5.0], ink, Color32::TRANSPARENT);
            p.fill(
                &[(6.5, 6.0), (10.5, 3.5), (10.5, 11.5), (6.5, 14.0)],
                tint(warn, 140),
            );
        }
        Ortho => {
            p.rect(3.0, 3.0, 13.0, 13.0, ink);
            p.line(&[(3.0, 8.0), (13.0, 8.0)], acc);
            p.line(&[(8.0, 3.0), (8.0, 13.0)], acc);
        }
        Part => {
            p.block([2.0, 6.0, 9.0, 8.0, 5.0], ink, soft);
        }
        Origin => {
            p.line(&[(4.0, 12.0), (14.0, 12.0)], warn);
            p.line(&[(4.0, 12.0), (4.0, 2.0)], ok);
            p.line(&[(4.0, 12.0), (1.0, 15.0)], acc);
        }
        Sketch => {
            p.closed(&[(1.0, 12.0), (5.0, 4.0), (15.0, 4.0), (11.0, 12.0)], ink);
            p.line(&[(4.5, 10.0), (11.5, 6.0)], acc);
            p.circle(10.0, 9.0, 1.6, acc);
        }
        Project => {
            p.rect(2.0, 3.0, 14.0, 13.0, ink);
            p.block([4.0, 8.0, 5.0, 3.5, 3.0], acc, soft);
        }
        SheetMetal => {
            p.line(&[(2.0, 4.0), (2.0, 13.0), (12.0, 13.0)], ink);
            p.line(&[(4.0, 4.0), (4.0, 11.0), (12.0, 11.0)], acc);
            p.arc(4.0, 11.0, 2.0, 90.0, 180.0, hot);
        }
        SmFace => {
            let plate = [(1.5, 10.0), (6.0, 5.0), (14.5, 5.0), (10.0, 10.0)];
            p.fill(&plate, soft);
            p.closed(&plate, acc);
            p.line(&[(1.5, 10.0), (1.5, 11.5), (10.0, 11.5), (10.0, 10.0)], ink);
            p.line(&[(10.0, 11.5), (14.5, 6.5), (14.5, 5.0)], ink);
        }
        SmFlange => {
            p.line(&[(1.5, 13.5), (9.0, 13.5)], ink);
            p.line(&[(1.5, 11.5), (9.0, 11.5)], ink);
            p.arc(9.0, 9.5, 4.0, -90.0, 0.0, acc);
            p.arc(9.0, 9.5, 2.0, -90.0, 0.0, acc);
            p.line(&[(13.0, 9.5), (13.0, 2.0), (11.0, 2.0), (11.0, 9.5)], acc);
            p.head((13.0, 1.5), (0.0, -1.0), hot);
        }
        SmFlatPattern => {
            p.closed(
                &[
                    (5.0, 1.5),
                    (11.0, 1.5),
                    (11.0, 5.0),
                    (14.5, 5.0),
                    (14.5, 11.0),
                    (11.0, 11.0),
                    (11.0, 14.5),
                    (5.0, 14.5),
                    (5.0, 11.0),
                    (1.5, 11.0),
                    (1.5, 5.0),
                    (5.0, 5.0),
                ],
                ink,
            );
            p.dashed((5.0, 5.0), (11.0, 5.0), hot);
            p.dashed((5.0, 11.0), (11.0, 11.0), hot);
            p.dashed((5.0, 5.0), (5.0, 11.0), acc);
            p.dashed((11.0, 5.0), (11.0, 11.0), acc);
        }
        SmContour => {
            p.line(&[(1.5, 13.0), (5.5, 13.0)], ink);
            p.arc(5.5, 11.0, 2.0, -90.0, 0.0, acc);
            p.line(&[(7.5, 11.0), (7.5, 6.0)], ink);
            p.arc(9.5, 6.0, 2.0, 90.0, 180.0, acc);
            p.line(&[(9.5, 4.0), (14.5, 4.0)], ink);
            p.disc(1.5, 13.0, 1.0, hot);
            p.disc(14.5, 4.0, 1.0, hot);
        }
        SmHem => {
            p.line(&[(1.5, 12.0), (11.0, 12.0)], ink);
            p.arc(11.0, 9.5, 2.5, -90.0, 90.0, acc);
            p.line(&[(11.0, 7.0), (5.0, 7.0)], acc);
            p.line(&[(1.5, 14.0), (11.0, 14.0)], tint(ink, 120));
        }
        SmFold => {
            p.closed(&[(1.5, 13.0), (8.0, 13.0), (8.0, 9.0), (1.5, 9.0)], ink);
            p.closed(&[(8.0, 13.0), (13.5, 7.0), (13.5, 3.0), (8.0, 9.0)], acc);
            p.dashed((8.0, 14.5), (8.0, 7.5), hot);
        }
        SmBend => {
            p.line(&[(1.5, 4.0), (8.0, 4.0)], ink);
            p.line(&[(8.0, 12.0), (14.5, 12.0)], ink);
            p.line(&[(8.0, 4.0), (10.0, 4.5), (11.5, 6.0), (12.0, 8.0)], acc);
            p.line(&[(12.0, 8.0), (12.0, 10.0)], acc);
            p.arc(10.0, 10.0, 2.0, -90.0, 0.0, acc);
            p.disc(8.0, 4.0, 1.0, hot);
            p.disc(8.0, 12.0, 1.0, hot);
        }
        SmContourRoll => {
            p.arc(8.0, 9.0, 6.0, 0.0, 180.0, acc);
            p.arc(8.0, 9.0, 4.5, 0.0, 180.0, ink);
            p.dashed((8.0, 15.0), (8.0, 1.5), hot);
        }
        SmLoftedFlange => {
            p.rect(4.0, 1.5, 12.0, 5.5, ink);
            p.circle(8.0, 12.0, 3.0, acc);
            p.line(&[(4.0, 5.5), (5.0, 12.0)], tint(acc, 160));
            p.line(&[(12.0, 5.5), (11.0, 12.0)], tint(acc, 160));
        }
        SmRip => {
            p.block([2.5, 6.0, 8.0, 7.5, 4.0], ink, soft);
            p.line(&[(10.5, 6.0), (10.5, 13.5)], hot);
            p.line(&[(11.3, 5.6), (11.3, 13.0)], hot);
        }
        SmCut => {
            let plate = [(1.5, 11.0), (5.5, 5.0), (14.5, 5.0), (10.5, 11.0)];
            p.fill(&plate, soft);
            p.closed(&plate, ink);
            p.closed(&[(5.5, 9.5), (7.5, 6.5), (10.5, 6.5), (8.5, 9.5)], hot);
        }
        SmCornerSeam => {
            p.line(&[(2.0, 2.0), (2.0, 14.0), (8.5, 14.0)], ink);
            p.line(&[(4.0, 2.0), (4.0, 12.0)], acc);
            p.line(&[(9.5, 12.0), (14.0, 12.0)], acc);
            p.line(&[(9.5, 14.0), (14.0, 14.0)], ink);
            p.line(&[(4.0, 12.0), (8.5, 12.0)], acc);
            p.line(&[(8.5, 11.0), (9.5, 15.0)], hot);
        }
        SmCornerRound => {
            p.line(&[(2.0, 14.0), (2.0, 2.0), (8.0, 2.0)], ink);
            p.arc(8.0, 8.0, 6.0, 0.0, 90.0, hot);
            p.line(&[(14.0, 8.0), (14.0, 14.0)], ink);
        }
        SmCornerChamfer => {
            p.line(&[(2.0, 14.0), (2.0, 2.0), (8.0, 2.0)], ink);
            p.line(&[(8.0, 2.0), (14.0, 8.0)], hot);
            p.line(&[(14.0, 8.0), (14.0, 14.0)], ink);
        }
        SmPunch => {
            let plate = [(1.5, 13.0), (5.0, 8.0), (14.5, 8.0), (11.0, 13.0)];
            p.fill(&plate, soft);
            p.closed(&plate, ink);
            p.line(&[(8.0, 1.5), (8.0, 8.5)], acc);
            p.head((8.0, 10.5), (0.0, 1.0), hot);
        }
        SmUnfold => {
            p.line(&[(1.5, 13.5), (14.5, 13.5)], ink);
            p.line(&[(4.0, 11.0), (4.0, 3.0)], tint(acc, 140));
            p.arc(4.0, 11.0, 7.0, 15.0, 80.0, acc);
            p.head((11.0, 9.0), (0.3, 1.0), hot);
        }
        SmRefold => {
            p.line(&[(1.5, 13.5), (8.0, 13.5)], ink);
            p.line(&[(8.0, 13.5), (8.0, 3.0)], acc);
            p.dashed((8.0, 13.5), (14.5, 13.5), tint(acc, 140));
            p.arc(8.0, 13.5, 6.0, 15.0, 75.0, hot);
            p.head((9.6, 7.7), (-0.4, -1.0), hot);
        }
        SmExportFlat => {
            p.closed(
                &[
                    (2.0, 4.0),
                    (8.0, 4.0),
                    (8.0, 7.0),
                    (10.5, 7.0),
                    (10.5, 14.0),
                    (2.0, 14.0),
                ],
                ink,
            );
            p.dashed((2.0, 7.0), (8.0, 7.0), acc);
            p.line(&[(9.0, 3.5), (14.0, 3.5)], hot);
            p.head((15.0, 3.5), (1.0, 0.0), hot);
        }
        Drawing => {
            p.rect(2.0, 2.0, 14.0, 14.0, ink);
            p.rect(4.0, 4.0, 9.0, 9.0, acc);
            p.line(&[(10.0, 12.0), (13.0, 12.0)], ink);
            p.line(&[(4.0, 12.0), (8.0, 12.0)], hot);
        }
        BaseView => {
            p.rect(1.5, 2.5, 14.5, 13.5, ink);
            p.block([4.0, 7.0, 6.0, 4.5, 3.0], acc, soft);
        }
        ProjectedView => {
            p.rect(1.5, 8.0, 7.5, 14.0, acc);
            p.rect(9.0, 2.0, 15.0, 8.0, ink);
            p.dashed((7.5, 8.0), (9.0, 8.0), hot);
            p.dashed((4.5, 8.0), (4.5, 5.0), hot);
            p.dashed((4.5, 5.0), (9.0, 5.0), hot);
        }
        AuxView => {
            p.rect(1.5, 8.0, 7.5, 14.0, ink);
            p.closed(&[(11.0, 1.5), (15.0, 5.5), (11.5, 9.0), (7.5, 5.0)], acc);
            p.line(&[(7.5, 8.0), (9.5, 7.0)], hot);
        }
        SectionView => {
            p.rect(2.5, 3.0, 13.5, 13.0, ink);
            for k in 0..4 {
                let x = 3.5 + k as f32 * 2.0;
                p.line(&[(x, 12.5), (x + 3.0, 9.0)], tint(acc, 200));
            }
            p.line(&[(1.0, 8.0), (15.0, 8.0)], hot);
            p.line(&[(1.0, 8.0), (1.0, 5.0)], hot);
            p.line(&[(15.0, 8.0), (15.0, 5.0)], hot);
        }
        DetailView => {
            p.circle(5.5, 5.5, 4.0, acc);
            p.rect(9.0, 9.0, 15.0, 15.0, ink);
            p.line(&[(8.3, 8.3), (9.0, 9.0)], hot);
            p.disc(5.5, 5.5, 1.0, hot);
        }
        BreakView => {
            p.rect(1.5, 5.0, 6.0, 11.0, ink);
            p.rect(10.0, 5.0, 14.5, 11.0, ink);
            p.line(&[(6.0, 4.0), (7.0, 6.5), (5.5, 9.5), (6.5, 12.0)], hot);
            p.line(&[(10.0, 4.0), (11.0, 6.5), (9.5, 9.5), (10.5, 12.0)], hot);
        }
        BreakOut => {
            p.rect(2.0, 2.0, 14.0, 14.0, ink);
            p.closed(
                &[
                    (5.0, 6.0),
                    (9.0, 4.5),
                    (12.0, 7.0),
                    (10.0, 11.5),
                    (5.5, 10.5),
                ],
                hot,
            );
            p.disc(8.5, 8.0, 1.2, acc);
        }
        SliceView => {
            p.rect(2.0, 3.0, 14.0, 13.0, ink);
            p.line(&[(1.0, 14.5), (15.0, 1.5)], hot);
        }
        CropView => {
            p.line(&[(4.0, 1.0), (4.0, 12.0), (15.0, 12.0)], ink);
            p.line(&[(1.0, 4.0), (12.0, 4.0), (12.0, 15.0)], acc);
        }
        DraftView => {
            p.rect(1.5, 2.5, 14.5, 13.5, ink);
            p.line(&[(4.0, 11.0), (11.0, 5.0)], acc);
            p.line(&[(11.0, 5.0), (12.5, 6.5)], hot);
        }
        NewSheet => {
            p.closed(
                &[
                    (3.0, 1.5),
                    (10.0, 1.5),
                    (13.0, 4.5),
                    (13.0, 14.5),
                    (3.0, 14.5),
                ],
                ink,
            );
            p.line(&[(8.0, 6.0), (8.0, 12.0)], ok);
            p.line(&[(5.0, 9.0), (11.0, 9.0)], ok);
        }
        CenterMark => {
            p.circle(8.0, 8.0, 4.5, ink);
            p.line(&[(1.0, 8.0), (15.0, 8.0)], acc);
            p.line(&[(8.0, 1.0), (8.0, 15.0)], acc);
        }
        CenterlineMark => {
            p.circle(4.0, 8.0, 2.5, ink);
            p.circle(12.0, 8.0, 2.5, ink);
            p.dashed((0.5, 8.0), (15.5, 8.0), acc);
        }
        Balloon => {
            p.circle(10.0, 6.0, 4.5, acc);
            p.text(10.0, 6.0, "1", 6.0, ink);
            p.line(&[(6.8, 9.2), (2.0, 14.0)], ink);
            p.disc(2.0, 14.0, 1.0, ink);
        }
        PartsList => {
            p.rect(1.5, 2.5, 14.5, 13.5, ink);
            for y in [5.5, 8.25, 11.0] {
                p.line(&[(1.5, y), (14.5, y)], ink);
            }
            p.line(&[(5.0, 2.5), (5.0, 13.5)], acc);
        }
        HoleNote => {
            p.circle(4.5, 11.0, 3.0, ink);
            p.line(&[(6.5, 9.0), (10.0, 5.0), (15.0, 5.0)], acc);
            p.text(12.0, 3.0, "Ø", 5.0, hot);
        }
        SurfaceSymbol => {
            p.line(&[(1.5, 8.0), (4.5, 13.5), (11.0, 2.0), (15.0, 2.0)], acc);
            p.line(&[(1.0, 14.5), (15.0, 14.5)], ink);
        }
        Datum => {
            p.rect(4.5, 1.5, 11.5, 8.5, ink);
            p.text(8.0, 5.0, "A", 6.0, acc);
            p.line(&[(8.0, 8.5), (8.0, 11.5)], ink);
            p.fill(&[(5.0, 15.0), (11.0, 15.0), (8.0, 11.5)], ink);
        }
        ControlFrame => {
            p.rect(1.0, 4.5, 15.0, 11.5, ink);
            p.line(&[(5.5, 4.5), (5.5, 11.5)], ink);
            p.line(&[(11.0, 4.5), (11.0, 11.5)], ink);
            p.circle(3.25, 8.0, 1.6, acc);
        }
        Folder => {
            p.fill(
                &[
                    (1.5, 4.0),
                    (6.0, 4.0),
                    (7.5, 5.5),
                    (14.5, 5.5),
                    (14.5, 13.0),
                    (1.5, 13.0),
                ],
                soft,
            );
            p.closed(
                &[
                    (1.5, 4.0),
                    (6.0, 4.0),
                    (7.5, 5.5),
                    (14.5, 5.5),
                    (14.5, 13.0),
                    (1.5, 13.0),
                ],
                ink,
            );
        }
        Assembly => {
            p.block([1.5, 9.0, 6.0, 5.0, 3.5], ink, soft);
            p.block([8.0, 4.5, 5.0, 4.5, 3.0], acc, soft);
        }
        Place => {
            p.block([2.0, 9.5, 7.0, 5.0, 3.0], ink, soft);
            p.line(&[(12.5, 1.5), (12.5, 7.0)], hot);
            p.head((12.5, 8.0), (0.0, 1.0), hot);
        }
        FreeMove => {
            p.block([3.5, 9.5, 6.0, 4.5, 3.0], acc, soft);
            p.line(&[(8.0, 1.5), (8.0, 5.0)], hot);
            p.head((8.0, 1.0), (0.0, -1.0), hot);
            p.line(&[(11.5, 9.0), (15.0, 9.0)], hot);
            p.head((15.5, 9.0), (1.0, 0.0), hot);
        }
        FreeRotate => {
            p.block([4.5, 8.0, 5.5, 4.5, 2.5], acc, soft);
            p.arc(8.0, 8.0, 6.5, 200.0, 340.0, hot);
            p.head((14.0, 6.0), (0.4, 1.0), hot);
        }
        Ground => {
            p.disc(8.0, 4.5, 3.0, warn);
            p.line(&[(8.0, 7.5), (8.0, 12.5)], ink);
            p.line(&[(2.0, 14.0), (14.0, 14.0)], ink);
        }
        // Direction: the sketch plane (a line) and where the feature goes.
        DirDefault => {
            p.line(&[(1.0, 12.0), (15.0, 12.0)], ink);
            p.line(&[(8.0, 11.0), (8.0, 3.0)], acc);
            p.head((8.0, 2.0), (0.0, -1.0), acc);
        }
        DirFlipped => {
            p.line(&[(1.0, 4.0), (15.0, 4.0)], ink);
            p.line(&[(8.0, 5.0), (8.0, 13.0)], acc);
            p.head((8.0, 14.0), (0.0, 1.0), acc);
        }
        DirSymmetric => {
            p.line(&[(1.0, 8.0), (15.0, 8.0)], ink);
            p.line(&[(8.0, 7.0), (8.0, 3.0)], acc);
            p.head((8.0, 2.0), (0.0, -1.0), acc);
            p.line(&[(8.0, 9.0), (8.0, 13.0)], acc);
            p.head((8.0, 14.0), (0.0, 1.0), acc);
        }
        DirAsymmetric => {
            p.line(&[(1.0, 10.0), (15.0, 10.0)], ink);
            p.line(&[(6.0, 9.0), (6.0, 3.0)], acc);
            p.head((6.0, 2.0), (0.0, -1.0), acc);
            p.line(&[(10.0, 11.0), (10.0, 13.0)], hot);
            p.head((10.0, 14.0), (0.0, 1.0), hot);
        }
        // Operations: the body (ink) and the feature (accent / warn).
        OpJoin => {
            p.block([1.5, 8.0, 8.0, 6.0, 3.0], ink, tint(ink, 50));
            p.block([6.0, 4.0, 5.0, 4.0, 3.0], acc, soft);
        }
        OpCut => {
            p.block([1.5, 7.0, 11.0, 7.0, 3.0], ink, tint(ink, 50));
            p.closed(&[(5.0, 7.0), (9.0, 7.0), (9.0, 11.0), (5.0, 11.0)], warn);
        }
        OpIntersect => {
            p.circle(6.0, 8.0, 4.5, ink);
            p.circle(10.0, 8.0, 4.5, ink);
            p.fill(
                &[
                    (8.0, 4.4),
                    (9.6, 6.0),
                    (9.6, 10.0),
                    (8.0, 11.6),
                    (6.4, 10.0),
                    (6.4, 6.0),
                ],
                soft,
            );
            p.line(&[(8.0, 4.4), (8.0, 11.6)], acc);
        }
        OpNewSolid => {
            p.block([1.5, 9.0, 5.0, 5.0, 2.5], ink, tint(ink, 50));
            p.block([9.0, 5.0, 5.0, 5.0, 2.5], acc, soft);
        }
        OutSolid => {
            p.block([2.0, 6.0, 9.0, 8.0, 5.0], acc, soft);
        }
        OutSurface => {
            p.line(&[(2.0, 12.0), (5.0, 6.0), (12.0, 6.0), (15.0, 12.0)], acc);
            p.line(&[(2.0, 12.0), (15.0, 12.0)], tint(acc, 120));
        }
        UiPanels => {
            p.fill(&[(1.5, 2.5), (14.5, 2.5), (14.5, 4.5), (1.5, 4.5)], soft);
            p.fill(&[(1.5, 4.5), (4.5, 4.5), (4.5, 13.5), (1.5, 13.5)], soft);
            p.fill(
                &[(11.5, 4.5), (14.5, 4.5), (14.5, 13.5), (11.5, 13.5)],
                soft,
            );
            p.rect(1.5, 2.5, 14.5, 13.5, ink);
            p.line(&[(4.5, 4.5), (4.5, 13.5)], acc);
            p.line(&[(11.5, 4.5), (11.5, 13.5)], acc);
        }
        CleanScreen => {
            p.rect(1.5, 2.5, 14.5, 13.5, ink);
            p.block([5.0, 7.5, 4.5, 4.0, 2.5], acc, soft);
        }
        FullScreen => {
            p.rect(5.0, 5.0, 11.0, 11.0, ink);
            p.line(&[(2.0, 6.0), (2.0, 2.0), (6.0, 2.0)], acc);
            p.line(&[(10.0, 2.0), (14.0, 2.0), (14.0, 6.0)], acc);
            p.line(&[(14.0, 10.0), (14.0, 14.0), (10.0, 14.0)], acc);
            p.line(&[(6.0, 14.0), (2.0, 14.0), (2.0, 10.0)], acc);
        }
        ResetLayout => {
            p.rect(1.5, 2.5, 14.5, 13.5, ink);
            p.line(&[(1.5, 4.5), (14.5, 4.5)], ink);
            p.arc(8.0, 9.0, 3.2, -60.0, 200.0, acc);
            p.head((5.0, 10.1), (0.34, 0.94), acc);
        }
        DocNewPart | DocNewAssembly | DocNewDrawing | DocNewPresentation => {
            sheet(p, ink);
            match icon {
                DocNewPart => p.block([4.0, 8.0, 5.0, 3.5, 2.5], acc, soft),
                DocNewAssembly => {
                    p.block([3.5, 9.5, 3.5, 3.0, 2.0], acc, soft);
                    p.block([6.5, 6.5, 3.5, 3.0, 2.0], ink, tint(ink, 50));
                }
                DocNewDrawing => {
                    p.rect(4.0, 6.0, 10.0, 12.0, acc);
                    p.line(&[(7.0, 10.5), (10.0, 10.5)], ink);
                }
                _ => {
                    p.block([4.0, 9.0, 3.5, 3.0, 2.0], acc, soft);
                    p.dashed((8.5, 9.0), (11.0, 6.0), acc);
                }
            }
            plus(p, hot);
        }
        DocInsert => {
            sheet(p, ink);
            p.line(&[(0.5, 9.0), (8.0, 9.0)], acc);
            p.head((8.0, 9.0), (1.0, 0.0), acc);
        }
        DocLink => {
            sheet(p, ink);
            p.closed(&[(4.0, 7.0), (8.0, 7.0), (8.0, 10.0), (4.0, 10.0)], acc);
            p.closed(&[(7.0, 9.0), (11.0, 9.0), (11.0, 12.0), (7.0, 12.0)], hot);
        }
        DocDuplicate => {
            p.closed(
                &[(1.5, 3.5), (7.0, 3.5), (9.5, 6.0), (9.5, 14.0), (1.5, 14.0)],
                ink,
            );
            p.fill(
                &[
                    (6.0, 1.5),
                    (11.5, 1.5),
                    (14.5, 4.5),
                    (14.5, 12.0),
                    (6.0, 12.0),
                ],
                soft,
            );
            p.closed(
                &[
                    (6.0, 1.5),
                    (11.5, 1.5),
                    (14.5, 4.5),
                    (14.5, 12.0),
                    (6.0, 12.0),
                ],
                acc,
            );
        }
        DocRename => {
            sheet(p, ink);
            p.line(&[(4.0, 9.0), (8.5, 9.0)], acc);
            p.line(&[(10.0, 6.5), (10.0, 11.5)], hot);
        }
        DocDelete => {
            sheet(p, ink);
            p.line(&[(5.0, 7.0), (11.0, 13.0)], warn);
            p.line(&[(11.0, 7.0), (5.0, 13.0)], warn);
        }
        Documents => {
            for (k, c) in [(0.0, ink), (4.0, acc), (8.0, ink)] {
                p.rect(1.5, 2.0 + k, 4.5, 4.5 + k, c);
                p.line(&[(6.5, 3.2 + k), (14.5, 3.2 + k)], c);
            }
        }
        DocExtract => {
            sheet(p, ink);
            p.line(&[(6.0, 9.0), (15.0, 9.0)], acc);
            p.head((15.0, 9.0), (1.0, 0.0), acc);
        }
        DocEmbed => {
            sheet(p, ink);
            p.block([4.5, 8.5, 4.0, 3.0, 2.0], acc, soft);
            p.line(&[(1.0, 4.0), (4.5, 7.0)], ok);
            p.head((4.5, 7.0), (0.76, 0.65), ok);
        }
        DocPack | DocUnpack => {
            p.fill(&[(2.0, 8.0), (14.0, 8.0), (14.0, 14.5), (2.0, 14.5)], soft);
            p.rect(2.0, 8.0, 14.0, 14.5, ink);
            if icon == DocPack {
                p.line(&[(8.0, 1.0), (8.0, 7.0)], acc);
                p.head((8.0, 7.0), (0.0, 1.0), acc);
            } else {
                p.line(&[(8.0, 7.0), (8.0, 1.0)], acc);
                p.head((8.0, 1.0), (0.0, -1.0), acc);
            }
        }
    }
}

/// A sheet of paper with a folded corner (document icons).
fn sheet(p: &Pen, c: Color32) {
    p.closed(
        &[
            (3.0, 1.5),
            (10.0, 1.5),
            (13.0, 4.5),
            (13.0, 14.5),
            (3.0, 14.5),
        ],
        c,
    );
    p.line(&[(10.0, 1.5), (10.0, 4.5), (13.0, 4.5)], c);
}

/// The "new" sign in a document icon's corner.
fn plus(p: &Pen, c: Color32) {
    p.line(&[(12.5, 10.5), (12.5, 15.5)], c);
    p.line(&[(10.0, 13.0), (15.0, 13.0)], c);
}
