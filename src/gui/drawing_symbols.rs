//! Sketched symbols on a sheet (spec 4.13 §3.4): the Sketch Symbols dialog
//! (a definition with its preview, Scale, Rotate, Static, Leader, Visible
//! and the prompted entries), the placing tool (one click per symbol; with
//! Leader the arrow's tip first), Edit Symbol.

use eframe::egui::{self, Color32, Rect, RichText};

use super::CadCraftApp;
use super::drawing::{DrwTool, SheetCam};
use super::drawing_annot::paint_prims;
use super::valuefield::ExprValue;
use crate::drawing::resources::{self, PlacedSymbol};
use crate::drawing::sheet::Prim;
use crate::drawing::{AnnotationKind, annot, parts::Tables};

const PREVIEW: Color32 = Color32::from_rgb(60, 140, 90);

/// The Sketch Symbols dialog's values.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SymbolDialog {
    /// The instance it edits (`None`: places new ones).
    pub edit: Option<u32>,
    /// Search For.
    pub search: String,
    /// The chosen definition.
    pub name: Option<String>,
    /// Scale.
    pub scale: f64,
    /// Rotate (degrees).
    pub rotate: f64,
    /// Static.
    pub fixed: bool,
    /// Leader.
    pub leader: bool,
    /// Visible (the leader).
    pub visible: bool,
    /// The prompted entries (title, value).
    pub prompts: Vec<(String, String)>,
}

impl SymbolDialog {
    /// A new instance from these values at `at`.
    fn placed(&self, at: [f64; 2], tip: Option<[f64; 2]>) -> Option<PlacedSymbol> {
        Some(PlacedSymbol {
            name: self.name.clone()?,
            at,
            scale: self.scale,
            rotate: self.rotate.to_radians(),
            fixed: self.fixed,
            leader: tip.into_iter().collect(),
            leader_hidden: !self.visible,
            prompts: self.prompts.clone(),
        })
    }
}

/// The box of everything drawn in `prims`.
fn art_box(prims: &[Prim]) -> Option<([f64; 2], [f64; 2])> {
    let mut lo = [f64::MAX; 2];
    let mut hi = [f64::MIN; 2];
    let mut grow = |q: &[f64; 2]| {
        lo = [lo[0].min(q[0]), lo[1].min(q[1])];
        hi = [hi[0].max(q[0]), hi[1].max(q[1])];
    };
    for p in prims {
        match p {
            Prim::Line { a, b, .. } => {
                grow(a);
                grow(b);
            }
            Prim::Path { pts, .. } | Prim::Fill { pts } => pts.iter().for_each(&mut grow),
            Prim::Text { at, .. } => grow(at),
            _ => {}
        }
    }
    (lo[0] <= hi[0] && lo[1] <= hi[1]).then_some((lo, hi))
}

impl CadCraftApp {
    /// The prompted entries of definition `name`, empty.
    fn symbol_prompts(&self, name: &str) -> Vec<(String, String)> {
        self.drawing()
            .and_then(|d| d.resources.symbols.iter().find(|s| s.name == name))
            .map(|s| {
                s.prompts()
                    .into_iter()
                    .map(|t| (t, String::new()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Annotate › Symbols: the dialog, on definition `name` when given.
    pub(super) fn open_symbols(&mut self, name: Option<String>) {
        self.drw_close_tools();
        let name = name.or_else(|| {
            self.drawing()
                .and_then(|d| d.resources.symbols.first())
                .map(|s| s.name.clone())
        });
        let prompts = name
            .as_deref()
            .map(|n| self.symbol_prompts(n))
            .unwrap_or_default();
        self.drw.symbol_dialog = Some(SymbolDialog {
            edit: None,
            search: String::new(),
            name,
            scale: 1.0,
            rotate: 0.0,
            fixed: false,
            leader: false,
            visible: true,
            prompts,
        });
    }

    /// Edit Symbol: the dialog with instance `id`'s values; `false` when
    /// `id` is no symbol.
    pub(super) fn open_symbol_edit(&mut self, id: u32) -> bool {
        let Some(s) = self
            .drawing()
            .and_then(|d| d.sheet())
            .and_then(|s| s.annotations.iter().find(|a| a.id == id))
            .and_then(|a| match &a.kind {
                AnnotationKind::Symbol(s) => Some(s.clone()),
                _ => None,
            })
        else {
            return false;
        };
        let mut prompts = self.symbol_prompts(&s.name);
        for (t, v) in &mut prompts {
            if let Some((_, old)) = s.prompts.iter().find(|(o, _)| o == t) {
                v.clone_from(old);
            }
        }
        self.drw.symbol_dialog = Some(SymbolDialog {
            edit: Some(id),
            search: String::new(),
            name: Some(s.name),
            scale: s.scale,
            rotate: s.rotate.to_degrees(),
            fixed: s.fixed,
            leader: !s.leader.is_empty(),
            visible: !s.leader_hidden,
            prompts,
        });
        true
    }

    /// What instance `s` draws (preview and dialog; not yet on the sheet).
    fn symbol_prims(&self, s: &PlacedSymbol) -> Vec<Prim> {
        let Some(d) = self.drawing() else {
            return Vec::new();
        };
        let i = d.active_sheet;
        let props = self.field_props(d, i);
        let Some(art) = resources::symbol_art(d, i, s, &|f| props.get(f)) else {
            return Vec::new();
        };
        let mut t = Tables::default();
        t.symbols.insert((s.name.clone(), s.prompts.clone()), art);
        annot::resolve_in(
            &AnnotationKind::Symbol(s.clone()),
            &|_| None,
            &self.drw.look,
            &t,
        )
        .map(|r| r.prims)
        .unwrap_or_default()
    }

    /// The Sketch Symbols window.
    pub(super) fn symbol_window(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.drw.symbol_dialog.clone() else {
            return;
        };
        let names: Vec<String> = self
            .drawing()
            .map(|d| d.resources.symbols.iter().map(|s| s.name.clone()).collect())
            .unwrap_or_default();
        let art = dlg
            .placed([0.0, 0.0], None)
            .map(|s| {
                self.symbol_prims(&PlacedSymbol {
                    scale: 1.0,
                    rotate: 0.0,
                    leader: Vec::new(),
                    ..s
                })
            })
            .unwrap_or_default();
        let (mut open, mut ok, mut cancel) = (true, false, false);
        let mut picked = None;
        let win = egui::Window::new("Sketch Symbols")
            .id(egui::Id::new("symbol_dialog"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false);
        self.layout
            .place("symbol_dialog", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                if names.is_empty() {
                    ui.label(
                        "No symbols yet: Define Sketched Symbol, or Drawing Resources › Sketched Symbols › Define New Symbol",
                    );
                }
                ui.horizontal(|ui| {
                    ui.label("Search For");
                    ui.text_edit_singleline(&mut dlg.search);
                });
                let needle = dlg.search.to_lowercase();
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(150.0);
                        egui::ScrollArea::vertical()
                            .id_salt("symbol_list")
                            .max_height(120.0)
                            .show(ui, |ui| {
                                for n in names
                                    .iter()
                                    .filter(|n| n.to_lowercase().contains(&needle))
                                {
                                    let on = dlg.name.as_ref() == Some(n);
                                    if ui.selectable_label(on, n).clicked() && !on {
                                        picked = Some(n.clone());
                                    }
                                }
                            });
                    });
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(140.0, 120.0),
                        egui::Sense::hover(),
                    );
                    let p = ui.painter_at(rect);
                    p.rect_filled(rect, 2.0, Color32::WHITE);
                    if let Some((lo, hi)) = art_box(&art) {
                        let (w, h) = ((hi[0] - lo[0]).max(1.0), (hi[1] - lo[1]).max(1.0));
                        let zoom = (130.0 / w).min(110.0 / h);
                        let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
                        paint_prims(&p, &SheetCam::at(c, zoom), rect, &art, Color32::BLACK);
                    }
                });
                ui.separator();
                egui::Grid::new("symbol_values").num_columns(2).show(ui, |ui| {
                    ui.label("Scale");
                    ui.add(ExprValue::new(&mut dlg.scale).range(0.001..=1000.0));
                    ui.end_row();
                    ui.label("Rotate");
                    ui.add(ExprValue::new(&mut dlg.rotate).range(-360.0..=360.0).suffix("°"));
                    ui.end_row();
                });
                ui.add_enabled(false, egui::Checkbox::new(&mut false, "Symbol Clipping"))
                    .on_disabled_hover_text("Planned: spec 4.13 §3.4");
                ui.checkbox(&mut dlg.fixed, "Static");
                ui.add_enabled(
                    dlg.edit.is_none(),
                    egui::Checkbox::new(&mut dlg.leader, "Leader"),
                );
                ui.add_enabled(dlg.leader, egui::Checkbox::new(&mut dlg.visible, "Visible"));
                if !dlg.prompts.is_empty() {
                    ui.separator();
                    ui.label(RichText::new("Prompted Entries").strong());
                    egui::Grid::new("symbol_prompts").num_columns(2).show(ui, |ui| {
                        for (t, v) in &mut dlg.prompts {
                            ui.label(t.as_str());
                            ui.text_edit_singleline(v);
                            ui.end_row();
                        }
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let can = dlg.name.is_some();
                    if ui.add_enabled(can, super::theme::ok_button("OK")).clicked() {
                        ok = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if let Some(n) = picked {
            dlg.prompts = self.symbol_prompts(&n);
            dlg.name = Some(n);
        }
        if cancel || !open {
            self.drw.symbol_dialog = None;
        } else if ok {
            self.drw.symbol_dialog = None;
            match dlg.edit {
                Some(id) => self.apply_symbol_edit(id, &dlg),
                None => self.drw.tool = Some(DrwTool::Symbol(Box::new(dlg), None)),
            }
        } else {
            self.drw.symbol_dialog = Some(dlg);
        }
    }

    /// OK of Edit Symbol: the dialog's values go on instance `id`.
    fn apply_symbol_edit(&mut self, id: u32, dlg: &SymbolDialog) {
        let dlg = dlg.clone();
        self.edit_drawing("Edit Symbol", move |d| {
            let a = d
                .sheets
                .iter_mut()
                .flat_map(|s| s.annotations.iter_mut())
                .find(|a| a.id == id);
            if let Some(AnnotationKind::Symbol(s)) = a.map(|a| &mut a.kind) {
                if let Some(n) = dlg.name.clone() {
                    s.name = n;
                }
                s.scale = dlg.scale;
                s.rotate = dlg.rotate.to_radians();
                s.fixed = dlg.fixed;
                s.leader_hidden = !dlg.visible;
                s.prompts.clone_from(&dlg.prompts);
            }
        });
        self.history.break_coalescing();
    }

    /// The status bar's prompt of the placing tool.
    pub(super) fn symbol_prompt(&self) -> Option<String> {
        match &self.drw.tool {
            Some(DrwTool::Symbol(d, None)) if d.leader => {
                Some("Symbols: click the leader's arrow tip (right-click: Done)".into())
            }
            Some(DrwTool::Symbol(_, None)) => {
                Some("Symbols: click where the symbol goes (right-click: Done)".into())
            }
            Some(DrwTool::Symbol(_, Some(_))) => Some(
                "Symbols: click where the symbol goes (right-click: back to the arrow tip)".into(),
            ),
            _ => None,
        }
    }

    /// The placing tool: a click per symbol; with Leader the arrow's tip
    /// first, then the symbol.
    pub(super) fn symbol_tool(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
    ) {
        let Some(DrwTool::Symbol(dlg, tip)) = self.drw.tool.clone() else {
            return;
        };
        let Some(p) = response.hover_pos() else {
            return;
        };
        let cam = self.drw.cam;
        let q = super::drawing::snap(cam.to_sheet(rect, p));
        if dlg.leader && tip.is_none() {
            painter.circle_stroke(p, 4.0, egui::Stroke::new(1.5, PREVIEW));
            if response.clicked() {
                self.drw.tool = Some(DrwTool::Symbol(dlg, Some(q)));
            }
            return;
        }
        let Some(s) = dlg.placed(q, tip) else {
            return;
        };
        let prims = self.symbol_prims(&s);
        paint_prims(painter, &cam, rect, &prims, PREVIEW);
        if response.clicked() {
            self.edit_drawing("Symbols", move |d| {
                d.add_annotation(AnnotationKind::Symbol(s));
            });
            self.history.break_coalescing();
            self.drw.tool = Some(DrwTool::Symbol(dlg, None));
        }
    }
}
