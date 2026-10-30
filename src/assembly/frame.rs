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
use crate::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Project};
use crate::kernel::{Point3, Real, Transform3, Vector3};
use crate::part::frame::{EndCut, LENGTH_PROPERTY, Library, NotchTool, Section};
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
}

impl Skeleton {
    /// The occurrence it lies on.
    pub fn occurrence(&self) -> u32 {
        match self {
            Skeleton::Edge(g) => g.occurrence(),
            Skeleton::SketchLine { occurrence, .. } => *occurrence,
        }
    }

    /// The path below the occurrence to the part.
    pub fn sub(&self) -> &[u32] {
        match self {
            Skeleton::Edge(g) => g.sub(),
            Skeleton::SketchLine { sub, .. } => sub,
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
    /// Full Miter Cut where `a` and `b` meet: both cut on the bisector
    /// plane, `gap` between the cut faces.
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
            loops: s.custom.clone(),
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
        Custom => "custom",
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
    /// A new member ending on the middle of another is notched by it.
    pub tees: bool,
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
                return Err("only straight edges take frame members yet".into());
            }
            let pts = body
                .sample_edge(id, &crate::kernel::TessellationOptions::default())
                .map_err(|x| x.to_string())?;
            match (pts.first(), pts.last()) {
                (Some(a), Some(b)) => (*a, *b),
                _ => return Err("the skeleton edge has no ends".into()),
            }
        }
        Skeleton::Edge(_) => return Err("a frame member follows an edge or a sketch line".into()),
        Skeleton::SketchLine { sketch, line, .. } => {
            let ps = part
                .sketches
                .iter()
                .find(|s| &s.id == sketch)
                .ok_or_else(|| format!("{}: its skeleton sketch no longer exists", n.name))?;
            let sk = &ps.sketch;
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

/// A member's run direction and its section's axes (horizontal, vertical;
/// turned by the rotation angle) on line `a`–`b`.
fn section_axes(a: Point3, b: Point3, o: &Orientation) -> Option<(Vector3, Vector3, Vector3)> {
    let (a, b) = if o.flip { (b, a) } else { (a, b) };
    let d = (b - a).try_normalize(1e-9)?;
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
        let lines = {
            let a = asm_of(project, asm)?;
            edges
                .iter()
                .map(|e| skeleton_line(project, linked, a, e))
                .collect::<Result<Vec<_>, String>>()?
        };
        let centre = centre_of(&lines);
        for (e, &(p, q)) in edges.iter().zip(&lines) {
            let length = (q - p).norm();
            let orientation = o.layout.orient(&o.orientation, p, q, centre);
            let place =
                placement(&section, p, q, &orientation).ok_or("the skeleton edge has no length")?;
            let part = section
                .member_part(length, o.material.as_deref())
                .map_err(|e| e.to_string())?;
            let name = project.free_name(&format!("{}-", section.stock_number()));
            let doc = project.add_document(name.clone(), DocumentData::Part(part));
            if let Some(d) = project.document_mut(doc) {
                d.properties = section.member_properties(length);
            }
            let a = asm_mut(project, asm)?;
            let occ = a.place(DocRef::local(doc), &name, place, true);
            made.push(FrameMember {
                occurrence: occ,
                skeleton: e.clone(),
                family: section.family.clone(),
                size: section.size.clone(),
                section: SectionData::of(&section),
                orientation,
                built: Some(Shape {
                    length,
                    ..Default::default()
                }),
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
    let Treatment::Miter { a, b, gap, offset } = t else {
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
    let (ga, gb) = match offset {
        MiterOffset::Default => (*gap, 0.0),
        MiterOffset::Flipped => (0.0, *gap),
        MiterOffset::Symmetric => (gap / 2.0, gap / 2.0),
    };
    // A keeps the side n points to, B the other one.
    Ok([(ia, ea, c + n * ga, -n), (ib, eb, c - n * gb, n)])
}

/// The shape a live member's cuts give it (part coordinates), or why not.
fn shape_of(m: &Live) -> Result<Shape, String> {
    let length = (m.end - m.start).norm();
    let inv = m.place.inverse();
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
            let sec = m.section();
            match (skeleton_line(project, linked, a, &m.skeleton), sec) {
                (Ok((p, q)), Some(s)) => match placement(&s, p, q, &m.orientation) {
                    Some(t) => {
                        let (start, end) = if m.orientation.flip { (q, p) } else { (p, q) };
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
                            notches: Vec::new(),
                            built: m.built.clone(),
                            material,
                        });
                    }
                    None => out
                        .sick
                        .push((m.occurrence, "its skeleton edge has no length".into())),
                },
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
        for t in &treatments {
            match treatment_cuts(t, &live, &face) {
                Ok(cuts) => {
                    for (i, end, p, n) in cuts {
                        live[i].cuts[end] = Some((p, n));
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
            match shape_of(m) {
                Ok(sh) => {
                    let old = m.built.clone().unwrap_or_else(|| Shape {
                        length: part_length(project, m.doc).unwrap_or(0.0),
                        ..Default::default()
                    });
                    if !old.same(&sh) {
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
                        let r = sec
                            .member_part_cut(
                                sh.length,
                                sh.ext,
                                &sh.end_cuts(),
                                &sh.notches,
                                mat.as_deref(),
                            )
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
                let outer = outer_length(&part).unwrap_or(sh.length);
                if let Some(d) = project.document_mut(doc) {
                    d.data = DocumentData::Part(part);
                    for c in &mut d.properties.custom {
                        if c.name == LENGTH_PROPERTY {
                            c.value = crate::document::iprops::PropValue::Number(outer);
                        }
                    }
                }
                changed.insert(occ);
            }
            Err(e) => {
                failed.insert(occ);
                out.sick.push((occ, e));
            }
        }
    }
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
            let f = a
                .frames
                .iter_mut()
                .find(|f| {
                    f.members.iter().any(|m| m.occurrence == x)
                        && f.members.iter().any(|m| m.occurrence == y)
                })
                .ok_or("both members must belong to one frame")?;
            f.treatments.retain(|t| {
                let m = t.members();
                !(matches!(t, Treatment::Miter { .. }) && m.contains(&x) && m.contains(&y))
            });
            f.treatments.push(Treatment::Miter {
                a: x,
                b: y,
                gap,
                offset,
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
    if ts.is_empty() {
        return Err("select the frame members".into());
    }
    let before = project.clone();
    let result = (|| {
        let a = asm_mut(project, asm)?;
        let mut touched = Vec::new();
        for t in ts {
            let ms = t.members();
            let f = a
                .frames
                .iter_mut()
                .find(|f| {
                    ms.iter()
                        .all(|o| f.members.iter().any(|m| m.occurrence == *o))
                })
                .ok_or("the members must belong to one frame")?;
            f.treatments.retain(|x| {
                !(std::mem::discriminant(x) == std::mem::discriminant(&t) && x.members() == ms)
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
    if edits.is_empty() {
        return Err("select the frame members".into());
    }
    let section = lib
        .find(&o.family, &o.size)
        .ok_or_else(|| format!("{} {} is not in the library", o.family, o.size))?
        .clone();
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
            let doc = a
                .find(occ)
                .map(|x| x.component)
                .filter(|c| c.library.is_none())
                .ok_or("the member's part is not in this file")?
                .document;
            let skeleton = e.skeleton.clone().unwrap_or_else(|| m.skeleton.clone());
            let (p, q) = skeleton_line(project, linked, a, &skeleton)?;
            let orientation = if o.layout == Layout::default() {
                e.orientation
            } else {
                let lines: Vec<(Point3, Point3)> = f
                    .members
                    .iter()
                    .map(|x| {
                        if x.occurrence == occ {
                            Ok((p, q))
                        } else {
                            skeleton_line(project, linked, a, &x.skeleton)
                        }
                    })
                    .collect::<Result<_, _>>()?;
                o.layout.orient(&e.orientation, p, q, centre_of(&lines))
            };
            let length = (q - p).norm();
            let part = section
                .member_part(length, o.material.as_deref())
                .map_err(|e| e.to_string())?;
            if let Some(d) = project.document_mut(doc) {
                d.data = DocumentData::Part(part);
                d.properties = section.member_properties(length);
            }
            let a = asm_mut(project, asm)?;
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
                    ..Default::default()
                });
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
            tees: false,
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
        for &occ in members {
            if !a
                .frames
                .iter()
                .any(|f| f.members.iter().any(|m| m.occurrence == occ))
            {
                return Err("only frame members can be deleted here".to_owned());
            }
            for f in &mut a.frames {
                f.members.retain(|m| m.occurrence != occ);
                f.treatments.retain(|t| !t.members().contains(&occ));
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
                let reach = 2.0 * (lines[i].2 + lines[j].2);
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
                if k != through {
                    out.push(Treatment::CornerJoint {
                        a: lines[through].0,
                        b: lines[k].0,
                        offsets: [0.0, corners.gap],
                    });
                }
            }
        }
    }
    if corners.tees {
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
                    out.push(Treatment::Notch {
                        notched: *oi,
                        by: *oj,
                        offset: corners.gap,
                    });
                }
            }
        }
    }
    out
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
            let lines: Vec<(Point3, Point3)> = f
                .members
                .iter()
                .map(|m| skeleton_line(project, linked, a, &m.skeleton))
                .collect::<Result<_, _>>()?;
            let centre = centre_of(&lines);
            for (m, &(p, q)) in f.members.iter().zip(&lines) {
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
    let body = &part.body;
    let opts = crate::kernel::TessellationOptions::default();
    let mut out: Vec<crate::part::EdgeRef> = Vec::new();
    for (id, _) in body.edges() {
        let faces = body.edge_faces(id);
        let [f0, f1] = faces[..] else {
            continue;
        };
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
            .any(|r| (Point3::new(r.hint[0], r.hint[1], r.hint[2]) - m).norm() < 1e-6)
        {
            continue;
        }
        out.push(crate::part::EdgeRef {
            faces: [n0, n1],
            hint: [m.x, m.y, m.z],
        });
    }
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
                tees: false,
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
            tees: false,
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
            tees: true,
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

    /// The 400 × 300 × 200 box skeleton alone (a frame from a solid, GUI
    /// check), saved to `CADCRAFT_BOX_DEMO` (run with `--ignored`).
    #[test]
    #[ignore]
    fn write_box_demo() {
        let Ok(path) = std::env::var("CADCRAFT_BOX_DEMO") else {
            return;
        };
        let (mut p, aid, _) = project();
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
