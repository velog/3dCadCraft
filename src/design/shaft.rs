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

use crate::kernel::Point3;
use crate::part::axial::{AxialOp, AxialRun, AxialSeg, AxialShape};
use crate::part::hole::{ThreadFeature, ThreadSpec};
use crate::part::standards::{ISO_METRIC, Standards};
use crate::part::work::{WorkDef, WorkFeature, WorkPlaneDef};
use crate::part::{
    BodyOp, BodyOpFeature, ExtentDirection, ExtrudeFeature, Operation, OriginPlane, PartDocument,
    PartError, PlaneRef, ProfileRef, SketchOptions, SketchPlacement,
};
use crate::sketch::Point2;

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
    /// A relief groove (undercut) at a shoulder rather than a ring's groove;
    /// the same geometry, named `Relief<n>`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub relief: bool,
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
    /// An ISO metric coarse thread over the whole cylinder (a cosmetic
    /// thread of the part, M<d>).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub thread: bool,
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
            thread: false,
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

/// A bore into one end of the shaft: a cylindrical hole on the axis.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bore {
    /// Its diameter.
    pub d: f64,
    /// How deep it goes from the end.
    pub depth: f64,
}

/// A shaft: its sections from the start.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ShaftDesign {
    /// The sections in order along +Z.
    pub sections: Vec<Section>,
    /// A bore into the start (left) end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bore_left: Option<Bore>,
    /// A bore into the end (right) end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bore_right: Option<Bore>,
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
        ShaftDesign {
            sections: s,
            ..ShaftDesign::default()
        }
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

    /// The bore's diameter at `z` (0: solid there).
    pub fn inner_at(&self, z: f64) -> f64 {
        let len = self.length();
        let left = self.bore_left.filter(|b| z <= b.depth).map_or(0.0, |b| b.d);
        let right = self
            .bore_right
            .filter(|b| z >= len - b.depth)
            .map_or(0.0, |b| b.d);
        left.max(right)
    }

    /// The bores with the end they start from (false: the start).
    pub fn bores(&self) -> impl Iterator<Item = (bool, Bore)> + '_ {
        [(false, self.bore_left), (true, self.bore_right)]
            .into_iter()
            .filter_map(|(end, b)| b.map(|b| (end, b)))
    }

    /// How far section `k`'s start (false) or end (true) feature reaches into
    /// it along the axis, as the meridian builds it: a chamfer of the larger
    /// side of a step or of a shaft end, the fillet at the foot of a step
    /// (moved to the concave side, `corner`). 0 where nothing is built.
    pub fn edge_extent(&self, k: usize, end: bool) -> f64 {
        let Some(s) = self.sections.get(k) else {
            return 0.0;
        };
        if matches!(s.shape, SectionShape::Polygon { .. }) {
            return 0.0;
        }
        let turned = |q: &Section| !matches!(q.shape, SectionShape::Polygon { .. });
        let size = |e: EdgeFeature| match e {
            EdgeFeature::Chamfer(c) | EdgeFeature::Fillet(c) => c,
            EdgeFeature::None => 0.0,
        };
        let chamfer = |e: EdgeFeature| match e {
            EdgeFeature::Chamfer(c) => c,
            _ => 0.0,
        };
        if end {
            match self.sections.get(k + 1).filter(|q| turned(q)) {
                None => chamfer(s.end),
                Some(q) => {
                    let (r2, rq) = (s.diameters().1 / 2.0, q.diameters().0 / 2.0);
                    let (e, _, _) = corner(r2, rq, s.end, q.start);
                    corner_back(r2, rq, e)
                }
            }
        } else {
            match k
                .checked_sub(1)
                .map(|i| &self.sections[i])
                .filter(|p| turned(p))
            {
                None => chamfer(s.start),
                Some(p) => {
                    let (rp, rn) = (p.diameters().1 / 2.0, s.diameters().0 / 2.0);
                    let (_, st, _) = corner(rp, rn, p.end, s.start);
                    if rp > rn + 1e-9 {
                        size(st)
                    } else if rp < rn - 1e-9 {
                        chamfer(st)
                    } else {
                        0.0
                    }
                }
            }
        }
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
                // A groove and a chamfer or fillet on the same stretch would
                // run the outline back on itself.
                let (a0, a1) = (self.edge_extent(k, false), self.edge_extent(k, true));
                if g.at < a0 - 1e-9 || g.at + g.width > s.length - a1 + 1e-9 {
                    let side = if g.at < a0 - 1e-9 { "left" } else { "right" };
                    return Err(format!(
                        "section {n}: a groove overlaps the chamfer or fillet at its {side} end                          (a relief takes the edge's place: remove one of them)"
                    ));
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
            if let Some(l) = s.keyway {
                if !matches!(s.shape, SectionShape::Cylinder { .. }) {
                    return Err(format!("section {n}: keyways go into cylinders only"));
                }
                let k =
                    crate::design::key::size_for(d1).map_err(|e| format!("section {n}: {e}"))?;
                if !(l > k.b && l <= s.length) {
                    return Err(format!(
                        "section {n}: the keyway must be longer than its width ({} mm) and fit the section",
                        k.b
                    ));
                }
            }
        }
        let len = self.length();
        for (end, b) in self.bores() {
            let side = if end { "right" } else { "left" };
            if !(b.d > 0.0 && b.depth > 0.0 && b.depth <= len + 1e-9) {
                return Err(format!(
                    "the {side} bore needs a diameter and a depth within the shaft"
                ));
            }
            // The thinnest wall along the bore (grooves and keyways count).
            let (a, z1) = if end {
                (len - b.depth, len)
            } else {
                (0.0, b.depth)
            };
            let steps = 64;
            for i in 0..=steps {
                let z = a + (z1 - a) * f64::from(i) / f64::from(steps);
                let mut outer = self.diameter_at(z);
                if let Some((k, at)) = self.section_at(z) {
                    let s = &self.sections[k];
                    if let (Some(l), Ok(key)) = (s.keyway, crate::design::key::size_for(outer)) {
                        let k0 = (s.length - l) / 2.0;
                        if at >= k0 && at <= k0 + l {
                            outer -= 2.0 * key.t1;
                        }
                    }
                }
                if b.d >= outer - 1e-6 {
                    return Err(format!(
                        "the {side} bore (Ø{}) breaks through the wall at z = {z:.1} mm",
                        num(b.d)
                    ));
                }
            }
        }
        if let (Some(l), Some(r)) = (self.bore_left, self.bore_right)
            && l.depth + r.depth > len + 1e-9
            && (l.d - r.d).abs() > 1e-9
        {
            return Err("the two bores overlap with different diameters".into());
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
                let gt = if gr.relief {
                    format!("{tag}:Relief{}", g + 1)
                } else {
                    format!("{tag}:Groove{}", g + 1)
                };
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
    // A groove at a section's very start or end repeats a point, or goes on
    // along the shoulder: drop segments of no length, join straight ones in line.
    let mut out: Vec<AxialSeg> = Vec::with_capacity(segs.len());
    let mut last = from;
    let mut before = from;
    for s in segs {
        if (s.to[0] - last[0]).hypot(s.to[1] - last[1]) <= 1e-9 {
            continue;
        }
        let a = [last[0] - before[0], last[1] - before[1]];
        let b = [s.to[0] - last[0], s.to[1] - last[1]];
        // Only radial lines (a groove's side going on along a shoulder): walls
        // in line keep their own section names.
        let in_line = a[1].abs() <= 1e-12 && b[1].abs() <= 1e-12 && a[0] * b[0] > 0.0;
        if let Some(p) = out.last_mut()
            && p.arc.is_none()
            && s.arc.is_none()
            && in_line
        {
            p.to = s.to;
            last = s.to;
            continue;
        }
        before = last;
        last = s.to;
        out.push(s);
    }
    let segs = out;
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
        generator: Some(crate::design::Generator::Shaft {
            design: design.clone(),
            loads: None,
        }),
        ..Default::default()
    };
    let id = part
        .add_body_op(BodyOpFeature::new(BodyOp::Axial(op)))
        .map_err(|e: PartError| e.to_string())?;
    let starts = design.starts();
    let opts = SketchOptions {
        auto_project_origin: false,
        auto_reference_edges: false,
        ..Default::default()
    };
    for (k, s) in design.sections.iter().enumerate() {
        if let (Some(l), SectionShape::Cylinder { d }) = (s.keyway, s.shape) {
            keyway_cut(&mut part, d, starts[k] + s.length / 2.0, l, &opts)
                .map_err(|e| format!("section {}: the keyway: {e}", k + 1))?;
        }
    }
    let len = design.length();
    for (end, b) in design.bores() {
        bore_cut(&mut part, &b, end, len, &opts)
            .map_err(|e| format!("the {} bore: {e}", if end { "right" } else { "left" }))?;
    }
    for (k, s) in design.sections.iter().enumerate() {
        if !s.thread {
            continue;
        }
        let SectionShape::Cylinder { d } = s.shape else {
            return Err(format!("section {}: threads go on cylinders", k + 1));
        };
        let name = format!("M{}", num(d));
        let spec = ThreadSpec::from_table(Standards::global(), ISO_METRIC, &name, false)
            .ok_or_else(|| format!("section {}: no ISO metric thread {name}", k + 1))?;
        let role = format!("{id}:S{}", k + 1);
        let z = starts[k] + s.length / 2.0;
        let hint = Point3::new(d / 2.0, 0.0, z);
        let face = part
            .body
            .faces()
            .find(|(_, f)| f.name.as_deref() == Some(role.as_str()))
            .and_then(|(fid, _)| crate::part::shell::face_ref(&part.body, fid, Some(hint)))
            .ok_or_else(|| format!("section {}: its cylinder was not found", k + 1))?;
        part.add_thread(ThreadFeature::new(face, spec))
            .map_err(|e| e.to_string())?;
    }
    Ok(part)
}

/// The parallel key's groove of a cylinder of diameter `d` (DIN 6885 form A:
/// rounded ends, width b, depth t1 below the top line), `l` long, centred at
/// `zc`, on the +X side: a slot sketched on a plane square to X at the
/// groove's floor and cut outwards (Extrude Cut past the cylinder).
fn keyway_cut(
    part: &mut PartDocument,
    d: f64,
    zc: f64,
    l: f64,
    opts: &SketchOptions,
) -> Result<(), String> {
    let k = crate::design::key::size_for(d)?;
    let floor = d / 2.0 - k.t1;
    let w = part
        .add_work(hidden_work(WorkDef::Plane(WorkPlaneDef::Offset {
            base: PlaneRef::Origin(OriginPlane::YZ),
            distance: floor,
        })))
        .map_err(|e| e.to_string())?;
    let i = part
        .create_sketch(
            SketchPlacement {
                plane: PlaneRef::Work(w),
                ..Default::default()
            },
            opts,
        )
        .map_err(|e| e.to_string())?;
    let pl = part.sketches[i].sketch.plane.clone();
    let e1 = pl.project(&Point3::new(floor, 0.0, zc - l / 2.0));
    let e2 = pl.project(&Point3::new(floor, 0.0, zc + l / 2.0));
    part.sketches[i]
        .sketch
        .add_slot_overall(Point2::new(e1.x, e1.y), Point2::new(e2.x, e2.y), k.b)
        .map_err(|e| e.to_string())?;
    let mid = pl.project(&Point3::new(floor, 0.0, zc));
    cut_region(
        part,
        i,
        Point2::new(mid.x, mid.y),
        k.t1 + 1.0,
        pl.normal.x < 0.0,
    )
}

/// A bore: a circle on the end's plane cut `depth` into the shaft.
fn bore_cut(
    part: &mut PartDocument,
    b: &Bore,
    end: bool,
    len: f64,
    opts: &SketchOptions,
) -> Result<(), String> {
    let placement = if end {
        let w = part
            .add_work(hidden_work(WorkDef::Plane(WorkPlaneDef::Offset {
                base: PlaneRef::Origin(OriginPlane::XY),
                distance: len,
            })))
            .map_err(|e| e.to_string())?;
        SketchPlacement {
            plane: PlaneRef::Work(w),
            ..Default::default()
        }
    } else {
        SketchPlacement::on_origin_plane(OriginPlane::XY)
    };
    let i = part
        .create_sketch(placement, opts)
        .map_err(|e| e.to_string())?;
    let pl = part.sketches[i].sketch.plane.clone();
    let c = pl.project(&Point3::new(0.0, 0.0, if end { len } else { 0.0 }));
    let c = Point2::new(c.x, c.y);
    part.sketches[i]
        .sketch
        .add_circle(c, b.d / 2.0)
        .map_err(|e| e.to_string())?;
    // Into the shaft: -Z from the end, +Z from the start.
    let into = if end { -1.0 } else { 1.0 };
    cut_region(part, i, c, b.depth, pl.normal.z * into < 0.0)
}

/// Extrude Cut of the region of sketch `i` round `seed`, `dist` along the
/// sketch's normal (`flip`: against it).
fn cut_region(
    part: &mut PartDocument,
    i: usize,
    seed: Point2,
    dist: f64,
    flip: bool,
) -> Result<(), String> {
    let regions = part.sketches[i].sketch.regions();
    let r = regions
        .iter()
        .find(|r| r.contains(seed))
        .ok_or("its outline is not closed")?;
    let mut e = ExtrudeFeature::new(vec![ProfileRef::of(r, seed)], dist);
    e.operation = Operation::Cut;
    if flip {
        e.direction = ExtentDirection::Flipped;
    }
    part.add_extrude(i, e).map_err(|e| e.to_string())?;
    Ok(())
}

fn hidden_work(def: WorkDef) -> WorkFeature {
    let mut w = WorkFeature::new(def);
    w.visible = false;
    w
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
                let key = s
                    .keyway
                    .and_then(|l| crate::design::key::size_for(d).ok().map(|k| (l, k)))
                    .map_or(0.0, |(l, k)| keyway_volume(d, k.b, k.t1, l));
                pi / 4.0 * d * d * s.length - grooves - key
            }
            SectionShape::Cone { d1, d2 } => pi / 12.0 * s.length * (d1 * d1 + d1 * d2 + d2 * d2),
            SectionShape::Polygon { sides, d } => {
                let n = f64::from(sides);
                let r = d / 2.0;
                0.5 * n * r * r * (2.0 * pi / n).sin() * s.length
            }
        })
        .sum::<f64>()
        - design
            .bores()
            .map(|(_, b)| pi / 4.0 * b.d * b.d * b.depth)
            .sum::<f64>()
}

/// What a rounded-end keyway of width `b`, depth `t1` (below the top line)
/// and length `l` takes from a cylinder of diameter `d`: the height of the
/// cylinder above the flat floor, integrated over the slot (a rectangle of
/// l - b and a disc of b), by Simpson's rule across the width.
pub fn keyway_volume(d: f64, b: f64, t1: f64, l: f64) -> f64 {
    let r = d / 2.0;
    let h = |y: f64| ((r * r - y * y).max(0.0).sqrt() - (r - t1)).max(0.0);
    let n = 200;
    let step = b / f64::from(n);
    let (mut strip, mut disc) = (0.0, 0.0);
    for i in 0..=n {
        let y = -b / 2.0 + step * f64::from(i);
        let w = if i == 0 || i == n {
            1.0
        } else if i % 2 == 1 {
            4.0
        } else {
            2.0
        };
        strip += w * h(y);
        disc += w * h(y) * 2.0 * ((b / 2.0).powi(2) - y * y).max(0.0).sqrt();
    }
    (strip * (l - b) + disc) * step / 3.0
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
            ..ShaftDesign::default()
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
            relief: false,
        });
        d.sections[1].grooves.push(RingGroove {
            at: 56.0,
            width: 2.0,
            depth: 0.4,
            relief: true,
        });
        d.sections[0].thread = true;
        d.sections.push(Section::cone(40.0, 34.0, 20.0));
        d.sections.push(Section::polygon(6, 30.0, 15.0));
        let p = built(&d);
        let v = solid_volume(&p);
        let want = volume(&d);
        assert!((want - v).abs() / want < 0.01, "{v} vs {want}");
        let names: Vec<String> = p.body.faces().filter_map(|(_, f)| f.name.clone()).collect();
        for role in [
            "Start",
            "End",
            "S2:Fillet",
            "S4:Groove1:Floor",
            "S2:Relief1:Floor",
            "S6:1",
        ] {
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
            ..ShaftDesign::default()
        };
        let p = built(&d);
        let v = solid_volume(&p);
        let want = volume(&d);
        assert!((v - want).abs() / want < 3e-3, "{v} vs {want}");
    }

    /// Every corner treatment next to a relief at the step, stepping up and
    /// down: the meridian never runs back on itself and the body builds with
    /// its sections' volume (the owner's relief after a chamfered step gave an
    /// invalid solid).
    #[test]
    fn reliefs_beside_every_corner() {
        let edges = [
            EdgeFeature::None,
            EdgeFeature::Chamfer(1.0),
            EdgeFeature::Fillet(1.0),
        ];
        let relief = RingGroove {
            at: 0.0,
            width: 2.5,
            depth: 0.4,
            relief: true,
        };
        let mut cases = Vec::new();
        for (da, db) in [(30.0, 40.0), (40.0, 30.0)] {
            for end in edges {
                for start in edges {
                    for relief_in_b in [true, false] {
                        let mut a = Section::cylinder(da, 40.0);
                        let mut b = Section::cylinder(db, 40.0);
                        a.end = end;
                        b.start = start;
                        if relief_in_b {
                            b.grooves.push(relief);
                        } else {
                            a.grooves.push(RingGroove { at: 37.5, ..relief });
                        }
                        cases.push(vec![a, b]);
                    }
                }
            }
        }
        // The shaft's own ends and a polygon's sides.
        let mut first = Section::cylinder(30.0, 40.0);
        first.start = EdgeFeature::Chamfer(1.0);
        first.grooves.push(relief);
        cases.push(vec![first.clone()]);
        let mut last = Section::cylinder(30.0, 40.0);
        last.end = EdgeFeature::Chamfer(1.0);
        last.grooves.push(RingGroove { at: 37.5, ..relief });
        cases.push(vec![Section::polygon(6, 40.0, 10.0), first, last]);
        let mut failed = Vec::new();
        let mut refused = 0;
        for sections in cases {
            let d = ShaftDesign {
                sections,
                ..ShaftDesign::default()
            };
            let case = format!("{:?}", d.sections);
            let plain = d
                .sections
                .iter()
                .all(|s| s.start == EdgeFeature::None && s.end == EdgeFeature::None);
            if let Err(e) = d.validate() {
                if plain || !e.contains("overlaps") {
                    failed.push(format!("{case}: {e}"));
                }
                refused += 1;
                continue;
            }
            // The meridian never runs back along the axis.
            let op = d.axial_op().unwrap();
            for run in &op.runs {
                if let AxialShape::Turned { from, segs } = &run.shape {
                    let mut z = from[1];
                    for sg in segs {
                        if sg.to[1] < z - 1e-9 {
                            failed.push(format!("{case}: back to z {} from {z}", sg.to[1]));
                        }
                        z = sg.to[1];
                    }
                }
            }
            let mut p = match part(&d) {
                Ok(p) => p,
                Err(e) => {
                    failed.push(format!("{case}: {e}"));
                    continue;
                }
            };
            if let Err(e) = p.regenerate() {
                failed.push(format!("{case}: {e}"));
                continue;
            }
            if !p.feature_health.is_empty() {
                failed.push(format!("{case}: {:?}", p.feature_health));
                continue;
            }
            let v = solid_volume(&p);
            let want = volume(&d);
            if (v - want).abs() / want > 0.01 {
                failed.push(format!("{case}: volume {v} vs {want}"));
            }
        }
        assert!(failed.is_empty(), "{failed:#?}");
        // Some combinations do overlap (a chamfer of the larger side, a fillet
        // at the foot) and those are refused, not built inside out.
        assert!(refused >= 6, "{refused}");
    }

    /// A DIN 6885 keyway (Ø40: 12 × 8, t1 5) and bores at both ends take
    /// what they should: the slot's volume under the cylinder, π/4 d² depth.
    #[test]
    fn keyways_and_bores_are_cut() {
        let mut d = ShaftDesign::starter();
        d.sections[1].keyway = Some(45.0);
        d.bore_left = Some(Bore {
            d: 12.0,
            depth: 30.0,
        });
        d.bore_right = Some(Bore {
            d: 16.0,
            depth: 25.0,
        });
        let p = built(&d);
        let v = solid_volume(&p);
        let want = volume(&d);
        let key = keyway_volume(40.0, 12.0, 5.0, 45.0);
        assert!(key > 2000.0 && key < 12.0 * 5.0 * 45.0, "{key}");
        assert!((v - want).abs() / want < 3e-3, "{v} vs {want}");
        // Without them the solid is that much larger.
        let plain = solid_volume(&built(&ShaftDesign::starter()));
        let bores = std::f64::consts::PI / 4.0 * (144.0 * 30.0 + 256.0 * 25.0);
        assert!(
            ((plain - v) - (key + bores)).abs() < 0.01 * (key + bores),
            "{} vs {}",
            plain - v,
            key + bores
        );
        // A bore through the wall is refused.
        d.bore_left = Some(Bore {
            d: 30.5,
            depth: 30.0,
        });
        assert!(d.validate().is_err());
    }

    #[test]
    fn bad_sizes_are_refused() {
        let mut d = ShaftDesign::starter();
        d.sections[0].grooves.push(RingGroove {
            at: 1.0,
            width: 1.0,
            depth: 20.0,
            relief: false,
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
