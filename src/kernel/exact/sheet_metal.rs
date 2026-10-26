//! Sheet metal (`docs/spec/5.6-sheet-metal.md`, stage S1): SolidCraft's sheet
//! body model (`solidcraft_doc::sheet`, MIT OR Apache-2.0) behind our types.
//! A sheet is kept as its flat pattern — the base panel and the flanges
//! hanging off panel edges, each a bend zone BA = θ·(R + K·T) and a panel —
//! and folded to a solid on demand. No SolidCraft type leaves this module.

use std::sync::Arc;

use solidcraft_doc::sheet as sd;

use super::{ExactProfile, ExactSolid, guarded, p3, plane_of, v3};
use crate::kernel::{Plane, Point3, Real};

/// A sheet metal rule's values (mm; K unitless).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RuleValues {
    /// Thickness.
    pub thickness: Real,
    /// K-factor (neutral layer at K·T from the inside).
    pub k_factor: Real,
    /// Inner bend radius.
    pub bend_radius: Real,
    /// Bend relief width and depth.
    pub relief_width: Real,
    /// Bend relief depth.
    pub relief_depth: Real,
    /// Corner relief size (round corners).
    pub corner_relief: Real,
    /// Gap between flanges meeting at a corner.
    pub gap: Real,
    /// The relief cut where two bends meet at a corner.
    pub corner_shape: CornerShape,
}

/// The corner relief where two bends meet at a corner of the base.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CornerShape {
    /// No cut: both bends end square at the corner.
    Notch,
    /// A round relief `corner_relief` across, centred on the corner.
    #[default]
    Round,
    /// A square relief `corner_relief` across.
    Rectangular,
    /// No relief: each bend's end is torn straight to its panel.
    Tear,
}

/// Where a flange's bend lies against the edge picked (Inventor's Bend
/// Position).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BendPosition {
    /// The flange's outer face flush with the edge: the base keeps its size.
    Inside,
    /// The bend starts at the edge: the flange is outside the base.
    Outside,
    /// The bend centred on the edge.
    Middle,
}

impl BendPosition {
    fn key(self) -> &'static str {
        match self {
            BendPosition::Inside => "inside",
            BendPosition::Outside => "outside",
            BendPosition::Middle => "middle",
        }
    }
}

/// A sheet body's model (opaque; cheap to clone).
#[derive(Clone)]
pub struct SheetModel(Arc<sd::SheetBody>);

impl std::fmt::Debug for SheetModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SheetModel({} flanges)", self.0.flanges.len())
    }
}

/// One flange to make: a point on the edge (world), its height and angle.
#[derive(Clone, Copy, Debug)]
pub struct FlangeEdge {
    /// A point on the sheet edge.
    pub at: Point3,
    /// Height to the outer face (from the virtual sharp).
    pub height: Real,
    /// Bend angle (radians, 0…π).
    pub angle: Real,
    /// Held back from the edge's (start, end), mm along it (Width
    /// extents; 0 = the whole edge).
    pub offsets: (Real, Real),
}

/// A bend line of the flat pattern (flat coordinates, for drawings / DXF).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BendLine {
    /// Angle (degrees).
    pub angle_deg: Real,
    /// Inner radius.
    pub radius: Real,
    /// Bends up (towards the top face).
    pub up: bool,
    /// Bend allowance.
    pub allowance: Real,
    /// Centre line ends (flat coordinates).
    pub start: [Real; 2],
    /// End.
    pub end: [Real; 2],
}

fn rule_of(rv: &RuleValues) -> sd::RuleValues {
    sd::RuleValues {
        thickness: rv.thickness,
        k_factor: rv.k_factor,
        bend_radius: rv.bend_radius,
        relief_width: rv.relief_width,
        relief_depth: rv.relief_depth,
        corner_relief: rv.corner_relief,
        corner_relief_shape: match rv.corner_shape {
            CornerShape::Notch => sd::ReliefShape::Notch,
            CornerShape::Round => sd::ReliefShape::Round,
            CornerShape::Rectangular => sd::ReliefShape::Rectangular,
            CornerShape::Tear => sd::ReliefShape::Tear,
        },
        hem_gap: 0.01,
        gap: rv.gap,
    }
}

/// The sheet's solid with its faces named: flat faces by the sheet's own
/// persistent names under `prefix` (`<prefix>:<panel>:top`), the others
/// (bends, rims it cannot place) `<prefix>:Bend:<k>` in face order.
fn named_solid(m: &sd::SheetBody, prefix: &str, flat: bool) -> Result<ExactSolid, String> {
    let body = guarded("sheet metal", || {
        if flat { m.flat_solid() } else { m.solid() }
            .map_err(|e| solidcraft_kernel::KernelError::Failed(e.to_string()))
    })?;
    let mut s = ExactSolid {
        sheet: false,
        name: Some(prefix.to_owned()),
        body,
        face_names: Vec::new(),
        profiles: Vec::new(),
    };
    let samples = super::face_samples(&s)?;
    let mut k = 0usize;
    s.face_names = samples
        .iter()
        .map(|p| {
            let named = p
                .and_then(|p| m.panel_face_at(v3(&p), m.t * 0.2))
                .map(|(n, _)| n);
            Some(match named {
                Some(n) => format!("{prefix}:{n}"),
                None => {
                    k += 1;
                    format!("{prefix}:Bend:{k}")
                }
            })
        })
        .collect();
    Ok(s)
}

/// A straight sketch curve of the base profile: its id and ends in the
/// sketch plane — the base panel's edges are named after them.
pub type BaseCurve = (String, [Real; 2], [Real; 2]);

/// Face (base flange): the profile in `plane`, `rv.thickness` thick along
/// its normal (or against it, `flip`); `curves` name its edges.
pub fn face(
    prefix: &str,
    rv: &RuleValues,
    plane: &Plane,
    prof: &ExactProfile,
    curves: &[BaseCurve],
    flip: bool,
) -> Result<(ExactSolid, SheetModel), String> {
    if rv.thickness.is_nan() || rv.thickness <= 0.0 {
        return Err("the sheet thickness must be positive".into());
    }
    let mut pl = plane_of(plane)?;
    if flip {
        let n = plane.normal.into_inner() * rv.thickness;
        pl = plane_of(&Plane {
            origin: plane.origin - n,
            ..plane.clone()
        })?;
    }
    // A whole circle as one arc (one vertex) cannot be sewn: quarter
    // turns at most.
    let prof = super::sweep::split_closed_arcs(prof);
    let mut m = sd::base_flange(prefix, "rule", &rule_of(rv), pl, prof.region.clone());
    m.base_curves = curves
        .iter()
        .map(|(id, a, b)| {
            (
                id.clone(),
                solidcraft_geom::Vec2::new(a[0], a[1]),
                solidcraft_geom::Vec2::new(b[0], b[1]),
            )
        })
        .collect();
    let s = named_solid(&m, prefix, false)?;
    Ok((s, SheetModel(Arc::new(m))))
}

/// Tags the flanges added after `n0` `<tag>.<k>` (their edges and faces are
/// named after it) and rebuilds the solid.
fn finish(
    mut next: sd::SheetBody,
    n0: usize,
    prefix: &str,
    tag: &str,
) -> Result<(ExactSolid, SheetModel), String> {
    for (k, f) in next.flanges.iter_mut().enumerate().skip(n0) {
        f.tag = format!("{tag}.{}", k - n0 + 1);
    }
    let s = named_solid(&next, prefix, false)?;
    Ok((s, SheetModel(Arc::new(next))))
}

fn guarded_sheet<T>(what: &str, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    crate::diagnostics::crash::guard(f)
        .map_err(|_| format!("{what}: internal error in the sheet metal model"))?
}

/// Hem: a 180° bend folded back over the face at each edge (a point on it),
/// `length` to its outer face, `gap` between the hem and the sheet.
pub fn hem(
    m: &SheetModel,
    prefix: &str,
    tag: &str,
    edges: &[Point3],
    length: Real,
    gap: Real,
    flip: bool,
) -> Result<(ExactSolid, SheetModel), String> {
    let mut next = (*m.0).clone();
    if next.flat {
        return Err("refold the sheet before adding hems".into());
    }
    let n0 = next.flanges.len();
    guarded_sheet("Hem", || {
        for e in edges {
            next.add_hem(v3(e), length, gap, flip)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })?;
    finish(next, n0, prefix, tag)
}

/// Where a fold's line lies in its bend zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoldLine {
    /// The middle of the bend zone.
    Centerline,
    /// Its start (the fixed side).
    Start,
    /// Its end.
    End,
    /// Where the outer faces meet (the mould line).
    Mould,
}

impl FoldLine {
    fn key(self) -> &'static str {
        match self {
            FoldLine::Centerline => "centerline",
            FoldLine::Start => "start",
            FoldLine::End => "end",
            FoldLine::Mould => "mould",
        }
    }
}

/// Fold: bends a panel along the line `a`–`b` on its face; `fixed` (a point
/// of the side that stays) else the larger side. Returns notes too.
#[allow(clippy::too_many_arguments)]
pub fn fold(
    m: &SheetModel,
    prefix: &str,
    tag: &str,
    a: Point3,
    b: Point3,
    angle: Real,
    radius: Real,
    line: FoldLine,
    flip: bool,
    fixed: Option<Point3>,
) -> Result<(ExactSolid, SheetModel, Vec<String>), String> {
    let mut next = (*m.0).clone();
    if next.flat {
        return Err("refold the sheet before folding it".into());
    }
    let n0 = next.flanges.len();
    let mut warning = None;
    guarded_sheet("Fold", || {
        next.add_fold(
            v3(&a),
            v3(&b),
            angle,
            radius,
            line.key(),
            flip,
            fixed.as_ref().map(v3),
            &mut warning,
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
    })?;
    let (s, model) = finish(next, n0, prefix, tag)?;
    Ok((s, model, warning.into_iter().collect()))
}

/// Contour Flange: a new sheet from an open chain of lines `pts` in
/// `plane` (the material on the right of travel, left when `flip`), `width`
/// along the plane's normal; every corner becomes a bend of the rule.
pub fn contour(
    prefix: &str,
    rv: &RuleValues,
    plane: &Plane,
    pts: &[[Real; 2]],
    width: Real,
    flip: bool,
) -> Result<(ExactSolid, SheetModel), String> {
    let pl = plane_of(plane)?;
    let pts: Vec<_> = pts
        .iter()
        .map(|p| solidcraft_geom::Vec2::new(p[0], p[1]))
        .collect();
    let mut m = guarded_sheet("Contour Flange", || {
        sd::contour_flange(prefix, "rule", &rule_of(rv), &pl, &pts, width, flip)
            .map_err(|e| e.to_string())
    })?;
    for (k, f) in m.flanges.iter_mut().enumerate() {
        f.tag = format!("{prefix}.{}", k + 1);
    }
    // The first segment is the base panel: its two long edges by name.
    let n = m.base.outer.ccw().polyline(1e-3).len();
    let outline = m.base.outer.ccw().polyline(1e-3);
    m.base_curves = (0..n)
        .filter_map(|k| {
            let (a, b) = (*outline.get(k)?, *outline.get((k + 1) % n)?);
            (a.dist(b) > 1e-9).then(|| (format!("E{}", k + 1), a, b))
        })
        .collect();
    let s = named_solid(&m, prefix, false)?;
    Ok((s, SheetModel(Arc::new(m))))
}

/// Unfold (`flat`) or Refold the whole sheet: features after it work on
/// the flat pattern until it is refolded.
pub fn set_flat(
    m: &SheetModel,
    prefix: &str,
    flat: bool,
) -> Result<(ExactSolid, SheetModel), String> {
    let mut next = (*m.0).clone();
    if next.flat == flat {
        return Err(if flat {
            "the sheet is unfolded already".into()
        } else {
            "the sheet is not unfolded".into()
        });
    }
    next.flat = flat;
    let s = named_solid(&next, prefix, false)?;
    Ok((s, SheetModel(Arc::new(next))))
}

/// Whether the sheet is unfolded.
pub fn is_flat(m: &SheetModel) -> bool {
    m.0.flat
}

/// Cut: the profile's region through the sheet on the face its sketch lies
/// on (kept in the flat pattern as a cut-out).
pub fn cut(
    m: &SheetModel,
    prefix: &str,
    plane: &Plane,
    profiles: &[ExactProfile],
) -> Result<(ExactSolid, SheetModel), String> {
    let pl = plane_of(plane)?;
    let regions: Vec<_> = profiles
        .iter()
        .map(|p| super::sweep::split_closed_arcs(p).region)
        .collect();
    let mut next = (*m.0).clone();
    guarded_sheet("Cut", || {
        next.cut_through(&pl, &regions)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })?;
    let s = named_solid(&next, prefix, false)?;
    Ok((s, SheetModel(Arc::new(next))))
}

/// The flat pattern's material as closed loops (flat coordinates; outer
/// loops counter-clockwise, cut-outs clockwise), for DXF.
pub fn flat_loops(m: &SheetModel) -> Result<Vec<Vec<[Real; 2]>>, String> {
    let regions = m.0.flat_regions().map_err(|e| e.to_string())?;
    let tol = (m.0.t * 0.01).max(1e-3);
    let mut out = Vec::new();
    for r in regions {
        out.push(
            r.outer
                .ccw()
                .polyline(tol)
                .iter()
                .map(|p| [p.x, p.y])
                .collect(),
        );
        for h in &r.holes {
            out.push(h.polyline(tol).iter().map(|p| [p.x, p.y]).collect());
        }
    }
    Ok(out)
}

/// The persistent name of the sheet edge nearest `at` (world).
pub fn edge_name(m: &SheetModel, at: Point3) -> Option<String> {
    m.0.edge_name(v3(&at))
}

/// The ends of the sheet edge nearest `at` (world), in the order flange
/// offsets count them, and whether it is on the top face.
pub fn edge_ends(m: &SheetModel, at: Point3) -> Option<(Point3, Point3, bool)> {
    m.0.edge_ends(v3(&at))
        .map(|(a, b, top)| (p3(a), p3(b), top))
}

/// Convert a solid to a sheet (SolidWorks / Inventor Convert to Sheet
/// Metal): `face` is a point on the face that stays put, `bends` points on
/// the sharp edges that become bends; a shelled body keeps its wall
/// thickness, any other body becomes the outside of a sheet of the rule's
/// thickness. Faces are named under `prefix`.
pub fn convert(
    solid: &ExactSolid,
    prefix: &str,
    rv: &RuleValues,
    face: Point3,
    bends: &[Point3],
) -> Result<(ExactSolid, SheetModel), String> {
    let mb = solidcraft_doc::ModelBody::new(prefix.to_owned(), solid.body.clone(), 0);
    let picks: Vec<_> = bends.iter().map(v3).collect();
    let m = crate::diagnostics::crash::guard(|| {
        sd::convert_body(&mb, "rule", &rule_of(rv), v3(&face), &picks).map_err(|e| e.to_string())
    })
    .map_err(|_| "Convert to Sheet Metal: internal error in the sheet metal model".to_owned())??;
    let mut m = m;
    // Edge names: the fixed face's straight edges `E<k>` (the base's
    // outline in order), the panels across the bends `<prefix>.<k>` — so
    // flanges on the converted sheet store names, as on a Face.
    let pts = m.base.outer.ccw().polyline(1e-3);
    let n = pts.len();
    m.base_curves = (0..n)
        .filter_map(|k| {
            let (a, b) = (*pts.get(k)?, *pts.get((k + 1) % n)?);
            (a.dist(b) > 1e-9).then(|| (format!("E{}", k + 1), a, b))
        })
        .collect();
    for (k, f) in m.flanges.iter_mut().enumerate() {
        f.tag = format!("{prefix}.{}", k + 1);
    }
    let mut s = named_solid(&m, prefix, false)?;
    // The sheet keeps the solid's name (its appearance, later features).
    s.name = solid.name.clone();
    Ok((s, SheetModel(Arc::new(m))))
}

/// A point on the sheet edge named `name`, nearest `near`.
pub fn edge_point(m: &SheetModel, name: &str, near: Point3) -> Option<Point3> {
    m.0.edge_by_name(name, v3(&near)).map(p3)
}

/// Flanges on sheet edges (one feature `tag`): every edge trimmed back, the
/// corners between them closed with the rule's gap. Returns the new solid
/// and model, and notes on corners left open.
pub fn flanges(
    m: &SheetModel,
    prefix: &str,
    tag: &str,
    edges: &[FlangeEdge],
    radius: Real,
    position: BendPosition,
    flip: bool,
) -> Result<(ExactSolid, SheetModel, Vec<String>), String> {
    let mut next = (*m.0).clone();
    if next.flat {
        return Err("refold the sheet before adding flanges".into());
    }
    let each: Vec<sd::EdgeFlange> = edges
        .iter()
        .map(|e| sd::EdgeFlange {
            pick: v3(&e.at),
            height: e.height,
            angle: e.angle,
        })
        .collect();
    let spans: Vec<(Real, Real)> = edges.iter().map(|e| e.offsets).collect();
    let n0 = next.flanges.len();
    let notes = crate::diagnostics::crash::guard(|| {
        next.add_flanges_spans(&each, &spans, radius, position.key(), flip)
            .map(|(_, n)| n)
            .map_err(|e| e.to_string())
    })
    .map_err(|_| "Flange: internal error in the sheet metal model".to_owned())??;
    for (k, f) in next.flanges.iter_mut().enumerate().skip(n0) {
        f.tag = format!("{tag}.{}", k - n0 + 1);
    }
    let s = named_solid(&next, prefix, false)?;
    Ok((s, SheetModel(Arc::new(next)), notes))
}

/// The flat pattern as a solid (the outline with its cut-outs, the
/// thickness) — the same face names where the sheet can tell them.
pub fn flat_solid(m: &SheetModel, prefix: &str) -> Result<ExactSolid, String> {
    named_solid(&m.0, prefix, true)
}

/// The flat pattern's extents (length, width).
pub fn flat_size(m: &SheetModel) -> Result<(Real, Real), String> {
    m.0.flat_size().map_err(|e| e.to_string())
}

/// The bend lines of the flat pattern.
pub fn bends(m: &SheetModel) -> Vec<BendLine> {
    m.0.bends()
        .into_iter()
        .map(|b| BendLine {
            angle_deg: b.angle_deg,
            radius: b.radius,
            up: b.up,
            allowance: b.allowance,
            start: [b.start.x, b.start.y],
            end: [b.end.x, b.end.y],
        })
        .collect()
}

/// The thickness of the sheet.
pub fn thickness(m: &SheetModel) -> Real {
    m.0.t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::Vector3;
    use crate::kernel::extrude::{ProfileSegment, Segment2};

    fn rect(w: Real, h: Real) -> ExactProfile {
        let p = |x, y| nalgebra::Point2::new(x, y);
        let pts = [p(0.0, 0.0), p(w, 0.0), p(w, h), p(0.0, h)];
        let segs: Vec<ProfileSegment> = (0..4)
            .map(|i| ProfileSegment {
                geometry: Segment2::Line {
                    a: pts[i],
                    b: pts[(i + 1) % 4],
                },
                name: format!("L{i}:0"),
            })
            .collect();
        super::super::profile(&segs, &[]).unwrap()
    }

    fn rule() -> RuleValues {
        RuleValues {
            thickness: 2.0,
            k_factor: 0.44,
            bend_radius: 2.0,
            relief_width: 2.0,
            relief_depth: 1.0,
            corner_relief: 8.0,
            gap: 2.0,
            corner_shape: CornerShape::Round,
        }
    }

    /// A 100 × 60 plate 2 thick; a 90° flange 20 high on its front edge
    /// (inside: the base keeps its size); the flat pattern is longer by the
    /// developed flange.
    #[test]
    fn face_flange_and_flat_pattern() {
        let rv = rule();
        let curves: Vec<BaseCurve> = [
            ([0.0, 0.0], [100.0, 0.0]),
            ([100.0, 0.0], [100.0, 60.0]),
            ([100.0, 60.0], [0.0, 60.0]),
            ([0.0, 60.0], [0.0, 0.0]),
        ]
        .iter()
        .enumerate()
        .map(|(i, (a, b))| (format!("L{i}"), *a, *b))
        .collect();
        let (s, m) = face(
            "Face1",
            &rv,
            &Plane::xy(),
            &rect(100.0, 60.0),
            &curves,
            false,
        )
        .unwrap();
        let v = s.volume_at(1e-3).unwrap();
        assert!((v - 100.0 * 60.0 * 2.0).abs() < 1e-6 * v, "{v}");
        assert!(s.face_names.iter().all(Option::is_some));
        let edge = Point3::new(50.0, 0.0, 2.0);
        let name = edge_name(&m, edge).expect("the front edge has a name");
        let at = edge_point(&m, &name, edge).unwrap();
        assert!((at - edge).norm() < 1.0, "{at}");
        let (f, m2, _) = flanges(
            &m,
            "Face1",
            "Flange1",
            &[FlangeEdge {
                at,
                height: 20.0,
                angle: std::f64::consts::FRAC_PI_2,
                offsets: (0.0, 0.0),
            }],
            rv.bend_radius,
            BendPosition::Inside,
            false,
        )
        .unwrap();
        let (lo, hi) = super::super::bounds(&f).unwrap();
        // The flange stands 20 high and the base keeps its 60 mm depth.
        assert!((hi.z - lo.z - 20.0).abs() < 1.0, "{lo} {hi}");
        assert!((hi.y - lo.y - 60.0).abs() < 1.0, "{lo} {hi}");
        // Flat: 60 − setback + BA + leg, BA = π/2·(R + K·T).
        let (a, b) = flat_size(&m2).unwrap();
        let ba = std::f64::consts::FRAC_PI_2 * (2.0 + 0.44 * 2.0);
        let want = 60.0 - (2.0 + 2.0) + ba + (20.0 - (2.0 + 2.0));
        let got = a.min(b);
        assert!((got - want).abs() < 0.05, "flat {a} × {b}, want {want}");
        assert_eq!(bends(&m2).len(), 1);
        let flat = flat_solid(&m2, "Face1").unwrap();
        let (lo, hi) = super::super::bounds(&flat).unwrap();
        assert!((hi.z - lo.z - 2.0).abs() < 1.0);
        let _ = Vector3::z();
    }
}
