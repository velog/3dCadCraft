//! Faces for agents: a short description of every face, aliases (`Extrusion1:Top`, `+X`) and the
//! face at a point (spec 0.7 §3.4a).
//!
//! Persistent names (rule 2) stay the only names stored anywhere: an alias is worked out from the
//! body each time it is read and resolved to a persistent name at once, and answers always give
//! the persistent name (an alias can mean another face after the next feature).

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::args::{p3, round};
use crate::kernel::{BRep, Point3, Real, Surface, Vector3};

/// The six axis directions with their alias words (Z up, the view cube's names).
pub const DIRECTIONS: [(&str, &str, [Real; 3]); 6] = [
    ("+X", "Right", [1.0, 0.0, 0.0]),
    ("-X", "Left", [-1.0, 0.0, 0.0]),
    ("+Y", "Back", [0.0, 1.0, 0.0]),
    ("-Y", "Front", [0.0, -1.0, 0.0]),
    ("+Z", "Top", [0.0, 0.0, 1.0]),
    ("-Z", "Bottom", [0.0, 0.0, -1.0]),
];

/// One face name of the body (all its pieces together).
#[derive(Clone, Debug)]
pub struct FaceInfo {
    /// The persistent name.
    pub name: String,
    /// `plane`, `cylinder`, `cone`, `sphere`, `torus`, `free-form`.
    pub surface: &'static str,
    /// Area (mm², from the display triangles).
    pub area: Real,
    /// Area-weighted centre.
    pub center: Point3,
    /// The outward normal of a plane (area-weighted over its triangles, unit).
    pub normal: Option<Vector3>,
    /// Index into [`DIRECTIONS`] when the plane's normal lies along an axis.
    pub axis: Option<usize>,
    /// Radius of a cylinder, cone (at its origin) or sphere.
    pub radius: Option<Real>,
    /// Aliases, the shortest first (`Top`, `Extrusion1:Top`, `Extrusion1:+Z`, `+Z`).
    pub aliases: Vec<String>,
}

impl FaceInfo {
    /// The alias shown in answers (the first), if any.
    pub fn alias(&self) -> Option<&str> {
        self.aliases.first().map(String::as_str)
    }

    /// `+Z` for an axis plane, else the rounded normal, else null.
    pub fn dir(&self) -> Value {
        match (self.axis, self.normal) {
            (Some(k), _) => json!(DIRECTIONS[k].0),
            (None, Some(n)) => json!([round(n.x), round(n.y), round(n.z)]),
            _ => Value::Null,
        }
    }

    /// The compact row `[name, surface, alias, dir]`.
    pub fn row(&self) -> Value {
        json!([self.name, self.surface, self.alias(), self.dir()])
    }

    /// Everything known, as an object.
    pub fn detail(&self) -> Value {
        let mut v = json!({
            "name": self.name,
            "surface": self.surface,
            "area": round(self.area),
            "center": p3(&self.center),
            "dir": self.dir(),
        });
        if !self.aliases.is_empty() {
            v["aliases"] = json!(self.aliases);
        }
        if let Some(n) = self.normal {
            v["normal"] = json!([round(n.x), round(n.y), round(n.z)]);
        }
        if let Some(r) = self.radius {
            v["radius"] = json!(round(r));
        }
        v
    }
}

/// The feature a face name belongs to: the text before its first `:`.
pub fn feature_of(name: &str) -> &str {
    name.split(':').next().unwrap_or(name)
}

fn surface_info(body: &BRep, f: &crate::kernel::Face) -> (&'static str, Option<Real>) {
    match body.surface(f.facet_of.unwrap_or(f.surface)) {
        Some(Surface::Plane(_)) => ("plane", None),
        Some(Surface::Cylinder(c)) => ("cylinder", Some(c.radius)),
        Some(Surface::Cone(c)) => ("cone", Some(c.radius)),
        Some(Surface::Sphere(s)) => ("sphere", Some(s.radius)),
        Some(Surface::Torus(_)) => ("torus", None),
        _ => ("free-form", None),
    }
}

/// Every face name of `body`, sorted by name, with its description and aliases.
pub fn faces(body: &BRep) -> Vec<FaceInfo> {
    // name → (surface, radius, area, Σ centre·area, Σ normal·area)
    type Acc = (&'static str, Option<Real>, Real, Vector3, Vector3);
    let mut by: BTreeMap<String, Acc> = BTreeMap::new();
    for (_, f) in body.faces() {
        let Some(name) = &f.name else { continue };
        let (kind, radius) = surface_info(body, f);
        let e = by.entry(name.clone()).or_insert((
            kind,
            radius,
            0.0,
            Vector3::zeros(),
            Vector3::zeros(),
        ));
        let Some(m) = &f.mesh else { continue };
        for t in &m.triangles {
            let (Some(p), Some(q), Some(r)) = (
                m.positions.get(t[0] as usize),
                m.positions.get(t[1] as usize),
                m.positions.get(t[2] as usize),
            ) else {
                continue;
            };
            let n = (q - p).cross(&(r - p));
            let ar = n.norm() / 2.0;
            e.2 += ar;
            e.3 += (p.coords + q.coords + r.coords) / 3.0 * ar;
            e.4 += n / 2.0;
        }
    }
    let mut out: Vec<FaceInfo> = by
        .into_iter()
        .map(|(name, (surface, radius, area, c, n))| {
            let center = Point3::from(if area > 0.0 { c / area } else { c });
            let normal = (surface == "plane" && n.norm() > 1e-12).then(|| n.normalize());
            let axis = normal.and_then(|n| {
                DIRECTIONS
                    .iter()
                    .position(|(_, _, d)| n.dot(&Vector3::from(*d)) > 1.0 - 1e-9)
            });
            FaceInfo {
                name,
                surface,
                area,
                center,
                normal,
                axis,
                radius,
                aliases: Vec::new(),
            }
        })
        .collect();
    add_aliases(&mut out);
    out
}

fn add_aliases(faces: &mut [FaceInfo]) {
    let names: std::collections::HashSet<String> = faces.iter().map(|f| f.name.clone()).collect();
    let size = faces
        .iter()
        .map(|f| f.center.coords.norm())
        .fold(1.0, Real::max);
    let tol = 1e-6 * size;
    let mut part: Vec<(usize, String)> = Vec::new();
    for (k, (sign, word, d)) in DIRECTIONS.iter().enumerate() {
        let d = Vector3::from(*d);
        // Part aliases: the outermost plane along d, alone in its plane.
        let along: Vec<(usize, Real)> = faces
            .iter()
            .enumerate()
            .filter(|(_, f)| f.axis == Some(k))
            .map(|(i, f)| (i, f.center.coords.dot(&d)))
            .collect();
        if let Some(&(best, top)) = along.iter().max_by(|a, b| a.1.total_cmp(&b.1))
            && along.iter().filter(|(_, h)| (h - top).abs() <= tol).count() == 1
        {
            for a in [*word, *sign] {
                if !names.contains(a) {
                    part.push((best, a.to_owned()));
                }
            }
        }
        // Feature aliases: the only face of its feature with that normal.
        let mut count: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for (i, f) in faces.iter().enumerate() {
            if f.axis == Some(k) {
                count.entry(feature_of(&f.name)).or_default().push(i);
            }
        }
        let mut feat: Vec<(usize, String)> = Vec::new();
        for (feature, list) in count {
            if let [i] = list.as_slice() {
                for a in [format!("{feature}:{word}"), format!("{feature}:{sign}")] {
                    if !names.contains(&a) {
                        feat.push((*i, a));
                    }
                }
            }
        }
        for (i, a) in feat {
            faces[i].aliases.push(a);
        }
    }
    for (i, a) in part {
        faces[i].aliases.push(a);
    }
    // The part's own words first, view words before signs (`Top`, `+Z`, `Extrusion1:Top`).
    for f in faces.iter_mut() {
        f.aliases.sort_by_key(|a| {
            let last = a.rsplit(':').next().unwrap_or(a);
            (a.contains(':'), last.starts_with(['+', '-']), a.len())
        });
        f.aliases.dedup();
    }
}

/// The persistent name `s` stands for: itself when a face has that name, else the face whose
/// alias it is (case-insensitive). `None`: no such face.
pub fn resolve_face(body: &BRep, s: &str) -> Option<String> {
    let s = s.trim();
    if body.faces().any(|(_, f)| f.name.as_deref() == Some(s)) {
        return Some(s.to_owned());
    }
    faces(body)
        .into_iter()
        .find(|f| f.aliases.iter().any(|a| a.eq_ignore_ascii_case(s)))
        .map(|f| f.name)
}

/// `FaceA|FaceB` with each side resolved ([`resolve_face`]); a side that is no face stays as
/// given (the caller reports it).
pub fn resolve_edge(body: &BRep, s: &str) -> String {
    match s.split_once('|') {
        Some((a, b)) => {
            let r = |x: &str| resolve_face(body, x).unwrap_or_else(|| x.trim().to_owned());
            format!("{}|{}", r(a), r(b))
        }
        None => s.to_owned(),
    }
}

/// A face reached by [`face_at`].
#[derive(Clone, Debug)]
pub struct Hit {
    /// Its persistent name.
    pub name: String,
    /// The point on it.
    pub point: Point3,
    /// From the query point (along the ray, or straight).
    pub distance: Real,
}

fn triangles(body: &BRep) -> impl Iterator<Item = (&str, [Point3; 3])> {
    body.faces().flat_map(|(_, f)| {
        let name = f.name.as_deref();
        let mesh = f.mesh.as_deref();
        mesh.into_iter()
            .flat_map(|m| m.triangles.iter().map(move |t| (m, t)))
            .filter_map(move |(m, t)| {
                let p = |k: usize| m.positions.get(t[k] as usize).copied();
                Some((name?, [p(0)?, p(1)?, p(2)?]))
            })
    })
}

/// Möller–Trumbore: the ray parameter where `o + t·d` meets the triangle.
fn ray_triangle(o: &Point3, d: &Vector3, [a, b, c]: &[Point3; 3]) -> Option<Real> {
    let (e1, e2) = (b - a, c - a);
    let p = d.cross(&e2);
    let det = e1.dot(&p);
    if det.abs() < 1e-14 {
        return None;
    }
    let s = o - a;
    let u = s.dot(&p) / det;
    if !(-1e-9..=1.0 + 1e-9).contains(&u) {
        return None;
    }
    let q = s.cross(&e1);
    let v = d.dot(&q) / det;
    if v < -1e-9 || u + v > 1.0 + 1e-9 {
        return None;
    }
    let t = e2.dot(&q) / det;
    (t >= -1e-9).then_some(t)
}

/// The point of triangle `abc` closest to `p` (Ericson, Real-Time Collision Detection 5.1.5).
fn closest_on_triangle(p: &Point3, [a, b, c]: &[Point3; 3]) -> Point3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(&ap), ac.dot(&ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return *a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(&bp), ac.dot(&bp));
    if d3 >= 0.0 && d4 <= d3 {
        return *b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(&cp), ac.dot(&cp));
    if d6 >= 0.0 && d5 <= d6 {
        return *c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// The face the ray from `o` along `dir` meets first, or (no direction) the face nearest `o`.
pub fn face_at(body: &BRep, o: &Point3, dir: Option<&Vector3>) -> Option<Hit> {
    let mut best: Option<Hit> = None;
    match dir {
        Some(d) => {
            let d = d.try_normalize(1e-12)?;
            for (name, t) in triangles(body) {
                if let Some(s) = ray_triangle(o, &d, &t)
                    && best.as_ref().is_none_or(|b| s < b.distance)
                {
                    best = Some(Hit {
                        name: name.to_owned(),
                        point: o + d * s,
                        distance: s.max(0.0),
                    });
                }
            }
        }
        None => {
            for (name, t) in triangles(body) {
                let q = closest_on_triangle(o, &t);
                let s = (q - o).norm();
                if best.as_ref().is_none_or(|b| s < b.distance) {
                    best = Some(Hit {
                        name: name.to_owned(),
                        point: q,
                        distance: s,
                    });
                }
            }
        }
    }
    best
}
