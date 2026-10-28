//! Replace / Replace All (`docs/spec/3.0-assembly-commands.md` §3.1): the
//! planned panel's Components selector, *Replace All occurrences* and *New
//! Component* (Browse... opens the Place window to choose it); OK swaps the
//! components through `AssemblyDocument::replace` and solves (rule 5:
//! nothing changes when the solve fails).

use super::CadCraftApp;
use super::part_planned::PartDialog;
use crate::document::DocRef;

/// The panel's rows (`asm_planned::pages`).
const ALL: &str = "Replace All occurrences";
const NEW: &str = "New Component";

impl CadCraftApp {
    /// Browse...: the Place window chooses the new component.
    pub(super) fn replace_browse(&mut self) {
        self.asm.place_for_replace = true;
        self.open_place_dialog();
    }

    /// The Place window chose `r` (named `name`) for Replace.
    pub(super) fn replace_chosen(&mut self, r: DocRef, name: String) {
        self.asm.place_for_replace = false;
        let label = self.ref_name(r);
        self.asm.replace_with = Some((r, name));
        if let Some(d) = self.part_planned.as_mut() {
            d.set_text(0, NEW, &label);
        }
        self.file_status = Some(Ok(format!(
            "Replace: the new component is {label}; OK replaces"
        )));
    }

    /// OK: replaces the picked components (all occurrences of their parts
    /// with Replace All). Returns whether the panel can close.
    pub(super) fn replace_ok(&mut self, d: &PartDialog) -> bool {
        let Some(a) = self.assembly() else {
            return false;
        };
        let ids: Vec<u32> = d
            .picks_of("components")
            .iter()
            .filter_map(|w| {
                let name = w.split(" · ").next().unwrap_or(w);
                a.occurrences.iter().find(|o| o.name == name).map(|o| o.id)
            })
            .collect();
        let fail = |app: &mut Self, e: &str| {
            app.file_status = Some(Err(format!("Replace: {e}")));
            false
        };
        if ids.is_empty() {
            return fail(self, "select the components to replace");
        }
        // The text row shows the choice; the reference is kept aside.
        let Some((new, base)) = self
            .asm
            .replace_with
            .clone()
            .filter(|(r, _)| self.ref_name(*r) == d.text(0, NEW))
        else {
            return fail(self, "choose the new component (Browse...)");
        };
        let targets = a.replace_targets(&ids, d.check(0, ALL));
        // Try on a copy first: the errors of `replace` change nothing.
        let mut trial = a.clone();
        let n = match trial.replace(&targets, new, &base) {
            Ok(n) => n,
            Err(e) => return fail(self, &e),
        };
        let r = self.asm_try("Replace", |a| {
            let _ = a.replace(&targets, new, &base);
        });
        if let Err(e) = r {
            return fail(self, &e);
        }
        self.asm.replace_with = None;
        self.asm.sel.retain(|i| !targets.contains(i));
        self.asm.sub.clear();
        self.scene_dirty = true;
        let sick = self
            .cons
            .last
            .sick
            .iter()
            .filter(|(c, _)| {
                self.assembly()
                    .and_then(|a| a.constraint(*c))
                    .is_some_and(|c| targets.iter().any(|&t| c.uses(t)))
            })
            .count();
        self.file_status = Some(Ok(if sick == 0 {
            format!("Replace: {n} component(s) replaced")
        } else {
            format!(
                "Replace: {n} component(s) replaced; {sick} relationship(s) lost their geometry (sick)"
            )
        }));
        true
    }
}

/// The panel's choices shared by Copy and Mirror.
fn copy_options(d: &PartDialog, status: usize) -> Option<crate::assembly::copy::CopyOptions> {
    use crate::assembly::copy::{CopyOptions, CopyStatus};
    let status = match status {
        0 => CopyStatus::New,
        1 => CopyStatus::Reuse,
        _ => return None,
    };
    let text = |on: &str, t: &str| {
        if d.check(1, on) {
            d.text(1, t)
        } else {
            String::new()
        }
    };
    Some(CopyOptions {
        status,
        prefix: text("Prefix", "Prefix text"),
        suffix: text("Suffix", "Suffix text"),
        relationships: d.check(0, "Copy Relationships") || d.check(0, "Mirror Relationships"),
        ground: d.check(0, "Ground New Components"),
    })
}

impl CadCraftApp {
    /// The occurrences picked in the panel's Components selector.
    fn picked_components(&self, d: &PartDialog) -> Vec<u32> {
        let Some(a) = self.assembly() else {
            return Vec::new();
        };
        d.picks_of("components")
            .iter()
            .filter_map(|w| {
                let name = w.split(" · ").next().unwrap_or(w);
                a.occurrences.iter().find(|o| o.name == name).map(|o| o.id)
            })
            .collect()
    }

    /// Copy Components' OK. Returns whether the panel can close.
    pub(super) fn copy_ok(&mut self, d: &PartDialog) -> bool {
        let ids = self.picked_components(d);
        let Some(o) = copy_options(d, d.index(0, "Status of all")) else {
            self.file_status = Some(Ok("Copy: every component is excluded".into()));
            return true;
        };
        self.copy_run("Copy", d, move |p, asm| {
            crate::assembly::copy::copy_components(p, asm, &ids, &o)
        })
    }

    /// Mirror Components' OK. Returns whether the panel can close.
    pub(super) fn mirror_ok(&mut self, d: &PartDialog) -> bool {
        let ids = self.picked_components(d);
        let plane = match self.mirror_plane(d) {
            Ok(p) => p,
            Err(e) => {
                self.file_status = Some(Err(format!("Mirror: {e}")));
                return false;
            }
        };
        let Some(o) = copy_options(d, d.index(0, "Status of all")) else {
            self.file_status = Some(Ok("Mirror: every component is excluded".into()));
            return true;
        };
        self.copy_run("Mirror", d, move |p, asm| {
            crate::assembly::copy::mirror_components(p, asm, &ids, plane, &o)
        })
    }

    /// The Mirror Plane pick in assembly coordinates.
    fn mirror_plane(&self, d: &PartDialog) -> Result<crate::assembly::copy::MirrorPlane, String> {
        use crate::assembly::relation::{ASSEMBLY, Geom};
        let a = self.assembly().ok_or("no assembly")?;
        let r = d
            .refs
            .get("plane")
            .and_then(|v| v.first())
            .ok_or("select the mirror plane")?;
        let g = self.asm_local_in(a, r)?;
        let g = if r.occurrence() == ASSEMBLY {
            g
        } else {
            let o = a
                .find(r.occurrence())
                .ok_or("the component no longer exists")?;
            g.moved(&o.transform)
        };
        match g {
            Geom::Plane { p, n } => Ok(crate::assembly::copy::MirrorPlane {
                point: p,
                normal: n,
            }),
            _ => Err("the mirror plane must be a plane or a planar face".into()),
        }
    }

    /// Runs Copy / Mirror on the project as one Undo step `label`; the new
    /// occurrences are solved and selected.
    fn copy_run(
        &mut self,
        label: &str,
        _d: &PartDialog,
        f: impl FnOnce(
            &mut crate::document::Project,
            crate::document::DocumentId,
        ) -> Result<crate::assembly::copy::CopyResult, String>,
    ) -> bool {
        if self.picks_empty_guard(label) {
            return false;
        }
        self.history.break_coalescing();
        self.checkpoint(label);
        let asm = self.part_id;
        match f(&mut self.project, asm) {
            Ok(r) => {
                self.history.break_coalescing();
                self.asm_solve(None);
                self.asm.sel = r.occurrences.iter().map(|x| x.1).collect();
                self.asm.sub.clear();
                self.scene_dirty = true;
                let docs = r
                    .documents
                    .iter()
                    .filter_map(|id| self.project.document(*id).map(|d| d.name.clone()))
                    .collect::<Vec<_>>();
                self.file_status = Some(Ok(if docs.is_empty() {
                    format!("{label}: {} component(s) placed", r.occurrences.len())
                } else {
                    format!(
                        "{label}: {} component(s) placed; new part(s) {}",
                        r.occurrences.len(),
                        docs.join(", ")
                    )
                }));
                true
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("{label}: {e}")));
                false
            }
        }
    }

    /// Nothing to work on: no assembly active.
    fn picks_empty_guard(&mut self, label: &str) -> bool {
        if self.is_assembly() {
            return false;
        }
        self.file_status = Some(Err(format!("{label}: no assembly is active")));
        true
    }
}
