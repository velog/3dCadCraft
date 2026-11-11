//! The 3D sketch in the GUI (`docs/spec/1.10-3d-sketch.md`): Start 3D
//! Sketch, the 3D Sketch panel (Line, Point, Bend, Coincident, Parallel,
//! Perpendicular, Precise Input), its picks in the view and its drawing.
//! The geometry is `part::sketch3d`.

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke};

use super::CadCraftApp;
use super::icons::Icon;
use super::panels::{plabel, psection, warning_box};
use crate::kernel::{Point3, Real, Vector3};
use crate::part::sketch3d::{Constraint3, Sketch3d};

/// The tool running in the open 3D sketch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tool3 {
    /// Connected straight lines.
    Line,
    /// Lone points.
    Point,
    /// A bend at a corner (click its point).
    Bend,
    /// Two points become one.
    Coincident,
    /// Two lines parallel.
    Parallel,
    /// Two lines square.
    Perpendicular,
    /// A three-point arc: its two ends, then a point on it.
    Arc,
    /// A spline through (or controlled by) the clicked points.
    Spline,
    /// A helical curve: its axis point, then its start point.
    Helix,
    /// A model edge copied into the sketch.
    Include,
    /// A point keeps its place (a click toggles it).
    Fix,
    /// Two lines equally long.
    Equal,
    /// A line along X, Y or Z (the panel's choice).
    AlongAxis,
    /// Dimension: a line's length, or the distance of two points.
    Dimension,
    /// Dimension: the angle between two lines.
    AngleDim,
}

impl Tool3 {
    /// Every tool, in the order of the ribbon's actions (`Action::Sk3Tool`).
    pub(super) const ALL: [Tool3; 15] = [
        Tool3::Line,
        Tool3::Point,
        Tool3::Bend,
        Tool3::Coincident,
        Tool3::Parallel,
        Tool3::Perpendicular,
        Tool3::Arc,
        Tool3::Spline,
        Tool3::Helix,
        Tool3::Include,
        Tool3::Fix,
        Tool3::Equal,
        Tool3::AlongAxis,
        Tool3::Dimension,
        Tool3::AngleDim,
    ];

    /// Its index in [`Self::ALL`].
    pub(super) fn index(self) -> u8 {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0) as u8
    }

    fn label(self) -> &'static str {
        match self {
            Tool3::Line => "Line",
            Tool3::Point => "Point",
            Tool3::Bend => "Bend",
            Tool3::Coincident => "Coincident",
            Tool3::Parallel => "Parallel",
            Tool3::Perpendicular => "Perpendicular",
            Tool3::Arc => "Arc",
            Tool3::Spline => "Spline",
            Tool3::Helix => "Helical Curve",
            Tool3::Include => "Include Geometry",
            Tool3::Fix => "Fix",
            Tool3::Equal => "Equal",
            Tool3::AlongAxis => "Parallel to Axis",
            Tool3::Dimension => "Dimension",
            Tool3::AngleDim => "Angle",
        }
    }

    /// The prompt in the status bar.
    pub(super) fn prompt(self) -> &'static str {
        match self {
            Tool3::Line => {
                "3D Line: click a model vertex, a work point, a face or a point of the sketch, or type X / Y / Z and Add Point"
            }
            Tool3::Point => {
                "3D Point: click a model vertex, a work point or a face, or type X / Y / Z"
            }
            Tool3::Bend => "Bend: click the corner point of two lines (the radius is in the panel)",
            Tool3::Coincident => "Coincident: click two points of the 3D sketch",
            Tool3::Parallel => "Parallel: click two lines of the 3D sketch",
            Tool3::Perpendicular => "Perpendicular: click two lines of the 3D sketch",
            Tool3::Arc => {
                "3D Arc: click the start, the end, then a point on the arc (vertices, work points, faces or points of the sketch)"
            }
            Tool3::Spline => {
                "3D Spline: click its points (vertices, work points, faces or points of the sketch), then Create Spline"
            }
            Tool3::Helix => {
                "Helical Curve: click a point on the axis, then the start point (pitch, turns and axis are in the panel)"
            }
            Tool3::Include => "Include Geometry: click a model edge to copy it into the 3D sketch",
            Tool3::Fix => "Fix: click a point of the 3D sketch to fix or free it",
            Tool3::Equal => "Equal: click two lines of the 3D sketch",
            Tool3::AlongAxis => "Parallel to Axis: click a line (the axis is in the panel)",
            Tool3::Dimension => {
                "Dimension: click a line (its length) or two points (their distance), then type the value in the box"
            }
            Tool3::AngleDim => "Angle: click two lines, then type the value in the box",
        }
    }

    /// The constraint tools pick lines.
    fn picks_lines(self) -> bool {
        matches!(
            self,
            Tool3::Parallel
                | Tool3::Perpendicular
                | Tool3::Equal
                | Tool3::AlongAxis
                | Tool3::AngleDim
        )
    }
}

/// The open 3D sketch.
#[derive(Clone, Debug)]
pub(super) struct Sk3dEdit {
    /// Index into `PartDocument::sketches3d`.
    pub index: usize,
    /// The running tool.
    pub tool: Option<Tool3>,
    /// Line: the point the next segment starts at.
    pub last: Option<u32>,
    /// Precise Input: the coordinates typed.
    pub xyz: [Real; 3],
    /// Precise Input: relative to the last point.
    pub relative: bool,
    /// Bend radius (mm).
    pub radius: Real,
    /// Points or lines picked so far by a constraint tool.
    pub picks: Vec<u32>,
    /// Spline: interpolation or control vertex.
    pub spline_kind: crate::part::sketch3d::SplineKind,
    /// Helical Curve: pitch (mm), turns, axis (0 X, 1 Y, 2 Z), clockwise.
    pub helix: (Real, Real, usize, bool),
    /// Parallel to Axis: which axis.
    pub axis: usize,
    /// Dimension / Angle: the value (mm or degrees).
    pub value: Real,
    /// Selected points (no tool running): what 3D Move moves.
    pub sel: Vec<u32>,
    /// The selected constraint glyph (Delete removes it).
    pub sel_cons: Option<usize>,
    /// The dimension edited in place, and whether its box was focused.
    pub dim_edit: Option<(usize, bool)>,
    /// The 3D Move triad, when on.
    pub gizmo: Option<super::sketch3d_view::Gizmo>,
}

impl Sk3dEdit {
    fn new(index: usize) -> Self {
        Self {
            index,
            tool: Some(Tool3::Line),
            last: None,
            xyz: [0.0; 3],
            relative: false,
            radius: 10.0,
            picks: Vec::new(),
            spline_kind: Default::default(),
            helix: (5.0, 3.0, 2, false),
            axis: 2,
            value: 50.0,
            sel: Vec::new(),
            sel_cons: None,
            dim_edit: None,
            gizmo: None,
        }
    }
}

/// What a click in the view hits for the 3D sketch.
enum Hit3 {
    /// A point of the open sketch.
    Own(u32),
    /// A line of the open sketch.
    Line(u32),
    /// A position from the model (vertex, work point: fixed) or a face hit.
    At(Point3, bool),
}

/// Screen distance from `p` to segment `a`–`b`.
fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-9)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

impl CadCraftApp {
    /// **Start 3D Sketch**: a new empty 3D sketch, open with the Line tool.
    pub(super) fn start_sketch3d(&mut self) {
        self.finish_sketch();
        self.cancel_extrude();
        self.mode = super::Mode::Normal;
        self.checkpoint("Start 3D Sketch");
        let part = self.part_mut();
        let n = (1..)
            .find(|n| {
                !part
                    .sketches3d
                    .iter()
                    .any(|s| s.id == format!("Sketch3D{n}") || s.name == format!("3D Sketch{n}"))
            })
            .unwrap_or(1);
        part.sketches3d.push(Sketch3d::new(
            format!("Sketch3D{n}"),
            format!("3D Sketch{n}"),
        ));
        let k = part.sketches3d.len() - 1;
        self.history.break_coalescing();
        self.sketch3d = Some(Sk3dEdit::new(k));
        self.file_status = None;
        self.scene_dirty = true;
        self.sketch3d_tab();
    }

    /// The 3D Sketch tab comes forward (Finish returns to the tab before).
    fn sketch3d_tab(&mut self) {
        use super::ribbon::Tab;
        if self.tab != Tab::Sketch3d {
            self.tab_before_sketch = self.tab;
        }
        self.tab = Tab::Sketch3d;
    }

    /// Edit 3D Sketch `k` (browser double click).
    pub(super) fn edit_sketch3d(&mut self, k: usize) {
        if k >= self.part().sketches3d.len() {
            return;
        }
        self.finish_sketch();
        self.cancel_extrude();
        let mut e = Sk3dEdit::new(k);
        e.tool = None;
        self.sketch3d = Some(e);
        self.sketch3d_tab();
    }

    /// Finish 3D Sketch: closes it; features using it are rebuilt.
    pub(super) fn finish_sketch3d(&mut self) {
        let Some(e) = self.sketch3d.take() else {
            return;
        };
        if self.tab == super::ribbon::Tab::Sketch3d {
            self.tab = if self.tab_before_sketch.shown(false, false)
                && !matches!(
                    self.tab_before_sketch,
                    super::ribbon::Tab::Sketch | super::ribbon::Tab::Sketch3d
                ) {
                self.tab_before_sketch
            } else {
                super::ribbon::Tab::Model
            };
        }
        let Some(id) = self.part().sketches3d.get(e.index).map(|s| s.id.clone()) else {
            return;
        };
        let used = self
            .part()
            .features
            .iter()
            .any(|f| f.sketch3d_ids().contains(&id.as_str()));
        if used {
            if let Err(err) = self.part_mut().regenerate() {
                self.file_status = Some(Err(err.to_string()));
            }
            self.scene_dirty = true;
        }
    }

    /// A tool of the 3D Sketch tab: on (off when it runs already).
    pub(super) fn sketch3d_set_tool(&mut self, i: u8) {
        let Some(t) = Tool3::ALL.get(i as usize).copied() else {
            return;
        };
        let Some(e) = self.sketch3d.as_mut() else {
            self.file_status = Some(Err(format!("{}: open a 3D sketch first", t.label())));
            return;
        };
        e.tool = if e.tool == Some(t) { None } else { Some(t) };
        e.picks.clear();
        e.gizmo = None;
        if e.tool != Some(Tool3::Line) {
            e.last = None;
        }
    }

    /// Esc: stops the running tool, or finishes the sketch when none runs.
    pub(super) fn sketch3d_escape(&mut self) {
        match self.sketch3d.as_mut() {
            Some(e) if e.gizmo.is_some() || e.dim_edit.is_some() => {
                e.gizmo = None;
                e.dim_edit = None;
            }
            Some(e) if e.tool.is_some() => {
                e.tool = None;
                e.last = None;
                e.picks.clear();
            }
            Some(_) => self.finish_sketch3d(),
            None => {}
        }
    }

    /// Delete of 3D sketch `k` (refused while a feature uses it).
    pub(super) fn delete_sketch3d(&mut self, k: usize) {
        let Some(id) = self.part().sketches3d.get(k).map(|s| s.id.clone()) else {
            return;
        };
        if let Some(f) = self
            .part()
            .features
            .iter()
            .find(|f| f.sketch3d_ids().contains(&id.as_str()))
        {
            self.file_status = Some(Err(format!(
                "Delete: {} uses this 3D sketch (delete it first)",
                f.name()
            )));
            return;
        }
        self.sketch3d = None;
        self.checkpoint("Delete");
        self.part_mut().sketches3d.remove(k);
        self.scene_dirty = true;
    }

    /// Visibility of 3D sketch `k`.
    pub(super) fn show_sketch3d(&mut self, k: usize, on: bool) {
        self.checkpoint("Visibility");
        if let Some(s) = self.part_mut().sketches3d.get_mut(k) {
            s.visible = on;
        }
    }

    /// Changes the open 3D sketch through `f` on a copy: one Undo step,
    /// nothing changes when `f` fails (rule 5).
    fn edit3d(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Sketch3d, &mut Sk3dEdit) -> Result<(), String>,
    ) {
        let Some(mut e) = self.sketch3d.clone() else {
            return;
        };
        let Some(mut s) = self.part().sketches3d.get(e.index).cloned() else {
            return;
        };
        match f(&mut s, &mut e) {
            Ok(()) => {
                self.checkpoint(label);
                self.history.break_coalescing();
                if let Some(slot) = self.part_mut().sketches3d.get_mut(e.index) {
                    *slot = s;
                }
                self.sketch3d = Some(e);
                self.file_status = None;
            }
            Err(err) => {
                let err = if err.contains(':') {
                    err
                } else {
                    format!("3D Sketch: {err}")
                };
                self.file_status = Some(Err(err));
                if let Some(x) = self.sketch3d.as_mut() {
                    x.picks.clear();
                }
            }
        }
    }

    /// Adds a point at `at` (Line: and a segment from the last point).
    fn sketch3d_add(&mut self, at: Point3, fixed: bool, own: Option<u32>) {
        let tool = self.sketch3d.as_ref().and_then(|e| e.tool);
        if let Some(t @ (Tool3::Arc | Tool3::Spline)) = tool {
            let hit = match own {
                Some(id) => Hit3::Own(id),
                None => Hit3::At(at, fixed),
            };
            self.sketch3d_pick_point(hit, t);
            return;
        }
        self.edit3d("3D Line", |s, e| {
            let id = own.unwrap_or_else(|| s.add_point(at, fixed));
            if tool == Some(Tool3::Line)
                && let Some(last) = e.last
                && last != id
            {
                s.add_line(last, id)?;
            }
            e.last = Some(id);
            if let Some(p) = s.at(id) {
                e.xyz = [p.x, p.y, p.z];
            }
            Ok(())
        });
    }

    /// Precise Input's Add Point: the typed coordinates (relative to the
    /// last point when chosen).
    fn sketch3d_add_typed(&mut self) {
        let Some(e) = self.sketch3d.as_ref() else {
            return;
        };
        let mut at = Point3::from(e.xyz);
        if e.relative
            && let Some(last) = e.last
            && let Some(p) = self.part().sketches3d.get(e.index).and_then(|s| s.at(last))
        {
            at = p + at.coords;
        }
        self.sketch3d_add(at, false, None);
        // Relative input starts from the new point: the fields go to 0.
        if let Some(e) = self.sketch3d.as_mut()
            && e.relative
        {
            e.xyz = [0.0; 3];
        }
    }

    /// What the cursor is over for the open 3D sketch.
    fn sketch3d_hit(&self, p: Pos2, rect: Rect, lines: bool) -> Option<Hit3> {
        let e = self.sketch3d.as_ref()?;
        let s = self.part().sketches3d.get(e.index)?;
        let screen = |id: u32| s.at(id).and_then(|q| self.camera.project(&q, rect));
        if lines {
            return s
                .lines
                .iter()
                .filter_map(|l| Some((seg_dist(p, screen(l.a)?, screen(l.b)?), l.id)))
                .filter(|(d, _)| *d <= super::overlay::CURVE_SNAP_PX)
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, id)| Hit3::Line(id));
        }
        if let Some((_, id)) = s
            .points
            .iter()
            .filter_map(|q| Some((screen(q.id)?.distance(p), q.id)))
            .filter(|(d, _)| *d <= super::overlay::POINT_SNAP_PX)
            .min_by(|a, b| a.0.total_cmp(&b.0))
        {
            return Some(Hit3::Own(id));
        }
        if let Some(r) = match self.work_axis_or_point_under(p, rect) {
            Some(super::work::WpInput::Point(r)) => Some(r),
            _ => self.vertex_under(p, rect),
        } && let Ok(q) =
            crate::part::placement::point_of(&r, &self.part().body, &self.part().work)
        {
            return Some(Hit3::At(q, true));
        }
        let (o, dir) = self.camera.ray(p, rect)?;
        let dir = dir.try_normalize(1e-12)?;
        let (_, t) = self.picker().pick(&o, &dir)?;
        Some(Hit3::At(o + dir * t, false))
    }

    /// A click in the view while the 3D sketch is open.
    pub(super) fn sketch3d_click(&mut self, p: Pos2, rect: Rect, ctrl: bool) {
        if self.sk3_gizmo_click(p, rect) {
            return;
        }
        let Some(tool) = self.sketch3d.as_ref().and_then(|e| e.tool) else {
            self.sk3_select_click(p, rect, ctrl);
            return;
        };
        let hit = match tool {
            // Dimension: a point of the sketch (a distance), else a line.
            Tool3::Dimension => match self.sketch3d_hit(p, rect, false) {
                Some(h @ Hit3::Own(_)) => Some(h),
                _ => self.sketch3d_hit(p, rect, true),
            },
            _ => self.sketch3d_hit(p, rect, tool.picks_lines()),
        };
        match (tool, hit) {
            (Tool3::Line | Tool3::Point, Some(Hit3::Own(id))) => {
                self.sketch3d_add(Point3::origin(), false, Some(id));
            }
            (Tool3::Line | Tool3::Point, Some(Hit3::At(q, fixed))) => {
                self.sketch3d_add(q, fixed, None);
            }
            (Tool3::Arc | Tool3::Spline, Some(hit @ (Hit3::Own(_) | Hit3::At(..)))) => {
                self.sketch3d_pick_point(hit, tool);
            }
            (Tool3::Bend, Some(Hit3::Own(id))) => {
                self.edit3d("Bend", |s, e| s.add_bend(id, e.radius));
            }
            (Tool3::Helix, Some(hit @ (Hit3::Own(_) | Hit3::At(..)))) => {
                self.edit3d("Helical Curve", |s, e| {
                    let id = match hit {
                        Hit3::Own(id) => id,
                        Hit3::At(q, fixed) => s.add_point(q, fixed),
                        Hit3::Line(_) => return Ok(()),
                    };
                    if e.picks.is_empty() {
                        e.picks.push(id);
                        return Ok(());
                    }
                    let c = e.picks[0];
                    e.picks.clear();
                    let (pitch, turns, k, cw) = e.helix;
                    let mut ax = Vector3::zeros();
                    ax[k.min(2)] = 1.0;
                    let h = s.add_helix(c, ax, id, pitch, turns, cw)?;
                    e.last = s.helices.iter().find(|x| x.id == h).map(|x| x.end);
                    Ok(())
                });
            }
            (Tool3::Fix, Some(Hit3::Own(id))) => {
                self.edit3d("Fix", |s, _| {
                    let on = s.point(id).is_some_and(|p| !p.fixed);
                    s.set_fixed(id, on)
                });
            }
            (Tool3::Equal | Tool3::AngleDim, Some(Hit3::Line(id))) => {
                let first = self
                    .sketch3d
                    .as_ref()
                    .and_then(|e| e.picks.first().copied());
                match first {
                    Some(a) if a != id => self.edit3d(tool.label(), |s, e| {
                        e.picks.clear();
                        s.add_constraint(if tool == Tool3::Equal {
                            Constraint3::Equal(a, id)
                        } else {
                            // The angle as it is: the box opens to change it.
                            let d = |l: u32| {
                                s.line(l)
                                    .and_then(|x| Some(s.at(x.b)? - s.at(x.a)?))
                                    .unwrap_or_else(Vector3::x)
                            };
                            let (x, y) = (d(a), d(id));
                            Constraint3::Angle(a, id, x.cross(&y).norm().atan2(x.dot(&y)))
                        })?;
                        if tool == Tool3::AngleDim {
                            e.dim_edit = Some((s.constraints.len() - 1, false));
                        }
                        Ok(())
                    }),
                    _ => {
                        if let Some(e) = self.sketch3d.as_mut() {
                            e.picks = vec![id];
                        }
                    }
                }
            }
            (Tool3::AlongAxis, Some(Hit3::Line(id))) => {
                self.edit3d("Parallel to Axis", |s, e| {
                    s.add_constraint(Constraint3::AlongAxis(id, e.axis))
                });
            }
            (Tool3::Dimension, Some(Hit3::Line(id))) => {
                self.edit3d("Dimension", |s, e| {
                    // The length as it is: the box opens to change it.
                    let len = s
                        .line(id)
                        .and_then(|x| Some((s.at(x.b)? - s.at(x.a)?).norm()))
                        .unwrap_or(e.value);
                    s.add_constraint(Constraint3::Length(id, len))?;
                    e.dim_edit = Some((s.constraints.len() - 1, false));
                    Ok(())
                });
            }
            (Tool3::Dimension, Some(Hit3::Own(id))) => {
                let first = self
                    .sketch3d
                    .as_ref()
                    .and_then(|e| e.picks.first().copied());
                match first {
                    Some(a) if a != id => self.edit3d("Dimension", |s, e| {
                        e.picks.clear();
                        let d = s
                            .at(a)
                            .zip(s.at(id))
                            .map_or(e.value, |(p, q)| (q - p).norm());
                        s.add_constraint(Constraint3::Distance(a, id, d))?;
                        e.dim_edit = Some((s.constraints.len() - 1, false));
                        Ok(())
                    }),
                    _ => {
                        if let Some(e) = self.sketch3d.as_mut() {
                            e.picks = vec![id];
                        }
                    }
                }
            }
            (Tool3::Include, _) => self.sketch3d_include(p, rect),
            (Tool3::Coincident, Some(Hit3::Own(id))) => {
                let first = self
                    .sketch3d
                    .as_ref()
                    .and_then(|e| e.picks.first().copied());
                match first {
                    Some(a) if a != id => self.edit3d("Coincident", |s, e| {
                        e.picks.clear();
                        s.merge_points(a, id)
                    }),
                    _ => {
                        if let Some(e) = self.sketch3d.as_mut() {
                            e.picks = vec![id];
                        }
                    }
                }
            }
            (Tool3::Parallel | Tool3::Perpendicular, Some(Hit3::Line(id))) => {
                let first = self
                    .sketch3d
                    .as_ref()
                    .and_then(|e| e.picks.first().copied());
                match first {
                    Some(a) if a != id => {
                        let c = if tool == Tool3::Parallel {
                            Constraint3::Parallel(a, id)
                        } else {
                            Constraint3::Perpendicular(a, id)
                        };
                        self.edit3d(tool.label(), |s, e| {
                            e.picks.clear();
                            s.add_constraint(c)
                        });
                    }
                    _ => {
                        if let Some(e) = self.sketch3d.as_mut() {
                            e.picks = vec![id];
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Arc / Spline: the clicked point (made when it is new) joins the
    /// picks; the third arc point makes the arc.
    fn sketch3d_pick_point(&mut self, hit: Hit3, tool: Tool3) {
        let label = tool.label();
        self.edit3d(label, |s, e| {
            let id = match hit {
                Hit3::Own(id) => id,
                Hit3::At(q, fixed) => s.add_point(q, fixed),
                Hit3::Line(_) => return Ok(()),
            };
            if e.picks.last() == Some(&id) {
                return Ok(());
            }
            e.picks.push(id);
            if tool == Tool3::Arc && e.picks.len() == 3 {
                let (a, b, via) = (e.picks[0], e.picks[1], e.picks[2]);
                e.picks.clear();
                s.add_arc(a, via, b)?;
                e.last = Some(b);
            }
            Ok(())
        });
    }

    /// Include Geometry: the model edge under `p` copied in (a line, an
    /// arc through its middle, or a spline through its samples).
    fn sketch3d_include(&mut self, p: Pos2, rect: Rect) {
        let Some(e) = self.edge_under(p, rect) else {
            return;
        };
        let body = &self.part().body;
        let straight = body.edge_direction(e).is_some();
        let Ok(pts) = body.sample_edge(e, &crate::kernel::TessellationOptions::default()) else {
            return;
        };
        if pts.len() < 2 {
            return;
        }
        // A whole circle (one closed edge) goes in as a spline of its samples.
        let closed = (pts[0] - pts[pts.len() - 1]).norm() < 1e-6;
        let circular = !straight
            && !closed
            && matches!(
                body.edge(e).and_then(|x| body.curve(x.curve)),
                Some(crate::kernel::Curve::Circle(_))
            );
        let pick: Vec<Point3> = if straight {
            vec![pts[0], pts[pts.len() - 1]]
        } else if circular {
            vec![pts[0], pts[pts.len() / 2], pts[pts.len() - 1]]
        } else {
            // At most 12 points along it.
            let step = (pts.len() / 11).max(1);
            let mut v: Vec<Point3> = pts.iter().step_by(step).copied().collect();
            if v.last() != pts.last() {
                v.push(pts[pts.len() - 1]);
            }
            v
        };
        self.edit3d("Include Geometry", |s, _| {
            s.include(&pick, circular).map(|_| ())
        });
    }

    /// Create Spline: the picked points as a spline.
    fn sketch3d_make_spline(&mut self) {
        self.edit3d("Spline", |s, e| {
            let pts = std::mem::take(&mut e.picks);
            let last = pts.last().copied();
            s.add_spline(pts, e.spline_kind)?;
            e.last = last;
            Ok(())
        });
    }

    /// The visible 3D sketch whose curves are under `p` (Sweep's path).
    pub(super) fn sketch3d_under(&self, p: Pos2, rect: Rect) -> Option<usize> {
        self.part()
            .sketches3d
            .iter()
            .enumerate()
            .filter(|(_, s)| s.visible)
            .find_map(|(k, s)| {
                s.polylines()
                    .iter()
                    .any(|pl| {
                        pl.windows(2).any(|w| {
                            match (
                                self.camera.project(&w[0], rect),
                                self.camera.project(&w[1], rect),
                            ) {
                                (Some(a), Some(b)) => {
                                    seg_dist(p, a, b) <= super::overlay::CURVE_SNAP_PX
                                }
                                _ => false,
                            }
                        })
                    })
                    .then_some(k)
            })
    }

    /// Draws the visible 3D sketches (the open one with its points, picks
    /// and what the cursor would take).
    pub(super) fn draw_sketches3d(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        pointer: Option<Pos2>,
    ) {
        let c = super::theme::canvas();
        let part = self.part();
        let open = self.sketch3d.as_ref();
        for (k, s) in part.sketches3d.iter().enumerate() {
            let editing = open.is_some_and(|e| e.index == k);
            if !s.visible && !editing {
                continue;
            }
            let col = if editing { c.sketch_open } else { c.reference };
            for pl in s.polylines() {
                let pts: Vec<Pos2> = pl
                    .iter()
                    .filter_map(|q| self.camera.project(q, rect))
                    .collect();
                painter.add(egui::Shape::line(pts, Stroke::new(2.0, col)));
            }
            if !editing {
                continue;
            }
            let Some(e) = open else { continue };
            let picked_lines = e.tool.is_some_and(Tool3::picks_lines);
            for l in &s.lines {
                if picked_lines
                    && e.picks.contains(&l.id)
                    && let (Some(a), Some(b)) = (s.at(l.a), s.at(l.b))
                    && let (Some(a), Some(b)) =
                        (self.camera.project(&a, rect), self.camera.project(&b, rect))
                {
                    painter.line_segment([a, b], Stroke::new(4.0, c.selected_edge));
                }
            }
            for q in &s.points {
                let Some(sp) = self.camera.project(&Point3::from(q.at), rect) else {
                    continue;
                };
                let picked = !picked_lines && e.picks.contains(&q.id);
                let bent = s.bends.iter().any(|b| b.corner == q.id);
                let fill = if picked || e.last == Some(q.id) {
                    c.selected_edge
                } else if q.fixed {
                    c.sketch_full
                } else {
                    col
                };
                painter.circle_filled(sp, if bent { 3.0 } else { 4.0 }, fill);
                if bent {
                    painter.circle_stroke(sp, 6.0, Stroke::new(1.5, col));
                }
            }
            // What a click would take.
            if let (Some(tool), Some(p)) = (e.tool, pointer)
                && rect.contains(p)
            {
                let hover = super::theme::hover_edge();
                match self.sketch3d_hit(p, rect, tool.picks_lines()) {
                    Some(Hit3::Own(id)) => {
                        if let Some(sp) = s.at(id).and_then(|q| self.camera.project(&q, rect)) {
                            painter.circle_stroke(sp, 7.0, Stroke::new(2.0, hover));
                        }
                    }
                    Some(Hit3::Line(id)) => {
                        if let Some(l) = s.line(id)
                            && let (Some(a), Some(b)) = (s.at(l.a), s.at(l.b))
                            && let (Some(a), Some(b)) =
                                (self.camera.project(&a, rect), self.camera.project(&b, rect))
                        {
                            painter.line_segment([a, b], Stroke::new(4.0, hover));
                        }
                    }
                    Some(Hit3::At(q, _)) if matches!(tool, Tool3::Line | Tool3::Point) => {
                        if let Some(sp) = self.camera.project(&q, rect) {
                            painter.circle_stroke(sp, 5.0, Stroke::new(2.0, hover));
                            // The rubber band from the last point.
                            if tool == Tool3::Line
                                && let Some(last) = e.last.and_then(|id| s.at(id))
                                && let Some(lp) = self.camera.project(&last, rect)
                            {
                                painter.line_segment([lp, sp], Stroke::new(1.5, with_alpha(col)));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// The 3D Sketch panel in Properties.
    pub(super) fn sketch3d_panel(&mut self, ui: &mut egui::Ui) {
        let Some(mut e) = self.sketch3d.clone() else {
            return;
        };
        let Some(s) = self.part().sketches3d.get(e.index).cloned() else {
            self.sketch3d = None;
            return;
        };
        self.panel_header(ui, Icon::Sketch3d, &s.name);
        let mut add_typed = false;
        let mut restart = false;
        let mut finish = false;
        // The tools are on the 3D Sketch tab (the ribbon); the panel holds
        // the running tool's options and the selection.
        let mut chain = false;
        let mut move_it = false;
        let mut delete_cons = false;
        psection(ui, "sk3_sel", "Selection", |ui| {
            plabel(ui, "Tool");
            ui.label(e.tool.map_or("Select (Esc)", Tool3::label));
            ui.end_row();
            plabel(ui, "Selected");
            ui.label(format!("{} point(s)", e.sel.len()));
            ui.end_row();
            plabel(ui, "");
            ui.vertical(|ui| {
                if ui
                    .add_enabled(!e.sel.is_empty(), egui::Button::new("Select Chain"))
                    .on_hover_text("Every point of the chains through the selection")
                    .clicked()
                {
                    chain = true;
                }
                if ui
                    .add_enabled(!e.sel.is_empty(), egui::Button::new("3D Move (M)"))
                    .on_hover_text("The triad on the selection: drag an arrow or a square, or click an arrow to type the distance")
                    .clicked()
                {
                    move_it = true;
                }
            });
            ui.end_row();
            if let Some(k) = e.sel_cons
                && let Some(c) = s.constraints.get(k)
            {
                plabel(ui, "Constraint");
                ui.horizontal(|ui| {
                    ui.label(c.label());
                    if ui
                        .small_button("Delete")
                        .on_hover_text("Delete (Del)")
                        .clicked()
                    {
                        delete_cons = true;
                    }
                });
                ui.end_row();
            }
        });
        if e.sel_cons.is_some()
            && ui.ctx().memory(|m| m.focused().is_none())
            && ui.input(|i| i.key_pressed(egui::Key::Delete))
        {
            delete_cons = true;
        }
        if matches!(
            e.tool,
            Some(Tool3::Line | Tool3::Point | Tool3::Arc | Tool3::Spline)
        ) {
            psection(ui, "sk3_precise", "Precise Input", |ui| {
                plabel(ui, "Mode");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut e.relative, false, "Absolute")
                        .on_hover_text("X / Y / Z in the part's coordinates");
                    ui.selectable_value(&mut e.relative, true, "Relative")
                        .on_hover_text("dX / dY / dZ from the last point");
                });
                ui.end_row();
                let names = if e.relative {
                    ["dX", "dY", "dZ"]
                } else {
                    ["X", "Y", "Z"]
                };
                for (k, n) in names.iter().enumerate() {
                    plabel(ui, n);
                    ui.add(
                        super::valuefield::ExprValue::new(&mut e.xyz[k])
                            .suffix(" mm")
                            .cursor(n),
                    );
                    ui.end_row();
                }
                plabel(ui, "");
                ui.horizontal(|ui| {
                    if ui
                        .button("Add Point")
                        .on_hover_text("A point at these coordinates (Line: and a segment to it)")
                        .clicked()
                    {
                        add_typed = true;
                    }
                    if e.tool == Some(Tool3::Line)
                        && ui
                            .button("Restart")
                            .on_hover_text("The next point starts a new line")
                            .clicked()
                    {
                        restart = true;
                    }
                });
                ui.end_row();
            });
        }
        if e.tool == Some(Tool3::Helix) {
            psection(ui, "sk3_helix", "Helical Curve", |ui| {
                plabel(ui, "Axis");
                ui.horizontal(|ui| {
                    for (k, n) in ["X", "Y", "Z"].iter().enumerate() {
                        ui.selectable_value(&mut e.helix.2, k, *n);
                    }
                });
                ui.end_row();
                plabel(ui, "Pitch");
                ui.add(
                    super::valuefield::ExprValue::new(&mut e.helix.0)
                        .range(0.001..=1e6)
                        .suffix(" mm")
                        .cursor("Pitch"),
                );
                ui.end_row();
                plabel(ui, "Revolution");
                ui.add(
                    super::valuefield::ExprValue::new(&mut e.helix.1)
                        .range(0.001..=1e4)
                        .cursor("Revolution"),
                );
                ui.end_row();
                plabel(ui, "Rotation");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut e.helix.3, false, "CCW");
                    ui.selectable_value(&mut e.helix.3, true, "CW");
                });
                ui.end_row();
            });
        }
        if e.tool == Some(Tool3::AlongAxis) {
            psection(ui, "sk3_axis", "Parallel to Axis", |ui| {
                plabel(ui, "Axis");
                ui.horizontal(|ui| {
                    for (k, n) in ["X", "Y", "Z"].iter().enumerate() {
                        ui.selectable_value(&mut e.axis, k, *n);
                    }
                });
                ui.end_row();
            });
        }
        let mut remove: Option<usize> = None;
        if !s.constraints.is_empty() {
            psection(ui, "sk3_cons", "Constraints", |ui| {
                for (k, c) in s.constraints.iter().enumerate() {
                    plabel(ui, &format!("{}", k + 1));
                    ui.horizontal(|ui| {
                        ui.label(c.label());
                        if ui
                            .small_button("x")
                            .on_hover_text("Delete this constraint")
                            .clicked()
                        {
                            remove = Some(k);
                        }
                    });
                    ui.end_row();
                }
            });
        }
        let mut make_spline = false;
        if e.tool == Some(Tool3::Spline) {
            psection(ui, "sk3_spline", "Spline", |ui| {
                plabel(ui, "Type");
                ui.horizontal(|ui| {
                    use crate::part::sketch3d::SplineKind;
                    ui.selectable_value(
                        &mut e.spline_kind,
                        SplineKind::Interpolation,
                        "Interpolation",
                    )
                    .on_hover_text("Through every point");
                    ui.selectable_value(
                        &mut e.spline_kind,
                        SplineKind::ControlVertex,
                        "Control Vertex",
                    )
                    .on_hover_text(
                        "The points pull the curve (it passes through the first and last)",
                    );
                });
                ui.end_row();
                plabel(ui, "Points");
                ui.label(format!("{} picked", e.picks.len()));
                ui.end_row();
                plabel(ui, "");
                if ui
                    .add_enabled(e.picks.len() >= 2, egui::Button::new("Create Spline"))
                    .on_hover_text("The picked points as a spline")
                    .clicked()
                {
                    make_spline = true;
                }
                ui.end_row();
            });
        }
        if e.tool == Some(Tool3::Bend) {
            psection(ui, "sk3_bend", "Bend", |ui| {
                plabel(ui, "Radius");
                ui.add(
                    super::valuefield::ExprValue::new(&mut e.radius)
                        .range(0.0..=1e6)
                        .suffix(" mm")
                        .cursor("Radius"),
                );
                ui.end_row();
                plabel(ui, "Equal");
                ui.add_enabled(false, egui::Checkbox::new(&mut false, ""))
                    .on_disabled_hover_text("Comes later (spec 1.10 Bend: Equal)");
                ui.end_row();
            });
        }
        psection(ui, "sk3_info", "3D Sketch", |ui| {
            plabel(ui, "Contents");
            ui.label(format!(
                "{} point(s), {} line(s), {} bend(s), {} constraint(s)",
                s.points.len(),
                s.lines.len(),
                s.bends.len(),
                s.constraints.len()
            ));
            ui.end_row();
            plabel(ui, "Path");
            match s.pieces() {
                Ok(_) => ui.label(format!("one chain, {:.3} mm", s.length())),
                Err(err) => ui.label(RichText::new(err).weak()),
            };
            ui.end_row();
        });
        if let Some(Err(err)) = &self.file_status
            && err.starts_with("3D")
        {
            warning_box(ui, err);
        }
        super::theme::panel_footer(ui, |ui| {
            if ui
                .add(super::theme::ok_button("Finish 3D Sketch").min_size(egui::vec2(60.0, 22.0)))
                .on_hover_text("Esc with no tool running")
                .clicked()
            {
                finish = true;
            }
        });
        if restart {
            e.last = None;
        }
        self.sketch3d = Some(e);
        if add_typed {
            self.sketch3d_add_typed();
        }
        if make_spline {
            self.sketch3d_make_spline();
        }
        if chain {
            self.sk3_select_chain();
        }
        if move_it {
            self.sk3_start_move();
        }
        if delete_cons {
            self.sk3_delete_selected_constraint();
        }
        if let Some(k) = remove {
            self.edit3d("Delete Constraint", |s, _| {
                s.remove_constraint(k);
                Ok(())
            });
        }
        if finish {
            self.finish_sketch3d();
        }
    }
}

fn with_alpha(c: Color32) -> Color32 {
    super::theme::with_alpha(c, 140)
}
