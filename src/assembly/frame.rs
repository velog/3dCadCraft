//! Frames (`docs/spec/6.1-frame-generator.md`, stage F1), GUI-free: frame
//! members placed along straight skeleton edges of an assembly's
//! components. Each member is a part of the design file (the section
//! extruded by its length, `part::frame`) placed by an occurrence whose
//! frame follows the skeleton edge; the assembly keeps what each member was
//! made from ([`FrameMember`]), so [`solve`] can follow the skeleton after a
//! change (new length → the member part's extrusion, new place → the
//! occurrence). A member is grounded: only its skeleton moves it.
//!
//! Members go straight into the assembly for now (Inventor makes a Frame
//! subassembly; spec 6.1 F1 notes the deviation).

use serde::{Deserialize, Serialize};

use super::tree::Linked;
use crate::assembly::relation::GeomRef;
use crate::document::iprops::{CustomProperty, PropValue};
use crate::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Project};
use crate::kernel::exact::PathPiece;
use crate::kernel::{Point3, Real, Transform3, Vector3};
use crate::part::frame::stored_length;
use crate::part::frame::{
    CUT_PROPERTIES, CapCorners, EndCut, LENGTH_PROPERTY, Library, MemberPiece, NotchTool, Section,
};
use crate::part::sweep::{PathGeom, edge_piece, ends, reversed, sketch_piece};
use crate::part::{Feature, PartDocument};

/// Which point of the section's box lies on the skeleton line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Anchor {
    /// The bounding box's centre.
    #[default]
    Center,
    /// The section's centroid.
    Centroid,
    #[allow(missing_docs)]
    TopLeft,
    #[allow(missing_docs)]
    Top,
    #[allow(missing_docs)]
    TopRight,
    #[allow(missing_docs)]
    Left,
    #[allow(missing_docs)]
    Right,
    #[allow(missing_docs)]
    BottomLeft,
    #[allow(missing_docs)]
    Bottom,
    #[allow(missing_docs)]
    BottomRight,
}

impl Anchor {
    /// In the panel's order (the Anchor list).
    pub const ALL: [Anchor; 10] = [
        Anchor::Center,
        Anchor::Centroid,
        Anchor::TopLeft,
        Anchor::Top,
        Anchor::TopRight,
        Anchor::Left,
        Anchor::Right,
        Anchor::BottomLeft,
        Anchor::Bottom,
        Anchor::BottomRight,
    ];

    /// The point in section coordinates (box centred on the origin).
    pub fn point(self, s: &Section) -> (Real, Real) {
        let (w, h) = s.extent();
        let (x, y) = (w / 2.0, h / 2.0);
        match self {
            Anchor::Center => (0.0, 0.0),
            Anchor::Centroid => {
                let c = s.properties().centroid;
                (c.x, c.y)
            }
            Anchor::TopLeft => (-x, y),
            Anchor::Top => (0.0, y),
            Anchor::TopRight => (x, y),
            Anchor::Left => (-x, 0.0),
            Anchor::Right => (x, 0.0),
            Anchor::BottomLeft => (-x, -y),
            Anchor::Bottom => (0.0, -y),
            Anchor::BottomRight => (x, -y),
        }
    }
}

fn is_zero(v: &Real) -> bool {
    *v == 0.0
}

/// How a member's section sits on its skeleton line (Insert Frame's
/// Orientation group).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Orientation {
    /// The point of the section on the line.
    #[serde(default)]
    pub anchor: Anchor,
    /// Horizontal Offset (section x), mm.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub h_offset: Real,
    /// Vertical Offset (section y), mm.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub v_offset: Real,
    /// Rotation Angle about the line, radians.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub angle: Real,
    /// Rotate Around Selected Point: rotate about the anchor (offset with
    /// it) instead of the skeleton line.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub about_anchor: bool,
    /// Mirror the section about its vertical axis.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mirror: bool,
    /// Flip Direction: run from the line's end to its start.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub flip: bool,
    /// Align: the direction (frame coordinates, a copy of the picked edge's
    /// at insertion) the section's horizontal axis follows, square to the
    /// line; `None` = the default orientation. Older builds ignore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<[Real; 3]>,
}

/// The skeleton line a member follows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Skeleton {
    /// A straight model edge of a component.
    Edge(GeomRef),
    /// A line of a component part's sketch (by sketch id and entity id).
    SketchLine {
        /// The occurrence's id.
        occurrence: u32,
        /// Occurrence ids below it down to the part (spec 3.1a).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        sub: Vec<u32>,
        /// The part sketch's id.
        sketch: String,
        /// The line.
        line: crate::sketch::EntityId,
    },
    /// Insert Frame's Between Points (format revision 58): a straight run
    /// from an end of one skeleton line (an edge's vertex, a sketch line's
    /// point) to an end of another, following both.
    Between {
        /// The line the start point ends.
        from: Box<Skeleton>,
        /// Which end of it (0 start, 1 end).
        from_end: u8,
        /// The line the end point ends.
        to: Box<Skeleton>,
        /// Which end of it.
        to_end: u8,
    },
    /// Merge Frame Members (format revision 68): curves joined end to end
    /// and tangent there, one member along all of them.
    Chain(Vec<Skeleton>),
}

impl Skeleton {
    /// The occurrence it lies on (Between Points: its start's).
    pub fn occurrence(&self) -> u32 {
        match self {
            Skeleton::Edge(g) => g.occurrence(),
            Skeleton::SketchLine { occurrence, .. } => *occurrence,
            Skeleton::Between { from, .. } => from.occurrence(),
            Skeleton::Chain(l) => l.first().map_or(0, Skeleton::occurrence),
        }
    }

    /// The path below the occurrence to the part.
    pub fn sub(&self) -> &[u32] {
        match self {
            Skeleton::Edge(g) => g.sub(),
            Skeleton::SketchLine { sub, .. } => sub,
            Skeleton::Between { from, .. } => from.sub(),
            Skeleton::Chain(l) => l.first().map_or(&[], Skeleton::sub),
        }
    }
}

/// One member: what it was made from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameMember {
    /// Its occurrence in the assembly.
    pub occurrence: u32,
    /// The skeleton line it follows.
    pub skeleton: Skeleton,
    /// Section family and size (copied into the part; kept to tell the
    /// user and for Change / Refresh).
    pub family: String,
    /// Size within the family.
    pub size: String,
    /// The section's numbers at insertion (the member part regenerates
    /// without the library — rule 8; Refresh reads the library again).
    pub section: SectionData,
    /// Placement on the line.
    #[serde(default)]
    pub orientation: Orientation,
    /// What its part was last built as (length, extensions, cuts), so
    /// [`solve`] rebuilds only members whose shape changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub built: Option<Shape>,
    /// Reuse: the member whose part this occurrence shows (by occurrence).
    /// It has no shape of its own: its cuts are the source's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reuse_of: Option<u32>,
}

/// The shape of a member part: skeleton length, extensions past the start
/// and the end, end cuts (part coordinates: point, normal to the removed
/// side).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    /// Skeleton length.
    pub length: Real,
    /// Extrusion past the start and past the end.
    #[serde(default)]
    pub ext: [Real; 2],
    /// End cuts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cuts: Vec<[[Real; 3]; 2]>,
    /// Notches (Notch).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notches: Vec<NotchTool>,
    /// A curved member's path in the part (arcs, splines, Merge).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<MemberPiece>>,
}

impl Shape {
    fn same(&self, o: &Shape) -> bool {
        let near = |a: Real, b: Real| (a - b).abs() < 1e-7;
        near(self.length, o.length)
            && near(self.ext[0], o.ext[0])
            && near(self.ext[1], o.ext[1])
            && self.cuts.len() == o.cuts.len()
            && self
                .cuts
                .iter()
                .zip(&o.cuts)
                .all(|(a, b)| (0..2).all(|i| (0..3).all(|k| near(a[i][k], b[i][k]))))
            && self.notches.len() == o.notches.len()
            && self.notches.iter().zip(&o.notches).all(|(a, b)| a.same(b))
            && match (&self.path, &o.path) {
                (None, None) => true,
                (Some(a), Some(b)) => {
                    a.len() == b.len()
                        && a.iter().zip(b).all(|(x, y)| {
                            let (x, y) = (x.numbers(), y.numbers());
                            x.len() == y.len() && x.iter().zip(&y).all(|(p, q)| near(*p, *q))
                        })
                }
                _ => false,
            }
    }

    /// The member part's Z range, as its square ends leave it.
    fn z_range(&self) -> (Real, Real) {
        let (mut z0, mut z1) = (-self.ext[0], self.length + self.ext[1]);
        for c in &self.cuts {
            let n = c[1];
            if n[0].abs() < 1e-9 && n[1].abs() < 1e-9 {
                if n[2] > 0.0 {
                    z1 = z1.min(c[0][2]);
                } else {
                    z0 = z0.max(c[0][2]);
                }
            }
        }
        (z0, z1)
    }

    /// Is the high (else the low) end cut slanted?
    fn slanted(&self, high: bool) -> bool {
        self.cuts.iter().any(|c| {
            let n = c[1];
            (n[0].abs() >= 1e-9 || n[1].abs() >= 1e-9) && (n[2] > 0.0) == high
        })
    }

    fn end_cuts(&self) -> Vec<EndCut> {
        self.cuts
            .iter()
            .map(|c| EndCut {
                point: Point3::new(c[0][0], c[0][1], c[0][2]),
                normal: Vector3::new(c[1][0], c[1][1], c[1][2]),
            })
            .collect()
    }
}

/// Where Miter puts the gap between the two cut faces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MiterOffset {
    /// The first member is shortened by the whole gap.
    #[default]
    Default,
    /// The second member is.
    Flipped,
    /// Half each.
    Symmetric,
}

/// An end treatment between members (by occurrence id).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Treatment {
    /// A mitre where `a` and `b` meet, `gap` between the cut faces: Full
    /// Miter Cut (`full`) cuts both on the plane through the meeting points
    /// of their outer and of their inner edges (edge to edge, also for
    /// sections of different sizes); Bisect Miter Cut on the bisector plane
    /// of their axes (every mitre saved before 2026-11-19).
    Miter {
        /// First member.
        a: u32,
        /// Second member.
        b: u32,
        /// Distance between the cut faces.
        #[serde(default)]
        gap: Real,
        /// Which member gives the gap.
        #[serde(default)]
        offset: MiterOffset,
        /// Full Miter Cut (edge to edge); off = Bisect Miter Cut. Older
        /// builds read it as a bisect cut.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        full: bool,
    },
    /// Corner Joint: `a` (the first pick) runs on to the far side of `b`;
    /// `b` stops at `a`'s near side. `offsets` pull each end back.
    CornerJoint {
        /// The member that runs through.
        a: u32,
        /// The member that stops at it.
        b: u32,
        /// Offset1 (of `a`), Offset2 (of `b`).
        #[serde(default)]
        offsets: [Real; 2],
    },
    /// Trim/Extend To Face: the member's end nearer the face's plane is cut
    /// on that plane (trimmed or extended to it), `offset` short of it.
    TrimToFace {
        /// The member.
        member: u32,
        /// The planar face (a component's, in the assembly's references).
        face: GeomRef,
        /// Distance kept from the face.
        #[serde(default)]
        offset: Real,
    },
    /// Lengthen/Shorten: the ends move along the member by these amounts
    /// (start, end; positive = longer), after every other treatment.
    Lengthen {
        /// The member.
        member: u32,
        /// Start and end offsets.
        offsets: [Real; 2],
    },
    /// Notch, Basic Profile: `notched` loses what lies inside the outer
    /// outline of `by` grown by `offset`, along `by`'s run.
    Notch {
        /// The member that is cut.
        notched: u32,
        /// The member whose shape cuts it.
        by: u32,
        /// Gap round the notching member's outline.
        #[serde(default)]
        offset: Real,
    },
}

impl Treatment {
    /// The members it treats.
    pub fn members(&self) -> Vec<u32> {
        match self {
            Treatment::Miter { a, b, .. } | Treatment::CornerJoint { a, b, .. } => vec![*a, *b],
            Treatment::Notch { notched, by, .. } => vec![*notched, *by],
            Treatment::TrimToFace { member, .. } | Treatment::Lengthen { member, .. } => {
                vec![*member]
            }
        }
    }

    /// The command's name (`Miter`, `Corner Joint`…).
    pub fn label(&self) -> &'static str {
        match self {
            Treatment::Miter { .. } => "Miter",
            Treatment::CornerJoint { .. } => "Corner Joint",
            Treatment::TrimToFace { .. } => "Trim/Extend",
            Treatment::Lengthen { .. } => "Lengthen/Shorten",
            Treatment::Notch { .. } => "Notch",
        }
    }
}

/// The numbers of a [`Section`] as stored data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SectionData {
    /// The template (`shs`, `ibeam`…).
    pub shape: String,
    /// Standard.
    pub standard: String,
    /// h, b, t, tw, tf, r1, r2.
    pub dims: [Real; 7],
    /// A custom section's outline (copied in, rule 8).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loops: Vec<crate::part::frame::Loop>,
}

impl SectionData {
    fn of(s: &Section) -> Self {
        Self {
            shape: shape_name(s).into(),
            standard: s.standard.clone(),
            dims: [s.h, s.b, s.t, s.tw, s.tf, s.r1, s.r2],
            // A T-slot section is stored as its outline: its grid and area
            // need no new field, and older builds read it as a custom one.
            loops: if s.shape == crate::part::frame::Shape::TSlot {
                s.loops()
            } else {
                s.custom.clone()
            },
        }
    }
}

fn shape_name(s: &Section) -> &'static str {
    use crate::part::frame::Shape::*;
    match s.shape {
        Shs => "shs",
        Rhs => "rhs",
        Chs => "chs",
        Angle => "angle",
        IBeam => "ibeam",
        Flat => "flat",
        Bar => "bar",
        Channel => "channel",
        Tee => "tee",
        ChannelTapered => "upn",
        IBeamTapered => "ipn",
        SquareBar => "sqbar",
        HexBar => "hexbar",
        ZedFormed => "zcf",
        ChannelFormed => "ccf",
        Custom | TSlot => "custom",
    }
}

impl FrameMember {
    /// The section it was made with.
    pub fn section(&self) -> Option<Section> {
        let d = &self.section.dims;
        let line = format!(
            "{},{},{},{},{},{},{},{},{},{},{}",
            self.family,
            self.section.standard,
            self.section.shape,
            self.size,
            d[0],
            d[1],
            d[2],
            d[3],
            d[4],
            d[5],
            d[6]
        );
        let mut s = Library::parse(&line).0.sections.into_iter().next()?;
        s.custom = self.section.loops.clone();
        Some(s)
    }
}

/// A frame: members inserted together or later added to it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// Stable id inside the assembly (never reused).
    pub id: u32,
    /// Browser name (`Frame1`).
    pub name: String,
    /// The members.
    pub members: Vec<FrameMember>,
    /// End treatments between them, in creation order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub treatments: Vec<Treatment>,
    /// End caps of its members. Older builds ignore them (the caps are
    /// ordinary occurrences, where they were saved).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caps: Vec<EndCap>,
}

/// Insert End Cap's choices.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CapOptions {
    /// Inside the tube (else on the end face).
    pub internal: bool,
    /// The plate's corners.
    pub corners: CapCorners,
    /// Plate thickness.
    pub thickness: Real,
    /// [`Self::edge_offset`] is a share of the section's smaller side.
    pub ratio: bool,
    /// How much smaller than the section's outline the plate is.
    pub edge_offset: Real,
    /// Turn about the member's axis (radians).
    pub rotate: Real,
    /// Distance between the end face and the plate.
    pub offset: Real,
}

impl CapOptions {
    fn inset(&self, s: &Section) -> Real {
        if self.ratio {
            let (w, h) = s.extent();
            self.edge_offset * w.min(h)
        } else {
            self.edge_offset
        }
    }
}

/// An end cap: a plate closing one end of a member.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EndCap {
    /// The plate's occurrence.
    pub occurrence: u32,
    /// The member it closes.
    pub member: u32,
    /// The end: the high (else the low) end of the member part's Z.
    pub high: bool,
    /// How it was made.
    pub options: CapOptions,
    /// The member's section when the plate was built (`family size`).
    pub built_for: String,
}

/// Insert Frame's choices.
#[derive(Clone, Debug)]
pub struct InsertOptions {
    /// Section family.
    pub family: String,
    /// Size.
    pub size: String,
    /// Material name (library), `None` = Generic.
    pub material: Option<String>,
    /// Orientation of every new member.
    pub orientation: Orientation,
    /// What the corners between the new members get (spec 6.1 §3.10).
    pub corners: Corners,
    /// Where the sections lie relative to the frame's centre (§3.12).
    pub layout: Layout,
    /// Appearance of the new members (`None`: As Material).
    pub appearance: Option<String>,
    /// Advanced Settings › Apply Appearance to Part: the appearance goes
    /// into the member parts (else it is the occurrence's override in the
    /// frame assembly).
    pub appearance_to_part: bool,
}

/// Gives member `occ` (part `doc`) appearance `name` the way `to_part`
/// says: on the part (the occurrence's override removed) or as the
/// occurrence's override in `asm` (the part's own removed). `None` = As
/// Material on both. An unknown appearance is an error.
pub fn set_member_appearance(
    project: &mut Project,
    asm: DocumentId,
    occ: u32,
    doc: DocumentId,
    name: Option<&str>,
    to_part: bool,
) -> Result<(), String> {
    let lib = crate::materials::with_user();
    let part_look = if to_part { name } else { None };
    if let Some(DocumentData::Part(p)) = project.document_mut(doc).map(|d| &mut d.data) {
        p.set_appearance(&crate::part::appearance::LookTarget::Part, part_look, &lib)?;
    }
    let occ_look = if to_part { None } else { name };
    let a = asm_mut(project, asm)?;
    a.set_look(
        &super::looks::AsmLookTarget::Occurrence(occ),
        occ_look,
        &lib,
        &[],
    )?;
    Ok(())
}

/// `new` (a rebuilt member or cap part) with the appearance `old` had:
/// a rebuild must not lose the user's colour.
fn keep_looks(old: &DocumentData, mut new: PartDocument) -> PartDocument {
    if let DocumentData::Part(p) = old {
        new.appearance = p.appearance.clone();
        for a in &p.looks.appearances {
            if !new.looks.appearances.iter().any(|x| x.name == a.name) {
                new.looks.appearances.push(a.clone());
            }
        }
    }
    new
}

/// A member's appearance now: the occurrence's override, else its part's
/// (`None`: As Material), and whether it sits on the part.
pub fn member_appearance(
    project: &Project,
    a: &AssemblyDocument,
    occ: u32,
) -> (Option<String>, bool) {
    let Some(o) = a.find(occ) else {
        return (None, false);
    };
    if let Some(n) = &o.looks.appearance {
        return (Some(n.clone()), false);
    }
    match project.document(o.component.document).map(|d| &d.data) {
        Some(DocumentData::Part(p)) if p.appearance.is_some() => (p.appearance.clone(), true),
        _ => (None, false),
    }
}

/// Where a member's section lies across its line, relative to the centre of
/// the frame's lines (spec 6.1 §3.12).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Side {
    /// The Orientation group's Anchor.
    #[default]
    Center,
    /// Towards the centre in both section axes (flush with a box's faces).
    Inside,
    /// Away from the centre.
    Outside,
}

/// Above or below the line, for members whose section's vertical axis is
/// near the world Z.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    /// As [`Side`].
    #[default]
    Auto,
    /// Above the line.
    Top,
    /// Below the line.
    Bottom,
}

/// Position and Level of Insert Frame / Edit Frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    /// Across the line.
    pub side: Side,
    /// Up or down.
    pub level: Level,
}

impl Layout {
    /// The orientation `o` with its anchor chosen for line `p`–`q` of a
    /// frame whose lines' centre is `centre` (unchanged for Center + Auto).
    pub fn orient(&self, o: &Orientation, p: Point3, q: Point3, centre: Point3) -> Orientation {
        if *self == Layout::default() {
            return *o;
        }
        let Some((_, u, v)) = section_axes(p, q, o) else {
            return *o;
        };
        let w = centre - p;
        let sign = |a: Real| {
            if a > 1e-6 {
                1
            } else if a < -1e-6 {
                -1
            } else {
                0
            }
        };
        // Which way the section reaches from the line, in its own axes.
        let (mut sx, mut sy) = match self.side {
            Side::Center => (0, 0),
            Side::Inside => (sign(w.dot(&u)), sign(w.dot(&v))),
            Side::Outside => (-sign(w.dot(&u)), -sign(w.dot(&v))),
        };
        let up = v.dot(&Vector3::z());
        if up.abs() > 0.5 {
            let s = if up > 0.0 { 1 } else { -1 };
            match self.level {
                Level::Top => sy = s,
                Level::Bottom => sy = -s,
                Level::Auto => {}
            }
        }
        if o.mirror {
            sx = -sx;
        }
        let (side, level) = (self.side, self.level);
        if side == Side::Center && sx == 0 && sy == 0 && level == Level::Auto {
            return *o;
        }
        // The anchor is the section's edge on the line: reaching to +x puts
        // its left edge there.
        let anchor = match (sx, sy) {
            (1, 1) => Anchor::BottomLeft,
            (0, 1) => Anchor::Bottom,
            (-1, 1) => Anchor::BottomRight,
            (1, 0) => Anchor::Left,
            (-1, 0) => Anchor::Right,
            (1, -1) => Anchor::TopLeft,
            (0, -1) => Anchor::Top,
            (-1, -1) => Anchor::TopRight,
            _ => Anchor::Center,
        };
        Orientation { anchor, ..*o }
    }
}

/// How Insert Frame finishes the corners where two new members meet at
/// their ends (our addition, like Solid Edge's end conditions; Inventor
/// leaves them uncut).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CornerType {
    /// Uncut (Inventor's behaviour).
    #[default]
    None,
    /// Full Miter Cut, the gap split between both members.
    Miter,
    /// Corner Joint: the member picked first runs through.
    ButtFirst,
    /// Corner Joint: the member picked second runs through.
    ButtSecond,
    /// Corner Joint: the longer member runs through (Solid Edge's Butt 2).
    ButtLonger,
    /// Corner Joint: the shorter member runs through (Solid Edge's Butt 1).
    ButtShorter,
}

/// The corner choice of Insert Frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Corners {
    /// Which treatment each corner gets.
    pub kind: CornerType,
    /// The gap at the corner (Miter distance / Corner Joint offset of the
    /// member that stops).
    pub gap: Real,
    /// What a new member ending on the middle of another gets.
    pub tees: TeeJoint,
}

/// T joints of Insert Frame / Edit Frame (a member ending on the middle of
/// another one): Solid Edge's *Tee* end condition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TeeJoint {
    /// Uncut.
    #[default]
    None,
    /// A square cut at the through member's near face (a Corner Joint).
    Butt,
    /// Cut to the through member's outline (a Notch; slower).
    Notch,
}

impl Corners {
    /// The treatment of the corner between `first` and `second` (picked
    /// in this order, of these lengths).
    fn treatment(&self, first: u32, second: u32, lengths: [Real; 2]) -> Option<Treatment> {
        let joint = |a, b| Treatment::CornerJoint {
            a,
            b,
            offsets: [0.0, self.gap],
        };
        match self.kind {
            CornerType::None => None,
            CornerType::Miter => Some(Treatment::Miter {
                a: first,
                b: second,
                gap: self.gap,
                offset: MiterOffset::Symmetric,
                full: true,
            }),
            CornerType::ButtFirst => Some(joint(first, second)),
            CornerType::ButtSecond => Some(joint(second, first)),
            CornerType::ButtLonger if lengths[1] > lengths[0] + 1e-6 => Some(joint(second, first)),
            CornerType::ButtLonger => Some(joint(first, second)),
            CornerType::ButtShorter if lengths[1] < lengths[0] - 1e-6 => Some(joint(second, first)),
            CornerType::ButtShorter => Some(joint(first, second)),
        }
    }
}

/// A skeleton line in assembly coordinates: its start and end. `linked`
/// holds the linked part files (a skeleton placed from another file).
pub fn skeleton_line(
    project: &Project,
    linked: &Linked,
    asm: &AssemblyDocument,
    r: &Skeleton,
) -> Result<(Point3, Point3), String> {
    if let Skeleton::Between {
        from,
        from_end,
        to,
        to_end,
    } = r
    {
        let end = |l: (Point3, Point3), k: u8| if k == 0 { l.0 } else { l.1 };
        let a = end(skeleton_line(project, linked, asm, from)?, *from_end);
        let b = end(skeleton_line(project, linked, asm, to)?, *to_end);
        if (b - a).norm() < 1e-6 {
            return Err("its two points coincide".into());
        }
        return Ok((a, b));
    }
    if matches!(r, Skeleton::Chain(_)) {
        return Err(CURVED.into());
    }
    let n = super::tree::nested(project, linked, asm, r.occurrence(), r.sub())?;
    let part = super::tree::part_of(project, linked, n.component)
        .ok_or_else(|| format!("{} shows no part", n.name))?;
    let (a, b) = match r {
        Skeleton::Edge(GeomRef::Edge { edge, .. }) => {
            let body = &part.body;
            let id = crate::part::draft::edge_of(body, edge)
                .ok_or_else(|| format!("{}: its skeleton edge no longer exists", n.name))?;
            let e = body.edge(id).ok_or("the skeleton edge no longer exists")?;
            if !matches!(body.curve(e.curve), Some(crate::kernel::Curve::Line(_))) {
                return Err(CURVED.into());
            }
            let pts = body
                .sample_edge(id, &crate::kernel::TessellationOptions::default())
                .map_err(|x| x.to_string())?;
            match (pts.first(), pts.last()) {
                (Some(a), Some(b)) => (*a, *b),
                _ => return Err("the skeleton edge has no ends".into()),
            }
        }
        Skeleton::Chain(_) => return Err(CURVED.into()),
        Skeleton::Edge(_) | Skeleton::Between { .. } => {
            return Err("a frame member follows an edge or a sketch line".into());
        }
        Skeleton::SketchLine { sketch, line, .. } => {
            let ps = part
                .sketches
                .iter()
                .find(|s| &s.id == sketch)
                .ok_or_else(|| format!("{}: its skeleton sketch no longer exists", n.name))?;
            let sk = &ps.sketch;
            if sk
                .entity(*line)
                .is_some_and(|e| !matches!(e, crate::sketch::Entity::Line { .. }))
            {
                return Err(CURVED.into());
            }
            let (p, q) = sk
                .line_points(*line)
                .map_err(|_| format!("{}: its skeleton line no longer exists", n.name))?;
            let at = |id| sk.point(id).map(|x| sk.to_world(&x.position));
            match (at(p), at(q)) {
                (Some(a), Some(b)) => (a, b),
                _ => return Err("the skeleton line has no ends".into()),
            }
        }
    };
    let top = asm
        .find(r.occurrence())
        .ok_or("the component no longer exists")?;
    let m = top.transform * n.at;
    Ok((m.transform_point(&a), m.transform_point(&b)))
}

/// What a member needs of a straight-only command, in its error.
const CURVED: &str = "this needs a straight member";

/// What a skeleton line is, in assembly coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum Run {
    /// Straight, from one point to another.
    Line(Point3, Point3),
    /// Along an arc, a spline or a merged chain (an open one).
    Curve(PathGeom),
}

impl Run {
    /// Its two ends.
    pub fn chord(&self) -> (Point3, Point3) {
        match self {
            Run::Line(a, b) => (*a, *b),
            Run::Curve(c) => (c.eval(0.0).0, c.eval(c.length()).0),
        }
    }

    /// Its length along the curve.
    pub fn length(&self) -> Real {
        match self {
            Run::Line(a, b) => (b - a).norm(),
            Run::Curve(c) => c.length(),
        }
    }
}

/// A piece of a path moved by the rigid motion `m`; a spline keeps its
/// points only.
fn moved_piece(p: &PathPiece, m: &Transform3) -> PathPiece {
    let at = |q: &Point3| m.transform_point(q);
    match p {
        PathPiece::Line { a, b } => PathPiece::Line { a: at(a), b: at(b) },
        PathPiece::Arc {
            a,
            center,
            axis,
            angle,
        } => PathPiece::Arc {
            a: at(a),
            center: at(center),
            axis: m.transform_vector(axis),
            angle: *angle,
        },
        PathPiece::Points(v) | PathPiece::Spline { points: v, .. } => {
            PathPiece::Points(v.iter().map(at).collect())
        }
    }
}

/// One skeleton curve as a path piece, in assembly coordinates.
fn skeleton_piece(
    project: &Project,
    linked: &Linked,
    asm: &AssemblyDocument,
    r: &Skeleton,
) -> Result<PathPiece, String> {
    match skeleton_line(project, linked, asm, r) {
        Ok((a, b)) => return Ok(PathPiece::Line { a, b }),
        Err(e) if e != CURVED => return Err(e),
        Err(_) => {}
    }
    let n = super::tree::nested(project, linked, asm, r.occurrence(), r.sub())?;
    let part = super::tree::part_of(project, linked, n.component)
        .ok_or_else(|| format!("{} shows no part", n.name))?;
    let piece = match r {
        Skeleton::Edge(GeomRef::Edge { edge, .. }) => edge_piece(&part.body, edge)?,
        Skeleton::SketchLine { sketch, line, .. } => {
            let ps = part
                .sketches
                .iter()
                .find(|s| &s.id == sketch)
                .ok_or_else(|| format!("{}: its skeleton sketch no longer exists", n.name))?;
            let curve = ps
                .sketch
                .curve_of(*line)
                .ok_or_else(|| format!("{}: its skeleton curve no longer exists", n.name))?;
            sketch_piece(&ps.sketch.plane, &curve)
        }
        _ => return Err("a frame member follows an edge or a sketch curve".into()),
    };
    let top = asm
        .find(r.occurrence())
        .ok_or("the component no longer exists")?;
    Ok(moved_piece(&piece, &(top.transform * n.at)))
}

/// The run of a skeleton line: straight, or a curve for an arc, a spline or
/// a merged chain (which must be open and tangent where its curves meet).
pub fn skeleton_run(
    project: &Project,
    linked: &Linked,
    asm: &AssemblyDocument,
    r: &Skeleton,
) -> Result<Run, String> {
    let open = |c: PathGeom| {
        if c.closed {
            Err("a closed curve cannot be a member".to_string())
        } else {
            Ok(Run::Curve(c))
        }
    };
    if let Skeleton::Chain(list) = r {
        let pieces = list
            .iter()
            .map(|s| skeleton_piece(project, linked, asm, s))
            .collect::<Result<Vec<_>, _>>()?;
        let near = pieces.first().map_or_else(Point3::origin, |p| ends(p).0);
        let c = PathGeom::chained(pieces, near)?;
        if c.sharpest_joint() > MERGE_ANGLE {
            return Err("merged curves must be tangent where they meet".into());
        }
        return open(c);
    }
    match skeleton_piece(project, linked, asm, r)? {
        PathPiece::Line { a, b } => Ok(Run::Line(a, b)),
        p => {
            let (a, b) = ends(&p);
            open(PathGeom {
                pieces: vec![p],
                closed: (b - a).norm() < 1e-6,
            })
        }
    }
}

/// How far merged curves may turn where they meet (radians).
const MERGE_ANGLE: Real = 1e-3;

/// The occurrence placement of a curved member and its run: Flip and Mirror
/// both run the path from its far end (the section's horizontal axis turns
/// round, so doing both cancels out).
pub fn curved_place(
    s: &Section,
    path: &PathGeom,
    o: &Orientation,
) -> Option<(Transform3, PathGeom)> {
    let run = if o.flip != o.mirror {
        PathGeom {
            pieces: path.pieces.iter().rev().map(reversed).collect(),
            closed: path.closed,
        }
    } else {
        path.clone()
    };
    let (a, t) = run.eval(0.0);
    let plain = Orientation {
        flip: false,
        mirror: false,
        ..*o
    };
    placement(s, a, a + t * run.length().max(1.0), &plain).map(|p| (p, run))
}

/// `path` as a member part's path pieces, moved by `m`.
pub fn member_pieces(path: &PathGeom, m: &Transform3) -> Vec<MemberPiece> {
    let at = |q: &Point3| {
        let q = m.transform_point(q);
        [q.x, q.y, q.z]
    };
    path.pieces
        .iter()
        .map(|p| match p {
            PathPiece::Line { a, b } => MemberPiece::Line { a: at(a), b: at(b) },
            PathPiece::Arc {
                a,
                center,
                axis,
                angle,
            } => {
                let k = m.transform_vector(axis);
                MemberPiece::Arc {
                    a: at(a),
                    center: at(center),
                    axis: [k.x, k.y, k.z],
                    angle: *angle,
                }
            }
            PathPiece::Points(v) | PathPiece::Spline { points: v, .. } => {
                MemberPiece::Points(v.iter().map(at).collect())
            }
        })
        .collect()
}

/// A member's run direction and its section's axes (horizontal, vertical;
/// turned by the rotation angle) on line `a`–`b`.
fn section_axes(a: Point3, b: Point3, o: &Orientation) -> Option<(Vector3, Vector3, Vector3)> {
    let (a, b) = if o.flip { (b, a) } else { (a, b) };
    let d = (b - a).try_normalize(1e-9)?;
    // Align: the horizontal axis along the picked direction, square to the line.
    if let Some(w) = o.align
        && let Some(u) = {
            let w = Vector3::new(w[0], w[1], w[2]);
            (w - d * d.dot(&w)).try_normalize(1e-6)
        }
    {
        let v = d.cross(&u);
        let (c, sn) = (o.angle.cos(), o.angle.sin());
        return Some((d, u * c + v * sn, v * c - u * sn));
    }
    let up = if d.z.abs() > 0.999 {
        Vector3::x()
    } else {
        Vector3::z()
    };
    let v = (up - d * d.dot(&up)).normalize();
    let u = v.cross(&d);
    let (c, sn) = (o.angle.cos(), o.angle.sin());
    Some((d, u * c + v * sn, v * c - u * sn))
}

/// The occurrence placement of a member of section `s` along `a → b`:
/// part Z along the line, part X / Y the section's horizontal / vertical
/// axes (vertical = the assembly's Z projected square to the line, or its X
/// for a vertical line), the anchor on the line, then offsets and rotation.
pub fn placement(s: &Section, a: Point3, b: Point3, o: &Orientation) -> Option<Transform3> {
    let (d, ur, vr) = section_axes(a, b, o)?;
    let (u, v) = {
        let (c, sn) = (o.angle.cos(), o.angle.sin());
        (ur * c - vr * sn, vr * c + ur * sn)
    };
    let (a, b) = if o.flip { (b, a) } else { (a, b) };
    let (ax, ay) = o.anchor.point(s);
    // Section point (x, y) → a + R·(x − anchor + offset) (rotated about the
    // line) or a + offset + R·(x − anchor) (about the anchor).
    let mut origin = if o.about_anchor {
        a + u * o.h_offset + v * o.v_offset - (ur * ax + vr * ay)
    } else {
        a + ur * (o.h_offset - ax) + vr * (o.v_offset - ay)
    };
    let (mut x, mut z) = (ur, d);
    if o.mirror {
        // Half a turn about the section's vertical axis through the anchor:
        // x and the run reverse, so the part starts at the far end.
        let anchor = origin + ur * ax + vr * ay;
        x = -ur;
        z = -d;
        origin = anchor - x * ax - vr * ay + d * (b - a).norm();
    }
    let rot = nalgebra::Rotation3::from_basis_unchecked(&[x, vr, z]);
    let q = nalgebra::UnitQuaternion::from_rotation_matrix(&rot);
    Some(Transform3::from_parts(origin.coords.into(), q))
}

/// The assembly `asm` of `project`.
fn asm_of(project: &Project, id: DocumentId) -> Result<&AssemblyDocument, String> {
    match project.document(id).map(|d| &d.data) {
        Some(DocumentData::Assembly(a)) => Ok(a),
        _ => Err("no assembly is open".into()),
    }
}

fn asm_mut(project: &mut Project, id: DocumentId) -> Result<&mut AssemblyDocument, String> {
    match project.document_mut(id).map(|d| &mut d.data) {
        Some(DocumentData::Assembly(a)) => Ok(a),
        _ => Err("no assembly is open".into()),
    }
}

/// Insert Frame: one member per edge of `edges`, each a new part of the
/// design file placed along its edge, all in a new frame (or `into`, an
/// existing frame of the assembly). On an error nothing is changed (rule 5).
/// Returns the frame id and the new occurrences.
pub fn insert(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    edges: &[Skeleton],
    o: &InsertOptions,
    lib: &Library,
    into: Option<u32>,
) -> Result<(u32, Vec<u32>), String> {
    insert_with(project, linked, asm, edges, &[], o, lib, into)
}

/// [`insert`] where member `k` may have an orientation of its own (`own[k]`,
/// the Insert Frame list's placement per member, spec 6.1 §3.1a): it is taken
/// as it is (no Position / Level); `None` or a missing entry takes the panel's.
#[allow(clippy::too_many_arguments)]
pub fn insert_with(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    edges: &[Skeleton],
    own: &[Option<Orientation>],
    o: &InsertOptions,
    lib: &Library,
    into: Option<u32>,
) -> Result<(u32, Vec<u32>), String> {
    if edges.is_empty() {
        return Err("select the skeleton edges".into());
    }
    let section = lib
        .find(&o.family, &o.size)
        .ok_or_else(|| format!("{} {} is not in the library", o.family, o.size))?
        .clone();
    let before = project.clone();
    let result = (|| {
        let mut made = Vec::new();
        let runs = {
            let a = asm_of(project, asm)?;
            edges
                .iter()
                .map(|e| skeleton_run(project, linked, a, e))
                .collect::<Result<Vec<_>, String>>()?
        };
        let lines: Vec<(Point3, Point3)> = runs.iter().map(Run::chord).collect();
        let centre = centre_of(&lines);
        for (k, (e, run)) in edges.iter().zip(&runs).enumerate() {
            let length = run.length();
            let mine = own.get(k).copied().flatten();
            let (orientation, place, part, path) = match run {
                Run::Line(p, q) => {
                    let orientation =
                        mine.unwrap_or_else(|| o.layout.orient(&o.orientation, *p, *q, centre));
                    let place = placement(&section, *p, *q, &orientation)
                        .ok_or("the skeleton edge has no length")?;
                    let part = section
                        .member_part(length, o.material.as_deref())
                        .map_err(|e| e.to_string())?;
                    (orientation, place, part, None)
                }
                Run::Curve(c) => {
                    let orientation = mine.unwrap_or(o.orientation);
                    let (place, run) = curved_place(&section, c, &orientation)
                        .ok_or("the skeleton curve has no length")?;
                    let pieces = member_pieces(&run, &place.inverse());
                    let part = section
                        .member_part_path(&pieces, o.material.as_deref())
                        .map_err(|e| e.to_string())?;
                    (orientation, place, part, Some(pieces))
                }
            };
            let name = project.free_name(&format!("{}-", section.stock_number()));
            let doc = project.add_document(name.clone(), DocumentData::Part(part));
            if let Some(d) = project.document_mut(doc) {
                d.properties = section.member_properties(length);
            }
            let a = asm_mut(project, asm)?;
            let occ = a.place(DocRef::local(doc), &name, place, true);
            if o.appearance.is_some() {
                set_member_appearance(
                    project,
                    asm,
                    occ,
                    doc,
                    o.appearance.as_deref(),
                    o.appearance_to_part,
                )?;
            }
            made.push(FrameMember {
                occurrence: occ,
                skeleton: e.clone(),
                family: section.family.clone(),
                size: section.size.clone(),
                section: SectionData::of(&section),
                orientation,
                built: Some(Shape {
                    length,
                    path,
                    ..Default::default()
                }),
                reuse_of: None,
            });
        }
        if !(o.corners.gap.is_finite() && o.corners.gap >= 0.0) {
            return Err("the corner gap must be zero or more".into());
        }
        let a = asm_mut(project, asm)?;
        let occs: Vec<u32> = made.iter().map(|m| m.occurrence).collect();
        let id = match into.and_then(|id| a.frames.iter_mut().find(|f| f.id == id)) {
            Some(f) => {
                f.members.extend(made);
                f.id
            }
            None => {
                let id = a.frames.iter().map(|f| f.id).max().unwrap_or(0) + 1;
                let name = (1..)
                    .map(|n| format!("Frame{n}"))
                    .find(|n| !a.frames.iter().any(|f| &f.name == n))
                    .unwrap_or_default();
                a.frames.push(Frame {
                    id,
                    name,
                    members: made,
                    treatments: Vec::new(),
                    caps: Vec::new(),
                });
                id
            }
        };
        // Corners between the new members (in pick order: the earlier
        // member is the first).
        {
            let a = asm_of(project, asm)?;
            let ts = corner_treatments(project, linked, a, &occs, &o.corners);
            if !ts.is_empty() {
                let a = asm_mut(project, asm)?;
                if let Some(f) = a.frames.iter_mut().find(|f| f.id == id) {
                    f.treatments.extend(ts);
                }
                let s = solve(project, linked, asm)?;
                if let Some((_, why)) = s.sick.iter().find(|(m, _)| occs.contains(m)) {
                    return Err(format!("corners: {why}"));
                }
            }
        }
        Ok((id, occs))
    })();
    if result.is_err() {
        *project = before;
    }
    result
}

/// What [`solve`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Solved {
    /// Members whose place or length changed.
    pub moved: usize,
    /// Members that could not follow their skeleton (occurrence, why):
    /// they keep their last shape and place (health, rule 5).
    pub sick: Vec<(u32, String)>,
}

/// One member's state during [`solve`].
struct Live {
    occ: u32,
    doc: DocumentId,
    section: Section,
    /// The member's run in assembly coordinates (Flip applied).
    start: Point3,
    end: Point3,
    place: Transform3,
    /// Cut planes in assembly coordinates per end (start, end): point and
    /// normal towards the removed side.
    cuts: [Option<(Point3, Vector3)>; 2],
    /// Shows another member's part (Reuse): no shape of its own.
    reuse_of: Option<u32>,
    /// The run of a curved member (no end treatments).
    path: Option<PathGeom>,
    /// Which cut ends a Miter made (the word in its cut detail).
    mitred: [bool; 2],
    /// Notch prisms in assembly coordinates.
    notches: Vec<NotchTool>,
    built: Option<Shape>,
    material: Option<String>,
}

/// Where two members' axes meet: the point and, for each, which end
/// (0 = start, 1 = end) lies at it. `None` when the axes do not meet near
/// an end of each.
fn joint(a: (Point3, Point3), b: (Point3, Point3), reach: Real) -> Option<(Point3, usize, usize)> {
    let (da, db) = (a.1 - a.0, b.1 - b.0);
    let w = a.0 - b.0;
    let (aa, bb, ab) = (da.dot(&da), db.dot(&db), da.dot(&db));
    let den = aa * bb - ab * ab;
    // Parallel axes: only end to end (a butt joint).
    let (ta, tb) = if den.abs() < 1e-12 * aa * bb {
        return None;
    } else {
        let (dw, ew) = (da.dot(&w), db.dot(&w));
        ((ab * ew - bb * dw) / den, (aa * ew - ab * dw) / den)
    };
    let (pa, pb) = (a.0 + da * ta, b.0 + db * tb);
    if (pa - pb).norm() > 1e-3 {
        return None;
    }
    let (la, lb) = (aa.sqrt(), bb.sqrt());
    let end_a = usize::from(ta > 0.5);
    let end_b = usize::from(tb > 0.5);
    // The meeting point near the chosen end of each (within `reach`).
    let off_a = (ta - end_a as Real).abs() * la;
    let off_b = (tb - end_b as Real).abs() * lb;
    (off_a <= reach && off_b <= reach).then_some(((pa + pb.coords) / 2.0, end_a, end_b))
}

/// Where `b` ends on the middle of `a` (a T joint): the meeting point and
/// b's end there (0 start / 1 end). `None` when b does not end on a's axis
/// or a's end is within `near` of the meeting point (a corner).
fn tee(a: (Point3, Point3), b: (Point3, Point3), near: Real) -> Option<(Point3, usize)> {
    let d = a.1 - a.0;
    let l = d.norm();
    if l < 1e-9 {
        return None;
    }
    let u = d / l;
    for (k, e) in [b.0, b.1].into_iter().enumerate() {
        let t = (e - a.0).dot(&u);
        let off = (a.0 + u * t - e).norm();
        if off < 1e-3 && t > near && t < l - near {
            return Some((a.0 + u * t, k));
        }
    }
    None
}

/// The cut planes Miter `t` gives its two members (assembly coordinates).
/// A cut a treatment gives: member index, end (0 start / 1 end), plane
/// point, normal towards the removed side.
type Cut = (usize, usize, Point3, Vector3);

/// Away from end `e` of `m` along it (unit).
fn away(m: &Live, e: usize) -> Vector3 {
    let (from, to) = if e == 0 {
        (m.start, m.end)
    } else {
        (m.end, m.start)
    };
    (to - from).normalize()
}

/// How far `m`'s section reaches from its axis in direction `u` (square to
/// the axis).
fn reach(m: &Live, u: &Vector3) -> Real {
    let d = (m.end - m.start).normalize();
    m.section
        .outline()
        .iter()
        .map(|q| {
            let v = m.place.transform_point(&Point3::new(q.x, q.y, 0.0)) - m.start;
            let v = v - d * v.dot(&d);
            u.dot(&v)
        })
        .fold(Real::MIN, Real::max)
}

/// Resolves a component face to its plane (point, normal; assembly
/// coordinates).
type FacePlane<'a> = dyn Fn(&GeomRef) -> Result<(Point3, Vector3), String> + 'a;

/// The cuts treatment `t` gives (assembly coordinates). `face` resolves a
/// component face to its plane (assembly coordinates).
fn treatment_cuts(t: &Treatment, live: &[Live], face: &FacePlane) -> Result<Vec<Cut>, String> {
    let find = |o: u32| {
        live.iter()
            .position(|m| m.occ == o)
            .ok_or_else(|| "one of its members no longer exists".to_string())
    };
    match t {
        Treatment::Miter { .. } => miter_cuts(t, live, &find).map(|c| c.to_vec()),
        Treatment::CornerJoint { a, b, offsets } => {
            let (ia, ib) = (find(*a)?, find(*b)?);
            let (ma, mb) = (&live[ia], &live[ib]);
            let size = {
                let (w, h) = ma.section.extent();
                let (w2, h2) = mb.section.extent();
                2.0 * (w + h + w2 + h2)
            };
            // A T joint: b ends on the middle of a, which stays whole; b
            // stops at a's near side.
            if let Some((c, eb)) = tee((ma.start, ma.end), (mb.start, mb.end), {
                let (w2, h2) = mb.section.extent();
                w2.max(h2)
            }) {
                let da = (ma.end - ma.start).normalize();
                let db = away(mb, eb);
                let ub = (db - da * db.dot(&da))
                    .try_normalize(1e-9)
                    .ok_or("the members are parallel")?;
                let pb = c + ub * (reach(ma, &ub) + offsets[1]);
                return Ok(vec![(ib, eb, pb, -ub)]);
            }
            let (c, ea, eb) = joint((ma.start, ma.end), (mb.start, mb.end), size)
                .ok_or("the members do not meet")?;
            let (da, db) = (away(ma, ea), away(mb, eb));
            // Square to the other member, towards each one's own body.
            let ua = (da - db * da.dot(&db))
                .try_normalize(1e-9)
                .ok_or("the members are parallel")?;
            let ub = (db - da * db.dot(&da))
                .try_normalize(1e-9)
                .ok_or("the members are parallel")?;
            // A ends at b's far side, b at a's near side.
            let pa = c - ua * reach(mb, &(-ua)) + ua * offsets[0];
            let pb = c + ub * (reach(ma, &ub) + offsets[1]);
            Ok(vec![(ia, ea, pa, -ua), (ib, eb, pb, -ub)])
        }
        Treatment::TrimToFace {
            member,
            face: f,
            offset,
        } => {
            let i = find(*member)?;
            let m = &live[i];
            let (p, n) = face(f)?;
            let d = (m.end - m.start).normalize();
            if n.dot(&d).abs() < 0.05 {
                return Err("the face runs along the member".into());
            }
            // The end nearer the plane; the other end stays.
            let dist = |q: &Point3| (q - p).dot(&n);
            let e = usize::from(dist(&m.end).abs() < dist(&m.start).abs());
            let keep = if e == 0 { m.end } else { m.start };
            let n = if dist(&keep) > 0.0 { -n } else { n };
            Ok(vec![(i, e, p - n * *offset, n)])
        }
        Treatment::Lengthen { .. } | Treatment::Notch { .. } => Ok(Vec::new()),
    }
}

/// How far a notch outline is widened across the notched member: equal
/// tubes in a T have the notching tube's walls in the planes of the
/// notched one's and their corner rounds on crossing axes, which took the
/// kernel's Cut 11-15 s; 0.1 mm apart, about 2 s. Only material next to
/// the notched member's own sides could change, by at most this much.
const NOTCH_CLEARANCE: Real = 0.1;

/// `l` with every point moved `by` along `across`, away from the line
/// through the origin square to it (a circle keeps its size).
fn widened(
    l: crate::part::frame::Loop,
    across: crate::sketch::Vector2,
    by: Real,
) -> crate::part::frame::Loop {
    use crate::part::frame::Loop;
    let push = |q: crate::sketch::Point2| {
        let s = q.coords.dot(&across);
        if s.abs() < 1e-9 {
            q
        } else {
            q + across * (by * s.signum())
        }
    };
    match l {
        Loop::Corners(cs) => Loop::Corners(cs.into_iter().map(|(q, r)| (push(q), r)).collect()),
        Loop::Path(ps) => Loop::Path(
            ps.into_iter()
                .map(|(q, a)| (push(q), a.map(|(c, ccw)| (push(c), ccw))))
                .collect(),
        ),
        c @ Loop::Circle(..) => c,
    }
}

/// The prism Notch `t` cuts out of its notched member (assembly
/// coordinates): member index and tool.
fn notch_tool(t: &Treatment, live: &[Live]) -> Result<(usize, NotchTool), String> {
    let Treatment::Notch {
        notched,
        by,
        offset,
    } = t
    else {
        return Err("not a notch".into());
    };
    let find = |o: u32| {
        live.iter()
            .position(|m| m.occ == o)
            .ok_or_else(|| "one of its members no longer exists".to_string())
    };
    let (i, k) = (find(*notched)?, find(*by)?);
    if i == k {
        return Err("a member cannot notch itself".into());
    }
    if *offset < 0.0 {
        return Err("the offset must not be negative".into());
    }
    let (m, b) = (&live[i], &live[k]);
    // The members must come near each other: the closest points of the two
    // runs (sampled) within both sections' reach.
    let reach = {
        let (w, h) = m.section.extent();
        let (w2, h2) = b.section.extent();
        (w.hypot(h) + w2.hypot(h2)) / 2.0 + offset
    };
    let n = 64;
    let at = |s: &Live, j: usize| s.start + (s.end - s.start) * (j as Real / n as Real);
    let near = (0..=n)
        .flat_map(|j| (0..=n).map(move |l| (j, l)))
        .map(|(j, l)| (at(m, j) - at(b, l)).norm())
        .fold(Real::MAX, Real::min);
    if near > reach {
        return Err("the members do not meet".into());
    }
    let outline = b
        .section
        .notch_outline(*offset)
        .ok_or("an offset round this profile is not supported yet")?;
    // The notched member's run in the notching section's axes; the outline
    // is widened across it (the face its end stops on stays exact).
    let d = (m.end - m.start).normalize();
    let (bx, by_) = (
        b.place.transform_vector(&Vector3::x()),
        b.place.transform_vector(&Vector3::y()),
    );
    let run = crate::sketch::Vector2::new(d.dot(&bx), d.dot(&by_));
    let across = crate::sketch::Vector2::new(-run.y, run.x)
        .try_normalize(1e-6)
        .ok_or("the members are parallel")?;
    let outline = widened(outline, across, NOTCH_CLEARANCE);
    // Only the stretch of the notching member the notched one can reach:
    // a long prism made the kernel's Cut slower.
    let z = b.place.transform_vector(&Vector3::z());
    let (w, h) = m.section.extent();
    let pad = w.hypot(h) / 2.0 + 1.0;
    let base = b.place.transform_point(&Point3::origin());
    let along = |q: Point3| (q - base).dot(&z);
    let (a0, a1) = (along(m.start), along(m.end));
    let len = (b.end - b.start).norm();
    let lo = (a0.min(a1) - pad).max(0.0);
    let hi = (a0.max(a1) + pad).min(len);
    if hi - lo < 1e-6 {
        return Err("the members do not meet".into());
    }
    let o = base + z * lo;
    let x = b.place.transform_vector(&Vector3::x());
    let y = b.place.transform_vector(&Vector3::y());
    Ok((
        i,
        NotchTool {
            origin: [o.x, o.y, o.z],
            x: [x.x, x.y, x.z],
            y: [y.x, y.y, y.z],
            length: hi - lo,
            outline,
        },
    ))
}

fn miter_cuts(
    t: &Treatment,
    live: &[Live],
    find: &dyn Fn(u32) -> Result<usize, String>,
) -> Result<[Cut; 2], String> {
    let Treatment::Miter {
        a,
        b,
        gap,
        offset,
        full,
    } = t
    else {
        return Err("not a mitre".into());
    };
    let (ia, ib) = (find(*a)?, find(*b)?);
    let (ma, mb) = (&live[ia], &live[ib]);
    let reach = {
        let (w, h) = ma.section.extent();
        let (w2, h2) = mb.section.extent();
        2.0 * (w + h + w2 + h2)
    };
    let (c, ea, eb) =
        joint((ma.start, ma.end), (mb.start, mb.end), reach).ok_or("the members do not meet")?;
    let (da, db) = (away(ma, ea), away(mb, eb));
    let n = (da - db)
        .try_normalize(1e-9)
        .ok_or("the members run the same way")?;
    // Full: the plane through the meeting points of the outer edges and of
    // the inner edges (it is the bisector when both bands lie alike).
    let (c, n) = match full.then(|| edge_to_edge(ma, mb, c, da, db)).flatten() {
        Some((q, m)) => (q, if m.dot(&n) < 0.0 { -m } else { m }),
        None => (c, n),
    };
    let (ga, gb) = match offset {
        MiterOffset::Default => (*gap, 0.0),
        MiterOffset::Flipped => (0.0, *gap),
        MiterOffset::Symmetric => (gap / 2.0, gap / 2.0),
    };
    // A keeps the side n points to, B the other one.
    Ok([(ia, ea, c + n * ga, -n), (ib, eb, c - n * gb, n)])
}

/// Full Miter Cut's plane (point, unit normal) for members `ma` and `mb`
/// meeting at `c`, running away from it along `da` / `db`: through the point
/// where their outer edges meet and the one where their inner edges meet,
/// square to the plane of the two runs. `None` for runs in one line.
fn edge_to_edge(
    ma: &Live,
    mb: &Live,
    c: Point3,
    da: Vector3,
    db: Vector3,
) -> Option<(Point3, Vector3)> {
    let k = da.cross(&db).try_normalize(1e-9)?;
    let (pa, pb) = (k.cross(&da), k.cross(&db));
    // Each member's band across its run in the runs' plane: the section's
    // box corners as placed, measured from the corner point.
    let band = |m: &Live, p: Vector3| {
        let (w, h) = m.section.extent();
        let (lo, hi) = [(-w, -h), (w, -h), (w, h), (-w, h)]
            .iter()
            .map(|(x, y)| {
                let q = m.place.transform_point(&Point3::new(x / 2.0, y / 2.0, 0.0));
                (q - c).dot(&p)
            })
            .fold((Real::MAX, Real::MIN), |(a, b), v| (a.min(v), b.max(v)));
        (lo, hi)
    };
    let (a0, a1) = band(ma, pa);
    let (b0, b1) = band(mb, pb);
    // The point of line c + sa·pa + t·da on line c + sb·pb + u·db (2D in
    // the plane, coordinates along da and pa).
    let meet = |sa: Real, sb: Real| -> Option<Point3> {
        // Along pa: sa = sb·(pb·pa) + u·(db·pa).
        let y = db.dot(&pa);
        if y.abs() < 1e-9 {
            return None;
        }
        let u = (sa - sb * pb.dot(&pa)) / y;
        Some(c + pb * sb + db * u)
    };
    let outer = meet(a0, b1)?;
    let inner = meet(a1, b0)?;
    let n = (inner - outer).cross(&k).try_normalize(1e-9)?;
    Some((nalgebra::center(&outer, &inner), n))
}

/// The shape a live member's cuts give it (part coordinates), or why not.
fn shape_of(m: &Live) -> Result<Shape, String> {
    let inv = m.place.inverse();
    if let Some(path) = &m.path {
        return Ok(Shape {
            length: path.length(),
            path: Some(member_pieces(path, &inv)),
            ..Default::default()
        });
    }
    let length = (m.end - m.start).norm();
    let (w, h) = m.section.extent();
    let mut ext = [0.0, 0.0];
    let mut cuts = Vec::new();
    for (k, c) in m.cuts.iter().enumerate() {
        let Some((p, n)) = c else {
            continue;
        };
        let (p, n) = (inv.transform_point(p), inv.transform_vector(n));
        if n.z.abs() < 0.05 {
            return Err("the cut runs along the member".into());
        }
        // How far the plane reaches past the end over the section's box.
        let mut far: Real = 0.0;
        for (x, y) in [(-w, -h), (w, -h), (w, h), (-w, h)] {
            let z = p.z - (n.x * (x - p.x) + n.y * (y - p.y)) / n.z;
            far = far.max(if k == 0 { -z } else { z - length });
        }
        ext[k] = far.max(0.0) + 1.0;
        cuts.push([[p.x, p.y, p.z], [n.x, n.y, n.z]]);
    }
    let v = |a: &[Real; 3]| Vector3::new(a[0], a[1], a[2]);
    let notches = m
        .notches
        .iter()
        .map(|t| {
            let o = inv.transform_point(&Point3::from(v(&t.origin)));
            let (x, y) = (
                inv.transform_vector(&v(&t.x)),
                inv.transform_vector(&v(&t.y)),
            );
            NotchTool {
                origin: [o.x, o.y, o.z],
                x: [x.x, x.y, x.z],
                y: [y.x, y.y, y.z],
                ..t.clone()
            }
        })
        .collect();
    Ok(Shape {
        length,
        ext,
        cuts,
        notches,
        path: None,
    })
}

/// Follows the skeleton: every member of assembly `asm` gets the place and
/// length of its skeleton line now, and its end treatments again; member
/// parts whose shape changed are rebuilt (in parallel). A member that cannot
/// follow keeps its last state and is reported (health, rule 5).
pub fn solve(project: &mut Project, linked: &Linked, asm: DocumentId) -> Result<Solved, String> {
    let a = asm_of(project, asm)?;
    let mut out = Solved::default();
    let mut frames: Vec<(Vec<Live>, Vec<Treatment>)> = Vec::new();
    for f in &a.frames {
        let mut live = Vec::new();
        for m in &f.members {
            let Some(occ) = a.find(m.occurrence) else {
                continue;
            };
            if occ.component.library.is_some() {
                continue;
            }
            let doc = occ.component.document;
            // A reused member stands as its source's section does.
            let source = m
                .reuse_of
                .and_then(|s| f.members.iter().find(|x| x.occurrence == s))
                .unwrap_or(m);
            let sec = source.section();
            match (skeleton_run(project, linked, a, &m.skeleton), sec) {
                (Ok(run), Some(s)) => {
                    let placed = match run {
                        Run::Line(p, q) => placement(&s, p, q, &m.orientation).map(|t| {
                            let (start, end) = if m.orientation.flip { (q, p) } else { (p, q) };
                            (t, start, end, None)
                        }),
                        Run::Curve(c) => curved_place(&s, &c, &m.orientation).map(|(t, run)| {
                            let start = run.eval(0.0).0;
                            let end = run.eval(run.length()).0;
                            (t, start, end, Some(run))
                        }),
                    };
                    match placed {
                        Some((t, start, end, path)) => {
                            let material = match project.document(doc).map(|d| &d.data) {
                                Some(DocumentData::Part(x)) => x.material.clone(),
                                _ => None,
                            };
                            live.push(Live {
                                occ: m.occurrence,
                                doc,
                                section: s,
                                start,
                                end,
                                place: t,
                                cuts: [None, None],
                                mitred: [false, false],
                                reuse_of: m.reuse_of,
                                path,
                                notches: Vec::new(),
                                built: m.built.clone(),
                                material,
                            });
                        }
                        None => out
                            .sick
                            .push((m.occurrence, "its skeleton edge has no length".into())),
                    }
                }
                (Err(e), _) => out.sick.push((m.occurrence, e)),
                (_, None) => out
                    .sick
                    .push((m.occurrence, "its section data is unreadable".into())),
            }
        }
        frames.push((live, f.treatments.clone()));
    }
    // End treatments, in creation order (a later one at the same end wins).
    let mut jobs: Vec<(u32, DocumentId, Section, Shape, Option<String>)> = Vec::new();
    let mut places: Vec<(u32, Transform3)> = Vec::new();
    let mut shapes: Vec<(u32, Shape)> = Vec::new();
    let mut details: Vec<(DocumentId, [Option<String>; 2])> = Vec::new();
    let mut cap_places: Vec<(u32, Transform3)> = Vec::new();
    let face = |g: &GeomRef| -> Result<(Point3, Vector3), String> {
        let top = a
            .find(g.occurrence())
            .ok_or("its face's component no longer exists")?;
        match super::relation::resolve_with(project, linked, a, g)?.moved(&top.transform) {
            super::relation::Geom::Plane { p, n } => Ok((p, n)),
            _ => Err("only planar faces trim members yet".into()),
        }
    };
    for (mut live, treatments) in frames {
        // A curved member takes no end treatments (spec 6.1 Merge).
        let treatments: Vec<Treatment> = treatments
            .into_iter()
            .filter(|t| {
                let curved = t
                    .members()
                    .iter()
                    .any(|o| live.iter().any(|m| m.occ == *o && m.path.is_some()));
                if curved {
                    for m in t.members() {
                        out.sick.push((m, format!("{}: {CURVED}", t.label())));
                    }
                }
                !curved
            })
            .collect();
        for t in &treatments {
            match treatment_cuts(t, &live, &face) {
                Ok(cuts) => {
                    for (i, end, p, n) in cuts {
                        live[i].cuts[end] = Some((p, n));
                        live[i].mitred[end] = matches!(t, Treatment::Miter { .. });
                    }
                }
                Err(e) => {
                    for m in t.members() {
                        out.sick.push((m, format!("{}: {e}", t.label())));
                    }
                }
            }
        }
        for t in &treatments {
            if !matches!(t, Treatment::Notch { .. }) {
                continue;
            }
            match notch_tool(t, &live) {
                Ok((i, tool)) => live[i].notches.push(tool),
                Err(e) => {
                    for m in t.members() {
                        out.sick.push((m, format!("Notch: {e}")));
                    }
                }
            }
        }
        // Lengthen/Shorten last: a cut end moves along the member, a plain
        // end gets a square cut there.
        for t in &treatments {
            let Treatment::Lengthen { member, offsets } = t else {
                continue;
            };
            let Some(i) = live.iter().position(|m| m.occ == *member) else {
                continue;
            };
            for (e, &by) in offsets.iter().enumerate() {
                if by.abs() < 1e-12 {
                    continue;
                }
                let out_dir = -away(&live[i], e);
                let end = if e == 0 { live[i].start } else { live[i].end };
                live[i].cuts[e] = Some(match live[i].cuts[e] {
                    Some((p, n)) => (p + out_dir * by, n),
                    None => (end + out_dir * by, out_dir),
                });
            }
        }
        for m in &live {
            places.push((m.occ, m.place));
            if let Some(src) = m.reuse_of {
                // The line must stay as long as the part it shows.
                let own = (m.end - m.start).norm();
                if let Some(s) = live.iter().find(|x| x.occ == src)
                    && ((s.end - s.start).norm() - own).abs() > REUSE_TOLERANCE
                {
                    out.sick.push((
                        m.occ,
                        "its line is no longer as long as its source's".into(),
                    ));
                }
                continue;
            }
            details.push((m.doc, cut_details(m)));
            match shape_of(m) {
                Ok(sh) => {
                    let old = m.built.clone().unwrap_or_else(|| Shape {
                        length: part_length(project, m.doc).unwrap_or(0.0),
                        ..Default::default()
                    });
                    if let Some(why) = failed_before(m.doc, &sh) {
                        // The kernel failed on this very shape already: not
                        // again (a failing Notch took it 3–8 s each solve).
                        out.sick.push((m.occ, why));
                    } else if !old.same(&sh) {
                        jobs.push((
                            m.occ,
                            m.doc,
                            m.section.clone(),
                            sh.clone(),
                            m.material.clone(),
                        ));
                    }
                    shapes.push((m.occ, sh));
                }
                Err(e) => out.sick.push((m.occ, e)),
            }
        }
    }
    // Rebuild the changed members on all cores.
    let built: Vec<(u32, DocumentId, Shape, Result<PartDocument, String>)> =
        std::thread::scope(|sc| {
            let hs: Vec<_> = jobs
                .into_iter()
                .map(|(occ, doc, sec, sh, mat)| {
                    sc.spawn(move || {
                        let r = match &sh.path {
                            Some(path) => sec.member_part_path(path, mat.as_deref()),
                            None => sec.member_part_cut(
                                sh.length,
                                sh.ext,
                                &sh.end_cuts(),
                                &sh.notches,
                                mat.as_deref(),
                            ),
                        }
                        .map_err(|e| e.to_string())
                        .and_then(|p| {
                            if p.feature_health.is_empty() {
                                Ok(p)
                            } else {
                                Err(p
                                    .feature_health
                                    .values()
                                    .next()
                                    .cloned()
                                    .unwrap_or_default())
                            }
                        });
                        (occ, doc, sh, r)
                    })
                })
                .collect();
            hs.into_iter().filter_map(|h| h.join().ok()).collect()
        });
    let mut changed: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    let mut failed: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    for (occ, doc, sh, r) in built {
        match r {
            Ok(part) => {
                let outer = if sh.path.is_some() {
                    sh.length
                } else {
                    outer_length(&part).unwrap_or(sh.length)
                };
                if let Some(d) = project.document_mut(doc) {
                    d.data = DocumentData::Part(keep_looks(&d.data, part));
                    for c in &mut d.properties.custom {
                        if c.name == LENGTH_PROPERTY {
                            c.value = PropValue::Number(stored_length(outer));
                        }
                    }
                }
                changed.insert(occ);
            }
            Err(e) => {
                failed.insert(occ);
                remember_failure(doc, &sh, &e);
                out.sick.push((occ, e));
            }
        }
    }
    for (doc, cuts) in details {
        set_cut_details(project, doc, cuts);
    }
    let a = asm_of(project, asm)?;
    let mut rebuilt_caps: Vec<(u32, DocumentId, Section, CapOptions, Option<String>, String)> =
        Vec::new();
    for f in &a.frames {
        for c in &f.caps {
            let Some(m) = f.members.iter().find(|m| m.occurrence == c.member) else {
                continue;
            };
            let (Some((_, sh)), Some((_, place))) = (
                shapes.iter().find(|(o, _)| *o == c.member),
                places.iter().find(|(o, _)| *o == c.member),
            ) else {
                continue;
            };
            if sh.slanted(c.high) {
                out.sick
                    .push((c.occurrence, "its end is no longer square".into()));
                continue;
            }
            cap_places.push((c.occurrence, cap_placement(place, sh.z_range(), c)));
            let Some(sec) = m.section() else {
                continue;
            };
            let now = format!("{} {}", m.family, m.size);
            if now != c.built_for
                && let Some(o) = a.find(c.occurrence)
                && o.component.library.is_none()
            {
                rebuilt_caps.push((
                    c.occurrence,
                    o.component.document,
                    sec,
                    c.options,
                    part_material(project, o.component.document),
                    now,
                ));
            }
        }
    }
    for (occ, doc, sec, opts, mat, now) in rebuilt_caps {
        match cap_part(&sec, &opts, mat.as_deref()) {
            Ok((part, _)) => {
                if let Some(d) = project.document_mut(doc) {
                    d.data = DocumentData::Part(keep_looks(&d.data, part));
                }
                let a = asm_mut(project, asm)?;
                for c in a.frames.iter_mut().flat_map(|f| &mut f.caps) {
                    if c.occurrence == occ {
                        c.built_for = now.clone();
                    }
                }
                changed.insert(occ);
            }
            Err(e) => out.sick.push((occ, format!("End Cap: {e}"))),
        }
    }
    places.extend(cap_places);
    let a = asm_mut(project, asm)?;
    for (occ, t) in places {
        if let Some(o) = a.occurrences.iter_mut().find(|o| o.id == occ) {
            let d = (o.transform.translation.vector - t.translation.vector).norm()
                + o.transform.rotation.angle_to(&t.rotation);
            if d > 1e-9 {
                o.transform = t;
                changed.insert(occ);
            }
        }
    }
    for f in &mut a.frames {
        for m in &mut f.members {
            if let Some((_, sh)) = shapes.iter().find(|(o, _)| *o == m.occurrence)
                && !failed.contains(&m.occurrence)
            {
                m.built = Some(sh.clone());
            }
        }
    }
    out.moved = changed.len();
    Ok(out)
}

/// How much a reused member's line may differ from its source's (mm).
const REUSE_TOLERANCE: Real = 1e-3;

/// What each end of `m` is cut like, from the final geometry: the angle off
/// square (a saw's setting); a square end has no text.
fn cut_details(m: &Live) -> [Option<String>; 2] {
    let axis = (m.end - m.start).try_normalize(1e-9);
    let at = |e: usize| {
        let (_, n) = m.cuts[e]?;
        let cos = n.try_normalize(1e-12)?.dot(&axis?).abs().min(1.0);
        let deg = cos.acos().to_degrees();
        (deg >= 0.005).then(|| {
            let kind = if m.mitred[e] { "Miter" } else { "Cut" };
            format!("{kind} {deg:.2} deg")
        })
    };
    [at(0), at(1)]
}

/// Writes a member part's cut details (nothing when they are the same, so a
/// solve that changes nothing leaves the file unchanged).
fn set_cut_details(project: &mut Project, doc: DocumentId, cuts: [Option<String>; 2]) {
    let Some(d) = project.document_mut(doc) else {
        return;
    };
    let custom = &mut d.properties.custom;
    // A length saved before it was rounded.
    for c in custom.iter_mut().filter(|c| c.name == LENGTH_PROPERTY) {
        if let PropValue::Number(l) = c.value
            && stored_length(l) != l
        {
            c.value = PropValue::Number(stored_length(l));
        }
    }
    for (name, text) in CUT_PROPERTIES.iter().zip(cuts) {
        let now = custom.iter().position(|c| c.name == *name);
        match (now, text) {
            (Some(k), None) => {
                custom.remove(k);
            }
            (Some(k), Some(t)) => {
                let v = PropValue::Text(t);
                if custom[k].value != v {
                    custom[k].value = v;
                }
            }
            (None, Some(t)) => custom.push(CustomProperty {
                name: (*name).to_owned(),
                value: PropValue::Text(t),
            }),
            (None, None) => {}
        }
    }
}

/// Member shapes the kernel failed on (document, shape, why), this run only.
static FAILED: std::sync::Mutex<Vec<(DocumentId, Shape, String)>> =
    std::sync::Mutex::new(Vec::new());

/// Why the kernel failed on member part `doc` with shape `sh`, if it did.
fn failed_before(doc: DocumentId, sh: &Shape) -> Option<String> {
    let f = FAILED.lock().ok()?;
    f.iter()
        .find(|(d, s, _)| *d == doc && s.same(sh))
        .map(|(_, _, why)| why.clone())
}

fn remember_failure(doc: DocumentId, sh: &Shape, why: &str) {
    if let Ok(mut f) = FAILED.lock() {
        if f.len() > 256 {
            f.remove(0);
        }
        f.push((doc, sh.clone(), why.to_owned()));
    }
}

/// The member part's length along its axis (the body's extent in Z):
/// the outer length of a mitred member, its `G_L`.
pub fn outer_length(part: &PartDocument) -> Option<Real> {
    let (lo, hi) = part.body.bounding_box()?;
    Some(hi.z - lo.z)
}

fn part_length(project: &Project, doc: DocumentId) -> Option<Real> {
    match project.document(doc).map(|d| &d.data) {
        Some(DocumentData::Part(p)) => length_of(p),
        _ => None,
    }
}

/// Miter: adds a Full Miter Cut between members `a` and `b` of one frame of
/// assembly `asm` (replacing their earlier treatments at that joint when
/// `replace`), then solves. On an error nothing is changed (rule 5).
pub fn add_miter(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    pairs: &[(u32, u32)],
    gap: Real,
    offset: MiterOffset,
) -> Result<Solved, String> {
    add_miter_typed(project, linked, asm, pairs, gap, offset, true, false)
}

/// [`add_miter`] of either type: `full` = Full Miter Cut (edge to edge),
/// else Bisect Miter Cut; `delete_existing` also drops the pair's other
/// joints (a Corner Joint, a Notch between them).
#[allow(clippy::too_many_arguments)]
pub fn add_miter_typed(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    pairs: &[(u32, u32)],
    gap: Real,
    offset: MiterOffset,
    full: bool,
    delete_existing: bool,
) -> Result<Solved, String> {
    if pairs.is_empty() {
        return Err("select two frame members that meet".into());
    }
    if !(gap.is_finite() && gap >= 0.0) {
        return Err("the distance must be zero or more".into());
    }
    let before = project.clone();
    let result = (|| {
        let a = asm_mut(project, asm)?;
        for &(x, y) in pairs {
            merge_frames_of(a, &[x, y]);
            let f = a
                .frames
                .iter_mut()
                .find(|f| {
                    f.members.iter().any(|m| m.occurrence == x)
                        && f.members.iter().any(|m| m.occurrence == y)
                })
                .ok_or("both members must be frame members")?;
            f.treatments.retain(|t| {
                let m = t.members();
                let pair = m.contains(&x) && m.contains(&y);
                !(pair && (delete_existing || matches!(t, Treatment::Miter { .. })))
            });
            f.treatments.push(Treatment::Miter {
                a: x,
                b: y,
                gap,
                offset,
                full,
            });
        }
        let s = solve(project, linked, asm)?;
        let pick: Vec<u32> = pairs.iter().flat_map(|&(x, y)| [x, y]).collect();
        if let Some((_, why)) = s.sick.iter().find(|(o, _)| pick.contains(o)) {
            return Err(why.trim_start_matches("Miter: ").to_owned());
        }
        Ok(s)
    })();
    if result.is_err() {
        *project = before;
    }
    result
}

/// Adds end treatments `ts` to the frame(s) of their members (an earlier one
/// of the same kind on the same members is replaced), then solves. A member
/// a new treatment leaves sick refuses the whole step (rule 5).
pub fn add_treatments(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    ts: Vec<Treatment>,
) -> Result<Solved, String> {
    add_treatments_replacing(project, linked, asm, ts, false)
}

/// [`add_treatments`]; with `delete_existing` (Advanced Settings › Delete
/// existing end treatments) a joint of two members also replaces their
/// earlier joint of another kind (a Miter replaces their Corner Joint).
pub fn add_treatments_replacing(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    ts: Vec<Treatment>,
    delete_existing: bool,
) -> Result<Solved, String> {
    if ts.is_empty() {
        return Err("select the frame members".into());
    }
    let before = project.clone();
    let result = (|| {
        let a = asm_mut(project, asm)?;
        let mut touched = Vec::new();
        for t in ts {
            let ms = t.members();
            if a.frames
                .iter()
                .flat_map(|f| &f.members)
                .any(|m| ms.contains(&m.occurrence) && m.reuse_of.is_some())
            {
                return Err("a reused member's end treatments are changed on its source".into());
            }
            merge_frames_of(a, &ms);
            let f = a
                .frames
                .iter_mut()
                .find(|f| {
                    ms.iter()
                        .all(|o| f.members.iter().any(|m| m.occurrence == *o))
                })
                .ok_or("the members must be frame members")?;
            f.treatments.retain(|x| {
                let same = std::mem::discriminant(x) == std::mem::discriminant(&t);
                !(same && x.members() == ms || delete_existing && same_joint(x, &ms))
            });
            touched.extend(ms);
            f.treatments.push(t);
        }
        let s = solve(project, linked, asm)?;
        if let Some((_, why)) = s.sick.iter().find(|(o, _)| touched.contains(o)) {
            return Err(why.clone());
        }
        Ok(s)
    })();
    if result.is_err() {
        *project = before;
    }
    result
}

/// Is `x` a joint of exactly the two members `ms` (either way round)?
fn same_joint(x: &Treatment, ms: &[u32]) -> bool {
    let m = x.members();
    ms.len() == 2 && m.len() == 2 && m.contains(&ms[0]) && m.contains(&ms[1])
}

/// Members of different frames treated together (a Trim/Extend, a corner
/// between frames inserted apart): their frames become one, the first's
/// (members and treatments moved over). Inventor keeps one frame per
/// assembly that Insert Frame adds to; files with several keep working.
pub fn merge_frames_of(a: &mut AssemblyDocument, members: &[u32]) {
    let idx: Vec<usize> = a
        .frames
        .iter()
        .enumerate()
        .filter(|(_, f)| f.members.iter().any(|m| members.contains(&m.occurrence)))
        .map(|(i, _)| i)
        .collect();
    let [keep, rest @ ..] = idx.as_slice() else {
        return;
    };
    // From the last, so the indices stay valid.
    for &i in rest.iter().rev() {
        let f = a.frames.remove(i);
        let k = if i < *keep { *keep - 1 } else { *keep };
        a.frames[k].members.extend(f.members);
        a.frames[k].treatments.extend(f.treatments);
    }
}

/// Change: members `members` get the section, material and orientation of
/// `o`; their end treatments are kept and solved again. On an error nothing
/// changes (rule 5).
pub fn change(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    members: &[u32],
    o: &InsertOptions,
    lib: &Library,
) -> Result<Solved, String> {
    let edits: Vec<MemberEdit> = members
        .iter()
        .map(|&occurrence| MemberEdit {
            occurrence,
            skeleton: None,
            orientation: o.orientation,
        })
        .collect();
    edit_members(project, linked, asm, &edits, o, lib)
}

/// Change of several members (spec 6.1 §3.3): only what the panel changed
/// from `base` (the values it showed when it was filled) is given to every
/// member; each member keeps its own section, material, appearance and each
/// orientation value the user left as it was. On an error nothing changes.
pub fn change_keeping(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    members: &[u32],
    o: &InsertOptions,
    base: &InsertOptions,
    lib: &Library,
) -> Result<Solved, String> {
    let a = asm_of(project, asm)?;
    let edits: Vec<MemberEdit> = members
        .iter()
        .map(|&occurrence| MemberEdit {
            occurrence,
            skeleton: None,
            orientation: merge_orientation(
                orientation_of(a, occurrence).unwrap_or(base.orientation),
                &o.orientation,
                &base.orientation,
            ),
        })
        .collect();
    edit_members_in(project, linked, asm, &edits, o, lib, Some(base))
}

/// `own` with every value of `new` that differs from `base`.
pub fn merge_orientation(own: Orientation, new: &Orientation, base: &Orientation) -> Orientation {
    let pick = |n: Real, b: Real, o: Real| if (n - b).abs() > 1e-9 { n } else { o };
    Orientation {
        anchor: if new.anchor != base.anchor {
            new.anchor
        } else {
            own.anchor
        },
        h_offset: pick(new.h_offset, base.h_offset, own.h_offset),
        v_offset: pick(new.v_offset, base.v_offset, own.v_offset),
        angle: pick(new.angle, base.angle, own.angle),
        about_anchor: if new.about_anchor != base.about_anchor {
            new.about_anchor
        } else {
            own.about_anchor
        },
        mirror: if new.mirror != base.mirror {
            new.mirror
        } else {
            own.mirror
        },
        flip: if new.flip != base.flip {
            new.flip
        } else {
            own.flip
        },
        align: if new.align != base.align {
            new.align
        } else {
            own.align
        },
    }
}

/// One member's new place for [`edit_members`].
#[derive(Clone, Debug, PartialEq)]
pub struct MemberEdit {
    /// The member (occurrence id).
    pub occurrence: u32,
    /// A new skeleton line (`None`: keep it).
    pub skeleton: Option<Skeleton>,
    /// Its orientation (before `o.layout`, when that is not Center).
    pub orientation: Orientation,
}

/// A member's new anchor, at once (Edit Frame Members' points): only its
/// place changes, then the frame is solved (its corners follow). On an
/// error nothing changes.
pub fn set_anchor(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    occ: u32,
    anchor: Anchor,
) -> Result<Solved, String> {
    let now =
        orientation_of(asm_of(project, asm)?, occ).ok_or("only frame members have an anchor")?;
    set_orientations(
        project,
        linked,
        asm,
        &[(occ, Orientation { anchor, ..now })],
    )
}

/// Members' new orientations at once (the browser's Position menu of a
/// member, spec 6.1 §3.9): only their places change, then the frame is
/// solved (corners follow). A reused member is refused (it stands as its
/// source). On an error nothing changes (rule 5).
pub fn set_orientations(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    changes: &[(u32, Orientation)],
) -> Result<Solved, String> {
    if changes.is_empty() {
        return Err("select the frame members".into());
    }
    let before = project.clone();
    let result = (|| {
        let a = asm_mut(project, asm)?;
        for (occ, o) in changes {
            if !(o.h_offset.is_finite() && o.v_offset.is_finite() && o.angle.is_finite()) {
                return Err("the offsets and the angle must be numbers".into());
            }
            let m = a
                .frames
                .iter_mut()
                .flat_map(|f| &mut f.members)
                .find(|m| m.occurrence == *occ)
                .ok_or("only frame members have an orientation")?;
            if m.reuse_of.is_some() {
                return Err("a reused member is placed as its source".into());
            }
            m.orientation = *o;
        }
        let s = solve(project, linked, asm)?;
        if let Some((_, why)) = s
            .sick
            .iter()
            .find(|(o, _)| changes.iter().any(|c| c.0 == *o))
        {
            return Err(why.clone());
        }
        Ok(s)
    })();
    if result.is_err() {
        *project = before;
    }
    result
}

/// A member's orientation now (`None`: no frame member).
pub fn orientation_of(a: &AssemblyDocument, occ: u32) -> Option<Orientation> {
    a.frames
        .iter()
        .flat_map(|f| &f.members)
        .find(|m| m.occurrence == occ)
        .map(|m| m.orientation)
}

/// Edit of frame members (Inventor's edit of a member with the Insert
/// Frame panel): each member of `edits` gets its skeleton line and
/// orientation, all of them the section and material of `o` (and its
/// Position / Level about the centre of their frame's lines); treatments are
/// kept and solved again. On an error nothing changes (rule 5).
pub fn edit_members(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    edits: &[MemberEdit],
    o: &InsertOptions,
    lib: &Library,
) -> Result<Solved, String> {
    edit_members_in(project, linked, asm, edits, o, lib, None)
}

/// [`edit_members`]; with `keep`, a member keeps its own section when `o`'s
/// is `keep`'s, its material and appearance likewise ([`change_keeping`]).
fn edit_members_in(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    edits: &[MemberEdit],
    o: &InsertOptions,
    lib: &Library,
    keep: Option<&InsertOptions>,
) -> Result<Solved, String> {
    if edits.is_empty() {
        return Err("select the frame members".into());
    }
    let chosen = lib
        .find(&o.family, &o.size)
        .ok_or_else(|| format!("{} {} is not in the library", o.family, o.size))?
        .clone();
    let same_section = keep.is_some_and(|b| b.family == o.family && b.size == o.size);
    let same_material = keep.is_some_and(|b| b.material == o.material);
    let set_look = match keep {
        Some(b) => b.appearance != o.appearance || b.appearance_to_part != o.appearance_to_part,
        None => o.appearance.is_some(),
    };
    let members: Vec<u32> = edits.iter().map(|e| e.occurrence).collect();
    let before = project.clone();
    let result = (|| {
        for e in edits {
            let occ = e.occurrence;
            let a = asm_of(project, asm)?;
            let f = a
                .frames
                .iter()
                .find(|f| f.members.iter().any(|m| m.occurrence == occ))
                .ok_or("only frame members can be changed")?;
            let m = f
                .members
                .iter()
                .find(|m| m.occurrence == occ)
                .ok_or("only frame members can be changed")?;
            if m.reuse_of.is_some() {
                return Err("a reused member is changed on its source".into());
            }
            let doc = a
                .find(occ)
                .map(|x| x.component)
                .filter(|c| c.library.is_none())
                .ok_or("the member's part is not in this file")?
                .document;
            let section = match same_section.then(|| m.section()).flatten() {
                Some(s) => s,
                None => chosen.clone(),
            };
            let material = if same_material {
                part_material(project, doc)
            } else {
                o.material.clone()
            };
            let skeleton = e.skeleton.clone().unwrap_or_else(|| m.skeleton.clone());
            let run = skeleton_run(project, linked, a, &skeleton)?;
            let (p, q) = run.chord();
            // A panel without an Align pick keeps the member's.
            let mut given = e.orientation;
            if given.align.is_none() {
                given.align = m.orientation.align;
            }
            let orientation = if o.layout == Layout::default() || matches!(run, Run::Curve(_)) {
                given
            } else {
                let lines: Vec<(Point3, Point3)> = f
                    .members
                    .iter()
                    .filter_map(|x| {
                        if x.occurrence == occ {
                            Some((p, q))
                        } else {
                            skeleton_line(project, linked, a, &x.skeleton).ok()
                        }
                    })
                    .collect();
                o.layout.orient(&given, p, q, centre_of(&lines))
            };
            let length = run.length();
            let (part, path) = match &run {
                Run::Line(..) => (
                    section
                        .member_part(length, material.as_deref())
                        .map_err(|e| e.to_string())?,
                    None,
                ),
                Run::Curve(c) => {
                    let (place, r) = curved_place(&section, c, &orientation)
                        .ok_or("the skeleton curve has no length")?;
                    let pieces = member_pieces(&r, &place.inverse());
                    (
                        section
                            .member_part_path(&pieces, material.as_deref())
                            .map_err(|e| e.to_string())?,
                        Some(pieces),
                    )
                }
            };
            if let Some(d) = project.document_mut(doc) {
                d.data = DocumentData::Part(keep_looks(&d.data, part));
                d.properties = section.member_properties(length);
            }
            // A generated name follows the section (it kept saying the old size); a name the
            // user gave stays.
            let stock = section.stock_number();
            let renamed = project.document(doc).and_then(|d| {
                let (head, tail) = d.name.rsplit_once('-')?;
                let generated = !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit());
                (generated && head != stock).then_some(())
            });
            let new_name = renamed.map(|()| project.free_name(&format!("{stock}-")));
            if let (Some(n), Some(d)) = (&new_name, project.document_mut(doc)) {
                d.name = n.clone();
            }
            let a = asm_mut(project, asm)?;
            if let (Some(n), Some(o)) = (&new_name, a.find_mut(occ)) {
                o.name = format!("{n}:1");
            }
            if let Some(m) = a
                .frames
                .iter_mut()
                .flat_map(|f| &mut f.members)
                .find(|m| m.occurrence == occ)
            {
                m.skeleton = skeleton;
                m.family = section.family.clone();
                m.size = section.size.clone();
                m.section = SectionData::of(&section);
                m.orientation = orientation;
                // The part is uncut now: solve cuts it again.
                m.built = Some(Shape {
                    length,
                    path,
                    ..Default::default()
                });
            }
            if set_look {
                set_member_appearance(
                    project,
                    asm,
                    occ,
                    doc,
                    o.appearance.as_deref(),
                    o.appearance_to_part,
                )?;
            }
        }
        let s = solve(project, linked, asm)?;
        if let Some((_, why)) = s.sick.iter().find(|(o, _)| members.contains(o)) {
            return Err(why.clone());
        }
        Ok(s)
    })();
    if result.is_err() {
        *project = before;
    }
    result
}

/// The end treatments of every frame of `a` with their browser names
/// (`Miter 1`, `Corner Joint 2`: numbered per command in creation order):
/// (frame id, index in the frame, name, treatment).
pub fn treatment_rows(a: &AssemblyDocument) -> Vec<(u32, usize, String, &Treatment)> {
    let mut count: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut out = Vec::new();
    for f in &a.frames {
        for (i, t) in f.treatments.iter().enumerate() {
            let n = count.entry(t.label()).or_default();
            *n += 1;
            out.push((f.id, i, format!("{} {n}", t.label()), t));
        }
    }
    out
}

/// Takes end treatment `index` out of frame `frame` (no solve: Edit puts a
/// new one in its place through [`add_treatments`] / [`add_miter`]).
pub fn take_treatment(a: &mut AssemblyDocument, frame: u32, index: usize) -> Option<Treatment> {
    let f = a.frames.iter_mut().find(|f| f.id == frame)?;
    (index < f.treatments.len()).then(|| f.treatments.remove(index))
}

/// Changes corner `index` of frame `frame` (a Miter or a Corner Joint) to
/// `kind`, keeping its members (in their first / second order) and gap; on
/// an error nothing changes.
pub fn retype_corner(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    frame: u32,
    index: usize,
    kind: CornerType,
) -> Result<Solved, String> {
    let before = project.clone();
    let result = (|| {
        let f = asm_mut(project, asm)?
            .frames
            .iter_mut()
            .find(|f| f.id == frame)
            .ok_or("no such frame")?;
        let t = f.treatments.get(index).ok_or("no such end treatment")?;
        let (a, b, gap) = match t {
            Treatment::Miter { a, b, gap, .. } => (*a, *b, *gap),
            // The member that stops is the second; it carries the gap.
            Treatment::CornerJoint { a, b, offsets } => (*a, *b, offsets[1]),
            _ => return Err("only a Miter or a Corner Joint is a corner".into()),
        };
        let len = |o: u32| {
            let m = f.members.iter().find(|m| m.occurrence == o)?;
            m.built.as_ref().map(|s| s.length)
        };
        let lengths = [len(a).unwrap_or(0.0), len(b).unwrap_or(0.0)];
        let t = f.treatments.get_mut(index).ok_or("no such end treatment")?;
        *t = Corners {
            kind,
            gap,
            tees: TeeJoint::None,
        }
        .treatment(a, b, lengths)
        .ok_or("a corner cannot become uncut here: Delete it")?;
        let s = solve(project, linked, asm)?;
        if let Some((_, why)) = s.sick.iter().find(|(o, _)| *o == a || *o == b) {
            return Err(why.clone());
        }
        Ok(s)
    })();
    if result.is_err() {
        *project = before;
    }
    result
}

/// Deletes end treatment `index` of frame `frame`; its members are rebuilt.
pub fn delete_treatment(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    frame: u32,
    index: usize,
) -> Result<Solved, String> {
    take_treatment(asm_mut(project, asm)?, frame, index).ok_or("no such end treatment")?;
    solve(project, linked, asm)
}

/// Deletes frame members: their occurrences, their parts (when nothing
/// else places them) and every end treatment they take part in; a frame
/// left without members goes too.
pub fn delete_members(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    members: &[u32],
) -> Result<Solved, String> {
    let before = project.clone();
    let result = (|| {
        let a = asm_mut(project, asm)?;
        let mut docs = Vec::new();
        let mut caps = Vec::new();
        for &occ in members {
            if !a
                .frames
                .iter()
                .any(|f| f.members.iter().any(|m| m.occurrence == occ))
            {
                return Err("only frame members can be deleted here".to_owned());
            }
            for f in &mut a.frames {
                promote_reuse(f, occ);
                f.members.retain(|m| m.occurrence != occ);
                f.treatments.retain(|t| !t.members().contains(&occ));
                for c in f.caps.iter().filter(|c| c.member == occ) {
                    caps.push(c.occurrence);
                }
                f.caps.retain(|c| c.member != occ);
            }
            for c in caps.drain(..) {
                if let Some(o) = a.remove(c)
                    && o.component.library.is_none()
                {
                    docs.push(o.component.document);
                }
            }
            if let Some(o) = a.remove(occ)
                && o.component.library.is_none()
            {
                docs.push(o.component.document);
            }
        }
        a.frames.retain(|f| !f.members.is_empty());
        for d in docs {
            // Still placed elsewhere (a reused member): kept.
            let _ = project.remove_document(d);
        }
        solve(project, linked, asm)
    })();
    if result.is_err() {
        *project = before;
    }
    result
}

/// Before source `gone` is deleted: the first member reusing it becomes the
/// source of the others.
fn promote_reuse(f: &mut Frame, gone: u32) {
    let Some(next) = f
        .members
        .iter()
        .find(|m| m.reuse_of == Some(gone))
        .map(|m| m.occurrence)
    else {
        return;
    };
    let built = f
        .members
        .iter()
        .find(|m| m.occurrence == gone)
        .and_then(|m| m.built.clone());
    for m in &mut f.members {
        if m.occurrence == next {
            m.reuse_of = None;
            m.built = built.clone();
        } else if m.reuse_of == Some(gone) {
            m.reuse_of = Some(next);
        }
    }
}

/// Reuse: further occurrences of `source`'s part on `lines` of the same
/// length (a reused member names its own source). On an error nothing is
/// changed. Returns the new occurrences.
pub fn reuse(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    source: u32,
    lines: &[Skeleton],
    flip: bool,
) -> Result<Vec<u32>, String> {
    if lines.is_empty() {
        return Err("select the lines to reuse the member on".into());
    }
    let before = project.clone();
    let result = (|| {
        let a = asm_of(project, asm)?;
        let found = |id: u32| {
            a.frames.iter().enumerate().find_map(|(k, f)| {
                f.members
                    .iter()
                    .find(|m| m.occurrence == id)
                    .map(|m| (k, m))
            })
        };
        let (frame, m) = found(source).ok_or("that is not a frame member")?;
        let (frame, m) = match m.reuse_of {
            Some(s) => found(s).unwrap_or((frame, m)),
            None => (frame, m),
        };
        let src = m.occurrence;
        let o = a.find(src).ok_or("the member no longer exists")?;
        let component = o.component;
        let base = o.name.split(':').next().unwrap_or(&o.name).to_owned();
        let (p, q) = skeleton_line(project, linked, a, &m.skeleton)?;
        let want = (q - p).norm();
        let template = m.clone();
        let mut new = Vec::new();
        for line in lines {
            let (p, q) = skeleton_line(project, linked, a, line)?;
            let len = (q - p).norm();
            if (len - want).abs() > REUSE_TOLERANCE {
                return Err(format!(
                    "a line is {len:.3} mm long, the source's {want:.3} mm"
                ));
            }
            new.push(line.clone());
        }
        let mut made = Vec::new();
        let a = asm_mut(project, asm)?;
        for skeleton in new {
            let occ = a.place(component, &base, Transform3::identity(), true);
            let mut orientation = template.orientation;
            orientation.flip ^= flip;
            a.frames[frame].members.push(FrameMember {
                occurrence: occ,
                skeleton,
                orientation,
                built: None,
                reuse_of: Some(src),
                ..template.clone()
            });
            made.push(occ);
        }
        let s = solve(project, linked, asm)?;
        if let Some((_, why)) = s.sick.iter().find(|(m, _)| made.contains(m)) {
            return Err(why.clone());
        }
        Ok(made)
    })();
    if result.is_err() {
        *project = before;
    }
    result
}

/// Change Reuse (spec 6.1 §3.5): reused members `members` move to `lines`
/// (one line each, in order; empty keeps their lines) and run the source's
/// way, or the other way with `flip`. Lines must be as long as the source's.
/// On an error nothing changes (rule 5).
pub fn change_reuse(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    members: &[u32],
    lines: &[Skeleton],
    flip: bool,
) -> Result<Solved, String> {
    if members.is_empty() {
        return Err("select the reused members".into());
    }
    if !lines.is_empty() && lines.len() != members.len() {
        return Err(format!(
            "select one line for each reused member ({} members, {} lines)",
            members.len(),
            lines.len()
        ));
    }
    let before = project.clone();
    let result = (|| {
        let mut moves = Vec::new();
        {
            let a = asm_of(project, asm)?;
            let all: Vec<&FrameMember> = a.frames.iter().flat_map(|f| &f.members).collect();
            for (k, &occ) in members.iter().enumerate() {
                let m = all
                    .iter()
                    .find(|m| m.occurrence == occ)
                    .ok_or("only frame members can be changed")?;
                let src = m
                    .reuse_of
                    .ok_or("only reused members can be changed here")?;
                let source = all
                    .iter()
                    .find(|m| m.occurrence == src)
                    .ok_or("the source member no longer exists")?;
                let skeleton = match lines.get(k) {
                    Some(l) => {
                        let (p, q) = skeleton_line(project, linked, a, &source.skeleton)?;
                        let (r, s) = skeleton_line(project, linked, a, l)?;
                        let (want, len) = ((q - p).norm(), (s - r).norm());
                        if (len - want).abs() > REUSE_TOLERANCE {
                            return Err(format!(
                                "a line is {len:.3} mm long, the source's {want:.3} mm"
                            ));
                        }
                        l.clone()
                    }
                    None => m.skeleton.clone(),
                };
                let mut orientation = source.orientation;
                orientation.flip ^= flip;
                moves.push((occ, skeleton, orientation));
            }
        }
        let a = asm_mut(project, asm)?;
        for (occ, skeleton, orientation) in moves {
            if let Some(m) = a
                .frames
                .iter_mut()
                .flat_map(|f| &mut f.members)
                .find(|m| m.occurrence == occ)
            {
                m.skeleton = skeleton;
                m.orientation = orientation;
            }
        }
        let s = solve(project, linked, asm)?;
        if let Some((_, why)) = s.sick.iter().find(|(o, _)| members.contains(o)) {
            return Err(why.clone());
        }
        Ok(s)
    })();
    if result.is_err() {
        *project = before;
    }
    result
}

/// Refresh (spec 6.1 §3.5): members `members` (empty: every member of the
/// assembly's frames) whose section numbers differ from the library's entry
/// of their family and size get the library's and are rebuilt; everything
/// else of them stays. A size no longer in the library keeps its data;
/// reused members follow their source. Returns the members updated. On an
/// error nothing changes.
pub fn refresh(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    members: &[u32],
    lib: &Library,
) -> Result<Vec<u32>, String> {
    let a = asm_of(project, asm)?;
    let mut groups: Vec<((String, String), Vec<u32>)> = Vec::new();
    for m in a.frames.iter().flat_map(|f| &f.members) {
        if !members.is_empty() && !members.contains(&m.occurrence) || m.reuse_of.is_some() {
            continue;
        }
        let Some(s) = lib.find(&m.family, &m.size) else {
            continue;
        };
        if SectionData::of(s) == m.section {
            continue;
        }
        let key = (m.family.clone(), m.size.clone());
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.1.push(m.occurrence),
            None => groups.push((key, vec![m.occurrence])),
        }
    }
    let before = project.clone();
    let mut done = Vec::new();
    for ((family, size), occs) in groups {
        // The section counts as changed, the rest as kept (change_keeping).
        let o = InsertOptions {
            family,
            size,
            material: None,
            orientation: Orientation::default(),
            corners: Corners::default(),
            layout: Layout::default(),
            appearance: None,
            appearance_to_part: false,
        };
        let base = InsertOptions {
            family: String::new(),
            ..o.clone()
        };
        if let Err(e) = change_keeping(project, linked, asm, &occs, &o, &base, lib) {
            *project = before;
            return Err(e);
        }
        done.extend(occs);
    }
    Ok(done)
}

fn part_material(project: &Project, doc: DocumentId) -> Option<String> {
    match project.document(doc).map(|d| &d.data) {
        Some(DocumentData::Part(p)) => p.material.clone(),
        _ => None,
    }
}

/// The plate part of an end cap on section `s`, and its name's base.
fn cap_part(
    s: &Section,
    o: &CapOptions,
    material: Option<&str>,
) -> Result<(PartDocument, String), String> {
    if !(o.thickness.is_finite() && o.thickness > 1e-6) {
        return Err("the thickness must be more than zero".into());
    }
    if !o.offset.is_finite() {
        return Err("the offset is not a number".into());
    }
    let plate = s.cap_section(o.internal, o.corners, o.inset(s))?;
    let part = plate
        .member_part(o.thickness, material)
        .map_err(|e| e.to_string())?;
    if let Some(why) = part.feature_health.values().next() {
        return Err(why.clone());
    }
    let (w, h) = s.extent();
    Ok((part, format!("End Cap {w:.0}x{h:.0}x{:.1}", o.thickness)))
}

/// Where the plate of cap `c` stands: on the member placed by `member`, whose
/// part spans `z` along its Z.
fn cap_placement(member: &Transform3, z: (Real, Real), c: &EndCap) -> Transform3 {
    let t = c.options.thickness;
    let off = c.options.offset;
    let at = match (c.high, c.options.internal) {
        (true, false) => z.1 + off,
        (false, false) => z.0 - off - t,
        (true, true) => z.1 - off - t,
        (false, true) => z.0 + off,
    };
    let turn = nalgebra::UnitQuaternion::from_axis_angle(&Vector3::z_axis(), c.options.rotate);
    *member * Transform3::from_parts(Vector3::new(0.0, 0.0, at).into(), turn)
}

/// Insert End Cap: a plate on each (member, high end) of `ends`; a part per
/// member section, placed where [`solve`] keeps it. On an error nothing is
/// changed. Returns the new occurrences.
pub fn insert_caps(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    ends: &[(u32, bool)],
    o: &CapOptions,
    material: Option<&str>,
) -> Result<Vec<u32>, String> {
    if ends.is_empty() {
        return Err("select the end faces of the members".into());
    }
    let before = project.clone();
    let result = (|| {
        let mut made = Vec::new();
        for &(member, high) in ends {
            let a = asm_of(project, asm)?;
            let frame = a
                .frames
                .iter()
                .position(|f| f.members.iter().any(|m| m.occurrence == member))
                .ok_or("that is not a frame member")?;
            let f = &a.frames[frame];
            if f.caps.iter().any(|c| c.member == member && c.high == high) {
                return Err("that end has a cap already".into());
            }
            let m = f
                .members
                .iter()
                .find(|m| m.occurrence == member)
                .ok_or("that is not a frame member")?;
            if m.reuse_of.is_some() {
                return Err("a cap goes on the source of a reused member".into());
            }
            let sec = m.section().ok_or("its section data is unreadable")?;
            if m.built.as_ref().is_some_and(|b| b.slanted(high)) {
                return Err("a cap goes on a square end".into());
            }
            let built_for = format!("{} {}", m.family, m.size);
            let (part, base) = cap_part(&sec, o, material)?;
            let name = project.free_name(&format!("{base}-"));
            let doc = project.add_document(name.clone(), DocumentData::Part(part));
            if let Some(d) = project.document_mut(doc) {
                d.properties.set("stock_number", &base);
                d.properties.set("description", "End cap");
            }
            let a = asm_mut(project, asm)?;
            let occurrence = a.place(DocRef::local(doc), &name, Transform3::identity(), true);
            a.frames[frame].caps.push(EndCap {
                occurrence,
                member,
                high,
                options: *o,
                built_for,
            });
            made.push(occurrence);
        }
        let s = solve(project, linked, asm)?;
        if let Some((_, why)) = s.sick.iter().find(|(m, _)| made.contains(m)) {
            return Err(why.clone());
        }
        Ok(made)
    })();
    if result.is_err() {
        *project = before;
    }
    result
}

/// Edit of end caps (Edit with Frame Generator on a cap, spec 6.1 §3.2):
/// caps `caps` (occurrences) are rebuilt with `o` and `material` on the
/// ends they close. On an error nothing changes.
pub fn edit_caps(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    caps: &[u32],
    o: &CapOptions,
    material: Option<&str>,
) -> Result<Solved, String> {
    if caps.is_empty() {
        return Err("select the end caps".into());
    }
    let before = project.clone();
    let result = (|| {
        for &c in caps {
            let a = asm_of(project, asm)?;
            let (k, cap) = a
                .frames
                .iter()
                .enumerate()
                .find_map(|(k, f)| f.caps.iter().find(|x| x.occurrence == c).map(|x| (k, x)))
                .ok_or("that is not an end cap")?;
            let m = a.frames[k]
                .members
                .iter()
                .find(|m| m.occurrence == cap.member)
                .ok_or("the cap's member no longer exists")?;
            let sec = m.section().ok_or("its section data is unreadable")?;
            let built_for = format!("{} {}", m.family, m.size);
            let doc = a
                .find(c)
                .map(|x| x.component)
                .filter(|x| x.library.is_none())
                .ok_or("the cap's part is not in this file")?
                .document;
            let (part, base) = cap_part(&sec, o, material)?;
            if let Some(d) = project.document_mut(doc) {
                d.data = DocumentData::Part(keep_looks(&d.data, part));
                d.properties.set("stock_number", &base);
            }
            let a = asm_mut(project, asm)?;
            if let Some(x) = a.frames[k].caps.iter_mut().find(|x| x.occurrence == c) {
                x.options = *o;
                x.built_for = built_for;
            }
        }
        let s = solve(project, linked, asm)?;
        if let Some((_, why)) = s.sick.iter().find(|(m, _)| caps.contains(m)) {
            return Err(why.clone());
        }
        Ok(s)
    })();
    if result.is_err() {
        *project = before;
    }
    result
}

/// One row of the Frame Member Naming dialog (spec 6.1 §3.1): a member's
/// occurrence, its Display Name (the occurrence's) and File Name (its part
/// document's).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberName {
    /// The member's occurrence.
    pub occurrence: u32,
    /// Display Name in the browser.
    pub display: String,
    /// File Name: the member part's document name in the design file.
    pub file: String,
}

/// The names members `occs` have now (Frame Member Naming's rows).
pub fn member_names(project: &Project, a: &AssemblyDocument, occs: &[u32]) -> Vec<MemberName> {
    occs.iter()
        .filter_map(|&o| {
            let x = a.find(o)?;
            let file = project.document(x.component.document)?.name.clone();
            Some(MemberName {
                occurrence: o,
                display: x.name.clone(),
                file,
            })
        })
        .collect()
}

/// File Naming Defaults › Frame Generator: the default pattern of a new
/// member's file name (Stock Number and a unique number).
pub const DEFAULT_NAMING: &str = "{Stock Number}-{#}";

/// The attributes a naming pattern can hold (the Options' **+** list).
pub const NAMING_TOKENS: &[&str] = &[
    "{Stock Number}",
    "{Standard}",
    "{Family}",
    "{Size}",
    "{Length}",
    "{#}",
];

/// The names `pattern` gives members `occs` of `a` (Frame Member Naming's
/// prefill): each attribute replaced by the member's, `{#}` by the smallest
/// number that makes the file name unique in the project (a name without
/// `{#}` that is taken gets `-<n>`). Display Name = File Name + `:1`.
pub fn names_from_pattern(
    project: &Project,
    a: &AssemblyDocument,
    occs: &[u32],
    pattern: &str,
) -> Vec<MemberName> {
    let pattern = if pattern.trim().is_empty() {
        DEFAULT_NAMING
    } else {
        pattern
    };
    let mut taken: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for &occ in occs {
        let Some(m) = a
            .frames
            .iter()
            .flat_map(|f| &f.members)
            .find(|m| m.occurrence == occ)
        else {
            continue;
        };
        let Some(doc) = a.find(occ).map(|o| o.component.document) else {
            continue;
        };
        let s = m.section();
        let stock = s.as_ref().map(Section::stock_number).unwrap_or_default();
        let length = m
            .built
            .as_ref()
            .map(|b| format!("{:.0}", b.length))
            .unwrap_or_default();
        let base = pattern
            .replace("{Stock Number}", &stock)
            .replace("{Standard}", &m.section.standard)
            .replace("{Family}", &m.family)
            .replace("{Size}", &m.size)
            .replace("{Length}", &length);
        let free = |n: &str, taken: &[String]| {
            !n.trim().is_empty()
                && !project.name_taken(n, Some(doc))
                && !taken.iter().any(|t| t.eq_ignore_ascii_case(n))
        };
        let file = if base.contains("{#}") {
            (1..)
                .map(|k| base.replace("{#}", &k.to_string()))
                .find(|n| free(n, &taken))
                .unwrap_or_default()
        } else if free(&base, &taken) {
            base
        } else {
            (2..)
                .map(|k| format!("{base}-{k}"))
                .find(|n| free(n, &taken))
                .unwrap_or_default()
        };
        taken.push(file.clone());
        out.push(MemberName {
            occurrence: occ,
            display: format!("{file}:1"),
            file,
        });
    }
    out
}

/// Frame Member Naming's OK: every row's occurrence and part take its names.
/// An empty name, a file name used twice or by another document is refused
/// and nothing changes (rule 5).
pub fn rename_members(
    project: &mut Project,
    asm: DocumentId,
    rows: &[MemberName],
) -> Result<(), String> {
    for (k, r) in rows.iter().enumerate() {
        if r.display.trim().is_empty() || r.file.trim().is_empty() {
            return Err("a name cannot be empty".into());
        }
        if rows[..k].iter().any(|x| {
            x.file.trim().eq_ignore_ascii_case(r.file.trim()) && x.occurrence != r.occurrence
        }) {
            return Err(format!("\"{}\" is given twice", r.file.trim()));
        }
    }
    let before = project.clone();
    let result = (|| {
        let a = asm_of(project, asm)?;
        let docs: Vec<DocumentId> = rows
            .iter()
            .map(|r| {
                a.find(r.occurrence)
                    .map(|o| o.component.document)
                    .ok_or_else(|| "that member no longer exists".to_owned())
            })
            .collect::<Result<_, _>>()?;
        // First out of the way, so two members can swap names.
        for (k, d) in docs.iter().enumerate() {
            let tmp = format!("~frame-member-naming~{k}");
            project
                .rename_document(*d, &tmp)
                .map_err(|e| e.to_string())?;
        }
        for (r, d) in rows.iter().zip(&docs) {
            project
                .rename_document(*d, r.file.trim())
                .map_err(|e| e.to_string())?;
        }
        let a = asm_mut(project, asm)?;
        for r in rows {
            if let Some(o) = a.find_mut(r.occurrence) {
                o.name = r.display.trim().to_owned();
            }
        }
        Ok(())
    })();
    if result.is_err() {
        *project = before;
    }
    result
}

/// Which end of member `member` a point of its part (Z) lies on: the high
/// one or not; `None` when it is on neither.
pub fn member_end_at(a: &AssemblyDocument, member: u32, z: Real) -> Option<bool> {
    let m = a
        .frames
        .iter()
        .flat_map(|f| &f.members)
        .find(|m| m.occurrence == member)?;
    let (z0, z1) = m.built.as_ref()?.z_range();
    if (z - z1).abs() < 1e-3 {
        Some(true)
    } else if (z - z0).abs() < 1e-3 {
        Some(false)
    } else {
        None
    }
}

/// Deletes end caps by their occurrences (with their parts).
pub fn delete_caps(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    caps: &[u32],
) -> Result<Solved, String> {
    let before = project.clone();
    let result = (|| {
        let a = asm_mut(project, asm)?;
        let mut docs = Vec::new();
        for &c in caps {
            for f in &mut a.frames {
                f.caps.retain(|x| x.occurrence != c);
            }
            if let Some(o) = a.remove(c)
                && o.component.library.is_none()
            {
                docs.push(o.component.document);
            }
        }
        for d in docs {
            let _ = project.remove_document(d);
        }
        solve(project, linked, asm)
    })();
    if result.is_err() {
        *project = before;
    }
    result
}

/// A member's run in assembly coordinates (start, end; Flip applied).
pub fn member_run(
    project: &Project,
    linked: &Linked,
    asm: &AssemblyDocument,
    occ: u32,
) -> Option<(Point3, Point3)> {
    let m = asm
        .frames
        .iter()
        .flat_map(|f| &f.members)
        .find(|m| m.occurrence == occ)?;
    let (p, q) = skeleton_line(project, linked, asm, &m.skeleton).ok()?;
    Some(if m.orientation.flip { (q, p) } else { (p, q) })
}

/// Remove End Treatments: the members return to their inserted state.
pub fn remove_treatments(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    members: &[u32],
) -> Result<Solved, String> {
    let a = asm_mut(project, asm)?;
    let mut n = 0;
    for f in &mut a.frames {
        let before = f.treatments.len();
        f.treatments
            .retain(|t| !t.members().iter().any(|m| members.contains(m)));
        n += before - f.treatments.len();
    }
    if n == 0 {
        return Err("the members have no end treatments".into());
    }
    solve(project, linked, asm)
}

/// The pairs of `members` (occurrence ids) whose axes meet near their ends,
/// for Miter with more than two members picked.
pub fn meeting_pairs(
    project: &Project,
    linked: &Linked,
    asm: &AssemblyDocument,
    members: &[u32],
) -> Vec<(u32, u32)> {
    let lines: Vec<(u32, (Point3, Point3), Real)> = members
        .iter()
        .filter_map(|&o| {
            let m = asm
                .frames
                .iter()
                .flat_map(|f| &f.members)
                .find(|m| m.occurrence == o)?;
            let l = skeleton_line(project, linked, asm, &m.skeleton).ok()?;
            let (w, h) = m.section()?.extent();
            Some((o, l, w + h))
        })
        .collect();
    let mut out = Vec::new();
    for i in 0..lines.len() {
        for j in i + 1..lines.len() {
            let reach = 2.0 * (lines[i].2 + lines[j].2);
            if joint(lines[i].1, lines[j].1, reach).is_some() {
                out.push((lines[i].0, lines[j].0));
            }
        }
    }
    out
}

/// The corners of `asm`'s frames (Miter and Corner Joint treatments): the
/// frame, the treatment's index, where the two members' skeleton lines meet
/// and the members (the glyphs in the view, spec 6.1 §3.10).
pub fn corner_points(
    project: &Project,
    linked: &Linked,
    asm: &AssemblyDocument,
) -> Vec<(u32, usize, Point3, [u32; 2])> {
    let mut out = Vec::new();
    for f in &asm.frames {
        let line = |o: u32| {
            let m = f.members.iter().find(|m| m.occurrence == o)?;
            skeleton_line(project, linked, asm, &m.skeleton).ok()
        };
        for (i, t) in f.treatments.iter().enumerate() {
            let (Treatment::Miter { a, b, .. } | Treatment::CornerJoint { a, b, .. }) = t else {
                continue;
            };
            if let (Some(la), Some(lb)) = (line(*a), line(*b))
                && let Some((c, _, _)) = joint(la, lb, Real::MAX)
            {
                out.push((f.id, i, c, [*a, *b]));
            }
        }
    }
    out
}

/// The middle of the lines' middles (the centre Position works from).
fn centre_of(lines: &[(Point3, Point3)]) -> Point3 {
    let n = lines.len().max(1) as Real;
    let sum = lines.iter().fold(Vector3::zeros(), |s, (p, q)| {
        s + (p.coords + q.coords) / 2.0
    });
    Point3::from(sum / n)
}

/// The corner treatments Insert Frame / Edit Frame give `members` (spec 6.1
/// §3.10, §3.12): at a corner of two, `corners.kind`; at a corner of three
/// or more, the most vertical member (else the longest) runs through and
/// every other one stops at it; with `tees`, a member ending on the middle
/// of another is notched by it.
pub fn corner_treatments(
    project: &Project,
    linked: &Linked,
    asm: &AssemblyDocument,
    members: &[u32],
    corners: &Corners,
) -> Vec<Treatment> {
    let lines: Vec<(u32, (Point3, Point3), Real)> = members
        .iter()
        .filter_map(|&o| {
            let m = asm
                .frames
                .iter()
                .flat_map(|f| &f.members)
                .find(|m| m.occurrence == o)?;
            let l = skeleton_line(project, linked, asm, &m.skeleton).ok()?;
            let (w, h) = m.section()?.extent();
            Some((o, l, w + h))
        })
        .collect();
    let mut out = Vec::new();
    if corners.kind != CornerType::None {
        // Corners: the meeting point and the members (index into `lines`).
        let mut vertices: Vec<(Point3, Vec<usize>)> = Vec::new();
        for i in 0..lines.len() {
            for j in i + 1..lines.len() {
                // Near the ends of both (within a section's size): a line
                // ending on the middle of another is a T joint, not a corner.
                let reach = lines[i].2.max(lines[j].2);
                if let Some((c, _, _)) = joint(lines[i].1, lines[j].1, reach) {
                    match vertices.iter_mut().find(|(p, _)| (p - c).norm() < 1e-3) {
                        Some((_, v)) => {
                            for k in [i, j] {
                                if !v.contains(&k) {
                                    v.push(k);
                                }
                            }
                        }
                        None => vertices.push((c, vec![i, j])),
                    }
                }
            }
        }
        let len = |k: usize| (lines[k].1.1 - lines[k].1.0).norm();
        for (_, v) in &vertices {
            if let [i, j] = v[..] {
                if let Some(t) = corners.treatment(lines[i].0, lines[j].0, [len(i), len(j)]) {
                    out.push(t);
                }
                continue;
            }
            let upright = |k: usize| {
                let (p, q) = lines[k].1;
                ((q - p).normalize().z.abs(), len(k))
            };
            let Some(&through) = v.iter().max_by(|&&x, &&y| {
                let (a, b) = (upright(x), upright(y));
                (a.0 > 0.9)
                    .cmp(&(b.0 > 0.9))
                    .then(a.1.total_cmp(&b.1))
                    .then(y.cmp(&x))
            }) else {
                continue;
            };
            for &k in v {
                // Only members that meet the column at an angle (a piece
                // of an edge split in line with it does not).
                let meets = joint(
                    lines[through].1,
                    lines[k].1,
                    lines[through].2.max(lines[k].2),
                );
                if k != through && meets.is_some() {
                    out.push(Treatment::CornerJoint {
                        a: lines[through].0,
                        b: lines[k].0,
                        offsets: [0.0, corners.gap],
                    });
                }
            }
        }
    }
    if corners.tees != TeeJoint::None {
        for (i, (oi, (p, q), _)) in lines.iter().enumerate() {
            for (j, (oj, (a, b), _)) in lines.iter().enumerate() {
                if i == j {
                    continue;
                }
                let d = b - a;
                let l2 = d.norm_squared();
                let on_middle = |e: &Point3| {
                    let t = (e - a).dot(&d) / l2;
                    let margin = 1e-3 / l2.sqrt();
                    t > margin && t < 1.0 - margin && (a + d * t - e).norm() < 1e-3
                };
                if on_middle(p) || on_middle(q) {
                    out.push(match corners.tees {
                        TeeJoint::Notch => Treatment::Notch {
                            notched: *oi,
                            by: *oj,
                            offset: corners.gap,
                        },
                        _ => Treatment::CornerJoint {
                            a: *oj,
                            b: *oi,
                            offsets: [0.0, corners.gap],
                        },
                    });
                }
            }
        }
    }
    out
}

/// The corners new members `new` make with each other and with the rest
/// of frame `frame` (the frame's own treatments stay), then a solve; on an
/// error nothing changes.
pub fn add_corners(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    frame: u32,
    new: &[u32],
    corners: &Corners,
) -> Result<Solved, String> {
    let before = project.clone();
    let result = (|| {
        let a = asm_of(project, asm)?;
        let f = a
            .frames
            .iter()
            .find(|f| f.id == frame)
            .ok_or("no such frame")?;
        let all: Vec<u32> = f.members.iter().map(|m| m.occurrence).collect();
        let ts: Vec<Treatment> = corner_treatments(project, linked, a, &all, corners)
            .into_iter()
            .filter(|t| t.members().iter().any(|m| new.contains(m)))
            .collect();
        let a = asm_mut(project, asm)?;
        let f = a
            .frames
            .iter_mut()
            .find(|f| f.id == frame)
            .ok_or("no such frame")?;
        f.treatments.extend(ts);
        let s = solve(project, linked, asm)?;
        if let Some((_, why)) = s.sick.iter().find(|(o, _)| new.contains(o)) {
            return Err(format!("corners: {why}"));
        }
        Ok(s)
    })();
    if result.is_err() {
        *project = before;
    }
    result
}

/// Edit Frame (spec 6.1 §3.12): every member of frame `frame` gets
/// `layout` (when given), and with `corners` the frame's Miter / Corner
/// Joint / Notch treatments are replaced by new ones; then the frame is
/// solved. Failure changes nothing.
pub fn edit_frame(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    frame: u32,
    layout: Option<Layout>,
    corners: Option<Corners>,
) -> Result<Solved, String> {
    let before = project.clone();
    let result = (|| {
        let a = asm_of(project, asm)?;
        let f = a
            .frames
            .iter()
            .find(|f| f.id == frame)
            .ok_or("no such frame")?;
        let occs: Vec<u32> = f.members.iter().map(|m| m.occurrence).collect();
        let mut orient = Vec::new();
        if let Some(l) = layout {
            // Curved members keep their orientation.
            let members: Vec<(&FrameMember, (Point3, Point3))> = f
                .members
                .iter()
                .filter_map(|m| Some((m, skeleton_line(project, linked, a, &m.skeleton).ok()?)))
                .collect();
            let lines: Vec<(Point3, Point3)> = members.iter().map(|x| x.1).collect();
            let centre = centre_of(&lines);
            for &(m, (p, q)) in &members {
                let base = Orientation {
                    anchor: Anchor::Center,
                    ..m.orientation
                };
                let o = if l == Layout::default() {
                    base
                } else {
                    l.orient(&base, p, q, centre)
                };
                orient.push((m.occurrence, o));
            }
        }
        let fresh = corners
            .as_ref()
            .map(|c| corner_treatments(project, linked, a, &occs, c));
        let a = asm_mut(project, asm)?;
        let f = a
            .frames
            .iter_mut()
            .find(|f| f.id == frame)
            .ok_or("no such frame")?;
        for (occ, o) in orient {
            if let Some(m) = f.members.iter_mut().find(|m| m.occurrence == occ) {
                m.orientation = o;
            }
        }
        if let Some(ts) = fresh {
            f.treatments.retain(|t| {
                !matches!(
                    t,
                    Treatment::Miter { .. }
                        | Treatment::CornerJoint { .. }
                        | Treatment::Notch { .. }
                )
            });
            f.treatments.extend(ts);
        }
        let s = solve(project, linked, asm)?;
        if let Some((_, why)) = s.sick.iter().find(|(o, _)| occs.contains(o)) {
            return Err(why.clone());
        }
        Ok(s)
    })();
    if result.is_err() {
        *project = before;
    }
    result
}

/// The straight edges of `part`'s body (Insert Frame's Body Edges), or of
/// its face `face` only (Face Edges), as edge references (part
/// coordinates): two face names and the edge's middle.
pub fn straight_edges(part: &PartDocument, face: Option<&str>) -> Vec<crate::part::EdgeRef> {
    straight_edges_in(part, face, None)
}

/// The body of `part` (one of its solids) whose face named `face` lies
/// nearest `at` (part coordinates): the body a click on that face hit, in a
/// part of several bodies (Insert Frame's Body Edges takes one body only,
/// rule 19).
pub fn body_at(part: &PartDocument, face: &str, at: Point3) -> Option<crate::kernel::SolidId> {
    let f = crate::part::appearance::nearest_face(&part.body, face, &[at.x, at.y, at.z])?;
    part.body
        .solids()
        .find(|(_, s)| s.shells.iter().any(|sh| sh.faces.contains(&f)))
        .map(|(id, _)| id)
}

/// [`straight_edges`] of one body only (`solid`; `None`: every body).
pub fn straight_edges_in(
    part: &PartDocument,
    face: Option<&str>,
    solid: Option<crate::kernel::SolidId>,
) -> Vec<crate::part::EdgeRef> {
    straight_edge_lines(part, face, solid)
        .into_iter()
        .map(|(r, ..)| r)
        .collect()
}

/// [`straight_edges_in`] with each edge's ends (part coordinates), for
/// drawing what a click would take.
pub fn straight_edge_lines(
    part: &PartDocument,
    face: Option<&str>,
    solid: Option<crate::kernel::SolidId>,
) -> Vec<(crate::part::EdgeRef, Point3, Point3)> {
    let body = &part.body;
    let own: Option<std::collections::HashSet<crate::kernel::FaceId>> = solid.and_then(|id| {
        body.solids().find(|(s, _)| *s == id).map(|(_, s)| {
            s.shells
                .iter()
                .flat_map(|sh| sh.faces.iter().copied())
                .collect()
        })
    });
    let opts = crate::kernel::TessellationOptions::default();
    let mut out: Vec<(crate::part::EdgeRef, Point3, Point3)> = Vec::new();
    for (id, _) in body.edges() {
        let faces = body.edge_faces(id);
        let [f0, f1] = faces[..] else {
            continue;
        };
        if own
            .as_ref()
            .is_some_and(|set| !set.contains(&f0) || !set.contains(&f1))
        {
            continue;
        }
        let name = |f| body.face(f).and_then(|f| f.name.clone());
        let (Some(n0), Some(n1)) = (name(f0), name(f1)) else {
            continue;
        };
        if face.is_some_and(|w| w != n0 && w != n1) {
            continue;
        }
        let Ok(pts) = body.sample_edge(id, &opts) else {
            continue;
        };
        let (Some(s), Some(e)) = (pts.first(), pts.last()) else {
            continue;
        };
        let d = e - s;
        if d.norm() < 1e-6
            || pts
                .iter()
                .any(|p| (p - s).cross(&d).norm() > 1e-6 * d.norm_squared())
        {
            continue;
        }
        let m = s + d / 2.0;
        // A split edge or a seam listed twice: keep one.
        if out
            .iter()
            .any(|(r, ..)| (Point3::new(r.hint[0], r.hint[1], r.hint[2]) - m).norm() < 1e-6)
        {
            continue;
        }
        out.push((
            crate::part::EdgeRef {
                faces: [n0, n1],
                hint: [m.x, m.y, m.z],
            },
            *s,
            *e,
        ));
    }
    out
}

/// How near two lines must be to lie on one another (Remove Overlapping
/// Members, spec 6.1 §3.1a): 1 µm across, 1e-6 rad between their directions,
/// and a shared length of more than 0.01 mm (members meeting end to end do
/// not overlap).
const OVERLAP: crate::kernel::Tolerance = crate::kernel::Tolerance {
    linear: 1e-3,
    angular: 1e-6,
};

/// Whether straight lines `a` and `b` (start, end) lie on one another:
/// parallel, on one line, and sharing a piece of it.
pub fn lines_overlap(a: (Point3, Point3), b: (Point3, Point3)) -> bool {
    let da = a.1 - a.0;
    let la = da.norm();
    let lb = (b.1 - b.0).norm();
    if la < OVERLAP.linear || lb < OVERLAP.linear {
        return false;
    }
    let u = da / la;
    // Both ends of b on a's line (which also makes them parallel).
    let off = |p: Point3| (p - a.0 - u * (p - a.0).dot(&u)).norm();
    let tol = OVERLAP.linear + OVERLAP.angular * la.max(lb);
    if off(b.0) > tol || off(b.1) > tol {
        return false;
    }
    let (t0, t1) = ((b.0 - a.0).dot(&u), (b.1 - a.0).dot(&u));
    let (lo, hi) = (t0.min(t1), t0.max(t1));
    hi.min(la) - lo.max(0.0) > 10.0 * OVERLAP.linear
}

/// Remove Overlapping Members: which of `lines` go so that no two left lie
/// on one another. Of two overlapping lines the shorter goes, of two as long
/// the later one. The first `fixed` lines are members already placed: two of
/// them overlapping each other are left as they are (the browser warns), but
/// one goes when a longer new line covers it. `None`: no straight line (it
/// never overlaps). Returns the indices, ascending.
pub fn overlap_drops(lines: &[Option<(Point3, Point3)>], fixed: usize) -> Vec<usize> {
    // Longest first; lengths within the tolerance count as equal (then the
    // earlier one first).
    let q = |i: usize| {
        lines[i]
            .as_ref()
            .map_or(0, |l| ((l.1 - l.0).norm() / OVERLAP.linear).round() as i64)
    };
    let mut order: Vec<usize> = (0..lines.len()).filter(|&i| lines[i].is_some()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(q(i)), i));
    let mut kept: Vec<usize> = Vec::new();
    let mut out = Vec::new();
    for i in order {
        let Some(l) = lines[i] else {
            continue;
        };
        let covered = kept
            .iter()
            .any(|&k| !(k < fixed && i < fixed) && lines[k].is_some_and(|m| lines_overlap(m, l)));
        if covered {
            out.push(i);
        } else {
            kept.push(i);
        }
    }
    out.sort_unstable();
    out
}

/// The pairs of straight members of assembly `asm` that lie on one another
/// (occurrence ids, the earlier member first): the browser's warning. Members
/// whose line cannot be found are left out (they are sick already).
pub fn overlapping_members(
    project: &Project,
    linked: &Linked,
    asm: &AssemblyDocument,
) -> Vec<(u32, u32)> {
    let lines: Vec<(u32, (Point3, Point3))> = asm
        .frames
        .iter()
        .flat_map(|f| &f.members)
        .filter_map(|m| {
            skeleton_line(project, linked, asm, &m.skeleton)
                .ok()
                .map(|l| (m.occurrence, l))
        })
        .collect();
    overlapping_pairs(&lines)
}

/// [`overlapping_members`] of lines already found (in member order): sorted
/// by their x extent first, so a frame of many members compares only
/// neighbours in full.
pub fn overlapping_pairs(lines: &[(u32, (Point3, Point3))]) -> Vec<(u32, u32)> {
    let pad = 10.0 * OVERLAP.linear;
    let mut boxes: Vec<(Real, Real, usize)> = lines
        .iter()
        .enumerate()
        .map(|(i, (_, (a, b)))| (a.x.min(b.x) - pad, a.x.max(b.x) + pad, i))
        .collect();
    boxes.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    for (k, &(_, hi, i)) in boxes.iter().enumerate() {
        for &(lo2, _, j) in &boxes[k + 1..] {
            if lo2 > hi {
                break;
            }
            let ((oi, li), (oj, lj)) = (lines[i], lines[j]);
            if lines_overlap(li, lj) {
                out.push(if i < j { (oi, oj) } else { (oj, oi) });
            }
        }
    }
    out.sort_unstable();
    out
}

/// The member's length now (its extrusion's distance).
pub fn length_of(part: &PartDocument) -> Option<Real> {
    part.features.iter().find_map(|f| match f {
        Feature::Extrude(e) => Some(e.distance),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::EdgeRef;

    /// A 400 × 300 × 200 box as the skeleton, grounded at the origin.
    fn project() -> (Project, DocumentId, DocumentId) {
        let mut p = Project::new("T");
        let sk = PartDocument::with_box("Box1", [400.0, 300.0, 200.0]).unwrap();
        let sk = p.add_document("Skeleton", DocumentData::Part(sk));
        let mut a = AssemblyDocument::default();
        a.place(
            DocRef::local(sk),
            "Skeleton:1",
            Transform3::identity(),
            true,
        );
        let aid = p.add_document("Frame", DocumentData::Assembly(a));
        (p, aid, sk)
    }

    /// The box edge between two faces, by a point on it.
    fn edge(f0: &str, f1: &str, hint: [Real; 3]) -> Skeleton {
        Skeleton::Edge(GeomRef::Edge {
            occurrence: 1,
            sub: Vec::new(),
            edge: EdgeRef {
                faces: [format!("Box1:{f0}"), format!("Box1:{f1}")],
                hint,
            },
        })
    }

    fn opts() -> InsertOptions {
        InsertOptions {
            family: "Square hollow section (EN 10219)".into(),
            size: "40x40x3".into(),
            material: Some("Steel S235".into()),
            orientation: Orientation::default(),
            corners: Corners::default(),
            layout: Layout::default(),
            appearance: None,
            appearance_to_part: false,
        }
    }

    fn asm(p: &Project, id: DocumentId) -> &AssemblyDocument {
        asm_of(p, id).unwrap()
    }

    fn part(p: &Project, a: &AssemblyDocument, occ: u32) -> PartDocument {
        let o = a.find(occ).unwrap();
        match p.document(o.component.document).map(|d| &d.data) {
            Some(DocumentData::Part(x)) => x.clone(),
            _ => panic!("no part"),
        }
    }

    #[test]
    fn a_member_follows_its_edge_with_the_section_square_to_it() {
        let (mut p, aid, _) = project();
        let e = find_edge(&p, aid, Point3::new(200.0, 0.0, 0.0));
        let (fid, occs) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &[e],
            &opts(),
            Library::builtin(),
            None,
        )
        .unwrap();
        let a = asm(&p, aid);
        assert_eq!(a.frames.len(), 1);
        assert_eq!(a.frames[0].id, fid);
        assert_eq!(a.frames[0].name, "Frame1");
        let o = a.find(occs[0]).unwrap();
        assert!(o.grounded);
        let m = part(&p, a, occs[0]);
        assert!((length_of(&m).unwrap() - 400.0).abs() < 1e-9);
        // Part Z along the edge (±X), the section centred on it.
        let z = o.transform.transform_vector(&Vector3::z());
        assert!(z.cross(&Vector3::x()).norm() < 1e-9, "{z}");
        let start = o.transform.transform_point(&Point3::origin());
        assert!(start.y.abs() < 1e-9 && start.z.abs() < 1e-9, "{start}");
        // The section's vertical axis is the assembly's Z.
        let y = o.transform.transform_vector(&Vector3::y());
        assert!((y - Vector3::z()).norm() < 1e-9, "{y}");
        let doc = p.document(o.component.document).unwrap();
        assert_eq!(doc.name, "SHS 40x40x3-1");
        assert_eq!(
            doc.properties
                .standard
                .get("stock_number")
                .map(String::as_str),
            Some("SHS 40x40x3")
        );
    }

    /// Offsets, anchor, rotation and mirror move the section as Inventor's
    /// Orientation group does.
    #[test]
    fn orientation_places_the_section() {
        let s = Library::builtin()
            .find("Equal angle (EN 10056-1)", "L 50x50x5")
            .unwrap()
            .clone();
        let (a, b) = (Point3::origin(), Point3::new(1000.0, 0.0, 0.0));
        // Anchor bottom left on the line: the heel lies on it.
        let o = Orientation {
            anchor: Anchor::BottomLeft,
            ..Default::default()
        };
        let t = placement(&s, a, b, &o).unwrap();
        let heel = t.transform_point(&Point3::new(-25.0, -25.0, 0.0));
        assert!((heel - a).norm() < 1e-9, "{heel}");
        // Offsets along the section's axes.
        let o = Orientation {
            h_offset: 10.0,
            v_offset: 20.0,
            ..Default::default()
        };
        let t = placement(&s, a, b, &o).unwrap();
        let c = t.transform_point(&Point3::origin());
        let (u, v) = (
            t.transform_vector(&Vector3::x()),
            t.transform_vector(&Vector3::y()),
        );
        assert!((c - (a + u * 10.0 + v * 20.0)).norm() < 1e-9);
        // 90° about the line: the section's up turns.
        let o = Orientation {
            angle: std::f64::consts::FRAC_PI_2,
            ..Default::default()
        };
        let t = placement(&s, a, b, &o).unwrap();
        let v2 = t.transform_vector(&Vector3::y());
        assert!(v2.dot(&Vector3::z()).abs() < 1e-9);
        // Mirror: same line covered, the legs on the other side.
        let o = Orientation {
            anchor: Anchor::BottomLeft,
            mirror: true,
            ..Default::default()
        };
        let t = placement(&s, a, b, &o).unwrap();
        let heel = t.transform_point(&Point3::new(-25.0, -25.0, 0.0));
        let far = t.transform_point(&Point3::new(-25.0, -25.0, 1000.0));
        assert!(
            (heel - b).norm() < 1e-9 && (far - a).norm() < 1e-9,
            "{heel} {far}"
        );
        // A right-handed frame (a rigid placement).
        assert!((t.rotation.to_rotation_matrix().matrix().determinant() - 1.0).abs() < 1e-9);
    }

    /// A longer skeleton: Solve lengthens the member and keeps its frame.
    #[test]
    fn a_changed_skeleton_changes_the_member() {
        let (mut p, aid, sk) = project();
        let e = find_edge(&p, aid, Point3::new(200.0, 0.0, 0.0));
        let (_, occs) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &[e],
            &opts(),
            Library::builtin(),
            None,
        )
        .unwrap();
        if let Some(DocumentData::Part(b)) = p.document_mut(sk).map(|d| &mut d.data) {
            b.box_feature_mut().unwrap().size[0] = 600.0;
            b.regenerate().unwrap();
        }
        let s = solve(&mut p, &Linked::new(), aid).unwrap();
        assert!(s.sick.is_empty(), "{:?}", s.sick);
        assert_eq!(s.moved, 1);
        let a = asm(&p, aid);
        let m = part(&p, a, occs[0]);
        assert!((length_of(&m).unwrap() - 600.0).abs() < 1e-9);
        let v: Real = m
            .exact_bodies
            .0
            .iter()
            .filter_map(|b| b.volume_at(1e-3))
            .sum();
        assert!((v - 421.0 * 600.0).abs() < 0.01 * 421.0 * 600.0, "{v}");
        // A second solve has nothing to do.
        assert_eq!(solve(&mut p, &Linked::new(), aid).unwrap().moved, 0);
    }

    /// An edge that is gone leaves the member as it was and reports it.
    #[test]
    fn a_member_without_its_edge_keeps_its_shape() {
        let (mut p, aid, _) = project();
        let e = find_edge(&p, aid, Point3::new(200.0, 0.0, 0.0));
        let (_, occs) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &[e],
            &opts(),
            Library::builtin(),
            None,
        )
        .unwrap();
        let before = asm(&p, aid).find(occs[0]).unwrap().transform;
        if let Ok(a) = asm_mut(&mut p, aid)
            && let Skeleton::Edge(GeomRef::Edge { edge, .. }) = &mut a.frames[0].members[0].skeleton
        {
            edge.faces[0] = "Box1:Gone".into();
        }
        let s = solve(&mut p, &Linked::new(), aid).unwrap();
        assert_eq!(s.sick.len(), 1);
        assert_eq!(asm(&p, aid).find(occs[0]).unwrap().transform, before);
    }

    /// Failure leaves the project unchanged (rule 5).
    #[test]
    fn a_failed_insert_changes_nothing() {
        let (mut p, aid, _) = project();
        let e = find_edge(&p, aid, Point3::new(200.0, 0.0, 0.0));
        let bad = edge("Gone", "Front", [0.0, 0.0, 0.0]);
        let n = p.documents().len();
        assert!(
            insert(
                &mut p,
                &Linked::new(),
                aid,
                &[e, bad],
                &opts(),
                Library::builtin(),
                None
            )
            .is_err()
        );
        assert_eq!(p.documents().len(), n);
        assert!(asm(&p, aid).frames.is_empty());
        let mut o = opts();
        o.size = "1x1x1".into();
        let e = find_edge(&p, aid, Point3::new(200.0, 0.0, 0.0));
        assert!(
            insert(
                &mut p,
                &Linked::new(),
                aid,
                &[e],
                &o,
                Library::builtin(),
                None
            )
            .is_err()
        );
    }

    /// Every edge of the box: twelve members, the save round trip keeps
    /// the frame.
    #[test]
    fn twelve_edges_make_a_box_frame_that_survives_a_save() {
        let (mut p, aid, _) = project();
        let mids = [
            [200.0, 0.0, 0.0],
            [200.0, 300.0, 0.0],
            [200.0, 0.0, 200.0],
            [200.0, 300.0, 200.0],
            [0.0, 150.0, 0.0],
            [400.0, 150.0, 0.0],
            [0.0, 150.0, 200.0],
            [400.0, 150.0, 200.0],
            [0.0, 0.0, 100.0],
            [400.0, 0.0, 100.0],
            [0.0, 300.0, 100.0],
            [400.0, 300.0, 100.0],
        ];
        let edges: Vec<Skeleton> = mids
            .iter()
            .map(|m| find_edge(&p, aid, Point3::new(m[0], m[1], m[2])))
            .collect();
        let (_, occs) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &edges,
            &opts(),
            Library::builtin(),
            None,
        )
        .unwrap();
        assert_eq!(occs.len(), 12);
        let a = asm(&p, aid);
        let total: Real = occs
            .iter()
            .map(|&o| length_of(&part(&p, a, o)).unwrap())
            .sum();
        assert!(
            (total - 4.0 * (400.0 + 300.0 + 200.0)).abs() < 1e-6,
            "{total}"
        );
        let json = serde_json::to_string(a).unwrap();
        let back: AssemblyDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(back.frames, a.frames);
    }

    /// A line of a skeleton part's sketch takes a member; a longer line
    /// lengthens the member on the next solve.
    #[test]
    fn a_sketch_line_is_a_skeleton() {
        use crate::part::{OriginPlane, SketchOptions, SketchPlacement};
        use crate::sketch::Point2;
        let mut sk = PartDocument::default();
        let o = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let i = sk
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
            .unwrap();
        let line = sk.sketches[i]
            .sketch
            .add_line(Point2::new(0.0, 0.0), Point2::new(0.0, 750.0));
        let sid = sk.sketches[i].id.clone();
        let mut p = Project::new("T");
        let skd = p.add_document("Skeleton", DocumentData::Part(sk));
        let mut a = AssemblyDocument::default();
        let t = Transform3::new(Vector3::new(100.0, 0.0, 0.0), Vector3::zeros());
        a.place(DocRef::local(skd), "Skeleton:1", t, true);
        let aid = p.add_document("Frame", DocumentData::Assembly(a));
        let s = Skeleton::SketchLine {
            occurrence: 1,
            sub: Vec::new(),
            sketch: sid,
            line,
        };
        let (_, occs) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &[s],
            &opts(),
            Library::builtin(),
            None,
        )
        .unwrap();
        let o = asm(&p, aid).find(occs[0]).unwrap();
        let start = o.transform.transform_point(&Point3::origin());
        assert!(
            (start - Point3::new(100.0, 0.0, 0.0)).norm() < 1e-9,
            "{start}"
        );
        let z = o.transform.transform_vector(&Vector3::z());
        assert!((z - Vector3::y()).norm() < 1e-9, "{z}");
        assert!((length_of(&part(&p, asm(&p, aid), occs[0])).unwrap() - 750.0).abs() < 1e-9);
        // The line's end moves: the member follows.
        if let Some(DocumentData::Part(x)) = p.document_mut(skd).map(|d| &mut d.data) {
            let sk = &mut x.sketches[0].sketch;
            let (_, end) = sk.line_points(line).unwrap();
            sk.move_point(end, Point2::new(0.0, 900.0)).unwrap();
        }
        assert_eq!(solve(&mut p, &Linked::new(), aid).unwrap().moved, 1);
        assert!((length_of(&part(&p, asm(&p, aid), occs[0])).unwrap() - 900.0).abs() < 1e-9);
    }

    /// A skeleton part whose XY sketch `draw` fills (it returns the curves),
    /// and an assembly showing it: (project, assembly, skeleton part, curves).
    fn curve_skeleton(
        draw: impl FnOnce(&mut crate::sketch::Sketch) -> Vec<crate::sketch::EntityId>,
    ) -> (Project, DocumentId, DocumentId, Vec<Skeleton>) {
        use crate::part::{OriginPlane, SketchOptions, SketchPlacement};
        let mut sk = PartDocument::default();
        let o = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let i = sk
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
            .unwrap();
        let curves = draw(&mut sk.sketches[i].sketch);
        let sketch = sk.sketches[i].id.clone();
        let mut p = Project::new("T");
        let skd = p.add_document("Skeleton", DocumentData::Part(sk));
        let mut a = AssemblyDocument::default();
        a.place(
            DocRef::local(skd),
            "Skeleton:1",
            Transform3::identity(),
            true,
        );
        let aid = p.add_document("Frame", DocumentData::Assembly(a));
        let list = curves
            .into_iter()
            .map(|line| Skeleton::SketchLine {
                occurrence: 1,
                sub: Vec::new(),
                sketch: sketch.clone(),
                line,
            })
            .collect();
        (p, aid, skd, list)
    }

    fn insert_one(p: &mut Project, aid: DocumentId, s: Skeleton) -> Result<u32, String> {
        insert(
            p,
            &Linked::new(),
            aid,
            &[s],
            &opts(),
            Library::builtin(),
            None,
        )
        .map(|r| r.1[0])
    }

    fn section_area() -> Real {
        let o = opts();
        Library::builtin()
            .find(&o.family, &o.size)
            .unwrap()
            .properties()
            .area
    }

    fn quarter_arc(sk: &mut crate::sketch::Sketch, r: Real) -> Vec<crate::sketch::EntityId> {
        use crate::sketch::Point2;
        let c = sk.add_point(Point2::new(r, 0.0));
        let s = sk.add_point(Point2::new(0.0, 0.0));
        let e = sk.add_point(Point2::new(r, -r));
        vec![sk.add_arc(c, s, e).unwrap()]
    }

    /// An arc: the section swept round it (Pappus), `G_L` the arc's length;
    /// solving again rebuilds nothing; the arc's end moved, the member follows.
    #[test]
    fn a_member_along_an_arc() {
        let (mut p, aid, skd, s) = curve_skeleton(|sk| quarter_arc(sk, 300.0));
        let occ = insert_one(&mut p, aid, s[0].clone()).unwrap();
        let want = 300.0 * std::f64::consts::FRAC_PI_2;
        close_to(
            volume_of(&p, aid, occ),
            section_area() * want,
            1.0e-3 * section_area() * want,
        );
        let doc = component_of(&p, aid, occ).document;
        let g = p
            .document(doc)
            .unwrap()
            .properties
            .custom
            .iter()
            .find(|c| c.name == LENGTH_PROPERTY)
            .map(|c| c.value.clone());
        assert_eq!(g, Some(PropValue::Number(stored_length(want))));
        assert_eq!(solve(&mut p, &Linked::new(), aid).unwrap().moved, 0);
        if let Some(DocumentData::Part(x)) = p.document_mut(skd).map(|d| &mut d.data) {
            let sk = &mut x.sketches[0].sketch;
            let ids: Vec<_> = sk.points().map(|(id, _)| id).collect();
            sk.move_point(ids[0], crate::sketch::Point2::new(400.0, 0.0))
                .unwrap();
            sk.move_point(ids[2], crate::sketch::Point2::new(400.0, -400.0))
                .unwrap();
        }
        let done = solve(&mut p, &Linked::new(), aid).unwrap();
        assert!(done.sick.is_empty(), "{:?}", done.sick);
        let want = 400.0 * std::f64::consts::FRAC_PI_2;
        close_to(
            volume_of(&p, aid, occ),
            section_area() * want,
            1.0e-3 * section_area() * want,
        );
    }

    fn line_arc_line(sk: &mut crate::sketch::Sketch) -> Vec<crate::sketch::EntityId> {
        use crate::sketch::Point2;
        let a = sk.add_point(Point2::new(0.0, 200.0));
        let b = sk.add_point(Point2::new(0.0, 0.0));
        let c = sk.add_point(Point2::new(150.0, 0.0));
        let d = sk.add_point(Point2::new(150.0, -150.0));
        let e = sk.add_point(Point2::new(250.0, -150.0));
        vec![
            sk.add_line_between(a, b).unwrap(),
            sk.add_arc(c, b, d).unwrap(),
            sk.add_line_between(d, e).unwrap(),
        ]
    }

    /// Merge: a line, an arc and a line picked in any order are one bent
    /// member; A x L, whichever way it runs.
    #[test]
    fn merge_makes_one_member_of_tangent_curves() {
        let (mut p, aid, _, s) = curve_skeleton(line_arc_line);
        let chain = Skeleton::Chain(vec![s[2].clone(), s[0].clone(), s[1].clone()]);
        let occ = insert_one(&mut p, aid, chain).unwrap();
        let want = 300.0 + 150.0 * std::f64::consts::FRAC_PI_2;
        let tol = 2.0e-3 * section_area() * want;
        close_to(volume_of(&p, aid, occ), section_area() * want, tol);
        assert_eq!(asm(&p, aid).frames[0].members.len(), 1);
        // Flip runs it from the other end: the same solid.
        let f = &mut asm_mut(&mut p, aid).unwrap().frames[0];
        f.members[0].orientation.flip = true;
        let done = solve(&mut p, &Linked::new(), aid).unwrap();
        assert!(done.sick.is_empty(), "{:?}", done.sick);
        close_to(volume_of(&p, aid, occ), section_area() * want, tol);
    }

    /// What Merge refuses leaves the project as it was (rule 5).
    #[test]
    fn merge_refuses_a_corner_and_a_gap() {
        use crate::sketch::Point2;
        let (mut p, aid, _, s) = curve_skeleton(|sk| {
            let a = sk.add_point(Point2::new(0.0, 0.0));
            let b = sk.add_point(Point2::new(200.0, 0.0));
            let c = sk.add_point(Point2::new(200.0, 200.0));
            let d = sk.add_point(Point2::new(500.0, 500.0));
            let e = sk.add_point(Point2::new(600.0, 500.0));
            vec![
                sk.add_line_between(a, b).unwrap(),
                sk.add_line_between(b, c).unwrap(),
                sk.add_line_between(d, e).unwrap(),
            ]
        });
        let docs = p.documents().len();
        let e = insert_one(
            &mut p,
            aid,
            Skeleton::Chain(vec![s[0].clone(), s[1].clone()]),
        );
        assert!(e.unwrap_err().contains("tangent"));
        let e = insert_one(
            &mut p,
            aid,
            Skeleton::Chain(vec![s[0].clone(), s[2].clone()]),
        );
        assert!(e.unwrap_err().contains("not connected"));
        assert_eq!(p.documents().len(), docs);
    }

    /// A bend tighter than the section, and a whole circle, are refused
    /// before the kernel sees them.
    #[test]
    fn a_bend_the_section_does_not_fit_is_refused() {
        let (mut p, aid, _, s) = curve_skeleton(|sk| quarter_arc(sk, 15.0));
        let e = insert_one(&mut p, aid, s[0].clone()).unwrap_err();
        assert!(e.contains("does not fit"), "{e}");
        use crate::sketch::Point2;
        let (mut p, aid, _, s) = curve_skeleton(|sk| {
            let c = sk.add_point(Point2::new(0.0, 0.0));
            vec![sk.add_circle_at(c, 300.0).unwrap()]
        });
        let e = insert_one(&mut p, aid, s[0].clone()).unwrap_err();
        assert!(e.contains("closed"), "{e}");
    }

    /// A planar spline: the section goes along it (A x L within the spline's
    /// sampled length).
    #[test]
    fn a_member_along_a_spline() {
        use crate::sketch::Point2;
        let (mut p, aid, _, s) = curve_skeleton(|sk| {
            let v: Vec<_> = [(0.0, 0.0), (300.0, 120.0), (600.0, 0.0), (900.0, 120.0)]
                .iter()
                .map(|&(x, y)| sk.add_point(Point2::new(x, y)))
                .collect();
            vec![sk.add_interp_spline(v).unwrap()]
        });
        let occ = insert_one(&mut p, aid, s[0].clone()).unwrap();
        let l = asm(&p, aid).frames[0].members[0]
            .built
            .as_ref()
            .unwrap()
            .length;
        assert!(l > 960.0 && l < 1100.0, "{l}");
        let v = volume_of(&p, aid, occ);
        close_to(v, section_area() * l, 0.015 * section_area() * l);
    }

    /// End treatments need straight members: a curved one reports it and
    /// keeps its shape (health).
    #[test]
    fn a_curved_member_takes_no_end_treatment() {
        let (mut p, aid, _, s) = curve_skeleton(|sk| quarter_arc(sk, 300.0));
        let occ = insert_one(&mut p, aid, s[0].clone()).unwrap();
        let before = volume_of(&p, aid, occ);
        asm_mut(&mut p, aid).unwrap().frames[0]
            .treatments
            .push(Treatment::Lengthen {
                member: occ,
                offsets: [20.0, 0.0],
            });
        let done = solve(&mut p, &Linked::new(), aid).unwrap();
        assert!(
            done.sick
                .iter()
                .any(|(m, w)| *m == occ && w.contains("straight"))
        );
        assert!((volume_of(&p, aid, occ) - before).abs() < 1e-6);
    }

    /// Two members on the top edges of the box that meet at a corner:
    /// Miter cuts both on the bisector plane (45°), each keeps its outer
    /// length, and the volume is that of a mitred tube (mean length × A).
    #[test]
    fn miter_cuts_both_members_at_the_corner() {
        let (mut p, aid, _) = project();
        let x = find_edge(&p, aid, Point3::new(200.0, 0.0, 200.0));
        let y = find_edge(&p, aid, Point3::new(0.0, 150.0, 200.0));
        let mut o = opts();
        o.family = "Flat bar (EN 10058)".into();
        o.size = "40x5".into();
        let (_, occs) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &[x, y],
            &o,
            Library::builtin(),
            None,
        )
        .unwrap();
        let s = add_miter(
            &mut p,
            &Linked::new(),
            aid,
            &[(occs[0], occs[1])],
            0.0,
            MiterOffset::Default,
        )
        .unwrap();
        assert!(s.sick.is_empty(), "{:?}", s.sick);
        assert_eq!(s.moved, 2);
        let a = asm(&p, aid);
        // The flat lies with its 40 mm across in the horizontal plane: one
        // side 20 mm longer, the other 20 mm shorter at the mitred end.
        for (occ, len) in [(occs[0], 400.0), (occs[1], 300.0)] {
            let m = part(&p, a, occ);
            assert!(m.feature_health.is_empty(), "{:?}", m.feature_health);
            let v: Real = m
                .exact_bodies
                .0
                .iter()
                .filter_map(|b| b.volume_at(1e-3))
                .sum();
            assert!((v - 200.0 * len).abs() < 0.003 * 200.0 * len, "{occ}: {v}");
            let outer = outer_length(&m).unwrap();
            assert!((outer - (len + 20.0)).abs() < 1e-3, "{occ}: {outer}");
        }
        // Solving again changes nothing (no rebuild).
        assert_eq!(solve(&mut p, &Linked::new(), aid).unwrap().moved, 0);
        // A gap of 2 (Symmetric): each loses 1 mm along the bisector.
        add_miter(
            &mut p,
            &Linked::new(),
            aid,
            &[(occs[0], occs[1])],
            2.0,
            MiterOffset::Symmetric,
        )
        .unwrap();
        let a = asm(&p, aid);
        assert_eq!(a.frames[0].treatments.len(), 1);
        let m = part(&p, a, occs[0]);
        let v: Real = m
            .exact_bodies
            .0
            .iter()
            .filter_map(|b| b.volume_at(1e-3))
            .sum();
        let lost = 200.0 * std::f64::consts::SQRT_2; // 1 mm along the bisector = √2 along the axis
        assert!(
            (v - (200.0 * 400.0 - lost)).abs() < 0.003 * 200.0 * 400.0,
            "{v}"
        );
        // Remove End Treatments: back to the plain members.
        remove_treatments(&mut p, &Linked::new(), aid, &[occs[0]]).unwrap();
        let a = asm(&p, aid);
        assert!(a.frames[0].treatments.is_empty());
        let m = part(&p, a, occs[0]);
        assert!((outer_length(&m).unwrap() - 400.0).abs() < 1e-6);
    }

    /// Full Miter Cut of flats of different widths (40 and 60 across, in the
    /// corner's plane) meets edge to edge: each runs on by half the other's
    /// width (430 and 320 outside); Bisect Miter Cut cuts on the axes'
    /// bisector (420 and 330).
    #[test]
    fn full_and_bisect_miters_of_different_widths() {
        for (full, want) in [(true, [430.0, 320.0]), (false, [420.0, 330.0])] {
            let (mut p, aid, _) = project();
            let lk = Linked::new();
            let lib = Library::builtin();
            let x = find_edge(&p, aid, Point3::new(200.0, 0.0, 200.0));
            let y = find_edge(&p, aid, Point3::new(0.0, 150.0, 200.0));
            let mut o = opts();
            o.family = "Flat bar (EN 10058)".into();
            o.size = "40x5".into();
            let (_, a) = insert(&mut p, &lk, aid, &[x], &o, lib, None).unwrap();
            o.size = "60x10".into();
            let (_, b) = insert(&mut p, &lk, aid, &[y], &o, lib, None).unwrap();
            let s = add_miter_typed(
                &mut p,
                &lk,
                aid,
                &[(a[0], b[0])],
                0.0,
                MiterOffset::Default,
                full,
                false,
            )
            .unwrap();
            assert!(s.sick.is_empty(), "{:?}", s.sick);
            let d = asm(&p, aid);
            for (occ, len) in [(a[0], want[0]), (b[0], want[1])] {
                let outer = outer_length(&part(&p, d, occ)).unwrap();
                assert!((outer - len).abs() < 1e-3, "full {full}: {outer} vs {len}");
            }
        }
    }

    /// Delete existing end treatments: on, a Miter replaces the pair's
    /// Corner Joint; off, both stay (the mitre is applied after the joint).
    #[test]
    fn delete_existing_end_treatments_replaces_the_pairs_joint() {
        for delete in [true, false] {
            let (mut p, aid, _) = project();
            let lk = Linked::new();
            let x = find_edge(&p, aid, Point3::new(200.0, 0.0, 200.0));
            let y = find_edge(&p, aid, Point3::new(0.0, 150.0, 200.0));
            let (_, m) =
                insert(&mut p, &lk, aid, &[x, y], &opts(), Library::builtin(), None).unwrap();
            let t = Treatment::CornerJoint {
                a: m[0],
                b: m[1],
                offsets: [0.0, 0.0],
            };
            add_treatments(&mut p, &lk, aid, vec![t]).unwrap();
            add_miter_typed(
                &mut p,
                &lk,
                aid,
                &[(m[0], m[1])],
                0.0,
                MiterOffset::Default,
                true,
                delete,
            )
            .unwrap();
            let labels: Vec<&str> = asm(&p, aid).frames[0]
                .treatments
                .iter()
                .map(Treatment::label)
                .collect();
            if delete {
                assert_eq!(labels, ["Miter"]);
            } else {
                assert_eq!(labels, ["Corner Joint", "Miter"]);
            }
        }
    }

    /// Corner Joint of two flats at the top front corner: the first runs on
    /// to the far side of the second, which stops at the first's near side.
    #[test]
    fn corner_joint_butts_one_member_against_the_other() {
        let (mut p, aid, _) = project();
        let x = find_edge(&p, aid, Point3::new(200.0, 0.0, 200.0));
        let y = find_edge(&p, aid, Point3::new(0.0, 150.0, 200.0));
        let mut o = opts();
        o.family = "Flat bar (EN 10058)".into();
        o.size = "40x5".into();
        let (_, occs) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &[x, y],
            &o,
            Library::builtin(),
            None,
        )
        .unwrap();
        let t = Treatment::CornerJoint {
            a: occs[0],
            b: occs[1],
            offsets: [0.0, 0.0],
        };
        add_treatments(&mut p, &Linked::new(), aid, vec![t]).unwrap();
        let a = asm(&p, aid);
        // The flats lie with 40 mm across horizontally: a is 20 mm longer,
        // b 20 mm shorter, both square-cut.
        let la = outer_length(&part(&p, a, occs[0])).unwrap();
        let lb = outer_length(&part(&p, a, occs[1])).unwrap();
        assert!((la - 420.0).abs() < 1e-3, "{la}");
        assert!((lb - 280.0).abs() < 1e-3, "{lb}");
        let vb: Real = part(&p, a, occs[1])
            .exact_bodies
            .0
            .iter()
            .filter_map(|b| b.volume_at(1e-3))
            .sum();
        assert!((vb - 200.0 * 280.0).abs() < 0.002 * 200.0 * 280.0, "{vb}");
    }

    /// Insert Frame with Corners: the four top edges come out with a
    /// treatment at each of their four corners.
    #[test]
    fn insert_finishes_the_corners() {
        let lk = Linked::new();
        let top = [
            (200.0, 0.0, 200.0),
            (400.0, 150.0, 200.0),
            (200.0, 300.0, 200.0),
            (0.0, 150.0, 200.0),
        ];
        for (kind, label) in [
            (CornerType::Miter, "Miter"),
            (CornerType::ButtFirst, "Corner Joint"),
        ] {
            let (mut p, aid, _) = project();
            let edges: Vec<Skeleton> = top
                .iter()
                .map(|&(x, y, z)| find_edge(&p, aid, Point3::new(x, y, z)))
                .collect();
            let mut o = opts();
            o.corners = Corners {
                kind,
                gap: 0.0,
                tees: TeeJoint::None,
            };
            let (_, occs) = insert(&mut p, &lk, aid, &edges, &o, Library::builtin(), None).unwrap();
            let a = asm(&p, aid);
            let ts = &a.frames[0].treatments;
            assert_eq!(ts.len(), 4, "{label}");
            assert!(ts.iter().all(|t| t.label() == label));
            if kind == CornerType::ButtFirst {
                // One corner back to a mitre: one Miter, three joints.
                let (fid, before) = (a.frames[0].id, ts.len());
                retype_corner(&mut p, &lk, aid, fid, 0, CornerType::Miter).unwrap();
                let ts = &asm(&p, aid).frames[0].treatments;
                assert_eq!(ts.len(), before);
                assert_eq!(ts.iter().filter(|t| t.label() == "Miter").count(), 1);
                assert!(retype_corner(&mut p, &lk, aid, fid, 9, CornerType::Miter).is_err());
                continue;
            }
            if kind == CornerType::Miter {
                // Mitred both ends: outer length = edge + 2 × 20 (half the 40 tube).
                let l = outer_length(&part(&p, a, occs[0])).unwrap();
                assert!((l - 440.0).abs() < 1e-3, "{l}");
            } else {
                // The first member runs through both its corners; the second
                // stops at the first (and runs through at the third).
                let l0 = outer_length(&part(&p, a, occs[0])).unwrap();
                let l1 = outer_length(&part(&p, a, occs[1])).unwrap();
                assert!((l0 - 440.0).abs() < 1e-3, "{l0}");
                assert!((l1 - 300.0).abs() < 1e-3, "{l1}");
            }
        }
        // A negative gap is refused and nothing changes.
        let (mut p, aid, _) = project();
        let e = find_edge(&p, aid, Point3::new(200.0, 0.0, 200.0));
        let mut o = opts();
        o.corners.gap = -1.0;
        assert!(insert(&mut p, &lk, aid, &[e], &o, Library::builtin(), None).is_err());
        assert!(asm(&p, aid).frames.is_empty());
    }

    /// Change keeps the corner joint and solves it with the new section;
    /// a treatment deleted on its own leaves the others; deleting a member
    /// takes its part and its treatments.
    #[test]
    fn change_and_delete_keep_the_rest_of_the_frame() {
        let (mut p, aid, _) = project();
        let x = find_edge(&p, aid, Point3::new(200.0, 0.0, 200.0));
        let y = find_edge(&p, aid, Point3::new(0.0, 150.0, 200.0));
        let z = find_edge(&p, aid, Point3::new(0.0, 0.0, 100.0));
        let mut o = opts();
        o.family = "Flat bar (EN 10058)".into();
        o.size = "40x5".into();
        let lk = Linked::new();
        let (fid, occs) =
            insert(&mut p, &lk, aid, &[x, y, z], &o, Library::builtin(), None).unwrap();
        let t = Treatment::CornerJoint {
            a: occs[0],
            b: occs[1],
            offsets: [0.0, 0.0],
        };
        add_treatments(&mut p, &lk, aid, vec![t]).unwrap();
        let l = Treatment::Lengthen {
            member: occs[2],
            offsets: [0.0, 15.0],
        };
        add_treatments(&mut p, &lk, aid, vec![l]).unwrap();
        // 60 mm flats: a runs 30 past the corner, b stops 30 short.
        o.size = "60x10".into();
        change(
            &mut p,
            &lk,
            aid,
            &[occs[0], occs[1]],
            &o,
            Library::builtin(),
        )
        .unwrap();
        let a = asm(&p, aid);
        assert_eq!(a.frames[0].members[0].size, "60x10");
        // Its generated name follows the section.
        let named = a.find(occs[0]).map(|o| o.name.clone()).unwrap_or_default();
        assert!(named.contains("60x10"), "{named}");
        let la = outer_length(&part(&p, a, occs[0])).unwrap();
        let lb = outer_length(&part(&p, a, occs[1])).unwrap();
        assert!((la - 430.0).abs() < 1e-3, "{la}");
        assert!((lb - 270.0).abs() < 1e-3, "{lb}");
        let rows = treatment_rows(a);
        let names: Vec<&str> = rows.iter().map(|r| r.2.as_str()).collect();
        assert_eq!(names, ["Corner Joint 1", "Lengthen/Shorten 1"]);
        // Delete the corner joint only.
        delete_treatment(&mut p, &lk, aid, fid, 0).unwrap();
        let a = asm(&p, aid);
        assert_eq!(a.frames[0].treatments.len(), 1);
        assert!((outer_length(&part(&p, a, occs[0])).unwrap() - 400.0).abs() < 1e-3);
        assert!((outer_length(&part(&p, a, occs[2])).unwrap() - 215.0).abs() < 1e-3);
        // Delete the upright: its part and its Lengthen go.
        let doc = a.find(occs[2]).unwrap().component.document;
        delete_members(&mut p, &lk, aid, &[occs[2]]).unwrap();
        let a = asm(&p, aid);
        assert_eq!(a.frames[0].members.len(), 2);
        assert!(a.frames[0].treatments.is_empty());
        assert!(p.document(doc).is_none());
        // The skeleton is no member.
        assert!(delete_members(&mut p, &lk, aid, &[1]).is_err());
        assert!(change(&mut p, &lk, aid, &[1], &o, Library::builtin()).is_err());
    }

    /// Change of several members keeps what the panel did not change: each
    /// member's own size and rotation, while a new material reaches all.
    /// An appearance goes on the occurrence, or on the part with Apply
    /// Appearance to Part, and a rebuild keeps it.
    #[test]
    fn change_of_several_members_keeps_their_own_values() {
        let (mut p, aid, _) = project();
        let x = find_edge(&p, aid, Point3::new(200.0, 0.0, 200.0));
        let y = find_edge(&p, aid, Point3::new(0.0, 150.0, 200.0));
        let lk = Linked::new();
        let lib = Library::builtin();
        let (_, a) = insert(&mut p, &lk, aid, &[x], &opts(), lib, None).unwrap();
        let mut o = opts();
        o.size = "50x50x4".into();
        o.orientation.angle = 0.5;
        let (_, b) = insert(&mut p, &lk, aid, &[y], &o, lib, None).unwrap();
        let (a, b) = (a[0], b[0]);
        // The panel filled from the first member; only the material changed.
        let base = opts();
        let mut new = opts();
        new.material = Some("Aluminium 6061-T6".into());
        change_keeping(&mut p, &lk, aid, &[a, b], &new, &base, lib).unwrap();
        let d = asm(&p, aid);
        let mb = d
            .frames
            .iter()
            .flat_map(|f| &f.members)
            .find(|m| m.occurrence == b)
            .unwrap();
        assert_eq!(mb.size, "50x50x4");
        assert!((mb.orientation.angle - 0.5).abs() < 1e-12);
        for m in [a, b] {
            let pm = part(&p, d, m);
            assert_eq!(pm.material.as_deref(), Some("Aluminium 6061-T6"));
        }
        // A new size and offset reach both; the rotation stays b's own.
        let base = new.clone();
        new.size = "30x30x3".into();
        new.orientation.h_offset = 5.0;
        change_keeping(&mut p, &lk, aid, &[a, b], &new, &base, lib).unwrap();
        let d = asm(&p, aid);
        for m in d.frames.iter().flat_map(|f| &f.members) {
            assert_eq!(m.size, "30x30x3");
            assert!((m.orientation.h_offset - 5.0).abs() < 1e-12);
        }
        let mb = d
            .frames
            .iter()
            .flat_map(|f| &f.members)
            .find(|m| m.occurrence == b)
            .unwrap();
        assert!((mb.orientation.angle - 0.5).abs() < 1e-12);
        // Appearance: on the occurrence, then on the part.
        let look = crate::materials::builtin().appearances[0].name.clone();
        let base = new.clone();
        new.appearance = Some(look.clone());
        change_keeping(&mut p, &lk, aid, &[a], &new, &base, lib).unwrap();
        assert_eq!(
            member_appearance(&p, asm(&p, aid), a),
            (Some(look.clone()), false)
        );
        assert_eq!(member_appearance(&p, asm(&p, aid), b), (None, false));
        let base = new.clone();
        new.appearance_to_part = true;
        change_keeping(&mut p, &lk, aid, &[a], &new, &base, lib).unwrap();
        assert_eq!(
            member_appearance(&p, asm(&p, aid), a),
            (Some(look.clone()), true)
        );
        // A Lengthen rebuilds the part: the colour stays.
        let l = Treatment::Lengthen {
            member: a,
            offsets: [10.0, 0.0],
        };
        add_treatments(&mut p, &lk, aid, vec![l]).unwrap();
        assert_eq!(member_appearance(&p, asm(&p, aid), a), (Some(look), true));
        // The member carries its standard.
        assert_eq!(
            custom_text(&p, aid, a, crate::part::frame::STANDARD_PROPERTY).as_deref(),
            Some("EN")
        );
    }

    /// A T joint of sketch lines: a flat bar 40x5 up the Y axis ends on the
    /// axis of an RHS 100x50x4 along X. The skeleton assembly and the
    /// members (flat bar, RHS) of one frame.
    pub(crate) fn tee_frame() -> (Project, DocumentId, u32, u32) {
        tee_frame_of(
            ("Flat bar (EN 10058)", "40x5"),
            ("Rectangular hollow section (EN 10219)", "100x50x4"),
        )
    }

    /// [`tee_frame`] with the upright's and the cross member's sections.
    fn tee_frame_of(up_s: (&str, &str), across_s: (&str, &str)) -> (Project, DocumentId, u32, u32) {
        use crate::part::{OriginPlane, SketchOptions, SketchPlacement};
        use crate::sketch::Point2;
        let mut sk = PartDocument::default();
        let o = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let i = sk
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
            .unwrap();
        let s = &mut sk.sketches[i].sketch;
        let up = s.add_line(Point2::new(0.0, 0.0), Point2::new(0.0, 400.0));
        let across = s.add_line(Point2::new(-300.0, 400.0), Point2::new(300.0, 400.0));
        let sid = sk.sketches[i].id.clone();
        let mut p = Project::new("T");
        let skd = p.add_document("Skeleton", DocumentData::Part(sk));
        let mut a = AssemblyDocument::default();
        a.place(
            DocRef::local(skd),
            "Skeleton:1",
            Transform3::identity(),
            true,
        );
        let aid = p.add_document("Frame", DocumentData::Assembly(a));
        let line = |line| Skeleton::SketchLine {
            occurrence: 1,
            sub: Vec::new(),
            sketch: sid.clone(),
            line,
        };
        let mut o = opts();
        o.family = up_s.0.into();
        o.size = up_s.1.into();
        let (fid, a) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &[line(up)],
            &o,
            Library::builtin(),
            None,
        )
        .unwrap();
        o.family = across_s.0.into();
        o.size = across_s.1.into();
        let (_, b) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &[line(across)],
            &o,
            Library::builtin(),
            Some(fid),
        )
        .unwrap();
        (p, aid, a[0], b[0])
    }

    fn volume_of(p: &Project, aid: DocumentId, occ: u32) -> Real {
        part(p, asm(p, aid), occ)
            .exact_bodies
            .0
            .iter()
            .filter_map(|b| b.volume_at(1e-3))
            .sum()
    }

    /// Notch, Basic Profile: the flat bar loses what lies inside the RHS
    /// (50 wide across the bar's run, so 25 of its 400), 5 more with an
    /// offset of 5; the RHS keeps its volume; solving again rebuilds
    /// nothing; the notch survives a save.
    #[test]
    fn notch_cuts_a_member_to_the_other_ones_outline() {
        let (mut p, aid, bar, rhs) = tee_frame();
        let before = volume_of(&p, aid, rhs);
        let t = Treatment::Notch {
            notched: bar,
            by: rhs,
            offset: 0.0,
        };
        add_treatments(&mut p, &Linked::new(), aid, vec![t]).unwrap();
        let v = volume_of(&p, aid, bar);
        assert!((v - 200.0 * 375.0).abs() < 1e-3 * 200.0 * 375.0, "{v}");
        assert!((volume_of(&p, aid, rhs) - before).abs() < 1e-6 * before);
        assert_eq!(solve(&mut p, &Linked::new(), aid).unwrap().moved, 0);
        let t = Treatment::Notch {
            notched: bar,
            by: rhs,
            offset: 5.0,
        };
        add_treatments(&mut p, &Linked::new(), aid, vec![t]).unwrap();
        assert_eq!(asm(&p, aid).frames[0].treatments.len(), 1);
        let v = volume_of(&p, aid, bar);
        assert!((v - 200.0 * 370.0).abs() < 1e-3 * 200.0 * 370.0, "{v}");
        let dir = std::env::temp_dir().join(format!("cc-notch-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("notch.3dcadcraft");
        p.save(&f).unwrap();
        let mut q = Project::load(&f).unwrap();
        assert_eq!(
            asm(&q, aid).frames[0].treatments,
            asm(&p, aid).frames[0].treatments
        );
        assert_eq!(solve(&mut q, &Linked::new(), aid).unwrap().moved, 0);
        let _ = std::fs::remove_dir_all(&dir);
        // A member notching itself is refused, the frame unchanged.
        let t = Treatment::Notch {
            notched: rhs,
            by: rhs,
            offset: 0.0,
        };
        assert!(add_treatments(&mut p, &Linked::new(), aid, vec![t]).is_err());
        assert_eq!(asm(&p, aid).frames[0].treatments.len(), 1);
    }

    /// Members inserted apart into one frame get the corners with the ones
    /// already there (and the frame's own treatments stay); a Corner Joint
    /// between two frames (older files) makes them one frame.
    #[test]
    fn one_frame_for_inserts_apart_and_merged_frames() {
        let (mut p, aid, _) = project();
        let x = find_edge(&p, aid, Point3::new(200.0, 0.0, 200.0));
        let y = find_edge(&p, aid, Point3::new(0.0, 150.0, 200.0));
        let mut o = opts();
        let (fid, a) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &[x],
            &o,
            Library::builtin(),
            None,
        )
        .unwrap();
        o.corners.kind = CornerType::Miter;
        let (fid2, b) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &[y],
            &o,
            Library::builtin(),
            Some(fid),
        )
        .unwrap();
        assert_eq!(fid, fid2);
        let c = o.corners;
        add_corners(&mut p, &Linked::new(), aid, fid, &b, &c).unwrap();
        let ts = &asm(&p, aid).frames[0].treatments;
        assert_eq!(ts.len(), 1, "{ts:?}");
        assert!(matches!(ts[0], Treatment::Miter { .. }));
        // Two frames (inserted apart before): a Corner Joint merges them.
        let (mut p, aid, _) = project();
        let x = find_edge(&p, aid, Point3::new(200.0, 0.0, 200.0));
        let y = find_edge(&p, aid, Point3::new(0.0, 150.0, 200.0));
        let (_, a2) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &[x],
            &opts(),
            Library::builtin(),
            None,
        )
        .unwrap();
        let (_, b2) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &[y],
            &opts(),
            Library::builtin(),
            None,
        )
        .unwrap();
        assert_eq!(asm(&p, aid).frames.len(), 2);
        let t = Treatment::CornerJoint {
            a: a2[0],
            b: b2[0],
            offsets: [0.0, 0.0],
        };
        add_treatments(&mut p, &Linked::new(), aid, vec![t]).unwrap();
        assert_eq!(asm(&p, aid).frames.len(), 1);
        assert_eq!(asm(&p, aid).frames[0].members.len(), 2);
        let _ = a;
    }

    /// Between Points: a member from a vertex of the box (the start of one
    /// top edge) to the opposite corner of the top (the end of another): the
    /// diagonal, 500 long; it survives a save and follows the box.
    #[test]
    fn a_member_between_two_points() {
        let (mut p, aid, _) = project();
        let front = find_edge(&p, aid, Point3::new(200.0, 0.0, 200.0));
        let back = find_edge(&p, aid, Point3::new(200.0, 300.0, 200.0));
        let a = asm(&p, aid);
        let (f0, f1) = skeleton_line(&p, &Linked::new(), a, &front).unwrap();
        let (b0, b1) = skeleton_line(&p, &Linked::new(), a, &back).unwrap();
        // Start at front's end with x = 0, end at back's end with x = 400.
        let from_end = u8::from(f1.x < f0.x);
        let to_end = u8::from(b1.x > b0.x);
        let s = Skeleton::Between {
            from: Box::new(front),
            from_end,
            to: Box::new(back),
            to_end,
        };
        let (_, occs) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &[s],
            &opts(),
            Library::builtin(),
            None,
        )
        .unwrap();
        let m = part(&p, asm(&p, aid), occs[0]);
        assert!((length_of(&m).unwrap() - 500.0).abs() < 1e-6);
        let dir = std::env::temp_dir().join(format!("cc-between-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("between.3dcadcraft");
        p.save(&f).unwrap();
        let mut q = Project::load(&f).unwrap();
        assert_eq!(
            asm(&q, aid).frames[0].members[0].skeleton,
            asm(&p, aid).frames[0].members[0].skeleton
        );
        assert_eq!(solve(&mut q, &Linked::new(), aid).unwrap().moved, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Body Edges / Face Edges: the box skeleton has 12 straight edges, its
    /// top face 4.
    #[test]
    fn straight_edges_of_a_body_and_of_a_face() {
        let (p, aid, _) = project();
        let a = asm(&p, aid);
        let sk = part(&p, a, 1);
        let all = straight_edges(&sk, None);
        assert_eq!(all.len(), 12);
        let top = straight_edges(&sk, Some("Box1:Top"));
        assert_eq!(top.len(), 4);
        assert!(top.iter().all(|e| (e.hint[2] - 200.0).abs() < 1e-9));
    }

    /// The world box of a member's body.
    fn member_box(p: &Project, aid: DocumentId, occ: u32) -> (Point3, Point3) {
        let a = asm(p, aid);
        let t = a.find(occ).unwrap().transform;
        let (lo, hi) = part(p, a, occ).body.bounding_box().unwrap();
        let mut b = (
            Point3::new(Real::MAX, Real::MAX, Real::MAX),
            Point3::new(Real::MIN, Real::MIN, Real::MIN),
        );
        for x in [lo.x, hi.x] {
            for y in [lo.y, hi.y] {
                for z in [lo.z, hi.z] {
                    let q = t.transform_point(&Point3::new(x, y, z));
                    b.0 = b.0.inf(&q);
                    b.1 = b.1.sup(&q);
                }
            }
        }
        b
    }

    /// A frame from the whole box (its 12 edges) placed Inside with corners:
    /// every member lies inside the 400 × 300 × 200 volume, flush with its
    /// faces; at each corner of three the column runs through (200) and the
    /// others stop at it (400 − 2·40 = 320, 300 − 80 = 220). Edit Frame then
    /// puts them Outside: the column at the origin corner lies at x, y < 0.
    #[test]
    fn a_box_frame_inside_then_outside() {
        let (mut p, aid, _) = project();
        let sk = part(&p, asm(&p, aid), 1);
        let edges: Vec<Skeleton> = straight_edges(&sk, None)
            .into_iter()
            .map(|edge| {
                Skeleton::Edge(GeomRef::Edge {
                    occurrence: 1,
                    sub: Vec::new(),
                    edge,
                })
            })
            .collect();
        let mut o = opts();
        o.layout = Layout {
            side: Side::Inside,
            level: Level::Auto,
        };
        o.corners.kind = CornerType::Miter;
        let (fid, occs) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &edges,
            &o,
            Library::builtin(),
            None,
        )
        .unwrap();
        assert_eq!(occs.len(), 12);
        // 8 corners × 2 joints against the column.
        assert_eq!(asm(&p, aid).frames[0].treatments.len(), 16);
        let mut lengths: Vec<i64> = Vec::new();
        for &m in &occs {
            let (lo, hi) = member_box(&p, aid, m);
            assert!(lo.x > -1e-6 && lo.y > -1e-6 && lo.z > -1e-6, "{lo}");
            assert!(
                hi.x < 400.0 + 1e-6 && hi.y < 300.0 + 1e-6 && hi.z < 200.0 + 1e-6,
                "{hi}"
            );
            let d = hi - lo;
            lengths.push(d.x.max(d.y).max(d.z).round() as i64);
        }
        lengths.sort();
        assert_eq!(
            lengths,
            [200, 200, 200, 200, 220, 220, 220, 220, 320, 320, 320, 320]
        );
        // Each member a whole tube (the hole kept, outward faces): A × length.
        let area = Library::builtin()
            .find("Square hollow section (EN 10219)", "40x40x3")
            .unwrap()
            .properties()
            .area;
        for &m in &occs {
            let (lo, hi) = member_box(&p, aid, m);
            let d = hi - lo;
            let l = d.x.max(d.y).max(d.z);
            let v = volume_of(&p, aid, m);
            assert!(
                (v - area * l).abs() < 2e-3 * area * l,
                "member {m}: {v} vs {}",
                area * l
            );
            let body = part(&p, asm(&p, aid), m).body;
            let opts = crate::kernel::TessellationOptions::default();
            let signed: Real = body
                .solids()
                .map(|(sid, _)| {
                    let t = body.tessellate_solid(sid, &opts).unwrap();
                    t.faces
                        .triangles
                        .iter()
                        .map(|tr| {
                            let [a, b, c] = tr.map(|i| t.faces.positions[i as usize].coords);
                            a.dot(&b.cross(&c)) / 6.0
                        })
                        .sum::<Real>()
                })
                .sum();
            assert!(
                (signed - area * l).abs() < 5e-3 * area * l,
                "member {m} display mesh: {signed} vs {}",
                area * l
            );
            let ph = crate::part::physical::physical_properties(
                &part(&p, asm(&p, aid), m),
                Default::default(),
            )
            .unwrap();
            let want = area * l * 7.85e-3;
            assert!(
                (ph.total.mass - want).abs() < 5e-3 * want,
                "member {m} mass {} vs {want}",
                ph.total.mass
            );
        }
        // The same after a save and a load.
        {
            let dir = std::env::temp_dir().join(format!("cc-boxmass-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            let f = dir.join("box.3dcadcraft");
            p.save(&f).unwrap();
            let q = Project::load(&f).unwrap();
            for &m in &occs {
                let mp = part(&q, asm(&q, aid), m);
                let ph =
                    crate::part::physical::physical_properties(&mp, Default::default()).unwrap();
                let (lo, hi) = member_box(&p, aid, m);
                let d = hi - lo;
                let want = area * d.x.max(d.y).max(d.z) * 7.85e-3;
                assert!(
                    (ph.total.mass - want).abs() < 5e-3 * want,
                    "loaded member {m} mass {} vs {want}, warnings {:?}",
                    ph.total.mass,
                    ph.warnings
                );
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
        let out = Layout {
            side: Side::Outside,
            level: Level::Auto,
        };
        edit_frame(&mut p, &Linked::new(), aid, fid, Some(out), None).unwrap();
        let column = occs
            .iter()
            .copied()
            .find(|&m| {
                let (lo, hi) = member_box(&p, aid, m);
                hi.z - lo.z > 150.0 && hi.x < 1.0 && hi.y < 1.0
            })
            .expect("a column outside the origin corner");
        let (lo, _) = member_box(&p, aid, column);
        assert!(
            (lo.x + 40.0).abs() < 1e-6 && (lo.y + 40.0).abs() < 1e-6,
            "{lo}"
        );
        // Corners again: the treatments are replaced, not added.
        let c = Corners {
            kind: CornerType::Miter,
            gap: 0.0,
            tees: TeeJoint::None,
        };
        edit_frame(&mut p, &Linked::new(), aid, fid, None, Some(c)).unwrap();
        assert_eq!(asm(&p, aid).frames[0].treatments.len(), 16);
    }

    /// Edit of a member: it moves to another skeleton edge and takes a new
    /// anchor; a second member keeps its place.
    #[test]
    fn a_member_moves_to_another_edge_with_its_own_anchor() {
        let (mut p, aid, _) = project();
        let front = find_edge(&p, aid, Point3::new(200.0, 0.0, 200.0));
        let left = find_edge(&p, aid, Point3::new(0.0, 150.0, 200.0));
        let back = find_edge(&p, aid, Point3::new(200.0, 300.0, 200.0));
        let (_, occs) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &[front, left],
            &opts(),
            Library::builtin(),
            None,
        )
        .unwrap();
        let kept = member_box(&p, aid, occs[1]);
        let e = MemberEdit {
            occurrence: occs[0],
            skeleton: Some(back),
            orientation: Orientation {
                anchor: Anchor::TopRight,
                ..Default::default()
            },
        };
        edit_members(
            &mut p,
            &Linked::new(),
            aid,
            &[e],
            &opts(),
            Library::builtin(),
        )
        .unwrap();
        let (lo, hi) = member_box(&p, aid, occs[0]);
        // Along X at y = 300, z = 200, hanging below the edge on one side.
        assert!((hi.x - lo.x - 400.0).abs() < 1e-6, "{lo} {hi}");
        assert!(
            (hi.z - 200.0).abs() < 1e-6 && (lo.z - 160.0).abs() < 1e-6,
            "{lo} {hi}"
        );
        assert!(
            (hi.y - lo.y - 40.0).abs() < 1e-6
                && ((lo.y - 300.0).abs() < 1e-6 || (hi.y - 300.0).abs() < 1e-6),
            "{lo} {hi}"
        );
        let a = asm(&p, aid);
        let m = a.frames[0]
            .members
            .iter()
            .find(|m| m.occurrence == occs[0])
            .unwrap();
        assert_eq!(m.orientation.anchor, Anchor::TopRight);
        assert_eq!(member_box(&p, aid, occs[1]), kept);
    }

    /// A member's mass (Frame Member Info): steel S235 at 7.85 g/cm³ over
    /// the tube's volume.
    #[test]
    fn member_mass_is_steel() {
        let (mut p, aid, _) = project();
        let e = find_edge(&p, aid, Point3::new(200.0, 0.0, 200.0));
        let (_, occs) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &[e],
            &opts(),
            Library::builtin(),
            None,
        )
        .unwrap();
        let m = part(&p, asm(&p, aid), occs[0]);
        let ph = crate::part::physical::physical_properties(&m, Default::default()).unwrap();
        let area = Library::builtin()
            .find("Square hollow section (EN 10219)", "40x40x3")
            .unwrap()
            .properties()
            .area;
        let want = area * 400.0 * 7.85e-3;
        assert!(ph.warnings.is_empty(), "{:?}", ph.warnings);
        assert!(
            (ph.total.mass - want).abs() < 2e-3 * want,
            "{} vs {want} (volume {})",
            ph.total.mass,
            ph.total.volume
        );
    }

    /// Butt by length (Solid Edge's Butt 1 / Butt 2): of the 400 and 300
    /// top edges, the longer or the shorter runs through whatever the pick
    /// order; T Joints notch a member ending on the middle of another.
    #[test]
    fn butt_by_length_and_tee_joints() {
        let (mut p, aid, _) = project();
        let x = find_edge(&p, aid, Point3::new(200.0, 0.0, 200.0));
        let y = find_edge(&p, aid, Point3::new(0.0, 150.0, 200.0));
        let mut o = opts();
        o.family = "Flat bar (EN 10058)".into();
        o.size = "40x5".into();
        o.corners.kind = CornerType::ButtShorter;
        let (_, occs) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &[x, y],
            &o,
            Library::builtin(),
            None,
        )
        .unwrap();
        // The 300 one runs through: 300 + 20, the 400 one 400 − 20.
        let a = asm(&p, aid);
        assert!((outer_length(&part(&p, a, occs[1])).unwrap() - 320.0).abs() < 1e-3);
        assert!((outer_length(&part(&p, a, occs[0])).unwrap() - 380.0).abs() < 1e-3);
        let (mut p, aid, bar, rhs) = tee_frame();
        let a = asm(&p, aid);
        let c = Corners {
            kind: CornerType::None,
            gap: 0.0,
            tees: TeeJoint::Notch,
        };
        let ts = corner_treatments(&p, &Linked::new(), a, &[bar, rhs], &c);
        assert_eq!(
            ts,
            [Treatment::Notch {
                notched: bar,
                by: rhs,
                offset: 0.0
            }]
        );
        let fid = asm(&p, aid).frames[0].id;
        edit_frame(&mut p, &Linked::new(), aid, fid, None, Some(c)).unwrap();
        assert_eq!(asm(&p, aid).frames[0].treatments.len(), 1);
    }

    /// Two SHS 40x40x3 in a T: the notching member's top and bottom walls
    /// lie in the planes of the upright's; the notch still cuts the upright
    /// 20 short of the axis.
    #[test]
    fn notch_of_equal_tubes() {
        let shs = ("Square hollow section (EN 10219)", "40x40x3");
        let (mut p, aid, up, across) = tee_frame_of(shs, shs);
        let full = volume_of(&p, aid, up);
        let t = Treatment::Notch {
            notched: up,
            by: across,
            offset: 0.0,
        };
        add_treatments(&mut p, &Linked::new(), aid, vec![t]).unwrap();
        let v = volume_of(&p, aid, up);
        // 20 of 400 gone, less what the notching tube's corner rounds
        // (r 6) leave in the upright's top and bottom walls: at most two
        // corners' (1 - π/4)·6² over the 40 width.
        let want = full * 380.0 / 400.0;
        let rounds = 2.0 * (1.0 - std::f64::consts::FRAC_PI_4) * 36.0 * 40.0;
        assert!(v > want - 1e-4 * want && v < want + rounds, "{v} vs {want}");
    }

    /// Trim/Extend to the skeleton box's top face (offset 10): the vertical
    /// member at a corner ends 10 mm below it; Lengthen/Shorten then adds 25.
    #[test]
    fn trim_to_face_and_lengthen() {
        let (mut p, aid, _) = project();
        let e = find_edge(&p, aid, Point3::new(0.0, 0.0, 100.0));
        let (_, occs) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &[e],
            &opts(),
            Library::builtin(),
            None,
        )
        .unwrap();
        let top = GeomRef::Face {
            occurrence: 1,
            sub: Vec::new(),
            name: "Box1:Top".into(),
            hint: [200.0, 150.0, 200.0],
        };
        let t = Treatment::TrimToFace {
            member: occs[0],
            face: top,
            offset: 10.0,
        };
        add_treatments(&mut p, &Linked::new(), aid, vec![t]).unwrap();
        let l = outer_length(&part(&p, asm(&p, aid), occs[0])).unwrap();
        assert!((l - 190.0).abs() < 1e-3, "{l}");
        let t = Treatment::Lengthen {
            member: occs[0],
            offsets: [0.0, 25.0],
        };
        add_treatments(&mut p, &Linked::new(), aid, vec![t]).unwrap();
        let l = outer_length(&part(&p, asm(&p, aid), occs[0])).unwrap();
        assert!((l - 215.0).abs() < 1e-3, "{l}");
        // Shorten the start by 30 as well.
        let t = Treatment::Lengthen {
            member: occs[0],
            offsets: [-30.0, 25.0],
        };
        add_treatments(&mut p, &Linked::new(), aid, vec![t]).unwrap();
        let a = asm(&p, aid);
        assert_eq!(a.frames[0].treatments.len(), 2);
        let l = outer_length(&part(&p, a, occs[0])).unwrap();
        assert!((l - 185.0).abs() < 1e-3, "{l}");
    }

    /// Members that do not meet are refused and nothing changes.
    #[test]
    fn miter_of_members_apart_is_refused() {
        let (mut p, aid, _) = project();
        let x = find_edge(&p, aid, Point3::new(200.0, 0.0, 0.0));
        let y = find_edge(&p, aid, Point3::new(200.0, 300.0, 200.0));
        let (_, occs) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &[x, y],
            &opts(),
            Library::builtin(),
            None,
        )
        .unwrap();
        let e = add_miter(
            &mut p,
            &Linked::new(),
            aid,
            &[(occs[0], occs[1])],
            0.0,
            MiterOffset::Default,
        )
        .unwrap_err();
        assert!(e.contains("do not meet"), "{e}");
        assert!(asm(&p, aid).frames[0].treatments.is_empty());
    }

    /// The four top edges of the box: every pair that meets.
    fn custom_text(p: &Project, a: DocumentId, occ: u32, name: &str) -> Option<String> {
        let doc = asm_of(p, a).ok()?.find(occ)?.component.document;
        let c = p
            .document(doc)?
            .properties
            .custom
            .iter()
            .find(|c| c.name == name)?;
        Some(c.value.text())
    }

    /// A mitred corner of 90° reads 45° at the cut end only; removing the
    /// treatment removes the text; a skewed mitre of a 60° corner reads 60°.
    #[test]
    fn cut_details_come_from_the_final_ends() {
        let (mut p, aid, _) = project();
        let linked = Linked::new();
        let e = [
            edge("Front", "Top", [200.0, 0.0, 200.0]),
            edge("Right", "Top", [400.0, 150.0, 200.0]),
        ];
        let (_, occs) =
            insert(&mut p, &linked, aid, &e, &opts(), Library::builtin(), None).unwrap();
        for &o in &occs {
            assert_eq!(custom_text(&p, aid, o, "CUTDETAIL1"), None);
            assert_eq!(custom_text(&p, aid, o, "CUTDETAIL2"), None);
        }
        add_miter(
            &mut p,
            &linked,
            aid,
            &[(occs[0], occs[1])],
            0.0,
            MiterOffset::Default,
        )
        .unwrap();
        let text = |p: &Project, o, k| custom_text(p, aid, o, k);
        let all = [
            text(&p, occs[0], "CUTDETAIL1"),
            text(&p, occs[0], "CUTDETAIL2"),
            text(&p, occs[1], "CUTDETAIL1"),
            text(&p, occs[1], "CUTDETAIL2"),
        ];
        assert_eq!(
            all.iter()
                .flatten()
                .filter(|t| *t == "Miter 45.00 deg")
                .count(),
            2,
            "{all:?}"
        );
        assert_eq!(all.iter().flatten().count(), 2, "{all:?}");
        // A solve with nothing to do writes nothing new.
        let before = p.clone();
        solve(&mut p, &linked, aid).unwrap();
        assert_eq!(
            text(&before, occs[0], "CUTDETAIL2"),
            text(&p, occs[0], "CUTDETAIL2")
        );
        remove_treatments(&mut p, &linked, aid, &occs).unwrap();
        for &o in &occs {
            assert_eq!(custom_text(&p, aid, o, "CUTDETAIL1"), None);
            assert_eq!(custom_text(&p, aid, o, "CUTDETAIL2"), None);
        }
    }

    fn caps_of(p: &Project, a: DocumentId) -> Vec<EndCap> {
        asm_of(p, a).unwrap().frames[0].caps.clone()
    }

    fn cap_opts() -> CapOptions {
        CapOptions {
            internal: false,
            corners: CapCorners::Sharp,
            thickness: 5.0,
            ratio: false,
            edge_offset: 0.0,
            rotate: 0.0,
            offset: 0.0,
        }
    }

    /// One 40x40x3 member along the box's front top edge, 400 long.
    fn one_member(p: &mut Project, a: DocumentId) -> u32 {
        let e = [edge("Front", "Top", [200.0, 0.0, 200.0])];
        insert(p, &Linked::new(), a, &e, &opts(), Library::builtin(), None)
            .unwrap()
            .1[0]
    }

    fn close_to(v: Real, want: Real, tol: Real) {
        assert!((v - want).abs() < tol, "{v} vs {want}");
    }

    #[test]
    fn an_external_cap_sits_beyond_the_end_with_the_outline_area() {
        let (mut p, aid, _) = project();
        let m = one_member(&mut p, aid);
        let c = insert_caps(
            &mut p,
            &Linked::new(),
            aid,
            &[(m, true)],
            &cap_opts(),
            Some("Steel S235"),
        )
        .unwrap()[0];
        // Sharp corners: 40 x 40 x 5.
        close_to(volume_of(&p, aid, c), 8000.0, 1.0);
        let a = asm_of(&p, aid).unwrap();
        let mt = a.find(m).unwrap().transform;
        let at = mt.inverse() * a.find(c).unwrap().transform;
        close_to(at.translation.vector.z, 400.0, 1e-6);
    }

    #[test]
    fn an_internal_cap_fits_the_hole_and_stays_inside() {
        let (mut p, aid, _) = project();
        let m = one_member(&mut p, aid);
        let o = CapOptions {
            internal: true,
            edge_offset: 0.5,
            offset: 2.0,
            ..cap_opts()
        };
        let c = insert_caps(&mut p, &Linked::new(), aid, &[(m, false)], &o, None).unwrap()[0];
        // A 34 x 34 hole 0.5 smaller all round, 5 thick.
        close_to(volume_of(&p, aid, c), 33.0 * 33.0 * 5.0, 1.0);
        let a = asm_of(&p, aid).unwrap();
        let at = a.find(m).unwrap().transform.inverse() * a.find(c).unwrap().transform;
        close_to(at.translation.vector.z, 2.0, 1e-6);
    }

    #[test]
    fn filleted_and_chamfered_plates_lose_their_corners() {
        let (mut p, aid, _) = project();
        let m = one_member(&mut p, aid);
        let r = 6.0; // the section's outer radius (2 t)
        let square = 1600.0 * 5.0;
        let fillet = (4.0 - std::f64::consts::PI) * r * r * 5.0;
        let chamfer = 2.0 * r * r * 5.0;
        let filleted = CapOptions {
            corners: CapCorners::Filleted,
            ..cap_opts()
        };
        let chamfered = CapOptions {
            corners: CapCorners::Chamfered,
            ..cap_opts()
        };
        let linked = Linked::new();
        let f = insert_caps(&mut p, &linked, aid, &[(m, true)], &filleted, None).unwrap()[0];
        let c = insert_caps(&mut p, &linked, aid, &[(m, false)], &chamfered, None).unwrap()[0];
        close_to(volume_of(&p, aid, f), square - fillet, 2.0);
        close_to(volume_of(&p, aid, c), square - chamfer, 2.0);
    }

    #[test]
    fn a_cap_follows_its_member_and_goes_with_it() {
        let (mut p, aid, sk) = project();
        let linked = Linked::new();
        let m = one_member(&mut p, aid);
        let c = insert_caps(&mut p, &linked, aid, &[(m, true)], &cap_opts(), None).unwrap()[0];
        // The skeleton grows: the end, and the cap on it, move.
        if let Some(DocumentData::Part(x)) = p.document_mut(sk).map(|d| &mut d.data) {
            *x = PartDocument::with_box("Box1", [500.0, 300.0, 200.0]).unwrap();
        }
        let s = solve(&mut p, &linked, aid).unwrap();
        assert!(s.sick.is_empty(), "{:?}", s.sick);
        let a = asm_of(&p, aid).unwrap();
        let at = a.find(m).unwrap().transform.inverse() * a.find(c).unwrap().transform;
        close_to(at.translation.vector.z, 500.0, 1e-6);
        // Change of section: the plate is rebuilt to it.
        let lib = Library::builtin();
        let mut o2 = opts();
        o2.size = "50x50x4".into();
        change(&mut p, &linked, aid, &[m], &o2, lib).unwrap();
        close_to(volume_of(&p, aid, c), 2500.0 * 5.0, 1.0);
        // A member deleted takes its cap and the cap's part.
        let docs = p.documents().len();
        delete_members(&mut p, &linked, aid, &[m]).unwrap();
        assert!(asm_of(&p, aid).unwrap().find(c).is_none());
        assert_eq!(p.documents().len(), docs - 2);
    }

    #[test]
    fn a_cap_is_refused_where_it_cannot_be_and_nothing_changes() {
        let (mut p, aid, _) = project();
        let linked = Linked::new();
        let m = one_member(&mut p, aid);
        let n = p.documents().len();
        // No such end twice, no zero thickness, no plate left by the offset.
        insert_caps(&mut p, &linked, aid, &[(m, true)], &cap_opts(), None).unwrap();
        assert!(insert_caps(&mut p, &linked, aid, &[(m, true)], &cap_opts(), None).is_err());
        let thin = CapOptions {
            thickness: 0.0,
            ..cap_opts()
        };
        assert!(insert_caps(&mut p, &linked, aid, &[(m, false)], &thin, None).is_err());
        let wide = CapOptions {
            edge_offset: 30.0,
            corners: CapCorners::Filleted,
            ..cap_opts()
        };
        assert!(insert_caps(&mut p, &linked, aid, &[(m, false)], &wide, None).is_err());
        assert_eq!(caps_of(&p, aid).len(), 1);
        assert_eq!(p.documents().len(), n + 1);
        // Removing it brings back the free end.
        let c = caps_of(&p, aid)[0].occurrence;
        delete_caps(&mut p, &linked, aid, &[c]).unwrap();
        assert!(caps_of(&p, aid).is_empty());
        assert_eq!(p.documents().len(), n);
    }

    fn component_of(p: &Project, a: DocumentId, occ: u32) -> DocRef {
        asm_of(p, a).unwrap().find(occ).unwrap().component
    }

    /// The front and back top edges are equally long; the sides are not.
    fn front_and_back(p: &mut Project, a: DocumentId) -> (u32, Skeleton, Skeleton) {
        let m = one_member(p, a);
        (
            m,
            edge("Back", "Top", [200.0, 300.0, 200.0]),
            edge("Right", "Top", [400.0, 150.0, 200.0]),
        )
    }

    #[test]
    fn reuse_places_the_same_part_and_refuses_another_length() {
        let (mut p, aid, _) = project();
        let linked = Linked::new();
        let (m, back, side) = front_and_back(&mut p, aid);
        let n = p.documents().len();
        assert!(reuse(&mut p, &linked, aid, m, std::slice::from_ref(&side), false).is_err());
        assert_eq!(p.documents().len(), n);
        let r = reuse(&mut p, &linked, aid, m, &[back], false).unwrap()[0];
        assert_eq!(p.documents().len(), n, "no new part");
        assert_eq!(component_of(&p, aid, r), component_of(&p, aid, m));
        let a = asm_of(&p, aid).unwrap();
        close_to(
            a.find(r).unwrap().transform.translation.vector.y,
            300.0,
            21.0,
        );
        // A reused member is not cut, changed or capped on its own.
        let t = Treatment::Lengthen {
            member: r,
            offsets: [5.0, 0.0],
        };
        assert!(add_treatments(&mut p, &linked, aid, vec![t]).is_err());
        assert!(change(&mut p, &linked, aid, &[r], &opts(), Library::builtin()).is_err());
        assert!(insert_caps(&mut p, &linked, aid, &[(r, true)], &cap_opts(), None).is_err());
        // Picking a reused member finds its source.
        let again = edge("Front", "Bottom", [200.0, 0.0, 0.0]);
        let r2 = reuse(&mut p, &linked, aid, r, &[again], false).unwrap()[0];
        let f = &asm_of(&p, aid).unwrap().frames[0];
        assert_eq!(
            f.members
                .iter()
                .find(|x| x.occurrence == r2)
                .unwrap()
                .reuse_of,
            Some(m)
        );
    }

    #[test]
    fn a_change_on_the_source_reaches_the_reused_members() {
        let (mut p, aid, _) = project();
        let linked = Linked::new();
        let (m, back, _) = front_and_back(&mut p, aid);
        let r = reuse(&mut p, &linked, aid, m, &[back], false).unwrap()[0];
        let t = Treatment::Lengthen {
            member: m,
            offsets: [10.0, 0.0],
        };
        add_treatments(&mut p, &linked, aid, vec![t]).unwrap();
        close_to(volume_of(&p, aid, r), volume_of(&p, aid, m), 1e-6);
        assert!(solve(&mut p, &linked, aid).unwrap().sick.is_empty());
    }

    #[test]
    fn deleting_a_source_hands_it_on_and_the_last_takes_the_part() {
        let (mut p, aid, _) = project();
        let linked = Linked::new();
        let (m, back, _) = front_and_back(&mut p, aid);
        let again = edge("Front", "Bottom", [200.0, 0.0, 0.0]);
        let r = reuse(&mut p, &linked, aid, m, &[back, again], false).unwrap();
        let n = p.documents().len();
        delete_members(&mut p, &linked, aid, &[m]).unwrap();
        assert_eq!(p.documents().len(), n, "the part is still shown");
        let f = &asm_of(&p, aid).unwrap().frames[0];
        assert_eq!(
            f.members
                .iter()
                .find(|x| x.occurrence == r[0])
                .unwrap()
                .reuse_of,
            None
        );
        assert_eq!(
            f.members
                .iter()
                .find(|x| x.occurrence == r[1])
                .unwrap()
                .reuse_of,
            Some(r[0])
        );
        delete_members(&mut p, &linked, aid, &r).unwrap();
        assert_eq!(p.documents().len(), n - 1);
    }

    #[test]
    fn change_reuse_moves_and_flips_a_reused_member() {
        let (mut p, aid, _) = project();
        let linked = Linked::new();
        let (src, back, _) = front_and_back(&mut p, aid);
        let r = reuse(&mut p, &linked, aid, src, &[back], false).unwrap()[0];
        let left = edge("Left", "Bottom", [0.0, 150.0, 0.0]);
        let side = edge("Right", "Bottom", [400.0, 150.0, 0.0]);
        let bottom = edge("Front", "Bottom", [200.0, 0.0, 0.0]);
        // Another length is refused and changes nothing.
        let before = asm(&p, aid).frames.clone();
        assert!(change_reuse(&mut p, &linked, aid, &[r], &[left], false).is_err());
        assert_eq!(asm(&p, aid).frames, before);
        // The source is no reused member; two lines for one member is wrong.
        assert!(change_reuse(&mut p, &linked, aid, &[src], &[], true).is_err());
        assert!(change_reuse(&mut p, &linked, aid, &[r], &[side.clone(), side], false).is_err());
        change_reuse(&mut p, &linked, aid, &[r], &[bottom.clone()], true).unwrap();
        let a = asm(&p, aid);
        let m = a.frames[0]
            .members
            .iter()
            .find(|m| m.occurrence == r)
            .unwrap();
        assert_eq!(m.skeleton, bottom);
        assert!(m.orientation.flip);
        // It now lies along the bottom front edge (z near 0).
        let (lo, hi) = member_box(&p, aid, r);
        assert!(lo.z < 1.0 && hi.z < 41.0, "{lo:?} {hi:?}");
        assert_eq!(component_of(&p, aid, r), component_of(&p, aid, src));
    }

    #[test]
    fn refresh_takes_the_changed_table_and_keeps_the_rest() {
        let (mut p, aid, _) = project();
        let lk = Linked::new();
        let lib = Library::builtin();
        let m = one_member(&mut p, aid);
        let before = volume_of(&p, aid, m);
        // Nothing changed: nothing to do.
        assert!(refresh(&mut p, &lk, aid, &[], lib).unwrap().is_empty());
        // The table's 40x40x3 now has a 4 mm wall.
        let mut changed = lib.clone();
        for s in &mut changed.sections {
            if s.size == "40x40x3" && s.family.starts_with("Square hollow") {
                s.t = 4.0;
            }
        }
        let rotated = Orientation {
            angle: 0.3,
            ..Default::default()
        };
        set_orientations(&mut p, &lk, aid, &[(m, rotated)]).unwrap();
        assert_eq!(refresh(&mut p, &lk, aid, &[], &changed).unwrap(), vec![m]);
        let a = asm(&p, aid);
        assert!((a.frames[0].members[0].section.dims[2] - 4.0).abs() < 1e-12);
        assert!((a.frames[0].members[0].orientation.angle - 0.3).abs() < 1e-12);
        assert_eq!(
            part(&p, a, m).material.as_deref(),
            Some("Steel S235"),
            "the material stays"
        );
        assert!(volume_of(&p, aid, m) > before * 1.2);
    }

    #[test]
    fn an_edited_cap_takes_its_new_thickness_and_place() {
        let (mut p, aid, _) = project();
        let lk = Linked::new();
        let m = one_member(&mut p, aid);
        let c = insert_caps(&mut p, &lk, aid, &[(m, true)], &cap_opts(), None).unwrap()[0];
        let thin = volume_of(&p, aid, c);
        let mut o = cap_opts();
        o.thickness *= 2.0;
        edit_caps(&mut p, &lk, aid, &[c], &o, Some("Steel S235")).unwrap();
        close_to(volume_of(&p, aid, c), 2.0 * thin, thin * 1e-3);
        let a = asm(&p, aid);
        assert_eq!(caps_of(&p, aid)[0].options, o);
        assert_eq!(part(&p, a, c).material.as_deref(), Some("Steel S235"));
        // A bad thickness changes nothing.
        let mut bad = o;
        bad.thickness = 0.0;
        assert!(edit_caps(&mut p, &lk, aid, &[c], &bad, None).is_err());
        assert_eq!(caps_of(&p, aid)[0].options, o);
        // A member is no cap.
        assert!(edit_caps(&mut p, &lk, aid, &[m], &o, None).is_err());
    }

    #[test]
    fn members_take_the_names_given_and_refuse_clashes() {
        let (mut p, aid, sk) = project();
        let lk = Linked::new();
        let x = find_edge(&p, aid, Point3::new(200.0, 0.0, 200.0));
        let y = find_edge(&p, aid, Point3::new(0.0, 150.0, 200.0));
        let (_, occs) =
            insert(&mut p, &lk, aid, &[x, y], &opts(), Library::builtin(), None).unwrap();
        let mut rows = member_names(&p, asm(&p, aid), &occs);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].file.starts_with("SHS 40x40x3-"), "{}", rows[0].file);
        // Swapping the two file names works.
        let (f0, f1) = (rows[0].file.clone(), rows[1].file.clone());
        rows[0].file = f1.clone();
        rows[1].file = f0.clone();
        rows[0].display = "Front rail".into();
        rename_members(&mut p, aid, &rows).unwrap();
        let now = member_names(&p, asm(&p, aid), &occs);
        assert_eq!(now[0].file, f1);
        assert_eq!(now[1].file, f0);
        assert_eq!(now[0].display, "Front rail");
        // The skeleton's name, a doubled name and an empty one are refused.
        let skel = p.document(sk).unwrap().name.clone();
        for bad in [skel, f0.clone(), String::new()] {
            let mut r = now.clone();
            r[0].file = bad.clone();
            assert!(rename_members(&mut p, aid, &r).is_err(), "{bad}");
            assert_eq!(member_names(&p, asm(&p, aid), &occs), now);
        }
    }

    /// Align: the section's horizontal axis follows the given direction
    /// (square to the line); a direction along the line is ignored.
    #[test]
    fn align_turns_the_section_after_a_direction() {
        let s = Library::builtin()
            .find("Rectangular hollow section (EN 10219)", "100x50x4")
            .unwrap()
            .clone();
        let (a, b) = (
            Point3::origin(),
            Point3::new(0.0, 0.0, 0.0) + Vector3::x() * 500.0,
        );
        let plain = placement(&s, a, b, &Orientation::default()).unwrap();
        let tilted = Orientation {
            align: Some([0.0, 0.0, 1.0]),
            ..Default::default()
        };
        let t = placement(&s, a, b, &tilted).unwrap();
        // Part X (the section's horizontal axis) now points up.
        let x = t.transform_vector(&Vector3::x());
        assert!((x - Vector3::z()).norm() < 1e-9, "{x:?}");
        assert!((plain.transform_vector(&Vector3::x()) - Vector3::z()).norm() > 0.5);
        // Along the line: the default.
        let along = Orientation {
            align: Some([1.0, 0.0, 0.0]),
            ..Default::default()
        };
        let p = placement(&s, a, b, &along).unwrap();
        assert!(
            (p.transform_vector(&Vector3::x()) - plain.transform_vector(&Vector3::x())).norm()
                < 1e-9
        );
    }

    /// File Naming Defaults: a pattern of attributes and `{#}`; numbers skip
    /// names taken in the file and by the other new members.
    #[test]
    fn member_names_follow_the_pattern() {
        let (mut p, aid, _) = project();
        let lk = Linked::new();
        let x = find_edge(&p, aid, Point3::new(200.0, 0.0, 200.0));
        let y = find_edge(&p, aid, Point3::new(0.0, 150.0, 200.0));
        let (_, occs) =
            insert(&mut p, &lk, aid, &[x, y], &opts(), Library::builtin(), None).unwrap();
        let a = asm(&p, aid);
        let n = names_from_pattern(&p, a, &occs, "{Standard} {Stock Number} {Length} #{#}");
        assert_eq!(n[0].file, "EN SHS 40x40x3 400 #1");
        assert_eq!(n[1].file, "EN SHS 40x40x3 300 #1");
        let n = names_from_pattern(&p, a, &occs, "Rail");
        assert_eq!(n[0].file, "Rail");
        assert_eq!(n[1].file, "Rail-2");
        assert_eq!(n[1].display, "Rail-2:1");
        // The skeleton's name is taken.
        let n = names_from_pattern(&p, a, &occs[..1], "Skeleton");
        assert_eq!(n[0].file, "Skeleton-2");
        // The default: what insert gave them already.
        let n = names_from_pattern(&p, a, &occs, DEFAULT_NAMING);
        assert_eq!(
            n,
            member_names(&p, a, &occs)
                .into_iter()
                .map(|r| MemberName {
                    display: format!("{}:1", r.file),
                    ..r
                })
                .collect::<Vec<_>>()
        );
    }

    fn length_of_member(p: &Project, a: DocumentId, occ: u32) -> Real {
        let doc = asm_of(p, a).unwrap().find(occ).unwrap().component.document;
        let c = p
            .document(doc)
            .unwrap()
            .properties
            .custom
            .iter()
            .find(|c| c.name == LENGTH_PROPERTY)
            .unwrap();
        match c.value {
            PropValue::Number(l) => l,
            _ => panic!("the length is no number"),
        }
    }

    /// The length is stored clean: exactly 400, a mitred member its exact
    /// outer length, a noisy value saved earlier is cleaned, a save keeps it.
    #[test]
    fn a_member_length_has_no_mesh_noise() {
        let (mut p, aid, _) = project();
        let linked = Linked::new();
        let e = [
            edge("Front", "Top", [200.0, 0.0, 200.0]),
            edge("Right", "Top", [400.0, 150.0, 200.0]),
        ];
        let lib = Library::builtin();
        let (_, m) = insert(&mut p, &linked, aid, &e, &opts(), lib, None).unwrap();
        assert_eq!(length_of_member(&p, aid, m[0]), 400.0);
        add_miter(
            &mut p,
            &linked,
            aid,
            &[(m[0], m[1])],
            0.0,
            MiterOffset::Default,
        )
        .unwrap();
        assert_eq!(length_of_member(&p, aid, m[0]), 420.0);
        assert_eq!(length_of_member(&p, aid, m[1]), 320.0);
        // What an earlier build saved.
        let doc = asm_of(&p, aid)
            .unwrap()
            .find(m[0])
            .unwrap()
            .component
            .document;
        let c = p
            .document_mut(doc)
            .unwrap()
            .properties
            .custom
            .iter_mut()
            .find(|c| c.name == LENGTH_PROPERTY)
            .unwrap();
        c.value = PropValue::Number(419.99999998);
        solve(&mut p, &linked, aid).unwrap();
        assert_eq!(length_of_member(&p, aid, m[0]), 420.0);
        let dir = std::env::temp_dir().join(format!("cc3d_frame_gl_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("g.3dcadcraft");
        p.save(&file).unwrap();
        let q = Project::load(&file).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(length_of_member(&q, aid, m[0]), 420.0);
    }

    #[test]
    fn meeting_pairs_of_a_rectangle() {
        let (mut p, aid, _) = project();
        let e: Vec<Skeleton> = [
            [200.0, 0.0, 200.0],
            [400.0, 150.0, 200.0],
            [200.0, 300.0, 200.0],
            [0.0, 150.0, 200.0],
        ]
        .iter()
        .map(|m| find_edge(&p, aid, Point3::new(m[0], m[1], m[2])))
        .collect();
        let (_, occs) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &e,
            &opts(),
            Library::builtin(),
            None,
        )
        .unwrap();
        let pairs = meeting_pairs(&p, &Linked::new(), asm(&p, aid), &occs);
        assert_eq!(pairs.len(), 4, "{pairs:?}");
    }

    /// The skeleton edge through `at` (its two face names and the point).
    /// A demo frame for the GUI checks: the box's 4 top edges and 4
    /// uprights as SHS members, saved to `CADCRAFT_FRAME_DEMO` (run with
    /// `--ignored`).
    #[test]
    #[ignore]
    fn write_demo_frame() {
        let Ok(path) = std::env::var("CADCRAFT_FRAME_DEMO") else {
            return;
        };
        let (mut p, aid, _) = project();
        let at = [
            (200.0, 0.0, 200.0),
            (400.0, 150.0, 200.0),
            (200.0, 300.0, 200.0),
            (0.0, 150.0, 200.0),
            (0.0, 0.0, 100.0),
            (400.0, 0.0, 100.0),
            (400.0, 300.0, 100.0),
            (0.0, 300.0, 100.0),
        ];
        let edges: Vec<Skeleton> = at
            .iter()
            .map(|&(x, y, z)| find_edge(&p, aid, Point3::new(x, y, z)))
            .collect();
        insert(
            &mut p,
            &Linked::new(),
            aid,
            &edges,
            &opts(),
            Library::builtin(),
            None,
        )
        .unwrap();
        p.save(std::path::Path::new(&path)).unwrap();
    }

    /// A 600 × 400 rectangle of sketch lines with a cross bar at x = 300:
    /// Butt longer through at the corners, the bar butted against the rails
    /// at both ends (T joints), for every Level.
    #[test]
    fn rectangle_with_a_cross_bar() {
        use crate::part::{OriginPlane, SketchOptions, SketchPlacement};
        use crate::sketch::Point2;
        for level in [Level::Auto, Level::Top, Level::Bottom] {
            let mut sk = PartDocument::default();
            let o = SketchOptions {
                auto_project_origin: false,
                auto_reference_edges: false,
                ..Default::default()
            };
            let i = sk
                .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
                .unwrap();
            let s = &mut sk.sketches[i].sketch;
            let q = |x, y| Point2::new(x, y);
            let lines: Vec<_> = [
                (q(0.0, 0.0), q(600.0, 0.0)),
                (q(600.0, 0.0), q(600.0, 400.0)),
                (q(600.0, 400.0), q(0.0, 400.0)),
                (q(0.0, 400.0), q(0.0, 0.0)),
                (q(300.0, 0.0), q(300.0, 400.0)),
            ]
            .into_iter()
            .map(|(a, b)| s.add_line(a, b))
            .collect();
            let sid = sk.sketches[i].id.clone();
            let mut p = Project::new("Rect");
            let skd = p.add_document("Skeleton", DocumentData::Part(sk));
            let mut a = AssemblyDocument::default();
            a.place(
                DocRef::local(skd),
                "Skeleton:1",
                Transform3::identity(),
                true,
            );
            let aid = p.add_document("Frame", DocumentData::Assembly(a));
            let edges: Vec<Skeleton> = lines
                .iter()
                .map(|&line| Skeleton::SketchLine {
                    occurrence: 1,
                    sub: Vec::new(),
                    sketch: sid.clone(),
                    line,
                })
                .collect();
            let mut o = opts();
            o.corners = Corners {
                kind: CornerType::ButtLonger,
                gap: 0.0,
                tees: TeeJoint::Butt,
            };
            o.layout.level = level;
            let r = insert(
                &mut p,
                &Linked::new(),
                aid,
                &edges,
                &o,
                Library::builtin(),
                None,
            );
            let (_, occs) = r.unwrap_or_else(|e| panic!("{level:?}: {e}"));
            let ts = &asm(&p, aid).frames[0].treatments;
            assert_eq!(ts.len(), 6, "{level:?}: {ts:?}");
            // The bar: 400 between the rails' axes, less half a rail at each end.
            let bar = outer_length(&part(&p, asm(&p, aid), occs[4])).unwrap();
            assert!((bar - 360.0).abs() < 0.2, "{level:?}: {bar}");
        }
    }

    /// Timings on a saved assembly (`CADCRAFT_FRAME_PROBE` = its path, the
    /// skeleton the first part occurrence): resolving each member's line,
    /// and Insert Frame on all the skeleton body's straight edges.
    #[test]
    #[ignore]
    fn probe_frame_file() {
        let Ok(path) = std::env::var("CADCRAFT_FRAME_PROBE") else {
            return;
        };
        let path = std::path::Path::new(&path);
        let mut p = Project::load(path).unwrap();
        let mut linked = Linked::new();
        let dir = path.parent().unwrap();
        for l in p.libraries().to_vec() {
            if let Ok(mut q) = Project::load(&dir.join(&l.path)) {
                let ids: Vec<_> = q.documents().iter().map(|d| d.id).collect();
                for id in ids {
                    if let Some(DocumentData::Part(x)) = q.document_mut(id).map(|d| &mut d.data) {
                        let _ = x.regenerate();
                    }
                }
                linked.insert(l.id, std::rc::Rc::new(q));
            }
        }
        let aid = p
            .documents()
            .iter()
            .find(|d| matches!(&d.data, DocumentData::Assembly(a) if !a.frames.is_empty()))
            .unwrap()
            .id;
        let a = asm(&p, aid).clone();
        for t in &a.frames[0].treatments {
            eprintln!("PROBE treatment {t:?}");
        }
        for m in &a.frames[0].members {
            eprintln!(
                "PROBE member {} orient {:?} built {:?}",
                m.occurrence,
                m.orientation,
                m.built
                    .as_ref()
                    .map(|b| (b.length, b.ext, b.cuts.len(), b.notches.len()))
            );
        }
        for o in a.occurrences.iter().take(0) {
            let Some(part) = super::super::tree::part_of(&p, &linked, o.component) else {
                continue;
            };
            let t0 = std::time::Instant::now();
            let _ = part.body_signature();
            let t1 = t0.elapsed().as_secs_f64() * 1e3;
            let body = &part.body;
            let opts = crate::kernel::TessellationOptions::default();
            let mut ne = 0;
            let mut t_faces = 0.0;
            let mut t_sample = 0.0;
            for (e, _) in body.edges() {
                let t = std::time::Instant::now();
                let _ = crate::part::blend::edge_ref(body, e);
                t_faces += t.elapsed().as_secs_f64() * 1e3;
                let t = std::time::Instant::now();
                let _ = body.sample_edge(e, &opts);
                t_sample += t.elapsed().as_secs_f64() * 1e3;
                ne += 1;
            }
            eprintln!(
                "PROBE part {}: signature {t1:.1} ms, {ne} edges, edge_ref {t_faces:.1} ms, sample {t_sample:.1} ms, faces {}",
                o.name,
                body.faces().count()
            );
        }
        let t0 = std::time::Instant::now();
        let mut n = 0;
        for m in a.frames.iter().flat_map(|f| &f.members) {
            skeleton_line(&p, &linked, &a, &m.skeleton).unwrap();
            n += 1;
        }
        eprintln!(
            "PROBE {n} skeleton lines: {:.1} ms",
            t0.elapsed().as_secs_f64() * 1e3
        );
        if let Some(DocumentData::Assembly(x)) = p.document_mut(aid).map(|d| &mut d.data) {
            for m in x.frames.iter_mut().flat_map(|f| &mut f.members) {
                m.built = None;
            }
        }
        let t0 = std::time::Instant::now();
        let s = solve(&mut p, &linked, aid).unwrap();
        for o in asm(&p, aid).occurrences.iter() {
            if let Some(DocumentData::Part(x)) = p.document(o.component.document).map(|d| &d.data) {
                eprintln!(
                    "PROBE rebuilt {}: exact {} faces {}",
                    o.name,
                    x.exact,
                    x.body.faces().count()
                );
            }
        }
        eprintln!(
            "PROBE solve: {:.1} ms (moved {}, sick {:?})",
            t0.elapsed().as_secs_f64() * 1e3,
            s.moved,
            s.sick
        );
        {
            let mut q = p.clone();
            let fid = a.frames[0].id;
            let c = Corners {
                kind: CornerType::Miter,
                gap: 0.0,
                tees: TeeJoint::Butt,
            };
            let t0 = std::time::Instant::now();
            let r = edit_frame(&mut q, &linked, aid, fid, None, Some(c));
            eprintln!(
                "PROBE edit_frame corners: {:.1} ms {:?}",
                t0.elapsed().as_secs_f64() * 1e3,
                r.as_ref().err()
            );
            for t in &asm(&q, aid).frames[0].treatments {
                eprintln!("PROBE new treatment {t:?}");
            }
            for o in asm(&q, aid).occurrences.iter() {
                if let Some(DocumentData::Part(x)) =
                    q.document(o.component.document).map(|d| &d.data)
                    && !x.exact
                {
                    eprintln!("PROBE still faceted {}", o.name);
                }
            }
        }
        let skel = a
            .occurrences
            .iter()
            .find(|o| {
                !a.frames
                    .iter()
                    .any(|f| f.members.iter().any(|m| m.occurrence == o.id))
            })
            .unwrap();
        let part = super::super::tree::part_of(&p, &linked, skel.component)
            .unwrap()
            .clone();
        let t0 = std::time::Instant::now();
        let edges: Vec<Skeleton> = straight_edges(&part, None)
            .into_iter()
            .map(|edge| {
                Skeleton::Edge(GeomRef::Edge {
                    occurrence: skel.id,
                    sub: Vec::new(),
                    edge,
                })
            })
            .collect();
        eprintln!(
            "PROBE {} straight edges: {:.1} ms",
            edges.len(),
            t0.elapsed().as_secs_f64() * 1e3
        );
        for (kind, layout) in [
            (CornerType::None, Side::Center),
            (CornerType::Miter, Side::Inside),
        ] {
            let mut q = p.clone();
            let mut o = opts();
            o.corners.kind = kind;
            o.layout.side = layout;
            let t0 = std::time::Instant::now();
            let r = insert(&mut q, &linked, aid, &edges, &o, Library::builtin(), None);
            eprintln!(
                "PROBE insert {kind:?} {layout:?}: {:.1} ms {:?}",
                t0.elapsed().as_secs_f64() * 1e3,
                r.as_ref().err()
            );
        }
    }

    /// A sketch skeleton alone: a 600 × 400 rectangle with a cross bar at
    /// x = 300 (corners and T joints, GUI check), saved to
    /// `CADCRAFT_RECT_DEMO` (run with `--ignored`).
    #[test]
    #[ignore]
    fn write_rect_demo() {
        use crate::part::{OriginPlane, SketchOptions, SketchPlacement};
        use crate::sketch::Point2;
        let Ok(path) = std::env::var("CADCRAFT_RECT_DEMO") else {
            return;
        };
        let mut sk = PartDocument::default();
        let o = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let i = sk
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
            .unwrap();
        let s = &mut sk.sketches[i].sketch;
        let q = |x, y| Point2::new(x, y);
        for (a, b) in [
            (q(0.0, 0.0), q(600.0, 0.0)),
            (q(600.0, 0.0), q(600.0, 400.0)),
            (q(600.0, 400.0), q(0.0, 400.0)),
            (q(0.0, 400.0), q(0.0, 0.0)),
            (q(300.0, 0.0), q(300.0, 400.0)),
        ] {
            s.add_line(a, b);
        }
        let mut p = Project::new("Rect");
        let skd = p.add_document("Skeleton", DocumentData::Part(sk));
        let mut a = AssemblyDocument::default();
        a.place(
            DocRef::local(skd),
            "Skeleton:1",
            Transform3::identity(),
            true,
        );
        p.add_document("Frame", DocumentData::Assembly(a));
        p.save(std::path::Path::new(&path)).unwrap();
    }

    /// The 400 × 300 × 200 box skeleton alone (a frame from a solid, GUI
    /// check), saved to `CADCRAFT_BOX_DEMO` (run with `--ignored`).
    #[test]
    #[ignore]
    fn write_box_demo() {
        let Ok(path) = std::env::var("CADCRAFT_BOX_DEMO") else {
            return;
        };
        let (mut p, aid, _) = project();
        if let Ok(f) = std::env::var("CADCRAFT_BOX_SECTIONS") {
            // Two top edges with the sections named `family|size;family|size`.
            for (k, spec) in f.split(';').enumerate() {
                let (fam, size) = spec.split_once('|').unwrap();
                let at = [
                    Point3::new(200.0, 0.0, 200.0),
                    Point3::new(0.0, 150.0, 200.0),
                ][k];
                let e = find_edge(&p, aid, at);
                let mut o = opts();
                o.family = fam.into();
                o.size = size.into();
                insert(
                    &mut p,
                    &Linked::new(),
                    aid,
                    &[e],
                    &o,
                    Library::builtin(),
                    None,
                )
                .unwrap();
            }
        }
        if std::env::var_os("CADCRAFT_BOX_FRAMED").is_some() {
            let sk = part(&p, asm(&p, aid), 1);
            let edges: Vec<Skeleton> = straight_edges(&sk, None)
                .into_iter()
                .map(|edge| {
                    Skeleton::Edge(GeomRef::Edge {
                        occurrence: 1,
                        sub: Vec::new(),
                        edge,
                    })
                })
                .collect();
            let mut o = opts();
            o.layout.side = Side::Inside;
            o.corners.kind = CornerType::Miter;
            insert(
                &mut p,
                &Linked::new(),
                aid,
                &edges,
                &o,
                Library::builtin(),
                None,
            )
            .unwrap();
        }
        p.save(std::path::Path::new(&path)).unwrap();
    }

    /// A T of an SHS 40x40x3 upright and an RHS 100x50x4 across (Notch's GUI
    /// check), saved to `CADCRAFT_NOTCH_DEMO` (run with `--ignored`).
    #[test]
    #[ignore]
    fn write_notch_demo() {
        let Ok(path) = std::env::var("CADCRAFT_NOTCH_DEMO") else {
            return;
        };
        let (p, ..) = tee_frame_of(
            ("Square hollow section (EN 10219)", "40x40x3"),
            ("Rectangular hollow section (EN 10219)", "100x50x4"),
        );
        p.save(std::path::Path::new(&path)).unwrap();
    }

    /// The box skeleton plus a part `Profile` whose sketch is a D outline
    /// with a hole (Author Frame Profile's GUI check), saved to
    /// `CADCRAFT_PROFILE_DEMO` (run with `--ignored`).
    #[test]
    #[ignore]
    fn write_profile_demo() {
        let Ok(path) = std::env::var("CADCRAFT_PROFILE_DEMO") else {
            return;
        };
        let (mut p, _, _) = project();
        let mut part = PartDocument::default();
        let i = part
            .create_sketch(
                crate::part::SketchPlacement::on_origin_plane(crate::part::OriginPlane::XY),
                &crate::part::SketchOptions::default(),
            )
            .unwrap();
        let sk = &mut part.sketches[i].sketch;
        let a = sk.add_point(crate::sketch::Point2::new(0.0, 0.0));
        let b = sk.add_point(crate::sketch::Point2::new(60.0, 0.0));
        let c = sk.add_point(crate::sketch::Point2::new(30.0, 0.0));
        sk.add_line_between(a, b).unwrap();
        sk.add_arc(c, b, a).unwrap();
        sk.add_circle(crate::sketch::Point2::new(30.0, 12.0), 6.0)
            .unwrap();
        p.add_document("Profile", DocumentData::Part(part));
        p.save(std::path::Path::new(&path)).unwrap();
    }

    fn find_edge(p: &Project, aid: DocumentId, at: Point3) -> Skeleton {
        let a = asm(p, aid);
        let o = a.find(1).unwrap();
        let body = match p.document(o.component.document).map(|d| &d.data) {
            Some(DocumentData::Part(x)) => x.body.clone(),
            _ => panic!("no skeleton"),
        };
        for (id, _) in body.edges() {
            let pts = body
                .sample_edge(id, &crate::kernel::TessellationOptions::default())
                .unwrap();
            let (s, e) = (pts[0], pts[pts.len() - 1]);
            let d = e - s;
            let t = ((at - s).dot(&d) / d.norm_squared()).clamp(0.0, 1.0);
            if (s + d * t - at).norm() < 1e-6 {
                let faces = body.edge_faces(id);
                let name = |f| body.face(f).and_then(|f| f.name.clone()).unwrap();
                return Skeleton::Edge(GeomRef::Edge {
                    occurrence: 1,
                    sub: Vec::new(),
                    edge: EdgeRef {
                        faces: [name(faces[0]), name(faces[1])],
                        hint: [at.x, at.y, at.z],
                    },
                });
            }
        }
        panic!("no edge at {at}");
    }
}
