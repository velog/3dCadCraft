//! The dialog of the sketch pattern commands (Rectangular, Circular,
//! Pattern along Path, Pattern at Points), as Inventor's pattern dialogs:
//! selectors (Geometry, Direction, Centre, Path, Base, Targets) that clicks
//! in the graphics window fill one after the other, values, a live
//! preview and OK / Cancel. Nothing is created before OK
//! (`docs/spec/0-ui.md` §2.9).

use super::sketch_tools::{Pick, SketchTools, Snap, Tool};
use crate::kernel::Real;
use crate::sketch::{
    Constraint, Entity, EntityId, PatternDirection, Point2, PointId, Shape, Sketch, SketchError,
    Style, Vector2,
};

/// A selector of the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// The curves to pattern.
    Geometry,
    /// Direction 1 (line or sketch axis).
    Dir1,
    /// Direction 2 (optional).
    Dir2,
    /// Centre point (or by radius).
    Center,
    /// The path curve.
    Path,
    /// Base point of the geometry.
    Base,
    /// Target points.
    Targets,
}

impl Slot {
    /// Label in the dialog.
    pub fn label(self) -> &'static str {
        match self {
            Slot::Geometry => "Geometry",
            Slot::Dir1 => "Direction 1",
            Slot::Dir2 => "Direction 2",
            Slot::Center => "Centre",
            Slot::Path => "Path",
            Slot::Base => "Base point",
            Slot::Targets => "Targets",
        }
    }

    /// Clicks pick curves (else points).
    pub fn wants_curves(self) -> bool {
        matches!(self, Slot::Geometry | Slot::Dir1 | Slot::Dir2 | Slot::Path)
    }
}

/// A direction: a sketch line or a sketch axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirRef {
    /// A line of the sketch.
    Line(EntityId),
    /// The sketch X axis.
    AxisX,
    /// The sketch Y axis.
    AxisY,
}

/// The centre of a circular pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CenterRef {
    /// A point of the sketch.
    Point(PointId),
    /// A new point at `circular_radius` / `circular_ref_deg` from the
    /// geometry's first point.
    Radius,
}

/// A target of Pattern at Points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TargetRef {
    /// An existing point.
    Point(PointId),
    /// A new point here.
    At(Point2),
}

/// The open pattern dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct PatternDialog {
    /// Which command (`PatternRect`, `PatternCircular`, `PatternPath`,
    /// `PatternPoints`).
    pub tool: Tool,
    /// The selector clicks go to.
    pub active: Slot,
    /// Direction 1 and 2 (Rectangular).
    pub dir: [Option<DirRef>; 2],
    /// Centre (Circular).
    pub center: Option<CenterRef>,
    /// Path (Pattern along Path).
    pub path: Option<EntityId>,
    /// Base point and targets (Pattern at Points).
    pub base: Option<PointId>,
    /// Targets.
    pub targets: Vec<TargetRef>,
}

impl PatternDialog {
    /// The selectors of a command, in order.
    pub fn slots(tool: Tool) -> &'static [Slot] {
        match tool {
            Tool::PatternRect => &[Slot::Geometry, Slot::Dir1, Slot::Dir2],
            Tool::PatternCircular => &[Slot::Geometry, Slot::Center],
            Tool::PatternPath => &[Slot::Geometry, Slot::Path],
            _ => &[Slot::Geometry, Slot::Base, Slot::Targets],
        }
    }

    /// A new dialog; `has_geometry`: a pre-selection exists.
    pub fn new(tool: Tool, has_geometry: bool) -> Self {
        let mut d = Self {
            tool,
            active: Slot::Geometry,
            dir: [None, None],
            center: None,
            path: None,
            base: None,
            targets: Vec::new(),
        };
        if has_geometry {
            d.active = Self::slots(tool)[1];
        }
        d
    }

    fn is_set(&self, s: Slot, geometry: bool) -> bool {
        match s {
            Slot::Geometry => geometry,
            Slot::Dir1 => self.dir[0].is_some(),
            Slot::Dir2 => self.dir[1].is_some(),
            Slot::Center => self.center.is_some(),
            Slot::Path => self.path.is_some(),
            Slot::Base => self.base.is_some(),
            Slot::Targets => !self.targets.is_empty(),
        }
    }

    /// Moves to the next selector that is still empty (Direction 2 and
    /// Targets are optional / multiple, so they stay active).
    fn advance(&mut self, geometry: bool) {
        let slots = Self::slots(self.tool);
        let here = slots.iter().position(|&s| s == self.active).unwrap_or(0);
        if let Some(&next) = slots[here + 1..]
            .iter()
            .find(|&&s| !self.is_set(s, geometry))
        {
            self.active = next;
        } else if let Some(&last) = slots.last()
            && here + 1 < slots.len()
        {
            self.active = last;
        }
    }

    /// Everything required is chosen.
    pub fn ready(&self, geometry: bool) -> bool {
        geometry
            && match self.tool {
                Tool::PatternRect => self.dir[0].is_some(),
                Tool::PatternCircular => self.center.is_some(),
                Tool::PatternPath => self.path.is_some(),
                _ => self.base.is_some() && !self.targets.is_empty(),
            }
    }

    /// Status-bar prompt for the active selector.
    pub fn prompt(&self) -> &'static str {
        match self.active {
            Slot::Geometry => "Select the geometry to pattern (click curves; Enter: next)",
            Slot::Dir1 => "Direction 1: click a line (or X / Y in the dialog)",
            Slot::Dir2 => "Direction 2 (optional): click a line, or OK",
            Slot::Center => "Centre: click a point (or 'By radius' in the dialog)",
            Slot::Path => "Path: click a line, arc or spline",
            Slot::Base => "Base point: click a point of the geometry",
            Slot::Targets => "Targets: click points (empty space adds one); OK when done",
        }
    }
}

impl SketchTools {
    /// A click in the graphics window while the dialog is open: fills the
    /// active selector.
    pub fn dialog_pick(&mut self, sk: &Sketch, pick: Pick) -> Result<(), String> {
        let Some(mut d) = self.pattern_dialog.clone() else {
            return Ok(());
        };
        let curve = match pick.snap {
            Snap::OnEntity(e) | Snap::Midpoint(e) => Some(e),
            _ => None,
        };
        let point = match pick.snap {
            Snap::Point(p) => Some(p),
            _ => None,
        };
        match d.active {
            Slot::Geometry => {
                let e = curve.ok_or("Click a curve to add or remove it")?;
                if d.dir.contains(&Some(DirRef::Line(e))) || d.path == Some(e) {
                    return Err("That curve is the direction/path".into());
                }
                match self.selected.iter().position(|&x| x == e) {
                    Some(k) => {
                        self.selected.remove(k);
                    }
                    None => self.selected.push(e),
                }
            }
            Slot::Dir1 | Slot::Dir2 => {
                let e = curve.ok_or("Click a line")?;
                if !sk.entity(e).is_some_and(Entity::is_line) {
                    return Err("The direction must be a line".into());
                }
                if self.selected.contains(&e) {
                    return Err("The direction cannot be part of the pattern".into());
                }
                let k = usize::from(d.active == Slot::Dir2);
                d.dir[k] = Some(DirRef::Line(e));
                if k == 1 && self.pattern_count[1] < 2 {
                    self.pattern_count[1] = 2;
                }
                d.advance(!self.selected.is_empty());
            }
            Slot::Path => {
                let e = curve.ok_or("Click the path curve")?;
                if sk.entity(e).is_none_or(|x| x.endpoints().is_empty()) {
                    return Err("The path must be a line, an arc or a spline".into());
                }
                if self.selected.contains(&e) {
                    return Err("The path cannot be part of the pattern".into());
                }
                d.path = Some(e);
                d.advance(true);
            }
            Slot::Center => {
                d.center = Some(CenterRef::Point(point.ok_or("Click a point")?));
                d.advance(true);
            }
            Slot::Base => {
                d.base = Some(point.ok_or("Click a point")?);
                d.advance(true);
            }
            Slot::Targets => d.targets.push(match point {
                Some(p) => TargetRef::Point(p),
                None => TargetRef::At(pick.pos),
            }),
        }
        self.pattern_dialog = Some(d);
        Ok(())
    }

    /// Enter in the dialog: from Geometry to the next selector, else OK
    /// when ready (returns true when the pattern should be created).
    pub fn dialog_enter(&mut self) -> bool {
        let geometry = !self.selected.is_empty();
        let Some(d) = self.pattern_dialog.as_mut() else {
            return false;
        };
        if d.active == Slot::Geometry && geometry {
            d.advance(true);
            return false;
        }
        d.ready(geometry)
    }

    /// Builds the pattern described by the dialog into `sk` (helpers such
    /// as axis lines, a radius centre or new target points included).
    pub fn dialog_build(&self, sk: &mut Sketch) -> Result<Shape, SketchError> {
        let d = self
            .pattern_dialog
            .as_ref()
            .ok_or(SketchError::InvalidValue("no pattern dialog"))?;
        let sel = self.selected.clone();
        if !d.ready(!sel.is_empty()) {
            return Err(SketchError::InvalidValue("select the inputs first"));
        }
        let centre =
            selection_centre(sk, &sel).ok_or(SketchError::InvalidValue("nothing to pattern"))?;
        match d.tool {
            Tool::PatternRect => {
                let mut dirs = Vec::new();
                for k in 0..2 {
                    let Some(r) = d.dir[k] else { continue };
                    if k == 1 && self.pattern_count[1] < 2 {
                        continue;
                    }
                    let line = match r {
                        DirRef::Line(l) => l,
                        DirRef::AxisX | DirRef::AxisY => {
                            let v = if r == DirRef::AxisX {
                                Vector2::x()
                            } else {
                                Vector2::y()
                            };
                            let l = sk.add_line(centre - v * 5.0, centre + v * 5.0);
                            sk.set_style(l, Style::Construction)?;
                            let (a, b) = sk.line_points(l)?;
                            sk.set_fixed(a, true)?;
                            sk.set_fixed(b, true)?;
                            l
                        }
                    };
                    let spacing = if self.pattern_fitted[k] && self.pattern_count[k] > 1 {
                        self.pattern_spacing[k] / (self.pattern_count[k] - 1) as Real
                    } else {
                        self.pattern_spacing[k]
                    };
                    dirs.push(PatternDirection {
                        line,
                        flip: self.pattern_flip[k],
                        count: self.pattern_count[k],
                        spacing,
                        fitted: self.pattern_fitted[k],
                    });
                }
                sk.rectangular_pattern(&sel, dirs[0], dirs.get(1).copied())
            }
            Tool::PatternCircular => {
                let center = match d
                    .center
                    .ok_or(SketchError::InvalidValue("pick the centre"))?
                {
                    CenterRef::Point(p) => p,
                    CenterRef::Radius => {
                        let p0 = *sk
                            .points_of(&sel)
                            .first()
                            .ok_or(SketchError::InvalidValue("nothing to pattern"))?;
                        let from = sk.point(p0).ok_or(SketchError::UnknownPoint)?.position;
                        let r = self.circular_radius;
                        let t = self.circular_ref_deg.to_radians();
                        let c = sk.add_point(from + Vector2::new(t.cos(), t.sin()) * r);
                        let l = sk.add_line_between(p0, c)?;
                        sk.set_style(l, Style::Construction)?;
                        sk.add_constraint(Constraint::Distance {
                            a: p0,
                            b: c,
                            value: r,
                        })?;
                        c
                    }
                };
                let n = self.circular_count.max(2) as Real;
                let total = if self.circular_fitted {
                    self.circular_angle_deg
                } else if (self.circular_angle_deg * n - 360.0).abs() < 1e-6 {
                    360.0
                } else {
                    self.circular_angle_deg * (n - 1.0)
                };
                let sign = if self.circular_flip { -1.0 } else { 1.0 };
                sk.circular_pattern(
                    &sel,
                    center,
                    self.circular_count,
                    sign * total.to_radians(),
                    self.circular_fitted,
                )
            }
            Tool::PatternPath => sk.path_pattern(
                &sel,
                d.path.ok_or(SketchError::InvalidValue("pick the path"))?,
                self.path_count,
                self.path_spacing,
            ),
            _ => {
                let targets: Vec<PointId> = d
                    .targets
                    .iter()
                    .map(|t| match *t {
                        TargetRef::Point(p) => p,
                        TargetRef::At(q) => sk.add_point(q),
                    })
                    .collect();
                sk.points_pattern(
                    &sel,
                    d.base
                        .ok_or(SketchError::InvalidValue("pick the base point"))?,
                    &targets,
                )
            }
        }
    }
}

/// The average of the points of some curves.
pub fn selection_centre(sk: &Sketch, sel: &[EntityId]) -> Option<Point2> {
    let pts = sk.points_of(sel);
    if pts.is_empty() {
        return None;
    }
    let sum = pts
        .iter()
        .filter_map(|&q| sk.point(q))
        .fold(Vector2::zeros(), |a, q| a + q.position.coords);
    Some(Point2::from(sum / pts.len() as Real))
}
