//! Overlay View (spec 4-drawings, row Overlay; format revision 77): click an
//! assembly view, choose the positional (and design view) representation,
//! OK places the overlay over its parent. Its lines are drawn dashed (the
//! phantom of Inventor's overlays) and its hidden lines are left out.
//! Also the representation rows of a view's Properties.

use std::sync::Arc;

use eframe::egui;

use super::CadCraftApp;
use super::drawing::DrwTool;
use crate::drawing::hlr::ViewLines;
use crate::drawing::{DrawingView, ViewKind};

/// Overlay View in progress.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct OverlayDraft {
    /// The parent view (an assembly view).
    pub parent: Option<u32>,
    /// The positional representation chosen.
    pub position: Option<u32>,
    /// The design view (`None`: the parent's).
    pub design: Option<u32>,
}

impl OverlayDraft {
    /// The status bar's prompt.
    pub(super) fn prompt(&self) -> &'static str {
        match self.parent {
            None => "Overlay View: click a view of an assembly (Esc ends)",
            Some(_) => "Overlay View: choose the positional representation, then OK",
        }
    }
}

/// `l` drawn as an overlay: its visible lines as the dashed ones, its own
/// hidden lines left out.
pub(super) fn phantom(l: &ViewLines, center: [f64; 2]) -> ViewLines {
    ViewLines {
        anchor: Some(center),
        hidden: l.visible.clone(),
        hidden_src: l.visible_src.clone(),
        visible: Vec::new(),
        visible_src: Vec::new(),
        section: Vec::new(),
        section_owner: Vec::new(),
        section_cover: Vec::new(),
        threads: Vec::new(),
        ..l.clone()
    }
}

/// A representation combo: `sel` among `list`, `none` the text of `None`.
fn rep_combo(
    ui: &mut egui::Ui,
    id: &str,
    list: &[(u32, String)],
    sel: &mut Option<u32>,
    none: &str,
) {
    let shown = sel
        .and_then(|s| list.iter().find(|x| x.0 == s))
        .map_or(none, |x| x.1.as_str())
        .to_owned();
    egui::ComboBox::from_id_salt(id)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            ui.selectable_value(sel, None, none);
            for (k, n) in list {
                ui.selectable_value(sel, Some(*k), n);
            }
        });
}

impl CadCraftApp {
    /// Place Views › Overlay View: a selected assembly view is the parent.
    pub(super) fn start_overlay(&mut self) {
        let pre = self.drw.sel.filter(|&id| self.overlay_parent_ok(id));
        self.drw.dialog = None;
        self.drw.sel_ann = None;
        self.drw.tool = Some(DrwTool::Overlay(OverlayDraft {
            parent: pre,
            ..Default::default()
        }));
    }

    /// View `id` of the active sheet shows an assembly.
    fn overlay_parent_ok(&self, id: u32) -> bool {
        let Some(m) = self.drawing().and_then(|d| d.model_of(d.sheet()?, id)) else {
            return false;
        };
        !self.rep_lists(m)[0].is_empty()
    }

    /// The command on the sheet: a click on an assembly view picks it.
    pub(super) fn overlay_tool(&mut self, response: &egui::Response) {
        let Some(DrwTool::Overlay(d)) = &self.drw.tool else {
            return;
        };
        if d.parent.is_some() || !response.clicked() {
            return;
        }
        match self.drw.hover {
            Some(id) if self.overlay_parent_ok(id) => {
                if let Some(DrwTool::Overlay(d)) = &mut self.drw.tool {
                    d.parent = Some(id);
                }
            }
            Some(_) => {
                self.file_status = Some(Err("Overlay View: the view must show an assembly".into()));
            }
            None => {}
        }
    }

    /// The Overlay View window once the parent is picked.
    pub(super) fn overlay_window(&mut self, ctx: &egui::Context) {
        let Some(DrwTool::Overlay(d)) = &self.drw.tool else {
            return;
        };
        let Some(parent) = d.parent else {
            return;
        };
        let mut d = d.clone();
        let Some(model) = self
            .drawing()
            .and_then(|doc| doc.model_of(doc.sheet()?, parent))
        else {
            return;
        };
        let lists = self.rep_lists(model);
        let (mut ok, mut cancel) = (false, false);
        let r = ctx.content_rect();
        let win = egui::Window::new("Overlay View")
            .resizable(false)
            .collapsible(false)
            .default_width(280.0)
            .default_pos(egui::pos2(r.max.x - 620.0, r.min.y + 200.0));
        self.layout
            .place("drawing_overlay", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                egui::Grid::new("ov_grid").num_columns(2).show(ui, |ui| {
                    ui.label("Positional Representation");
                    rep_combo(ui, "ov_pos", &lists[1], &mut d.position, "(choose)");
                    ui.end_row();
                    ui.label("Design View Representation");
                    rep_combo(ui, "ov_view", &lists[0], &mut d.design, "(the parent's)");
                    ui.end_row();
                });
                ui.label(egui::RichText::new("Drawn over the parent view in phantom lines").weak());
                ui.separator();
                ui.horizontal(|ui| {
                    ok = ui
                        .add_enabled(d.position.is_some(), super::theme::ok_button("OK"))
                        .on_disabled_hover_text("Choose the positional representation")
                        .clicked()
                        || super::keys::panel_ok_key(ui, d.position.is_some());
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if let Some(DrwTool::Overlay(x)) = &mut self.drw.tool {
            *x = d.clone();
        }
        if cancel {
            self.drw.tool = None;
        } else if ok && let Some(position) = d.position {
            let design = d.design;
            self.edit_drawing("Overlay View", move |doc| {
                doc.add_overlay_view(parent, position, design);
            });
            self.drw.tool = None;
        }
    }

    /// The view's Properties rows of its representations (assembly base
    /// views and overlays); changes go into `nv`.
    pub(super) fn representation_rows(
        &self,
        ui: &mut egui::Ui,
        model: Option<crate::document::DocRef>,
        nv: &mut DrawingView,
    ) {
        let Some(m) = model else {
            return;
        };
        let lists = self.rep_lists(m);
        if lists[0].is_empty() {
            return;
        }
        let base = matches!(nv.kind, ViewKind::Base { .. });
        if !base && !matches!(nv.kind, ViewKind::Overlay { .. }) {
            return;
        }
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Representation").strong());
        egui::Grid::new("drw_rep_props")
            .num_columns(2)
            .show(ui, |ui| {
                if base {
                    for (label, k, sel) in [
                        ("Variant", 2, &mut nv.reps.variant),
                        ("Design View", 0, &mut nv.reps.view),
                        ("Position", 1, &mut nv.reps.position),
                    ] {
                        super::panels::plabel(ui, label);
                        rep_combo(
                            ui,
                            &format!("drw_rep_prop_{label}"),
                            &lists[k],
                            sel,
                            "(active in the assembly)",
                        );
                        ui.end_row();
                    }
                }
                if let ViewKind::Overlay {
                    position, design, ..
                } = &mut nv.kind
                {
                    super::panels::plabel(ui, "Position");
                    let mut p = Some(*position);
                    rep_combo(ui, "drw_ov_pos", &lists[1], &mut p, "(choose)");
                    if let Some(p) = p {
                        *position = p;
                    }
                    ui.end_row();
                    super::panels::plabel(ui, "Design View");
                    rep_combo(ui, "drw_ov_view", &lists[0], design, "(the parent's)");
                    ui.end_row();
                }
            });
    }

    /// Lines of overlay views turned phantom (cached per computed lines).
    pub(super) fn overlay_lines(
        &mut self,
        id: u32,
        l: Arc<ViewLines>,
        center: [f64; 2],
    ) -> Arc<ViewLines> {
        if let Some((from, to)) = self.drw.phantom.get(&id)
            && Arc::ptr_eq(from, &l)
            && to.anchor == Some(center)
        {
            return to.clone();
        }
        let to = Arc::new(phantom(&l, center));
        self.drw.phantom.insert(id, (l, to.clone()));
        to
    }
}
