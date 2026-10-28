//! Pattern Component in the assembly (`docs/spec/3.0-assembly-commands.md`
//! §3.7): the planned panel's Rectangular and Circular tabs become an
//! [`assembly::pattern::Layout`](crate::assembly::pattern::Layout); the copies
//! show see-through while the panel is open, OK adds the pattern (one Undo
//! step), and the browser lists it with its elements (Edit, Delete).

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::asm_planned::AsmPlanned;
use super::icons::Icon;
use super::part_planned::{PartDialog, Planned};
use crate::assembly::pattern::{self, ComponentPattern, Layout};
use crate::assembly::relation::{self, Geom, GeomRef};
use crate::document::AssemblyDocument;
use crate::kernel::Transform3;

/// Tabs of the panel.
const ASSOCIATIVE: usize = 0;
const RECTANGULAR: usize = 1;
const CIRCULAR: usize = 2;

/// What the browser asked for.
pub(super) enum PatternCmd {
    Select(u32),
    SelectElement(u32, u32),
    SelectOccurrence(u32),
    Edit(u32),
    Delete(u32),
}

impl CadCraftApp {
    /// A reference's geometry in its part, looked up in assembly `a`.
    pub(super) fn asm_local_in(&self, a: &AssemblyDocument, r: &GeomRef) -> Result<Geom, String> {
        if let GeomRef::Origin {
            occurrence: relation::ASSEMBLY,
            item,
        } = r
        {
            return Ok(item.geom());
        }
        let o = a
            .find(r.occurrence())
            .ok_or("the component no longer exists")?;
        let name = o.name.clone();
        self.with_part(o.component, |p| relation::resolve_in(p, r))
            .ok_or_else(|| format!("{name}: its part file was not found"))?
            .map_err(|e| format!("{name}: {e}"))
    }

    /// Places the copies of every pattern of `a` from their sources.
    pub(super) fn place_patterns(&self, a: &mut AssemblyDocument) -> Result<(), String> {
        if a.patterns.is_empty() {
            return Ok(());
        }
        let snapshot = a.clone();
        pattern::place(a, &|r| self.asm_local_in(&snapshot, r))
    }

    /// The sources and layout the panel describes.
    fn pattern_layout(&self, d: &PartDialog) -> Result<(Vec<u32>, Layout), String> {
        let a = self.assembly().ok_or("no assembly")?;
        let sources: Vec<u32> = d
            .picks_of("components")
            .iter()
            .filter_map(|w| {
                let name = w.split(" · ").next().unwrap_or(w);
                a.occurrences.iter().find(|o| o.name == name).map(|o| o.id)
            })
            .collect();
        if sources.is_empty() {
            return Err("select the components to pattern".into());
        }
        let r = |key: &str| d.refs.get(key).and_then(|v| v.first()).cloned();
        let count = |v: Option<f64>| v.unwrap_or(1.0).round().max(1.0) as u32;
        let layout = match d.tab() {
            ASSOCIATIVE => {
                return Err(
                    "the Associative tab (following a feature pattern) comes later; use Rectangular or Circular"
                        .into(),
                );
            }
            RECTANGULAR => Layout::Rectangular {
                column: r("column").ok_or("select the column direction")?,
                column_flip: d.check(RECTANGULAR, "Flip column"),
                column_count: count(d.num(RECTANGULAR, "Column Count")),
                column_spacing: d.num(RECTANGULAR, "Column Spacing").unwrap_or(0.0),
                row: r("row"),
                row_flip: d.check(RECTANGULAR, "Flip row"),
                row_count: count(d.num(RECTANGULAR, "Row Count")),
                row_spacing: d.num(RECTANGULAR, "Row Spacing").unwrap_or(0.0),
            },
            CIRCULAR => Layout::Circular {
                axis: r("axis").ok_or("select the rotation axis")?,
                flip: d.check(CIRCULAR, "Flip"),
                count: count(d.num(CIRCULAR, "Count")),
                angle: d.num(CIRCULAR, "Angle").unwrap_or(0.0).to_radians(),
            },
            _ => return Err("no such tab".into()),
        };
        if layout.count() < 2 {
            return Err("the count makes no copies".into());
        }
        if layout.count() > 1000 {
            return Err(format!("{} elements are more than 1000", layout.count()));
        }
        Ok((sources, layout))
    }

    /// OK of Pattern Component: adds (or changes) the pattern; false with
    /// the reason in the status bar when the panel is not complete.
    pub(super) fn pattern_ok(&mut self, d: &PartDialog) -> bool {
        let fail = |app: &mut Self, e: String| {
            app.file_status = Some(Err(format!("Pattern Component: {e}")));
            false
        };
        let (sources, layout) = match self.pattern_layout(d) {
            Ok(v) => v,
            Err(e) => return fail(self, e),
        };
        let Some(mut a) = self.assembly().cloned() else {
            return false;
        };
        let snapshot = a.clone();
        let local = |r: &GeomRef| self.asm_local_in(&snapshot, r);
        let done = match d.edit {
            Some(id) => match a.patterns.iter_mut().find(|p| p.id == id) {
                Some(p) => {
                    p.layout = layout;
                    p.sources = sources;
                    pattern::place(&mut a, &local).map(|_| id)
                }
                None => Err("the pattern no longer exists".into()),
            },
            None => a.add_pattern(sources, layout, &local),
        };
        let id = match done {
            Ok(id) => id,
            Err(e) => return fail(self, e),
        };
        let label = if d.edit.is_some() {
            "Edit Component Pattern"
        } else {
            "Pattern Component"
        };
        self.checkpoint(label);
        let (name, n) = a
            .patterns
            .iter()
            .find(|p| p.id == id)
            .map_or((String::new(), 0), |p| {
                (p.name.clone(), p.elements.len() + 1)
            });
        if let Some(cur) = self.assembly_mut() {
            *cur = a;
        }
        self.asm.pattern_preview = None;
        self.scene_dirty = true;
        self.file_status = Some(Ok(format!("{name}: {n} elements")));
        true
    }

    /// The copies the open Pattern panel would make (source, motion), for
    /// the see-through preview; none while the panel is incomplete.
    pub(super) fn pattern_preview_items(&self) -> Vec<(u32, Transform3)> {
        let Some(d) = self
            .part_planned
            .as_ref()
            .filter(|d| d.kind == Planned::Asm(AsmPlanned::Pattern))
        else {
            return Vec::new();
        };
        let Some(a) = self.assembly() else {
            return Vec::new();
        };
        let Ok((sources, layout)) = self.pattern_layout(d) else {
            return Vec::new();
        };
        let at = |id: u32| a.find(id).map(|o| o.transform);
        let Ok(moves) = pattern::motions(&layout, &|r| self.asm_local_in(a, r), &at) else {
            return Vec::new();
        };
        // Editing: the pattern's own copies are already drawn.
        let skip = d.edit.map_or(0, |id| {
            a.patterns
                .iter()
                .find(|p| p.id == id)
                .map_or(0, |p| p.elements.len())
        });
        moves
            .iter()
            .skip(skip)
            .flat_map(|&(_, m)| {
                sources
                    .iter()
                    .filter_map(move |&s| a.find(s).map(|o| (s, m * o.transform)))
            })
            .collect()
    }

    /// Called every frame in an assembly: rebuilds the scene when the
    /// Pattern panel's copies change.
    pub(super) fn pattern_preview_poll(&mut self) {
        let items = self.pattern_preview_items();
        let key = (!items.is_empty()).then(|| format!("{items:?}"));
        if key != self.asm.pattern_preview {
            self.asm.pattern_preview = key;
            self.scene_dirty = true;
        }
    }

    /// The preview copies, see-through (scene rebuild).
    pub(super) fn pattern_preview_scene(&self, g: &mut super::renderer::SceneGeometry) {
        if self.asm.pattern_preview.is_none() {
            return;
        }
        let Some(a) = self.assembly() else {
            return;
        };
        for (s, t) in self.pattern_preview_items() {
            let Some(o) = a.find(s) else {
                continue;
            };
            self.with_part(o.component, |p| {
                super::material::add_part_meshes_seen(
                    g,
                    p,
                    Some(&t),
                    Some([0.3, 0.6, 1.0]),
                    Some(0.35),
                );
            });
        }
    }

    /// Opens the Pattern panel on pattern `id` (browser Edit).
    pub(super) fn edit_pattern(&mut self, id: u32) {
        let Some(p) = self
            .assembly()
            .and_then(|a| a.patterns.iter().find(|p| p.id == id))
            .cloned()
        else {
            return;
        };
        let mut d = PartDialog::new(AsmPlanned::Pattern);
        d.edit = Some(id);
        let names: Vec<String> = p
            .sources
            .iter()
            .filter_map(|&s| self.occurrence_label_pub(s))
            .collect();
        d.picks.insert("components", names);
        let take = |d: &mut PartDialog, key: &'static str, g: &GeomRef| {
            d.picks.insert(key, vec![self.geom_label(g)]);
            d.refs.insert(key, vec![g.clone()]);
        };
        match &p.layout {
            Layout::Rectangular {
                column,
                column_flip,
                column_count,
                column_spacing,
                row,
                row_flip,
                row_count,
                row_spacing,
            } => {
                d.set_tab(RECTANGULAR);
                take(&mut d, "column", column);
                if let Some(r) = row {
                    take(&mut d, "row", r);
                }
                d.set_check(RECTANGULAR, "Flip column", *column_flip);
                d.set_num(RECTANGULAR, "Column Count", f64::from(*column_count));
                d.set_num(RECTANGULAR, "Column Spacing", *column_spacing);
                d.set_check(RECTANGULAR, "Flip row", *row_flip);
                d.set_num(RECTANGULAR, "Row Count", f64::from(*row_count));
                d.set_num(RECTANGULAR, "Row Spacing", *row_spacing);
            }
            Layout::Circular {
                axis,
                flip,
                count,
                angle,
            } => {
                d.set_tab(CIRCULAR);
                take(&mut d, "axis", axis);
                d.set_check(CIRCULAR, "Flip", *flip);
                d.set_num(CIRCULAR, "Count", f64::from(*count));
                d.set_num(CIRCULAR, "Angle", angle.to_degrees());
            }
        }
        d.active = None;
        self.asm.tool = None;
        self.part_planned = Some(d);
    }

    /// Deletes pattern `id` with its copies (one Undo step).
    pub(super) fn delete_pattern(&mut self, id: u32) {
        let Some(mut a) = self.assembly().cloned() else {
            return;
        };
        let Some(p) = a.remove_pattern(id) else {
            return;
        };
        self.checkpoint("Delete Component Pattern");
        if let Some(cur) = self.assembly_mut() {
            *cur = a;
        }
        self.asm.sel.clear();
        self.scene_dirty = true;
        self.file_status = Some(Ok(format!("{}: deleted with its copies", p.name)));
    }

    /// A pattern's node in the browser: its elements (Element:1 = the
    /// sources) with their components.
    pub(super) fn pattern_rows(
        &mut self,
        ui: &mut egui::Ui,
        p: &ComponentPattern,
        cmds: &mut Vec<PatternCmd>,
    ) {
        let names = |ids: &[u32]| -> Vec<(u32, String)> {
            ids.iter()
                .filter_map(|&i| {
                    self.assembly()
                        .and_then(|a| a.find(i))
                        .map(|o| (i, o.name.clone()))
                })
                .collect()
        };
        let mut elements = vec![(1, names(&p.sources))];
        elements.extend(p.elements.iter().map(|e| (e.index, names(&e.occurrences))));
        let all: Vec<u32> = elements
            .iter()
            .flat_map(|e| e.1.iter().map(|x| x.0))
            .collect();
        let on = !all.is_empty() && all.iter().all(|i| self.asm.sel.contains(i));
        egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            ui.make_persistent_id(("asm_pattern", p.id)),
            false,
        )
        .show_header(ui, |ui| {
            super::panels::tree_icon(ui, Icon::AsmPattern);
            let r = ui.selectable_label(on, &p.name);
            if r.clicked() {
                cmds.push(PatternCmd::Select(p.id));
            }
            if r.double_clicked() {
                cmds.push(PatternCmd::Edit(p.id));
            }
            r.context_menu(|ui| {
                if ui.button("Edit").clicked() {
                    cmds.push(PatternCmd::Edit(p.id));
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    cmds.push(PatternCmd::Delete(p.id));
                    ui.close();
                }
            });
        })
        .body(|ui| {
            for (index, occs) in elements {
                let ids: Vec<u32> = occs.iter().map(|x| x.0).collect();
                let on = !ids.is_empty() && ids.iter().all(|i| self.asm.sel.contains(i));
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    ui.make_persistent_id(("asm_pattern_el", p.id, index)),
                    false,
                )
                .show_header(ui, |ui| {
                    super::panels::tree_icon(ui, Icon::AsmPattern);
                    let r = ui.selectable_label(on, format!("Element:{index}"));
                    if r.clicked() {
                        cmds.push(PatternCmd::SelectElement(p.id, index));
                    }
                })
                .body(|ui| {
                    for (id, name) in occs {
                        ui.horizontal(|ui| {
                            super::panels::tree_icon(ui, Icon::Part);
                            let r = ui.selectable_label(self.asm.sel.contains(&id), name);
                            if r.clicked() {
                                cmds.push(PatternCmd::SelectOccurrence(id));
                            }
                            if r.hovered() {
                                self.asm.hover = Some(id);
                            }
                            r.context_menu(|ui| {
                                if !self.asm.sel.contains(&id) {
                                    self.asm.sel = vec![id];
                                }
                                let sel = self.asm.sel.clone();
                                self.occurrence_menu(ui, &sel);
                            });
                        });
                    }
                });
            }
        });
    }

    /// Runs a browser command of a pattern.
    pub(super) fn pattern_cmd(&mut self, c: PatternCmd) {
        let Some(a) = self.assembly() else {
            return;
        };
        match c {
            PatternCmd::Select(id) => {
                if let Some(p) = a.patterns.iter().find(|p| p.id == id) {
                    self.asm.sel = p
                        .sources
                        .iter()
                        .chain(p.elements.iter().flat_map(|e| e.occurrences.iter()))
                        .copied()
                        .filter(|&i| i != 0)
                        .collect();
                }
            }
            PatternCmd::SelectElement(id, index) => {
                if let Some(p) = a.patterns.iter().find(|p| p.id == id) {
                    self.asm.sel = if index == 1 {
                        p.sources.clone()
                    } else {
                        p.elements
                            .iter()
                            .filter(|e| e.index == index)
                            .flat_map(|e| e.occurrences.iter().copied())
                            .filter(|&i| i != 0)
                            .collect()
                    };
                }
            }
            PatternCmd::SelectOccurrence(id) => self.asm.sel = vec![id],
            PatternCmd::Edit(id) => self.edit_pattern(id),
            PatternCmd::Delete(id) => self.delete_pattern(id),
        }
        self.scene_dirty = true;
    }

    /// The pattern copy's note in the component's Properties.
    pub(super) fn pattern_note(&self, ui: &mut egui::Ui, id: u32) {
        let Some(a) = self.assembly() else {
            return;
        };
        if let Some(p) = a.pattern_of(id) {
            let what = if a.is_element(id) {
                "a copy placed by"
            } else {
                "a source of"
            };
            ui.label(RichText::new(format!("{what} {}", p.name)).weak());
        }
    }
}
