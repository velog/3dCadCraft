//! Show Dimensions / Pick from Feature (`docs/spec/0-ui.md` §2.7, the `>`
//! menu of a value field): a click on the model (or a choice in the small
//! window) picks a feature; its sketch's dimensions appear around the model
//! and the window lists them with the feature's own values (Extrude
//! distance…); a click on one puts its *name* into the field (`d3 = d1`
//! follows it), and the dimensions hide again.

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::measure_pick::result_id;
use crate::part::params::ParamOwner;

/// The running Show Dimensions.
#[derive(Clone, Debug)]
pub(super) struct ShowDims {
    /// The field that gets the name.
    pub target: egui::Id,
    /// The feature picked (index) and its sketch (index).
    pub feature: Option<usize>,
    pub sketch: Option<usize>,
}

/// Temp-data key a field uses to ask for Show Dimensions.
pub(super) fn request_id() -> egui::Id {
    egui::Id::new("show_dims_request")
}

impl CadCraftApp {
    /// A field's `>` → Show Dimensions asked for it this frame.
    pub(super) fn show_dims_requests(&mut self, ctx: &egui::Context) {
        let req = ctx.data_mut(|d| {
            let r = d.get_temp::<egui::Id>(request_id());
            d.remove::<egui::Id>(request_id());
            r
        });
        if let Some(target) = req {
            self.show_dims = Some(ShowDims {
                target,
                feature: None,
                sketch: None,
            });
            self.file_status = Some(Ok(
                "Show Dimensions: click a feature on the model (or pick it in the window)".into(),
            ));
        }
    }

    /// Feature `k` is the one whose dimensions show.
    fn show_dims_feature(&mut self, k: usize) {
        let sketch = self
            .part()
            .features
            .get(k)
            .and_then(|f| f.sketch().map(str::to_owned))
            .and_then(|id| self.part().sketches.iter().position(|s| s.id == id));
        if let Some(s) = self.show_dims.as_mut() {
            s.feature = Some(k);
            s.sketch = sketch;
        }
    }

    /// A click on the model while Show Dimensions waits, not on a
    /// dimension: the feature of the face under it.
    pub(super) fn show_dims_face(&mut self, p: egui::Pos2, rect: egui::Rect) -> bool {
        let face = match self.plane_under(p, rect, false) {
            Some(crate::part::PlaneRef::Face(n)) => n,
            _ => return false,
        };
        let id = face.split([':', '#']).next().unwrap_or_default().to_owned();
        match self.part().features.iter().position(|f| f.id() == id) {
            Some(k) => {
                self.show_dims_feature(k);
                true
            }
            None => false,
        }
    }

    /// The name `name` goes into the waiting field; Show Dimensions ends.
    pub(super) fn show_dims_pick(&mut self, ctx: &egui::Context, name: String) {
        if let Some(s) = self.show_dims.take() {
            ctx.data_mut(|d| d.insert_temp(result_id(s.target), name));
            self.file_status = None;
        }
    }

    /// The window: the feature (a list too) and its dimensions and values.
    pub(super) fn show_dims_window(&mut self, ctx: &egui::Context) {
        let Some(s) = self.show_dims.clone() else {
            return;
        };
        let names: Vec<(usize, String)> = self
            .part()
            .features
            .iter()
            .enumerate()
            .map(|(k, f)| (k, f.name().to_owned()))
            .collect();
        let rows: Vec<_> = self
            .part()
            .parameter_table()
            .into_iter()
            .filter(|r| match (&r.owner, s.feature) {
                (ParamOwner::Feature { feature, .. }, Some(k)) => *feature == k,
                (ParamOwner::Dimension { sketch, .. }, _) => Some(*sketch) == s.sketch,
                _ => false,
            })
            .collect();
        let mut pick: Option<String> = None;
        let mut feature: Option<usize> = None;
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let mut open = true;
        egui::Window::new("Show Dimensions")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos(self.viewport_rect.right_top() + egui::vec2(-300.0, 60.0))
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(
                        "Click a feature on the model, then a dimension (in the view or below).",
                    )
                    .weak(),
                );
                let shown = s
                    .feature
                    .and_then(|k| names.iter().find(|(i, _)| *i == k))
                    .map_or("Pick a feature".to_owned(), |(_, n)| n.clone());
                egui::ComboBox::from_id_salt("show_dims_feature")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        for (k, n) in &names {
                            if ui
                                .selectable_label(s.feature == Some(*k), n.as_str())
                                .clicked()
                            {
                                feature = Some(*k);
                            }
                        }
                    });
                ui.separator();
                if s.feature.is_some() && rows.is_empty() {
                    ui.label(RichText::new("No dimensions").weak());
                }
                for r in &rows {
                    let text = format!("{} = {}", r.name, r.shown());
                    let hover = match &r.expr {
                        Some(e) => format!("{} (= {e}), {}", r.consumed_by, r.name),
                        None => r.consumed_by.clone(),
                    };
                    if ui.button(text).on_hover_text(hover).clicked() {
                        pick = Some(r.name.clone());
                    }
                }
                ui.separator();
                close |= ui.button("Cancel").clicked();
            });
        if let Some(k) = feature {
            self.show_dims_feature(k);
        }
        if let Some(n) = pick {
            self.show_dims_pick(ctx, n);
        } else if close || !open {
            self.show_dims = None;
            self.file_status = None;
        }
    }
}
