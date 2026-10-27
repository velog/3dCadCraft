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

/// A surface body thickened into a solid by `thickness` along its faces'
/// normals (negative: against them), or half each way when `symmetric`.
/// The solid is named `prefix`.
pub fn thicken_sheet(
    sheet: &ExactSolid,
    thickness: Real,
    symmetric: bool,
    prefix: &str,
) -> Result<ExactSolid, String> {
    let body = guarded("thicken", || sk::thicken(&sheet.body, thickness, symmetric))?;
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
    prefix: &str,
) -> Result<ExactSolid, String> {
    let sheet = face_sheet(s, faces)?;
    thicken_sheet(&sheet, thickness, symmetric, prefix)
}

/// Faces `faces` of `s` moved by `distance` along their outward normals
/// (negative: against them) into one surface body named `prefix`.
pub fn offset_sheet(
    s: &ExactSolid,
    faces: &[usize],
    distance: Real,
    prefix: &str,
) -> Result<ExactSolid, String> {
    let body = guarded("offset surface", || {
        sk::offset_surface(&s.body, faces, distance, OFFSET_TOLERANCE)
    })?;
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
