//! 2D overlay drawn with egui's painter on top of the 3D image: sketches
//! (with projected reference geometry), the sketch coordinate frame and
//! grid, origin planes and face highlights while picking a sketch plane.
//!
//! Drawing sketches here (not in wgpu) keeps them visible on top of the
//! model, as in Inventor's sketch mode.

use std::f64::consts::TAU;

use eframe::egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke};

use super::camera::OrbitCamera;
use crate::kernel::{Plane, Point3, Real, TriangleMesh, Vector3};
use crate::part::{OriginPlane, PartSketch};
use crate::sketch::{Constraint, Entity, EntityId, Point2, PointId, Sketch, Style, Vector2};

/// Colour of projected (reference) geometry.
pub const REFERENCE: Color32 = Color32::from_rgb(200, 140, 0);
/// Highlight colour for hover/pre-selection.
pub const HIGHLIGHT: Color32 = Color32::from_rgb(255, 140, 0);

/// How to draw one sketch.
pub struct SketchStyle {
    /// How dimension texts are shown.
    pub dim_display: crate::sketch::DimDisplay,
    /// Colour of the sketch's own geometry.
    pub color: Color32,
    /// Colour of fully constrained entities.
    pub fully_color: Color32,
    /// Entities with no freedom left.
    pub fully: Vec<EntityId>,
    /// Degrees-of-freedom glyphs to draw (Show All Degrees of Freedom).
    pub dof: Option<crate::sketch::DofAnalysis>,
    /// Constraint under the cursor (its label and geometry are highlighted).
    pub hovered_constraint: Option<crate::sketch::ConstraintId>,
    /// Selected constraints.
    pub selected_constraints: Vec<crate::sketch::ConstraintId>,
    /// Colour of its projected reference geometry.
    pub reference: Color32,
    /// Show constraint/dimension labels.
    pub show_labels: bool,
    /// Point to highlight (hovered or dragged).
    pub highlighted: Option<PointId>,
    /// Entities to highlight (selected or hovered).
    pub selected: Vec<EntityId>,
}

/// Projects a sketch-plane point to the screen.
fn screen(cam: &OrbitCamera, rect: Rect, sketch: &Sketch, p: Point2) -> Option<Pos2> {
    cam.project(&sketch.to_world(&p), rect)
}

fn pos(sketch: &Sketch, id: PointId) -> Point2 {
    sketch.point(id).map_or(Point2::origin(), |p| p.position)
}

/// Points along an arc from `start` counter-clockwise to `end` around `center`.
fn arc_points(center: Point2, start: Point2, end: Point2) -> Vec<Point2> {
    let r = (start - center).norm();
    let a0 = (start.y - center.y).atan2(start.x - center.x);
    let mut a1 = (end.y - center.y).atan2(end.x - center.x);
    if a1 <= a0 {
        a1 += TAU;
    }
    let n = (((a1 - a0) / TAU) * 96.0).ceil().max(2.0) as usize;
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f64 / n as f64;
            center + r * Vector2::new(a.cos(), a.sin())
        })
        .collect()
}

/// The 2D polyline of an entity.
fn entity_polyline(sketch: &Sketch, e: &Entity) -> Vec<Point2> {
    match e {
        Entity::Line { start, end } => vec![pos(sketch, *start), pos(sketch, *end)],
        Entity::Circle { center, radius } => {
            let c = pos(sketch, *center);
            (0..=96)
                .map(|i| {
                    let a = TAU * i as f64 / 96.0;
                    c + *radius * Vector2::new(a.cos(), a.sin())
                })
                .collect()
        }
        Entity::Arc { center, start, end } => {
            arc_points(pos(sketch, *center), pos(sketch, *start), pos(sketch, *end))
        }
        Entity::Spline { .. } => Vec::new(), // needs the id; see `polyline`
    }
}

/// The 2D polyline of entity `id` (splines sampled densely).
pub fn polyline(sketch: &Sketch, id: EntityId) -> Vec<Point2> {
    match sketch.entity(id) {
        Some(Entity::Spline { .. }) => sketch.spline_curve(id).map_or_else(Vec::new, |c| {
            (0..=96)
                .map(|i| {
                    let p = c.point_at(i as f64 / 96.0);
                    Point2::new(p.x, p.y)
                })
                .collect()
        }),
        Some(e) => entity_polyline(sketch, e),
        None => Vec::new(),
    }
}

/// Midpoint of a line entity, or centre of a circular one.
fn entity_anchor(sketch: &Sketch, id: crate::sketch::EntityId) -> Option<Point2> {
    Some(match sketch.entity(id)? {
        Entity::Line { start, end } => nalgebra::center(&pos(sketch, *start), &pos(sketch, *end)),
        Entity::Circle { center, .. } | Entity::Arc { center, .. } => pos(sketch, *center),
        Entity::Spline { points } => pos(sketch, points[points.len() / 2]),
    })
}

/// Draws a sketch: its own geometry (fully constrained entities in
/// `style.fully`), projected reference geometry dashed, points, degrees of
/// freedom glyphs and constraint labels (hovered/selected ones highlighted,
/// with the geometry they constrain).
pub fn draw_sketch(
    painter: &Painter,
    cam: &OrbitCamera,
    rect: Rect,
    ps: &PartSketch,
    style: &SketchStyle,
) {
    let sketch = &ps.sketch;
    let to_screen = |p: Point2| screen(cam, rect, sketch, p);

    // Geometry referenced by the hovered/selected constraints.
    let mut hot_entities = style.selected.clone();
    let mut hot_points = Vec::new();
    for cid in style
        .hovered_constraint
        .iter()
        .chain(&style.selected_constraints)
    {
        if let Some(c) = sketch.constraint(*cid) {
            let (pts, ents) = constraint_refs(c);
            hot_points.extend(pts);
            hot_entities.extend(ents);
        }
    }

    for (id, _) in sketch.entities() {
        let pts: Option<Vec<Pos2>> = polyline(sketch, id).into_iter().map(to_screen).collect();
        let Some(pts) = pts else { continue };
        let hot = hot_entities.contains(&id);
        let base = if style.fully.contains(&id) {
            style.fully_color
        } else {
            style.color
        };
        let color = if hot { HIGHLIGHT } else { base };
        let width = if hot { 3.0 } else { 2.0 };
        if ps.reference_lines.contains(&id) {
            let c = if hot { HIGHLIGHT } else { style.reference };
            for w in pts.windows(2) {
                painter.extend(Shape::dashed_line(
                    &[w[0], w[1]],
                    Stroke::new(1.5, c),
                    6.0,
                    4.0,
                ));
            }
            continue;
        }
        match sketch.style(id) {
            Style::Normal => {
                painter.add(Shape::line(pts, Stroke::new(width, color)));
            }
            Style::Construction => {
                painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, color), 5.0, 4.0));
            }
            Style::Centerline => {
                painter.extend(Shape::dashed_line(&pts, Stroke::new(1.2, color), 14.0, 4.0));
            }
        }
        // Control frame of a control-vertex spline.
        if let Some(Entity::Spline { points }) = sketch.entity(id)
            && !sketch.is_interp_spline(id)
        {
            let frame: Option<Vec<Pos2>> =
                points.iter().map(|&q| to_screen(pos(sketch, q))).collect();
            if let Some(frame) = frame {
                painter.extend(Shape::dashed_line(
                    &frame,
                    Stroke::new(1.0, Color32::from_gray(150)),
                    3.0,
                    3.0,
                ));
            }
        }
    }

    for (id, p) in sketch.points() {
        let Some(s) = to_screen(p.position) else {
            continue;
        };
        if Some(id) == style.highlighted || hot_points.contains(&id) {
            painter.circle_filled(s, 6.0, HIGHLIGHT);
        } else if ps.is_projected(id) {
            painter.rect_filled(
                Rect::from_center_size(s, [6.0, 6.0].into()),
                0.0,
                style.reference,
            );
        } else if p.fixed {
            painter.rect_filled(
                Rect::from_center_size(s, [7.0, 7.0].into()),
                0.0,
                Color32::BLACK,
            );
        } else if p.style == crate::sketch::PointStyle::Center {
            // Inventor's Center Point: a cross.
            let k = Stroke::new(2.0, style.color);
            painter.line_segment(
                [
                    s - eframe::egui::vec2(5.0, 0.0),
                    s + eframe::egui::vec2(5.0, 0.0),
                ],
                k,
            );
            painter.line_segment(
                [
                    s - eframe::egui::vec2(0.0, 5.0),
                    s + eframe::egui::vec2(0.0, 5.0),
                ],
                k,
            );
        } else {
            painter.circle_filled(s, 3.5, style.color);
        }
    }

    if let Some(dof) = &style.dof {
        draw_dof(painter, cam, rect, ps, dof);
    }

    if style.show_labels {
        for (id, c) in sketch.constraints() {
            if c.is_dimension() {
                let color = if style.selected_constraints.contains(&id)
                    || style.hovered_constraint == Some(id)
                {
                    HIGHLIGHT
                } else if sketch.is_driven(id) {
                    Color32::from_gray(120)
                } else {
                    DIMENSION
                };
                draw_dimension(painter, cam, rect, sketch, id, color);
            }
        }
        for l in label_layout(painter, cam, rect, sketch, style.dim_display) {
            let hot = style.hovered_constraint == Some(l.constraint);
            let sel = style.selected_constraints.contains(&l.constraint);
            let fill = if sel {
                Color32::from_rgb(255, 170, 60)
            } else if hot {
                Color32::from_rgb(255, 215, 140)
            } else {
                Color32::from_rgba_unmultiplied(255, 255, 230, 220)
            };
            painter.rect_filled(l.rect, 3.0, fill);
            if sel || hot {
                painter.rect_stroke(
                    l.rect,
                    3.0,
                    Stroke::new(1.0, HIGHLIGHT),
                    eframe::egui::StrokeKind::Outside,
                );
            }
            painter.text(
                l.rect.center(),
                Align2::CENTER_CENTER,
                &l.text,
                FontId::proportional(12.0),
                Color32::BLACK,
            );
        }
    }
}

/// Colour of dimension lines and arrows.
pub const DIMENSION: Color32 = Color32::from_rgb(40, 70, 150);

/// Sketch units per screen pixel around `at`.
fn sketch_per_px(cam: &OrbitCamera, rect: Rect, sketch: &Sketch, at: Point2) -> Real {
    match (
        screen(cam, rect, sketch, at),
        screen(cam, rect, sketch, at + Vector2::new(1.0, 0.0)),
    ) {
        (Some(a), Some(b)) if a.distance(b) > 1e-6 => 1.0 / a.distance(b) as Real,
        _ => 1.0,
    }
}

/// Where the text of dimension `id` is drawn: its stored position, or a
/// default a little off the geometry (`None` for non-dimensions).
pub fn dimension_label(
    cam: &OrbitCamera,
    rect: Rect,
    sketch: &Sketch,
    id: crate::sketch::ConstraintId,
) -> Option<Point2> {
    let anchor = sketch.dimension_anchor(id)?;
    if sketch.has_dimension_label(id) {
        return sketch.dimension_label(id);
    }
    let px = sketch_per_px(cam, rect, sketch, anchor);
    let away = 22.0 * px;
    // Default side: away from the middle of the sketch's geometry, so the
    // text lands outside the shape it measures.
    let n_pts = sketch.points().count().max(1) as Real;
    let centre = sketch
        .points()
        .fold(Vector2::zeros(), |acc, (_, p)| acc + p.position.coords)
        / n_pts;
    let outward = |n: Vector2| {
        if (anchor.coords - centre).dot(&n) < 0.0 {
            -n
        } else {
            n
        }
    };
    Some(match *sketch.constraint(id)? {
        Constraint::Distance { a, b, .. } => {
            let d = (pos(sketch, b) - pos(sketch, a)).try_normalize(1e-12)?;
            anchor + outward(Vector2::new(-d.y, d.x)) * away
        }
        Constraint::HorizontalDistance { .. } => anchor + outward(Vector2::new(0.0, 1.0)) * away,
        Constraint::VerticalDistance { .. } => anchor + outward(Vector2::new(1.0, 0.0)) * away,
        Constraint::Radius { circle, .. } | Constraint::Diameter { circle, .. } => {
            let (c, r) = sketch.circle_geometry(circle)?;
            c + Vector2::new(1.0, 1.0).normalize() * (r + away)
        }
        _ => anchor + Vector2::new(away * 0.7, away * 0.7),
    })
}

/// Draws the lines of dimension `id`: extension lines, dimension line or
/// leader or arc, and arrowheads (the text is drawn with the labels).
pub fn draw_dimension(
    painter: &Painter,
    cam: &OrbitCamera,
    rect: Rect,
    sketch: &Sketch,
    id: crate::sketch::ConstraintId,
    color: Color32,
) {
    let Some(label) = dimension_label(cam, rect, sketch, id) else {
        return;
    };
    let Some(c) = sketch.constraint(id) else {
        return;
    };
    let stroke = Stroke::new(1.0, color);
    let s = |p: Point2| screen(cam, rect, sketch, p);
    let seg = |a: Point2, b: Point2| {
        if let (Some(p), Some(q)) = (s(a), s(b)) {
            painter.line_segment([p, q], stroke);
        }
    };
    // Extension line from `from` to `to`, a few pixels past it.
    let ext = |from: Point2, to: Point2| {
        if let (Some(p), Some(q)) = (s(from), s(to)) {
            let d = (q - p).normalized();
            let gap = if (q - p).length() > 6.0 {
                d * 3.0
            } else {
                eframe::egui::Vec2::ZERO
            };
            painter.line_segment([p + gap, q + d * 4.0], stroke);
        }
    };
    let arrow = |tip: Point2, from: Point2| {
        if let (Some(t), Some(f)) = (s(tip), s(from)) {
            let d = (t - f).normalized();
            if d.length() < 0.5 {
                return;
            }
            let n = eframe::egui::vec2(-d.y, d.x);
            painter.add(Shape::convex_polygon(
                vec![t, t - d * 9.0 + n * 3.0, t - d * 9.0 - n * 3.0],
                color,
                Stroke::NONE,
            ));
        }
    };
    // A dimension line between `p` and `q` (on the line), arrows at both
    // ends pointing outwards, extended to the text if it is outside.
    let dim_line = |p: Point2, q: Point2| {
        let d = q - p;
        let len2 = d.norm_squared().max(1e-24);
        let t = (label - p).dot(&d) / len2;
        let (lo, hi) = (p + d * t.min(0.0), p + d * t.max(1.0));
        seg(lo, hi);
        let mid = nalgebra::center(&p, &q);
        arrow(p, mid);
        arrow(q, mid);
    };
    match *c {
        Constraint::Distance { a, b, .. } => {
            let (pa, pb) = (pos(sketch, a), pos(sketch, b));
            let Some(d) = (pb - pa).try_normalize(1e-12) else {
                return;
            };
            let n = Vector2::new(-d.y, d.x);
            let h = (label - pa).dot(&n);
            let (qa, qb) = (pa + n * h, pb + n * h);
            ext(pa, qa);
            ext(pb, qb);
            dim_line(qa, qb);
        }
        Constraint::HorizontalDistance { a, b, .. } => {
            let (pa, pb) = (pos(sketch, a), pos(sketch, b));
            let (qa, qb) = (Point2::new(pa.x, label.y), Point2::new(pb.x, label.y));
            ext(pa, qa);
            ext(pb, qb);
            dim_line(qa, qb);
        }
        Constraint::VerticalDistance { a, b, .. } => {
            let (pa, pb) = (pos(sketch, a), pos(sketch, b));
            let (qa, qb) = (Point2::new(label.x, pa.y), Point2::new(label.x, pb.y));
            ext(pa, qa);
            ext(pb, qb);
            dim_line(qa, qb);
        }
        Constraint::PointLineDistance { point, line, .. } => {
            let Some((l0, l1)) = sketch.line_geometry(line) else {
                return;
            };
            let Some(u) = (l1 - l0).try_normalize(1e-12) else {
                return;
            };
            let p = pos(sketch, point);
            let f = l0 + u * (p - l0).dot(&u);
            let h = (label - f).dot(&u);
            let (qf, qp) = (f + u * h, p + u * h);
            ext(f, qf);
            ext(p, qp);
            dim_line(qf, qp);
        }
        Constraint::DiameterToAxis { point, line, .. } => {
            // From the point across the axis to its mirror image: the
            // diameter a revolve gives it.
            let Some((l0, l1)) = sketch.line_geometry(line) else {
                return;
            };
            let Some(u) = (l1 - l0).try_normalize(1e-12) else {
                return;
            };
            let p = pos(sketch, point);
            let f = l0 + u * (p - l0).dot(&u);
            let m = f + (f - p);
            let h = (label - f).dot(&u);
            let (qp, qm) = (p + u * h, m + u * h);
            ext(p, qp);
            ext(m, qm);
            dim_line(qp, qm);
        }
        Constraint::Radius { circle, .. } | Constraint::Diameter { circle, .. } => {
            let Some((cen, r)) = sketch.circle_geometry(circle) else {
                return;
            };
            let dir = (label - cen).try_normalize(1e-12).unwrap_or(Vector2::x());
            let q = cen + dir * r;
            let outside = (label - cen).norm() > r;
            if matches!(c, Constraint::Radius { .. }) {
                seg(cen, if outside { label } else { q });
                arrow(q, cen);
            } else {
                let q2 = cen - dir * r;
                seg(q2, if outside { label } else { q });
                arrow(q, cen);
                arrow(q2, cen);
            }
        }
        Constraint::Angle { a, b, value, .. } => {
            let (Some((a0, a1)), Some(_)) = (sketch.line_geometry(a), sketch.line_geometry(b))
            else {
                return;
            };
            let Some(x) = sketch.dimension_anchor(id) else {
                return;
            };
            let r = (label - x).norm();
            if r < 1e-9 {
                return;
            }
            let u = a1 - a0;
            let start = u.y.atan2(u.x);
            let measured = sketch.measure(id).unwrap_or(value);
            // Of the line's two rays, start on the one nearer the label.
            let la = (label - x).y.atan2((label - x).x);
            let near = |t: Real| crate::sketch::wrap_angle_pub(la - t).abs();
            let mid_a = start + measured / 2.0;
            let start = if near(mid_a) <= near(mid_a + std::f64::consts::PI) {
                start
            } else {
                start + std::f64::consts::PI
            };
            let n = 24;
            let pts: Vec<Point2> = (0..=n)
                .map(|i| {
                    let t = start + measured * i as Real / n as Real;
                    x + Vector2::new(t.cos(), t.sin()) * r
                })
                .collect();
            for w in pts.windows(2) {
                seg(w[0], w[1]);
            }
            arrow(pts[0], pts[1]);
            arrow(pts[n], pts[n - 1]);
        }
        _ => {}
    }
}

/// A constraint label on screen (for drawing and picking).
pub struct LabelHit {
    pub constraint: crate::sketch::ConstraintId,
    pub rect: Rect,
    pub text: String,
}

/// Positions of all constraint labels of a sketch.
pub fn label_layout(
    painter: &Painter,
    cam: &OrbitCamera,
    rect: Rect,
    sketch: &Sketch,
    mode: crate::sketch::DimDisplay,
) -> Vec<LabelHit> {
    let mut out = Vec::new();
    for (id, c) in sketch.constraints() {
        // Dimensions sit at their (stored or default) text position, with
        // the text of the display mode; geometric relations above their
        // geometry.
        let labels = match dimension_label(cam, rect, sketch, id) {
            Some(at) => vec![(at, sketch.dimension_text(id, mode))],
            None => constraint_labels(sketch, c),
        };
        let dy = if c.is_dimension() { 0.0 } else { -12.0 };
        for (anchor, text) in labels {
            if let Some(s) = screen(cam, rect, sketch, anchor) {
                let size = painter
                    .layout_no_wrap(text.clone(), FontId::proportional(12.0), Color32::BLACK)
                    .size();
                let r = Align2::CENTER_CENTER
                    .anchor_size(s + eframe::egui::vec2(0.0, dy), size)
                    .expand(2.0);
                out.push(LabelHit {
                    constraint: id,
                    rect: r,
                    text,
                });
            }
        }
    }
    // No overlaps: dimension texts stay where they are (their lines lead
    // to them); a constraint glyph that would cover a dimension or an
    // earlier glyph steps aside (alternately down and up).
    let is_dim = |h: &LabelHit| {
        sketch
            .constraint(h.constraint)
            .is_some_and(Constraint::is_dimension)
    };
    let (dims, glyphs): (Vec<LabelHit>, Vec<LabelHit>) = out.into_iter().partition(is_dim);
    let mut placed: Vec<Rect> = dims.iter().map(|h| h.rect).collect();
    let mut result = dims;
    for mut g in glyphs {
        let base = g.rect;
        for k in 1u32..=8 {
            if !placed.iter().any(|r| r.intersects(g.rect)) {
                break;
            }
            let step = (base.height() + 1.0) * k.div_ceil(2) as f32;
            let dy = if k % 2 == 1 { step } else { -step };
            g.rect = base.translate(eframe::egui::vec2(0.0, dy));
        }
        placed.push(g.rect);
        result.push(g);
    }
    result
}

/// Points and entities a constraint refers to (for highlighting).
pub fn constraint_refs(c: &Constraint) -> (Vec<PointId>, Vec<EntityId>) {
    use Constraint::*;
    match *c {
        Coincident { a, b } | HorizontalPoints { a, b } | VerticalPoints { a, b } => {
            (vec![a, b], vec![])
        }
        Distance { a, b, .. } | HorizontalDistance { a, b, .. } | VerticalDistance { a, b, .. } => {
            (vec![a, b], vec![])
        }
        Symmetric { a, b, line } | SymmetricOnArc { a, b, line, .. } => (vec![a, b], vec![line]),
        PointOnLine { point, line }
        | Midpoint { point, line }
        | PointLineDistance { point, line, .. }
        | DiameterToAxis { point, line, .. } => (vec![point], vec![line]),
        PointOnCircle { point, circle } => (vec![point], vec![circle]),
        PointOnSpline { point, spline } => (vec![point], vec![spline]),
        Fix { point, .. } => (vec![point], vec![]),
        Translated { a, a2, .. } | Rotated { a, a2, .. } => (vec![a, a2], vec![]),
        Horizontal { line } | Vertical { line } => (vec![], vec![line]),
        Radius { circle, .. } | Diameter { circle, .. } => (vec![], vec![circle]),
        Parallel { a, b }
        | Perpendicular { a, b }
        | Collinear { a, b }
        | Concentric { a, b }
        | Equal { a, b } => (vec![], vec![a, b]),
        Tangent { a, b, .. } | Angle { a, b, .. } => (vec![], vec![a, b]),
        EqualOffset {
            base_a,
            copy_a,
            base_b,
            copy_b,
        } => (vec![], vec![base_a, copy_a, base_b, copy_b]),
    }
}

/// Inventor-style degrees-of-freedom glyphs: a cross of arrows for points
/// free in the plane, a double arrow for points that can slide along one
/// direction, a curved arrow on curves that can rotate, and a radial arrow
/// on circles with a free radius.
fn draw_dof(
    painter: &Painter,
    cam: &OrbitCamera,
    rect: Rect,
    ps: &PartSketch,
    dof: &crate::sketch::DofAnalysis,
) {
    use crate::sketch::PointFreedom;
    let sketch = &ps.sketch;
    let color = Color32::from_rgb(150, 60, 200);
    let stroke = Stroke::new(1.5, color);
    // Screen direction of a sketch-plane vector at point `at`.
    let dir = |at: Point2, v: Vector2| -> Option<eframe::egui::Vec2> {
        let (a, b) = (
            screen(cam, rect, sketch, at)?,
            screen(cam, rect, sketch, at + v * 1e-3)?,
        );
        let d = b - a;
        (d.length() > 1e-6).then(|| d.normalized())
    };
    let double_arrow = |s: Pos2, d: eframe::egui::Vec2| {
        let len = 14.0;
        painter.arrow(s, d * len, stroke);
        painter.arrow(s, -d * len, stroke);
    };
    for (id, p) in sketch.points() {
        if ps.is_projected(id) {
            continue;
        }
        let Some(s) = screen(cam, rect, sketch, p.position) else {
            continue;
        };
        match dof.points.get(&id) {
            Some(PointFreedom::Free) => {
                if let (Some(dx), Some(dy)) =
                    (dir(p.position, Vector2::x()), dir(p.position, Vector2::y()))
                {
                    double_arrow(s, dx);
                    double_arrow(s, dy);
                }
            }
            Some(PointFreedom::Along(v)) => {
                if let Some(d) = dir(p.position, *v) {
                    double_arrow(s, d);
                }
            }
            _ => {}
        }
    }
    for &e in &dof.rotating {
        let Some(mid) = entity_anchor(sketch, e) else {
            continue;
        };
        let Some(c) = screen(cam, rect, sketch, mid) else {
            continue;
        };
        let r = 11.0;
        let pts: Vec<Pos2> = (0..=12)
            .map(|i| {
                let a = -0.3 + 4.2 * i as f32 / 12.0;
                c + eframe::egui::vec2(a.cos(), -a.sin()) * r
            })
            .collect();
        let tip = pts[pts.len() - 1];
        let before = pts[pts.len() - 2];
        painter.add(Shape::line(pts, stroke));
        painter.arrow(before, (tip - before).normalized() * 6.0, stroke);
    }
    for &e in &dof.free_radius {
        let Some((c, r)) = sketch.circle_geometry(e) else {
            continue;
        };
        let at = c + Vector2::new(r, r) * std::f64::consts::FRAC_1_SQRT_2;
        if let (Some(s), Some(d)) = (
            screen(cam, rect, sketch, at),
            dir(at, Vector2::new(1.0, 1.0)),
        ) {
            double_arrow(s, d);
        }
    }
}

/// Where and what to print for a constraint.
fn constraint_labels(sketch: &Sketch, c: &Constraint) -> Vec<(Point2, String)> {
    let at = |id| entity_anchor(sketch, id);
    let mid = |a: PointId, b: PointId| Some(nalgebra::center(&pos(sketch, a), &pos(sketch, b)));
    let mut out = Vec::new();
    let mut push = |p: Option<Point2>, t: &str| {
        if let Some(p) = p {
            out.push((p, t.to_owned()));
        }
    };
    let both = |push: &mut dyn FnMut(Option<Point2>, &str), a, b, t: &str| {
        push(at(a), t);
        push(at(b), t);
    };
    match *c {
        // Pattern ties are internal to the pattern (Inventor shows none).
        Constraint::Coincident { .. }
        | Constraint::Translated { .. }
        | Constraint::Rotated { .. } => {}
        Constraint::Horizontal { line } => push(at(line), "H"),
        Constraint::Vertical { line } => push(at(line), "V"),
        Constraint::HorizontalPoints { a, b } => push(mid(a, b), "H"),
        Constraint::VerticalPoints { a, b } => push(mid(a, b), "V"),
        Constraint::Parallel { a, b } => both(&mut push, a, b, "//"),
        Constraint::Perpendicular { a, b } => both(&mut push, a, b, "90°"),
        Constraint::Collinear { a, b } => both(&mut push, a, b, "col"),
        Constraint::Concentric { a, b } => push(
            at(a).zip(at(b)).map(|(p, q)| nalgebra::center(&p, &q)),
            "conc",
        ),
        Constraint::Equal { a, b } => both(&mut push, a, b, "="),
        Constraint::Symmetric { a, b, .. } | Constraint::SymmetricOnArc { a, b, .. } => {
            push(Some(pos(sketch, a)), "sym");
            push(Some(pos(sketch, b)), "sym");
        }
        Constraint::Midpoint { point, .. } => push(Some(pos(sketch, point)), "mid"),
        Constraint::EqualOffset { copy_a, .. } => push(at(copy_a), "off"),
        Constraint::Fix { point, .. } => push(Some(pos(sketch, point)), "fix"),
        Constraint::Tangent { a, b, .. } => {
            // Label halfway between the two entities' anchors.
            if let (Some(p), Some(q)) = (at(a), at(b)) {
                push(Some(nalgebra::center(&p, &q)), "T");
            }
        }
        Constraint::PointOnLine { point, .. }
        | Constraint::PointOnCircle { point, .. }
        | Constraint::PointOnSpline { point, .. } => push(Some(pos(sketch, point)), "on"),
        Constraint::Distance { a, b, value }
        | Constraint::HorizontalDistance { a, b, value }
        | Constraint::VerticalDistance { a, b, value } => push(mid(a, b), &format!("{value:.2}")),
        Constraint::PointLineDistance { point, line, value } => push(
            at(line).map(|l| nalgebra::center(&l, &pos(sketch, point))),
            &format!("{value:.2}"),
        ),
        Constraint::DiameterToAxis { point, line, value } => push(
            at(line).map(|l| nalgebra::center(&l, &pos(sketch, point))),
            &format!("Ø{value:.2}"),
        ),
        Constraint::Angle { a, b, value, .. } => push(
            at(a).zip(at(b)).map(|(p, q)| nalgebra::center(&p, &q)),
            &format!("{:.1}°", value.to_degrees().abs()),
        ),
        Constraint::Radius { circle, value } => push(at(circle), &format!("R{value:.2}")),
        Constraint::Diameter { circle, value } => push(at(circle), &format!("Ø{value:.2}")),
    }
    out
}

/// Fill of the closed regions of the edited sketch.
pub const REGION_FILL: Color32 = Color32::from_rgba_premultiplied(24, 56, 92, 40);
/// Fill of regions offered as an Extrude profile, and of the chosen ones.
pub const PROFILE_FILL: Color32 = Color32::from_rgba_premultiplied(30, 60, 30, 50);
pub const PROFILE_CHOSEN: Color32 = Color32::from_rgba_premultiplied(30, 110, 30, 120);

/// Shades the closed regions (profiles) of a sketch, as Inventor does.
pub fn draw_regions(
    painter: &Painter,
    cam: &OrbitCamera,
    rect: Rect,
    sketch: &Sketch,
    regions: &[crate::sketch::Region],
    fill: Color32,
) {
    for r in regions {
        let outer = r.outer.polygon(0.05);
        let holes: Vec<_> = r.holes.iter().map(|h| h.polygon(0.05)).collect();
        let (pts, tris) = crate::kernel::triangulate_with_holes(&outer, &holes);
        let mut m = eframe::egui::Mesh::default();
        for p in &pts {
            let Some(s) = screen(cam, rect, sketch, *p) else {
                return;
            };
            m.colored_vertex(s, fill);
        }
        for t in tris {
            m.add_triangle(t[0] as u32, t[1] as u32, t[2] as u32);
        }
        painter.add(Shape::mesh(m));
    }
}

/// Capture radius (screen points) for snapping to a sketch point. Points
/// win over curves, so this is the larger of the two.
pub const POINT_SNAP_PX: f32 = 12.0;
/// Point radius for tools that pick geometry (constraints, Trim, patterns…),
/// smaller than [`POINT_SNAP_PX`] so curves near a point stay pickable.
pub const POINT_PICK_PX: f32 = 6.0;
/// Capture radius (screen points) for snapping onto a curve.
pub const CURVE_SNAP_PX: f32 = 8.0;

/// The sketch point nearest to `cursor` within `radius` screen points.
/// Any sketch point (including fixed and projected ones) near `cursor`.
pub fn pick_any_point(
    cam: &OrbitCamera,
    rect: Rect,
    sketch: &Sketch,
    cursor: Pos2,
    radius: f32,
) -> Option<PointId> {
    sketch
        .points()
        .filter_map(|(id, p)| Some((id, screen(cam, rect, sketch, p.position)?.distance(cursor))))
        .filter(|&(_, d)| d <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// Like [`pick_any_point`], but ignoring the points for which `skip` is true
/// (e.g. the point being dragged and its curve's other end).
pub fn pick_other_point(
    cam: &OrbitCamera,
    rect: Rect,
    sketch: &Sketch,
    cursor: Pos2,
    radius: f32,
    skip: impl Fn(PointId) -> bool,
) -> Option<PointId> {
    sketch
        .points()
        .filter(|&(id, _)| !skip(id))
        .filter_map(|(id, p)| Some((id, screen(cam, rect, sketch, p.position)?.distance(cursor))))
        .filter(|&(_, d)| d <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

pub fn pick_point(
    cam: &OrbitCamera,
    rect: Rect,
    sketch: &Sketch,
    cursor: Pos2,
    radius: f32,
) -> Option<PointId> {
    sketch
        .points()
        .filter(|(_, p)| !p.fixed)
        .filter_map(|(id, p)| Some((id, screen(cam, rect, sketch, p.position)?.distance(cursor))))
        .filter(|&(_, d)| d <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// Draws the frame of the sketch being edited: grid lines, X/Y axes and
/// the coordinate-system indicator (triad), per the sketch options.
pub fn draw_sketch_frame(
    painter: &Painter,
    cam: &OrbitCamera,
    rect: Rect,
    plane: &Plane,
    extent: Real,
    (grid, axes, triad): (bool, bool, bool),
) {
    let to_screen = |u: Real, v: Real| cam.project(&plane.point_at(u, v), rect);
    let line = |a: (Real, Real), b: (Real, Real), stroke: Stroke| {
        if let (Some(p), Some(q)) = (to_screen(a.0, a.1), to_screen(b.0, b.1)) {
            painter.line_segment([p, q], stroke);
        }
    };
    if grid {
        let step = nice_step(extent / 10.0);
        let n = (extent / step).ceil() as i32;
        let e = n as Real * step;
        let stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(90, 110, 140, 60));
        for i in -n..=n {
            let t = i as Real * step;
            line((t, -e), (t, e), stroke);
            line((-e, t), (e, t), stroke);
        }
    }
    if axes {
        line(
            (-extent, 0.0),
            (extent, 0.0),
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(200, 40, 40, 150)),
        );
        line(
            (0.0, -extent),
            (0.0, extent),
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(40, 160, 40, 150)),
        );
    }
    if triad {
        let len = extent * 0.15;
        for ((u, v), color, name) in [
            ((len, 0.0), Color32::from_rgb(210, 30, 30), "X"),
            ((0.0, len), Color32::from_rgb(30, 160, 30), "Y"),
        ] {
            if let (Some(o), Some(tip)) = (to_screen(0.0, 0.0), to_screen(u, v)) {
                painter.arrow(o, tip - o, Stroke::new(2.5, color));
                painter.text(
                    tip,
                    Align2::LEFT_BOTTOM,
                    name,
                    FontId::proportional(12.0),
                    color,
                );
            }
        }
    }
}

/// 1, 2 or 5 × 10ⁿ just above `x`.
fn nice_step(x: Real) -> Real {
    let p = 10f64.powf(x.max(1e-9).log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * p)
        .find(|&s| s >= x)
        .unwrap_or(10.0 * p)
}

/// Corners of an origin plane square of half-size `half`, in world space.
pub fn origin_plane_quad(plane: OriginPlane, half: Real) -> [Point3; 4] {
    let (n, x) = plane.frame();
    let y = n.cross(&x);
    [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .map(|(a, b)| Point3::from(x * a * half + y * b * half))
}

/// Ray parameter where `origin + t·dir` hits the origin-plane square.
pub fn hit_origin_plane(
    plane: OriginPlane,
    half: Real,
    origin: &Point3,
    dir: &Vector3,
) -> Option<Real> {
    let (n, x) = plane.frame();
    let y = n.cross(&x);
    let denom = dir.dot(&n);
    if denom.abs() < 1e-12 {
        return None;
    }
    let t = -origin.coords.dot(&n) / denom;
    let p = origin + dir * t;
    (t > 0.0 && p.coords.dot(&x).abs() <= half && p.coords.dot(&y).abs() <= half).then_some(t)
}

/// Draws the three origin planes as translucent squares (shown while
/// picking a sketch plane, as Inventor does in a new part).
pub fn draw_origin_planes(
    painter: &Painter,
    cam: &OrbitCamera,
    rect: Rect,
    half: Real,
    hovered: Option<OriginPlane>,
) {
    for plane in OriginPlane::ALL {
        let quad: Option<Vec<Pos2>> = origin_plane_quad(plane, half)
            .iter()
            .map(|p| cam.project(p, rect))
            .collect();
        let Some(quad) = quad else { continue };
        let hot = hovered == Some(plane);
        let fill = if hot {
            Color32::from_rgba_unmultiplied(255, 160, 40, 90)
        } else {
            Color32::from_rgba_unmultiplied(230, 190, 90, 45)
        };
        let stroke = Stroke::new(if hot { 2.0 } else { 1.0 }, Color32::from_rgb(190, 140, 40));
        painter.add(Shape::convex_polygon(quad.clone(), fill, stroke));
        painter.text(
            quad[2],
            Align2::LEFT_BOTTOM,
            plane.label(),
            FontId::proportional(12.0),
            Color32::from_rgb(120, 80, 0),
        );
    }
}

/// Fills a face (given as its triangle mesh) with the highlight colour.
pub fn draw_face_highlight(painter: &Painter, cam: &OrbitCamera, rect: Rect, mesh: &TriangleMesh) {
    draw_face_fill(
        painter,
        cam,
        rect,
        mesh,
        Color32::from_rgba_unmultiplied(255, 150, 40, 110),
    );
}

/// Pick radius of model edges in the view (screen points).
pub const EDGE_PICK_PX: f32 = 6.0;

/// Fills a face (given as its triangle mesh) with `fill`.
pub fn draw_face_fill(
    painter: &Painter,
    cam: &OrbitCamera,
    rect: Rect,
    mesh: &TriangleMesh,
    fill: Color32,
) {
    let mut m = eframe::egui::Mesh::default();
    for p in &mesh.positions {
        let Some(s) = cam.project(p, rect) else {
            return;
        };
        m.colored_vertex(s, fill);
    }
    for t in &mesh.triangles {
        m.add_triangle(t[0], t[1], t[2]);
    }
    painter.add(Shape::mesh(m));
}

/// The sketch entity nearest to `cursor` within `radius` screen points,
/// with the closest point on it (sketch coordinates).
pub fn pick_entity(
    cam: &OrbitCamera,
    rect: Rect,
    sketch: &Sketch,
    cursor: Pos2,
    radius: f32,
) -> Option<(EntityId, Point2)> {
    pick_other_entity(cam, rect, sketch, cursor, radius, |_| false)
}

/// The line whose midpoint is nearest to `cursor` within `radius` screen
/// points, with that midpoint (sketch coordinates).
pub fn pick_midpoint(
    cam: &OrbitCamera,
    rect: Rect,
    sketch: &Sketch,
    cursor: Pos2,
    radius: f32,
) -> Option<(EntityId, Point2)> {
    sketch
        .entities()
        .filter(|(_, e)| e.is_line())
        .filter_map(|(id, _)| {
            let (a, b) = sketch.line_geometry(id)?;
            let m = nalgebra::center(&a, &b);
            Some((id, m, screen(cam, rect, sketch, m)?.distance(cursor)))
        })
        .filter(|&(.., d)| d <= radius)
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(id, m, _)| (id, m))
}

/// Like [`pick_entity`], but ignoring the entities for which `skip` is true.
pub fn pick_other_entity(
    cam: &OrbitCamera,
    rect: Rect,
    sketch: &Sketch,
    cursor: Pos2,
    radius: f32,
    skip: impl Fn(EntityId) -> bool,
) -> Option<(EntityId, Point2)> {
    let mut best: Option<(f32, EntityId, Point2)> = None;
    for (id, _) in sketch.entities().filter(|&(id, _)| !skip(id)) {
        let pts = polyline(sketch, id);
        let scr: Vec<Option<Pos2>> = pts.iter().map(|&p| screen(cam, rect, sketch, p)).collect();
        for i in 0..pts.len().saturating_sub(1) {
            let (Some(a), Some(b)) = (scr[i], scr[i + 1]) else {
                continue;
            };
            let ab = b - a;
            let t = ((cursor - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
            let d = (a + ab * t).distance(cursor);
            if d <= radius && best.is_none_or(|(bd, ..)| d < bd) {
                let on = pts[i] + (pts[i + 1] - pts[i]) * t as f64;
                best = Some((d, id, on));
            }
        }
    }
    best.map(|(_, id, p)| (id, p))
}

/// Draws a tool preview (rubber band) in sketch coordinates.
pub fn draw_preview(
    painter: &Painter,
    cam: &OrbitCamera,
    rect: Rect,
    sketch: &Sketch,
    lines: &[super::sketch_tools::PreviewLine],
) {
    for l in lines {
        let color = if l.warn {
            Color32::from_rgb(220, 40, 40)
        } else {
            Color32::from_rgb(40, 110, 230)
        };
        let pts: Option<Vec<Pos2>> = l
            .pts
            .iter()
            .map(|&p| screen(cam, rect, sketch, p))
            .collect();
        let Some(pts) = pts else { continue };
        if l.dashed {
            painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, color), 5.0, 4.0));
        } else {
            painter.add(Shape::line(pts, Stroke::new(1.5, color)));
        }
    }
}

/// Snap/inference glyph next to the cursor (Inventor shows the constraint
/// that will be created).
pub fn draw_snap(
    painter: &Painter,
    cam: &OrbitCamera,
    rect: Rect,
    sketch: &Sketch,
    pick: &super::sketch_tools::Pick,
) {
    use super::sketch_tools::{Infer, Snap};
    let Some(s) = screen(cam, rect, sketch, pick.pos) else {
        return;
    };
    // Point alignment: dotted line from the aligned point to the cursor.
    for q in [pick.align.x, pick.align.y].into_iter().flatten() {
        if let Some(a) = sketch
            .point(q)
            .and_then(|p| screen(cam, rect, sketch, p.position))
        {
            painter.extend(eframe::egui::Shape::dotted_line(
                &[a, s],
                HIGHLIGHT,
                5.0,
                1.3,
            ));
            painter.circle_stroke(a, 4.0, Stroke::new(1.5, HIGHLIGHT));
        }
    }
    let text = match (pick.snap, pick.infer) {
        (Snap::Point(_), _) => {
            painter.circle_stroke(s, 6.0, Stroke::new(2.0, HIGHLIGHT));
            "coincident"
        }
        (Snap::OnEntity(_), _) => {
            painter.circle_filled(s, 3.5, HIGHLIGHT);
            "on curve"
        }
        (Snap::Midpoint(_), _) => {
            painter.circle_stroke(s, 6.0, Stroke::new(2.0, HIGHLIGHT));
            "midpoint"
        }
        (Snap::Tangent(_), _) => {
            // Tangent glyph: a small circle with a line touching it.
            painter.circle_stroke(
                s + eframe::egui::vec2(0.0, -5.0),
                5.0,
                Stroke::new(1.8, HIGHLIGHT),
            );
            painter.line_segment(
                [
                    s + eframe::egui::vec2(-8.0, 0.0),
                    s + eframe::egui::vec2(8.0, 0.0),
                ],
                Stroke::new(1.8, HIGHLIGHT),
            );
            "tangent"
        }
        (_, Some(Infer::Horizontal)) => "H",
        (_, Some(Infer::Vertical)) => "V",
        (_, Some(Infer::Perpendicular(_))) => "90°",
        (_, Some(Infer::Parallel(_))) => "//",
        _ => return,
    };
    painter.text(
        s + eframe::egui::vec2(10.0, -10.0),
        Align2::LEFT_BOTTOM,
        text,
        FontId::proportional(11.0),
        HIGHLIGHT,
    );
}
