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

use crate::assembly::relation::GeomRef;
use crate::document::{AssemblyDocument, DocRef, DocumentData, DocumentId, Project};
use crate::kernel::{Point3, Real, Transform3, Vector3};
use crate::part::frame::{LENGTH_PROPERTY, Library, Section};
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

/// One member: what it was made from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameMember {
    /// Its occurrence in the assembly.
    pub occurrence: u32,
    /// The skeleton edge it follows.
    pub skeleton: GeomRef,
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
}

impl SectionData {
    fn of(s: &Section) -> Self {
        Self {
            shape: shape_name(s).into(),
            standard: s.standard.clone(),
            dims: [s.h, s.b, s.t, s.tw, s.tf, s.r1, s.r2],
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
        Library::parse(&line).0.sections.into_iter().next()
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
}

/// A straight skeleton edge in assembly coordinates: its start and end.
pub fn skeleton_line(
    project: &Project,
    asm: &AssemblyDocument,
    r: &GeomRef,
) -> Result<(Point3, Point3), String> {
    let GeomRef::Edge {
        occurrence,
        sub,
        edge,
    } = r
    else {
        return Err("a frame member follows an edge".into());
    };
    let linked = super::tree::Linked::new();
    let n = super::tree::nested(project, &linked, asm, *occurrence, sub)?;
    let part = super::tree::part_of(project, &linked, n.component)
        .ok_or_else(|| format!("{} shows no part", n.name))?;
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
    let (Some(a), Some(b)) = (pts.first(), pts.last()) else {
        return Err("the skeleton edge has no ends".into());
    };
    let top = asm
        .find(*occurrence)
        .ok_or("the component no longer exists")?;
    let m = top.transform * n.at;
    Ok((m.transform_point(a), m.transform_point(b)))
}

/// The occurrence placement of a member of section `s` along `a → b`:
/// part Z along the line, part X / Y the section's horizontal / vertical
/// axes (vertical = the assembly's Z projected square to the line, or its X
/// for a vertical line), the anchor on the line, then offsets and rotation.
pub fn placement(s: &Section, a: Point3, b: Point3, o: &Orientation) -> Option<Transform3> {
    let (a, b) = if o.flip { (b, a) } else { (a, b) };
    let d = (b - a).try_normalize(1e-9)?;
    let up = if d.z.abs() > 0.999 {
        Vector3::x()
    } else {
        Vector3::z()
    };
    let v = (up - d * d.dot(&up)).normalize();
    let u = v.cross(&d);
    // The section's axes turned about the line.
    let (c, sn) = (o.angle.cos(), o.angle.sin());
    let (ur, vr) = (u * c + v * sn, v * c - u * sn);
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
    asm: DocumentId,
    edges: &[GeomRef],
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
        for e in edges {
            let a = asm_of(project, asm)?;
            let (p, q) = skeleton_line(project, a, e)?;
            let length = (q - p).norm();
            let place = placement(&section, p, q, &o.orientation)
                .ok_or("the skeleton edge has no length")?;
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
                orientation: o.orientation,
            });
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
                });
                id
            }
        };
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

/// Follows the skeleton: every member of assembly `asm` gets the place and
/// length of its skeleton edge now. A member whose edge is gone keeps its
/// last state and is reported.
pub fn solve(project: &mut Project, asm: DocumentId) -> Result<Solved, String> {
    let a = asm_of(project, asm)?;
    let mut todo: Vec<(u32, Transform3, Real, DocumentId)> = Vec::new();
    let mut out = Solved::default();
    for m in a.frames.iter().flat_map(|f| &f.members) {
        let Some(occ) = a.find(m.occurrence) else {
            continue;
        };
        if occ.component.library.is_some() {
            continue;
        }
        let doc = occ.component.document;
        let line = skeleton_line(project, a, &m.skeleton);
        let sec = m.section();
        match (line, sec) {
            (Ok((p, q)), Some(s)) => match placement(&s, p, q, &m.orientation) {
                Some(t) => todo.push((m.occurrence, t, (q - p).norm(), doc)),
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
    for (occ, t, length, doc) in todo {
        let mut changed = false;
        if let Some(DocumentData::Part(part)) = project.document_mut(doc).map(|d| &mut d.data)
            && set_length(part, length)
        {
            changed = true;
            if let Some(d) = project.document_mut(doc) {
                for c in &mut d.properties.custom {
                    if c.name == LENGTH_PROPERTY {
                        c.value = crate::document::iprops::PropValue::Number(length);
                    }
                }
            }
        }
        let a = asm_mut(project, asm)?;
        if let Some(o) = a.occurrences.iter_mut().find(|o| o.id == occ) {
            let d = (o.transform.translation.vector - t.translation.vector).norm()
                + o.transform.rotation.angle_to(&t.rotation);
            if d > 1e-9 {
                o.transform = t;
                changed = true;
            }
        }
        if changed {
            out.moved += 1;
        }
    }
    Ok(out)
}

/// The member's length now (its extrusion's distance).
pub fn length_of(part: &PartDocument) -> Option<Real> {
    part.features.iter().find_map(|f| match f {
        Feature::Extrude(e) => Some(e.distance),
        _ => None,
    })
}

/// Sets the member part's length; true when it changed (the part is
/// regenerated; a failure keeps the old length — rule 5).
fn set_length(part: &mut PartDocument, length: Real) -> bool {
    let Some(old) = length_of(part) else {
        return false;
    };
    if (old - length).abs() < 1e-9 {
        return false;
    }
    let before = part.clone();
    for f in &mut part.features {
        if let Feature::Extrude(e) = f {
            e.distance = length;
            e.distance_b = length;
            break;
        }
    }
    if part.regenerate().is_err() || !part.feature_health.is_empty() {
        *part = before;
        return false;
    }
    true
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
    fn edge(f0: &str, f1: &str, hint: [Real; 3]) -> GeomRef {
        GeomRef::Edge {
            occurrence: 1,
            sub: Vec::new(),
            edge: EdgeRef {
                faces: [format!("Box1:{f0}"), format!("Box1:{f1}")],
                hint,
            },
        }
    }

    fn opts() -> InsertOptions {
        InsertOptions {
            family: "Square hollow section (EN 10219)".into(),
            size: "40x40x3".into(),
            material: Some("Steel S235".into()),
            orientation: Orientation::default(),
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
        let (fid, occs) = insert(&mut p, aid, &[e], &opts(), Library::builtin(), None).unwrap();
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
        let (_, occs) = insert(&mut p, aid, &[e], &opts(), Library::builtin(), None).unwrap();
        if let Some(DocumentData::Part(b)) = p.document_mut(sk).map(|d| &mut d.data) {
            b.box_feature_mut().unwrap().size[0] = 600.0;
            b.regenerate().unwrap();
        }
        let s = solve(&mut p, aid).unwrap();
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
        assert_eq!(solve(&mut p, aid).unwrap().moved, 0);
    }

    /// An edge that is gone leaves the member as it was and reports it.
    #[test]
    fn a_member_without_its_edge_keeps_its_shape() {
        let (mut p, aid, _) = project();
        let e = find_edge(&p, aid, Point3::new(200.0, 0.0, 0.0));
        let (_, occs) = insert(&mut p, aid, &[e], &opts(), Library::builtin(), None).unwrap();
        let before = asm(&p, aid).find(occs[0]).unwrap().transform;
        if let Ok(a) = asm_mut(&mut p, aid)
            && let GeomRef::Edge { edge, .. } = &mut a.frames[0].members[0].skeleton
        {
            edge.faces[0] = "Box1:Gone".into();
        }
        let s = solve(&mut p, aid).unwrap();
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
        assert!(insert(&mut p, aid, &[e, bad], &opts(), Library::builtin(), None).is_err());
        assert_eq!(p.documents().len(), n);
        assert!(asm(&p, aid).frames.is_empty());
        let mut o = opts();
        o.size = "1x1x1".into();
        let e = find_edge(&p, aid, Point3::new(200.0, 0.0, 0.0));
        assert!(insert(&mut p, aid, &[e], &o, Library::builtin(), None).is_err());
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
        let edges: Vec<GeomRef> = mids
            .iter()
            .map(|m| find_edge(&p, aid, Point3::new(m[0], m[1], m[2])))
            .collect();
        let (_, occs) = insert(&mut p, aid, &edges, &opts(), Library::builtin(), None).unwrap();
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

    /// The skeleton edge through `at` (its two face names and the point).
    fn find_edge(p: &Project, aid: DocumentId, at: Point3) -> GeomRef {
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
                return GeomRef::Edge {
                    occurrence: 1,
                    sub: Vec::new(),
                    edge: EdgeRef {
                        faces: [name(faces[0]), name(faces[1])],
                        hint: [at.x, at.y, at.z],
                    },
                };
            }
        }
        panic!("no edge at {at}");
    }
}
