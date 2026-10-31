//! Derive (spec 3.10 §3.5, T5) in the app: the Derive panel's Browse... (the
//! Place window chooses the source part: a part of this file or of a part
//! file), its OK (`PartDocument::add_derive`), and Update of derived parts
//! when they are shown (`PartDocument::update_derive`).

use super::CadCraftApp;
use super::part_planned::PartDialog;
use crate::document::DocRef;
use crate::part::derive::{DeriveLink, DeriveStyle, MirrorPlane};

impl CadCraftApp {
    /// A copy of source part `r`, built (a part of this file is stored
    /// without its bodies until it is shown).
    fn derive_source_part(&self, r: DocRef) -> Option<crate::part::PartDocument> {
        let mut p = self.with_part(r, Clone::clone)?;
        if !p.exact || p.exact_bodies.0.is_empty() {
            let _ = p.regenerate();
        }
        Some(p)
    }

    /// Browse...: the Place window chooses the source part.
    pub(super) fn derive_browse(&mut self) {
        self.asm.place_for_derive = true;
        self.open_place_dialog();
    }

    /// The Place window chose `r` (named `name`) as the source.
    pub(super) fn derive_chosen(&mut self, r: DocRef, name: String) {
        self.asm.place_for_derive = false;
        if r == DocRef::local(self.part_id) {
            self.file_status = Some(Err("Derive: a part cannot derive itself".into()));
            return;
        }
        let label = self.ref_name(r);
        self.asm.derive_source = Some((r, name));
        if let Some(d) = self.part_planned.as_mut() {
            d.set_text(0, "Source", &label);
        }
        self.file_status = Some(Ok(format!("Derive: the source is {label}; OK derives it")));
    }

    /// OK: the chosen source's bodies become this part's base feature.
    /// Returns whether the panel can close.
    pub(super) fn derive_ok(&mut self, d: &PartDialog) -> bool {
        let Some((r, name)) = self.asm.derive_source.clone() else {
            self.file_status = Some(Err("Derive: choose the source part (Browse...)".into()));
            return false;
        };
        let Some(source) = self.derive_source_part(r) else {
            self.file_status = Some(Err(format!("Derive: {name} was not found")));
            return false;
        };
        let mut link = DeriveLink::new(r, name);
        link.style = match d.index(0, "Derive Style") {
            0 => DeriveStyle::SingleMerged,
            1 => DeriveStyle::Single,
            _ => DeriveStyle::Each,
        };
        link.solids = d.check(0, "Solid Bodies");
        link.surfaces = d.check(0, "Surface Bodies");
        link.scale = d.num(1, "Scale Factor").unwrap_or(1.0);
        if d.check(1, "Mirror Part") {
            link.mirror = Some(match d.index(1, "Mirror Plane") {
                0 => MirrorPlane::Xy,
                1 => MirrorPlane::Xz,
                _ => MirrorPlane::Yz,
            });
        }
        self.checkpoint("Derive");
        match self.part_mut().add_derive(&source, link) {
            Ok(id) => {
                self.asm.derive_source = None;
                self.scene_dirty = true;
                self.file_status = Some(Ok(format!("Derive: {id} added")));
                true
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(e.to_string()));
                false
            }
        }
    }

    /// Update of the active part's derived features whose sources changed
    /// (when the part is shown): one Undo step, failures in the status bar
    /// (the last copy stays, rule 5).
    pub(super) fn derive_follow(&mut self) {
        if self.is_assembly() || self.is_drawing() {
            return;
        }
        let derived: Vec<(usize, DocRef)> = self
            .part()
            .derived()
            .into_iter()
            .map(|(k, l)| (k, l.source))
            .collect();
        let mut stale = Vec::new();
        for (k, r) in derived {
            let Some(sig) = self.with_part(r, crate::part::derive::signature) else {
                continue;
            };
            if self
                .part()
                .derived()
                .iter()
                .any(|(j, l)| *j == k && l.signature != sig)
                && let Some(src) = self.derive_source_part(r)
            {
                stale.push((k, src));
            }
        }
        if stale.is_empty() {
            return;
        }
        self.checkpoint("Update");
        let mut errors = Vec::new();
        for (k, src) in stale {
            if let Err(e) = self.part_mut().update_derive(k, &src) {
                errors.push(e.to_string());
            }
        }
        self.scene_dirty = true;
        self.file_status = Some(if errors.is_empty() {
            Ok("Update: the derived bodies follow their sources".into())
        } else {
            Err(errors.join("; "))
        });
    }
}
