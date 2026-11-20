//! Pattern Component (`docs/spec/3.0-assembly-commands.md` §3.7): repeats
//! components in rows and columns or about an axis. The copies are ordinary
//! occurrences (the BOM, interference and physical properties count them),
//! placed by the pattern: after every solve [`place`] moves each element
//! from its source's place, so moving a source moves its copies. The
//! solver never moves an element ([`AssemblyDocument::is_element`]).
//!
//! Element 1 is the sources themselves (as in Inventor's browser); elements
//! 2.. hold one occurrence per source, in the sources' order.
//!
//! At Placements (`docs/spec/3.7a-pattern-placements.md`, our extension):
//! each element is moved from a reference frame (plane, point, orientation
//! where the sources sit) onto a picked place's frame.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::relation::{ASSEMBLY, Geom, GeomRef};
use crate::document::AssemblyDocument;
use crate::kernel::{Point3, Real, Transform3, Vector3};

/// A component pattern of an assembly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComponentPattern {
    /// Never reused within the assembly.
    pub id: u32,
    /// Browser name (`Component Pattern 1`).
    pub name: String,
    /// The patterned occurrences (element 1).
    pub sources: Vec<u32>,
    /// How the elements are laid out.
    pub layout: Layout,
    /// Elements 2.. (index, one occurrence per source).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub elements: Vec<Element>,
}

/// One copy of the sources.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Element {
    /// Element number (2.. ; 1 is the sources).
    pub index: u32,
    /// Its occurrences, one per source in order (0 = deleted by the user).
    pub occurrences: Vec<u32>,
}

/// Rows and columns or a circle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Layout {
    /// Columns along one direction, rows along an optional second one.
    Rectangular {
        /// Column direction (edge or axis).
        column: GeomRef,
        /// Reverses the column direction.
        #[serde(default)]
        column_flip: bool,
        /// Columns (≥ 1).
        column_count: u32,
        /// Distance between columns (mm; negative = the other way).
        column_spacing: Real,
        /// Row direction (none: one row).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        row: Option<GeomRef>,
        /// Reverses the row direction.
        #[serde(default)]
        row_flip: bool,
        /// Rows (≥ 1).
        #[serde(default = "one")]
        row_count: u32,
        /// Distance between rows (mm).
        #[serde(default)]
        row_spacing: Real,
    },
    /// Follows a feature pattern of a component's part (Associative): one
    /// element per instance of the pattern, moved as that instance; a
    /// changed count in the part changes the elements.
    Feature {
        /// The occurrence whose part holds the pattern.
        occurrence: u32,
        /// The pattern feature's id (`CircularPattern1`).
        feature: String,
    },
    /// Copies turned about an axis.
    Circular {
        /// Rotation axis (straight edge, axis, round edge or cylinder).
        axis: GeomRef,
        /// Turns the other way.
        #[serde(default)]
        flip: bool,
        /// Elements including the sources (≥ 1).
        count: u32,
        /// Angle between neighbouring elements (radians).
        angle: Real,
    },
    /// Copies at picked places (our extension, spec 3.7a): element k + 2
    /// stands on `places[k]` as the sources stand on `reference`.
    Placements {
        /// Where the sources sit now.
        reference: Anchor,
        /// The new places, in the order picked.
        places: Vec<Anchor>,
    },
}

/// A place a pattern copies components to (spec 3.7a §2): a plane, a point
/// on it, an in-plane orientation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    /// The plane (none: a round edge's own plane, else the reference's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane: Option<GeomRef>,
    /// The point: a vertex, a round edge (its centre), an axis (where it
    /// crosses the plane), a work point.
    pub point: GeomRef,
    /// An in-plane direction (straight edge or axis).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orient: Option<GeomRef>,
}

impl Anchor {
    /// The references it reads.
    pub fn refs(&self) -> Vec<&GeomRef> {
        self.plane
            .iter()
            .chain(std::iter::once(&self.point))
            .chain(self.orient.iter())
            .collect()
    }

    /// [`Self::refs`], to change them (a reference moved into another
    /// assembly's terms).
    pub fn refs_mut(&mut self) -> Vec<&mut GeomRef> {
        self.plane
            .iter_mut()
            .chain(std::iter::once(&mut self.point))
            .chain(self.orient.iter_mut())
            .collect()
    }
}

/// A place's frame in assembly coordinates: origin, unit normal and the
/// in-plane x direction when an orientation was given.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaceFrame {
    /// Origin (the point on the plane).
    pub o: Point3,
    /// Unit normal.
    pub n: Vector3,
    /// Unit in-plane direction.
    pub x: Option<Vector3>,
}

/// The frame of `a` from its references' world geometry (`geom`); `fallback`
/// is the plane (point, normal) used when the anchor has none of its own.
pub fn frame_of(
    a: &Anchor,
    geom: &dyn Fn(&GeomRef) -> Result<Geom, String>,
    fallback: Option<(Point3, Vector3)>,
) -> Result<PlaceFrame, String> {
    let point = geom(&a.point)?;
    let plane = match &a.plane {
        Some(r) => match geom(r)? {
            Geom::Plane { p, n } => Some((p, n)),
            Geom::Frame { p, z, .. } => Some((p, z)),
            _ => return Err("the plane must be a planar face or a work plane".into()),
        },
        None => None,
    };
    // A round edge or a joint frame is a plane of its own.
    let own = match point {
        Geom::Circle { c, n, .. } => Some((c, n)),
        Geom::Frame { p, z, .. } => Some((p, z)),
        _ => None,
    };
    let (pp, n) = plane
        .or(own)
        .or(fallback)
        .ok_or("select a plane for this place")?;
    let n = n.try_normalize(1e-12).ok_or("the plane has no normal")?;
    let o = match point {
        Geom::Point { p } | Geom::Frame { p, .. } => p,
        Geom::Circle { c, .. } | Geom::Sphere { c, .. } => c,
        Geom::Axis { p, d } | Geom::Cylinder { p, d, .. } => {
            let dn = d.dot(&n);
            if dn.abs() < 1e-9 {
                return Err("the axis does not cross the plane".into());
            }
            p + d * ((pp - p).dot(&n) / dn)
        }
        Geom::Plane { .. } => {
            return Err("the point must be a vertex, a round edge or an axis".into());
        }
    };
    // Onto the plane.
    let o = o - n * (o - pp).dot(&n);
    let x = match &a.orient {
        Some(r) => {
            let d = match geom(r)? {
                Geom::Axis { d, .. } | Geom::Cylinder { d, .. } => d,
                Geom::Frame { x, .. } => x,
                _ => return Err("the orientation must be a straight edge or an axis".into()),
            };
            let d = d - n * d.dot(&n);
            Some(
                d.try_normalize(1e-9)
                    .ok_or("the orientation edge stands square to the plane")?,
            )
        }
        None => None,
    };
    Ok(PlaceFrame { o, n, x })
}

/// The rigid motion taking frame `from` onto frame `to`: with an orientation
/// on both, their axes line up; otherwise the shortest turn between the
/// normals (none on the same plane: a move only).
pub fn motion_between(from: &PlaceFrame, to: &PlaceFrame) -> Transform3 {
    use nalgebra::{Matrix3, Rotation3, Translation3, UnitQuaternion};
    let rot = match (from.x, to.x) {
        (Some(xa), Some(xb)) => {
            let basis = |x: Vector3, n: Vector3| {
                Rotation3::from_matrix_unchecked(Matrix3::from_columns(&[x, n.cross(&x), n]))
            };
            UnitQuaternion::from_rotation_matrix(&(basis(xb, to.n) * basis(xa, from.n).inverse()))
        }
        _ => UnitQuaternion::rotation_between(&from.n, &to.n).unwrap_or_else(|| {
            // Opposite normals: half a turn about a direction in the plane.
            let x = from.x.unwrap_or_else(|| {
                let t = if from.n.x.abs() < 0.9 {
                    Vector3::x()
                } else {
                    Vector3::y()
                };
                (t - from.n * t.dot(&from.n)).normalize()
            });
            UnitQuaternion::from_scaled_axis(x * std::f64::consts::PI)
        }),
    };
    let r = Transform3::from_parts(Translation3::identity(), rot);
    let a = from.o.coords;
    let b = to.o.coords;
    Transform3::translation(b.x, b.y, b.z) * r * Transform3::translation(-a.x, -a.y, -a.z)
}

/// What a pick of the Places selector is (spec 3.7a §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacePick {
    /// A round edge: a whole place.
    Rim,
    /// A planar face or work plane: the plane of the places after it.
    Plane,
    /// A vertex, work point or hole wall: a place on the current plane.
    Point,
    /// A straight edge: the last place's orientation.
    Edge,
}

impl PlacePick {
    /// What geometry `g` is in the Places selector.
    pub fn of(g: &Geom) -> Option<PlacePick> {
        Some(match g {
            Geom::Circle { .. } => PlacePick::Rim,
            Geom::Plane { .. } => PlacePick::Plane,
            Geom::Cylinder { .. }
            | Geom::Point { .. }
            | Geom::Frame { .. }
            | Geom::Sphere { .. } => PlacePick::Point,
            Geom::Axis { .. } => PlacePick::Edge,
        })
    }
}

/// The places a sequence of picks describes (spec 3.7a §2): a rim is a
/// place, a plane is the plane of the places after it, a point a place on
/// it, an edge the last place's orientation.
pub fn places_from_picks(picks: &[(GeomRef, PlacePick)]) -> Result<Vec<Anchor>, String> {
    let mut out: Vec<Anchor> = Vec::new();
    let mut plane: Option<GeomRef> = None;
    for (r, k) in picks {
        match k {
            PlacePick::Rim => out.push(Anchor {
                plane: None,
                point: r.clone(),
                orient: None,
            }),
            PlacePick::Plane => plane = Some(r.clone()),
            PlacePick::Point => out.push(Anchor {
                plane: plane.clone(),
                point: r.clone(),
                orient: None,
            }),
            PlacePick::Edge => match out.last_mut() {
                Some(a) => a.orient = Some(r.clone()),
                None => return Err("pick a place first; an edge turns the last place".into()),
            },
        }
    }
    Ok(out)
}

/// The picks that give `places` again (Edit): a plane only where it
/// changes; `kind` tells a point's pick kind from its geometry.
pub fn picks_of_places(
    places: &[Anchor],
    kind: &dyn Fn(&GeomRef) -> PlacePick,
) -> Vec<(GeomRef, PlacePick)> {
    let mut out = Vec::new();
    let mut plane: Option<&GeomRef> = None;
    for a in places {
        if let Some(p) = &a.plane
            && plane != Some(p)
        {
            out.push((p.clone(), PlacePick::Plane));
            plane = Some(p);
        }
        out.push((a.point.clone(), kind(&a.point)));
        if let Some(o) = &a.orient {
            out.push((o.clone(), PlacePick::Edge));
        }
    }
    out
}

fn one() -> u32 {
    1
}

impl Layout {
    /// Element count, the sources included.
    pub fn count(&self) -> u32 {
        match self {
            Layout::Rectangular {
                column_count,
                row_count,
                row,
                ..
            } => column_count.max(&1) * if row.is_some() { *row_count.max(&1) } else { 1 },
            Layout::Circular { count, .. } => *count.max(&1),
            Layout::Placements { places, .. } => places.len() as u32 + 1,
            // Known only from the part.
            Layout::Feature { .. } => 0,
        }
    }

    /// The references it reads.
    pub fn refs(&self) -> Vec<&GeomRef> {
        match self {
            Layout::Rectangular { column, row, .. } => {
                std::iter::once(column).chain(row.as_ref()).collect()
            }
            Layout::Circular { axis, .. } => vec![axis],
            Layout::Placements { reference, places } => reference
                .refs()
                .into_iter()
                .chain(places.iter().flat_map(Anchor::refs))
                .collect(),
            Layout::Feature { .. } => Vec::new(),
        }
    }

    /// [`Self::refs`], to change them (the pattern of a frame's members made
    /// from its parent: `assembly::frame_sub::geom_down`).
    pub fn refs_mut(&mut self) -> Vec<&mut GeomRef> {
        match self {
            Layout::Rectangular { column, row, .. } => {
                std::iter::once(column).chain(row.as_mut()).collect()
            }
            Layout::Circular { axis, .. } => vec![axis],
            Layout::Placements { reference, places } => reference
                .refs_mut()
                .into_iter()
                .chain(places.iter_mut().flat_map(Anchor::refs_mut))
                .collect(),
            Layout::Feature { .. } => Vec::new(),
        }
    }
}

/// Element indices (from 2) and motions of a feature pattern's instances.
pub type Instances = Result<Vec<(u32, Transform3)>, String>;

/// What a pattern reads from the components' parts.
pub struct Lookup<'a> {
    /// A reference's geometry in its part.
    pub local: &'a dyn Fn(&GeomRef) -> Result<Geom, String>,
    /// The instances (element index from 2, motion in part coordinates)
    /// of feature pattern `id` in the part of occurrence `occurrence`.
    pub feature: &'a dyn Fn(u32, &str) -> Instances,
}

fn no_features(_: u32, _: &str) -> Result<Vec<(u32, Transform3)>, String> {
    Err("feature patterns cannot be read here".into())
}

impl<'a> Lookup<'a> {
    /// Geometry only (no feature patterns).
    pub fn geometry(local: &'a dyn Fn(&GeomRef) -> Result<Geom, String>) -> Self {
        Self {
            local,
            feature: &no_features,
        }
    }
}

/// A reference in assembly coordinates: `local` gives it in its part, `at`
/// the occurrence's placement.
fn world(
    r: &GeomRef,
    local: &dyn Fn(&GeomRef) -> Result<Geom, String>,
    at: &dyn Fn(u32) -> Option<Transform3>,
) -> Result<Geom, String> {
    let g = local(r)?;
    if r.occurrence() == ASSEMBLY {
        return Ok(g);
    }
    let t = at(r.occurrence()).ok_or("the component of a direction no longer exists")?;
    Ok(g.moved(&t))
}

/// A direction's unit vector (a line, an axis, a round edge's axis).
fn direction(g: &Geom) -> Result<Vector3, String> {
    match *g {
        Geom::Axis { d, .. } | Geom::Cylinder { d, .. } => Ok(d),
        Geom::Circle { n, .. } => Ok(n),
        Geom::Plane { n, .. } => Ok(n),
        _ => Err("the direction must be an edge or an axis".into()),
    }
}

/// An axis: a point on it and its unit direction.
fn axis_of(g: &Geom) -> Result<(Point3, Vector3), String> {
    match *g {
        Geom::Axis { p, d } | Geom::Cylinder { p, d, .. } => Ok((p, d)),
        Geom::Circle { c, n, .. } => Ok((c, n)),
        _ => Err("the rotation axis must be a straight edge, an axis or a round edge".into()),
    }
}

/// Element motions (element index from 2, motion).
pub type Motions = Vec<(u32, Transform3)>;

/// The motions of a Placements layout, place by place: those found and the
/// errors of the others (`place k: ...`, k from 1). A reference that cannot be
/// found is an error for every place.
pub fn placement_motions(
    reference: &Anchor,
    places: &[Anchor],
    look: &Lookup,
    at: &dyn Fn(u32) -> Option<Transform3>,
) -> Result<(Motions, Vec<String>), String> {
    let geom = |r: &GeomRef| world(r, look.local, at);
    let from = frame_of(reference, &geom, None).map_err(|e| format!("reference: {e}"))?;
    let fallback = reference.plane.as_ref().map(|_| (from.o, from.n));
    let mut out = Vec::new();
    let mut errors = Vec::new();
    for (k, a) in places.iter().enumerate() {
        match frame_of(a, &geom, fallback) {
            Ok(to) => out.push((k as u32 + 2, motion_between(&from, &to))),
            Err(e) => errors.push(format!("place {}: {e}", k + 1)),
        }
    }
    Ok((out, errors))
}

/// The motion of each element 2.. (by element index), in assembly
/// coordinates, applied on top of a source's placement.
pub fn motions(
    layout: &Layout,
    look: &Lookup,
    at: &dyn Fn(u32) -> Option<Transform3>,
) -> Result<Vec<(u32, Transform3)>, String> {
    let local = look.local;
    let mut out = Vec::new();
    match layout {
        Layout::Placements { reference, places } => {
            let (moves, errors) = placement_motions(reference, places, look, at)?;
            if !errors.is_empty() {
                return Err(errors.join("; "));
            }
            out = moves;
        }
        Layout::Feature {
            occurrence,
            feature,
        } => {
            let t =
                at(*occurrence).ok_or("the component of the feature pattern no longer exists")?;
            let inv = t.inverse();
            for (k, m) in (look.feature)(*occurrence, feature)? {
                out.push((k, t * m * inv));
            }
        }
        Layout::Rectangular {
            column,
            column_flip,
            column_count,
            column_spacing,
            row,
            row_flip,
            row_count,
            row_spacing,
        } => {
            let sign = |f: bool| if f { -1.0 } else { 1.0 };
            let d1 = direction(&world(column, local, at)?)? * sign(*column_flip);
            let (d2, rows) = match row {
                Some(r) => (
                    direction(&world(r, local, at)?)? * sign(*row_flip),
                    (*row_count).max(1),
                ),
                None => (Vector3::zeros(), 1),
            };
            let cols = (*column_count).max(1);
            let mut index = 1;
            for j in 0..rows {
                for i in 0..cols {
                    if i == 0 && j == 0 {
                        continue;
                    }
                    index += 1;
                    let v = d1 * (i as Real * column_spacing) + d2 * (j as Real * row_spacing);
                    out.push((index, Transform3::translation(v.x, v.y, v.z)));
                }
            }
        }
        Layout::Circular {
            axis,
            flip,
            count,
            angle,
        } => {
            let (p, d) = axis_of(&world(axis, local, at)?)?;
            let d = if *flip { -d } else { d };
            let c = p.coords;
            for k in 1..(*count).max(1) {
                let r = Transform3::rotation(d * (k as Real * angle));
                let t = Transform3::translation(c.x, c.y, c.z)
                    * r
                    * Transform3::translation(-c.x, -c.y, -c.z);
                out.push((k + 1, t));
            }
        }
    }
    Ok(out)
}

/// The feature pattern a picked face of `part` belongs to: a copy's face
/// (`<pattern>#k:...`) or a face of a feature the pattern copies.
pub fn pattern_of_face(part: &crate::part::PartDocument, face: &str) -> Option<String> {
    use crate::part::Feature;
    let patterns = || {
        part.features
            .iter()
            .enumerate()
            .filter_map(|(k, f)| match f {
                Feature::Pattern(p) if !part.is_suppressed(k) => Some(p),
                _ => None,
            })
    };
    if let Some((id, _)) = face.split_once('#')
        && patterns().any(|p| p.id == id)
    {
        return Some(id.to_owned());
    }
    let owner = face.split(':').next()?;
    patterns()
        .find(|p| p.features.iter().any(|f| f == owner))
        .map(|p| p.id.clone())
}

/// The instances of feature pattern `id` of `part` (element index from 2,
/// rigid motion in part coordinates); a mirror cannot move a component.
pub fn feature_instances(
    part: &crate::part::PartDocument,
    id: &str,
) -> Result<Vec<(u32, Transform3)>, String> {
    use crate::part::Feature;
    if part
        .features
        .iter()
        .any(|f| matches!(f, Feature::Hole(h) if h.id == id))
    {
        return hole_instances(part, id);
    }
    let (k, p) = part
        .features
        .iter()
        .enumerate()
        .find_map(|(k, f)| match f {
            Feature::Pattern(p) if p.id == id => Some((k, p)),
            _ => None,
        })
        .ok_or_else(|| format!("the feature pattern {id} no longer exists"))?;
    if part.is_suppressed(k) {
        return Err(format!("{} is suppressed", p.name));
    }
    p.placements(&part.body, &part.work)?
        .into_iter()
        .map(|(k, m)| {
            nalgebra::try_convert::<_, Transform3>(m)
                .map(|t| (k as u32, t))
                .ok_or_else(|| format!("{} mirrors: a component cannot follow it", p.name))
        })
        .collect()
}

/// The holes of Hole feature `id` after its first (Smart Drop's Follow Pattern,
/// spec 6.3 §3.5): one element per hole, moved from the first hole to it. A part
/// whose holes are not built yet is an error, so no element is lost by it.
fn hole_instances(
    part: &crate::part::PartDocument,
    id: &str,
) -> Result<Vec<(u32, Transform3)>, String> {
    let holes: Vec<_> = part.holes().iter().filter(|h| h.feature == id).collect();
    let first = holes
        .first()
        .ok_or_else(|| format!("the holes of {id} are not built"))?;
    holes
        .iter()
        .skip(1)
        .enumerate()
        .map(|(k, h)| {
            if (h.direction - first.direction).norm() > 1e-6 {
                return Err(format!("{id}: its holes do not all point the same way"));
            }
            let d = h.origin - first.origin;
            Ok((k as u32 + 2, Transform3::translation(d.x, d.y, d.z)))
        })
        .collect()
}

impl AssemblyDocument {
    /// Whether occurrence `id` is a copy made by a pattern (placed by it,
    /// never by the solver or a drag).
    pub fn is_element(&self, id: u32) -> bool {
        self.patterns
            .iter()
            .any(|p| p.elements.iter().any(|e| e.occurrences.contains(&id)))
    }

    /// The pattern owning occurrence `id` (as a source or a copy).
    pub fn pattern_of(&self, id: u32) -> Option<&ComponentPattern> {
        self.patterns.iter().find(|p| {
            p.sources.contains(&id) || p.elements.iter().any(|e| e.occurrences.contains(&id))
        })
    }

    /// The next free pattern id.
    pub fn next_pattern_id(&self) -> u32 {
        self.patterns.iter().map(|p| p.id).max().unwrap_or(0) + 1
    }

    /// Adds a pattern of `sources` and its copies (placed at once); its id.
    pub fn add_pattern(
        &mut self,
        sources: Vec<u32>,
        layout: Layout,
        look: &Lookup,
    ) -> Result<u32, String> {
        if sources.is_empty() {
            return Err("select the components to pattern".into());
        }
        if let Some(&s) = sources.iter().find(|&&s| self.is_element(s)) {
            let name = self.find(s).map_or("?", |o| o.name.as_str());
            return Err(format!("{name} is already a copy of a pattern"));
        }
        let id = self.next_pattern_id();
        let n = self
            .patterns
            .iter()
            .filter_map(|p| {
                p.name
                    .strip_prefix("Component Pattern ")?
                    .parse::<u32>()
                    .ok()
            })
            .max()
            .unwrap_or(0)
            + 1;
        self.patterns.push(ComponentPattern {
            id,
            name: format!("Component Pattern {n}"),
            sources,
            layout,
            elements: Vec::new(),
        });
        if let Err(e) = place(self, look) {
            self.remove_pattern(id);
            return Err(e);
        }
        Ok(id)
    }

    /// Removes pattern `id` and its copies (the sources stay).
    pub fn remove_pattern(&mut self, id: u32) -> Option<ComponentPattern> {
        let i = self.patterns.iter().position(|p| p.id == id)?;
        let p = self.patterns.remove(i);
        for e in &p.elements {
            for &o in &e.occurrences {
                self.remove(o);
            }
        }
        Some(p)
    }

    /// Before occurrence `id` is deleted: a deleted source takes its
    /// pattern (and copies) along; a deleted copy leaves a gap that is not
    /// filled again.
    pub fn forget_in_patterns(&mut self, id: u32) {
        let gone: Vec<u32> = self
            .patterns
            .iter()
            .filter(|p| p.sources.contains(&id))
            .map(|p| p.id)
            .collect();
        for g in gone {
            self.remove_pattern(g);
        }
        for p in &mut self.patterns {
            for e in &mut p.elements {
                for o in &mut e.occurrences {
                    if *o == id {
                        *o = 0;
                    }
                }
            }
        }
    }
}

/// Makes every pattern's copies match its layout — adds missing elements,
/// removes extra ones — and places them from their sources. A pattern whose
/// references no longer resolve keeps its copies where they are; the
/// errors come back by pattern id.
pub fn place(asm: &mut AssemblyDocument, look: &Lookup) -> Result<(), String> {
    let errors: Vec<String> = place_report(asm, look)
        .into_iter()
        .map(|(id, e)| {
            let name = asm
                .patterns
                .iter()
                .find(|p| p.id == id)
                .map_or("Component Pattern", |p| p.name.as_str());
            format!("{name}: {e}")
        })
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// [`place`], with the problems by pattern id (a pattern's health: the
/// browser marks it sick with the reason).
pub fn place_report(asm: &mut AssemblyDocument, look: &Lookup) -> Vec<(u32, String)> {
    let mut errors: Vec<(u32, String)> = Vec::new();
    for k in 0..asm.patterns.len() {
        let p = asm.patterns[k].clone();
        let at = |id: u32| asm.find(id).map(|o| o.transform);
        // At Placements: a place whose geometry is gone keeps its copies
        // where they are; the others still follow (spec 3.7a §3 Health).
        let found = match &p.layout {
            Layout::Placements { reference, places } => {
                placement_motions(reference, places, look, &at).map(|(m, e)| {
                    if !e.is_empty() {
                        errors.push((p.id, e.join("; ")));
                    }
                    m
                })
            }
            layout => motions(layout, look, &at),
        };
        let moves = match found {
            Ok(m) => m,
            Err(e) => {
                errors.push((p.id, e));
                continue;
            }
        };
        let sources: Vec<_> = p
            .sources
            .iter()
            .filter_map(|&s| asm.find(s).cloned())
            .collect();
        // Extra elements (a count made smaller) go.
        let keep = match &p.layout {
            Layout::Placements { places, .. } => places.len() as u32 + 1,
            _ => moves.len() as u32 + 1,
        };
        let extra: Vec<u32> = p
            .elements
            .iter()
            .filter(|e| e.index > keep)
            .flat_map(|e| e.occurrences.clone())
            .collect();
        for o in extra {
            asm.remove(o);
        }
        let mut elements: Vec<Element> =
            p.elements.into_iter().filter(|e| e.index <= keep).collect();
        // Missing elements (new, or a count made larger) get occurrences.
        for &(index, m) in &moves {
            if elements.iter().any(|e| e.index == index) {
                continue;
            }
            let occurrences = sources
                .iter()
                .map(|s| {
                    let base = s.name.rsplit_once(':').map_or(s.name.as_str(), |b| b.0);
                    let id = asm.place(s.component, base, m * s.transform, false);
                    if let Some(o) = asm.find_mut(id) {
                        o.visible = s.visible;
                    }
                    id
                })
                .collect();
            elements.push(Element { index, occurrences });
        }
        // Every copy from its source (one lookup table: this runs after
        // every drag step).
        let at: HashMap<u32, usize> = asm
            .occurrences
            .iter()
            .enumerate()
            .map(|(i, o)| (o.id, i))
            .collect();
        let by_index: HashMap<u32, &Element> = elements.iter().map(|e| (e.index, e)).collect();
        for (index, m) in moves {
            let Some(e) = by_index.get(&index) else {
                continue;
            };
            for (s, o) in sources.iter().zip(&e.occurrences) {
                if let Some(o) = at.get(o).and_then(|&i| asm.occurrences.get_mut(i)) {
                    o.transform = m * s.transform;
                    o.component = s.component;
                }
            }
        }
        elements.sort_by_key(|e| e.index);
        asm.patterns[k].elements = elements;
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::relation::OriginItem;
    use crate::document::{DocRef, DocumentId};
    use std::sync::OnceLock;

    fn doc() -> DocRef {
        static ID: OnceLock<DocumentId> = OnceLock::new();
        DocRef {
            document: *ID.get_or_init(DocumentId::new_random),
            library: None,
            variant: None,
        }
    }

    /// Origin axes resolve to themselves (the assembly's own origin).
    fn local(r: &GeomRef) -> Result<Geom, String> {
        match r {
            GeomRef::Origin { item, .. } => Ok(item.geom()),
            _ => Err("not in this test".into()),
        }
    }

    fn axis(item: OriginItem) -> GeomRef {
        GeomRef::Origin {
            sub: Vec::new(),
            occurrence: ASSEMBLY,
            item,
        }
    }

    fn origin(a: &AssemblyDocument, id: u32) -> Point3 {
        a.find(id)
            .map_or(Point3::origin(), |o| o.transform * Point3::origin())
    }

    /// 3 columns × 2 rows: five copies at the grid points, numbered on;
    /// moving the source moves them; a smaller count removes copies.
    #[test]
    fn rectangular_grid_follows_its_source() {
        let mut a = AssemblyDocument::default();
        let s = a.place(doc(), "Bolt", Transform3::translation(5.0, 0.0, 0.0), false);
        let layout = Layout::Rectangular {
            column: axis(OriginItem::XAxis),
            column_flip: false,
            column_count: 3,
            column_spacing: 20.0,
            row: Some(axis(OriginItem::YAxis)),
            row_flip: true,
            row_count: 2,
            row_spacing: 10.0,
        };
        let pid = a
            .add_pattern(vec![s], layout, &Lookup::geometry(&local))
            .unwrap();
        assert_eq!(a.occurrences.len(), 6);
        let p = a.patterns[0].clone();
        assert_eq!(p.name, "Component Pattern 1");
        assert_eq!(p.elements.len(), 5);
        let at = |k: usize| origin(&a, p.elements[k].occurrences[0]);
        assert!((at(0) - Point3::new(25.0, 0.0, 0.0)).norm() < 1e-9);
        assert!((at(1) - Point3::new(45.0, 0.0, 0.0)).norm() < 1e-9);
        assert!((at(2) - Point3::new(5.0, -10.0, 0.0)).norm() < 1e-9);
        assert!((at(4) - Point3::new(45.0, -10.0, 0.0)).norm() < 1e-9);
        assert!(a.is_element(p.elements[0].occurrences[0]) && !a.is_element(s));
        assert_eq!(a.find(p.elements[0].occurrences[0]).unwrap().name, "Bolt:2");

        crate::assembly::move_by(a.find_mut(s).unwrap(), Vector3::new(0.0, 0.0, 7.0));
        place(&mut a, &Lookup::geometry(&local)).unwrap();
        assert!(
            (origin(&a, p.elements[4].occurrences[0]) - Point3::new(45.0, -10.0, 7.0)).norm()
                < 1e-9
        );

        if let Layout::Rectangular { row_count, .. } = &mut a.patterns[0].layout {
            *row_count = 1;
        }
        place(&mut a, &Lookup::geometry(&local)).unwrap();
        assert_eq!(a.occurrences.len(), 3);
        assert_eq!(a.patterns[0].elements.len(), 2);

        a.remove_pattern(pid);
        assert_eq!(a.occurrences.len(), 1);
    }

    /// 4 × 90° about Z: the copies turn about the axis, each element holds
    /// one copy of every source.
    #[test]
    fn circular_turns_every_source() {
        let mut a = AssemblyDocument::default();
        let s1 = a.place(doc(), "Pin", Transform3::translation(10.0, 0.0, 0.0), false);
        let s2 = a.place(doc(), "Nut", Transform3::translation(10.0, 0.0, 3.0), false);
        let layout = Layout::Circular {
            axis: axis(OriginItem::ZAxis),
            flip: false,
            count: 4,
            angle: std::f64::consts::FRAC_PI_2,
        };
        a.add_pattern(vec![s1, s2], layout, &Lookup::geometry(&local))
            .unwrap();
        let p = a.patterns[0].clone();
        assert_eq!(p.elements.len(), 3);
        assert!(p.elements.iter().all(|e| e.occurrences.len() == 2));
        let e2 = &p.elements[0];
        assert!((origin(&a, e2.occurrences[0]) - Point3::new(0.0, 10.0, 0.0)).norm() < 1e-9);
        assert!((origin(&a, e2.occurrences[1]) - Point3::new(0.0, 10.0, 3.0)).norm() < 1e-9);
        let e4 = &p.elements[2];
        assert!((origin(&a, e4.occurrences[0]) - Point3::new(0.0, -10.0, 0.0)).norm() < 1e-9);
        assert_eq!(a.find(e4.occurrences[1]).unwrap().name, "Nut:4");
    }

    /// Associative: one element per instance of the part's pattern, moved
    /// as that instance in assembly coordinates (the part turned 90° about
    /// Z); a pattern with fewer instances later drops elements.
    #[test]
    fn associative_follows_the_feature_pattern() {
        let mut a = AssemblyDocument::default();
        let plate = a.place(
            doc(),
            "Plate",
            Transform3::rotation(Vector3::z() * std::f64::consts::FRAC_PI_2),
            true,
        );
        let bolt = a.place(
            doc(),
            "Bolt",
            Transform3::translation(0.0, 10.0, 0.0),
            false,
        );
        let count = std::cell::Cell::new(4u32);
        let feature = |occ: u32, id: &str| -> Result<Vec<(u32, Transform3)>, String> {
            assert_eq!((occ, id), (plate, "RectangularPattern1"));
            // Holes every 20 mm along the part's X.
            Ok((2..=count.get())
                .map(|k| {
                    (
                        k,
                        Transform3::translation(20.0 * f64::from(k - 1), 0.0, 0.0),
                    )
                })
                .collect())
        };
        let look = Lookup {
            local: &local,
            feature: &feature,
        };
        let layout = Layout::Feature {
            occurrence: plate,
            feature: "RectangularPattern1".into(),
        };
        a.add_pattern(vec![bolt], layout, &look).unwrap();
        assert_eq!(a.patterns[0].elements.len(), 3);
        // The part's X is the assembly's Y.
        let e =
            |a: &AssemblyDocument, k: usize| origin(a, a.patterns[0].elements[k].occurrences[0]);
        assert!((e(&a, 0) - Point3::new(0.0, 30.0, 0.0)).norm() < 1e-9);
        assert!((e(&a, 2) - Point3::new(0.0, 70.0, 0.0)).norm() < 1e-9);
        count.set(2);
        place(&mut a, &look).unwrap();
        assert_eq!(a.patterns[0].elements.len(), 1);
        assert_eq!(a.occurrences.len(), 3);
    }

    /// Named geometry of the assembly itself (work features of occurrence
    /// 0), changeable between calls: the "plate" of the Placements tests.
    struct Plate(std::cell::RefCell<HashMap<String, Geom>>);

    impl Plate {
        fn new(items: &[(&str, Geom)]) -> Self {
            Self(std::cell::RefCell::new(
                items.iter().map(|(k, g)| ((*k).to_owned(), *g)).collect(),
            ))
        }
        fn local(&self, r: &GeomRef) -> Result<Geom, String> {
            match r {
                GeomRef::Work { id, .. } => self
                    .0
                    .borrow()
                    .get(id)
                    .copied()
                    .ok_or(format!("{id} is gone")),
                _ => local(r),
            }
        }
        fn set(&self, id: &str, g: Geom) {
            self.0.borrow_mut().insert(id.to_owned(), g);
        }
    }

    fn work(id: &str) -> GeomRef {
        GeomRef::Work {
            sub: Vec::new(),
            occurrence: ASSEMBLY,
            id: id.into(),
        }
    }

    fn rim(x: f64, y: f64, z: f64) -> Geom {
        Geom::Circle {
            c: Point3::new(x, y, z),
            n: Vector3::z(),
            r: 4.5,
        }
    }

    fn at(r: GeomRef) -> Anchor {
        Anchor {
            plane: None,
            point: r,
            orient: None,
        }
    }

    fn near(a: Point3, b: Point3) -> bool {
        (a - b).norm() < 1e-9
    }

    /// Holes on the reference plane: a bolt (head 5 mm up) and its nut (8 mm
    /// down) go to every rim, moved only; a moved hole takes its copies along
    /// (associative); a hole gone keeps its copies, the others still follow.
    #[test]
    fn placements_follow_hole_rims() {
        let plate = Plate::new(&[
            ("R0", rim(0.0, 0.0, 0.0)),
            ("R1", rim(30.0, 10.0, 0.0)),
            ("R2", rim(-20.0, 45.0, 0.0)),
        ]);
        let f = |r: &GeomRef| plate.local(r);
        let look = Lookup::geometry(&f);
        let mut a = AssemblyDocument::default();
        let bolt = a.place(doc(), "Bolt", Transform3::translation(0.0, 0.0, 5.0), false);
        let nut = a.place(doc(), "Nut", Transform3::translation(0.0, 0.0, -8.0), false);
        let layout = Layout::Placements {
            reference: at(work("R0")),
            places: vec![at(work("R1")), at(work("R2"))],
        };
        assert_eq!(layout.count(), 3);
        a.add_pattern(vec![bolt, nut], layout, &look).unwrap();
        assert_eq!(a.occurrences.len(), 6);
        let e = |a: &AssemblyDocument, k: usize, j: usize| {
            origin(a, a.patterns[0].elements[k].occurrences[j])
        };
        assert!(near(e(&a, 0, 0), Point3::new(30.0, 10.0, 5.0)));
        assert!(near(e(&a, 0, 1), Point3::new(30.0, 10.0, -8.0)));
        assert!(near(e(&a, 1, 0), Point3::new(-20.0, 45.0, 5.0)));
        // No turn on the same plane.
        let o = a.find(a.patterns[0].elements[1].occurrences[0]).unwrap();
        assert!(o.transform.rotation.angle() < 1e-12);

        // The plate changes: hole 1 moves.
        plate.set("R1", rim(35.0, 12.0, 0.0));
        place(&mut a, &look).unwrap();
        assert!(near(e(&a, 0, 0), Point3::new(35.0, 12.0, 5.0)));

        // Hole 2 deleted: its copies stay, hole 1 still follows.
        plate.0.borrow_mut().remove("R2");
        plate.set("R1", rim(36.0, 12.0, 0.0));
        let err = place(&mut a, &look).unwrap_err();
        assert!(err.contains("place 2"), "{err}");
        assert_eq!(a.occurrences.len(), 6);
        assert!(near(e(&a, 0, 0), Point3::new(36.0, 12.0, 5.0)));
        assert!(near(e(&a, 1, 0), Point3::new(-20.0, 45.0, 5.0)));
    }

    /// A reference plane and point; places as points on it (lifted ones
    /// projected), a place on another plane turned with its orientation.
    #[test]
    fn placements_on_planes_and_orientation() {
        let plate = Plate::new(&[
            (
                "Top",
                Geom::Plane {
                    p: Point3::origin(),
                    n: Vector3::z(),
                },
            ),
            (
                "P0",
                Geom::Point {
                    p: Point3::new(0.0, 0.0, 0.0),
                },
            ),
            (
                "P1",
                Geom::Point {
                    p: Point3::new(10.0, 0.0, 3.0),
                },
            ),
            (
                "X",
                Geom::Axis {
                    p: Point3::origin(),
                    d: Vector3::x(),
                },
            ),
            (
                "Side",
                Geom::Plane {
                    p: Point3::new(50.0, 0.0, 0.0),
                    n: Vector3::x(),
                },
            ),
            (
                "P2",
                Geom::Point {
                    p: Point3::new(50.0, 5.0, 5.0),
                },
            ),
            (
                "Y",
                Geom::Axis {
                    p: Point3::new(50.0, 0.0, 0.0),
                    d: Vector3::y(),
                },
            ),
        ]);
        let f = |r: &GeomRef| plate.local(r);
        let look = Lookup::geometry(&f);
        let mut a = AssemblyDocument::default();
        let s = a.place(doc(), "Bolt", Transform3::translation(1.0, 0.0, 5.0), false);
        let picks = vec![
            (work("P1"), PlacePick::Point),
            (work("Side"), PlacePick::Plane),
            (work("P2"), PlacePick::Point),
            (work("Y"), PlacePick::Edge),
        ];
        let places = places_from_picks(&picks).unwrap();
        assert_eq!(places.len(), 2);
        assert_eq!(places[0].plane, None);
        assert_eq!(places[1].plane, Some(work("Side")));
        assert_eq!(places[1].orient, Some(work("Y")));
        // Edit gives the same picks back.
        assert_eq!(picks_of_places(&places, &|_| PlacePick::Point), picks);
        let layout = Layout::Placements {
            reference: Anchor {
                plane: Some(work("Top")),
                point: work("P0"),
                orient: Some(work("X")),
            },
            places,
        };
        a.add_pattern(vec![s], layout, &look).unwrap();
        let e = |k: usize| origin(&a, a.patterns[0].elements[k].occurrences[0]);
        assert!(near(e(0), Point3::new(11.0, 0.0, 5.0)), "{}", e(0));
        // X → Y, Z → X: (1, 0, 5) from the reference is (5, 1, 0) from P2.
        assert!(near(e(1), Point3::new(55.0, 6.0, 5.0)), "{}", e(1));
        // An edge before any place is refused.
        assert!(places_from_picks(&[(work("Y"), PlacePick::Edge)]).is_err());
    }

    /// A place that cannot be found refuses the new pattern and leaves the
    /// assembly as it was; the pattern survives a save.
    #[test]
    fn placements_failure_and_round_trip() {
        let plate = Plate::new(&[("R0", rim(0.0, 0.0, 0.0)), ("R1", rim(9.0, 0.0, 0.0))]);
        let f = |r: &GeomRef| plate.local(r);
        let look = Lookup::geometry(&f);
        let mut a = AssemblyDocument::default();
        let s = a.place(doc(), "Bolt", Transform3::identity(), false);
        let before = a.clone();
        let bad = Layout::Placements {
            reference: at(work("R0")),
            places: vec![at(work("R1")), at(work("Gone"))],
        };
        assert!(a.add_pattern(vec![s], bad, &look).is_err());
        let ids = |a: &AssemblyDocument| -> Vec<(u32, Transform3)> {
            a.occurrences.iter().map(|o| (o.id, o.transform)).collect()
        };
        assert_eq!(ids(&a), ids(&before));
        assert_eq!(a.patterns, before.patterns);
        // Opposite normals: half a turn, the point lands on the rim.
        plate.set(
            "Under",
            Geom::Circle {
                c: Point3::new(0.0, 0.0, -10.0),
                n: -Vector3::z(),
                r: 4.5,
            },
        );
        let good = Layout::Placements {
            reference: at(work("R0")),
            places: vec![at(work("R1")), at(work("Under"))],
        };
        a.add_pattern(vec![s], good, &look).unwrap();
        let under = a.find(a.patterns[0].elements[1].occurrences[0]).unwrap();
        assert!(near(
            under.transform * Point3::origin(),
            Point3::new(0.0, 0.0, -10.0)
        ));
        assert!((under.transform * Vector3::z() + Vector3::z()).norm() < 1e-9);
        let text = serde_json::to_string(&a).unwrap();
        let back: AssemblyDocument = serde_json::from_str(&text).unwrap();
        assert_eq!(back.patterns, a.patterns);
    }

    /// A copy deleted leaves a gap; a source deleted takes the pattern;
    /// a copy cannot be patterned again; the pattern survives a round trip.
    #[test]
    fn deleting_and_saving() {
        let mut a = AssemblyDocument::default();
        let s = a.place(doc(), "Bolt", Transform3::identity(), false);
        let layout = Layout::Rectangular {
            column: axis(OriginItem::XAxis),
            column_flip: false,
            column_count: 3,
            column_spacing: 20.0,
            row: None,
            row_flip: false,
            row_count: 1,
            row_spacing: 0.0,
        };
        a.add_pattern(vec![s], layout.clone(), &Lookup::geometry(&local))
            .unwrap();
        let copy = a.patterns[0].elements[0].occurrences[0];
        assert!(
            a.add_pattern(vec![copy], layout.clone(), &Lookup::geometry(&local))
                .is_err()
        );
        a.forget_in_patterns(copy);
        a.remove(copy);
        place(&mut a, &Lookup::geometry(&local)).unwrap();
        assert_eq!(a.occurrences.len(), 2, "the gap is not filled again");

        let text = serde_json::to_string(&a).unwrap();
        let back: AssemblyDocument = serde_json::from_str(&text).unwrap();
        assert_eq!(back.patterns, a.patterns);

        a.forget_in_patterns(s);
        a.remove(s);
        assert!(a.patterns.is_empty());
        assert!(a.occurrences.is_empty());
    }
}
