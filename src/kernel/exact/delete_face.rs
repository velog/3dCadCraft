//! Delete Face through the exact kernel (`docs/spec/2.14-delete-face.md`):
//! SolidCraft's `delete_faces` (Heal: the planar neighbours extend over the
//! gap), `remove_faces` (no Heal: the faces left are a surface body) and our
//! `delete_shell` (Lump or Void: a whole shell goes).
//!
//! Every face left keeps its name (the kernel's history maps it to the
//! face it came from); Delete Face makes no new faces.

use super::{ExactSolid, guarded, names_from, sk};

fn named(s: &ExactSolid, body: sk::Body, sheet: bool) -> ExactSolid {
    let face_names = names_from(&body, &[s], &|_| None);
    ExactSolid {
        name: s.name.clone(),
        body,
        face_names,
        profiles: Vec::new(),
        sheet,
    }
}

/// Faces `faces` (kernel indices) of `s` deleted. `heal`: the neighbouring
/// planes extend until they close the gap (a solid stays a solid); else the
/// faces left are a surface body.
pub fn delete_faces(s: &ExactSolid, faces: &[usize], heal: bool) -> Result<ExactSolid, String> {
    if heal {
        let body = guarded("delete face", || sk::delete_faces(&s.body, faces))?;
        Ok(named(s, body, s.sheet))
    } else {
        let body = guarded("delete face", || sk::remove_faces(&s.body, faces))?;
        Ok(named(s, body, true))
    }
}

/// The shell (a lump, or a void) of `s` holding its face `face` deleted.
pub fn delete_shell(s: &ExactSolid, face: usize) -> Result<ExactSolid, String> {
    let body = guarded("delete face", || sk::delete_shell(&s.body, face))?;
    Ok(named(s, body, s.sheet))
}
