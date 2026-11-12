//! Create In-Place Component's OK (`docs/spec/3.10-top-down.md` §3.1, step
//! T1): the panel's values → `assembly::create`, one Undo step, then the new
//! part is edited in place for its first sketch (Return comes back). Store:
//! Separate file (the default, Inventor's model) writes it as a file of its
//! own next to the assembly; In this file keeps it in the design file.

use super::CadCraftApp;
use super::part_planned::PartDialog;
use crate::assembly::create::{
    CreateOptions, Template, create_as_file, create_in_place, new_file_path,
};
use crate::assembly::relation::{ASSEMBLY, Geom};
use crate::document::iprops::BomStructure;

impl CadCraftApp {
    /// Create's OK. Returns whether the panel can close.
    pub(super) fn create_ok(&mut self, d: &PartDialog) -> bool {
        if d.check(0, "Virtual Component") {
            self.file_status = Some(Err(
                "Create: virtual components come later (spec 3.10 §3.1)".into(),
            ));
            return false;
        }
        let template = match d.index(0, "Template") {
            1 => Template::SheetMetal,
            2 => Template::Assembly,
            _ => Template::Part,
        };
        let bom = BomStructure::ALL
            .get(d.index(0, "Default BOM Structure"))
            .copied()
            .unwrap_or_default();
        let on = if d.check(0, "Constrain sketch plane to selected face or plane") {
            match self.create_plane(d) {
                Ok(p) => p,
                Err(e) => {
                    self.file_status = Some(Err(format!("Create: {e}")));
                    return false;
                }
            }
        } else {
            None
        };
        let o = CreateOptions {
            name: d.text(0, "New Component Name"),
            template,
            bom,
            on,
        };
        let mut trial = self.project.clone();
        let asm = self.part_id;
        // Inventor's model: the new component is a file of its own next to
        // the assembly (Store: In this file keeps it in the design file).
        if d.index(0, "Store") == 0 {
            return self.create_file_ok(d, &o);
        }
        match create_in_place(&mut trial, asm, &o) {
            Ok(c) => {
                self.history.break_coalescing();
                self.checkpoint("Create Component");
                self.project = trial;
                self.history.break_coalescing();
                self.asm_solve(None);
                self.asm.sel = vec![c.occurrence];
                self.asm.sub.clear();
                self.scene_dirty = true;
                let name = self
                    .project
                    .document(c.document)
                    .map(|x| x.name.clone())
                    .unwrap_or_default();
                self.file_status = Some(Ok(format!(
                    "Create: {name} placed; sketch its first feature, then Return"
                )));
                if template != Template::Assembly {
                    self.edit_in_place(c.occurrence, Vec::new());
                }
                true
            }
            Err(e) => {
                self.file_status = Some(Err(format!("Create: {e}")));
                false
            }
        }
    }

    /// Create with Store: Separate file: the new part file is written next
    /// to the saved assembly (or in New File Location) and linked; it opens
    /// in its own tab for Edit in place, the assembly's tab stays.
    fn create_file_ok(&mut self, d: &PartDialog, o: &CreateOptions) -> bool {
        let owner = std::path::PathBuf::from(self.file_path.trim());
        if self.file_path.trim().is_empty() {
            self.file_status = Some(Err(
                "Create: save the assembly first: the new part file goes next to it (or choose Store: In this file)"
                    .into(),
            ));
            return false;
        }
        let loc = d.text(0, "New File Location");
        let loc = loc.trim();
        let folder =
            (!loc.is_empty() && !loc.starts_with('(')).then(|| std::path::PathBuf::from(loc));
        let path = match new_file_path(&owner, folder.as_deref(), &o.name, o.template) {
            Ok(p) => p,
            Err(e) => {
                self.file_status = Some(Err(format!("Create: {e}")));
                return false;
            }
        };
        let mut trial = self.project.clone();
        match create_as_file(&mut trial, self.part_id, &owner, &path, o) {
            Ok(made) => {
                self.history.break_coalescing();
                self.checkpoint("Create Component");
                self.project = trial;
                self.history.break_coalescing();
                self.asm.libs.forget_linked();
                self.asm_solve(None);
                self.asm.sel = vec![made.created.occurrence];
                self.asm.sub.clear();
                self.scene_dirty = true;
                self.file_status = Some(Ok(format!(
                    "Create: {} written and placed; sketch its first feature, then Return",
                    path.display()
                )));
                if o.template != Template::Assembly {
                    self.edit_in_place(made.created.occurrence, Vec::new());
                }
                true
            }
            Err(e) => {
                self.file_status = Some(Err(format!("Create: {e}")));
                false
            }
        }
    }

    /// The Sketch Plane pick and its plane in assembly coordinates (`None`:
    /// nothing picked, the component goes to the origin).
    fn create_plane(
        &self,
        d: &PartDialog,
    ) -> Result<
        Option<(
            crate::assembly::GeomRef,
            crate::kernel::Point3,
            crate::kernel::Vector3,
        )>,
        String,
    > {
        let a = self.assembly().ok_or("no assembly")?;
        let Some(r) = d.refs.get("plane").and_then(|v| v.first()) else {
            return Ok(None);
        };
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
            Geom::Plane { p, n } => Ok(Some((r.clone(), p, n))),
            _ => Err("the sketch plane must be a planar face or a plane".into()),
        }
    }
}
