//! The parts the gear and key generators place (spec 6.2-design-calculators
//! §3.3–3.4): a spur gear's teeth as one extrusion of its exact tooth outline
//! (the same layout the geometry factor reads, `gear::tooth_side`), and a
//! parallel key. No Boolean: each part is one sketch and one Extrude.

use std::f64::consts::PI;

use super::gear::{GearPair, Geometry, tooth_side};
use super::key::{Ends, KeySize};
use crate::part::{
    ExtrudeFeature, OriginPlane, PartDocument, ProfileRef, SketchOptions, SketchPlacement,
};
use crate::sketch::{Point2, PointId, Sketch};

fn opts() -> SketchOptions {
    SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    }
}

/// Samples of each tooth side: four circular arcs through them (an arc
/// through every three; arcs keep the extrusion's walls cylinders, which the
/// kernel builds in a fraction of the time of spline walls, and stay within
/// 0.5 % of the module of the exact side, 1 % on undercut teeth,
/// `gear_side_arcs_stay_close`).
const FLANK_POINTS: usize = 9;

/// Gear `i` of the pair as a part: the toothed outline on the XY plane (a
/// tooth centred on +X, the others every 360° / z), a bore of diameter `bore`
/// (0: none), extruded along +Z by the gear's face width.
pub fn gear_part(
    pair: &GearPair,
    g: &Geometry,
    i: usize,
    bore: f64,
) -> Result<PartDocument, String> {
    if pair.helix.abs() > 1e-9 {
        return Err("only spur gears are modelled yet".into());
    }
    let i = i.min(1);
    let (side, root, form) = tooth_side(pair, g, i, 400)?;
    if bore >= 2.0 * root - 1e-6 {
        return Err(format!(
            "the bore (Ø{bore:.2}) must be smaller than the root circle (Ø{:.2})",
            2.0 * root
        ));
    }
    let face = pair.face[i];
    if face.is_nan() || face <= 0.0 {
        return Err("the face width must be positive".into());
    }
    // Points of the side: evenly in radius from the root to the tip.
    let n = side.len();
    if n < FLANK_POINTS {
        return Err("the tooth could not be laid out".into());
    }
    let picks: Vec<(f64, f64)> = pick_indices(&side, form).iter().map(|&k| side[k]).collect();
    let (r_tip, psi_tip) = picks[FLANK_POINTS - 1];
    let (r_root, psi_root) = picks[0];
    if psi_tip <= 0.0 {
        return Err("the teeth are pointed".into());
    }
    let z = pair.teeth[i] as usize;
    let pitch = 2.0 * PI / z as f64;
    let mut part = PartDocument {
        exact_only: true,
        ..Default::default()
    };
    let k = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &opts())
        .map_err(|e| e.to_string())?;
    let sk = &mut part.sketches[k].sketch;
    let at = |r: f64, t: f64| Point2::new(r * t.cos(), r * t.sin());
    let err = |e: crate::sketch::SketchError| e.to_string();
    // Each tooth: its clockwise side up, the tip arc, the other side down;
    // then the root arc to the next tooth.
    let mut first_root: Option<PointId> = None;
    let mut last_root: Option<PointId> = None;
    for t in 0..z {
        let c = pitch * t as f64;
        let up: Vec<Point2> = picks.iter().map(|&(r, p)| at(r, c - p)).collect();
        let down: Vec<Point2> = picks.iter().rev().map(|&(r, p)| at(r, c + p)).collect();
        let (u0, u1) = flank_arcs(sk, &up)?;
        let (d0, d1) = flank_arcs(sk, &down)?;
        if let Some(prev) = last_root {
            root_arc(sk, prev, u0)?;
        } else {
            first_root = Some(u0);
        }
        let o = sk.add_point(Point2::origin());
        sk.add_arc(o, u1, d0).map_err(err)?;
        last_root = Some(d1);
    }
    if let (Some(a), Some(b)) = (last_root, first_root) {
        root_arc(sk, a, b)?;
    }
    if bore > 0.0 {
        sk.add_circle(Point2::origin(), bore / 2.0).map_err(err)?;
    }
    let _ = (r_tip, psi_root);
    // Inside the material: on a tooth's centre line between bore and root.
    let seed = at((r_root + bore / 2.0) / 2.0, 0.0);
    let regions = part.sketches[k].sketch.regions();
    let r = regions
        .iter()
        .find(|r| r.contains(seed))
        .ok_or("the tooth outline is not closed")?;
    let e = ExtrudeFeature::new(vec![ProfileRef::of(r, seed)], face);
    part.add_extrude(k, e).map_err(|e| e.to_string())?;
    Ok(part)
}

/// The samples the side's arcs go through: two arcs over the fillet (root
/// to the form circle), two over the involute.
fn pick_indices(side: &[(f64, f64)], form: f64) -> [usize; FLANK_POINTS] {
    let n = side.len();
    let f = side
        .iter()
        .position(|s| s.0 >= form)
        .unwrap_or(n / 3)
        .clamp(4, n.saturating_sub(5).max(4));
    // Spaced by length along the side: near the root the fillet runs almost
    // round the root circle, where even steps in radius bunch together.
    let mut len = vec![0.0; n];
    for k in 1..n {
        let (r0, p0) = side[k - 1];
        let (r1, p1) = side[k];
        let d = (r1 * p1.sin() - r0 * p0.sin()).hypot(r1 * p1.cos() - r0 * p0.cos());
        len[k] = len[k - 1] + d;
    }
    let at = |l: f64| len.partition_point(|&x| x < l).min(n - 1);
    let (lf, le) = (len[f], len[n - 1]);
    let mut out = [0; FLANK_POINTS];
    for k in 1..4 {
        out[k] = at(lf * k as f64 / 4.0).clamp(out[k - 1] + 1, f - 4 + k);
    }
    out[4] = f;
    for k in 5..8 {
        let t = (k - 4) as f64 / 4.0;
        out[k] = at(lf + (le - lf) * t).clamp(out[k - 1] + 1, n - 9 + k);
    }
    out[8] = n - 1;
    out
}

/// A tooth side through `pts` (an odd count) as circular arcs through each
/// three; returns its first and last points.
fn flank_arcs(sk: &mut Sketch, pts: &[Point2]) -> Result<(PointId, PointId), String> {
    let err = |e: crate::sketch::SketchError| e.to_string();
    let first = sk.add_point(pts[0]);
    let mut prev = first;
    let mut k = 0;
    while k + 2 < pts.len() {
        let (a, m, b) = (pts[k], pts[k + 1], pts[k + 2]);
        let end = sk.add_point(b);
        match circumcentre(a, m, b) {
            Some(c) => {
                let cid = sk.add_point(c);
                let turn = (m - a).perp(&(b - m));
                if turn > 0.0 {
                    sk.add_arc(cid, prev, end).map_err(err)?;
                } else {
                    sk.add_arc(cid, end, prev).map_err(err)?;
                }
            }
            None => {
                sk.add_line_between(prev, end).map_err(err)?;
            }
        }
        prev = end;
        k += 2;
    }
    Ok((first, prev))
}

/// The centre of the circle through three points (`None` when in line).
fn circumcentre(a: Point2, b: Point2, c: Point2) -> Option<Point2> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    let scale = (b - a).norm().max((c - b).norm());
    if d.abs() < 1e-9 * scale * scale {
        return None;
    }
    let (a2, b2, c2) = (
        a.coords.norm_squared(),
        b.coords.norm_squared(),
        c.coords.norm_squared(),
    );
    Some(Point2::new(
        (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d,
        (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d,
    ))
}

/// The root circle's arc between two teeth (counter-clockwise from `a`).
fn root_arc(sk: &mut Sketch, a: PointId, b: PointId) -> Result<(), String> {
    let pa = sk.point(a).map(|p| p.position);
    let pb = sk.point(b).map(|p| p.position);
    if let (Some(pa), Some(pb)) = (pa, pb)
        && (pa - pb).norm() < 1e-9
    {
        return Ok(());
    }
    let o = sk.add_point(Point2::origin());
    sk.add_arc(o, a, b).map_err(|e| e.to_string())?;
    Ok(())
}

/// A parallel key b × h × l (DIN 6885: form A with rounded ends, form B
/// square): its length along +Z from 0, its width across Y, its height along
/// +X from 0 — so placed with X pointing out of the shaft's groove floor and Z
/// along the shaft, it sits in the groove.
pub fn key_part(k: &KeySize, l: f64, ends: Ends) -> Result<PartDocument, String> {
    if l.is_nan() || l <= k.b {
        return Err(format!(
            "the key must be longer than its width ({} mm)",
            k.b
        ));
    }
    let mut part = PartDocument {
        exact_only: true,
        ..Default::default()
    };
    let i = part
        .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::YZ), &opts())
        .map_err(|e| e.to_string())?;
    let pl = part.sketches[i].sketch.plane.clone();
    let p = |x: f64, y: f64, z: f64| {
        let q = pl.project(&crate::kernel::Point3::new(x, y, z));
        Point2::new(q.x, q.y)
    };
    let sk = &mut part.sketches[i].sketch;
    let err = |e: crate::sketch::SketchError| e.to_string();
    match ends {
        Ends::Rounded => {
            sk.add_slot_overall(p(0.0, 0.0, 0.0), p(0.0, 0.0, l), k.b)
                .map_err(err)?;
        }
        Ends::Square => {
            let c: Vec<PointId> = [
                p(0.0, -k.b / 2.0, 0.0),
                p(0.0, k.b / 2.0, 0.0),
                p(0.0, k.b / 2.0, l),
                p(0.0, -k.b / 2.0, l),
            ]
            .into_iter()
            .map(|q| sk.add_point(q))
            .collect();
            for w in 0..4 {
                sk.add_line_between(c[w], c[(w + 1) % 4]).map_err(err)?;
            }
        }
    }
    let seed = p(0.0, 0.0, l / 2.0);
    let regions = part.sketches[i].sketch.regions();
    let r = regions
        .iter()
        .find(|r| r.contains(seed))
        .ok_or("the key outline is not closed")?;
    let mut e = ExtrudeFeature::new(vec![ProfileRef::of(r, seed)], k.h);
    if pl.normal.x < 0.0 {
        e.direction = crate::part::ExtentDirection::Flipped;
    }
    part.add_extrude(i, e).map_err(|e| e.to_string())?;
    Ok(part)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::key::size_for;

    fn volume(p: &mut PartDocument) -> f64 {
        if let Err(e) = p.regenerate() {
            panic!("regenerate: {e}");
        }
        assert!(p.feature_health.is_empty(), "{:?}", p.feature_health);
        p.exact_bodies
            .0
            .first()
            .and_then(|b| b.volume_at(1e-3))
            .unwrap_or(0.0)
    }

    /// The gear's volume is its outline's area (the side samples as a polygon,
    /// every tooth) times the face width, less the bore.
    #[test]
    fn spur_gear_part() {
        let pair = GearPair {
            module: 2.0,
            teeth: [20, 36],
            face: [12.0, 10.0],
            ..GearPair::default()
        };
        let g = pair.geometry().unwrap();
        for (i, bore) in [(0usize, 10.0), (1, 0.0)] {
            let mut p = gear_part(&pair, &g, i, bore).unwrap();
            let v = volume(&mut p);
            let (side, _, _) = tooth_side(&pair, &g, i, 2000).unwrap();
            let z = f64::from(pair.teeth[i]);
            // One tooth's share of the area: the pitch's sector up to the
            // root plus the tooth above it (polar integration of the side).
            let root = side[0].0;
            let mut a = 0.5 * root * root * 2.0 * PI / z;
            for w in side.windows(2) {
                let (r0, p0) = w[0];
                let (r1, p1) = w[1];
                a += (p0 + p1) * (r1 - r0) * (r0 + r1) / 2.0;
            }
            let want = (a * z - PI * bore * bore / 4.0) * pair.face[i];
            assert!((v - want).abs() / want < 4e-3, "gear {i}: {v} vs {want}");
        }
        // A bore wider than the root is refused.
        assert!(gear_part(&pair, &g, 0, 40.0).is_err());
    }

    /// The four arcs of a side stay within 0.5 % of the module of the
    /// exact side (sampled densely); undercut teeth within 1 %.
    #[test]
    fn gear_side_arcs_stay_close() {
        // Undercut teeth (12) have a kink in the side the arcs round off.
        for (teeth, tol) in [([12u32, 40], 0.01), ([20, 36], 0.005), ([60, 90], 0.005)] {
            let pair = GearPair {
                teeth,
                ..GearPair::default()
            };
            let g = pair.geometry().unwrap();
            let (side, _, form) = tooth_side(&pair, &g, 0, 600).unwrap();
            let at = |(r, p): (f64, f64)| Point2::new(r * p.sin(), r * p.cos());
            let idx = pick_indices(&side, form);
            let picks: Vec<Point2> = idx.iter().map(|&k| at(side[k])).collect();
            let mut worst = 0.0f64;
            for k in (0..FLANK_POINTS - 2).step_by(2) {
                let (a, m, b) = (picks[k], picks[k + 1], picks[k + 2]);
                let (i0, i1) = (idx[k], idx[k + 2]);
                let c = circumcentre(a, m, b);
                for s in &side[i0..=i1] {
                    let q = at(*s);
                    let d = match c {
                        Some(c) => ((q - c).norm() - (a - c).norm()).abs(),
                        None => 0.0,
                    };
                    worst = worst.max(d);
                }
            }
            assert!(worst < tol * pair.module, "{teeth:?}: {worst}");
        }
    }

    /// A DIN 6885 A key for a Ø40 shaft: 12 × 8, rounded ends: (l − b) b h
    /// plus the two half discs; form B is a plain block.
    #[test]
    fn key_parts() {
        let k = size_for(40.0).unwrap();
        let mut a = key_part(&k, 45.0, Ends::Rounded).unwrap();
        let want = ((45.0 - k.b) * k.b + PI * k.b * k.b / 4.0) * k.h;
        let v = volume(&mut a);
        assert!((v - want).abs() / want < 2e-3, "{v} vs {want}");
        let mut b = key_part(&k, 45.0, Ends::Square).unwrap();
        let v = volume(&mut b);
        assert!((v - 45.0 * k.b * k.h).abs() < 0.5, "{v}");
        // It stands on X = 0 and runs along +Z.
        let (lo, hi) = b.body.bounding_box().unwrap();
        assert!(
            lo.x.abs() < 1e-6 && (hi.x - k.h).abs() < 1e-6,
            "{lo:?} {hi:?}"
        );
        assert!(lo.z.abs() < 1e-6 && (hi.z - 45.0).abs() < 1e-6);
    }
}
