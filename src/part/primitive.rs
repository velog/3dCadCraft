//! **Primitives** (Inventor: 3D Model → Primitives): Box and Cylinder become
//! a sketch and an Extrude with every Extrude option; Sphere and Torus a
//! sketch and a full Revolve. See `docs/spec/2.1-extrude.md` §9 and
//! `docs/spec/2.2-revolve.md` §9.
//!
//! The sketch geometry is dimensioned (width / length, diameters), so the
//! sizes are parameters like any sketch dimension. The old base feature
//! [`super::BoxFeature`] (files from before) is unchanged; new boxes are
//! always sketch + extrusion.

use crate::sketch::{Constraint, EntityId, Point2, Sketch, Style};

use super::{
    AxisRef, ExtrudeFeature, PartDocument, PartError, ProfileRef, RevolveFeature, SketchOptions,
    SketchPlacement,
};
use crate::kernel::Real;

/// Which primitive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitiveKind {
    /// Rectangle (centre, corner) + Extrude.
    Box,
    /// Circle (centre, a point on it) + Extrude.
    Cylinder,
    /// Half circle closed by a centerline (centre, a point on it) + full
    /// Revolve.
    Sphere,
    /// Circle (section centre, a point on it) beside a centerline through
    /// the centre + full Revolve.
    Torus,
}

impl PrimitiveKind {
    /// Every primitive, in ribbon order.
    pub const ALL: [PrimitiveKind; 4] = [
        PrimitiveKind::Box,
        PrimitiveKind::Cylinder,
        PrimitiveKind::Sphere,
        PrimitiveKind::Torus,
    ];

    /// Inventor's command name.
    pub fn label(self) -> &'static str {
        match self {
            PrimitiveKind::Box => "Box",
            PrimitiveKind::Cylinder => "Cylinder",
            PrimitiveKind::Sphere => "Sphere",
            PrimitiveKind::Torus => "Torus",
        }
    }

    /// Clicks in the sketch plane that define the shape.
    pub fn clicks(self) -> usize {
        match self {
            PrimitiveKind::Torus => 3,
            _ => 2,
        }
    }

    /// The prompt for click `k` (0-based).
    pub fn prompt(self, k: usize) -> &'static str {
        match (self, k) {
            (PrimitiveKind::Box, 0) => "Box: click the centre of the rectangle",
            (PrimitiveKind::Box, _) => "Box: click a corner",
            (PrimitiveKind::Cylinder, 0) => "Cylinder: click the centre",
            (PrimitiveKind::Cylinder, _) => "Cylinder: click to set the diameter",
            (PrimitiveKind::Sphere, 0) => "Sphere: click the centre",
            (PrimitiveKind::Sphere, _) => "Sphere: click to set the diameter",
            (PrimitiveKind::Torus, 0) => "Torus: click the centre",
            (PrimitiveKind::Torus, 1) => "Torus: click the centre of the section",
            (PrimitiveKind::Torus, _) => "Torus: click to set the section diameter",
        }
    }

    /// Made by a Revolve (Sphere, Torus) rather than an Extrude.
    pub fn revolves(self) -> bool {
        matches!(self, PrimitiveKind::Sphere | PrimitiveKind::Torus)
    }
}

fn bad(msg: &str) -> PartError {
    PartError::Feature(msg.to_owned())
}

/// Draws the primitive's dimensioned geometry into `sk` from the clicks
/// (sketch coordinates). Returns the revolve axis (Sphere, Torus).
pub fn draw(
    sk: &mut Sketch,
    kind: PrimitiveKind,
    clicks: &[Point2],
) -> Result<Option<EntityId>, PartError> {
    if clicks.len() < kind.clicks() {
        return Err(bad("not enough points"));
    }
    let err = |e: crate::sketch::SketchError| PartError::Feature(e.to_string());
    let c = clicks[0];
    match kind {
        PrimitiveKind::Box => {
            let q = clicks[1];
            let d = q - c;
            if d.x.abs() < 1e-6 || d.y.abs() < 1e-6 {
                return Err(bad("the box has no width or length"));
            }
            let sh = sk.add_rectangle_center(c, q).map_err(err)?;
            // Corners in order: c - d, (c.x + d.x, c.y - d.y), c + d, ...
            let corner = |k: usize| sh.points.get(k).copied();
            let (Some(p0), Some(p1), Some(p2)) = (corner(0), corner(1), corner(2)) else {
                return Err(bad("the rectangle has no corners"));
            };
            let (a, b) = if d.x > 0.0 { (p0, p1) } else { (p1, p0) };
            sk.add_constraint(Constraint::HorizontalDistance {
                a,
                b,
                value: 2.0 * d.x.abs(),
            })
            .map_err(err)?;
            let (a, b) = if d.y > 0.0 { (p1, p2) } else { (p2, p1) };
            sk.add_constraint(Constraint::VerticalDistance {
                a,
                b,
                value: 2.0 * d.y.abs(),
            })
            .map_err(err)?;
            Ok(None)
        }
        PrimitiveKind::Cylinder => {
            let r = (clicks[1] - c).norm();
            if r < 1e-6 {
                return Err(bad("the cylinder has no diameter"));
            }
            let circle = sk.add_circle(c, r).map_err(err)?;
            sk.add_constraint(Constraint::Diameter {
                circle,
                value: 2.0 * r,
            })
            .map_err(err)?;
            Ok(None)
        }
        PrimitiveKind::Sphere => {
            let r = (clicks[1] - c).norm();
            if r < 1e-6 {
                return Err(bad("the sphere has no diameter"));
            }
            let center = sk.add_point(c);
            let a = sk.add_point(Point2::new(c.x + r, c.y));
            let b = sk.add_point(Point2::new(c.x - r, c.y));
            let arc = sk.add_arc(center, a, b).map_err(err)?;
            let axis = sk.add_line_between(b, a).map_err(err)?;
            sk.set_style(axis, Style::Centerline).map_err(err)?;
            sk.add_constraint(Constraint::Horizontal { line: axis })
                .map_err(err)?;
            sk.add_constraint(Constraint::PointOnLine {
                point: center,
                line: axis,
            })
            .map_err(err)?;
            sk.add_constraint(Constraint::Diameter {
                circle: arc,
                value: 2.0 * r,
            })
            .map_err(err)?;
            Ok(Some(axis))
        }
        PrimitiveKind::Torus => {
            let s = clicks[1];
            let big = (s - c).norm();
            let small = (clicks[2] - s).norm();
            if big < 1e-6 || small < 1e-6 {
                return Err(bad("the torus has no size"));
            }
            if small >= big - 1e-6 {
                return Err(bad(
                    "the section must not reach the axis (section radius < distance to the centre)",
                ));
            }
            // The axis through the centre, perpendicular to centre → section.
            let u = (s - c) / big;
            let t = crate::sketch::Vector2::new(-u.y, u.x);
            let half = big + small;
            let axis = sk.add_line(c - t * half, c + t * half);
            sk.set_style(axis, Style::Centerline).map_err(err)?;
            let circle = sk.add_circle(s, small).map_err(err)?;
            sk.add_constraint(Constraint::Diameter {
                circle,
                value: 2.0 * small,
            })
            .map_err(err)?;
            if let Some(crate::sketch::Entity::Circle { center, .. }) = sk.entity(circle).cloned()
            {
                sk.add_constraint(Constraint::PointLineDistance {
                    point: center,
                    line: axis,
                    value: big,
                })
                .map_err(err)?;
            }
            Ok(Some(axis))
        }
    }
}

/// Creates the primitive's sketch on `placement` (no projections) and
/// draws it. Returns the sketch index and, for Sphere / Torus, the axis.
pub fn primitive_sketch(
    part: &mut PartDocument,
    kind: PrimitiveKind,
    placement: SketchPlacement,
    clicks: &[Point2],
) -> Result<(usize, Option<AxisRef>), PartError> {
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    let i = part.create_sketch(placement, &opts)?;
    match draw(&mut part.sketches[i].sketch, kind, clicks) {
        Ok(axis) => {
            let _ = part.sketches[i].sketch.solve();
            Ok((i, axis.map(AxisRef::SketchLine)))
        }
        Err(e) => {
            let _ = part.delete_sketch(i);
            Err(e)
        }
    }
}

/// The settings of a primitive's feature: an Extrude (Box, Cylinder; its
/// profiles are filled in) or the operation of the full Revolve (Sphere,
/// Torus).
#[derive(Clone, Debug)]
pub enum PrimitiveFeature {
    /// Box / Cylinder.
    Extrude(ExtrudeFeature),
    /// Sphere / Torus (always Full, Solid).
    Revolve(super::Operation),
}

impl PrimitiveFeature {
    /// An extrusion of `height` with the default operation for `part`.
    pub fn extrude(part: &PartDocument, height: Real) -> Self {
        let mut e = ExtrudeFeature::new(Vec::new(), height);
        e.operation = super::Operation::default_for(&part.body);
        PrimitiveFeature::Extrude(e)
    }
}

/// **Primitives**: sketch + feature in one step (the API form of the
/// ribbon commands). On failure the part is left unchanged.
pub fn add_primitive(
    part: &mut PartDocument,
    kind: PrimitiveKind,
    placement: SketchPlacement,
    clicks: &[Point2],
    feature: PrimitiveFeature,
) -> Result<String, PartError> {
    let before = part.clone();
    let result = (|| {
        let (i, axis) = primitive_sketch(part, kind, placement, clicks)?;
        let regions = part.sketches[i].sketch.regions();
        let r = regions
            .first()
            .ok_or_else(|| bad("the primitive has no closed profile"))?;
        let profiles = vec![ProfileRef::of(r, r.interior_point())];
        match (kind.revolves(), feature) {
            (false, PrimitiveFeature::Extrude(mut e)) => {
                e.profiles = profiles;
                e.open = None;
                part.add_extrude(i, e)
            }
            (true, PrimitiveFeature::Revolve(op)) => {
                let axis = axis.ok_or_else(|| bad("the primitive has no axis"))?;
                let mut r = RevolveFeature::new(profiles, axis);
                r.operation = op;
                part.add_revolve(i, r)
            }
            _ => Err(bad("this primitive takes the other kind of feature")),
        }
    })();
    if result.is_err() {
        *part = before;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::part::{Operation, OriginPlane, PlaneRef};
    use std::f64::consts::PI;

    fn xy() -> SketchPlacement {
        SketchPlacement::on_origin_plane(OriginPlane::XY)
    }

    /// The exact kernel's volume (tessellation is too coarse on small
    /// spheres).
    fn volume(part: &PartDocument) -> Real {
        assert!(part.exact, "built by the exact kernel");
        part.exact_bodies
            .0
            .iter()
            .map(|s| s.measure().unwrap().0)
            .sum()
    }

    #[allow(dead_code)]
    fn mesh_volume(part: &PartDocument) -> Real {
        let opts = crate::kernel::TessellationOptions::default();
        part.body
            .solids()
            .map(|(sid, _)| {
                let m = part.body.tessellate_solid(sid, &opts).unwrap();
                m.faces
                    .triangles
                    .iter()
                    .map(|t| {
                        let [p, q, s] = t.map(|i| m.faces.positions[i as usize].coords);
                        p.dot(&q.cross(&s)) / 6.0
                    })
                    .sum::<Real>()
            })
            .sum()
    }

    fn close(got: Real, want: Real, rel: Real, what: &str) {
        assert!(
            (got - want).abs() <= rel * want.abs().max(1.0),
            "{what}: {got} vs {want}"
        );
    }

    fn p(x: Real, y: Real) -> Point2 {
        Point2::new(x, y)
    }

    /// Box and Cylinder primitives: several sizes, analytic volumes, fully
    /// dimensioned sketches.
    #[test]
    fn box_and_cylinder_volumes() {
        for (c, q, h) in [
            (p(0.0, 0.0), p(20.0, 10.0), 5.0),
            (p(5.0, 5.0), p(-15.0, 25.0), 12.0),
            (p(-30.0, 7.0), p(-20.0, -3.0), 1.5),
            (p(100.0, 0.0), p(140.0, 60.0), 30.0),
            (p(0.0, 0.0), p(0.5, 0.25), 0.1),
        ] {
            let mut part = PartDocument::default();
            let f = PrimitiveFeature::extrude(&part, h);
            add_primitive(&mut part, PrimitiveKind::Box, xy(), &[c, q], f).unwrap();
            let d = q - c;
            close(volume(&part), 4.0 * d.x.abs() * d.y.abs() * h, 3e-4, "box");
            assert_eq!(part.sketches.len(), 1);
            assert!(!part.sketches[0].visible, "consumed and hidden");
            let mut part = PartDocument::default();
            let f = PrimitiveFeature::extrude(&part, h);
            add_primitive(&mut part, PrimitiveKind::Cylinder, xy(), &[c, q], f).unwrap();
            let r = d.norm();
            close(volume(&part), PI * r * r * h, 3e-4, "cylinder");
        }
    }

    /// Sphere and Torus primitives: full revolves with analytic volumes.
    #[test]
    fn sphere_and_torus_volumes() {
        for (c, r) in [
            (p(0.0, 0.0), 10.0),
            (p(30.0, -5.0), 2.5),
            (p(-40.0, 40.0), 25.0),
            (p(1.0, 1.0), 0.5),
            (p(0.0, 60.0), 7.0),
        ] {
            let mut part = PartDocument::default();
            add_primitive(
                &mut part,
                PrimitiveKind::Sphere,
                xy(),
                &[c, c + crate::sketch::Vector2::new(0.6 * r, 0.8 * r)],
                PrimitiveFeature::Revolve(Operation::NewSolid),
            )
            .unwrap();
            close(volume(&part), 4.0 / 3.0 * PI * r.powi(3), 3e-4, "sphere");
        }
        for (c, s, rim) in [
            (p(0.0, 0.0), p(20.0, 0.0), p(25.0, 0.0)),
            (p(10.0, 10.0), p(10.0, 40.0), p(13.0, 40.0)),
            (p(0.0, 0.0), p(-15.0, -15.0), p(-15.0, -10.0)),
            (p(-5.0, 2.0), p(45.0, 2.0), p(45.0, 22.0)),
            (p(0.0, 0.0), p(3.0, 4.0), p(3.0, 5.0)),
        ] {
            let mut part = PartDocument::default();
            add_primitive(
                &mut part,
                PrimitiveKind::Torus,
                xy(),
                &[c, s, rim],
                PrimitiveFeature::Revolve(Operation::NewSolid),
            )
            .unwrap();
            let big = (s - c).norm();
            let small = (rim - s).norm();
            close(volume(&part), 2.0 * PI * PI * big * small * small, 3e-4, "torus");
        }
    }

    /// Primitives combine with the body: Join, Cut, Intersect and New
    /// Solid on a 40×40×20 block.
    #[test]
    fn primitives_with_operations() {
        let block = || {
            let mut part = PartDocument::default();
            let f = PrimitiveFeature::extrude(&part, 20.0);
            add_primitive(
                &mut part,
                PrimitiveKind::Box,
                xy(),
                &[p(0.0, 0.0), p(20.0, 20.0)],
                f,
            )
            .unwrap();
            part
        };
        let base = 40.0 * 40.0 * 20.0;
        // Cylinder r 5 through the top: Cut 10 deep (sketch on the top face).
        let top = || SketchPlacement {
            plane: PlaneRef::Face("Extrusion1:End".into()),
            ..Default::default()
        };
        let mut part = block();
        let mut e = ExtrudeFeature::new(Vec::new(), 10.0);
        e.operation = Operation::Cut;
        e.direction = crate::part::ExtentDirection::Flipped;
        add_primitive(
            &mut part,
            PrimitiveKind::Cylinder,
            top(),
            &[p(0.0, 0.0), p(5.0, 0.0)],
            PrimitiveFeature::Extrude(e),
        )
        .unwrap();
        close(volume(&part), base - PI * 25.0 * 10.0, 3e-4, "cut cylinder");
        // Join a box on top.
        let mut part = block();
        let mut e = ExtrudeFeature::new(Vec::new(), 5.0);
        e.operation = Operation::Join;
        add_primitive(
            &mut part,
            PrimitiveKind::Box,
            top(),
            &[p(0.0, 0.0), p(5.0, 5.0)],
            PrimitiveFeature::Extrude(e),
        )
        .unwrap();
        close(volume(&part), base + 100.0 * 5.0, 3e-4, "joined box");
        assert_eq!(part.body.solids().count(), 1);
        // Sphere r 10 at the top face centre: Cut removes a half ball.
        let mut part = block();
        add_primitive(
            &mut part,
            PrimitiveKind::Sphere,
            top(),
            &[p(0.0, 0.0), p(10.0, 0.0)],
            PrimitiveFeature::Revolve(Operation::Cut),
        )
        .unwrap();
        close(volume(&part), base - 2.0 / 3.0 * PI * 1000.0, 3e-4, "cut sphere");
        // Join a sphere: adds the upper half.
        let mut part = block();
        add_primitive(
            &mut part,
            PrimitiveKind::Sphere,
            top(),
            &[p(0.0, 0.0), p(10.0, 0.0)],
            PrimitiveFeature::Revolve(Operation::Join),
        )
        .unwrap();
        close(volume(&part), base + 2.0 / 3.0 * PI * 1000.0, 3e-4, "join sphere");
        // Intersect with a sphere: only the lower half ball is left.
        let mut part = block();
        add_primitive(
            &mut part,
            PrimitiveKind::Sphere,
            top(),
            &[p(0.0, 0.0), p(10.0, 0.0)],
            PrimitiveFeature::Revolve(Operation::Intersect),
        )
        .unwrap();
        close(volume(&part), 2.0 / 3.0 * PI * 1000.0, 3e-4, "intersect sphere");
        // New Solid torus beside the block: two bodies.
        let mut part = block();
        add_primitive(
            &mut part,
            PrimitiveKind::Torus,
            xy(),
            &[p(80.0, 0.0), p(95.0, 0.0), p(98.0, 0.0)],
            PrimitiveFeature::Revolve(Operation::NewSolid),
        )
        .unwrap();
        assert_eq!(part.body.solids().count(), 2);
        close(
            volume(&part),
            base + 2.0 * PI * PI * 15.0 * 9.0,
            3e-4,
            "block + torus",
        );
    }

    /// Bad clicks are refused and leave the part unchanged; a saved part
    /// with primitives loads and rebuilds the same.
    #[test]
    fn refusals_and_save_load() {
        let mut part = PartDocument::default();
        let f = PrimitiveFeature::extrude(&part, 5.0);
        let snapshot = serde_json::to_string(&part).unwrap();
        for (kind, clicks) in [
            (PrimitiveKind::Box, vec![p(0.0, 0.0), p(10.0, 0.0)]),
            (PrimitiveKind::Cylinder, vec![p(0.0, 0.0), p(0.0, 0.0)]),
            (PrimitiveKind::Torus, vec![p(0.0, 0.0), p(5.0, 0.0), p(5.0, 6.0)]),
            (PrimitiveKind::Sphere, vec![p(0.0, 0.0)]),
        ] {
            let feat = if kind.revolves() {
                PrimitiveFeature::Revolve(Operation::NewSolid)
            } else {
                f.clone()
            };
            assert!(add_primitive(&mut part, kind, xy(), &clicks, feat).is_err());
            assert_eq!(serde_json::to_string(&part).unwrap(), snapshot, "{kind:?}");
        }
        add_primitive(
            &mut part,
            PrimitiveKind::Box,
            xy(),
            &[p(0.0, 0.0), p(10.0, 5.0)],
            f,
        )
        .unwrap();
        add_primitive(
            &mut part,
            PrimitiveKind::Torus,
            xy(),
            &[p(50.0, 0.0), p(60.0, 0.0), p(62.0, 0.0)],
            PrimitiveFeature::Revolve(Operation::NewSolid),
        )
        .unwrap();
        let v = volume(&part);
        let json = serde_json::to_string(&part).unwrap();
        let mut back: PartDocument = serde_json::from_str(&json).unwrap();
        back.regenerate().unwrap();
        assert!(back.feature_health.is_empty());
        close(volume(&back), v, 1e-9, "reloaded");
    }

    /// Cross-feature: a Box primitive, then Fillet, Shell and a pattern of
    /// a Cylinder primitive cut.
    #[test]
    fn primitive_then_fillet_shell_pattern() {
        use crate::part::{BlendFeature, ShellFeature, blend::edge_ref};
        let mut part = PartDocument::default();
        let f = PrimitiveFeature::extrude(&part, 20.0);
        add_primitive(
            &mut part,
            PrimitiveKind::Box,
            xy(),
            &[p(0.0, 0.0), p(30.0, 20.0)],
            f,
        )
        .unwrap();
        let base = 60.0 * 40.0 * 20.0;
        close(volume(&part), base, 3e-4, "box");
        // Fillet one vertical edge (x = 30, y = 20) with r = 5: removes
        // (1 − π/4)·r²·h.
        let mut trial = part.clone();
        let edge = trial
            .body
            .edges()
            .filter_map(|(e, _)| edge_ref(&trial.body, e))
            .find(|r| {
                (r.hint[0] - 30.0).abs() < 1e-6
                    && (r.hint[1] - 20.0).abs() < 1e-6
                    && (r.hint[2] - 10.0).abs() < 1e-6
            })
            .expect("the vertical edge");
        trial
            .add_blend(BlendFeature::fillet(vec![edge], 5.0))
            .unwrap();
        close(
            volume(&trial),
            base - (1.0 - PI / 4.0) * 25.0 * 20.0,
            3e-4,
            "filleted",
        );
        // Shell 2 mm with the top face open.
        let mut trial = part.clone();
        let top = trial.body.find_face("Extrusion1:End").unwrap();
        let mut sh = ShellFeature::new(2.0);
        sh.remove
            .push(crate::part::shell::face_ref(&trial.body, top, None).unwrap());
        trial.add_shell(sh).unwrap();
        close(volume(&trial), base - 56.0 * 36.0 * 18.0, 3e-4, "shelled");
        // A cylinder cut on the top face, patterned 3× along X.
        let top = SketchPlacement {
            plane: PlaneRef::Face("Extrusion1:End".into()),
            ..Default::default()
        };
        let mut e = ExtrudeFeature::new(Vec::new(), 5.0);
        e.operation = Operation::Cut;
        e.direction = crate::part::ExtentDirection::Flipped;
        let id = add_primitive(
            &mut part,
            PrimitiveKind::Cylinder,
            top,
            &[p(-20.0, 0.0), p(-17.0, 0.0)],
            PrimitiveFeature::Extrude(e),
        )
        .unwrap();
        let hole = PI * 9.0 * 5.0;
        close(volume(&part), base - hole, 3e-4, "one hole");
        let mut pat = crate::part::PatternFeature::rectangular();
        pat.features = vec![id];
        if let crate::part::PatternKind::Rectangular { first, .. } = &mut pat.kind {
            first.dir = Some(crate::part::PullRef::Line(crate::part::LineRef::Origin(0)));
            first.count = 3;
            first.spacing = 20.0;
        }
        part.add_pattern(pat).unwrap();
        close(volume(&part), base - 3.0 * hole, 3e-4, "three holes");
    }
}
