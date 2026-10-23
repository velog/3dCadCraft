//! Manipulators: drag handles in the viewport that edit a feature's values
//! (Inventor's distance arrows and angle arcs, `docs/spec/0-ui.md` §2.8).
//!
//! Extrude: an arrow per distance (A, and B when Asymmetric) and a taper
//! handle beside the arrow tip. Revolve (Angle extent): an arc around the
//! axis with a handle at the end angle (A, and B when Asymmetric).

use std::f64::consts::{PI, TAU};

use eframe::egui::{self, Color32, Pos2, Rect, Stroke};
use nalgebra as na;

use super::camera::OrbitCamera;
use crate::kernel::{Point3, Real, Vector3};
use crate::part::{Extent, ExtentDirection, ExtrudeFeature, RevolveExtent, RevolveFeature};
use crate::sketch::Sketch;

/// How a drag maps to a value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DragGeom {
    /// Along a line: value = parameter of the closest point × `scale`.
    Linear {
        origin: Point3,
        dir: Vector3,
        scale: Real,
    },
    /// Taper: sideways offset at the tip, `depth` away from the start.
    Taper {
        tip: Point3,
        normal: Vector3,
        side: Vector3,
        r0: Real,
        depth: Real,
    },
    /// Angle around an axis, from the profile's radial direction.
    Angle {
        center: Point3,
        axis: Vector3,
        reference: Vector3,
        map: AngleMap,
    },
}

/// From the handle's position angle θ (0..2π about the axis) to the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AngleMap {
    /// value = θ.
    Plain,
    /// value = 2π − θ (handle on the negative side).
    Flip,
    /// value = 2θ (Symmetric: the handle is at half the angle).
    Double,
}

/// One handle.
#[derive(Clone, Debug)]
pub struct Handle {
    /// The feature field it edits.
    pub field: &'static str,
    /// What to draw: a polyline (shaft or arc) and the grip point.
    pub path: Vec<Point3>,
    /// Where it is grabbed.
    pub grip: Point3,
    /// Drag mapping.
    pub geom: DragGeom,
    /// Arrowhead direction at the grip (none for taper/angle grips).
    pub arrow: Option<Vector3>,
    /// Angle (shown in degrees) or length.
    pub angle: bool,
    /// The value may go through zero to negative (a work plane's offset
    /// and angle; an Extrude distance, where negative flips the direction).
    pub signed: bool,
}

/// The base point of a profile: a point inside its first region.
fn base(sk: &Sketch, profiles: &[crate::part::ProfileRef]) -> Option<Point3> {
    let regions = sk.regions();
    let r = profiles.first()?.resolve(&regions)?;
    // The area centroid of the outer loop when it is inside the region (a
    // ring's or an L's centroid may not be), else any interior point.
    let poly = r.outer.polygon(1e-3);
    let (mut a, mut c) = (0.0, nalgebra::Vector2::zeros());
    for i in 0..poly.len() {
        let (p, q) = (poly[i].coords, poly[(i + 1) % poly.len()].coords);
        let cr = p.perp(&q);
        a += cr;
        c += (p + q) * cr;
    }
    let centroid = (a.abs() > 1e-12).then(|| crate::sketch::Point2::from(c / (3.0 * a)));
    let p = centroid
        .filter(|&p| r.contains(p))
        .unwrap_or_else(|| r.interior_point());
    Some(sk.plane.point_at(p.x, p.y))
}

/// The distance arrow of an Offset work plane: from `origin` on the base
/// plane along its normal `n` to the work plane.
pub fn plane_offset_handle(origin: Point3, n: Vector3, distance: Real) -> Handle {
    let tip = origin + n * distance;
    Handle {
        field: "distance",
        path: vec![origin, tip],
        grip: tip,
        geom: DragGeom::Linear {
            origin,
            dir: n,
            scale: 1.0,
        },
        arrow: Some(if distance < 0.0 { -n } else { n }),
        angle: false,
        signed: true,
    }
}

/// The angle arc of an Angle to Plane work plane: around the hinge line
/// (`center`, unit `axis`), from the base plane (its in-plane direction
/// square to the hinge, `reference`, of length = the arc radius) to the
/// work plane at `angle`.
pub fn plane_angle_handle(
    center: Point3,
    axis: Vector3,
    reference: Vector3,
    angle: Real,
) -> Handle {
    let at = |t: Real| {
        center + na::Rotation3::from_axis_angle(&na::Unit::new_normalize(axis), t) * reference
    };
    let n = ((angle.abs() / (TAU / 96.0)).ceil().max(2.0)) as usize;
    let path = (0..=n).map(|k| at(angle * k as Real / n as Real)).collect();
    Handle {
        field: "angle",
        path,
        grip: at(angle),
        geom: DragGeom::Angle {
            center,
            axis,
            reference,
            map: AngleMap::Plain,
        },
        arrow: None,
        angle: true,
        signed: true,
    }
}

/// Extrude handles (only for the Distance extent from the sketch plane).
pub fn extrude_handles(def: &ExtrudeFeature, sk: &Sketch) -> Vec<Handle> {
    if def.extent != Extent::Distance || def.from.is_some() {
        return Vec::new();
    }
    let Some(b) = base(sk, &def.profiles) else {
        return Vec::new();
    };
    let n = sk.plane.normal.into_inner();
    let side = sk.plane.x_axis.into_inner();
    let one_sided = matches!(
        def.direction,
        ExtentDirection::Default | ExtentDirection::Flipped
    );
    let mut out = Vec::new();
    let mut arrow = |field: &'static str,
                     dir: Vector3,
                     d: Real,
                     scale: Real,
                     taper: Option<(&'static str, Real)>| {
        let tip = b + dir * (d / scale);
        out.push(Handle {
            field,
            path: vec![b, tip],
            grip: tip,
            geom: DragGeom::Linear {
                origin: b,
                dir,
                scale,
            },
            arrow: Some(dir),
            angle: false,
            // One-sided distances go through the profile (a flip, Inventor).
            signed: field == "distance" && one_sided,
        });
        if let Some((tf, t)) = taper {
            let depth = d / scale;
            let r0 = (depth * 0.35).max(2.0);
            let grip = tip + side * (r0 + depth * t.tan());
            out.push(Handle {
                field: tf,
                path: vec![tip, grip],
                grip,
                geom: DragGeom::Taper {
                    tip,
                    normal: n,
                    side,
                    r0,
                    depth,
                },
                arrow: None,
                angle: true,
                signed: false,
            });
        }
    };
    match def.direction {
        ExtentDirection::Default => {
            arrow("distance", n, def.distance, 1.0, Some(("taper", def.taper)))
        }
        ExtentDirection::Flipped => arrow(
            "distance",
            -n,
            def.distance,
            1.0,
            Some(("taper", def.taper)),
        ),
        ExtentDirection::Symmetric => {
            arrow("distance", n, def.distance, 2.0, Some(("taper", def.taper)))
        }
        ExtentDirection::Asymmetric => {
            arrow("distance", n, def.distance, 1.0, Some(("taper", def.taper)));
            arrow(
                "distance_b",
                -n,
                def.distance_b,
                1.0,
                Some(("taper_b", def.taper_b)),
            );
        }
    }
    out
}

/// Revolve handles (only for the Angle extent).
pub fn revolve_handles(
    def: &RevolveFeature,
    sk: &Sketch,
    works: &crate::part::placement::WorkGeom,
) -> Vec<Handle> {
    if def.extent != RevolveExtent::Angle {
        return Vec::new();
    }
    let (Some(c3), Ok(ax)) = (
        base(sk, &def.profiles),
        crate::part::revolve::resolve_axis_in(&def.axis, sk, works),
    ) else {
        return Vec::new();
    };
    let a0 = sk.plane.point_at(ax.origin.x, ax.origin.y);
    let a1 = sk
        .plane
        .point_at(ax.origin.x + ax.dir.x, ax.origin.y + ax.dir.y);
    let Some(axis) = (a1 - a0).try_normalize(1e-12) else {
        return Vec::new();
    };
    let center = a0 + axis * (c3 - a0).dot(&axis);
    let reference = c3 - center;
    if reference.norm() < 1e-9 {
        return Vec::new();
    }
    let at = |t: Real| {
        center + na::Rotation3::from_axis_angle(&na::Unit::new_unchecked(axis), t) * reference
    };
    let arc = |t0: Real, t1: Real| -> Vec<Point3> {
        let n = ((t1 - t0).abs() / (TAU / 96.0)).ceil().max(2.0) as usize;
        (0..=n)
            .map(|k| at(t0 + (t1 - t0) * k as Real / n as Real))
            .collect()
    };
    let handle = |field: &'static str, t: Real, path: Vec<Point3>, map: AngleMap| Handle {
        field,
        path,
        grip: at(t),
        geom: DragGeom::Angle {
            center,
            axis,
            reference,
            map,
        },
        arrow: None,
        angle: true,
        signed: false,
    };
    let (a, b) = (def.angle, def.angle_b);
    match def.direction {
        ExtentDirection::Default => vec![handle("angle", a, arc(0.0, a), AngleMap::Plain)],
        ExtentDirection::Flipped => vec![handle("angle", -a, arc(0.0, -a), AngleMap::Flip)],
        ExtentDirection::Symmetric => {
            vec![handle(
                "angle",
                a / 2.0,
                arc(-a / 2.0, a / 2.0),
                AngleMap::Double,
            )]
        }
        ExtentDirection::Asymmetric => vec![
            handle("angle", a, arc(0.0, a), AngleMap::Plain),
            handle("angle_b", -b, arc(0.0, -b), AngleMap::Flip),
        ],
    }
}

/// The value for the eye ray `(o, d)` (unsnapped).
pub fn drag_value(geom: &DragGeom, o: &Point3, d: &Vector3) -> Option<Real> {
    let plane_hit = |p: &Point3, n: &Vector3| {
        let den = d.dot(n);
        (den.abs() > 1e-12).then(|| o + d * ((p - o).dot(n) / den))
    };
    match *geom {
        DragGeom::Linear { origin, dir, scale } => {
            // Closest point of the line to the ray.
            let w = origin - o;
            let (a, b, c) = (dir.dot(&dir), dir.dot(d), d.dot(d));
            let (dd, e) = (dir.dot(&w), d.dot(&w));
            let den = a * c - b * b;
            if den.abs() < 1e-12 {
                return None;
            }
            let t = (b * e - c * dd) / den;
            Some(t * scale)
        }
        DragGeom::Taper {
            tip,
            normal,
            side,
            r0,
            depth,
        } => {
            let h = plane_hit(&tip, &normal)?;
            let lateral = (h - tip).dot(&side) - r0;
            Some(lateral.atan2(depth.max(1e-9)))
        }
        DragGeom::Angle {
            center,
            axis,
            reference,
            map,
        } => {
            let h = plane_hit(&center, &axis)?;
            let v = h - center;
            let y = axis.cross(&reference);
            let theta = v.dot(&y).atan2(v.dot(&reference)).rem_euclid(TAU);
            Some(match map {
                AngleMap::Plain => theta,
                AngleMap::Flip => TAU - theta,
                AngleMap::Double => 2.0 * if theta > PI { theta - TAU } else { theta }.abs(),
            })
        }
    }
}

/// Snaps and limits a dragged value: lengths to 1 mm (0.1 with `fine`),
/// tapers to 1°, angles to 5° (Inventor) or 1° with `fine`.
pub fn snap(h: &Handle, v: Real, fine: bool) -> Real {
    match h.geom {
        DragGeom::Linear { .. } => {
            let step = if fine { 0.1 } else { 1.0 };
            let r = (v / step).round() * step;
            if h.signed { r } else { r.max(step) }
        }
        DragGeom::Taper { .. } => {
            let step = 1f64.to_radians();
            ((v / step).round() * step).clamp(-80f64.to_radians(), 80f64.to_radians())
        }
        DragGeom::Angle { .. } => {
            let step = (if fine { 1.0_f64 } else { 5.0 }).to_radians();
            let r = (v / step).round() * step;
            if h.signed {
                r.clamp(-TAU, TAU)
            } else {
                r.clamp(step, TAU)
            }
        }
    }
}

/// Text of a value.
pub fn text(h: &Handle, v: Real) -> String {
    if h.angle {
        format!("{:.1} deg", v.to_degrees())
    } else {
        format!("{v:.2} mm")
    }
}

/// Grip radius on screen (px).
pub const GRIP_PX: f32 = 7.0;

/// The handle whose grip is under `pos`.
pub fn hit(handles: &[Handle], cam: &OrbitCamera, rect: Rect, pos: Pos2) -> Option<usize> {
    handles
        .iter()
        .enumerate()
        .filter_map(|(i, h)| Some((i, cam.project(&h.grip, rect)?.distance(pos))))
        .filter(|(_, d)| *d <= GRIP_PX + 4.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Draws the handles; `hot` is highlighted, `label` shown at it.
pub fn draw(
    painter: &egui::Painter,
    cam: &OrbitCamera,
    rect: Rect,
    handles: &[Handle],
    hot: Option<usize>,
    label: Option<String>,
) {
    let base = Color32::from_rgb(40, 120, 230);
    let lit = Color32::from_rgb(255, 150, 30);
    for (i, h) in handles.iter().enumerate() {
        let col = if hot == Some(i) { lit } else { base };
        let pts: Vec<Pos2> = h.path.iter().filter_map(|p| cam.project(p, rect)).collect();
        painter.add(egui::Shape::line(pts, Stroke::new(2.0, col)));
        let Some(g) = cam.project(&h.grip, rect) else {
            continue;
        };
        match h.arrow {
            Some(dir) => {
                // Arrowhead along the screen direction of the shaft.
                let back = cam.project(&(h.grip - dir * 1e-3 * cam.distance), rect);
                let d = back.map_or(egui::vec2(0.0, -1.0), |b| (g - b).normalized());
                let n = egui::vec2(-d.y, d.x);
                let s = 10.0;
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        g + d * s,
                        g - d * 2.0 + n * s * 0.55,
                        g - d * 2.0 - n * s * 0.55,
                    ],
                    col,
                    Stroke::new(1.0, Color32::WHITE),
                ));
            }
            None => {
                painter.circle(g, GRIP_PX - 1.0, col, Stroke::new(1.5, Color32::WHITE));
            }
        }
        if hot == Some(i)
            && let Some(t) = &label
        {
            let at = g + egui::vec2(14.0, -14.0);
            let galley =
                painter.layout_no_wrap(t.clone(), egui::FontId::proportional(13.0), Color32::BLACK);
            let r = egui::Rect::from_min_size(at, galley.size()).expand(3.0);
            painter.rect_filled(r, 3.0, Color32::from_rgb(255, 250, 225));
            painter.rect_stroke(r, 3.0, Stroke::new(1.0, lit), egui::StrokeKind::Inside);
            painter.galley(at, galley, Color32::BLACK);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A work plane's offset and angle go through zero; an Extrude's
    /// one-sided distance is signed (the flip), its taper and two-sided
    /// distances are not.
    #[test]
    fn signed_handles_go_negative() {
        let off = plane_offset_handle(Point3::origin(), Vector3::z(), 5.0);
        assert!(off.signed);
        assert_eq!(snap(&off, -3.4, false), -3.0);
        assert_eq!(snap(&off, 0.2, false), 0.0);
        let ang = plane_angle_handle(Point3::origin(), Vector3::z(), Vector3::x(), 0.5);
        assert!(snap(&ang, -0.5, false) < 0.0);
        let mut plain = off.clone();
        plain.signed = false;
        assert_eq!(snap(&plain, -3.4, false), 1.0, "unsigned stays at one step");
    }

    #[test]
    fn linear_and_angle_drags() {
        // Line along +Z from the origin; a ray crossing it at z = 7.
        let g = DragGeom::Linear {
            origin: Point3::origin(),
            dir: Vector3::z(),
            scale: 2.0,
        };
        let v = drag_value(&g, &Point3::new(-10.0, 0.0, 7.0), &Vector3::x()).unwrap();
        assert!((v - 14.0).abs() < 1e-9, "symmetric doubles");
        // Angle about +Z from +X; a ray down at (0, 5): 90°.
        let a = DragGeom::Angle {
            center: Point3::origin(),
            axis: Vector3::z(),
            reference: Vector3::x() * 5.0,
            map: AngleMap::Plain,
        };
        let v = drag_value(&a, &Point3::new(0.0, 5.0, 10.0), &-Vector3::z()).unwrap();
        assert!((v - PI / 2.0).abs() < 1e-9);
        let f = DragGeom::Angle {
            center: Point3::origin(),
            axis: Vector3::z(),
            reference: Vector3::x() * 5.0,
            map: AngleMap::Flip,
        };
        let v = drag_value(&f, &Point3::new(0.0, -5.0, 10.0), &-Vector3::z()).unwrap();
        assert!((v - PI / 2.0).abs() < 1e-9, "flipped side");
    }
}
