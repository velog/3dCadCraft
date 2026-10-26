//! Assembly views, Parts List, Balloon and Auto Balloon in drawings
//! (`docs/spec/4.4-parts-list.md`).
//!
//! - An assembly view's lines: every occurrence's part model moved into
//!   place (`hlr::assembly_model`), computed on the drawing worker from
//!   copies of the parts. The assembly is a document of this project or a
//!   linked file; its components are reached the way the assembly reaches
//!   them (a linked assembly's own links are relative to its file).
//! - BOM rows for parts lists and balloons come from `assembly::bom`; a part
//!   view's "BOM" is its one part.
//! - The commands: Parts List (dialog, then the table follows the cursor and
//!   snaps to the border corner or onto the title block), Balloon (a
//!   component's edge, then the balloon's place; repeats), Auto Balloon (a
//!   view, components, placement) and the parts list editor (double-click).

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use eframe::egui::{self, Color32, Rect, RichText};

use super::CadCraftApp;
use super::drawing::{DrwTool, ShownView};
use super::drawing_annot::paint_prims;
use super::valuefield::ExprValue;
use crate::assembly::bom::{self, BomRow, ComponentInfo};
use crate::document::{AssemblyDocument, DocRef, DocumentData, Project};
use crate::drawing::annot::{AnnotationKind, Placement};
use crate::drawing::hlr::ViewLines;
use crate::drawing::hlr::{self, ViewModel};
use crate::drawing::parts::{self, Balloon, BalloonShape, BomKind, PartsList, Tables};
use crate::drawing::{DrawingDocument, ViewFrame, sheet as fmt};
use crate::kernel::Transform3;
use crate::part::PartDocument;

const PREVIEW: Color32 = Color32::from_rgb(60, 140, 90);
const HOVER: Color32 = Color32::from_rgb(230, 120, 40);
/// Background of an overridden cell in the editor.
const OVERRIDE_BG: Color32 = Color32::from_rgb(255, 226, 160);

/// Where an assembly and its components live.
#[derive(Clone)]
pub(super) enum AsmSrc {
    /// In this project.
    Local,
    /// In a linked file: its folder (its links are relative to it) and
    /// its project.
    File { dir: PathBuf, project: Rc<Project> },
}

/// What a view's lines are computed from: copies of the parts and, for an
/// assembly, the occurrences (id, placement, index into `parts`).
pub(super) struct ModelJob {
    parts: Vec<PartDocument>,
    occurrences: Vec<(u32, Transform3, usize)>,
    assembly: bool,
    /// A part's flat pattern instead of the folded part.
    flat: bool,
}

impl ModelJob {
    /// The view's lines (on the worker): the model seen in `frame`; a flat
    /// pattern in its own frame with its bends.
    pub(super) fn lines(&self, frame: &ViewFrame, aligned: Option<&hlr::Aligned>) -> ViewLines {
        if self.flat
            && !self.assembly
            && let Some(part) = self.parts.first()
            && let Some(lines) = hlr::flat_lines(part, frame, aligned)
        {
            return lines;
        }
        hlr::view_lines_with(&self.model(), frame, aligned)
    }

    /// The view model (on the worker).
    pub(super) fn model(&self) -> ViewModel {
        if !self.assembly {
            return self.parts.first().map(hlr::part_model).unwrap_or_default();
        }
        let models: Vec<Arc<ViewModel>> = self
            .parts
            .iter()
            .map(|p| {
                let mut m = hlr::part_model(p);
                m.smooth = Some(m.meshes.iter().flat_map(hlr::smooth_edges).collect());
                Arc::new(m)
            })
            .collect();
        let instances: Vec<hlr::Instance> = self
            .occurrences
            .iter()
            .filter_map(|&(id, transform, k)| {
                Some(hlr::Instance {
                    id,
                    transform,
                    model: models.get(k)?.clone(),
                })
            })
            .collect();
        hlr::assembly_model(&instances, hlr::Simplify::default())
    }
}

/// The Parts List command's dialog.
#[derive(Clone, Debug)]
pub(super) struct PlDialog {
    tab: usize,
    /// The source view.
    pub view: Option<u32>,
    /// The settings (position set at placement).
    pub pl: PartsList,
    /// OK pressed: the table follows the cursor.
    pub placing: bool,
}

/// Balloon options while the command runs.
#[derive(Clone, Debug)]
pub(super) struct BalloonOpts {
    pub shape: BalloonShape,
    pub bom: BomKind,
    /// Edit of an existing balloon (shape / text).
    pub edit: Option<u32>,
    pub text: String,
}

/// The Auto Balloon dialog.
#[derive(Clone, Debug)]
pub(super) struct AutoDialog {
    tab: usize,
    pub view: Option<u32>,
    /// Occurrences left out (Add or Remove Components).
    pub removed: Vec<u32>,
    /// The components selector is active (else the view selector).
    pub components: bool,
    pub ignore_multiple: bool,
    /// Around, Horizontal, Vertical.
    pub placement: usize,
    pub offset: f64,
    pub shape: BalloonShape,
    pub bom: BomKind,
}

/// The parts list editor (double-click on a parts list).
#[derive(Clone, Debug)]
pub(super) struct PlEditor {
    pub id: u32,
    pub pl: PartsList,
    /// Column chooser shown.
    chooser: bool,
}

/// Parts list and balloon UI state.
#[derive(Default)]
pub(super) struct BomUi {
    pub dialog: Option<PlDialog>,
    pub balloon: Option<BalloonOpts>,
    pub auto: Option<AutoDialog>,
    pub editor: Option<PlEditor>,
}

impl BomUi {
    pub(super) fn open(&self) -> bool {
        self.dialog.is_some() || self.balloon.is_some() || self.auto.is_some()
    }
}

impl CadCraftApp {
    // ---- models ----------------------------------------------------------------

    /// The assembly a drawing view shows, with where its components live.
    pub(super) fn drawing_assembly(&self, model: DocRef) -> Option<(AssemblyDocument, AsmSrc)> {
        match model.library {
            None => match self.project.document(model.document).map(|d| &d.data) {
                Some(DocumentData::Assembly(a)) => Some((a.clone(), AsmSrc::Local)),
                _ => None,
            },
            Some(lib) => {
                let path = self.library_path(lib)?;
                let project = self.part_file_pub(&path).ok()?;
                let asm = match project.document(model.document).map(|d| &d.data) {
                    Some(DocumentData::Assembly(a)) => a.clone(),
                    _ => return None,
                };
                let dir = path.parent()?.to_path_buf();
                Some((asm, AsmSrc::File { dir, project }))
            }
        }
    }

    /// The path of a linked file of a linked project.
    fn src_path(dir: &std::path::Path, project: &Project, r: DocRef) -> Option<PathBuf> {
        let link = project.library(r.library?)?;
        let stored = dir.join(&link.path);
        if stored.exists() {
            return Some(stored);
        }
        let flat = dir.join(std::path::Path::new(&link.path).file_name()?);
        flat.exists().then_some(flat)
    }

    /// Runs `f` on component `r` of an assembly from `src`.
    fn src_part<R>(
        &self,
        src: &AsmSrc,
        r: DocRef,
        f: impl FnOnce(&PartDocument) -> R,
    ) -> Option<R> {
        match src {
            AsmSrc::Local => self.with_part(r, f),
            AsmSrc::File { dir, project } => match r.library {
                None => match project.document(r.document).map(|d| &d.data) {
                    Some(DocumentData::Part(p)) => Some(f(p)),
                    _ => None,
                },
                Some(_) => {
                    let path = Self::src_path(dir, project, r)?;
                    let p = self.part_file_pub(&path).ok()?;
                    match p.document(r.document).map(|d| &d.data) {
                        Some(DocumentData::Part(part)) => Some(f(part)),
                        _ => None,
                    }
                }
            },
        }
    }

    /// What the BOM reads of component `r` from `src`.
    fn src_info(&self, src: &AsmSrc, r: DocRef) -> ComponentInfo {
        let AsmSrc::File { dir, project } = src else {
            return self.bom_info(r);
        };
        let material = self.src_part(src, r, |p| p.material.clone()).flatten();
        let flat = self
            .src_part(src, r, |p| {
                p.flat_extents().map(|x| (x.length, x.width, x.area))
            })
            .flatten();
        let (name, props) = match r.library {
            None => match project.document(r.document) {
                Some(d) => (d.name.clone(), Some(d.properties.clone())),
                None => ("?".into(), None),
            },
            Some(_) => {
                let path = Self::src_path(dir, project, r);
                let name = path
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .and_then(|s| s.to_str())
                    .map_or_else(|| "?".into(), str::to_owned);
                let props = path
                    .and_then(|p| self.part_file_pub(&p).ok())
                    .and_then(|p| p.document(r.document).map(|d| d.properties.clone()));
                (name, props)
            }
        };
        ComponentInfo {
            file_name: name,
            missing: props.is_none(),
            props: props.unwrap_or_default(),
            material,
            flat,
        }
    }

    /// The signature of an assembly view's model (`None`: not an assembly):
    /// each component's body and each occurrence's placement.
    pub(super) fn assembly_signature(&self, model: DocRef) -> Option<String> {
        let (asm, src) = self.drawing_assembly(model)?;
        let mut s = String::new();
        let mut seen: Vec<DocRef> = Vec::new();
        for o in asm.occurrences.iter().filter(|o| o.visible) {
            if !seen.contains(&o.component) {
                seen.push(o.component);
                let sig = self
                    .src_part(&src, o.component, |p| p.body_signature())
                    .unwrap_or_default();
                s.push_str(&format!("{:?}={sig};", o.component));
            }
            s.push_str(&format!("{}:{:?}:{:.6?};", o.id, o.component, o.transform));
        }
        Some(s)
    }

    /// Copies of what a view of `model` is computed from.
    pub(super) fn model_job(&self, model: DocRef, flat: bool) -> Option<ModelJob> {
        let Some((asm, src)) = self.drawing_assembly(model) else {
            let part = self.with_part(model, |p| p.clone())?;
            return Some(ModelJob {
                parts: vec![part],
                occurrences: Vec::new(),
                assembly: false,
                flat,
            });
        };
        let mut comps: Vec<DocRef> = Vec::new();
        let mut parts = Vec::new();
        let mut occurrences = Vec::new();
        for o in asm.occurrences.iter().filter(|o| o.visible) {
            let k = match comps.iter().position(|c| *c == o.component) {
                Some(k) => k,
                None => {
                    let Some(p) = self.src_part(&src, o.component, |p| p.clone()) else {
                        continue;
                    };
                    comps.push(o.component);
                    parts.push(p);
                    parts.len() - 1
                }
            };
            occurrences.push((o.id, o.transform, k));
        }
        Some(ModelJob {
            parts,
            occurrences,
            assembly: true,
            flat: false,
        })
    }

    /// The BOM rows of a view's model: an assembly's BOM, or a part's one
    /// row.
    pub(super) fn bom_rows(
        &self,
        model: DocRef,
        kind: BomKind,
        merge: bool,
    ) -> Option<Vec<BomRow>> {
        match self.drawing_assembly(model) {
            Some((asm, src)) => Some(bom::rows(&asm, kind.view(), merge, &|r| {
                self.src_info(&src, r)
            })),
            None => {
                self.with_part(model, |_| ())?;
                let mut one = AssemblyDocument::default();
                one.place(model, "part", Transform3::identity(), true);
                Some(bom::rows(&one, kind.view(), merge, &|r| self.bom_info(r)))
            }
        }
    }

    /// The BOM tables of sheet `i`.
    pub(super) fn bom_tables(&self, doc: &DrawingDocument, i: usize) -> Tables {
        Tables::build(doc, i, &|m, k, merge| self.bom_rows(m, k, merge))
    }

    // ---- commands ---------------------------------------------------------------

    /// Annotate › Parts List.
    pub(super) fn open_parts_list(&mut self) {
        self.drw_close_tools();
        // A selected view is the source (as Inventor takes a pre-selection).
        let view = self.drw.sel;
        self.drw.bom.dialog = Some(PlDialog {
            tab: 0,
            view,
            pl: PartsList::new(view.unwrap_or(0), [0.0, 0.0]),
            placing: false,
        });
        self.drw.tool = Some(DrwTool::PartsList);
    }

    /// Annotate › Balloon.
    pub(super) fn open_balloon(&mut self) {
        self.drw_close_tools();
        self.drw.bom.balloon = Some(BalloonOpts {
            shape: BalloonShape::Circle,
            bom: BomKind::Structured,
            edit: None,
            text: String::new(),
        });
        self.drw.tool = Some(DrwTool::Balloon(None));
    }

    /// Annotate › Auto Balloon.
    pub(super) fn open_auto_balloon(&mut self) {
        self.drw_close_tools();
        self.drw.bom.auto = Some(AutoDialog {
            tab: 0,
            view: self.drw.sel,
            removed: Vec::new(),
            components: self.drw.sel.is_some(),
            ignore_multiple: true,
            placement: 0,
            offset: 10.5,
            shape: BalloonShape::Circle,
            bom: BomKind::Structured,
        });
        self.drw.tool = Some(DrwTool::AutoBalloon);
    }

    fn drw_close_tools(&mut self) {
        self.drw.tool = None;
        self.drw.dialog = None;
        self.drw.planned = None;
        self.drw.bom.dialog = None;
        self.drw.bom.balloon = None;
        self.drw.bom.auto = None;
        self.drw.sel_ann = None;
    }

    /// Esc / Cancel of the parts list and balloon commands.
    pub(super) fn bom_cancel(&mut self) {
        self.drw.bom.dialog = None;
        self.drw.bom.balloon = None;
        self.drw.bom.auto = None;
    }

    /// The status-bar prompt of the running command.
    pub(super) fn bom_prompt(&self) -> Option<String> {
        Some(match &self.drw.tool {
            Some(DrwTool::PartsList) => match &self.drw.bom.dialog {
                Some(d) if d.placing => {
                    "Parts List: click where the table goes (it snaps to the border corner and onto the title block)".into()
                }
                Some(d) if d.view.is_none() => "Parts List: select a view: its model is the source".into(),
                _ => "Parts List: set the options and press OK".into(),
            },
            Some(DrwTool::Balloon(None)) => {
                "Balloon: click an edge of a component in an assembly view (Esc ends)".into()
            }
            Some(DrwTool::Balloon(Some(_))) => {
                "Balloon: click where the balloon goes (right-click starts over)".into()
            }
            Some(DrwTool::AutoBalloon) => match &self.drw.bom.auto {
                Some(d) if d.view.is_none() || !d.components => {
                    "Auto Balloon: select the view the balloons come from".into()
                }
                _ => "Auto Balloon: click components to leave them out or add them back, then OK".into(),
            },
            _ => return None,
        })
    }

    /// The view under sheet point `q`.
    fn view_at(views: &[ShownView], q: [f64; 2]) -> Option<u32> {
        views
            .iter()
            .rev()
            .find(|v| q[0] >= v.lo[0] && q[0] <= v.hi[0] && q[1] >= v.lo[1] && q[1] <= v.hi[1])
            .map(|v| v.id)
    }

    /// Clicks and previews of the parts list and balloon commands.
    pub(super) fn bom_tool(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        response: &egui::Response,
        views: &[ShownView],
    ) {
        let cam = self.drw.cam;
        let pointer = response.hover_pos();
        let at = pointer.map(|p| cam.to_sheet(rect, p));
        match self.drw.tool.clone() {
            Some(DrwTool::PartsList) => {
                let Some(dlg) = self.drw.bom.dialog.clone() else {
                    self.drw.tool = None;
                    return;
                };
                if !dlg.placing {
                    if response.clicked()
                        && let Some(q) = at
                        && let Some(v) = Self::view_at(views, q)
                        && let Some(d) = self.drw.bom.dialog.as_mut()
                    {
                        d.view = Some(v);
                        d.pl.view = v;
                    }
                    return;
                }
                let Some(q) = at else {
                    return;
                };
                let mut pl = dlg.pl.clone();
                pl.at = q;
                let Some(t) = self.drw.tables.table_of(&pl).or_else(|| {
                    let m = self
                        .drawing()
                        .and_then(|d| d.model_of(d.sheet()?, pl.view))?;
                    Some(parts::table(&pl, &self.bom_rows(m, pl.bom, pl.merge)?))
                }) else {
                    return;
                };
                pl.at = self.snap_parts_list(&pl, &t, q);
                let prims = parts::table_prims(&pl, &t, &self.drw.look);
                paint_prims(painter, &cam, rect, &prims, PREVIEW);
                if response.clicked() {
                    self.edit_drawing("Parts List", |d| {
                        d.add_annotation(AnnotationKind::PartsList(pl))
                    });
                    self.drw.bom.dialog = None;
                    self.drw.tool = None;
                }
            }
            Some(DrwTool::Balloon(first)) => {
                let opts = self.drw.bom.balloon.clone().unwrap_or(BalloonOpts {
                    shape: BalloonShape::Circle,
                    bom: BomKind::Structured,
                    edit: None,
                    text: String::new(),
                });
                match first {
                    None => {
                        let under = pointer
                            .and_then(|p| self.geometry_under(views, rect, p))
                            .filter(|g| hlr::occurrence_of(&g.edge).is_some())
                            .map(|g| {
                                // The leader starts where the pointer is on the edge.
                                let along =
                                    self.along_of(views, g.view, &g.edge, at.unwrap_or(g.at));
                                let p = self.edge_at(views, g.view, &g.edge, along).unwrap_or(g.at);
                                (g, along, p)
                            });
                        if let Some((_, _, p)) = &under {
                            painter.circle_filled(cam.to_screen(rect, *p), 4.0, HOVER);
                        }
                        if response.clicked() {
                            match under {
                                Some((g, along, _)) => {
                                    self.drw.tool =
                                        Some(DrwTool::Balloon(Some((g.view, g.edge, along))));
                                }
                                None => {
                                    self.file_status = Some(Err(
                                        "Balloon: click an edge of a component in an assembly view"
                                            .into(),
                                    ))
                                }
                            }
                        }
                    }
                    Some((view, edge, along)) => {
                        let Some(q) = at else {
                            return;
                        };
                        let Some(v) = views.iter().find(|v| v.id == view) else {
                            return;
                        };
                        let b = Balloon {
                            view,
                            edge: edge.clone(),
                            along,
                            at: [q[0] - v.position[0], q[1] - v.position[1]],
                            shape: opts.shape,
                            bom: opts.bom,
                            text: None,
                        };
                        let kind = AnnotationKind::Balloon(b);
                        let shown = self.preview_annotation(&kind, views);
                        match shown {
                            Some(prims) => paint_prims(painter, &cam, rect, &prims, PREVIEW),
                            None => {
                                if response.clicked() {
                                    self.file_status = Some(Err(
                                        "Balloon: this component is not in the BOM (Reference or Phantom)".into(),
                                    ));
                                    self.drw.tool = Some(DrwTool::Balloon(None));
                                }
                                return;
                            }
                        }
                        if response.clicked() {
                            self.edit_drawing("Balloon", |d| d.add_annotation(kind));
                            self.drw.tool = Some(DrwTool::Balloon(None));
                        }
                    }
                }
            }
            Some(DrwTool::AutoBalloon) => {
                let Some(dlg) = self.drw.bom.auto.clone() else {
                    self.drw.tool = None;
                    return;
                };
                // The balloons it would place.
                if let Some(view) = dlg.view {
                    for kind in self.auto_balloons(&dlg, view, views) {
                        if let Some(prims) = self.preview_annotation(&kind, views) {
                            paint_prims(painter, &cam, rect, &prims, PREVIEW);
                        }
                    }
                }
                if !response.clicked() {
                    return;
                }
                let Some(q) = at else {
                    return;
                };
                if dlg.view.is_some() && dlg.components {
                    let g = pointer.and_then(|p| self.geometry_under(views, rect, p));
                    if let Some(occ) = g
                        .filter(|g| Some(g.view) == dlg.view)
                        .and_then(|g| hlr::occurrence_of(&g.edge))
                        && let Some(d) = self.drw.bom.auto.as_mut()
                    {
                        if let Some(i) = d.removed.iter().position(|o| *o == occ) {
                            d.removed.remove(i);
                        } else {
                            d.removed.push(occ);
                        }
                    }
                } else if let Some(v) = Self::view_at(views, q)
                    && let Some(d) = self.drw.bom.auto.as_mut()
                {
                    d.view = Some(v);
                    d.removed.clear();
                    d.components = true;
                }
            }
            _ => {}
        }
    }

    /// Where sheet point `at` lies along view `view`'s edge `edge`.
    fn along_of(&self, views: &[ShownView], view: u32, edge: &str, at: [f64; 2]) -> f64 {
        let Some(v) = views.iter().find(|v| v.id == view) else {
            return 0.5;
        };
        let Some(l) = v.lines.as_deref() else {
            return 0.5;
        };
        let Some(e) = l.edge(edge) else {
            return 0.5;
        };
        let c = l.center();
        let q = [
            c[0] + (at[0] - v.position[0]) / v.scale,
            c[1] + (at[1] - v.position[1]) / v.scale,
        ];
        parts::edge_along(e, q)
    }

    /// The sheet point at `along` on view `view`'s edge `edge`.
    fn edge_at(&self, views: &[ShownView], view: u32, edge: &str, along: f64) -> Option<[f64; 2]> {
        let v = views.iter().find(|v| v.id == view)?;
        let l = v.lines.as_deref()?;
        let pl = Placement {
            position: v.position,
            scale: v.scale,
            center: l.center(),
        };
        Some(pl.to_sheet(parts::edge_point(l.edge(edge)?, along)))
    }

    /// An annotation drawn now (preview), with this frame's tables.
    fn preview_annotation(
        &self,
        kind: &AnnotationKind,
        views: &[ShownView],
    ) -> Option<Vec<fmt::Prim>> {
        let view = |id: u32| {
            let v = views.iter().find(|v| v.id == id)?;
            let l = v.lines.as_deref()?;
            Some((
                Placement {
                    position: v.position,
                    scale: v.scale,
                    center: l.center(),
                },
                l,
            ))
        };
        let tables = self.drw.tables.clone();
        crate::drawing::annot::resolve_in(kind, &view, &self.drw.look, &tables)
            .ok()
            .map(|s| s.prims)
    }

    /// The table's corner snapped to the border's upper right corner, or so
    /// that it stands on the title block, when the pointer is near.
    fn snap_parts_list(&self, pl: &PartsList, t: &parts::Table, q: [f64; 2]) -> [f64; 2] {
        let Some(sheet) = self.drawing().and_then(|d| d.sheet()) else {
            return q;
        };
        let (lo, hi) = parts::bounds(pl, t, self.drw.look.leader_text.height);
        let height = hi[1] - lo[1];
        let mut targets = vec![fmt::frame(sheet).1];
        if let Some((_, th)) = fmt::title_rect(sheet) {
            targets.push([th[0], th[1] + height]);
        }
        let tol = (12.0 / self.drw.cam.zoom).max(4.0);
        targets
            .into_iter()
            .find(|p| (p[0] - q[0]).hypot(p[1] - q[1]) < tol)
            .unwrap_or_else(|| super::drawing::snap(q))
    }

    /// The balloons Auto Balloon makes: one per BOM row (or occurrence),
    /// on the longest visible edge of the component, placed around the view
    /// or in a row / column beside it.
    fn auto_balloons(
        &self,
        dlg: &AutoDialog,
        view: u32,
        views: &[ShownView],
    ) -> Vec<AnnotationKind> {
        let Some(v) = views.iter().find(|v| v.id == view) else {
            return Vec::new();
        };
        let Some(l) = v.lines.as_deref() else {
            return Vec::new();
        };
        let tables = &self.drw.tables;
        let pl = Placement {
            position: v.position,
            scale: v.scale,
            center: l.center(),
        };
        // Each occurrence's leader starts on the middle of one of its visible
        // pieces: long pieces on the side the balloons go to win, so leaders
        // do not cross the view (the top edge for a row above it).
        let mid_sheet = [(v.lo[0] + v.hi[0]) * 0.5, (v.lo[1] + v.hi[1]) * 0.5];
        let mut best: std::collections::BTreeMap<u32, (f64, String, [f64; 2], f64)> =
            Default::default();
        let sheet_segments: Vec<([f64; 2], [f64; 2])> = l
            .visible
            .iter()
            .flat_map(|p| p.windows(2).map(|w| (pl.to_sheet(w[0]), pl.to_sheet(w[1]))))
            .collect();
        for (k, piece) in l.visible.iter().enumerate() {
            let Some(e) = l
                .visible_src
                .get(k)
                .copied()
                .flatten()
                .and_then(|i| l.edges.get(i as usize))
            else {
                continue;
            };
            let Some(name) = &e.name else {
                continue;
            };
            let Some(occ) = hlr::occurrence_of(name) else {
                continue;
            };
            if dlg.removed.contains(&occ) || tables.item(view, dlg.bom, occ).is_none() {
                continue;
            }
            let pts: Vec<[f64; 2]> = piece.iter().map(|q| pl.to_sheet(*q)).collect();
            let len: f64 = pts
                .windows(2)
                .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
                .sum();
            if len < 1.0 {
                continue;
            }
            // Points at a quarter, half and three quarters of the piece; a
            // leader from one must not cross other lines nor run along its
            // own edge.
            for f in [0.5, 0.25, 0.75] {
                let tip = point_along(&pts, len * f);
                let end = match dlg.placement {
                    1 => [tip[0], v.hi[1] + dlg.offset],
                    2 => [v.hi[0] + dlg.offset, tip[1]],
                    _ => {
                        let d = [tip[0] - mid_sheet[0], tip[1] - mid_sheet[1]];
                        let n = d[0].hypot(d[1]).max(1e-9);
                        let r = (v.hi[0] - v.lo[0]).hypot(v.hi[1] - v.lo[1]) * 0.5 + dlg.offset;
                        [mid_sheet[0] + d[0] / n * r, mid_sheet[1] + d[1] / n * r]
                    }
                };
                let crosses = sheet_segments
                    .iter()
                    .filter(|(a, b)| segments_cross(tip, end, *a, *b))
                    .count();
                let dir = [end[0] - tip[0], end[1] - tip[1]];
                let along_edge = pts.windows(2).any(|w| {
                    let e = [w[1][0] - w[0][0], w[1][1] - w[0][1]];
                    let cross = (e[0] * dir[1] - e[1] * dir[0]).abs();
                    cross < 0.05 * e[0].hypot(e[1]) * dir[0].hypot(dir[1])
                });
                let outward = match dlg.placement {
                    1 => tip[1] - v.lo[1],
                    2 => tip[0] - v.lo[0],
                    _ => (tip[0] - mid_sheet[0]).hypot(tip[1] - mid_sheet[1]),
                };
                let score = len.min(30.0) + outward
                    - 1000.0 * crosses as f64
                    - if along_edge { 500.0 } else { 0.0 };
                if best.get(&occ).is_none_or(|b| score > b.0) {
                    let c = l.center();
                    let q = [
                        c[0] + (tip[0] - v.position[0]) / v.scale,
                        c[1] + (tip[1] - v.position[1]) / v.scale,
                    ];
                    best.insert(occ, (score, name.clone(), tip, parts::edge_along(e, q)));
                }
            }
        }
        // One balloon per item unless every instance is wanted.
        let mut seen: Vec<String> = Vec::new();
        let mut picks: Vec<(String, [f64; 2])> = Vec::new();
        let mut alongs: Vec<f64> = Vec::new();
        for (occ, (_, name, tip, along)) in best {
            let item = tables
                .item(view, dlg.bom, occ)
                .map(|x| x.0.clone())
                .unwrap_or_default();
            if dlg.ignore_multiple {
                if seen.contains(&item) {
                    continue;
                }
                seen.push(item);
            }
            picks.push((name, tip));
            alongs.push(along);
        }
        let gap = (parts::BALLOON_DIAMETER + 2.0).max(dlg.offset * 0.6);
        let centres = place_balloons(&picks, v.lo, v.hi, dlg.placement, dlg.offset, gap);
        picks
            .into_iter()
            .zip(centres)
            .zip(alongs)
            .map(|(((edge, _), c), along)| {
                AnnotationKind::Balloon(Balloon {
                    view,
                    edge,
                    along,
                    at: [c[0] - v.position[0], c[1] - v.position[1]],
                    shape: dlg.shape,
                    bom: dlg.bom,
                    text: None,
                })
            })
            .collect()
    }

    // ---- windows ------------------------------------------------------------------

    /// The Parts List, Balloon, Auto Balloon and editor windows.
    pub(super) fn bom_windows(&mut self, ctx: &egui::Context) {
        self.parts_list_window(ctx);
        self.balloon_window(ctx);
        self.auto_balloon_window(ctx);
        self.parts_list_editor(ctx);
    }

    fn parts_list_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.drw.bom.dialog.clone() else {
            return;
        };
        if d.placing {
            return;
        }
        let models = self.drawing().map(|doc| {
            d.view
                .and_then(|v| doc.model_of(doc.sheet()?, v))
                .map(|m| self.drawing_model_name(m))
        });
        let mut ok = false;
        let mut cancel = false;
        let win = egui::Window::new("Parts List")
            .id(egui::Id::new("drw_parts_list"))
            .collapsible(false)
            .resizable(false);
        self.layout
            .place("drw_parts_list", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (i, t) in ["Source", "BOM Settings", "Table Wrapping"]
                        .iter()
                        .enumerate()
                    {
                        ui.selectable_value(&mut d.tab, i, *t);
                    }
                });
                ui.separator();
                egui::Grid::new("drw_pl_grid")
                    .num_columns(2)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| match d.tab {
                        0 => {
                            ui.label("Select View");
                            let text = match d.view {
                                Some(v) => format!("View {v}"),
                                None => "click a view on the sheet".into(),
                            };
                            ui.label(RichText::new(text).color(if d.view.is_some() {
                                Color32::from_rgb(60, 160, 80)
                            } else {
                                Color32::from_rgb(40, 120, 230)
                            }));
                            ui.end_row();
                            ui.label("File");
                            ui.label(models.flatten().flatten().unwrap_or_else(|| "-".into()));
                            ui.end_row();
                            ui.label("Model State");
                            ui.add_enabled(false, egui::Label::new("[Primary]"))
                                .on_disabled_hover_text("Model states come later (spec 4.4)");
                            ui.end_row();
                        }
                        1 => {
                            ui.label("BOM View");
                            egui::ComboBox::from_id_salt("drw_pl_bom")
                                .selected_text(d.pl.bom.label())
                                .show_ui(ui, |ui| {
                                    for k in [BomKind::Structured, BomKind::PartsOnly] {
                                        ui.selectable_value(&mut d.pl.bom, k, k.label());
                                    }
                                });
                            ui.end_row();
                            ui.label("Level");
                            ui.horizontal(|ui| {
                                let _ = ui.radio(true, "First Level");
                                ui.add_enabled(false, egui::RadioButton::new(false, "All Levels"))
                                    .on_disabled_hover_text("Sub-assemblies come later (spec 4.4)");
                            });
                            ui.end_row();
                            ui.label("Minimum Digits");
                            let mut digits = f64::from(d.pl.min_digits);
                            ui.add(ExprValue::new(&mut digits));
                            d.pl.min_digits = digits.round().clamp(1.0, 6.0) as u8;
                            ui.end_row();
                            ui.label("Numbering");
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut d.pl.alpha, false, "Numeric");
                                ui.radio_value(&mut d.pl.alpha, true, "Alpha");
                            });
                            ui.end_row();
                            ui.label("Case (Alpha)");
                            ui.add_enabled_ui(d.pl.alpha, |ui| {
                                ui.horizontal(|ui| {
                                    ui.radio_value(&mut d.pl.lower, false, "Upper");
                                    ui.radio_value(&mut d.pl.lower, true, "Lower");
                                });
                            });
                            ui.end_row();
                            ui.label("");
                            ui.checkbox(&mut d.pl.merge, "Part Number Row Merge");
                            ui.end_row();
                        }
                        _ => {
                            ui.label("Direction to Wrap Table");
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut d.pl.wrap.left, true, "Left");
                                ui.radio_value(&mut d.pl.wrap.left, false, "Right");
                            });
                            ui.end_row();
                            let mut auto = d.pl.wrap.max_rows.is_some();
                            ui.label("");
                            ui.checkbox(&mut auto, "Enable Automatic Wrap");
                            ui.end_row();
                            let mut rows = f64::from(d.pl.wrap.max_rows.unwrap_or(20));
                            ui.label("Maximum Rows");
                            ui.add_enabled(auto, ExprValue::new(&mut rows));
                            ui.end_row();
                            d.pl.wrap.max_rows = auto.then(|| rows.round().max(1.0) as u32);
                            let mut secs = f64::from(d.pl.wrap.sections);
                            ui.label("Number of Sections");
                            ui.add_enabled(!auto, ExprValue::new(&mut secs));
                            ui.end_row();
                            d.pl.wrap.sections = secs.round().clamp(1.0, 20.0) as u32;
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(d.view.is_some(), egui::Button::new("OK"))
                        .on_disabled_hover_text("Select a view first")
                        .clicked()
                        || (d.view.is_some() && super::keys::panel_ok_key(ui, true))
                    {
                        ok = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel {
            self.bom_cancel();
            self.drw.tool = None;
            return;
        }
        d.placing = ok;
        self.drw.bom.dialog = Some(d);
    }

    /// A model's name for the dialogs.
    fn drawing_model_name(&self, m: DocRef) -> Option<String> {
        match m.library {
            None => self.project.document(m.document).map(|d| d.name.clone()),
            Some(lib) => self.project.library(lib).map(|l| l.path.clone()),
        }
    }

    fn balloon_window(&mut self, ctx: &egui::Context) {
        let Some(mut o) = self.drw.bom.balloon.clone() else {
            return;
        };
        let mut done = false;
        let mut apply = false;
        let title = if o.edit.is_some() {
            "Edit Balloon"
        } else {
            "Balloon"
        };
        let win = egui::Window::new(title)
            .id(egui::Id::new("drw_balloon"))
            .collapsible(false)
            .resizable(false);
        self.layout
            .place("drw_balloon", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                egui::Grid::new("drw_balloon_grid")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Shape");
                        egui::ComboBox::from_id_salt("drw_balloon_shape")
                            .selected_text(o.shape.label())
                            .show_ui(ui, |ui| {
                                for s in BalloonShape::ALL {
                                    ui.selectable_value(&mut o.shape, s, s.label());
                                }
                            });
                        ui.end_row();
                        ui.label("BOM View");
                        egui::ComboBox::from_id_salt("drw_balloon_bom")
                            .selected_text(o.bom.label())
                            .show_ui(ui, |ui| {
                                for k in [BomKind::Structured, BomKind::PartsOnly] {
                                    ui.selectable_value(&mut o.bom, k, k.label());
                                }
                            });
                        ui.end_row();
                        if o.edit.is_some() {
                            ui.label("Override Text");
                            ui.add(
                                egui::TextEdit::singleline(&mut o.text).hint_text("item number"),
                            );
                            ui.end_row();
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if o.edit.is_some() {
                        if ui.button("OK").clicked() || super::keys::panel_ok_key(ui, true) {
                            apply = true;
                            done = true;
                        }
                        if ui.button("Cancel").clicked() {
                            done = true;
                        }
                    } else if ui.button("Done").clicked() {
                        done = true;
                    }
                });
            });
        if apply && let Some(id) = o.edit {
            let (shape, bom, text) = (o.shape, o.bom, o.text.trim().to_owned());
            self.edit_drawing("Edit Balloon", |d| {
                if let Some(AnnotationKind::Balloon(b)) = d
                    .sheet_mut()
                    .and_then(|s| s.annotations.iter_mut().find(|a| a.id == id))
                    .map(|a| &mut a.kind)
                {
                    b.shape = shape;
                    b.bom = bom;
                    b.text = (!text.is_empty()).then_some(text);
                }
            });
        }
        if done {
            self.drw.bom.balloon = None;
            if matches!(self.drw.tool, Some(DrwTool::Balloon(_))) {
                self.drw.tool = None;
            }
        } else {
            self.drw.bom.balloon = Some(o);
        }
    }

    /// Double-click on a balloon: shape, BOM view, text override.
    pub(super) fn open_balloon_edit(&mut self, id: u32, b: &Balloon) {
        self.drw.bom.balloon = Some(BalloonOpts {
            shape: b.shape,
            bom: b.bom,
            edit: Some(id),
            text: b.text.clone().unwrap_or_default(),
        });
    }

    fn auto_balloon_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.drw.bom.auto.clone() else {
            return;
        };
        let mut ok = false;
        let mut cancel = false;
        let win = egui::Window::new("Auto Balloon")
            .id(egui::Id::new("drw_auto_balloon"))
            .collapsible(false)
            .resizable(false);
        self.layout
            .place("drw_auto_balloon", ctx.content_rect(), false, win)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (i, t) in ["Selection", "BOM Settings", "Placement"]
                        .iter()
                        .enumerate()
                    {
                        ui.selectable_value(&mut d.tab, i, *t);
                    }
                });
                ui.separator();
                egui::Grid::new("drw_ab_grid")
                    .num_columns(2)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| match d.tab {
                        0 => {
                            if ui
                                .selectable_label(!d.components, "Select View Set")
                                .on_hover_text("Click the view the balloons come from")
                                .clicked()
                            {
                                d.components = false;
                            }
                            ui.label(
                                d.view
                                    .map_or_else(|| "click a view".into(), |v| format!("View {v}")),
                            );
                            ui.end_row();
                            if ui
                                .add_enabled(
                                    d.view.is_some(),
                                    egui::Button::selectable(
                                        d.components,
                                        "Add or Remove Components",
                                    ),
                                )
                                .on_hover_text(
                                    "Click a component to leave it out, again to add it back",
                                )
                                .clicked()
                            {
                                d.components = true;
                            }
                            ui.label(if d.removed.is_empty() {
                                "all components".to_string()
                            } else {
                                format!("{} left out", d.removed.len())
                            });
                            ui.end_row();
                            ui.label("");
                            ui.checkbox(&mut d.ignore_multiple, "Ignore Multiple Instances");
                            ui.end_row();
                        }
                        1 => {
                            ui.label("BOM View");
                            egui::ComboBox::from_id_salt("drw_ab_bom")
                                .selected_text(d.bom.label())
                                .show_ui(ui, |ui| {
                                    for k in [BomKind::Structured, BomKind::PartsOnly] {
                                        ui.selectable_value(&mut d.bom, k, k.label());
                                    }
                                });
                            ui.end_row();
                            ui.label("Level");
                            ui.add_enabled(false, egui::Label::new("First Level"))
                                .on_disabled_hover_text("Sub-assemblies come later (spec 4.4)");
                            ui.end_row();
                        }
                        _ => {
                            ui.label("Placement");
                            ui.horizontal(|ui| {
                                for (i, t) in
                                    ["Around", "Horizontal", "Vertical"].iter().enumerate()
                                {
                                    ui.radio_value(&mut d.placement, i, *t);
                                }
                            });
                            ui.end_row();
                            ui.label("Offset Spacing");
                            ui.add(ExprValue::new(&mut d.offset).suffix(" mm"));
                            ui.end_row();
                            ui.label("Shape");
                            egui::ComboBox::from_id_salt("drw_ab_shape")
                                .selected_text(d.shape.label())
                                .show_ui(ui, |ui| {
                                    for s in BalloonShape::ALL {
                                        ui.selectable_value(&mut d.shape, s, s.label());
                                    }
                                });
                            ui.end_row();
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(d.view.is_some(), egui::Button::new("OK"))
                        .on_disabled_hover_text("Select a view first")
                        .clicked()
                        || (d.view.is_some() && super::keys::panel_ok_key(ui, true))
                    {
                        ok = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel {
            self.bom_cancel();
            self.drw.tool = None;
            return;
        }
        if ok && let Some(view) = d.view {
            let views = self.shown_views(ctx);
            let kinds = self.auto_balloons(&d, view, &views);
            if kinds.is_empty() {
                self.file_status = Some(Err(
                    "Auto Balloon: the view has no component of the BOM (is it an assembly view?)"
                        .into(),
                ));
                self.drw.bom.auto = Some(d);
                return;
            }
            let n = kinds.len();
            self.edit_drawing("Auto Balloon", |doc| {
                for k in kinds {
                    doc.add_annotation(k);
                }
            });
            self.file_status = Some(Ok(format!("Auto Balloon: {n} balloons placed")));
            self.bom_cancel();
            self.drw.tool = None;
            return;
        }
        self.drw.bom.auto = Some(d);
    }

    /// Double-click on a parts list: the editor.
    pub(super) fn open_parts_list_editor(&mut self, id: u32) {
        let pl = self
            .drawing()
            .and_then(|d| d.sheet())
            .and_then(|s| s.annotations.iter().find(|a| a.id == id))
            .and_then(|a| match &a.kind {
                AnnotationKind::PartsList(p) => Some(p.clone()),
                _ => None,
            });
        if let Some(pl) = pl {
            self.drw.bom.editor = Some(PlEditor {
                id,
                pl,
                chooser: false,
            });
        }
    }

    /// The parts list editor: cells (typed text overrides the BOM's value
    /// and is kept by part number, shown coloured), row visibility, custom
    /// rows, columns, sort.
    fn parts_list_editor(&mut self, ctx: &egui::Context) {
        let Some(mut ed) = self.drw.bom.editor.clone() else {
            return;
        };
        let Some(doc) = self.drawing().cloned() else {
            return;
        };
        let rows = doc
            .sheet()
            .and_then(|s| doc.model_of(s, ed.pl.view))
            .and_then(|m| self.bom_rows(m, ed.pl.bom, ed.pl.merge))
            .unwrap_or_default();
        let table = parts::table(&ed.pl, &rows);
        let available = parts::available_columns(&rows);
        let mut open = true;
        let mut ok = false;
        let mut apply = false;
        let mut cancel = false;
        let win = egui::Window::new("Parts List")
            .id(egui::Id::new("drw_pl_editor"))
            .open(&mut open)
            .resizable(true)
            .default_width(760.0);
        self.layout
            .place("drw_pl_editor", ctx.content_rect(), true, win)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .button("Column Chooser...")
                        .on_hover_text("Add, remove and order the columns (any iProperty or custom property)")
                        .clicked()
                    {
                        ed.chooser = !ed.chooser;
                    }
                    ui.label("Sort by");
                    let current = ed.pl.sort.as_ref().map(|s| s.column.clone());
                    let label = current
                        .as_deref()
                        .map_or_else(|| "(BOM order)".to_string(), parts::column_title);
                    egui::ComboBox::from_id_salt("drw_pl_sort")
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(current.is_none(), "(BOM order)").clicked() {
                                ed.pl.sort = None;
                            }
                            for c in &ed.pl.columns {
                                if ui.selectable_label(current.as_deref() == Some(&c.key), &c.title).clicked() {
                                    ed.pl.sort = Some(parts::SortBy {
                                        column: c.key.clone(),
                                        ascending: true,
                                        renumber: false,
                                    });
                                }
                            }
                        });
                    if let Some(s) = ed.pl.sort.as_mut() {
                        ui.checkbox(&mut s.ascending, "Ascending");
                        ui.checkbox(&mut s.renumber, "Renumber Items")
                            .on_hover_text("Item numbers follow the sorted order (balloons too)");
                    }
                    if ui.button("Add Custom Row").clicked() {
                        ed.pl.custom_rows.push(parts::CustomRow::default());
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Title");
                    ui.add(egui::TextEdit::singleline(&mut ed.pl.title).desired_width(200.0));
                    ui.weak("Typed cells override the BOM (coloured); the override stays with the part number.");
                });
                if ed.chooser {
                    ui.group(|ui| {
                        ui.label(RichText::new("Columns").strong());
                        let mut remove = None;
                        let mut up = None;
                        ui.horizontal_wrapped(|ui| {
                            for (i, c) in ed.pl.columns.iter_mut().enumerate() {
                                ui.group(|ui| {
                                    ui.add(egui::TextEdit::singleline(&mut c.title).desired_width(90.0));
                                    ui.add(ExprValue::new(&mut c.width).suffix(" mm"));
                                    if i > 0 && ui.small_button("<").on_hover_text("Move left").clicked() {
                                        up = Some(i);
                                    }
                                    if ui.small_button("x").on_hover_text("Remove").clicked() {
                                        remove = Some(i);
                                    }
                                });
                            }
                        });
                        if let Some(i) = up {
                            ed.pl.columns.swap(i - 1, i);
                        }
                        if let Some(i) = remove
                            && ed.pl.columns.len() > 1
                        {
                            ed.pl.columns.remove(i);
                        }
                        ui.menu_button("Add Column", |ui| {
                            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                                for (k, t) in &available {
                                    if ed.pl.columns.iter().any(|c| c.key == *k) {
                                        continue;
                                    }
                                    if ui.button(t).clicked() {
                                        ed.pl.columns.push(parts::Column::new(k));
                                        ui.close();
                                    }
                                }
                            });
                        });
                    });
                }
                ui.separator();
                egui::ScrollArea::both().max_height(420.0).show(ui, |ui| {
                    egui::Grid::new("drw_pl_cells")
                        .striped(true)
                        .num_columns(ed.pl.columns.len() + 1)
                        .show(ui, |ui| {
                            ui.label(RichText::new("Shown").strong());
                            for c in &ed.pl.columns {
                                ui.label(RichText::new(&c.title).strong());
                            }
                            ui.end_row();
                            for r in &table.rows {
                                let mut shown = !r.hidden;
                                if ui.checkbox(&mut shown, "").on_hover_text("Visible in the table").changed() {
                                    if shown {
                                        ed.pl.hidden.retain(|h| *h != r.key);
                                    } else {
                                        ed.pl.hidden.push(r.key.clone());
                                    }
                                }
                                for (ci, c) in ed.pl.columns.clone().iter().enumerate() {
                                    let mut text = r.cells.get(ci).cloned().unwrap_or_default();
                                    let over = r.overridden.get(ci).copied().unwrap_or(false);
                                    let mut edit = egui::TextEdit::singleline(&mut text).desired_width(c.width as f32 * 2.2);
                                    if over {
                                        edit = edit.background_color(OVERRIDE_BG).text_color(Color32::BLACK);
                                    }
                                    let resp = ui.add(edit);
                                    if resp.changed() {
                                        if let Some(k) = r.key.strip_prefix("custom:").and_then(|k| k.parse::<usize>().ok()) {
                                            if let Some(row) = ed.pl.custom_rows.get_mut(k) {
                                                row.cells.insert(c.key.clone(), text.clone());
                                            }
                                        } else {
                                            let bom_value = rows
                                                .iter()
                                                .find(|b| b.part_number == r.key)
                                                .map(|b| parts::bom_cell(b, &c.key))
                                                .unwrap_or_default();
                                            let same = text == bom_value && c.key != "item";
                                            ed.pl.set_override(&r.key, &c.key, (!same).then_some(text.clone()));
                                        }
                                    }
                                    if over {
                                        resp.context_menu(|ui| {
                                            if ui.button("Remove Override").clicked() {
                                                ed.pl.set_override(&r.key, &c.key, None);
                                                ui.close();
                                            }
                                        });
                                    }
                                }
                                ui.end_row();
                            }
                        });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() || super::keys::panel_ok_key(ui, true) {
                        ok = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    if ui.button("Apply").clicked() {
                        apply = true;
                    }
                });
            });
        if ok || apply {
            let (id, pl) = (ed.id, ed.pl.clone());
            self.edit_drawing("Edit Parts List", |d| {
                if let Some(a) = d
                    .sheet_mut()
                    .and_then(|s| s.annotations.iter_mut().find(|a| a.id == id))
                {
                    a.kind = AnnotationKind::PartsList(pl);
                }
            });
        }
        if ok || cancel || !open {
            self.drw.bom.editor = None;
        } else {
            self.drw.bom.editor = Some(ed);
        }
    }
}

/// The point `d` along polyline `pts` (sheet mm).
fn point_along(pts: &[[f64; 2]], d: f64) -> [f64; 2] {
    let mut left = d;
    for w in pts.windows(2) {
        let l = (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
        if l >= left {
            let t = left / l.max(1e-12);
            return [
                w[0][0] + (w[1][0] - w[0][0]) * t,
                w[0][1] + (w[1][1] - w[0][1]) * t,
            ];
        }
        left -= l;
    }
    pts.last().copied().unwrap_or_default()
}

/// Segment `p`–`q` crosses `a`–`b` away from `p` (a leader leaving its
/// own edge does not count).
fn segments_cross(p: [f64; 2], q: [f64; 2], a: [f64; 2], b: [f64; 2]) -> bool {
    let d = [q[0] - p[0], q[1] - p[1]];
    let e = [b[0] - a[0], b[1] - a[1]];
    let den = d[0] * e[1] - d[1] * e[0];
    if den.abs() < 1e-12 {
        return false;
    }
    let w = [a[0] - p[0], a[1] - p[1]];
    let t = (w[0] * e[1] - w[1] * e[0]) / den;
    let u = (w[0] * d[1] - w[1] * d[0]) / den;
    let len = d[0].hypot(d[1]).max(1e-9);
    t * len > 0.3 && t < 1.0 && (0.0..=1.0).contains(&u)
}

/// Balloon centres for tips `picks` of a view (sheet box `lo`–`hi`):
/// `placement` 0 around the view, 1 a row above it, 2 a column to its
/// right; `offset` from the view, at least `gap` between balloons.
pub(super) fn place_balloons(
    picks: &[(String, [f64; 2])],
    lo: [f64; 2],
    hi: [f64; 2],
    placement: usize,
    offset: f64,
    gap: f64,
) -> Vec<[f64; 2]> {
    let n = picks.len();
    if n == 0 {
        return Vec::new();
    }
    let mid = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5];
    // Spreads sorted coordinates apart by `gap`, around their mean.
    let spread = |vals: &mut [(usize, f64)]| {
        vals.sort_by(|a, b| a.1.total_cmp(&b.1));
        for k in 1..vals.len() {
            if vals[k].1 < vals[k - 1].1 + gap {
                vals[k].1 = vals[k - 1].1 + gap;
            }
        }
    };
    let mut out = vec![[0.0, 0.0]; n];
    match placement {
        1 | 2 => {
            let axis = if placement == 1 { 0 } else { 1 };
            let mut vals: Vec<(usize, f64)> = picks
                .iter()
                .enumerate()
                .map(|(i, p)| (i, p.1[axis]))
                .collect();
            let before: f64 = vals.iter().map(|v| v.1).sum::<f64>() / n as f64;
            spread(&mut vals);
            let after: f64 = vals.iter().map(|v| v.1).sum::<f64>() / n as f64;
            for (i, v) in vals {
                let c = v - (after - before);
                out[i] = if placement == 1 {
                    [c, hi[1] + offset]
                } else {
                    [hi[0] + offset, c]
                };
            }
        }
        _ => {
            // Around: each balloon outward from the view's middle through its
            // tip, on a rounded box `offset` outside the view; neighbours on
            // the ring pushed apart.
            let rx = (hi[0] - lo[0]) * 0.5 + offset;
            let ry = (hi[1] - lo[1]) * 0.5 + offset;
            let r = rx.max(ry);
            let mut ang: Vec<(usize, f64)> = picks
                .iter()
                .enumerate()
                .map(|(i, p)| (i, (p.1[1] - mid[1]).atan2(p.1[0] - mid[0])))
                .collect();
            ang.sort_by(|a, b| a.1.total_cmp(&b.1));
            let step = (gap / r).min(std::f64::consts::TAU / n as f64);
            for k in 1..ang.len() {
                if ang[k].1 < ang[k - 1].1 + step {
                    ang[k].1 = ang[k - 1].1 + step;
                }
            }
            for (i, a) in ang {
                let (c, s) = (a.cos(), a.sin());
                // On the box: scale the direction to touch rx / ry.
                let t = (rx / c.abs().max(1e-9)).min(ry / s.abs().max(1e-9));
                out[i] = [mid[0] + c * t, mid[1] + s * t];
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Auto Balloon placement: a row above, a column beside, a ring around;
    /// balloons never closer than the gap.
    #[test]
    fn balloon_placement() {
        let picks: Vec<(String, [f64; 2])> = [[10.0, 10.0], [11.0, 20.0], [30.0, 5.0]]
            .iter()
            .map(|p| (String::new(), *p))
            .collect();
        let (lo, hi) = ([0.0, 0.0], [40.0, 30.0]);
        let row = place_balloons(&picks, lo, hi, 1, 10.0, 8.0);
        assert!(row.iter().all(|c| (c[1] - 40.0).abs() < 1e-9));
        let mut xs: Vec<f64> = row.iter().map(|c| c[0]).collect();
        xs.sort_by(f64::total_cmp);
        assert!(xs.windows(2).all(|w| w[1] - w[0] >= 8.0 - 1e-9));
        let col = place_balloons(&picks, lo, hi, 2, 10.0, 8.0);
        assert!(col.iter().all(|c| (c[0] - 50.0).abs() < 1e-9));
        let ring = place_balloons(&picks, lo, hi, 0, 10.0, 8.0);
        for c in &ring {
            let outside = c[0] <= lo[0] - 9.9
                || c[0] >= hi[0] + 9.9
                || c[1] <= lo[1] - 9.9
                || c[1] >= hi[1] + 9.9;
            assert!(outside, "{c:?}");
        }
    }
}
