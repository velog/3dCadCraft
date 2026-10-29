//! **Copy Object** (`docs/spec/2.13-copy-object.md`, format revision 40): faces or whole bodies
//! copied into new surface bodies, one composite, one solid or several solids, as the
//! [`BodyOp::CopyObject`](super::BodyOp::CopyObject) variant.
//!
//! References follow rule 2: faces are [`FaceRef`]s, bodies persistent names. New bodies are
//! named after the feature (`CopyObject1`, then `CopyObject1#2`, `#3` …) and every copied face
//! is `<body>:<source face>` (`CopyObject1:Extrusion1:Top`), so a copy never shares a face name
//! with its source.

use serde::{Deserialize, Serialize};

use super::exact::Fail::{self, User};
use super::shell::FaceRef;
use crate::kernel::exact::{self, ExactOp, ExactSolid, surface_ops};
use crate::kernel::{Point3, Real};

/// What the Objects selector takes (Inventor's Select: Face · Body).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CopySelect {
    /// Faces of solids or of surface bodies.
    #[default]
    Face,
    /// Whole bodies (solids or surface bodies).
    Body,
}

/// Inventor's Create New (Group needs the construction environment and is not offered).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CopyOutput {
    /// One surface body per set of contiguous faces (per body in Body mode).
    #[default]
    Surface,
    /// Everything in one surface body.
    Composite,
    /// One solid (the faces must close a volume).
    Solid,
    /// One solid per closed set of faces (per body in Body mode).
    MultipleSolids,
}

/// **Copy Object**.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CopyObjectOp {
    /// Face or Body.
    #[serde(default)]
    pub select: CopySelect,
    /// The faces (Face mode).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub faces: Vec<FaceRef>,
    /// The bodies, by name (Body mode).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bodies: Vec<String>,
    /// Create New.
    #[serde(default)]
    pub output: CopyOutput,
    /// Delete Original: the copied bodies leave the part, copied faces leave their body.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub delete_original: bool,
}

impl CopyObjectOp {
    /// Refuses a definition with nothing to copy.
    pub fn validate(&self) -> Result<(), String> {
        let empty = match self.select {
            CopySelect::Face => self.faces.is_empty(),
            CopySelect::Body => self.bodies.is_empty(),
        };
        if empty {
            return Err("select the faces or bodies to copy".into());
        }
        Ok(())
    }

    /// The browser's tooltip.
    pub fn describe(&self) -> String {
        let what = match self.select {
            CopySelect::Face => format!("{} face(s)", self.faces.len()),
            CopySelect::Body => format!("{} body(ies)", self.bodies.len()),
        };
        let into = match self.output {
            CopyOutput::Surface => "surface bodies",
            CopyOutput::Composite => "one composite",
            CopyOutput::Solid => "one solid",
            CopyOutput::MultipleSolids => "solids",
        };
        let moved = if self.delete_original {
            " (original deleted)"
        } else {
            ""
        };
        format!("Copy of {what} into {into}{moved}")
    }
}

/// The name of new body `k` (from 0) of feature `id`: `id`, `id#2`, `id#3` …
fn body_name(id: &str, k: usize) -> String {
    if k == 0 {
        id.to_owned()
    } else {
        format!("{id}#{}", k + 1)
    }
}

/// `s` as body `name`: each face name prefixed by it (once).
fn named(mut s: ExactSolid, name: &str) -> ExactSolid {
    let prefix = format!("{name}:");
    for n in s.face_names.iter_mut() {
        *n = Some(match n.take() {
            Some(x) if x.starts_with(&prefix) => x,
            Some(x) => format!("{prefix}{x}"),
            None => format!("{prefix}Face"),
        });
    }
    s.name = Some(name.to_owned());
    s
}

/// The display edges of a body as straight pieces.
fn segments(s: &ExactSolid) -> Vec<[Point3; 2]> {
    let Ok(b) = exact::to_brep(std::slice::from_ref(s)) else {
        return Vec::new();
    };
    let opts = crate::kernel::TessellationOptions::default();
    b.edges()
        .filter_map(|(e, _)| b.sample_edge(e, &opts).ok())
        .flat_map(|pts| pts.windows(2).map(|w| [w[0], w[1]]).collect::<Vec<_>>())
        .collect()
}

/// Whether two pieces of edges overlap (two of one's ends and middle lie on the other).
fn overlap(a: &[Point3; 2], b: &[Point3; 2], tol: Real) -> bool {
    let on = |q: &Point3| {
        let ab = b[1] - b[0];
        let t = ((q - b[0]).dot(&ab) / ab.norm_squared().max(1e-24)).clamp(0.0, 1.0);
        (b[0] + ab * t - q).norm() <= tol
    };
    let m = nalgebra::center(&a[0], &a[1]);
    [a[0], a[1], m].iter().filter(|q| on(q)).count() >= 2
}

/// Faces `faces` (kernel indices) of `s` in sets of contiguous faces (sharing an edge).
fn contiguous(s: &ExactSolid, faces: &[usize]) -> Result<Vec<Vec<usize>>, Fail> {
    if faces.len() < 2 {
        return Ok(vec![faces.to_vec()]);
    }
    let tol = (s.size() * 1e-6).max(1e-5);
    let mut segs = Vec::with_capacity(faces.len());
    for &f in faces {
        let one =
            exact::face_sheet(s, &[f]).map_err(|e| User(format!("cannot copy a face ({e})")))?;
        segs.push(segments(&one));
    }
    // Union-find over the faces.
    let mut root: Vec<usize> = (0..faces.len()).collect();
    fn find(root: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while root[i] != i {
            root[i] = root[root[i]];
            i = root[i];
        }
        i
    }
    for i in 0..faces.len() {
        for j in i + 1..faces.len() {
            let meet = segs[i]
                .iter()
                .any(|a| segs[j].iter().any(|b| overlap(a, b, tol)));
            if meet {
                let (ri, rj) = (find(&mut root, i), find(&mut root, j));
                root[ri] = rj;
            }
        }
    }
    let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
    for (i, &f) in faces.iter().enumerate() {
        let r = find(&mut root, i);
        match groups.iter_mut().find(|(g, _)| *g == r) {
            Some((_, v)) => v.push(f),
            None => groups.push((r, vec![f])),
        }
    }
    Ok(groups.into_iter().map(|(_, v)| v).collect())
}

/// Several bodies sewn into one where they meet (side by side elsewhere); `solid`: it must
/// close a volume.
fn sewn(parts: &[ExactSolid], name: &str, solid: bool) -> Result<ExactSolid, Fail> {
    if let [one] = parts {
        return one.clone().flagged(!solid).map_err(User);
    }
    let refs: Vec<&ExactSolid> = parts.iter().collect();
    let s = surface_ops::stitch(&refs, 0.00025, !solid, name)
        .map_err(|e| User(format!("the copies cannot be put into one body ({e})")))?;
    s.flagged(!solid).map_err(User)
}

/// Copy Object of the solids `cur` and the surface bodies `sheets` built so far. Returns every
/// body after it (solids and surface bodies).
pub(super) fn copy_object(
    op: &CopyObjectOp,
    id: &str,
    cur: &[ExactSolid],
    sheets: &[ExactSolid],
) -> Result<Vec<ExactSolid>, Fail> {
    op.validate().map_err(User)?;
    let mut all: Vec<ExactSolid> = cur.iter().chain(sheets).cloned().collect();
    let kernel = |e: String| User(format!("the geometry kernel cannot copy it ({e})"));
    let mut made: Vec<ExactSolid> = Vec::new();
    // Indices of `all` to drop (Delete Original of whole bodies).
    let mut gone: Vec<usize> = Vec::new();
    match op.select {
        CopySelect::Body => {
            let mut src = Vec::new();
            for b in &op.bodies {
                let i = all
                    .iter()
                    .position(|s| s.name.as_deref() == Some(b.as_str()))
                    .ok_or_else(|| User(format!("the body {b} no longer exists")))?;
                if !src.contains(&i) {
                    src.push(i);
                }
            }
            let identity = nalgebra::Matrix4::<Real>::identity();
            let copy = |i: usize, name: &str| -> Result<ExactSolid, Fail> {
                let s = &all[i];
                exact::transformed(s, &identity, name, Some(name))
                    .map_err(kernel)?
                    .flagged(s.is_open())
                    .map_err(User)
            };
            match op.output {
                CopyOutput::Surface | CopyOutput::MultipleSolids => {
                    let solid = op.output == CopyOutput::MultipleSolids;
                    for (k, &i) in src.iter().enumerate() {
                        let name = body_name(id, k);
                        let c = copy(i, &name)?.flagged(!solid).map_err(|e| {
                            User(format!(
                                "{}: {e}",
                                all[i].name.as_deref().unwrap_or("a body")
                            ))
                        })?;
                        made.push(c);
                    }
                }
                CopyOutput::Composite => {
                    let parts: Vec<ExactSolid> =
                        src.iter().map(|&i| copy(i, id)).collect::<Result<_, _>>()?;
                    made.push(named(sewn(&parts, id, false)?, id));
                }
                CopyOutput::Solid => {
                    let mut parts = Vec::new();
                    for &i in &src {
                        parts.push(copy(i, id)?.flagged(false).map_err(|e| {
                            User(format!(
                                "{}: {e}",
                                all[i].name.as_deref().unwrap_or("a body")
                            ))
                        })?);
                    }
                    let mut it = parts.into_iter();
                    let Some(mut joined) = it.next() else {
                        return Err(User("select the bodies to copy".into()));
                    };
                    for p in it {
                        joined = exact::boolean(&joined, &p, ExactOp::Union)
                            .map_err(kernel)?
                            .ok_or_else(|| User("the bodies cannot be joined".into()))?;
                    }
                    made.push(named(joined, id));
                }
            }
            if op.delete_original {
                gone = src;
            }
        }
        CopySelect::Face => {
            let asks: Vec<(&str, Point3)> = op
                .faces
                .iter()
                .map(|r| (r.face.as_str(), Point3::from(r.hint)))
                .collect();
            // The picked faces per body, in the order the bodies were first picked.
            let mut by_body: Vec<(usize, Vec<usize>)> = Vec::new();
            for (r, found) in op.faces.iter().zip(exact::faces_in_solids(&all, &asks)) {
                let Some((i, f)) = found else {
                    return Err(User(format!("the face {} no longer exists", r.face)));
                };
                match by_body.iter_mut().find(|(b, _)| *b == i) {
                    Some((_, v)) if !v.contains(&f) => v.push(f),
                    Some(_) => {}
                    None => by_body.push((i, vec![f])),
                }
            }
            let sheet =
                |i: usize, faces: &[usize]| exact::face_sheet(&all[i], faces).map_err(kernel);
            match op.output {
                CopyOutput::Surface | CopyOutput::MultipleSolids => {
                    let solid = op.output == CopyOutput::MultipleSolids;
                    for (i, faces) in &by_body {
                        for g in contiguous(&all[*i], faces)? {
                            let name = body_name(id, made.len());
                            let s = sheet(*i, &g)?.flagged(!solid).map_err(User)?;
                            made.push(named(s, &name));
                        }
                    }
                }
                CopyOutput::Composite | CopyOutput::Solid => {
                    let solid = op.output == CopyOutput::Solid;
                    let parts: Vec<ExactSolid> = by_body
                        .iter()
                        .map(|(i, faces)| sheet(*i, faces))
                        .collect::<Result<_, _>>()?;
                    made.push(named(sewn(&parts, id, solid)?, id));
                }
            }
            if op.delete_original {
                for (i, faces) in &by_body {
                    if faces.len() >= all[*i].face_names.len() {
                        gone.push(*i);
                    } else {
                        all[*i] = exact::delete_faces(&all[*i], faces, false)
                            .map_err(|e| User(e.replace("Delete Face: ", "")))?;
                    }
                }
            }
        }
    }
    let mut out: Vec<ExactSolid> = all
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !gone.contains(i))
        .map(|(_, s)| s)
        .collect();
    out.extend(made);
    Ok(out)
}
