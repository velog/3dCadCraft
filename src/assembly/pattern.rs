//! Pattern Component (`docs/spec/3.0-assembly-commands.md` §3.7): repeats
//! components in rows and columns or about an axis. The copies are ordinary
//! occurrences (the BOM, interference and physical properties count them),
//! placed by the pattern: after every solve [`place`] moves each element
//! from its source's place, so moving a source moves its copies. The
//! solver never moves an element ([`AssemblyDocument::is_element`]).
//!
//! Element 1 is the sources themselves (as in Inventor's browser); elements
//! 2.. hold one occurrence per source, in the sources' order.

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
        }
    }

    /// The references it reads.
    pub fn refs(&self) -> Vec<&GeomRef> {
        match self {
            Layout::Rectangular { column, row, .. } => {
                std::iter::once(column).chain(row.as_ref()).collect()
            }
            Layout::Circular { axis, .. } => vec![axis],
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

/// The motion of each element 2.. (by element index), in assembly
/// coordinates, applied on top of a source's placement.
pub fn motions(
    layout: &Layout,
    local: &dyn Fn(&GeomRef) -> Result<Geom, String>,
    at: &dyn Fn(u32) -> Option<Transform3>,
) -> Result<Vec<(u32, Transform3)>, String> {
    let mut out = Vec::new();
    match layout {
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
        local: &dyn Fn(&GeomRef) -> Result<Geom, String>,
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
        if let Err(e) = place(self, local) {
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
pub fn place(
    asm: &mut AssemblyDocument,
    local: &dyn Fn(&GeomRef) -> Result<Geom, String>,
) -> Result<(), String> {
    let mut errors = Vec::new();
    for k in 0..asm.patterns.len() {
        let p = asm.patterns[k].clone();
        let at = |id: u32| asm.find(id).map(|o| o.transform);
        let moves = match motions(&p.layout, local, &at) {
            Ok(m) => m,
            Err(e) => {
                errors.push(format!("{}: {e}", p.name));
                continue;
            }
        };
        let sources: Vec<_> = p
            .sources
            .iter()
            .filter_map(|&s| asm.find(s).cloned())
            .collect();
        // Extra elements (a count made smaller) go.
        let keep = moves.len() as u32 + 1;
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
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
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
        let pid = a.add_pattern(vec![s], layout, &local).unwrap();
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
        place(&mut a, &local).unwrap();
        assert!(
            (origin(&a, p.elements[4].occurrences[0]) - Point3::new(45.0, -10.0, 7.0)).norm()
                < 1e-9
        );

        if let Layout::Rectangular { row_count, .. } = &mut a.patterns[0].layout {
            *row_count = 1;
        }
        place(&mut a, &local).unwrap();
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
        a.add_pattern(vec![s1, s2], layout, &local).unwrap();
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
        a.add_pattern(vec![s], layout.clone(), &local).unwrap();
        let copy = a.patterns[0].elements[0].occurrences[0];
        assert!(a.add_pattern(vec![copy], layout.clone(), &local).is_err());
        a.forget_in_patterns(copy);
        a.remove(copy);
        place(&mut a, &local).unwrap();
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
