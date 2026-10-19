//! Sketch patterns (Inventor: Sketch → Pattern → Rectangular / Circular).
//!
//! Patterns are associative: every copy is tied to the previous one by
//! [`Constraint::Translated`] / [`Constraint::Rotated`], which repeat the
//! *first step* of the pattern. That step is ordinary, editable geometry:
//!
//! - rectangular: a construction line from the anchor to its first copy,
//!   Parallel to the direction line, with a Distance dimension = spacing;
//! - circular: construction lines from the centre to the anchor and to its
//!   first copy, Equal, with an Angle dimension = angular step.
//!
//! Editing the spacing or angle (or moving the original) moves the whole
//! pattern; the copies add no degrees of freedom. Copies of circles get an
//! Equal radius. A copied arc already carries the solver's "end on the
//! circle" equation, so one of its end points is tied only tangentially
//! (`arc_center`) — otherwise every arc copy would be over-constrained.

use std::collections::HashMap;
use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};

use super::{
    Constraint, ConstraintId, Entity, EntityId, PatternId, Point2, PointId, Shape, Sketch,
    SketchError, Style,
};
use crate::kernel::Real;

/// One pattern instance: original point → its copy, and the copied entities.
type Instance = (HashMap<PointId, PointId>, Vec<EntityId>);

/// One direction of a rectangular pattern (Inventor's Direction 1 / 2).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatternDirection {
    /// A line giving the direction.
    pub line: EntityId,
    /// Run against the line's start → end direction.
    pub flip: bool,
    /// Number of instances, including the original (≥ 1).
    pub count: usize,
    /// Distance between neighbouring instances, mm.
    pub spacing: Real,
    /// Fitted (Inventor): the sketch dimension is the overall length from
    /// the original to the last instance, not the step.
    #[serde(default)]
    pub fitted: bool,
}

/// The definition of a pattern (what Edit Pattern changes).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PatternKind {
    /// Along one or two lines.
    Rectangular {
        /// Direction 1.
        dir1: PatternDirection,
        /// Direction 2 (count 1 = unused).
        dir2: Option<PatternDirection>,
    },
    /// Around a point.
    Circular {
        /// Centre point.
        center: PointId,
        /// Instances, with the original.
        count: usize,
        /// Overall angle (radians; sign = direction; a full turn spaces
        /// evenly).
        angle: Real,
        /// Fitted: the sketch dimension is the overall angle (when it is
        /// below 180°), not the step.
        #[serde(default)]
        fitted: bool,
    },
    /// At points (Inventor's Sketch Driven Pattern, in the sketch): a copy
    /// moved from `base` to each of [`SketchPattern::targets`].
    Points {
        /// Reference point of the original geometry.
        base: PointId,
    },
    /// Along a curve (line, arc or spline; our extension): copies moved by
    /// the offset of points on the curve at equal chords from its start.
    Path {
        /// The curve.
        path: EntityId,
        /// Instances, with the original.
        count: usize,
        /// Chord between neighbouring instances (mm).
        spacing: Real,
    },
}

/// An individually offset element of a rectangular pattern.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct InstanceOffset {
    /// Index into [`SketchPattern::instances`].
    pub instance: usize,
    /// Extra offset along direction 1 and direction 2 (mm); without a
    /// second direction, across direction 1 (to its left).
    pub along: [Real; 2],
}

/// One copy of the pattern's geometry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatternInstance {
    /// Copied curves, in the order of [`SketchPattern::source`].
    pub entities: Vec<EntityId>,
    /// Points created for this copy.
    pub points: Vec<PointId>,
}

/// A pattern as an object of the sketch (Inventor: right-click a pattern
/// element → Edit Pattern / Suppress Element / Delete Pattern).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SketchPattern {
    /// Its definition.
    pub kind: PatternKind,
    /// The original curves.
    pub source: Vec<EntityId>,
    /// The copies (not the original).
    pub instances: Vec<PatternInstance>,
    /// Construction lines of the first step.
    pub helpers: Vec<EntityId>,
    /// Every constraint the pattern created.
    pub constraints: Vec<ConstraintId>,
    /// Suppressed instances (indices into `instances`).
    #[serde(default)]
    pub suppressed: Vec<usize>,
    /// Individually offset instances (rectangular).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub offsets: Vec<InstanceOffset>,
    /// Target points of a [`PatternKind::Points`] pattern.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<PointId>,
    /// Equations of the instance counts (direction 1 / 2, or the circular
    /// count first), evaluated with the part's parameters; the count is not
    /// a sketch dimension, so it is kept here (Inventor: Count accepts a
    /// parameter).
    #[serde(default, skip_serializing_if = "no_counts")]
    pub count_exprs: [Option<String>; 2],
}

fn no_counts(c: &[Option<String>; 2]) -> bool {
    c.iter().all(Option::is_none)
}

impl SketchPattern {
    /// All curves of the pattern: original, copies, helpers.
    pub fn all_entities(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.source
            .iter()
            .chain(self.instances.iter().flat_map(|i| i.entities.iter()))
            .chain(&self.helpers)
            .copied()
    }
}

impl Sketch {
    /// The patterns of the sketch.
    pub fn patterns(&self) -> impl Iterator<Item = (PatternId, &SketchPattern)> {
        self.patterns.iter()
    }

    /// A pattern by id.
    pub fn pattern(&self, id: PatternId) -> Option<&SketchPattern> {
        self.patterns.get(id)
    }

    /// Sets the equations of a pattern's counts (see
    /// [`SketchPattern::count_exprs`]); the part evaluates them.
    pub fn set_pattern_count_exprs(
        &mut self,
        id: PatternId,
        exprs: [Option<String>; 2],
    ) -> Result<(), SketchError> {
        let p = self
            .patterns
            .get_mut(id)
            .ok_or(SketchError::UnknownEntity)?;
        p.count_exprs = exprs;
        Ok(())
    }

    /// The pattern a curve belongs to, and which instance it is in (`None`
    /// for the original and the helper lines).
    pub fn pattern_containing(&self, e: EntityId) -> Option<(PatternId, Option<usize>)> {
        self.patterns.iter().find_map(|(id, p)| {
            if p.source.contains(&e) || p.helpers.contains(&e) {
                return Some((id, None));
            }
            p.instances
                .iter()
                .position(|i| i.entities.contains(&e))
                .map(|k| (id, Some(k)))
        })
    }

    /// What is wrong with a pattern, if anything: its direction line,
    /// centre or original geometry was deleted (Inventor shows the pattern
    /// as sick; Delete Pattern or Break Pattern resolve it).
    pub fn pattern_health(&self, id: PatternId) -> Option<String> {
        let p = self.patterns.get(id)?;
        if p.source.iter().any(|e| self.entity(*e).is_none()) {
            return Some("its original geometry was deleted".into());
        }
        match p.kind {
            PatternKind::Rectangular { dir1, dir2 } => {
                let line_ok = |d: &PatternDirection| {
                    d.count < 2 || self.entity(d.line).is_some_and(Entity::is_line)
                };
                if !line_ok(&dir1) || dir2.as_ref().is_some_and(|d| !line_ok(d)) {
                    return Some("its direction line was deleted".into());
                }
            }
            PatternKind::Circular { center, .. } => {
                if self.point(center).is_none() {
                    return Some("its centre point was deleted".into());
                }
            }
            PatternKind::Points { base } => {
                if self.point(base).is_none() || p.targets.iter().any(|t| self.point(*t).is_none())
                {
                    return Some("its base or a target point was deleted".into());
                }
            }
            PatternKind::Path { path, .. } => {
                if self.entity(path).is_none() {
                    return Some("its path curve was deleted".into());
                }
            }
        }
        None
    }

    /// The pattern's definition with the values its dimensions have *now*
    /// (the spacing or angle may have been edited in the sketch since).
    pub fn pattern_current_kind(&self, id: PatternId) -> Option<PatternKind> {
        let p = self.patterns.get(id)?;
        let values: Vec<Constraint> = p
            .constraints
            .iter()
            .filter_map(|c| self.constraint(*c).cloned())
            .filter(|c| c.is_dimension())
            .collect();
        Some(match p.kind {
            PatternKind::Rectangular { mut dir1, mut dir2 } => {
                let mut dims = values.iter().filter_map(Constraint::value);
                for d in std::iter::once(&mut dir1).chain(dir2.as_mut()) {
                    if d.count < 2 {
                        continue;
                    }
                    if let Some(v) = dims.next() {
                        d.spacing = if d.fitted && d.count > 2 {
                            v / (d.count - 1) as Real
                        } else {
                            v
                        };
                    }
                }
                PatternKind::Rectangular { dir1, dir2 }
            }
            PatternKind::Circular {
                center,
                count,
                angle,
                fitted,
            } => {
                let full = (angle.abs() - TAU).abs() < 1e-9;
                let dim = values.iter().find_map(Constraint::value).map(Real::abs);
                let n = (count - 1) as Real;
                let total_fitted =
                    fitted && count > 2 && (angle.abs() < std::f64::consts::PI - 1e-6);
                let angle = match dim {
                    _ if full => angle,
                    Some(v) if total_fitted => v.copysign(angle),
                    Some(v) => (v * n).copysign(angle),
                    None => angle,
                };
                PatternKind::Circular {
                    center,
                    count,
                    angle,
                    fitted,
                }
            }
            k @ PatternKind::Points { .. } => k,
            PatternKind::Path { path, count, .. } => PatternKind::Path {
                path,
                count,
                spacing: values
                    .iter()
                    .find_map(Constraint::value)
                    .unwrap_or(match p.kind {
                        PatternKind::Path { spacing, .. } => spacing,
                        _ => 10.0,
                    }),
            },
        })
    }

    /// Removes the pattern's copies, helpers and constraints (the record
    /// stays).
    fn clear_pattern(&mut self, id: PatternId) -> Result<(), SketchError> {
        let p = self
            .patterns
            .get(id)
            .ok_or(SketchError::UnknownEntity)?
            .clone();
        for c in &p.constraints {
            let _ = self.remove_constraint(*c);
        }
        for e in p
            .instances
            .iter()
            .flat_map(|i| i.entities.iter())
            .chain(&p.helpers)
        {
            if self.entity(*e).is_some() {
                self.remove_entity(*e)?;
            }
        }
        for q in p.instances.iter().flat_map(|i| i.points.iter()) {
            if self.point(*q).is_some() {
                self.drop_orphan(*q)?;
            }
        }
        Ok(())
    }

    /// **Delete Pattern**: removes the copies and the pattern's constraints;
    /// the original geometry stays.
    pub fn delete_pattern(&mut self, id: PatternId) -> Result<(), SketchError> {
        self.clear_pattern(id)?;
        self.patterns.remove(id);
        Ok(())
    }

    /// **Break Pattern** (Associative off): the copies become independent
    /// geometry; the pattern's ties and helper lines go away.
    pub fn break_pattern(&mut self, id: PatternId) -> Result<(), SketchError> {
        let p = self.patterns.remove(id).ok_or(SketchError::UnknownEntity)?;
        for c in &p.constraints {
            let _ = self.remove_constraint(*c);
        }
        for e in &p.helpers {
            if self.entity(*e).is_some() {
                self.remove_entity(*e)?;
            }
        }
        // Suppressed copies come back as normal geometry.
        for &k in &p.suppressed {
            if let Some(inst) = p.instances.get(k) {
                for (i, e) in inst.entities.iter().enumerate() {
                    let style = p.source.get(i).map_or(Style::Normal, |s| self.style(*s));
                    let _ = self.set_style(*e, style);
                }
            }
        }
        Ok(())
    }

    /// **Suppress / Unsuppress Element** `k`: a suppressed copy is shown
    /// dashed (construction) and left out of profiles; its ties stay.
    pub fn set_pattern_suppressed(
        &mut self,
        id: PatternId,
        k: usize,
        suppressed: bool,
    ) -> Result<(), SketchError> {
        let p = self
            .patterns
            .get(id)
            .ok_or(SketchError::UnknownEntity)?
            .clone();
        let inst = p
            .instances
            .get(k)
            .ok_or(SketchError::InvalidValue("no such pattern element"))?;
        for (i, e) in inst.entities.iter().enumerate() {
            let style = if suppressed {
                Style::Construction
            } else {
                p.source.get(i).map_or(Style::Normal, |s| self.style(*s))
            };
            if self.entity(*e).is_some() {
                self.set_style(*e, style)?;
            }
        }
        let rec = &mut self.patterns[id];
        rec.suppressed.retain(|&x| x != k);
        if suppressed {
            rec.suppressed.push(k);
        }
        Ok(())
    }

    /// **Edit Pattern**: rebuilds the copies with the definition `kind`
    /// (same original, same id; suppressed elements stay suppressed while
    /// they exist). On failure the sketch is unchanged.
    pub fn edit_pattern(&mut self, id: PatternId, kind: PatternKind) -> Result<(), SketchError> {
        let mut trial = self.clone();
        let old = trial
            .patterns
            .get(id)
            .ok_or(SketchError::UnknownEntity)?
            .clone();
        trial.clear_pattern(id)?;
        match kind {
            PatternKind::Rectangular { dir1, dir2 } => {
                // Offsets only of elements that still exist.
                let n = dir1.count * dir2.map_or(1, |d| d.count) - 1;
                let offsets: Vec<InstanceOffset> = old
                    .offsets
                    .iter()
                    .copied()
                    .filter(|o| o.instance < n)
                    .collect();
                trial.rectangular_pattern_with(&old.source, dir1, dir2, &offsets)?;
            }
            PatternKind::Points { base } => {
                trial.points_pattern(&old.source, base, &old.targets)?;
            }
            PatternKind::Path {
                path,
                count,
                spacing,
            } => {
                trial.path_pattern(&old.source, path, count, spacing)?;
            }
            PatternKind::Circular {
                center,
                count,
                angle,
                fitted,
            } => {
                trial.circular_pattern(&old.source, center, count, angle, fitted)?;
            }
        }
        // The builder recorded a new pattern: move it into the old id.
        let new_id = trial
            .patterns
            .iter()
            .find(|(pid, _)| *pid != id && trial.patterns[*pid].source == old.source)
            .map(|(pid, _)| pid)
            .ok_or(SketchError::UnknownEntity)?;
        let rebuilt = trial
            .patterns
            .remove(new_id)
            .ok_or(SketchError::UnknownEntity)?;
        trial.patterns[id] = rebuilt;
        trial.patterns[id].count_exprs = old.count_exprs.clone();
        // The parameters of the pattern's dimensions (name, equation,
        // tolerance) stay on the rebuilt dimensions, in order, so other
        // equations naming them keep working.
        let dims = |sk: &Sketch, p: &SketchPattern| -> Vec<ConstraintId> {
            p.constraints
                .iter()
                .copied()
                .filter(|&k| sk.constraint(k).and_then(|c| c.value()).is_some())
                .collect()
        };
        let old_dims = dims(self, &old);
        let new_dims = dims(&trial, &trial.patterns[id]);
        for (o, n) in old_dims.into_iter().zip(new_dims) {
            if self.dim_params.contains_key(o)
                && let Some(p) = trial.dim_param_mut(n)
            {
                *p = self.dim_param(o);
            }
        }
        for k in old.suppressed {
            if k < trial.patterns[id].instances.len() {
                trial.set_pattern_suppressed(id, k, true)?;
            }
        }
        *self = trial;
        Ok(())
    }

    /// How each point of `entities` is tied in a copy: fully (`None`), only
    /// tangentially to the arc with the given centre, or not at all (the end
    /// of two arcs, fixed by their two circle equations).
    fn pattern_ties(&self, entities: &[EntityId]) -> HashMap<PointId, Vec<PointId>> {
        let mut reduced: HashMap<PointId, Vec<PointId>> = HashMap::new();
        for e in entities {
            if let Some(&Entity::Arc { center, start, end }) = self.entity(*e) {
                let p = if reduced.contains_key(&end) && !reduced.contains_key(&start) {
                    start
                } else {
                    end
                };
                reduced.entry(p).or_default().push(center);
            }
        }
        reduced
    }

    /// Ties copy `to` to copy `from` (point maps from the originals) with
    /// `make(a, a2, arc_center)`; circles get Equal radius to the original.
    fn tie_copy(
        &mut self,
        entities: &[EntityId],
        from: &HashMap<PointId, PointId>,
        (to, to_entities): &Instance,
        skip: Option<PointId>,
        make: &dyn Fn(PointId, PointId, Option<PointId>) -> Constraint,
        sh: &mut Shape,
    ) -> Result<(), SketchError> {
        let ties = self.pattern_ties(entities);
        for p in self.points_of(entities) {
            // Skipped, or shared by the copy (the centre of a rotation).
            if Some(p) == skip || from.get(&p) == to.get(&p) {
                continue;
            }
            let arc_center = match ties.get(&p).map(Vec::as_slice) {
                None => None,
                Some([c]) => Some(to[c]),
                Some(_) => continue,
            };
            sh.constraints
                .push(self.add_constraint(make(from[&p], to[&p], arc_center))?);
        }
        for (i, &e) in entities.iter().enumerate() {
            if matches!(self.entity(e), Some(Entity::Circle { .. })) {
                sh.constraints.push(self.add_constraint(Constraint::Equal {
                    a: e,
                    b: to_entities[i],
                })?);
            }
        }
        Ok(())
    }

    fn pattern_input(&self, entities: &[EntityId]) -> Result<Vec<PointId>, SketchError> {
        if entities.is_empty() {
            return Err(SketchError::InvalidValue("select the geometry to pattern"));
        }
        let pts = self.points_of(entities);
        if pts.is_empty() {
            return Err(SketchError::UnknownEntity);
        }
        Ok(pts)
    }

    /// **Rectangular pattern** of `entities` along `dir1` (and `dir2`).
    /// Returns all copies, the construction step lines and the constraints.
    pub fn rectangular_pattern(
        &mut self,
        entities: &[EntityId],
        dir1: PatternDirection,
        dir2: Option<PatternDirection>,
    ) -> Result<Shape, SketchError> {
        self.rectangular_pattern_with(entities, dir1, dir2, &[])
    }

    /// [`Sketch::rectangular_pattern`] with individually offset elements.
    /// The first copy of each direction (it defines the spacing) and, when
    /// Fitted, the last one cannot be offset.
    pub fn rectangular_pattern_with(
        &mut self,
        entities: &[EntityId],
        dir1: PatternDirection,
        dir2: Option<PatternDirection>,
        offsets: &[InstanceOffset],
    ) -> Result<Shape, SketchError> {
        let pts = self.pattern_input(entities)?;
        let dirs: Vec<PatternDirection> = std::iter::once(dir1).chain(dir2).collect();
        let mut vecs = Vec::new();
        for d in &dirs {
            if d.count == 0 || !(d.spacing.is_finite() && d.spacing > 0.0) {
                return Err(SketchError::InvalidValue(
                    "pattern count must be ≥ 1 and spacing positive",
                ));
            }
            if entities.contains(&d.line) {
                return Err(SketchError::InvalidValue(
                    "the direction line cannot be part of the pattern",
                ));
            }
            let (l0, l1) = self
                .line_geometry(d.line)
                .ok_or(SketchError::WrongEntityKind("pattern direction"))?;
            let u = (l1 - l0).normalize() * if d.flip { -1.0 } else { 1.0 };
            vecs.push(u * d.spacing);
        }
        let (n1, n2) = (dir1.count, dir2.map_or(1, |d| d.count));
        if n1 * n2 < 2 {
            return Err(SketchError::InvalidValue(
                "the pattern needs at least 2 instances",
            ));
        }
        // Offset of grid cell (i, j), in sketch coordinates.
        let units: Vec<nalgebra::Vector2<Real>> =
            vecs.iter().map(|v| v / v.norm().max(1e-12)).collect();
        let fixed = |i: usize, j: usize| {
            (i, j) == (1, 0)
                || (i, j) == (0, 1)
                || (dir1.fitted && dir1.count > 2 && (i, j) == (dir1.count - 1, 0))
                || dir2.is_some_and(|d| d.fitted && d.count > 2 && (i, j) == (0, d.count - 1))
        };
        let mut off = vec![vec![nalgebra::Vector2::zeros(); n2]; n1];
        for o in offsets {
            let (i, j) = ((o.instance + 1) / n2, (o.instance + 1) % n2);
            if i >= n1 {
                continue;
            }
            if fixed(i, j) {
                return Err(SketchError::InvalidValue(
                    "this element defines the spacing and cannot be offset",
                ));
            }
            // Across: direction 2, or the left normal of direction 1.
            let across = units
                .get(1)
                .copied()
                .unwrap_or_else(|| nalgebra::Vector2::new(-units[0].y, units[0].x));
            off[i][j] = units[0] * o.along[0] + across * o.along[1];
        }

        let anchor = pts[0];
        let identity: HashMap<PointId, PointId> = pts.iter().map(|&p| (p, p)).collect();
        let mut sh = Shape::default();
        // grid[i][j]: point map and entities of instance (i, j); (0, 0) = original.
        let mut grid: Vec<Vec<Instance>> = Vec::new();
        let mut instances = Vec::new();
        let mut helpers = Vec::new();
        for (i, off_row) in off.iter().enumerate() {
            let mut row = Vec::new();
            for (j, off_ij) in off_row.iter().enumerate() {
                if i == 0 && j == 0 {
                    row.push((identity.clone(), entities.to_vec()));
                    continue;
                }
                let mut v = vecs[0] * i as Real + off_ij;
                if j > 0 {
                    v += vecs[1] * j as Real;
                }
                let (copy, map) = self.copy_geometry(entities, |q| q + v, false, false)?;
                sh.points.extend(&copy.points);
                sh.entities.extend(&copy.entities);
                instances.push(PatternInstance {
                    entities: copy.entities.clone(),
                    points: copy.points.clone(),
                });
                row.push((map, copy.entities));
            }
            grid.push(row);
        }
        // The first step of each direction: anchor → its copy, as an
        // editable construction line.
        let mut steps = Vec::new();
        for (k, d) in dirs.iter().enumerate() {
            if d.count < 2 {
                steps.push(None);
                continue;
            }
            let (i, j) = if k == 0 { (1, 0) } else { (0, 1) };
            let a1 = grid[i][j].0[&anchor];
            let step = self.add_line_between(anchor, a1)?;
            self.set_style(step, Style::Construction)?;
            sh.entities.push(step);
            helpers.push(step);
            // Fitted: dimension the original to the last instance; the
            // Translated chain makes the step a fraction of it.
            let (far, value) = if d.fitted && d.count > 2 {
                let (li, lj) = if k == 0 {
                    (d.count - 1, 0)
                } else {
                    (0, d.count - 1)
                };
                (grid[li][lj].0[&anchor], d.spacing * (d.count - 1) as Real)
            } else {
                (a1, d.spacing)
            };
            for c in [
                Constraint::Parallel { a: d.line, b: step },
                Constraint::Distance {
                    a: anchor,
                    b: far,
                    value,
                },
            ] {
                sh.constraints.push(self.add_constraint(c)?);
            }
            steps.push(Some(a1));
        }
        for i in 0..n1 {
            for j in 0..n2 {
                let (pi, pj, k) = match (i, j) {
                    (0, 0) => continue,
                    (0, _) => (0, j - 1, 1),
                    _ => (i - 1, j, 0),
                };
                let Some(b2) = steps[k] else {
                    return Err(SketchError::InvalidValue(
                        "pattern direction needs two instances",
                    ));
                };
                // The defining step itself is fixed by its line's dimension.
                let skip = ((i, j) == (1, 0) || (i, j) == (0, 1)).then_some(anchor);
                let from = grid[pi][pj].0.clone();
                let to = grid[i][j].clone();
                // This element's offset minus its neighbour's: the chain
                // returns to the grid after an offset element.
                let d = off[i][j] - off[pi][pj];
                let offset = [d.x, d.y];
                let make = move |a, a2, arc_center| Constraint::Translated {
                    a,
                    a2,
                    b: anchor,
                    b2,
                    arc_center,
                    offset,
                };
                self.tie_copy(entities, &from, &to, skip, &make, &mut sh)?;
            }
        }
        self.patterns.insert(SketchPattern {
            kind: PatternKind::Rectangular { dir1, dir2 },
            source: entities.to_vec(),
            instances,
            helpers,
            constraints: sh.constraints.clone(),
            suppressed: Vec::new(),
            offsets: offsets.to_vec(),
            targets: Vec::new(),
            count_exprs: Default::default(),
        });
        Ok(sh)
    }

    /// **Circular pattern** of `entities` about point `center`: `count`
    /// instances (with the original) over `angle` radians (sign = direction;
    /// a full turn spaces them evenly, otherwise the last one sits at
    /// `angle`). Returns the copies, construction lines and constraints.
    pub fn circular_pattern(
        &mut self,
        entities: &[EntityId],
        center: PointId,
        count: usize,
        angle: Real,
        fitted: bool,
    ) -> Result<Shape, SketchError> {
        let pts = self.pattern_input(entities)?;
        if count < 2 {
            return Err(SketchError::InvalidValue(
                "the pattern needs at least 2 instances",
            ));
        }
        if !(angle.is_finite() && angle.abs() > 1e-9 && angle.abs() <= TAU + 1e-9) {
            return Err(SketchError::InvalidValue("the angle must be within ±360°"));
        }
        let c = self
            .point(center)
            .ok_or(SketchError::UnknownPoint)?
            .position;
        let step = if (angle.abs() - TAU).abs() < 1e-9 {
            angle / count as Real
        } else {
            angle / (count - 1) as Real
        };
        // Anchor: the point farthest from the centre (defines the rotation).
        let dist = |p: &PointId| self.point(*p).map_or(0.0, |q| (q.position - c).norm());
        let anchor = *pts
            .iter()
            .filter(|&&p| p != center)
            .max_by(|a, b| dist(a).total_cmp(&dist(b)))
            .ok_or(SketchError::InvalidValue("nothing to rotate"))?;
        if dist(&anchor) < 1e-9 {
            return Err(SketchError::InvalidValue("the geometry lies at the centre"));
        }
        let rotate = |q: Point2, t: Real| {
            let v = q - c;
            let (s, co) = t.sin_cos();
            c + nalgebra::Vector2::new(co * v.x - s * v.y, s * v.x + co * v.y)
        };
        let mut sh = Shape::default();
        let identity: HashMap<PointId, PointId> = pts.iter().map(|&p| (p, p)).collect();
        let mut copies = vec![(identity, entities.to_vec())];
        let mut instances = Vec::new();
        for k in 1..count {
            let t = step * k as Real;
            // A selected point at the centre itself stays shared.
            let (copy, map) =
                self.copy_geometry_keeping(entities, |q| rotate(q, t), false, false, Some(center))?;
            sh.points.extend(&copy.points);
            sh.entities.extend(&copy.entities);
            instances.push(PatternInstance {
                entities: copy.entities.clone(),
                points: copy.points.clone(),
            });
            copies.push((map, copy.entities));
        }
        // First step: centre → anchor and centre → its first copy.
        let a1 = copies[1].0[&anchor];
        let mut helpers_c = Vec::new();
        let r0 = self.add_line_between(center, anchor)?;
        let r1 = self.add_line_between(center, a1)?;
        for l in [r0, r1] {
            self.set_style(l, Style::Construction)?;
            sh.entities.push(l);
            helpers_c.push(l);
        }
        let pa = self
            .point(anchor)
            .ok_or(SketchError::UnknownPoint)?
            .position;
        // Fitted (and below a half turn): dimension the angle to the last
        // instance; the Rotated chain makes the step a fraction of it.
        let total = step * (count - 1) as Real;
        let (to_line, to_angle) =
            if fitted && count > 2 && total.abs() < std::f64::consts::PI - 1e-6 {
                let a_last = copies[count - 1].0[&anchor];
                let r_last = self.add_line_between(center, a_last)?;
                self.set_style(r_last, Style::Construction)?;
                sh.entities.push(r_last);
                helpers_c.push(r_last);
                (r_last, total)
            } else {
                (r1, step)
            };
        let (u, v) = (pa - c, rotate(pa, to_angle) - c);
        for k in [
            Constraint::Equal { a: r0, b: r1 },
            Constraint::Angle {
                a: r0,
                b: to_line,
                value: u.perp(&v).atan2(u.dot(&v)),
                supplement: false,
            },
        ] {
            sh.constraints.push(self.add_constraint(k)?);
        }
        for k in 1..count {
            let from = copies[k - 1].0.clone();
            let to = copies[k].clone();
            let skip = (k == 1).then_some(anchor);
            let make = move |a, a2, arc_center| Constraint::Rotated {
                center,
                a,
                a2,
                b: anchor,
                b2: a1,
                arc_center,
            };
            self.tie_copy(entities, &from, &to, skip, &make, &mut sh)?;
        }
        self.patterns.insert(SketchPattern {
            kind: PatternKind::Circular {
                center,
                count,
                angle,
                fitted,
            },
            source: entities.to_vec(),
            instances,
            helpers: helpers_c,
            constraints: sh.constraints.clone(),
            suppressed: Vec::new(),
            offsets: Vec::new(),
            targets: Vec::new(),
            count_exprs: Default::default(),
        });
        Ok(sh)
    }

    /// **Pattern at points** (Inventor's Sketch Driven Pattern, in the
    /// sketch): a copy of `entities` moved from `base` to each target point.
    /// Each copy follows its point (`Translated` with `base → target`).
    pub fn points_pattern(
        &mut self,
        entities: &[EntityId],
        base: PointId,
        targets: &[PointId],
    ) -> Result<Shape, SketchError> {
        let pts = self.pattern_input(entities)?;
        let b = self.point(base).ok_or(SketchError::UnknownPoint)?.position;
        let targets: Vec<PointId> = targets
            .iter()
            .copied()
            .filter(|t| *t != base && !pts.contains(t))
            .collect();
        if targets.is_empty() {
            return Err(SketchError::InvalidValue("pick at least one target point"));
        }
        let identity: HashMap<PointId, PointId> = pts.iter().map(|&p| (p, p)).collect();
        let mut sh = Shape::default();
        let mut instances = Vec::new();
        for &t in &targets {
            let v = self.point(t).ok_or(SketchError::UnknownPoint)?.position - b;
            let (copy, map) = self.copy_geometry(entities, |q| q + v, false, false)?;
            sh.points.extend(&copy.points);
            sh.entities.extend(&copy.entities);
            instances.push(PatternInstance {
                entities: copy.entities.clone(),
                points: copy.points.clone(),
            });
            let make = move |a, a2, arc_center| Constraint::Translated {
                a,
                a2,
                b: base,
                b2: t,
                arc_center,
                offset: [0.0, 0.0],
            };
            // The base point itself (if it is one of the originals) maps to
            // the target: tie it with Coincident instead.
            let base_in = map.get(&base).copied();
            self.tie_copy(
                entities,
                &identity,
                &(map, copy.entities),
                Some(base),
                &make,
                &mut sh,
            )?;
            if let Some(q) = base_in {
                sh.constraints
                    .push(self.add_constraint(Constraint::Coincident { a: q, b: t })?);
            }
        }
        self.patterns.insert(SketchPattern {
            kind: PatternKind::Points { base },
            source: entities.to_vec(),
            instances,
            helpers: Vec::new(),
            constraints: sh.constraints.clone(),
            suppressed: Vec::new(),
            offsets: Vec::new(),
            targets,
            count_exprs: Default::default(),
        });
        Ok(sh)
    }

    /// **Pattern along a path** (line, arc or spline): `count` instances
    /// (with the original), the copies moved by `H_k − S`, where `S` is the
    /// path's start and `H_k` points on the path at equal chords `spacing`
    /// (the first chord is a dimension, the others are Equal to it).
    pub fn path_pattern(
        &mut self,
        entities: &[EntityId],
        path: EntityId,
        count: usize,
        spacing: Real,
    ) -> Result<Shape, SketchError> {
        let pts = self.pattern_input(entities)?;
        if entities.contains(&path) {
            return Err(SketchError::InvalidValue(
                "the path cannot be part of the pattern",
            ));
        }
        if count < 2 || !(spacing.is_finite() && spacing > 0.0) {
            return Err(SketchError::InvalidValue(
                "the pattern needs at least 2 instances and a positive spacing",
            ));
        }
        let poly = self.curve_polyline(path).ok_or(SketchError::InvalidValue(
            "the path must be a line, an arc or a spline",
        ))?;
        let start = self
            .entity(path)
            .and_then(|e| e.endpoints().first().copied())
            .ok_or(SketchError::InvalidValue("the path must be an open curve"))?;
        let length: Real = poly.windows(2).map(|w| (w[1] - w[0]).norm()).sum();
        if spacing * (count - 1) as Real > length + 1e-9 {
            return Err(SketchError::InvalidValue(
                "the pattern is longer than the path",
            ));
        }
        // Initial positions at arc length k·spacing (the solver makes the
        // chords exact).
        let at = |d: Real| -> Point2 {
            let mut left = d;
            for w in poly.windows(2) {
                let l = (w[1] - w[0]).norm();
                if left <= l {
                    return w[0] + (w[1] - w[0]) * (left / l.max(1e-12));
                }
                left -= l;
            }
            poly.last().copied().unwrap_or_else(Point2::origin)
        };
        let s0 = self.point(start).ok_or(SketchError::UnknownPoint)?.position;
        let identity: HashMap<PointId, PointId> = pts.iter().map(|&p| (p, p)).collect();
        let mut sh = Shape::default();
        let mut instances = Vec::new();
        let mut helpers = Vec::new();
        let mut prev = start;
        let mut first_chord: Option<EntityId> = None;
        for k in 1..count {
            let hk = at(spacing * k as Real);
            let h = self.add_point(hk);
            // On the path, one chord from the previous station.
            if let Some(c) = self.on_curve(h, path) {
                sh.constraints.push(self.add_constraint(c)?);
            }
            let chord = self.add_line_between(prev, h)?;
            self.set_style(chord, Style::Construction)?;
            sh.entities.push(chord);
            helpers.push(chord);
            match first_chord {
                None => {
                    sh.constraints
                        .push(self.add_constraint(Constraint::Distance {
                            a: prev,
                            b: h,
                            value: spacing,
                        })?);
                    first_chord = Some(chord);
                }
                Some(f) => {
                    sh.constraints
                        .push(self.add_constraint(Constraint::Equal { a: f, b: chord })?);
                }
            }
            let v = hk - s0;
            let (copy, map) = self.copy_geometry(entities, |q| q + v, false, false)?;
            sh.points.extend(&copy.points);
            sh.entities.extend(&copy.entities);
            let mut points = copy.points.clone();
            points.push(h); // removed with the copy
            instances.push(PatternInstance {
                entities: copy.entities.clone(),
                points,
            });
            let make = move |a, a2, arc_center| Constraint::Translated {
                a,
                a2,
                b: start,
                b2: h,
                arc_center,
                offset: [0.0, 0.0],
            };
            self.tie_copy(
                entities,
                &identity,
                &(map, copy.entities),
                None,
                &make,
                &mut sh,
            )?;
            prev = h;
        }
        self.patterns.insert(SketchPattern {
            kind: PatternKind::Path {
                path,
                count,
                spacing,
            },
            source: entities.to_vec(),
            instances,
            helpers,
            constraints: sh.constraints.clone(),
            suppressed: Vec::new(),
            offsets: Vec::new(),
            targets: Vec::new(),
            count_exprs: Default::default(),
        });
        Ok(sh)
    }

    /// A curve as a polyline from its start to its end (arcs counter-
    /// clockwise); `None` for circles.
    fn curve_polyline(&self, e: EntityId) -> Option<Vec<Point2>> {
        let pos = |p: PointId| self.point(p).map(|q| q.position);
        match self.entity(e)? {
            Entity::Line { start, end } => Some(vec![pos(*start)?, pos(*end)?]),
            Entity::Arc { center, start, end } => {
                let (c, a, b) = (pos(*center)?, pos(*start)?, pos(*end)?);
                let r = (a - c).norm();
                let a0 = (a - c).y.atan2((a - c).x);
                let mut sweep = (b - c).y.atan2((b - c).x) - a0;
                if sweep <= 0.0 {
                    sweep += TAU;
                }
                Some(
                    (0..=96)
                        .map(|i| {
                            let t = a0 + sweep * i as Real / 96.0;
                            c + nalgebra::Vector2::new(t.cos(), t.sin()) * r
                        })
                        .collect(),
                )
            }
            Entity::Spline { .. } => {
                let c = self.spline_curve(e)?;
                let d = c.domain();
                Some(
                    (0..=200)
                        .map(|i| {
                            let p = c.point_at(d.min + (d.max - d.min) * i as Real / 200.0);
                            Point2::new(p.x, p.y)
                        })
                        .collect(),
                )
            }
            Entity::Circle { .. } => None,
        }
    }

    /// Offsets element `k` of a rectangular pattern by `along` (mm along
    /// direction 1 and 2; zero removes the offset). The other elements stay
    /// on the grid. On failure the sketch is unchanged.
    pub fn set_pattern_offset(
        &mut self,
        id: PatternId,
        k: usize,
        along: [Real; 2],
    ) -> Result<(), SketchError> {
        let kind = self
            .pattern_current_kind(id)
            .ok_or(SketchError::UnknownEntity)?;
        if !matches!(kind, PatternKind::Rectangular { .. }) {
            return Err(SketchError::InvalidValue(
                "only rectangular pattern elements can be offset",
            ));
        }
        let before = self.clone();
        let rec = &mut self.patterns[id];
        rec.offsets.retain(|o| o.instance != k);
        if along != [0.0, 0.0] {
            rec.offsets.push(InstanceOffset { instance: k, along });
        }
        if let Err(e) = self.edit_pattern(id, kind) {
            *self = before;
            return Err(e);
        }
        Ok(())
    }

    /// **Edit Pattern → Geometry**: patterns `entities` instead of the
    /// current original (add or remove curves); the definition, offsets and
    /// suppressed elements stay. On failure the sketch is unchanged.
    pub fn set_pattern_source(
        &mut self,
        id: PatternId,
        entities: &[EntityId],
    ) -> Result<(), SketchError> {
        let kind = self
            .pattern_current_kind(id)
            .ok_or(SketchError::UnknownEntity)?;
        let p = self.patterns.get(id).ok_or(SketchError::UnknownEntity)?;
        let mut src: Vec<EntityId> = Vec::new();
        for &e in entities {
            if self.entity(e).is_none() {
                return Err(SketchError::UnknownEntity);
            }
            // Copies and helper lines of this pattern cannot be its input.
            if p.instances.iter().any(|i| i.entities.contains(&e)) || p.helpers.contains(&e) {
                return Err(SketchError::InvalidValue(
                    "the pattern's own copies cannot be its geometry",
                ));
            }
            if !src.contains(&e) {
                src.push(e);
            }
        }
        if src.is_empty() {
            return Err(SketchError::InvalidValue("select the geometry to pattern"));
        }
        let before = self.clone();
        self.patterns[id].source = src;
        // `edit_pattern` clears the old copies (recorded in the pattern) and
        // rebuilds from the new source.
        if let Err(e) = self.edit_pattern(id, kind) {
            *self = before;
            return Err(e);
        }
        Ok(())
    }

    /// The offset of pattern element `k` (zero if none).
    pub fn pattern_offset(&self, id: PatternId, k: usize) -> [Real; 2] {
        self.patterns
            .get(id)
            .and_then(|p| p.offsets.iter().find(|o| o.instance == k))
            .map_or([0.0, 0.0], |o| o.along)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Centre of a copied circle.
    fn centre(sk: &Sketch, e: EntityId) -> Point2 {
        let Some(&Entity::Circle { center, .. }) = sk.entity(e) else {
            panic!("not a circle");
        };
        sk.point(center).unwrap().position
    }

    #[test]
    fn an_offset_element_moves_alone_and_survives_edits() {
        let mut sk = Sketch::default();
        let dir = sk.add_line(Point2::new(0.0, -10.0), Point2::new(50.0, -10.0));
        let c = sk.add_circle(Point2::new(0.0, 0.0), 2.0).unwrap();
        let d1 = PatternDirection {
            line: dir,
            flip: false,
            count: 4,
            spacing: 10.0,
            fitted: false,
        };
        sk.rectangular_pattern(&[c], d1, None).unwrap();
        let (id, _) = sk.patterns().next().unwrap();
        // The first copy defines the spacing: refused, unchanged.
        assert!(sk.set_pattern_offset(id, 0, [2.0, 0.0]).is_err());
        // Third element (index 1, the 2nd copy) +3 along, +4 across.
        sk.set_pattern_offset(id, 1, [3.0, 4.0]).unwrap();
        let rep = sk.solve().unwrap();
        assert_eq!(rep.redundant, 0);
        let inst =
            |sk: &Sketch, k: usize| centre(sk, sk.pattern(id).unwrap().instances[k].entities[0]);
        assert!((inst(&sk, 0) - Point2::new(10.0, 0.0)).norm() < 1e-6);
        assert!(
            (inst(&sk, 1) - Point2::new(23.0, 4.0)).norm() < 1e-6,
            "{:?}",
            inst(&sk, 1)
        );
        assert!(
            (inst(&sk, 2) - Point2::new(30.0, 0.0)).norm() < 1e-6,
            "back on the grid"
        );
        assert_eq!(sk.pattern_offset(id, 1), [3.0, 4.0]);
        // Edit Pattern (spacing 12) keeps the offset.
        let mut kind = sk.pattern_current_kind(id).unwrap();
        if let PatternKind::Rectangular { dir1, .. } = &mut kind {
            dir1.spacing = 12.0;
        }
        sk.edit_pattern(id, kind).unwrap();
        sk.solve().unwrap();
        assert!((inst(&sk, 1) - Point2::new(27.0, 4.0)).norm() < 1e-6);
        // Zero removes it.
        sk.set_pattern_offset(id, 1, [0.0, 0.0]).unwrap();
        sk.solve().unwrap();
        assert!((inst(&sk, 1) - Point2::new(24.0, 0.0)).norm() < 1e-6);
    }

    #[test]
    fn pattern_geometry_can_be_changed() {
        let mut sk = Sketch::default();
        let dir = sk.add_line(Point2::new(0.0, -10.0), Point2::new(50.0, -10.0));
        let c = sk.add_circle(Point2::new(0.0, 0.0), 2.0).unwrap();
        let l = sk.add_line(Point2::new(-3.0, 5.0), Point2::new(3.0, 5.0));
        let d1 = PatternDirection {
            line: dir,
            flip: false,
            count: 3,
            spacing: 10.0,
            fitted: false,
        };
        sk.rectangular_pattern(&[c], d1, None).unwrap();
        let (id, _) = sk.patterns().next().unwrap();
        sk.set_pattern_suppressed(id, 1, true).unwrap();
        // Add the line: each element now has a circle and a line.
        sk.set_pattern_source(id, &[c, l]).unwrap();
        let p = sk.pattern(id).unwrap();
        assert_eq!(p.source, vec![c, l]);
        assert!(p.instances.iter().all(|i| i.entities.len() == 2));
        assert_eq!(p.suppressed, vec![1], "suppression kept");
        assert_eq!(sk.solve().unwrap().redundant, 0);
        // Remove the circle again; own copies are refused.
        sk.set_pattern_source(id, &[l]).unwrap();
        let copy = sk.pattern(id).unwrap().instances[0].entities[0];
        assert!(sk.set_pattern_source(id, &[l, copy]).is_err());
        assert_eq!(sk.pattern(id).unwrap().source, vec![l]);
        assert!(
            sk.entity(c).is_some(),
            "the removed curve stays as plain geometry"
        );
    }

    #[test]
    fn path_pattern_follows_its_curve() {
        let mut sk = Sketch::default();
        // Arc of radius 50 around the origin, from (50, 0) counter-clockwise.
        let c0 = sk.add_point(Point2::new(0.0, 0.0));
        sk.set_fixed(c0, true).unwrap();
        let a = sk.add_point(Point2::new(50.0, 0.0));
        let b = sk.add_point(Point2::new(0.0, 50.0));
        let arc = sk.add_arc(c0, a, b).unwrap();
        sk.set_fixed(a, true).unwrap();
        let circle = sk.add_circle(Point2::new(50.0, -10.0), 2.0).unwrap();
        sk.path_pattern(&[circle], arc, 4, 15.0).unwrap();
        let rep = sk.solve().unwrap();
        assert_eq!(rep.redundant, 0);
        let (id, p) = sk.patterns().next().unwrap();
        assert_eq!(p.instances.len(), 3);
        // Each copy is offset from the original like its station from the
        // arc start; stations are on the arc, 15 mm chords apart.
        let orig = centre(&sk, circle);
        let start = sk.point(a).unwrap().position;
        let mut prev = start;
        for inst in &p.instances {
            let station = start + (centre(&sk, inst.entities[0]) - orig);
            assert!((station.coords.norm() - 50.0).abs() < 1e-6, "on the arc");
            assert!(
                ((station - prev).norm() - 15.0).abs() < 1e-6,
                "equal chords"
            );
            prev = station;
        }
        // Edit: 3 instances at 20 mm.
        sk.edit_pattern(
            id,
            PatternKind::Path {
                path: arc,
                count: 3,
                spacing: 20.0,
            },
        )
        .unwrap();
        sk.solve().unwrap();
        assert_eq!(sk.pattern(id).unwrap().instances.len(), 2);
        assert!(matches!(
            sk.pattern_current_kind(id),
            Some(PatternKind::Path { spacing, .. }) if (spacing - 20.0).abs() < 1e-9
        ));
        // Too long for the path: refused.
        assert!(
            sk.edit_pattern(
                id,
                PatternKind::Path {
                    path: arc,
                    count: 10,
                    spacing: 20.0
                }
            )
            .is_err()
        );
    }

    #[test]
    fn path_pattern_on_a_spline() {
        let mut sk = Sketch::default();
        let pts: Vec<PointId> = [(0.0, 0.0), (20.0, 15.0), (45.0, -5.0), (70.0, 10.0)]
            .iter()
            .map(|&(x, y)| sk.add_point(Point2::new(x, y)))
            .collect();
        let sp = sk.add_spline(pts.clone()).unwrap();
        for p in &pts {
            sk.set_fixed(*p, true).unwrap();
        }
        let circle = sk.add_circle(Point2::new(0.0, -10.0), 2.0).unwrap();
        let r = sk.path_pattern(&[circle], sp, 4, 10.0);
        assert!(r.is_ok(), "{r:?}");
        let rep = sk.solve();
        assert!(rep.is_ok(), "{rep:?}");
        let rep = rep.unwrap();
        assert_eq!(rep.redundant, 0, "{rep:?}");
    }

    #[test]
    fn points_pattern_follows_its_points() {
        let mut sk = Sketch::default();
        let c = sk.add_circle(Point2::new(0.0, 0.0), 2.0).unwrap();
        let Some(&Entity::Circle { center, .. }) = sk.entity(c) else {
            panic!("unexpected entity kind")
        };
        let t1 = sk.add_point(Point2::new(20.0, 5.0));
        let t2 = sk.add_point(Point2::new(-7.0, 12.0));
        sk.points_pattern(&[c], center, &[t1, t2]).unwrap();
        let (id, p) = sk.patterns().next().unwrap();
        assert_eq!(p.instances.len(), 2);
        assert!(matches!(p.kind, PatternKind::Points { .. }));
        let rep = sk.solve().unwrap();
        assert_eq!(rep.redundant, 0);
        sk.move_point(t1, Point2::new(30.0, -5.0)).unwrap();
        sk.set_fixed(t1, true).unwrap();
        sk.solve().unwrap();
        let first = sk.pattern(id).unwrap().instances[0].entities[0];
        assert!((centre(&sk, first) - Point2::new(30.0, -5.0)).norm() < 1e-6);
        // Rebuilt by Edit Pattern with the same targets.
        let kind = sk.pattern_current_kind(id).unwrap();
        sk.edit_pattern(id, kind).unwrap();
        assert_eq!(sk.pattern(id).unwrap().targets, vec![t1, t2]);
    }
    use crate::sketch::SolveStatus;
    use approx::assert_relative_eq;

    fn p(x: Real, y: Real) -> Point2 {
        Point2::new(x, y)
    }

    fn check(s: &mut Sketch, dof: usize) {
        let r = s.solve().unwrap();
        assert_eq!(
            (r.status, r.dof, r.redundant),
            (SolveStatus::Solved, dof, 0),
            "{r:?}"
        );
    }

    /// A fixed horizontal and a fixed vertical direction line.
    fn axes(s: &mut Sketch) -> (EntityId, EntityId) {
        let o = s.add_fixed_point(p(-50.0, -50.0));
        let x = s.add_fixed_point(p(-40.0, -50.0));
        let y = s.add_fixed_point(p(-50.0, -40.0));
        (
            s.add_line_between(o, x).unwrap(),
            s.add_line_between(o, y).unwrap(),
        )
    }

    #[test]
    fn rectangular_grid_of_a_slot_adds_no_freedom_and_follows_spacing() {
        let mut s = Sketch::default();
        let (hx, vy) = axes(&mut s);
        // A slot has arcs (end-on-circle equations) and a circle: the tricky parts.
        let slot = s
            .add_slot_center_to_center(p(0.0, 0.0), p(10.0, 0.0), 4.0)
            .unwrap();
        let hole = s.add_circle(p(5.0, 8.0), 1.5).unwrap();
        let mut sel = slot.entities.clone();
        sel.push(hole);
        let before = s.clone().solve().unwrap().dof;
        let dir = |line, count, spacing| PatternDirection {
            line,
            flip: false,
            count,
            spacing,
            fitted: false,
        };
        let sh = s
            .rectangular_pattern(&sel, dir(hx, 3, 20.0), Some(dir(vy, 2, 15.0)))
            .unwrap();
        check(&mut s, before);
        // Change the first spacing: the far column follows.
        let id = sh
            .constraints
            .iter()
            .copied()
            .find(|&c| matches!(s.constraint(c), Some(Constraint::Distance { value, .. }) if *value == 20.0))
            .unwrap();
        s.set_dimension(id, 25.0).unwrap();
        check(&mut s, before);
        let last_hole = sh
            .entities
            .iter()
            .copied()
            .rfind(|&e| matches!(s.entity(e), Some(Entity::Circle { .. })))
            .unwrap();
        let (c, r) = s.circle_geometry(last_hole).unwrap();
        let (c0, _) = s.circle_geometry(hole).unwrap();
        assert_relative_eq!(c - c0, nalgebra::Vector2::new(50.0, 15.0), epsilon = 1e-7);
        assert_relative_eq!(r, 1.5, epsilon = 1e-9);
    }

    #[test]
    fn circular_pattern_of_a_hole_and_an_arc() {
        let mut s = Sketch::default();
        let center = s.add_fixed_point(p(0.0, 0.0));
        let hole = s.add_circle(p(20.0, 0.0), 2.0).unwrap();
        let (c, a, b) = (
            s.add_point(p(30.0, 0.0)),
            s.add_point(p(33.0, 0.0)),
            s.add_point(p(30.0, 3.0)),
        );
        let arc = s.add_arc(c, a, b).unwrap();
        let before = s.clone().solve().unwrap().dof;
        let sh = s
            .circular_pattern(&[hole, arc], center, 6, TAU, false)
            .unwrap();
        check(&mut s, before);
        let holes: Vec<_> = sh
            .entities
            .iter()
            .copied()
            .filter(|&e| matches!(s.entity(e), Some(Entity::Circle { .. })))
            .collect();
        assert_eq!(holes.len(), 5);
        let (c1, _) = s.circle_geometry(holes[0]).unwrap();
        assert_relative_eq!(c1, p(10.0, 10.0 * 3f64.sqrt()), epsilon = 1e-9);
        // Moving (and holding) the original hole moves every copy.
        let hc = match s.entity(hole) {
            Some(Entity::Circle { center, .. }) => *center,
            _ => panic!("unexpected entity kind"),
        };
        s.move_point(hc, p(25.0, 0.0)).unwrap();
        s.set_fixed(hc, true).unwrap();
        check(&mut s, before - 2);
        let (c5, _) = s.circle_geometry(holes[4]).unwrap();
        assert_relative_eq!((c5 - p(0.0, 0.0)).norm(), 25.0, epsilon = 1e-7);
    }

    #[test]
    fn pattern_object_edit_suppress_delete_break() {
        let mut s = Sketch::default();
        let (hx, _) = axes(&mut s);
        let hole = s.add_circle(p(0.0, 0.0), 1.0).unwrap();
        let dir = |count| PatternDirection {
            line: hx,
            flip: false,
            count,
            spacing: 5.0,
            fitted: false,
        };
        s.rectangular_pattern(&[hole], dir(3), None).unwrap();
        let before = s.clone().solve().unwrap().dof;
        let (id, p0) = s.patterns().next().map(|(id, p)| (id, p.clone())).unwrap();
        assert_eq!(p0.instances.len(), 2);
        let copy = p0.instances[1].entities[0];
        assert_eq!(s.pattern_containing(copy), Some((id, Some(1))));
        assert_eq!(s.pattern_containing(hole), Some((id, None)));

        // Suppress the last copy, then Edit Pattern to 5: same id, still suppressed.
        s.set_pattern_suppressed(id, 1, true).unwrap();
        assert_eq!(s.style(copy), Style::Construction);
        s.edit_pattern(
            id,
            PatternKind::Rectangular {
                dir1: dir(5),
                dir2: None,
            },
        )
        .unwrap();
        let p1 = s.pattern(id).unwrap().clone();
        assert_eq!(p1.instances.len(), 4);
        assert_eq!(p1.suppressed, vec![1]);
        assert_eq!(s.style(p1.instances[1].entities[0]), Style::Construction);
        check(&mut s, before);
        let circles = |s: &Sketch| {
            s.entities()
                .filter(|(_, e)| matches!(e, Entity::Circle { .. }))
                .count()
        };
        assert_eq!(circles(&s), 5);
        // A count of 1 is refused and changes nothing.
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            s.edit_pattern(
                id,
                PatternKind::Rectangular {
                    dir1: dir(1),
                    dir2: None
                }
            )
            .is_err()
        );
        assert_eq!(serde_json::to_string(&s).unwrap(), json);
        // Deleting the direction line makes the pattern sick, not broken.
        let mut sick = s.clone();
        sick.remove_entity(hx).unwrap();
        assert!(sick.pattern_health(id).is_some());
        assert!(
            sick.pattern_current_kind(id)
                .is_some_and(|k| sick.edit_pattern(id, k).is_err())
        );
        sick.delete_pattern(id).unwrap();
        assert!(s.pattern_health(id).is_none());
        // Saved and loaded with the sketch.
        let back: Sketch = serde_json::from_str(&json).unwrap();
        assert_eq!(back.pattern(id), s.pattern(id));

        // Break: copies stay, independent; the pattern is gone.
        let mut b = s.clone();
        b.break_pattern(id).unwrap();
        assert!(b.pattern(id).is_none());
        assert_eq!(circles(&b), 5);
        assert!(b.solve().unwrap().dof > before, "copies are free now");
        // Delete: only the original stays.
        s.delete_pattern(id).unwrap();
        assert_eq!(circles(&s), 1);
        assert!(s.patterns().next().is_none());
        assert_eq!(s.solve().unwrap().redundant, 0);
    }

    #[test]
    fn fitted_patterns_dimension_the_overall_size() {
        let mut s = Sketch::default();
        let (hx, _) = axes(&mut s);
        let hole = s.add_circle(p(0.0, 0.0), 1.0).unwrap();
        let before = s.clone().solve().unwrap().dof;
        let dir = PatternDirection {
            line: hx,
            flip: false,
            count: 4,
            spacing: 5.0,
            fitted: true,
        };
        s.rectangular_pattern(&[hole], dir, None).unwrap();
        check(&mut s, before);
        let total = s
            .constraints()
            .find(|(_, c)| matches!(c, Constraint::Distance { .. }))
            .map(|(id, c)| (id, c.value().unwrap()))
            .unwrap();
        assert!((total.1 - 15.0).abs() < 1e-9, "the dimension is the total");
        s.set_dimension(total.0, 30.0).unwrap();
        check(&mut s, before);
        let (pid, _) = s.patterns().next().unwrap();
        match s.pattern_current_kind(pid).unwrap() {
            PatternKind::Rectangular { dir1, .. } => {
                assert!(
                    (dir1.spacing - 10.0).abs() < 1e-9,
                    "Edit Pattern sees the edit"
                )
            }
            _ => panic!("unexpected entity kind"),
        }
        let far = s
            .entities()
            .filter_map(|(id, _)| s.circle_geometry(id))
            .map(|(c, _)| c.x)
            .fold(Real::MIN, Real::max);
        // Relative to the original (free, so the solver may move it too).
        let x0 = s.circle_geometry(hole).unwrap().0.x;
        assert!((far - x0 - 30.0).abs() < 1e-7, "last copy at the new total");

        // Circular, fitted 90° over 4: the angle dimension is the total.
        let mut s = Sketch::default();
        let c = s.add_fixed_point(p(0.0, 0.0));
        let hole = s.add_circle(p(10.0, 0.0), 1.0).unwrap();
        let before = s.clone().solve().unwrap().dof;
        s.circular_pattern(&[hole], c, 4, 90f64.to_radians(), true)
            .unwrap();
        check(&mut s, before);
        let a = s
            .constraints()
            .find_map(|(_, c)| match c {
                Constraint::Angle { value, .. } => Some(*value),
                _ => None,
            })
            .unwrap();
        assert!((a.to_degrees().abs() - 90.0).abs() < 1e-9);
    }

    /// A radial line from the pattern's centre: the copies share the
    /// centre (it failed: "the two points must differ"); 8 sectors.
    #[test]
    fn circular_pattern_of_a_line_from_the_centre() {
        let mut s = Sketch::default();
        let c = s.add_circle(p(0.0, 0.0), 15.0).unwrap();
        let Some(Entity::Circle { center, .. }) = s.entity(c).cloned() else {
            panic!("circle")
        };
        let b = s.add_point(p(20.0, 0.0));
        let l = s.add_line_between(center, b).unwrap();
        s.circular_pattern(&[l], center, 8, TAU, true).unwrap();
        let rep = s.solve().unwrap();
        assert_eq!(rep.redundant, 0);
        assert_eq!(s.regions().len(), 8);
        let at_centre = s
            .entities()
            .filter(|(_, e)| matches!(e, Entity::Line { start, .. } if *start == center))
            .count();
        assert!(at_centre >= 8, "{at_centre} lines start at the centre");
    }

    #[test]
    fn pattern_input_errors() {
        let mut s = Sketch::default();
        let (hx, _) = axes(&mut s);
        let l = s.add_line(p(0.0, 0.0), p(5.0, 0.0));
        let d = PatternDirection {
            line: hx,
            flip: false,
            count: 1,
            spacing: 5.0,
            fitted: false,
        };
        assert!(
            s.rectangular_pattern(&[l], d, None).is_err(),
            "one instance"
        );
        assert!(s.rectangular_pattern(&[], d, None).is_err());
        let c = s.add_fixed_point(p(0.0, 0.0));
        assert!(s.circular_pattern(&[l], c, 1, TAU, false).is_err());
    }
}
