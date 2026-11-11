//! Frame Analysis of an assembly's frames (`docs/spec/7.1a-frame-analysis.md`),
//! GUI-free: what a simulation stores ([`Simulation`], in
//! `AssemblyDocument::simulations`), the conversion of the frames into a beam
//! model ([`convert`]) and the runs that answer in the members' terms
//! ([`run_static`], [`run_modes`]). Loads and constraints name a member and a
//! distance along it, never a node: nodes are found again at each conversion,
//! so a simulation follows the frame.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::frame::{Frame, FrameMember, Run, skeleton_run};
use super::tree::Linked;
use crate::analysis::beam::{
    Beam, BeamMaterial, BeamSection, Dofs, Load, Mode, Model, RigidLink, Solution, Station, V3,
};
use crate::document::{AssemblyDocument, DocumentData, Project};
use crate::kernel::{Point3, Real, Vector3};

/// Static or modal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum SimKind {
    /// Linear static.
    #[default]
    Static,
    /// Natural frequencies.
    Modal {
        /// How many.
        count: u32,
    },
}

/// A place on the frame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct At {
    /// The member (its occurrence).
    pub member: u32,
    /// Distance from the beam's start, mm (clamped to its length).
    pub s: Real,
}

/// What a constraint holds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Hold {
    /// All six.
    Fixed,
    /// The three displacements.
    Pinned,
    /// Displacement along one assembly axis (0 X, 1 Y, 2 Z).
    Floating {
        /// The axis.
        axis: u8,
    },
    /// Chosen ones (X Y Z displacements, X Y Z rotations).
    Custom {
        /// Held values.
        dofs: Dofs,
    },
}

impl Hold {
    /// The held degrees of freedom.
    pub fn dofs(self) -> Dofs {
        match self {
            Hold::Fixed => [true; 6],
            Hold::Pinned => [true, true, true, false, false, false],
            Hold::Floating { axis } => {
                let mut d = [false; 6];
                d[usize::from(axis.min(2))] = true;
                d
            }
            Hold::Custom { dofs } => dofs,
        }
    }

    /// Its name in the browser.
    pub fn label(self) -> &'static str {
        match self {
            Hold::Fixed => "Fixed",
            Hold::Pinned => "Pinned",
            Hold::Floating { .. } => "Floating",
            Hold::Custom { .. } => "Custom",
        }
    }
}

/// A constraint.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaConstraint {
    /// Where.
    pub at: At,
    /// What it holds.
    pub hold: Hold,
    /// Left out of the analysis.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
}

/// A load.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum FaLoadKind {
    /// Force, N (assembly axes).
    Force {
        /// Where.
        at: At,
        /// Components.
        f: [Real; 3],
    },
    /// Moment, N·mm (assembly axes).
    Moment {
        /// Where.
        at: At,
        /// Components.
        m: [Real; 3],
    },
    /// Torque about the beam, N·mm.
    AxialMoment {
        /// Where.
        at: At,
        /// Value.
        m: Real,
    },
    /// A moment in a plane of the beam, N·mm: `angle` turns it from the
    /// section's horizontal axis about the beam.
    BendingMoment {
        /// Where.
        at: At,
        /// Value.
        m: Real,
        /// Radians.
        angle: Real,
    },
    /// A uniform load over a stretch, N/mm (assembly axes).
    Continuous {
        /// The member.
        member: u32,
        /// Start, mm from the beam's start.
        from: Real,
        /// End.
        to: Real,
        /// Load per mm.
        q: [Real; 3],
    },
    /// Weight of every beam: acceleration, mm/s².
    Gravity {
        /// Components.
        g: [Real; 3],
    },
}

impl FaLoadKind {
    /// Its name in the browser.
    pub fn label(&self) -> &'static str {
        match self {
            FaLoadKind::Force { .. } => "Force",
            FaLoadKind::Moment { .. } => "Moment",
            FaLoadKind::AxialMoment { .. } => "Axial Moment",
            FaLoadKind::BendingMoment { .. } => "Bending Moment",
            FaLoadKind::Continuous { .. } => "Continuous Load",
            FaLoadKind::Gravity { .. } => "Gravity",
        }
    }
}

/// A load in a simulation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaLoad {
    /// What and where.
    pub kind: FaLoadKind,
    /// Left out of the analysis.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
}

/// What a beam's ends do not carry (local axes: along, u, v, then rotations).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FaRelease {
    /// The member.
    pub member: u32,
    /// At its start.
    pub start: Dofs,
    /// At its end.
    pub end: Dofs,
}

/// One rigid link: `children` follow `parent`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaLink {
    /// The parent point.
    pub parent: At,
    /// The child points.
    pub children: Vec<At>,
}

/// Numbers a simulation overrides for one member.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BeamOverride {
    /// The member.
    pub member: u32,
    /// Young's modulus, MPa.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub e: Option<Real>,
    /// Shear modulus, MPa.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub g: Option<Real>,
    /// Density, t/mm³.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rho: Option<Real>,
    /// Area, mm².
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<Real>,
    /// Second moment about the section's horizontal axis, mm⁴.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ix: Option<Real>,
    /// Second moment about its vertical axis, mm⁴.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iy: Option<Real>,
    /// Torsion constant, mm⁴.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub j: Option<Real>,
}

fn default_tolerance() -> Real {
    2.0
}

fn default_points() -> u32 {
    21
}

/// A simulation of an assembly's frames.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Simulation {
    /// Its name.
    pub name: String,
    /// Static or modal.
    #[serde(default)]
    pub kind: SimKind,
    /// Beams nearer than this many section sizes are linked.
    #[serde(default = "default_tolerance")]
    pub tolerance: Real,
    /// Result stations per beam.
    #[serde(default = "default_points")]
    pub beam_points: u32,
    /// Constraints.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<FaConstraint>,
    /// Loads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loads: Vec<FaLoad>,
    /// Custom nodes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<At>,
    /// Releases.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub releases: Vec<FaRelease>,
    /// Rigid links.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<FaLink>,
    /// Overridden numbers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<BeamOverride>,
    /// Members left out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded: Vec<u32>,
}

impl Simulation {
    /// A new, empty simulation.
    pub fn new(name: impl Into<String>, kind: SimKind) -> Self {
        Self {
            name: name.into(),
            kind,
            tolerance: default_tolerance(),
            beam_points: default_points(),
            constraints: Vec::new(),
            loads: Vec::new(),
            nodes: Vec::new(),
            releases: Vec::new(),
            links: Vec::new(),
            overrides: Vec::new(),
            excluded: Vec::new(),
        }
    }
}

/// A member as a beam of the analysis.
#[derive(Clone, Debug, PartialEq)]
pub struct BeamLine {
    /// The member.
    pub member: u32,
    /// Its display name.
    pub name: String,
    /// Centroid axis, start (assembly coordinates of the frame's document).
    pub start: Point3,
    /// Centroid axis, end.
    pub end: Point3,
    /// The section's horizontal axis.
    pub u: Vector3,
    /// Section numbers.
    pub section: BeamSection,
    /// Material numbers.
    pub material: BeamMaterial,
    /// Half of the section's larger side, mm.
    pub reach: Real,
    /// Yield strength, MPa, when the material knows it.
    pub yield_strength: Option<Real>,
}

impl BeamLine {
    /// Its length.
    pub fn length(&self) -> Real {
        (self.end - self.start).norm()
    }

    /// The point at distance `s` from its start.
    pub fn at(&self, s: Real) -> Point3 {
        self.start + (self.end - self.start) * (s.clamp(0.0, self.length()) / self.length())
    }
}

/// A member left out of the model and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Skipped {
    /// The member.
    pub member: u32,
    /// Why.
    pub why: String,
}

/// The beams of the frames of `asm`, with the members that are not beams.
pub fn beam_lines(
    project: &Project,
    linked: &Linked,
    asm: &AssemblyDocument,
    sim: &Simulation,
) -> Result<(Vec<BeamLine>, Vec<Skipped>), String> {
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    for f in &asm.frames {
        for m in &f.members {
            match beam_of(project, linked, asm, f, m, sim) {
                Ok(Some(b)) => out.push(b),
                Ok(None) => {}
                Err(why) => skipped.push(Skipped {
                    member: m.occurrence,
                    why,
                }),
            }
        }
    }
    Ok((out, skipped))
}

fn beam_of(
    project: &Project,
    linked: &Linked,
    asm: &AssemblyDocument,
    f: &Frame,
    m: &FrameMember,
    sim: &Simulation,
) -> Result<Option<BeamLine>, String> {
    let o = asm.find(m.occurrence).ok_or("its component is gone")?;
    if o.suppressed || !o.visible || sim.excluded.contains(&m.occurrence) {
        return Ok(None);
    }
    let source = m
        .reuse_of
        .and_then(|s| f.members.iter().find(|x| x.occurrence == s))
        .unwrap_or(m);
    let sec = source.section().ok_or("its section is unreadable")?;
    let run = skeleton_run(project, linked, asm, &m.skeleton)?;
    let Run::Line(p, q) = run else {
        return Err("curved members are not beams of the analysis".into());
    };
    let length = (q - p).norm();
    let t = o.transform;
    let c = sec.properties();
    // The placement's frame: part Z runs along the member.
    let start = t.transform_point(&Point3::new(c.centroid.x, c.centroid.y, 0.0));
    let end = t.transform_point(&Point3::new(c.centroid.x, c.centroid.y, length));
    let u = t.transform_vector(&Vector3::x());
    let (j, wt, au, av) = sec.torsion();
    let doc = o.component.document;
    let part = match project.document(doc).map(|d| &d.data) {
        Some(DocumentData::Part(x)) if o.component.library.is_none() => Some(x),
        _ => None,
    };
    let name = part.and_then(|p| p.material.clone());
    let mat = name
        .as_deref()
        .and_then(|n| crate::materials::with_user().material(n).cloned());
    let over = sim.overrides.iter().find(|b| b.member == m.occurrence);
    let e = over.and_then(|b| b.e).or_else(|| {
        mat.as_ref()
            .and_then(|x| x.youngs_modulus)
            .map(|g| g * 1000.0)
    });
    let Some(e) = e else {
        return Err(format!(
            "its material ({}) has no Young's modulus",
            name.as_deref().unwrap_or("none")
        ));
    };
    let g = over
        .and_then(|b| b.g)
        .unwrap_or(e / (2.0 * (1.0 + POISSON)));
    let rho = over
        .and_then(|b| b.rho)
        .or_else(|| mat.as_ref().map(|x| x.density * 1e-9))
        .unwrap_or(0.0);
    let pick = |v: Option<Real>, d: Real| v.unwrap_or(d);
    let section = BeamSection {
        area: pick(over.and_then(|b| b.area), c.area),
        iu: pick(over.and_then(|b| b.ix), c.ix),
        iv: pick(over.and_then(|b| b.iy), c.iy),
        j: pick(over.and_then(|b| b.j), j),
        wu: c.wx,
        wv: c.wy,
        wt,
        au,
        av,
    };
    let (w, h) = sec.extent();
    Ok(Some(BeamLine {
        member: m.occurrence,
        name: o.name.clone(),
        start,
        end,
        u,
        section,
        material: BeamMaterial { e, g, rho },
        reach: w.max(h) / 2.0,
        yield_strength: mat.and_then(|x| x.yield_strength),
    }))
}

/// Poisson's ratio used for the shear modulus (the library has no column for it).
const POISSON: Real = 0.3;

/// Positions closer than this are one node, mm.
const SAME_POINT: Real = 1e-3;

/// The closest points of two segments: (s on a, t on b, distance), `None`
/// when they run parallel.
fn closest(a0: &Point3, a1: &Point3, b0: &Point3, b1: &Point3) -> Option<(Real, Real, Real)> {
    let (u, v, w) = (a1 - a0, b1 - b0, a0 - b0);
    let (uu, uv, vv, uw, vw) = (u.dot(&u), u.dot(&v), v.dot(&v), u.dot(&w), v.dot(&w));
    let den = uu * vv - uv * uv;
    if den <= 1e-9 * uu * vv {
        return None;
    }
    let mut s = ((uv * vw - vv * uw) / den).clamp(0.0, 1.0);
    let mut t = (uv * s + vw) / vv;
    if !(0.0..=1.0).contains(&t) {
        t = t.clamp(0.0, 1.0);
        s = ((uv * t - uw) / uu).clamp(0.0, 1.0);
    }
    let d = ((a0 + u * s) - (b0 + v * t)).norm();
    Some((s, t, d))
}

/// A piece of a member in the model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    /// Index in [`Converted::lines`].
    pub line: usize,
    /// The model beam.
    pub beam: usize,
    /// Where it starts along the member.
    pub from: Real,
    /// Where it ends.
    pub to: Real,
}

/// A frame as a beam model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Converted {
    /// The members as beams.
    pub lines: Vec<BeamLine>,
    /// Members left out.
    pub skipped: Vec<Skipped>,
    /// The model (constraints and loads of the simulation included).
    pub model: Model,
    /// Beams of the model per member piece, in order along each member.
    pub pieces: Vec<Piece>,
    /// The constraints applied: index in the simulation, node.
    pub held: Vec<(usize, usize)>,
}

impl Converted {
    /// The member's beam.
    pub fn line_of(&self, member: u32) -> Option<usize> {
        self.lines.iter().position(|l| l.member == member)
    }

    /// The model beam and the distance along it of a place on a member.
    fn piece_at(&self, line: usize, s: Real) -> Option<(usize, Real)> {
        let len = self.lines[line].length();
        let s = s.clamp(0.0, len);
        let own: Vec<&Piece> = self.pieces.iter().filter(|p| p.line == line).collect();
        own.iter()
            .find(|p| s <= p.to + 1e-9)
            .or(own.last())
            .map(|p| (p.beam, (s - p.from).max(0.0)))
    }
}

/// One node to be: all the stations that share it.
#[derive(Default)]
struct Nodes {
    at: Vec<Point3>,
    /// Per line: station (distance) → node.
    stations: Vec<BTreeMap<i64, usize>>,
}

fn key(s: Real) -> i64 {
    (s * 1e3).round() as i64
}

impl Nodes {
    /// The node at distance `s` of `line`, new when there is none near.
    fn node(&mut self, lines: &[BeamLine], line: usize, s: Real) -> usize {
        let len = lines[line].length();
        let s = s.clamp(0.0, len);
        if let Some(&n) = self.stations[line].get(&key(s)) {
            return n;
        }
        let p = lines[line].at(s);
        let found = self.at.iter().position(|q| (q - p).norm() <= SAME_POINT);
        let n = found.unwrap_or_else(|| {
            self.at.push(p);
            self.at.len() - 1
        });
        self.stations[line].insert(key(s), n);
        n
    }
}

/// The beam model of the frames of `asm` under `sim`.
pub fn convert(
    project: &Project,
    linked: &Linked,
    asm: &AssemblyDocument,
    sim: &Simulation,
) -> Result<Converted, String> {
    let (lines, skipped) = beam_lines(project, linked, asm, sim)?;
    if lines.is_empty() {
        return Err(match skipped.first() {
            Some(s) => format!("no member can be a beam: {}", s.why),
            None => "the frame has no straight members".into(),
        });
    }
    let mut n = Nodes {
        at: Vec::new(),
        stations: vec![BTreeMap::new(); lines.len()],
    };
    for (i, l) in lines.iter().enumerate() {
        n.node(&lines, i, 0.0);
        n.node(&lines, i, l.length());
    }
    // Where beams meet or nearly so.
    let mut links: Vec<(usize, usize)> = Vec::new();
    for i in 0..lines.len() {
        for j in i + 1..lines.len() {
            let (a, b) = (&lines[i], &lines[j]);
            let Some((s, t, d)) = closest(&a.start, &a.end, &b.start, &b.end) else {
                continue;
            };
            let limit = sim.tolerance * (a.reach + b.reach);
            if d > limit.max(SAME_POINT) {
                continue;
            }
            let (ni, nj) = (n.node(&lines, i, s * a.length()), {
                n.node(&lines, j, t * b.length())
            });
            if ni != nj {
                links.push((ni, nj));
            }
        }
    }
    // Every place the simulation names is a node, loads excepted.
    let line_of = |m: u32| lines.iter().position(|l| l.member == m);
    let mut place = |at: &At| -> Option<usize> {
        let i = line_of(at.member)?;
        Some(n.node(&lines, i, at.s))
    };
    let mut held = Vec::new();
    let mut held_dofs = Vec::new();
    for (k, c) in sim.constraints.iter().enumerate() {
        if c.suppressed {
            continue;
        }
        let Some(node) = place(&c.at) else {
            continue;
        };
        held.push((k, node));
        held_dofs.push((node, c.hold.dofs()));
    }
    for a in &sim.nodes {
        let _ = place(a);
    }
    for l in &sim.links {
        let Some(parent) = place(&l.parent) else {
            continue;
        };
        for c in &l.children {
            if let Some(child) = place(c)
                && child != parent
            {
                links.push((parent, child));
            }
        }
    }
    let node_loads: Vec<Option<usize>> = sim
        .loads
        .iter()
        .map(|l| match &l.kind {
            FaLoadKind::Force { at, .. }
            | FaLoadKind::Moment { at, .. }
            | FaLoadKind::AxialMoment { at, .. }
            | FaLoadKind::BendingMoment { at, .. } => {
                // A load exactly at a station uses that node.
                let i = line_of(at.member)?;
                n.stations[i]
                    .get(&key(at.s.clamp(0.0, lines[i].length())))
                    .copied()
            }
            _ => None,
        })
        .collect();
    // Rigid clusters: one parent each.
    let count = n.at.len();
    let mut root: Vec<usize> = (0..count).collect();
    fn find(r: &mut [usize], mut i: usize) -> usize {
        while r[i] != i {
            r[i] = r[r[i]];
            i = r[i];
        }
        i
    }
    for (a, b) in &links {
        let (x, y) = (find(&mut root, *a), find(&mut root, *b));
        if x != y {
            root[y.max(x)] = y.min(x);
        }
    }
    let mut rigid = Vec::new();
    for i in 0..count {
        let r = find(&mut root, i);
        if r != i {
            rigid.push(RigidLink {
                parent: r,
                child: i,
            });
        }
    }
    // Beams, one between each two neighbouring stations of a member.
    let mut beams = Vec::new();
    let mut pieces = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let st: Vec<(Real, usize)> = n.stations[i]
            .iter()
            .map(|(k, v)| (*k as Real / 1e3, *v))
            .collect();
        let rel = sim.releases.iter().find(|r| r.member == l.member);
        for (k, w) in st.windows(2).enumerate() {
            if w[0].1 == w[1].1 {
                continue;
            }
            let mut release_start = [false; 6];
            let mut release_end = [false; 6];
            if let Some(r) = rel {
                if k == 0 {
                    release_start = r.start;
                }
                if k + 2 == st.len() {
                    release_end = r.end;
                }
            }
            pieces.push(Piece {
                line: i,
                beam: beams.len(),
                from: w[0].0,
                to: w[1].0,
            });
            beams.push(Beam {
                start: w[0].1,
                end: w[1].1,
                u: l.u,
                section: l.section,
                material: l.material,
                release_start,
                release_end,
            });
        }
    }
    let mut c = Converted {
        lines,
        skipped,
        pieces,
        held,
        ..Default::default()
    };
    let v3 = |a: &[Real; 3]| V3::new(a[0], a[1], a[2]);
    let mut loads = Vec::new();
    for (l, on_node) in sim.loads.iter().zip(&node_loads) {
        if l.suppressed {
            continue;
        }
        let pt = |at: &At, force: V3, moment: V3, c: &Converted, loads: &mut Vec<Load>| {
            if let Some(node) = on_node {
                loads.push(Load::Node {
                    node: *node,
                    force,
                    moment,
                });
            } else if let Some(line) = c.line_of(at.member)
                && let Some((beam, s)) = c.piece_at(line, at.s)
            {
                loads.push(Load::Point {
                    beam,
                    at: s,
                    force,
                    moment,
                });
            }
        };
        let frame = |at: &At, c: &Converted| -> Option<(V3, V3, V3)> {
            let l = &c.lines[c.line_of(at.member)?];
            let x = (l.end - l.start).normalize();
            let u = l.u;
            Some((x, u, x.cross(&u)))
        };
        match &l.kind {
            FaLoadKind::Force { at, f } => pt(at, v3(f), V3::zeros(), &c, &mut loads),
            FaLoadKind::Moment { at, m } => pt(at, V3::zeros(), v3(m), &c, &mut loads),
            FaLoadKind::AxialMoment { at, m } => {
                if let Some((x, ..)) = frame(at, &c) {
                    pt(at, V3::zeros(), x * *m, &c, &mut loads);
                }
            }
            FaLoadKind::BendingMoment { at, m, angle } => {
                if let Some((_, u, v)) = frame(at, &c) {
                    let dir = u * angle.cos() + v * angle.sin();
                    pt(at, V3::zeros(), dir * *m, &c, &mut loads);
                }
            }
            FaLoadKind::Continuous {
                member,
                from,
                to,
                q,
            } => {
                let Some(line) = c.line_of(*member) else {
                    continue;
                };
                let len = c.lines[line].length();
                let (a, b) = (from.min(*to).clamp(0.0, len), from.max(*to).clamp(0.0, len));
                for p in c.pieces.iter().filter(|p| p.line == line) {
                    let (lo, hi) = (a.max(p.from), b.min(p.to));
                    if hi > lo + 1e-9 {
                        loads.push(Load::Uniform {
                            beam: p.beam,
                            from: lo - p.from,
                            to: hi - p.from,
                            q: v3(q),
                        });
                    }
                }
            }
            FaLoadKind::Gravity { g } => loads.push(Load::Gravity { g: v3(g) }),
        }
    }
    c.model = Model {
        nodes: n.at.iter().map(|p| p.coords).collect(),
        beams,
        held: held_dofs,
        links: rigid,
        loads,
    };
    Ok(c)
}

/// One member's static results.
#[derive(Clone, Debug, PartialEq)]
pub struct MemberResult {
    /// The member.
    pub member: u32,
    /// Index in [`Converted::lines`].
    pub line: usize,
    /// Stations along the member (`s` from its start).
    pub stations: Vec<Station>,
}

/// The outcome of a static run.
#[derive(Clone, Debug, PartialEq)]
pub struct StaticResult {
    /// The converted model.
    pub model: Converted,
    /// Solver output.
    pub solution: Solution,
    /// Per member.
    pub members: Vec<MemberResult>,
}

/// A static run of `sim`.
pub fn run_static(
    project: &Project,
    linked: &Linked,
    asm: &AssemblyDocument,
    sim: &Simulation,
) -> Result<StaticResult, String> {
    let model = convert(project, linked, asm, sim)?;
    static_of(model, sim)
}

/// The static run of a model already converted (needs no document, so it
/// can run on a worker thread).
pub fn static_of(model: Converted, sim: &Simulation) -> Result<StaticResult, String> {
    if model.model.held.is_empty() {
        return Err("add a constraint first".into());
    }
    if model.model.loads.is_empty() {
        return Err("add a load first".into());
    }
    let points = sim.beam_points.max(2) as usize;
    let solution = model.model.solve_static(points)?;
    let mut members = Vec::with_capacity(model.lines.len());
    for (i, l) in model.lines.iter().enumerate() {
        let len = l.length();
        let mut stations: Vec<Station> = Vec::new();
        for p in model.pieces.iter().filter(|p| p.line == i) {
            for st in &solution.beams[p.beam] {
                let mut x = *st;
                x.s += p.from;
                stations.push(x);
            }
        }
        // A station shared by two pieces stays only where the forces do
        // not jump: both are kept for the diagrams, in order.
        stations.sort_by(|a, b| a.s.total_cmp(&b.s));
        let _ = len;
        members.push(MemberResult {
            member: l.member,
            line: i,
            stations,
        });
    }
    Ok(StaticResult {
        model,
        solution,
        members,
    })
}

/// The outcome of a modal run.
#[derive(Clone, Debug, PartialEq)]
pub struct ModalResult {
    /// The converted model.
    pub model: Converted,
    /// The modes, lowest first.
    pub modes: Vec<Mode>,
}

/// A modal run of `sim` (no loads needed; a free frame shows its rigid modes).
pub fn run_modes(
    project: &Project,
    linked: &Linked,
    asm: &AssemblyDocument,
    sim: &Simulation,
    count: usize,
) -> Result<ModalResult, String> {
    modes_of(convert(project, linked, asm, sim)?, count)
}

/// The modal run of a model already converted (see [`static_of`]).
pub fn modes_of(model: Converted, count: usize) -> Result<ModalResult, String> {
    let modes = model.model.solve_modes(count, MODAL_PIECES)?;
    Ok(ModalResult { model, modes })
}

/// Elements per beam for the modal run.
const MODAL_PIECES: usize = 8;

/// The largest values over all members (for colours and the status bar).
pub fn largest(members: &[MemberResult], read: impl Fn(&MemberResult, &Station) -> Real) -> Real {
    members
        .iter()
        .flat_map(|m| m.stations.iter().map(move |s| (m, s)))
        .map(|(m, s)| read(m, s).abs())
        .fold(0.0, Real::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::frame::{InsertOptions, Skeleton, insert};
    use crate::document::{DocRef, DocumentId};
    use crate::kernel::Transform3;
    use crate::part::frame::Library;
    use crate::part::{OriginPlane, PartDocument, SketchOptions, SketchPlacement};
    use crate::sketch::Point2;

    fn opts() -> InsertOptions {
        InsertOptions {
            family: "Square hollow section (EN 10219)".into(),
            size: "40x40x3".into(),
            material: Some("Steel S235".into()),
            orientation: Default::default(),
            corners: Default::default(),
            layout: Default::default(),
        }
    }

    type Line2 = ((Real, Real), (Real, Real));

    /// Lines of an XZ-free sketch (XY plane), each a member.
    fn frame_of(lines: &[Line2]) -> (Project, DocumentId, Vec<u32>) {
        let mut sk = PartDocument::default();
        let o = SketchOptions {
            auto_project_origin: false,
            auto_reference_edges: false,
            ..Default::default()
        };
        let i = sk
            .create_sketch(SketchPlacement::on_origin_plane(OriginPlane::XY), &o)
            .unwrap();
        let ids: Vec<_> = lines
            .iter()
            .map(|(a, b)| {
                sk.sketches[i]
                    .sketch
                    .add_line(Point2::new(a.0, a.1), Point2::new(b.0, b.1))
            })
            .collect();
        let sketch = sk.sketches[i].id.clone();
        let mut p = Project::new("T");
        let skd = p.add_document("Skeleton", DocumentData::Part(sk));
        let mut a = AssemblyDocument::default();
        a.place(
            DocRef::local(skd),
            "Skeleton:1",
            Transform3::identity(),
            true,
        );
        let aid = p.add_document("Frame", DocumentData::Assembly(a));
        let list: Vec<Skeleton> = ids
            .into_iter()
            .map(|line| Skeleton::SketchLine {
                occurrence: 1,
                sub: Vec::new(),
                sketch: sketch.clone(),
                line,
            })
            .collect();
        let (_, occs) = insert(
            &mut p,
            &Linked::new(),
            aid,
            &list,
            &opts(),
            Library::builtin(),
            None,
        )
        .unwrap();
        (p, aid, occs)
    }

    fn asm(p: &Project, id: DocumentId) -> &AssemblyDocument {
        match &p.document(id).unwrap().data {
            DocumentData::Assembly(a) => a,
            _ => panic!("not an assembly"),
        }
    }

    fn sim() -> Simulation {
        Simulation::new("Simulation:1", SimKind::Static)
    }

    fn fixed(member: u32, s: Real) -> FaConstraint {
        FaConstraint {
            at: At { member, s },
            hold: Hold::Fixed,
            suppressed: false,
        }
    }

    fn force(member: u32, s: Real, f: [Real; 3]) -> FaLoad {
        FaLoad {
            kind: FaLoadKind::Force {
                at: At { member, s },
                f,
            },
            suppressed: false,
        }
    }

    fn close(got: Real, want: Real, rel: Real, what: &str) {
        assert!(
            (got - want).abs() <= rel * want.abs().max(1e-9),
            "{what}: got {got}, want {want}"
        );
    }

    /// A cantilever member: the section's own numbers and the steel's come
    /// through; tip deflection PL³/3EI.
    #[test]
    fn a_member_is_a_cantilever() {
        let (p, a, occ) = frame_of(&[((0.0, 0.0), (1000.0, 0.0))]);
        let mut s = sim();
        s.constraints.push(fixed(occ[0], 0.0));
        s.loads.push(force(occ[0], 1000.0, [0.0, 0.0, -1000.0]));
        let r = run_static(&p, &Linked::new(), asm(&p, a), &s).unwrap();
        let l = &r.model.lines[0];
        close(l.material.e, 210_000.0, 0.03, "E");
        // EN 10219: SHS 40x40x3, I = 9.32 cm⁴.
        close(l.section.iu, 9.32e4, 0.01, "I");
        let want = -1000.0 * 1000.0_f64.powi(3) / (3.0 * l.material.e * l.section.iu);
        let tip = r.members[0].stations.last().unwrap();
        close(tip.displacement[2], want, 1e-6, "tip");
        // The root stress is M / W.
        let root = r.members[0].stations[0];
        close(
            root.s_max(&l.section),
            1000.0 * 1000.0 / l.section.wu,
            1e-6,
            "stress",
        );
    }

    /// A crossing and a T make nodes of their own: two members joined at
    /// the middle of a third carry a load down to both supports.
    #[test]
    fn crossing_beams_share_a_node() {
        let (p, a, occ) = frame_of(&[((0.0, 0.0), (1000.0, 0.0)), ((500.0, 0.0), (500.0, 800.0))]);
        let mut s = sim();
        s.constraints.push(fixed(occ[0], 0.0));
        s.constraints.push(fixed(occ[0], 1000.0));
        s.loads.push(force(occ[1], 800.0, [0.0, 0.0, -500.0]));
        let c = convert(&p, &Linked::new(), asm(&p, a), &s).unwrap();
        // 4 nodes (three ends, the junction) and 3 beams; no rigid link.
        assert_eq!(c.model.nodes.len(), 4);
        assert_eq!(c.model.beams.len(), 3);
        assert!(c.model.links.is_empty());
        let r = run_static(&p, &Linked::new(), asm(&p, a), &s).unwrap();
        let sum: Real = r.solution.reactions.iter().map(|x| x[2]).sum();
        close(sum, 500.0, 1e-9, "the supports carry the load");
    }

    /// A member beside another, nearer than the tolerance, is joined by a
    /// rigid link; below the tolerance it stands alone.
    #[test]
    fn near_beams_are_rigidly_linked() {
        let (p, a, _) = frame_of(&[((0.0, 0.0), (1000.0, 0.0)), ((500.0, 60.0), (500.0, 800.0))]);
        let mut s = sim();
        let c = convert(&p, &Linked::new(), asm(&p, a), &s).unwrap();
        assert_eq!(c.model.links.len(), 1);
        s.tolerance = 0.5;
        let c = convert(&p, &Linked::new(), asm(&p, a), &s).unwrap();
        assert!(c.model.links.is_empty());
    }

    /// Curved members are listed, not converted.
    #[test]
    fn a_frame_without_a_constraint_or_load_is_refused() {
        let (p, a, occ) = frame_of(&[((0.0, 0.0), (1000.0, 0.0))]);
        let mut s = sim();
        assert!(run_static(&p, &Linked::new(), asm(&p, a), &s).is_err());
        s.constraints.push(fixed(occ[0], 0.0));
        let e = run_static(&p, &Linked::new(), asm(&p, a), &s).unwrap_err();
        assert!(e.contains("load"), "{e}");
    }

    /// A load inside the member keeps its place: Pab/L at the load's station.
    #[test]
    fn a_load_inside_a_member() {
        let (p, a, occ) = frame_of(&[((0.0, 0.0), (1500.0, 0.0))]);
        let mut s = sim();
        s.constraints.push(FaConstraint {
            at: At {
                member: occ[0],
                s: 0.0,
            },
            hold: Hold::Custom {
                dofs: [true, true, true, true, false, false],
            },
            suppressed: false,
        });
        s.constraints.push(FaConstraint {
            at: At {
                member: occ[0],
                s: 1500.0,
            },
            hold: Hold::Custom {
                dofs: [false, true, true, false, false, false],
            },
            suppressed: false,
        });
        s.loads.push(force(occ[0], 500.0, [0.0, 0.0, -1000.0]));
        let r = run_static(&p, &Linked::new(), asm(&p, a), &s).unwrap();
        let st = r.members[0]
            .stations
            .iter()
            .find(|x| (x.s - 500.0).abs() < 1e-3)
            .unwrap();
        close(st.mu.abs(), 1000.0 * 500.0 * 1000.0 / 1500.0, 1e-9, "Pab/L");
    }

    /// A release at a member's end carries no moment there.
    #[test]
    fn a_release_is_applied_to_the_member_end() {
        let (p, a, occ) = frame_of(&[((0.0, 0.0), (1000.0, 0.0))]);
        let mut s = sim();
        s.constraints.push(fixed(occ[0], 0.0));
        s.constraints.push(fixed(occ[0], 1000.0));
        s.releases.push(FaRelease {
            member: occ[0],
            start: [false; 6],
            end: [false, false, false, false, true, true],
        });
        s.loads.push(force(occ[0], 500.0, [0.0, 0.0, -1000.0]));
        let r = run_static(&p, &Linked::new(), asm(&p, a), &s).unwrap();
        let end = r.members[0].stations.last().unwrap();
        assert!(end.mu.abs() < 1e-6, "{}", end.mu);
    }

    /// A simulation survives a save.
    #[test]
    fn a_simulation_round_trips() {
        let mut s = sim();
        s.constraints.push(fixed(3, 0.0));
        s.loads.push(force(3, 10.0, [1.0, 2.0, 3.0]));
        s.loads.push(FaLoad {
            kind: FaLoadKind::Continuous {
                member: 3,
                from: 0.0,
                to: 100.0,
                q: [0.0, 0.0, -1.0],
            },
            suppressed: true,
        });
        let t = serde_json::to_string(&s).unwrap();
        let back: Simulation = serde_json::from_str(&t).unwrap();
        assert_eq!(back, s);
    }
}
