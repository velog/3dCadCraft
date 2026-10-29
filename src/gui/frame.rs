//! Insert Frame's logic in the GUI (`docs/spec/6.1-frame-generator.md`,
//! stage F1): the panel is the planned one (`gui/asm_planned.rs`); its
//! Standard / Family / Size lists come from the section table
//! (`part::frame::Library`), OK inserts one member per picked edge
//! (`assembly::frame::insert`), and the members follow their skeleton when
//! the assembly is shown again (`follow_frames`).

use super::CadCraftApp;
use super::asm_planned::AsmPlanned;
use super::part_planned::{PartDialog, Planned};
use crate::assembly::frame::{self, Anchor, InsertOptions, Orientation};
use crate::part::frame::Library;

/// Is `d` a frame panel whose lists come from the section table?
fn is_frame(d: &PartDialog) -> bool {
    matches!(
        d.kind,
        Planned::Asm(AsmPlanned::InsertFrame | AsmPlanned::FrameChange)
    )
}

/// The choices of drop-down `label` (tab `ti`) of a frame panel, from the
/// section table; `None` for any other list.
pub(super) fn frame_combo(d: &PartDialog, ti: usize, label: &str) -> Option<Vec<String>> {
    if !is_frame(d) {
        return None;
    }
    let lib = Library::builtin();
    let own = |v: Vec<&str>| v.into_iter().map(str::to_owned).collect::<Vec<_>>();
    match label {
        "Standard" => Some(own(lib.standards())),
        "Family" => Some(own(lib.families(&frame_choice(d, ti, "Standard")?))),
        "Size" => Some(own(lib.sizes(&frame_choice(d, ti, "Family")?))),
        _ => None,
    }
}

/// The chosen entry of list `label` (clamped to the list).
pub(super) fn frame_choice(d: &PartDialog, ti: usize, label: &str) -> Option<String> {
    let list = frame_combo(d, ti, label)?;
    let i = d.index(ti, label).min(list.len().checked_sub(1)?);
    list.into_iter().nth(i)
}

/// The Orientation group of the panel.
fn orientation(d: &PartDialog) -> Orientation {
    let deg = |l: &str| d.num(0, l).unwrap_or(0.0);
    Orientation {
        anchor: Anchor::ALL
            .get(d.index(0, "Anchor"))
            .copied()
            .unwrap_or_default(),
        h_offset: deg("Horizontal Offset"),
        v_offset: deg("Vertical Offset"),
        angle: deg("Rotation Angle").to_radians(),
        about_anchor: d.check(0, "Rotate Around Selected Point"),
        mirror: d.check(0, "Mirror"),
        flip: d.check(0, "Flip Direction"),
    }
}

impl CadCraftApp {
    /// OK of Insert Frame: a member on every picked edge. False (with the
    /// reason in the status bar) leaves everything as it was.
    pub(super) fn frame_ok(&mut self, d: &PartDialog) -> bool {
        let fail = |app: &mut Self, e: &str| {
            app.file_status = Some(Err(format!("Insert Frame: {e}")));
            false
        };
        if d.index(0, "Placement") != 0 {
            return fail(self, "Between Points comes with stage F2 (spec 6.1)");
        }
        if d.refs.get("align").is_some_and(|v| !v.is_empty()) {
            return fail(self, "Align comes with stage F2 (spec 6.1)");
        }
        let edges = d.refs.get("edges").cloned().unwrap_or_default();
        if edges.is_empty() {
            return fail(self, "select the skeleton edges");
        }
        let (Some(family), Some(size)) = (frame_choice(d, 0, "Family"), frame_choice(d, 0, "Size"))
        else {
            return fail(self, "the section table is empty");
        };
        let material = super::asm_planned::choice(d, 0, "Material").map(str::to_owned);
        let o = InsertOptions {
            family,
            size,
            material,
            orientation: orientation(d),
        };
        let mut trial = self.project.clone();
        match frame::insert(
            &mut trial,
            self.part_id,
            &edges,
            &o,
            Library::builtin(),
            None,
        ) {
            Ok((_, occs)) => {
                self.history.break_coalescing();
                self.checkpoint("Insert Frame");
                self.project = trial;
                self.history.break_coalescing();
                self.asm.sel = occs.clone();
                self.asm.sub.clear();
                self.scene_dirty = true;
                self.file_status = Some(Ok(format!(
                    "Insert Frame: {} member(s) of {} {}",
                    occs.len(),
                    o.family,
                    o.size
                )));
                true
            }
            Err(e) => fail(self, &e),
        }
    }

    /// Members follow their skeleton (after the skeleton part was edited);
    /// members that cannot are reported in the log and keep their state.
    pub(super) fn follow_frames(&mut self) {
        if !self.assembly().is_some_and(|a| !a.frames.is_empty()) {
            return;
        }
        match frame::solve(&mut self.project, self.part_id) {
            Ok(s) => {
                for (occ, why) in &s.sick {
                    log::warn!("frame member {occ}: {why}");
                }
                if s.moved > 0 {
                    self.scene_dirty = true;
                }
                if let Some((_, why)) = s.sick.first() {
                    self.file_status = Some(Err(format!(
                        "Frame: {} member(s) cannot follow the skeleton ({why})",
                        s.sick.len()
                    )));
                }
            }
            Err(e) => log::warn!("{e}"),
        }
    }
}
