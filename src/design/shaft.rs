//! The shaft generator's geometry (spec 6.2-design-calculators §3.1): sections
//! from the start (z = 0) along +Z, each a cylinder, a cone or a polygon, with
//! chamfers and fillets at their ends and retaining-ring grooves along them.
//!
//! The part is one stepped body of revolution (`BodyOp::Axial`): the sections'
//! meridian as turned runs, polygons as runs of their own, joined by planar
//! faces — no Boolean, so a shaft of many sections builds in milliseconds.
//! Faces are `<feature id>:<role>` with roles by section number (`S1`,
//! `S1:Chamfer`, `S2:Fillet`, `S2:Groove1:Floor`, the caps `Start` / `End`),
//! the same at every size.

use serde::{Deserialize, Serialize};

use crate::part::axial::{AxialOp, AxialRun, AxialSeg, AxialShape};
use crate::part::{BodyOp, BodyOpFeature, PartDocument, PartError};

/// The shape of one section.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SectionShape {
    /// A cylinder of diameter `d`.
    Cylinder {
        /// Diameter.
        d: f64,
    },
    /// A cone from `d1` at its start to `d2` at its end.
    Cone {
        /// Diameter at the start.
        d1: f64,
        /// Diameter at the end.
        d2: f64,
    },
    /// A regular polygon of `sides` sides on a circle of diameter `d`
    /// (Inventor's *Diameter Circumscribed*).
    Polygon {
        /// Number of sides.
        sides: u32,
        /// Circumscribed diameter.
        d: f64,
    },
}

/// What an end of a section carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum EdgeFeature {
    /// A plain edge.
    #[default]
    None,
    /// A 45° chamfer of this size, on the larger section's edge.
    Chamfer(f64),
    /// A fillet of this radius in the corner at the foot of a step.
    Fillet(f64),
}

/// A retaining-ring groove in a cylinder: its start `at` from the section's
/// start, its width and its depth.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RingGroove {
    /// Distance of the groove's first side from the section's start.
    pub at: f64,
    /// Width (the ring's thickness plus play).
    pub width: f64,
    /// Depth below the cylinder.
    pub depth: f64,
}

/// One section of the shaft.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    /// Its shape.
    pub shape: SectionShape,
    /// Its length along the axis.
    pub length: f64,
    /// The feature at its start.
    #[serde(default)]
    pub start: EdgeFeature,
    /// The feature at its end.
    #[serde(default)]
    pub end: EdgeFeature,
    /// Retaining-ring grooves (cylinders only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grooves: Vec<RingGroove>,
    /// A parallel key's groove along the section (its DIN 6885 size follows from
    /// the diameter, `design::key`): its length, centred. Not modelled yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyway: Option<f64>,
}

impl Section {
    /// A cylinder D × L.
    pub fn cylinder(d: f64, length: f64) -> Section {
        Section {
            shape: SectionShape::Cylinder { d },
            length,
            start: EdgeFeature::None,
            end: EdgeFeature::None,
            grooves: Vec::new(),
            keyway: None,
        }
    }

    /// A cone D1 → D2 × L.
    pub fn cone(d1: f64, d2: f64, length: f64) -> Section {
        Section {
            shape: SectionShape::Cone { d1, d2 },
            ..Section::cylinder(d1, length)
        }
    }

    /// A polygon of `sides` on a circle of `d`, × L.
    pub fn polygon(sides: u32, d: f64, length: f64) -> Section {
        Section {
            shape: SectionShape::Polygon { sides, d },
            ..Section::cylinder(d, length)
        }
    }

    /// Diameters at its start and its end. A polygon's is the diameter of the
    /// largest circle inside it (what carries load, and what a step meets).
    pub fn diameters(&self) -> (f64, f64) {
        match self.shape {
            SectionShape::Cylinder { d } => (d, d),
            SectionShape::Cone { d1, d2 } => (d1, d2),
            SectionShape::Polygon { sides, d } => {
                let din = d * (std::f64::consts::PI / f64::from(sides.max(3))).cos();
                (din, din)
            }
        }
    }

    /// The diameter at `s` from its start (a groove's floor counts).
    pub fn diameter_at(&self, s: f64) -> f64 {
        let (d1, d2) = self.diameters();
        let t = if self.length > 0.0 {
            (s / self.length).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let d = d1 + (d2 - d1) * t;
        let groove = self
            .grooves
            .iter()
            .filter(|g| s >= g.at && s <= g.at + g.width)
            .map(|g| g.depth)
            .fold(0.0, f64::max);
        d - 2.0 * groove
    }

    /// A short description for lists: `Cylinder 40 × 100 mm`.
    pub fn label(&self) -> String {
        match self.shape {
            SectionShape::Cylinder { d } => {
                format!("Cylinder {} × {} mm", num(d), num(self.length))
            }
            SectionShape::Cone { d1, d2 } => {
                format!("Cone {}/{} × {} mm", num(d1), num(d2), num(self.length))
            }
            SectionShape::Polygon { sides, d } => {
                format!("Polygon {sides} × {} × {} mm", num(d), num(self.length))
            }
        }
    }
}

fn num(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// A shaft: its sections from the start.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ShaftDesign {
    /// The sections in order along +Z.
    pub sections: Vec<Section>,
}

impl ShaftDesign {
    /// Inventor's starting shaft: four cylinders with chamfers at both ends.
    pub fn starter() -> ShaftDesign {
        let mut s = vec![
            Section::cylinder(30.0, 40.0),
            Section::cylinder(40.0, 60.0),
            Section::cylinder(50.0, 20.0),
            Section::cylinder(40.0, 60.0),
        ];
        s[0].start = EdgeFeature::Chamfer(1.0);
        if let Some(last) = s.last_mut() {
            last.end = EdgeFeature::Chamfer(1.0);
        }
        ShaftDesign { sections: s }
    }

    /// Total length.
    pub fn length(&self) -> f64 {
        self.sections.iter().map(|s| s.length).sum()
    }

    /// Where each section starts.
    pub fn starts(&self) -> Vec<f64> {
        let mut z = 0.0;
        self.sections
            .iter()
            .map(|s| {
                let a = z;
                z += s.length;
                a
            })
            .collect()
    }

    /// The section at `z` and the distance into it.
    pub fn section_at(&self, z: f64) -> Option<(usize, f64)> {
        let mut a = 0.0;
        for (k, s) in self.sections.iter().enumerate() {
            if z <= a + s.length + 1e-9 {
                return Some((k, (z - a).max(0.0)));
            }
            a += s.length;
        }
        None
    }

    /// The load-carrying diameter at `z` (grooves count, chamfers do not).
    pub fn diameter_at(&self, z: f64) -> f64 {
        self.section_at(z)
            .map_or(0.0, |(k, s)| self.sections[k].diameter_at(s))
    }

    /// Checks the sizes; the error names the section (1-based).
    pub fn validate(&self) -> Result<(), String> {
        if self.sections.is_empty() {
            return Err("add a section first".into());
        }
        for (k, s) in self.sections.iter().enumerate() {
            let n = k + 1;
            if s.length.is_nan() || s.length <= 0.0 {
                return Err(format!("section {n}: the length must be positive"));
            }
            let (d1, d2) = s.diameters();
            if !(d1 > 0.0 && d2 > 0.0) {
                return Err(format!("section {n}: the diameter must be positive"));
            }
            if let SectionShape::Polygon { sides, .. } = s.shape
                && sides < 3
            {
                return Err(format!("section {n}: a polygon needs at least 3 sides"));
            }
            // A round section next to a polygon must clear its corners or stay
            // inside its flats.
            if k > 0 {
                let p = &self.sections[k - 1];
                for (poly, round, r) in [(p, s, s.diameters().0), (s, p, p.diameters().1)] {
                    if let SectionShape::Polygon { d, .. } = poly.shape
                        && !matches!(round.shape, SectionShape::Polygon { .. })
                    {
                        let inner = poly.diameters().0;
                        if r > inner - 1e-3 && r < d + 1e-3 {
                            return Err(format!(
                                "section {n}: a round section beside a polygon must be larger                                  than its corners ({d:.2} mm) or smaller than its flats ({inner:.2} mm)"
                            ));
                        }
                    }
                }
            }
            for g in &s.grooves {
                if !matches!(s.shape, SectionShape::Cylinder { .. }) {
                    return Err(format!("section {n}: grooves go into cylinders only"));
                }
                if !(g.width > 0.0 && g.depth > 0.0) || g.depth >= d1 / 2.0 {
                    return Err(format!(
                        "section {n}: a groove needs a width and a depth below the radius"
                    ));
                }
                if g.at < 0.0 || g.at + g.width > s.length {
                    return Err(format!("section {n}: a groove lies outside the section"));
                }
            }
            for e in [s.start, s.end] {
                if let EdgeFeature::Chamfer(c) | EdgeFeature::Fillet(c) = e
                    && !(c > 0.0 && c < s.length)
                {
                    return Err(format!(
                        "section {n}: a chamfer or fillet must be shorter than the section"
                    ));
                }
            }
        }
        Ok(())
    }

    /// The stepped body's definition (see the module doc for its face roles).
    pub fn axial_op(&self) -> Result<AxialOp, String> {
        self.validate()?;
        let mut runs: Vec<AxialRun> = Vec::new();
        let mut chain: Option<([f64; 2], Vec<AxialSeg>)> = None;
        let starts = self.starts();
        let n = self.sections.len();
        for (k, s) in self.sections.iter().enumerate() {
            let tag = format!("S{}", k + 1);
            let z0 = starts[k];
            let z1 = z0 + s.length;
            if let SectionShape::Polygon { sides, d } = s.shape {
                if let Some((from, segs)) = chain.take() {
                    runs.push(turned(from, segs, &format!("{tag}:Shoulder")));
                }
                let across = d * (std::f64::consts::PI / f64::from(sides)).cos();
                runs.push(AxialRun {
                    shape: AxialShape::Hexagon {
                        across,
                        z0,
                        z1,
                        role: tag.clone(),
                        sides,
                        start_cone: None,
                        end_cone: None,
                    },
                    after: Some(if k + 1 == n {
                        "End".into()
                    } else {
                        format!("S{}:Shoulder", k + 2)
                    }),
                });
                continue;
            }
            let (d1, d2) = s.diameters();
            let (r1, r2) = (d1 / 2.0, d2 / 2.0);
            // The previous section's end radius, when it was turned (a step).
            let prev = (k > 0)
                .then(|| &self.sections[k - 1])
                .filter(|p| !matches!(p.shape, SectionShape::Polygon { .. }));
            let (from, segs) = chain.get_or_insert_with(|| {
                // A new chain: at the shaft's start, or after a polygon.
                match (k, s.start) {
                    (0, EdgeFeature::Chamfer(c)) => ([r1 - c, z0], Vec::new()),
                    _ => ([r1, z0], Vec::new()),
                }
            });
            if k == 0 {
                if let EdgeFeature::Chamfer(c) = s.start {
                    segs.push(line(r1, z0 + c, &format!("{tag}:Chamfer")));
                }
            } else if let Some(p) = prev {
                let rp = p.diameters().1 / 2.0;
                step(segs, rp, r1, z0, p.end, s.start, &tag, k);
            } else if segs.is_empty() {
                // After a polygon: a chamfer of this section's start.
                if let EdgeFeature::Chamfer(c) = s.start {
                    *from = [r1 - c, z0];
                    segs.push(line(r1, z0 + c, &format!("{tag}:Chamfer")));
                }
            }
            for (g, gr) in s.grooves.iter().enumerate() {
                let gt = format!("{tag}:Groove{}", g + 1);
                let (a, b) = (z0 + gr.at, z0 + gr.at + gr.width);
                segs.push(line(r1, a, &tag));
                segs.push(line(r1 - gr.depth, a, &format!("{gt}:Side1")));
                segs.push(line(r1 - gr.depth, b, &format!("{gt}:Floor")));
                segs.push(line(r1, b, &format!("{gt}:Side2")));
            }
            // The section's end: the shaft's end, a polygon next, or a step (the
            // next section draws the step).
            let next_turned = self
                .sections
                .get(k + 1)
                .is_some_and(|q| !matches!(q.shape, SectionShape::Polygon { .. }));
            let chamfer_here = !next_turned || k + 1 == n;
            match s.end {
                EdgeFeature::Chamfer(c) if chamfer_here => {
                    let rc = r2 + (r1 - r2) * c / s.length;
                    segs.push(line(rc, z1 - c, &tag));
                    segs.push(line(r2 - c, z1, &format!("{tag}:EndChamfer")));
                }
                _ if next_turned && k + 1 < n => {
                    // The step: the corner's own features need the point before it.
                    let q = &self.sections[k + 1];
                    let rq = q.diameters().0 / 2.0;
                    let (end, _, _) = corner(r2, rq, s.end, q.start);
                    let back = corner_back(r2, rq, end);
                    let rb = r2 + (r1 - r2) * back / s.length;
                    segs.push(line(rb, z1 - back, &tag));
                }
                _ => segs.push(line(r2, z1, &tag)),
            }
            if (k + 1 == n || !next_turned)
                && let Some((from, segs)) = chain.take()
            {
                let after = if k + 1 == n {
                    "End".to_owned()
                } else {
                    format!("S{}:Shoulder", k + 2)
                };
                runs.push(turned(from, segs, &after));
            }
        }
        Ok(AxialOp {
            runs,
            first_cap: Some("Start".into()),
            ..AxialOp::default()
        })
    }
}

/// How far before a step the larger side's chamfer or the corner fillet starts.
/// The features of the corner between a section's `end` and the next one's
/// `start`, moved to where they can be: a fillet only fits the concave foot of
/// a step, so a fillet given on the larger side goes there; the second value
/// says whether the fillet came from the other side (its name follows the side
/// that asked for it).
fn corner(
    r_end: f64,
    r_next: f64,
    end: EdgeFeature,
    start: EdgeFeature,
) -> (EdgeFeature, EdgeFeature, bool) {
    if r_end < r_next {
        if let (EdgeFeature::None, EdgeFeature::Fillet(f)) = (end, start) {
            return (EdgeFeature::Fillet(f), EdgeFeature::None, true);
        }
    } else if r_end > r_next
        && let (EdgeFeature::Fillet(f), EdgeFeature::None) = (end, start)
    {
        return (EdgeFeature::None, EdgeFeature::Fillet(f), true);
    }
    (end, start, false)
}

fn corner_back(r_end: f64, r_next: f64, end: EdgeFeature) -> f64 {
    if r_end > r_next {
        // Stepping down: this section is the larger, its edge may be chamfered.
        match end {
            EdgeFeature::Chamfer(c) => c,
            _ => 0.0,
        }
    } else if r_end < r_next {
        // Stepping up: the corner at this section's end is concave.
        match end {
            EdgeFeature::Fillet(f) | EdgeFeature::Chamfer(f) => f,
            EdgeFeature::None => 0.0,
        }
    } else {
        0.0
    }
}

/// The step between a section ending at radius `rp` and the next starting at
/// `rn`, at height `z`. The previous segment ended `corner_back` before `z`.
#[allow(clippy::too_many_arguments)]
fn step(
    segs: &mut Vec<AxialSeg>,
    rp: f64,
    rn: f64,
    z: f64,
    end: EdgeFeature,
    start: EdgeFeature,
    tag: &str,
    k: usize,
) {
    let prev = format!("S{k}");
    if (rp - rn).abs() < 1e-9 {
        return;
    }
    let (end, start, moved) = corner(rp, rn, end, start);
    // The fillet's name: the section that asked for it.
    let (down_fillet, up_fillet) = if moved {
        (prev.clone(), tag.to_owned())
    } else {
        (tag.to_owned(), prev.clone())
    };
    if rp > rn {
        // Down: the convex edge of the previous (larger) section, then the
        // shoulder, then the concave foot of this one.
        if let EdgeFeature::Chamfer(c) = end {
            segs.push(line(rp - c, z, &format!("{prev}:EndChamfer")));
        }
        match start {
            EdgeFeature::Fillet(f) => {
                segs.push(line(rn + f, z, &format!("{tag}:Shoulder")));
                segs.push(arc(
                    rn,
                    z + f,
                    [rn + f, z + f],
                    &format!("{down_fillet}:Fillet"),
                ));
            }
            EdgeFeature::Chamfer(c) => {
                segs.push(line(rn + c, z, &format!("{tag}:Shoulder")));
                segs.push(line(rn, z + c, &format!("{tag}:Chamfer")));
            }
            EdgeFeature::None => segs.push(line(rn, z, &format!("{tag}:Shoulder"))),
        }
    } else {
        // Up: the concave corner at the previous section's end, the shoulder,
        // the convex edge of this (larger) section.
        match end {
            EdgeFeature::Fillet(f) => {
                segs.push(arc(
                    rp + f,
                    z,
                    [rp + f, z - f],
                    &format!("{up_fillet}:Fillet"),
                ));
            }
            EdgeFeature::Chamfer(c) => {
                segs.push(line(rp + c, z, &format!("{prev}:EndChamfer")));
            }
            EdgeFeature::None => {}
        }
        match start {
            EdgeFeature::Chamfer(c) => {
                segs.push(line(rn - c, z, &format!("{tag}:Shoulder")));
                segs.push(line(rn, z + c, &format!("{tag}:Chamfer")));
            }
            _ => segs.push(line(rn, z, &format!("{tag}:Shoulder"))),
        }
    }
}

fn line(r: f64, z: f64, role: &str) -> AxialSeg {
    AxialSeg {
        to: [r, z],
        arc: None,
        role: role.to_owned(),
    }
}

/// A concave corner's quarter circle: clockwise in (r, z) seen with r to the
/// right and z up.
fn arc(r: f64, z: f64, centre: [f64; 2], role: &str) -> AxialSeg {
    AxialSeg {
        to: [r, z],
        arc: Some((centre, false)),
        role: role.to_owned(),
    }
}

fn turned(from: [f64; 2], segs: Vec<AxialSeg>, after: &str) -> AxialRun {
    AxialRun {
        shape: AxialShape::Turned { from, segs },
        after: Some(after.to_owned()),
    }
}

/// The shaft as a part of its own (one `Axial` feature about Z, starting at the
/// origin). Errors leave nothing made.
pub fn part(design: &ShaftDesign) -> Result<PartDocument, String> {
    let op = design.axial_op()?;
    let mut part = PartDocument {
        exact_only: true,
        ..Default::default()
    };
    part.add_body_op(BodyOpFeature::new(BodyOp::Axial(op)))
        .map_err(|e: PartError| e.to_string())?;
    Ok(part)
}

/// Volume of the design's solid of revolution (polygons as their own prism),
/// chamfers and fillets left out: the calculation's mass model.
pub fn volume(design: &ShaftDesign) -> f64 {
    let pi = std::f64::consts::PI;
    design
        .sections
        .iter()
        .map(|s| match s.shape {
            SectionShape::Cylinder { d } => {
                let grooves: f64 = s
                    .grooves
                    .iter()
                    .map(|g| pi / 4.0 * (d * d - (d - 2.0 * g.depth).powi(2)) * g.width)
                    .sum();
                pi / 4.0 * d * d * s.length - grooves
            }
            SectionShape::Cone { d1, d2 } => pi / 12.0 * s.length * (d1 * d1 + d1 * d2 + d2 * d2),
            SectionShape::Polygon { sides, d } => {
                let n = f64::from(sides);
                let r = d / 2.0;
                0.5 * n * r * r * (2.0 * pi / n).sin() * s.length
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built(d: &ShaftDesign) -> PartDocument {
        let mut p = match part(d) {
            Ok(p) => p,
            Err(e) => panic!("shaft: {e}"),
        };
        if let Err(e) = p.regenerate() {
            panic!("regenerate: {e}");
        }
        assert!(
            p.feature_health.is_empty(),
            "health: {:?}",
            p.feature_health
        );
        p
    }

    fn solid_volume(p: &PartDocument) -> f64 {
        p.exact_bodies
            .0
            .first()
            .and_then(|b| b.volume_at(1e-3))
            .unwrap_or(0.0)
    }

    /// One cylinder: π/4 d² L, with its caps and wall named.
    #[test]
    fn a_cylinder_is_a_cylinder() {
        let d = ShaftDesign {
            sections: vec![Section::cylinder(20.0, 50.0)],
        };
        let p = built(&d);
        let v = solid_volume(&p);
        let want = std::f64::consts::PI / 4.0 * 400.0 * 50.0;
        assert!((v - want).abs() / want < 2e-3, "{v} vs {want}");
    }

    /// Steps with chamfers and a fillet, a groove, a cone and a hexagon:
    /// every combination builds, and the volume matches the sections' (the
    /// chamfers and the fillet take a little).
    #[test]
    fn stepped_shaft_builds() {
        let mut d = ShaftDesign::starter();
        d.sections[1].start = EdgeFeature::Fillet(1.5);
        d.sections[2].end = EdgeFeature::Chamfer(1.0);
        d.sections[3].grooves.push(RingGroove {
            at: 30.0,
            width: 1.6,
            depth: 1.0,
        });
        d.sections.push(Section::cone(40.0, 34.0, 20.0));
        d.sections.push(Section::polygon(6, 30.0, 15.0));
        let p = built(&d);
        let v = solid_volume(&p);
        let want = volume(&d);
        assert!((want - v).abs() / want < 0.01, "{v} vs {want}");
        let names: Vec<String> = p.body.faces().filter_map(|(_, f)| f.name.clone()).collect();
        for role in ["Start", "End", "S2:Fillet", "S4:Groove1:Floor", "S6:1"] {
            assert!(
                names.iter().any(|n| n.ends_with(&format!(":{role}"))),
                "{role} missing in {names:?}"
            );
        }
    }

    /// A polygon first and in the middle.
    #[test]
    fn polygons_anywhere() {
        let d = ShaftDesign {
            sections: vec![
                Section::polygon(4, 30.0, 10.0),
                Section::cylinder(20.0, 30.0),
                Section::polygon(6, 24.0, 10.0),
                Section::cylinder(20.0, 30.0),
            ],
        };
        let p = built(&d);
        let v = solid_volume(&p);
        let want = volume(&d);
        assert!((v - want).abs() / want < 3e-3, "{v} vs {want}");
    }

    #[test]
    fn bad_sizes_are_refused() {
        let mut d = ShaftDesign::starter();
        d.sections[0].grooves.push(RingGroove {
            at: 1.0,
            width: 1.0,
            depth: 20.0,
        });
        assert!(part(&d).is_err());
        assert!(part(&ShaftDesign::default()).is_err());
    }

    #[test]
    fn diameters_along() {
        let d = ShaftDesign::starter();
        assert_eq!(d.length(), 180.0);
        assert_eq!(d.diameter_at(10.0), 30.0);
        assert_eq!(d.diameter_at(50.0), 40.0);
        assert_eq!(d.diameter_at(110.0), 50.0);
    }
}
