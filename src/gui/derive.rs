//! Derive (spec 3.10 §3.5, T5) in the app: the Derive panel's Browse... (the
//! Place window chooses the source part: a part of this file or of a part
//! file), its OK (`PartDocument::add_derive`), and Update of derived parts
//! when they are shown (`PartDocument::update_derive`).

use super::CadCraftApp;
use super::part_planned::PartDialog;
use crate::document::DocRef;
use crate::kernel::Transform3;
use crate::part::PartDocument;
use crate::part::derive::{DeriveLink, DeriveSource, DeriveStyle, MirrorPlane};

/// How deep chains of derived parts are followed (A from B from C …).
const CHAIN: usize = 8;

impl CadCraftApp {
    /// The parts source `r` shows, built and placed: a part at the
    /// identity, or every part of an assembly where it stands. A part that
    /// is derived itself follows its own sources first (chains, at most
    /// `CHAIN` deep).
    fn derive_parts(&self, r: DocRef, depth: usize) -> Option<Vec<(PartDocument, Transform3)>> {
        let linked = self.linked();
        if let Some(a) = crate::assembly::tree::assembly_of(&self.project, &linked, r) {
            let mut out = Vec::new();
            for l in crate::assembly::tree::leaves(&self.project, &linked, r, a) {
                let p = self.derive_part(l.component, depth)?;
                out.push((p, l.world));
            }
            return Some(out);
        }
        Some(vec![(self.derive_part(r, depth)?, Transform3::identity())])
    }

    /// A copy of part `r`, built, its own derived features following their
    /// sources.
    fn derive_part(&self, r: DocRef, depth: usize) -> Option<PartDocument> {
        let mut p = self.with_part(r, Clone::clone)?;
        if !p.exact || p.exact_bodies.0.is_empty() {
            let _ = p.regenerate();
        }
        if depth < CHAIN {
            let derived: Vec<(usize, DocRef)> = p
                .derived()
                .into_iter()
                .map(|(k, l)| (k, l.source))
                .collect();
            for (k, s) in derived {
                if let Some(parts) = self.derive_parts(s, depth + 1) {
                    let src = DeriveSource::of(parts.iter().map(|(q, t)| (q, *t)).collect());
                    let _ = p.update_derive_from(k, &src);
                }
            }
        }
        Some(p)
    }

    /// Whether source `r` reaches the active part through its derived
    /// features (directly or through others): deriving it would loop.
    pub(super) fn derive_loops(&self, r: DocRef, depth: usize) -> bool {
        let me = DocRef::local(self.part_id);
        if r == me {
            return true;
        }
        if depth >= CHAIN {
            return false;
        }
        let linked = self.linked();
        let parts: Vec<DocRef> = match crate::assembly::tree::assembly_of(&self.project, &linked, r)
        {
            Some(a) => crate::assembly::tree::leaves(&self.project, &linked, r, a)
                .into_iter()
                .map(|l| l.component)
                .collect(),
            None => vec![r],
        };
        parts.into_iter().any(|p| {
            p == me
                || self
                    .with_part(p, |x| {
                        x.derived()
                            .iter()
                            .map(|(_, l)| l.source)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
                    .into_iter()
                    .any(|s| self.derive_loops(s, depth + 1))
        })
    }

    /// Browse...: the Place window chooses the source part.
    pub(super) fn derive_browse(&mut self) {
        self.asm.place_for_derive = true;
        self.open_place_dialog();
    }

    /// The Place window chose `r` (named `name`) as the source.
    pub(super) fn derive_chosen(&mut self, r: DocRef, name: String) {
        self.asm.place_for_derive = false;
        if self.derive_loops(r, 0) {
            self.file_status = Some(Err(
                "Derive: that source is or contains this part, or is derived from it (a loop)"
                    .into(),
            ));
            return;
        }
        let label = self.ref_name(r);
        self.asm.derive_source = Some((r, name));
        if let Some(d) = self.part_planned.as_mut() {
            d.set_text(0, "Source", &label);
        }
        self.file_status = Some(Ok(format!("Derive: the source is {label}; OK derives it")));
    }

    /// Edit Feature of derived feature `k`: the Derive panel with its
    /// options.
    pub(super) fn derive_edit(&mut self, k: usize, l: DeriveLink) {
        self.start_part_planned(crate::gui::part_planned::PartPlanned::Derive);
        let label = self.ref_name(l.source);
        let Some(d) = self.part_planned.as_mut() else {
            return;
        };
        d.feature = Some(k);
        d.set_text(0, "Source", &label);
        d.set_index(
            0,
            "Derive Style",
            match l.style {
                DeriveStyle::SingleMerged => 0,
                DeriveStyle::Single => 1,
                DeriveStyle::Each => 2,
                DeriveStyle::Surfaces => 3,
            },
        );
        d.set_check(0, "Solid Bodies", l.solids);
        d.set_check(0, "Surface Bodies", l.surfaces);
        d.set_num(1, "Scale Factor", l.scale);
        d.set_check(1, "Mirror Part", l.mirror.is_some());
        d.set_index(
            1,
            "Mirror Plane",
            match l.mirror {
                Some(MirrorPlane::Xy) | None => 0,
                Some(MirrorPlane::Xz) => 1,
                Some(MirrorPlane::Yz) => 2,
            },
        );
        self.asm.derive_source = Some((l.source, l.name.clone()));
        self.asm.derive_exclude = l.exclude.clone();
        self.file_status = Some(Ok(format!(
            "Derive: {} (Browse... chooses another source)",
            l.name
        )));
    }

    /// OK: the chosen source's bodies become this part's base feature.
    /// Returns whether the panel can close.
    pub(super) fn derive_ok(&mut self, d: &PartDialog) -> bool {
        let Some((r, name)) = self.asm.derive_source.clone() else {
            self.file_status = Some(Err("Derive: choose the source part (Browse...)".into()));
            return false;
        };
        let Some(parts) = self.derive_parts(r, 0) else {
            self.file_status = Some(Err(format!("Derive: {name} was not found")));
            return false;
        };
        let source = DeriveSource::of(parts.iter().map(|(p, t)| (p, *t)).collect());
        let mut link = DeriveLink::new(r, name);
        link.style = match d.index(0, "Derive Style") {
            0 => DeriveStyle::SingleMerged,
            1 => DeriveStyle::Single,
            _ => DeriveStyle::Each,
        };
        link.solids = d.check(0, "Solid Bodies");
        link.surfaces = d.check(0, "Surface Bodies");
        link.parameters = d.check(0, "Parameters");
        link.scale = d.num(1, "Scale Factor").unwrap_or(1.0);
        if d.check(1, "Mirror Part") {
            link.mirror = Some(match d.index(1, "Mirror Plane") {
                0 => MirrorPlane::Xy,
                1 => MirrorPlane::Xz,
                _ => MirrorPlane::Yz,
            });
        }
        // Edit Feature: the same feature with the new options.
        if let Some(k) = d.feature {
            link.exclude = std::mem::take(&mut self.asm.derive_exclude);
            self.checkpoint("Edit Derive");
            return match self.part_mut().edit_derive(k, &source, link) {
                Ok(()) => {
                    self.asm.derive_source = None;
                    self.scene_dirty = true;
                    self.file_status = Some(Ok("Derive: edited".into()));
                    true
                }
                Err(e) => {
                    self.history.cancel();
                    self.file_status = Some(Err(e.to_string()));
                    false
                }
            };
        }
        self.checkpoint("Derive");
        match self.part_mut().add_derive_from(&source, link) {
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
            let Some(parts) = self.derive_parts(r, 0) else {
                continue;
            };
            let sig = DeriveSource::of(parts.iter().map(|(p, t)| (p, *t)).collect()).signature;
            if self
                .part()
                .derived()
                .iter()
                .any(|(j, l)| *j == k && l.signature != sig)
            {
                stale.push((k, parts));
            }
        }
        if stale.is_empty() {
            return;
        }
        self.checkpoint("Update");
        let mut errors = Vec::new();
        for (k, parts) in stale {
            let src = DeriveSource::of(parts.iter().map(|(p, t)| (p, *t)).collect());
            if let Err(e) = self.part_mut().update_derive_from(k, &src) {
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

    /// Break Link of derived feature `k` (browser): it keeps its geometry
    /// and no longer follows its source.
    pub(super) fn break_derive_link(&mut self, k: usize) {
        if !self.part().derived().iter().any(|(j, _)| *j == k) {
            return;
        }
        self.checkpoint("Break Link");
        self.part_mut().break_derive(k);
        self.history.break_coalescing();
        self.file_status = Some(Ok(
            "Break Link: the derived bodies stay and no longer follow their source".into(),
        ));
    }
}
