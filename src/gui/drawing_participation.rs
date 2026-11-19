//! Section participation in the GUI (`docs/spec/4.15-section-participation.md`):
//! each assembly section view's components with their settings, the
//! components left whole given to the view's lines job, and the menus /
//! Properties rows that change a view's own choice.

use std::sync::Arc;

use eframe::egui;

use super::CadCraftApp;
use crate::document::DocRef;
use crate::document::iprops::SectionParticipation as Sp;
use crate::drawing::participation::{self as dp, Leaf, Source, StandardSection};
use crate::drawing::{DrawingView, LinesJob, Sheet, ViewKind, ViewReps};

/// One part of an assembly view with what decides its participation.
#[derive(Clone, Debug)]
pub(super) struct SecLeaf {
    /// Path and settings.
    pub leaf: Leaf,
    /// The occurrences' names from the top (`Plate:1 / Bolt:2`).
    pub name: String,
}

/// A path written as text, case and separators normalised.
fn norm(p: &std::path::Path) -> String {
    let s = crate::document::library::without_verbatim(p.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase();
    s.trim_end_matches('/').to_string()
}

impl CadCraftApp {
    /// A standard part (spec 4.15 §3, category default: not cut): a member
    /// of the Component Library or a generated shaft.
    pub(super) fn is_standard_part(&self, c: DocRef) -> bool {
        if self
            .with_part(c, |p| p.generator.is_some())
            .unwrap_or(false)
        {
            return true;
        }
        let Some(lib) = c.library else {
            return false;
        };
        let (Some(path), Some(root)) = (self.library_path(lib), crate::components::root_dir())
        else {
            return false;
        };
        let (p, r) = (norm(&path), norm(&root));
        !r.is_empty() && p.starts_with(&format!("{r}/"))
    }

    /// The parts of assembly `model` with their participation settings
    /// (cached by the assembly's signature); `None`: not an assembly.
    pub(super) fn section_leaves(
        &mut self,
        model: DocRef,
        reps: ViewReps,
    ) -> Option<Arc<Vec<SecLeaf>>> {
        let sig = super::drawing::hash_str(&self.assembly_signature(model, reps)?);
        // The settings live in Properties and occurrences the signature does
        // not see: read again once a second.
        if let Some((s, at, l)) = self.drw.sec_leaves.get(&(model, reps))
            && *s == sig
            && at.elapsed().as_secs_f64() < 1.0
        {
            return Some(l.clone());
        }
        type Raw = (Vec<u32>, DocRef, Vec<Sp>, String);
        let raw: Vec<Raw> = self.drawing_leaves_with(model, reps, |l, asm| {
            let top = asm.find(*l.path.first()?)?;
            let mut occs = vec![l.occurrence.section];
            let mut name = top.name.clone();
            if l.path.len() > 1 {
                occs.push(top.section);
                name = format!("{name} / {}", l.occurrence.name);
            }
            Some((l.path.clone(), l.component, occs, name))
        })?;
        let mut kinds: std::collections::HashMap<DocRef, (bool, Sp)> = Default::default();
        let mut out = Vec::with_capacity(raw.len());
        for (path, c, occurrences, name) in raw {
            let (standard, part) = match kinds.get(&c) {
                Some(k) => *k,
                None => {
                    let k = (self.is_standard_part(c), self.bom_info(c).props.section);
                    kinds.insert(c, k);
                    k
                }
            };
            let Some((top, sub)) = path.split_first() else {
                continue;
            };
            out.push(SecLeaf {
                leaf: Leaf {
                    occurrence: *top,
                    sub: sub.to_vec(),
                    occurrences,
                    part,
                    standard,
                },
                name,
            });
        }
        let out = Arc::new(out);
        self.drw
            .sec_leaves
            .insert((model, reps), (sig, std::time::Instant::now(), out.clone()));
        Some(out)
    }

    /// Gives an assembly section view's lines job the components it leaves
    /// whole.
    pub(super) fn fill_participation(&mut self, sheet: &Sheet, id: u32, job: &mut LinesJob) {
        let LinesJob::Model {
            model,
            aligned: Some(a),
            reps,
            ..
        } = job
        else {
            return;
        };
        if a.cut.is_none() && a.outs.is_empty() {
            return;
        }
        let Some(v) = sheet.view(id) else {
            return;
        };
        let Some(leaves) = self.section_leaves(*model, *reps) else {
            return;
        };
        let ls: Vec<Leaf> = leaves.iter().map(|l| l.leaf.clone()).collect();
        a.uncut = dp::uncut(&v.participation, v.display.standard_section, &ls);
    }

    /// Sets the Section Participation of the component at (`occurrence`,
    /// `sub`) in view `view` of sheet `sheet` (one Undo step).
    pub(super) fn set_view_participation(
        &mut self,
        sheet: usize,
        view: u32,
        occurrence: u32,
        sub: Vec<u32>,
        value: Sp,
    ) {
        let label = format!("Section Participation: {}", value.label());
        self.edit_drawing(&label, move |d| {
            if let Some(s) = d.sheets.get_mut(sheet)
                && let Some(v) = s.view_mut(view)
            {
                dp::set_override(&mut v.participation, occurrence, &sub, value);
            }
        });
    }

    /// The browser rows of every assembly section view of the drawing: by
    /// (sheet, view), each component's leaf, row text and own setting.
    pub(super) fn participation_tree_rows(
        &mut self,
    ) -> std::collections::HashMap<(usize, u32), Vec<TreeComp>> {
        type Src = (
            usize,
            u32,
            DocRef,
            ViewReps,
            Vec<dp::ParticipationOverride>,
            StandardSection,
        );
        let src: Vec<Src> = self
            .drawing()
            .map(|d| {
                let mut out = Vec::new();
                for (i, s) in d.sheets.iter().enumerate() {
                    for v in &s.views {
                        if !matches!(v.kind, ViewKind::Section { .. }) {
                            continue;
                        }
                        if let Some(m) = d.model_of(s, v.id) {
                            out.push((
                                i,
                                v.id,
                                m,
                                d.reps_of(s, v.id),
                                v.participation.clone(),
                                v.display.standard_section,
                            ));
                        }
                    }
                }
                out
            })
            .unwrap_or_default();
        let mut out = std::collections::HashMap::new();
        for (i, id, m, r, ov, std) in src {
            let Some(leaves) = self.section_leaves(m, r) else {
                continue;
            };
            let rows = leaves
                .iter()
                .map(|l| {
                    let (cut, by) = dp::decide(&ov, std, &l.leaf);
                    TreeComp {
                        leaf: l.leaf.clone(),
                        text: row_text(&l.name, cut, by),
                        own: own_override(&ov, &l.leaf),
                        cut,
                    }
                })
                .collect();
            out.insert((i, id), rows);
        }
        out
    }

    /// The section view's Properties rows: Section Standard Parts and every
    /// component with its state; changes go into `nv`.
    pub(super) fn participation_rows(
        &mut self,
        ui: &mut egui::Ui,
        src: Option<(DocRef, ViewReps)>,
        nv: &mut DrawingView,
    ) {
        if !matches!(nv.kind, ViewKind::Section { .. }) {
            return;
        }
        let Some(leaves) = src.and_then(|(m, r)| self.section_leaves(m, r)) else {
            return;
        };
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Section Participation").strong());
        ui.horizontal(|ui| {
            super::panels::plabel(ui, "Standard Parts");
            egui::ComboBox::from_id_salt("drw_std_section_prop")
                .selected_text(nv.display.standard_section.label())
                .show_ui(ui, |ui| {
                    for s in StandardSection::ALL {
                        ui.selectable_value(&mut nv.display.standard_section, s, s.label());
                    }
                })
                .response
                .on_hover_text(
                    "Obey Browser: each component's own setting; Never / Always: library parts and generated shafts",
                );
        });
        let std = nv.display.standard_section;
        egui::ScrollArea::vertical()
            .id_salt("drw_participation")
            .max_height(220.0)
            .show(ui, |ui| {
                for l in leaves.iter() {
                    let (cut, src) = dp::decide(&nv.participation, std, &l.leaf);
                    let own = own_override(&nv.participation, &l.leaf);
                    ui.horizontal(|ui| {
                        let before = own.unwrap_or(Sp::Default);
                        let mut choice = before;
                        egui::ComboBox::from_id_salt((
                            "drw_part_row",
                            l.leaf.occurrence,
                            &l.leaf.sub,
                        ))
                        .width(78.0)
                        .selected_text(choice.label())
                        .show_ui(ui, |ui| {
                            for p in Sp::ALL {
                                ui.selectable_value(&mut choice, p, p.label());
                            }
                        });
                        if choice != before {
                            dp::set_override(
                                &mut nv.participation,
                                l.leaf.occurrence,
                                &l.leaf.sub,
                                choice,
                            );
                        }
                        ui.label(row_text(&l.name, cut, src));
                    });
                }
            });
    }
}

/// The view's own entry for exactly this component.
pub(super) fn own_override(ov: &[dp::ParticipationOverride], leaf: &Leaf) -> Option<Sp> {
    ov.iter()
        .find(|o| o.occurrence == leaf.occurrence && o.sub == leaf.sub)
        .map(|o| o.value)
}

/// A component's row text: its name and, when not cut or set by the view,
/// a tag saying so.
pub(super) fn row_text(name: &str, cut: bool, src: Source) -> String {
    let state = if cut { "" } else { " (not cut)" };
    let by = match src {
        Source::View => " (view)",
        Source::ViewOption => " (standard parts)",
        Source::Occurrence => " (assembly)",
        Source::Part => " (part)",
        Source::Category if !cut => " (standard part)",
        Source::Category => "",
    };
    format!("{name}{state}{by}")
}

/// One component row under a section view in the browser.
#[derive(Clone, Debug)]
pub(super) struct TreeComp {
    /// Its path.
    pub leaf: Leaf,
    /// Name and tags.
    pub text: String,
    /// The view's own setting.
    pub own: Option<Sp>,
    /// Cut by the section.
    pub cut: bool,
}

/// The Section Participation submenu (`own`: the view's current choice);
/// the choice clicked.
pub(super) fn participation_menu(ui: &mut egui::Ui, own: Option<Sp>) -> Option<Sp> {
    let mut picked = None;
    ui.menu_button("Section Participation", |ui| {
        for p in Sp::ALL {
            let text = match p {
                Sp::Section => "Section (cut)",
                Sp::None => "None (not cut)",
                Sp::Default => "Default",
            };
            if ui
                .selectable_label(own.unwrap_or(Sp::Default) == p, text)
                .clicked()
            {
                picked = Some(p);
                ui.close();
            }
        }
    });
    picked
}
