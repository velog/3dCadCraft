//! Thicken/Offset through the exact kernel (`docs/spec/2.14-thicken-offset.md`):
//! SolidCraft's `copy_faces` (picked faces as a surface body), `thicken` (a
//! surface body into a solid) and `offset_surface` (faces moved along their
//! normals into a surface body).
//!
//! Face names (from the kernel's history): a face's own copy keeps the
//! face's name, its moved copy is `<prefix>:Offset:<face>`, a wall along a
//! free edge `<prefix>:Wall:<edge index>`.

use super::{ExactSolid, FaceOrigin, Real, guarded, names_from, sk, v3};

/// How far an offset's fitted faces may stray from the true offset (mm).
const OFFSET_TOLERANCE: Real = 0.01;

/// Faces `faces` (kernel indices) of `s` copied out as a surface body; the
/// copies keep the faces' names.
pub fn face_sheet(s: &ExactSolid, faces: &[usize]) -> Result<ExactSolid, String> {
    let mut at = Vec::with_capacity(faces.len());
    for &f in faces {
        let p = super::face_point(s, f).ok_or_else(|| format!("no point on face {f}"))?;
        at.push(v3(&p));
    }
    let body = guarded("copy faces", || sk::copy_faces(&s.body, &at))?;
    let face_names = names_from(&body, &[s], &|_| None);
    Ok(ExactSolid {
        name: s.name.clone(),
        body,
        face_names,
        profiles: Vec::new(),
        sheet: true,
    })
}

/// The name of the moved copy of `sheet`'s face `face`.
fn offset_name(sheet: &ExactSolid, face: usize, prefix: &str) -> String {
    match sheet.face_names.get(face).cloned().flatten() {
        Some(n) => format!("{prefix}:Offset:{n}"),
        None => format!("{prefix}:Offset:{face}"),
    }
}

/// The fitting tolerances tried for free-form faces when none is given
/// (Inventor's Optimized): the finest that fits wins.
const OPTIMIZED: [Real; 5] = [0.01, 0.03, 0.1, 0.3, 1.0];

/// A surface body thickened into a solid by `thickness` along its faces'
/// normals (negative: against them), or half each way when `symmetric`.
/// Free-form faces are fitted within `tolerance` (mm); `None`: the finest of
/// 0.01 … 1 mm that fits, never more than 2 % of the thickness. The solid is
/// named `prefix`.
pub fn thicken_sheet(
    sheet: &ExactSolid,
    thickness: Real,
    symmetric: bool,
    tolerance: Option<Real>,
    prefix: &str,
) -> Result<ExactSolid, String> {
    let tries: Vec<Real> = match tolerance {
        Some(t) => vec![t],
        None => {
            let cap = (thickness.abs() * 0.02).max(OPTIMIZED[0]);
            OPTIMIZED.iter().copied().filter(|t| *t <= cap).collect()
        }
    };
    let mut last = String::from("no tolerance to try");
    let mut made = None;
    for tol in tries {
        match guarded("thicken", || {
            sk::thicken_with_tolerance(&sheet.body, thickness, symmetric, tol)
        }) {
            Ok(b) => {
                made = Some(b);
                break;
            }
            // Only a fit that is too coarse is worth another try.
            Err(e) if e.contains("can't be fitted") => last = e,
            Err(e) => return Err(e),
        }
    }
    let body = made.ok_or(last)?;
    let face_names = names_from(&body, &[sheet], &|o| match o {
        FaceOrigin::Offset { face, .. } => Some(offset_name(sheet, *face, prefix)),
        FaceOrigin::Edge { edge, .. } => Some(format!("{prefix}:Wall:{edge}")),
        _ => None,
    });
    Ok(ExactSolid {
        name: Some(prefix.to_owned()),
        body,
        face_names,
        profiles: Vec::new(),
        sheet: false,
    })
}

/// Faces `faces` of `s` thickened into a slab (a solid named `prefix`),
/// to be joined to, cut from or intersected with a solid by the caller.
pub fn thicken_faces(
    s: &ExactSolid,
    faces: &[usize],
    thickness: Real,
    symmetric: bool,
    tolerance: Option<Real>,
    prefix: &str,
) -> Result<ExactSolid, String> {
    let sheet = face_sheet(s, faces)?;
    thicken_sheet(&sheet, thickness, symmetric, tolerance, prefix)
}

/// Every face of the closed solid `s` moved by `distance` (Join outward,
/// Cut inward of all its faces): the offset faces close up into the new
/// solid, named like `s`. A Boolean of `s` with its own shell finds no
/// curve where the two meet (concentric spheres: "no result").
pub fn offset_solid(
    s: &ExactSolid,
    distance: Real,
    tolerance: Option<Real>,
) -> Result<ExactSolid, String> {
    let all: Vec<usize> = (0..s.face_names.len()).collect();
    let mut r = offset_sheet(s, &all, distance, tolerance, "")?;
    if r.body.is_surface() {
        return Err("the offset faces do not close up".into());
    }
    r.sheet = false;
    r.name = s.name.clone();
    // The faces keep their own names (the solid's faces, moved).
    r.face_names = s.face_names.clone();
    Ok(r)
}

/// Faces `faces` of `s` moved by `distance` along their outward normals
/// (negative: against them) into one surface body named `prefix`.
pub fn offset_sheet(
    s: &ExactSolid,
    faces: &[usize],
    distance: Real,
    tolerance: Option<Real>,
    prefix: &str,
) -> Result<ExactSolid, String> {
    let tries: Vec<Real> = match tolerance {
        Some(t) => vec![t],
        None => OPTIMIZED
            .iter()
            .copied()
            .filter(|t| *t <= (distance.abs() * 0.02).max(OFFSET_TOLERANCE))
            .collect(),
    };
    let mut last = String::from("no tolerance to try");
    let mut made = None;
    for tol in tries {
        match guarded("offset surface", || {
            sk::offset_surface(&s.body, faces, distance, tol)
        }) {
            Ok(b) => {
                made = Some(b);
                break;
            }
            Err(e) if e.contains("can't be fitted") => last = e,
            Err(e) => return Err(e),
        }
    }
    let body = made.ok_or(last)?;
    let face_names = names_from(&body, &[s], &|o| match o {
        FaceOrigin::Offset { face, .. } => Some(offset_name(s, *face, prefix)),
        _ => None,
    });
    Ok(ExactSolid {
        name: Some(prefix.to_owned()),
        body,
        face_names,
        profiles: Vec::new(),
        sheet: true,
    })
}
