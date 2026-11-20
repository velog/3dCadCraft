//! The joints of a frame (spec 6.1 §3.14, owner's batch 2026-11-20): every
//! place where two members meet — at their ends (a corner) or with one
//! ending on the middle of the other (a T) — with the treatment it has now,
//! and the change of one joint's treatment (the pair's earlier joint
//! treatments replaced, the frame solved; on an error nothing changes).

use super::{
    Corners, CornerType, Frame, Linked, Real, Solved, TeeJoint, Treatment, asm_mut, asm_of, joint,
    skeleton_line, solve, tee,
};
use crate::document::{AssemblyDocument, DocumentId, Project};
use crate::kernel::Point3;

/// What kind of place a joint is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JointKind {
    /// Both members end there.
    Corner,
    /// The second member ends on the middle of the first.
    Tee,
}

/// A joint's treatment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JointType {
    /// Uncut.
    None,
    /// Miter (Full Miter Cut).
    Miter,
    /// Corner Joint: the first member runs through, the second is trimmed or
    /// extended to it.
    FirstThrough,
    /// Corner Joint the other way round (corners only).
    SecondThrough,
    /// Notch: the second member is notched by the first (T joints).
    Notch,
}

impl JointType {
    /// The choices of a joint of `kind`, in the menu's order.
    pub fn choices(kind: JointKind) -> &'static [JointType] {
        match kind {
            JointKind::Corner => &[
                JointType::Miter,
                JointType::FirstThrough,
                JointType::SecondThrough,
                JointType::None,
            ],
            JointKind::Tee => &[JointType::FirstThrough, JointType::Notch, JointType::None],
        }
    }
}

/// One joint of a frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Joint {
    /// The frame.
    pub frame: u32,
    /// The members (frame assembly ids): at a corner in the frame's order,
    /// at a T the member running on first, the one ending on it second.
    pub members: [u32; 2],
    /// Where the axes meet (frame assembly coordinates).
    pub at: Point3,
    /// Corner or T.
    pub kind: JointKind,
    /// The treatment the pair has now (the last joint treatment of the pair).
    pub current: JointType,
    /// Its gap (Miter distance, Corner Joint or Notch offset).
    pub gap: Real,
}

/// Every joint of every frame of `asm`.
pub fn joints(project: &Project, linked: &Linked, asm: &AssemblyDocument) -> Vec<Joint> {
    let mut out = Vec::new();
    for f in &asm.frames {
        frame_joints(project, linked, asm, f, &mut out);
    }
    out
}

fn frame_joints(
    project: &Project,
    linked: &Linked,
    asm: &AssemblyDocument,
    f: &Frame,
    out: &mut Vec<Joint>,
) {
    let lines: Vec<(u32, (Point3, Point3), Real)> = f
        .members
        .iter()
        .filter_map(|m| {
            let l = skeleton_line(project, linked, asm, &m.skeleton).ok()?;
            let (w, h) = m.section().map_or((0.0, 0.0), |s| s.extent());
            Some((m.occurrence, l, w + h))
        })
        .collect();
    for i in 0..lines.len() {
        for j in i + 1..lines.len() {
            let (oi, li, si) = lines[i];
            let (oj, lj, sj) = lines[j];
            let reach = si.max(sj);
            let (kind, members, at) = if let Some((c, _, _)) = joint(li, lj, reach) {
                (JointKind::Corner, [oi, oj], c)
            } else if let Some((c, _)) = tee(li, lj, reach) {
                (JointKind::Tee, [oi, oj], c)
            } else if let Some((c, _)) = tee(lj, li, reach) {
                (JointKind::Tee, [oj, oi], c)
            } else {
                continue;
            };
            let (current, gap) = current_of(f, members);
            out.push(Joint {
                frame: f.id,
                members,
                at,
                kind,
                current,
                gap,
            });
        }
    }
}

/// Is `t` a joint treatment of the pair `m` (either order)?
fn of_pair(t: &Treatment, m: [u32; 2]) -> bool {
    let ms = match t {
        Treatment::Miter { .. } | Treatment::CornerJoint { .. } | Treatment::Notch { .. } => {
            t.members()
        }
        _ => return false,
    };
    ms.len() == 2 && ms.contains(&m[0]) && ms.contains(&m[1])
}

/// The pair's treatment now and its gap.
fn current_of(f: &Frame, m: [u32; 2]) -> (JointType, Real) {
    let Some(t) = f.treatments.iter().rev().find(|t| of_pair(t, m)) else {
        return (JointType::None, 0.0);
    };
    match t {
        Treatment::Miter { gap, .. } => (JointType::Miter, *gap),
        Treatment::CornerJoint { a, offsets, .. } if *a == m[0] => {
            (JointType::FirstThrough, offsets[1])
        }
        Treatment::CornerJoint { offsets, .. } => (JointType::SecondThrough, offsets[1]),
        Treatment::Notch { offset, .. } => (JointType::Notch, *offset),
        _ => (JointType::None, 0.0),
    }
}

/// Gives the joint of `members` in frame `frame` treatment `t` with `gap`:
/// the pair's earlier Miter / Corner Joint / Notch go, the new one is added
/// and the frame solved. On an error nothing changes.
pub fn set_joint(
    project: &mut Project,
    linked: &Linked,
    asm: DocumentId,
    frame: u32,
    members: [u32; 2],
    t: JointType,
    gap: Real,
) -> Result<Solved, String> {
    let before = project.clone();
    let result = (|| {
        let [a, b] = members;
        let new = match t {
            JointType::None => None,
            JointType::Notch => Some(Treatment::Notch {
                notched: b,
                by: a,
                offset: gap,
            }),
            JointType::Miter | JointType::FirstThrough | JointType::SecondThrough => Corners {
                kind: match t {
                    JointType::Miter => CornerType::Miter,
                    JointType::FirstThrough => CornerType::ButtFirst,
                    _ => CornerType::ButtSecond,
                },
                gap,
                tees: TeeJoint::None,
            }
            .treatment(a, b, [0.0, 0.0]),
        };
        let f = asm_mut(project, asm)?
            .frames
            .iter_mut()
            .find(|f| f.id == frame)
            .ok_or("no such frame")?;
        for m in members {
            if !f.members.iter().any(|x| x.occurrence == m) {
                return Err("the joint's members are no longer in the frame".to_owned());
            }
        }
        f.treatments.retain(|x| !of_pair(x, members));
        f.treatments.extend(new);
        let s = solve(project, linked, asm)?;
        if let Some((_, why)) = s.sick.iter().find(|(o, _)| members.contains(o)) {
            return Err(why.clone());
        }
        Ok(s)
    })();
    if result.is_err() {
        *project = before;
    }
    result
}

/// The joints of frame document `asm` (for tests and callers holding only
/// the project).
pub fn joints_in(project: &Project, linked: &Linked, asm: DocumentId) -> Vec<Joint> {
    asm_of(project, asm).map_or_else(|_| Vec::new(), |a| joints(project, linked, a))
}
