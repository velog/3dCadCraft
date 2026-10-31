//! Make Part and Make Components (spec 3.10 §3.6, T6) in the app: the
//! planned panels' OK (`assembly::make`), and the parts made from a master
//! following it when a part or an assembly of the file is shown.

use super::CadCraftApp;
use super::part_planned::PartDialog;
use super::select::SelItem;
use crate::assembly::make::{self, Target};
use crate::document::DocumentData;

impl CadCraftApp {
    /// The bodies the panel's Bodies selector holds (a face stands for its
    /// body), each once.
    fn make_bodies(&self, d: &PartDialog) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (item, at) in d.items_of("bodies") {
            let name = match item {
                SelItem::Body(b) => Some(b.clone()),
                SelItem::Face(f) => self.body_of_face(f, *at),
                _ => None,
            };
            if let Some(n) = name
                && !out.contains(&n)
            {
                out.push(n);
            }
        }
        out
    }

    /// Make Part's OK: a new part of this file deriving the chosen bodies.
    pub(super) fn make_part_ok(&mut self, d: &PartDialog) -> bool {
        let bodies = self.make_bodies(d);
        let name = d.text(0, "New part name").trim().to_owned();
        let name = if name.is_empty() {
            "Part".to_owned()
        } else {
            name
        };
        let master = self.part_id;
        self.checkpoint("Make Part");
        match make::make_part(&mut self.project, master, &bodies, &name) {
            Ok(_) => {
                self.file_status = Some(Ok(format!(
                    "Make Part: {name} made from {} bod{} (it follows this part)",
                    bodies.len(),
                    if bodies.len() == 1 { "y" } else { "ies" }
                )));
                true
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("Make Part: {e}")));
                false
            }
        }
    }

    /// Make Components' OK: each chosen body a part, placed in the target
    /// assembly (an existing one of that name, else a new one).
    pub(super) fn make_components_ok(&mut self, d: &PartDialog) -> bool {
        let bodies = self.make_bodies(d);
        let target = if d.check(0, "Insert components in target assembly") {
            let name = d.text(0, "Target Assembly").trim().to_owned();
            let name = if name.is_empty() {
                "Assembly".to_owned()
            } else {
                name
            };
            match self
                .project
                .documents()
                .iter()
                .find(|x| x.name == name && matches!(x.data, DocumentData::Assembly(_)))
            {
                Some(a) => Target::Existing(a.id),
                None => Target::New(name),
            }
        } else {
            Target::None
        };
        let master = self.part_id;
        self.checkpoint("Make Components");
        match make::make_components(&mut self.project, master, &bodies, &[], target) {
            Ok(m) => {
                let into = m
                    .assembly
                    .and_then(|a| self.project.document(a))
                    .map(|a| format!(", placed in {}", a.name))
                    .unwrap_or_default();
                self.file_status = Some(Ok(format!(
                    "Make Components: {} part(s) made{into} (they follow this part)",
                    m.parts.len()
                )));
                true
            }
            Err(e) => {
                self.history.cancel();
                self.file_status = Some(Err(format!("Make Components: {e}")));
                false
            }
        }
    }

    /// Parts of this file made from other parts of it follow them (a
    /// master changed) when a document is shown; one Undo step.
    pub(super) fn make_follow(&mut self) {
        let any = self.project.documents().iter().any(|d| {
            matches!(&d.data, DocumentData::Part(p) if p.derived().iter().any(|(_, l)| l.source.library.is_none())
                || p.user_params.iter().any(|u| u.link.is_some()))
        });
        if !any {
            return;
        }
        let before = self.project.clone();
        let (changed, errors) = make::update_all(&mut self.project);
        if changed.is_empty() && errors.is_empty() {
            return;
        }
        // The Undo step holds the state before the update.
        let after = std::mem::replace(&mut self.project, before);
        self.checkpoint("Update");
        self.project = after;
        self.scene_dirty = true;
        if !errors.is_empty() {
            self.file_status = Some(Err(format!("Update: {}", errors.join("; "))));
        }
    }
}
