//! Assemble › Constrain (`docs/spec/3.1-assemblies.md` §3.2), step A2: the
//! Place Constraint panel (type Mate / Angle / Insert, Mate's Mate / Flush
//! solution, Insert's Opposed / Aligned, two selections, the offset or
//! angle as an expression field, Predict Offset and Orientation, Show
//! Preview, Apply / OK / Cancel), picking faces and edges of components,
//! solving after every change (a constraint that conflicts is refused and
//! changes nothing, rule 5), the Relationships folder of the browser and a
//! constraint's Properties.

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke};

use super::CadCraftApp;
use super::valuefield::ExprValue;
use crate::assembly::relation::{self, Constraint, ConstraintKind, Geom, GeomRef};
use crate::assembly::{self as asm, Solved};
use crate::document::{AssemblyDocument, DocRef};
use crate::kernel::{Point3, Real, Transform3};
use crate::part::EdgeRef;

/// The straight and circular edges of a part (part coordinates), for
/// picking them in an assembly.
pub(super) type EdgeList = Vec<(EdgeRef, Vec<Point3>)>;

/// The open Constrain command.
#[derive(Clone, Debug)]
pub(super) struct ConstrainDraft {
    /// Mate (or its Flush solution), Angle, Insert.
    kind: ConstraintKind,
    a: Option<GeomRef>,
    b: Option<GeomRef>,
    /// Which selection the next click fills (0 or 1).
    active: usize,
    value: Real,
    expr: Option<String>,
    /// The user typed a value (Predict leaves it alone).
    typed: bool,
    aligned: bool,
    predict: bool,
    preview: bool,
    /// Edit of an existing constraint.
    edit: Option<u32>,
}

impl ConstrainDraft {
    fn new() -> Self {
        Self {
            kind: ConstraintKind::Mate,
            a: None,
            b: None,
            active: 0,
            value: 0.0,
            expr: None,
            typed: false,
            aligned: false,
            predict: true,
            preview: true,
            edit: None,
        }
    }

    fn constraint(&self) -> Option<Constraint> {
        Some(Constraint {
            id: 0,
            name: String::new(),
            kind: self.kind,
            a: self.a.clone()?,
            b: self.b.clone()?,
            value: self.value,
            expr: self.expr.clone(),
            aligned: self.aligned,
            suppressed: false,
        })
    }

    /// Whether `g` is one of its selections.
    pub(super) fn has(&self, g: &GeomRef) -> bool {
        self.a.as_ref() == Some(g) || self.b.as_ref() == Some(g)
    }

    fn command(&self) -> &'static str {
        match self.kind {
            ConstraintKind::Mate | ConstraintKind::Flush => "Mate",
            ConstraintKind::Angle => "Angle",
            ConstraintKind::Insert => "Insert",
        }
    }
}

/// The constraint state shown with the assembly: the last solve.
#[derive(Default)]
pub(super) struct ConstrainUi {
    pub draft: Option<ConstrainDraft>,
    /// The constraint selected in the browser.
    pub selected: Option<u32>,
    /// The last solve (sick constraints, degrees of freedom).
    pub last: Solved,
    /// Placements shown instead of the real ones (the draft's preview).
    pub preview: Option<HashMap<u32, Transform3>>,
    preview_key: String,
    /// Edges per component (by body signature).
    edges: std::cell::RefCell<HashMap<DocRef, (String, Arc<EdgeList>)>>,
    /// The geometry under the cursor while picking.
    hover: Option<GeomRef>,
    /// Origin / work geometry under the cursor in the browser.
    pub browser_hover: Option<GeomRef>,
}

const PICK_PX: f32 = 7.0;

impl CadCraftApp {
    /// A reference's geometry in its part's coordinates (parts of this
    /// file or linked part files).
    pub(super) fn asm_local_geom(&self, r: &GeomRef) -> Result<Geom, String> {
        if let GeomRef::Origin {
            occurrence: relation::ASSEMBLY,
            item,
        } = r
        {
            return Ok(item.geom());
        }
        let a = self.assembly().ok_or("no assembly")?;
        let o = a
            .find(r.occurrence())
            .ok_or("the component no longer exists")?;
        let name = o.name.clone();
        self.with_part(o.component, |p| relation::resolve_in(p, r))
            .ok_or_else(|| format!("{name}: its part file was not found"))?
            .map_err(|e| format!("{name}: {e}"))
    }

    /// Solves `a` as the active assembly would be (its references resolved
    /// in this document's components).
    fn solve_copy(&self, a: &AssemblyDocument, heavy: Option<u32>) -> Solved {
        let local = |r: &GeomRef| -> Result<Geom, String> {
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
        };
        asm::solve(a, &local, heavy)
    }

    /// Solves the active assembly and moves its components (no Undo step:
    /// call it after the change's checkpoint). `heavy` is being dragged.
    pub(super) fn asm_solve(&mut self, heavy: Option<u32>) {
        let Some(a) = self.assembly() else {
            return;
        };
        if a.constraints.is_empty() {
            self.cons.last = Solved::default();
            return;
        }
        let s = self.solve_copy(a, heavy);
        if let Some(a) = self.assembly_mut() {
            for (id, t) in &s.transforms {
                if let Some(o) = a.find_mut(*id) {
                    o.transform = *t;
                }
            }
        }
        self.cons.last = s;
        self.scene_dirty = true;
    }

    /// The constraints' state (sick ones, degrees of freedom) of the
    /// assembly as opened, without moving anything (the file stays
    /// unchanged).
    pub(super) fn asm_refresh_status(&mut self) {
        self.cons.last = match self.assembly() {
            Some(a) if !a.constraints.is_empty() => self.solve_copy(a, None),
            _ => Solved::default(),
        };
    }

    /// Changes the constraints through `f` on a copy; only when the result
    /// solves does it become the assembly (with an Undo step `label`).
    pub(super) fn asm_try(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut AssemblyDocument),
    ) -> Result<(), String> {
        let mut a = self.assembly().ok_or("no assembly")?.clone();
        f(&mut a);
        let s = self.solve_copy(&a, None);
        if !s.converged && !a.constraints.iter().all(|c| c.suppressed) {
            return Err(
                "it conflicts with the existing relationships (nothing was changed)".into(),
            );
        }
        for (id, t) in &s.transforms {
            if let Some(o) = a.find_mut(*id) {
                o.transform = *t;
            }
        }
        self.checkpoint(label);
        if let Some(cur) = self.assembly_mut() {
            *cur = a;
        }
        self.cons.last = s;
        self.scene_dirty = true;
        Ok(())
    }

    // ---- the command -----------------------------------------------------------

    /// Assemble › Constrain.
    pub(super) fn start_constrain(&mut self) {
        self.asm.tool = None;
        self.cons.draft = Some(ConstrainDraft::new());
        self.cons.preview = None;
    }

    /// Edit of constraint `id` in the panel.
    pub(super) fn edit_constraint(&mut self, id: u32) {
        let Some(c) = self.assembly().and_then(|a| a.constraint(id)).cloned() else {
            return;
        };
        let mut d = ConstrainDraft::new();
        d.kind = c.kind;
        d.a = Some(c.a);
        d.b = Some(c.b);
        d.value = c.value;
        d.expr = c.expr;
        d.typed = true;
        d.aligned = c.aligned;
        d.edit = Some(id);
        self.asm.tool = None;
        self.cons.draft = Some(d);
    }

    pub(super) fn cancel_constrain(&mut self) {
        self.cons.draft = None;
        self.cons.preview = None;
        self.cons.hover = None;
        self.scene_dirty = true;
    }

    /// Apply (keep the panel open for the next one) or OK.
    fn commit_constrain(&mut self, close: bool) {
        let Some(d) = self.cons.draft.clone() else {
            return;
        };
        let Some(c) = d.constraint() else {
            self.file_status = Some(Err(format!("{}: pick both selections first", d.command())));
            return;
        };
        let r = match d.edit {
            Some(id) => self.asm_try("Edit Constraint", |a| {
                if let Some(x) = a.constraint_mut(id) {
                    let (keep_id, keep_name) = (x.id, x.name.clone());
                    *x = Constraint {
                        id: keep_id,
                        name: keep_name,
                        ..c
                    };
                }
            }),
            None => self.asm_try("Constrain", |a| {
                a.add_constraint(c);
            }),
        };
        match r {
            Ok(()) => {
                self.cons.preview = None;
                if close || d.edit.is_some() {
                    self.cons.draft = None;
                } else if let Some(x) = self.cons.draft.as_mut() {
                    // Apply: the next constraint of the same type.
                    *x = ConstrainDraft {
                        kind: x.kind,
                        ..ConstrainDraft::new()
                    };
                }
            }
            Err(e) => self.file_status = Some(Err(format!("{}: {e}", d.command()))),
        }
    }

    /// [`Self::commit_constrain`] for the view's menu.
    pub(super) fn commit_constrain_pub(&mut self, close: bool) {
        self.commit_constrain(close);
    }

    /// The edges of a component (cached).
    fn asm_edges(&self, r: DocRef) -> Option<Arc<EdgeList>> {
        let sig = self.with_part(r, |p| p.body_signature())?;
        if let Some((k, v)) = self.cons.edges.borrow().get(&r)
            && *k == sig
        {
            return Some(v.clone());
        }
        let list = self.with_part(r, |p| {
            let body = &p.body;
            let opts = crate::kernel::TessellationOptions::default();
            body.edges()
                .filter(|(_, e)| {
                    matches!(
                        body.curve(e.curve),
                        Some(crate::kernel::Curve::Line(_) | crate::kernel::Curve::Circle(_))
                    )
                })
                .filter_map(|(e, _)| {
                    let r = crate::part::blend::edge_ref(body, e)?;
                    Some((r, body.sample_edge(e, &opts).ok()?))
                })
                .collect::<EdgeList>()
        })?;
        let list = Arc::new(list);
        self.cons.edges.borrow_mut().insert(r, (sig, list.clone()));
        Some(list)
    }

    /// The face or edge of a component under the cursor (an edge within a
    /// few pixels wins over the face behind it).
    fn geom_under(&self, pos: Pos2, rect: Rect) -> Option<GeomRef> {
        let a = self.assembly()?;
        let face = self.asm_pick_pub(pos, rect);
        let (o, dir) = self.camera.ray(pos, rect)?;
        let dir = dir.try_normalize(1e-12)?;
        let mut best: Option<(f32, GeomRef)> = None;
        for occ in a.occurrences.iter().filter(|o| o.visible) {
            let Some(edges) = self.asm_edges(occ.component) else {
                continue;
            };
            for (r, pts) in edges.iter() {
                for w in pts.windows(2) {
                    let (p, q) = (occ.transform * w[0], occ.transform * w[1]);
                    let (Some(sp), Some(sq)) =
                        (self.camera.project(&p, rect), self.camera.project(&q, rect))
                    else {
                        continue;
                    };
                    let (d, s) = seg_dist(pos, sp, sq);
                    if d > PICK_PX || best.as_ref().is_some_and(|b| b.0 <= d) {
                        continue;
                    }
                    // Hidden behind a face nearer the eye?
                    let at = p + (q - p) * s as Real;
                    let t_edge = (at - o).dot(&dir);
                    let size = (q - p).norm().max(1.0);
                    if face.is_some_and(|(h, _)| h.t + 1e-3 * size.max(10.0) < t_edge) {
                        continue;
                    }
                    best = Some((
                        d,
                        GeomRef::Edge {
                            occurrence: occ.id,
                            edge: r.clone(),
                        },
                    ));
                }
            }
        }
        if let Some((_, g)) = best {
            return Some(g);
        }
        let (h, world) = face?;
        let occ = a.find(h.occurrence)?;
        let name = self.with_part(occ.component, |p| {
            p.body.face(h.face).and_then(|f| f.name.clone())
        })??;
        let local = occ.transform.inverse() * world;
        Some(GeomRef::Face {
            occurrence: occ.id,
            name,
            hint: [local.x, local.y, local.z],
        })
    }

    /// A click on `g` fills the active selection (both on different
    /// components), then predicts the offset and the solution.
    pub(super) fn constrain_pick(&mut self, g: GeomRef) {
        let geom = self.asm_local_geom(&g);
        let Some(d) = self.cons.draft.as_mut() else {
            return;
        };
        if let Ok(x) = &geom
            && !relation::accepts(d.kind, x)
        {
            self.file_status = Some(Err(match d.kind {
                ConstraintKind::Insert => "Insert: pick circular edges".into(),
                _ => format!("{}: that geometry does not fit", d.command()),
            }));
            return;
        }
        let other = if d.active == 0 { &d.b } else { &d.a };
        if other
            .as_ref()
            .is_some_and(|o| o.occurrence() == g.occurrence())
        {
            self.file_status = Some(Err(format!(
                "{}: pick the second selection on another component",
                d.command()
            )));
            return;
        }
        if d.active == 0 {
            d.a = Some(g);
            d.active = 1;
        } else {
            d.b = Some(g);
        }
        self.constrain_predict();
    }

    /// Predict Orientation: Mate or Flush from the normals. The offset is
    /// the distance as it is now when *Use the current distance* is on
    /// (remembered between sessions), else 0 (the parts snap together); an
    /// angle is always measured. A typed value is left alone.
    fn constrain_predict(&mut self) {
        let Some(d) = self.cons.draft.clone() else {
            return;
        };
        if d.typed {
            return;
        }
        let keep = self.constrain_current_offset;
        let (Some(a), Some(b)) = (&d.a, &d.b) else {
            return;
        };
        let Some(asm_doc) = self.assembly() else {
            return;
        };
        let world = |r: &GeomRef| -> Option<Geom> {
            let t = asm_doc.find(r.occurrence())?.transform;
            Some(self.asm_local_geom(r).ok()?.moved(&t))
        };
        let (Some(ga), Some(gb)) = (world(a), world(b)) else {
            return;
        };
        let (kind, value) = relation::predict(d.kind, &ga, &gb);
        if let Some(x) = self.cons.draft.as_mut() {
            if x.predict && matches!(x.kind, ConstraintKind::Mate | ConstraintKind::Flush) {
                x.kind = kind;
            }
            x.value = if keep || x.kind == ConstraintKind::Angle {
                value
            } else {
                0.0
            };
            x.expr = None;
        }
    }

    /// The draft's preview: the assembly solved with it (recomputed when
    /// the draft changes).
    fn constrain_preview(&mut self) {
        let Some(d) = self.cons.draft.clone() else {
            return;
        };
        let key = format!("{d:?}");
        if key == self.cons.preview_key {
            return;
        }
        self.cons.preview_key = key;
        self.cons.preview = None;
        self.scene_dirty = true;
        let (true, Some(c)) = (d.preview, d.constraint()) else {
            return;
        };
        let Some(mut a) = self.assembly().cloned() else {
            return;
        };
        match d.edit {
            Some(id) => {
                if let Some(x) = a.constraint_mut(id) {
                    *x = Constraint {
                        id,
                        name: x.name.clone(),
                        ..c
                    };
                }
            }
            None => {
                a.add_constraint(c);
            }
        }
        let s = self.solve_copy(&a, None);
        self.cons.preview = Some(s.transforms.into_iter().collect());
    }

    /// The view while Constrain is open: hover and pick geometry, draw the
    /// selections. Returns `true` when it took the click.
    pub(super) fn constrain_view(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        pointer: Option<Pos2>,
        clicked: bool,
    ) -> bool {
        if self.cons.draft.is_none() {
            return false;
        }
        self.constrain_preview();
        self.cons.hover = pointer.and_then(|p| self.geom_under(p, rect));
        if clicked && let Some(g) = self.cons.hover.clone() {
            self.constrain_pick(g);
        }
        let (a, b) = match &self.cons.draft {
            Some(d) => (d.a.clone(), d.b.clone()),
            None => (None, None),
        };
        for (g, col) in [
            (a, Color32::from_rgb(80, 160, 255)),
            (b, Color32::from_rgb(200, 110, 255)),
            (self.cons.hover.clone(), Color32::from_rgb(110, 220, 120)),
        ] {
            if let Some(g) = g {
                self.draw_geom(painter, rect, &g, col);
            }
        }
        clicked
    }

    /// Outlines a picked face (its triangles), edge (its polyline), or an
    /// origin / work plane (a square), axis (a line) or point (a ring).
    pub(super) fn draw_geom(&self, painter: &egui::Painter, rect: Rect, g: &GeomRef, col: Color32) {
        let Some(a) = self.assembly() else {
            return;
        };
        if matches!(g, GeomRef::Origin { .. } | GeomRef::Work { .. }) {
            self.draw_reference(painter, rect, g, col);
            return;
        }
        let Some(occ) = a.find(g.occurrence()) else {
            return;
        };
        let t = self
            .cons
            .preview
            .as_ref()
            .and_then(|p| p.get(&occ.id).copied())
            .unwrap_or(occ.transform);
        match g {
            GeomRef::Edge { edge, .. } => {
                let Some(edges) = self.asm_edges(occ.component) else {
                    return;
                };
                if let Some((_, pts)) = edges.iter().find(|(r, _)| r == edge) {
                    let line: Vec<Pos2> = pts
                        .iter()
                        .filter_map(|p| self.camera.project(&(t * p), rect))
                        .collect();
                    painter.add(egui::Shape::line(line, Stroke::new(3.0, col)));
                }
            }
            GeomRef::Face { name, .. } => {
                let Some(mesh) = self.asm_mesh(occ.component) else {
                    return;
                };
                let ids: Vec<crate::kernel::FaceId> = self
                    .with_part(occ.component, |p| {
                        p.body
                            .faces()
                            .filter(|(_, f)| f.name.as_deref() == Some(name.as_str()))
                            .map(|(id, _)| id)
                            .collect()
                    })
                    .unwrap_or_default();
                let fill = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 90);
                let mut m = egui::Mesh::default();
                for (f, [p, q, r]) in mesh.triangles() {
                    if !ids.contains(f) {
                        continue;
                    }
                    let (Some(sp), Some(sq), Some(sr)) = (
                        self.camera.project(&(t * p), rect),
                        self.camera.project(&(t * q), rect),
                        self.camera.project(&(t * r), rect),
                    ) else {
                        continue;
                    };
                    let i = m.vertices.len() as u32;
                    for s in [sp, sq, sr] {
                        m.colored_vertex(s, fill);
                    }
                    m.add_triangle(i, i + 1, i + 2);
                }
                painter.add(egui::Shape::mesh(m));
            }
            GeomRef::Origin { .. } | GeomRef::Work { .. } => {}
        }
    }

    /// The status-bar prompt while Constrain is open.
    pub(super) fn constrain_prompt(&self) -> Option<String> {
        let d = self.cons.draft.as_ref()?;
        let what = match d.kind {
            ConstraintKind::Insert => "a circular edge",
            ConstraintKind::Angle => "a planar face or a straight edge",
            _ => "a face or an edge",
        };
        Some(if d.a.is_none() || d.active == 0 {
            format!(
                "{}: pick the first selection, {what} of a component",
                d.command()
            )
        } else if d.b.is_none() {
            format!(
                "{}: pick the second selection, {what} of another component",
                d.command()
            )
        } else {
            format!(
                "{}: set the offset, then OK or Apply (Esc cancels)",
                d.command()
            )
        })
    }

    // ---- panel -----------------------------------------------------------------

    /// The Place Constraint panel in Properties.
    pub(super) fn constrain_panel(&mut self, ui: &mut egui::Ui) {
        let Some(mut d) = self.cons.draft.clone() else {
            return;
        };
        let title = if d.edit.is_some() {
            "Edit Constraint"
        } else {
            "Place Constraint"
        };
        ui.label(RichText::new(title).strong());
        ui.separator();
        ui.label("Type");
        ui.horizontal(|ui| {
            for (label, kind) in [
                ("Mate", ConstraintKind::Mate),
                ("Angle", ConstraintKind::Angle),
                ("Insert", ConstraintKind::Insert),
            ] {
                let on = d.kind == kind
                    || (kind == ConstraintKind::Mate && d.kind == ConstraintKind::Flush);
                if ui.selectable_label(on, label).clicked() && !on {
                    d.kind = kind;
                    d.a = None;
                    d.b = None;
                    d.active = 0;
                    d.typed = false;
                    d.value = 0.0;
                }
            }
        });
        ui.add_space(4.0);
        ui.label("Selections");
        for (k, (label, g)) in [("1", d.a.clone()), ("2", d.b.clone())]
            .into_iter()
            .enumerate()
        {
            let text = match &g {
                Some(g) => format!("{label}  {}", self.geom_label(g)),
                None => format!("{label}  (pick)"),
            };
            if ui
                .selectable_label(d.active == k, text)
                .on_hover_text("Click, then pick in the view")
                .clicked()
            {
                d.active = k;
            }
        }
        ui.add_space(4.0);
        let angle = d.kind == ConstraintKind::Angle;
        ui.horizontal(|ui| {
            ui.label(if angle { "Angle" } else { "Offset" });
            let mut v = if angle { d.value.to_degrees() } else { d.value };
            let before = v;
            ui.add(
                ExprValue::new(&mut v)
                    .suffix(if angle { "°" } else { " mm" })
                    .equation(&mut d.expr),
            );
            if v != before {
                d.value = if angle { v.to_radians() } else { v };
                d.typed = true;
            }
        });
        ui.add_space(4.0);
        ui.label("Solution");
        ui.horizontal(|ui| match d.kind {
            ConstraintKind::Mate | ConstraintKind::Flush => {
                for (label, k) in [
                    ("Mate", ConstraintKind::Mate),
                    ("Flush", ConstraintKind::Flush),
                ] {
                    if ui.selectable_label(d.kind == k, label).clicked() {
                        d.kind = k;
                    }
                }
            }
            ConstraintKind::Insert => {
                if ui.selectable_label(!d.aligned, "Opposed").clicked() {
                    d.aligned = false;
                }
                if ui.selectable_label(d.aligned, "Aligned").clicked() {
                    d.aligned = true;
                }
            }
            ConstraintKind::Angle => {
                ui.label("Undirected");
            }
        });
        ui.add_space(4.0);
        ui.checkbox(&mut d.preview, "Show Preview");
        if ui
            .checkbox(&mut d.predict, "Predict Orientation")
            .on_hover_text("Mate or Flush from the faces' normals as they are now")
            .changed()
        {
            d.typed = false;
        }
        let mut keep = self.constrain_current_offset;
        if ui
            .checkbox(&mut keep, "Use the current distance")
            .on_hover_text(
                "On: the offset is the distance the components have now. Off: they snap together at 0. Remembered for every constraint.",
            )
            .changed()
        {
            self.constrain_current_offset = keep;
            d.typed = false;
        }
        ui.separator();
        let mut action = None;
        ui.horizontal(|ui| {
            let ready = d.a.is_some() && d.b.is_some();
            let ok = ui.add_enabled(ready, egui::Button::new("OK"));
            if ok.clicked() || super::keys::panel_ok_key(ui, ready) {
                action = Some(true);
            }
            if d.edit.is_none() && ui.add_enabled(ready, egui::Button::new("Apply")).clicked() {
                action = Some(false);
            }
            if ui.button("Cancel").clicked() {
                self.cancel_constrain();
            }
        });
        if self.cons.draft.is_some() {
            let repredict = !d.typed;
            self.cons.draft = Some(d);
            if repredict {
                self.constrain_predict();
            }
        }
        if let Some(close) = action {
            self.commit_constrain(close);
        }
    }

    pub(super) fn geom_label(&self, g: &GeomRef) -> String {
        let occ = if g.occurrence() == relation::ASSEMBLY {
            "Assembly".to_owned()
        } else {
            self.assembly()
                .and_then(|a| a.find(g.occurrence()))
                .map_or("?".to_owned(), |o| o.name.clone())
        };
        match g {
            GeomRef::Face { .. } => format!("{occ} face"),
            GeomRef::Edge { .. } => format!("{occ} edge"),
            GeomRef::Origin { item, .. } => format!("{occ} {}", item.label()),
            GeomRef::Work { id, .. } => format!("{occ} {id}"),
        }
    }

    /// An origin or work plane / axis / point drawn where it is in the
    /// assembly, sized by its component.
    fn draw_reference(&self, painter: &egui::Painter, rect: Rect, g: &GeomRef, col: Color32) {
        let Ok(local) = self.asm_local_geom(g) else {
            return;
        };
        let (t, size) = if g.occurrence() == relation::ASSEMBLY {
            let size = self
                .assembly_box()
                .map_or(60.0, |(lo, hi)| (hi - lo).norm() * 0.4);
            (Transform3::identity(), size.max(20.0))
        } else {
            let Some(occ) = self.assembly().and_then(|a| a.find(g.occurrence())) else {
                return;
            };
            let t = self
                .cons
                .preview
                .as_ref()
                .and_then(|p| p.get(&occ.id).copied())
                .unwrap_or(occ.transform);
            let size = self
                .asm_mesh(occ.component)
                .and_then(|m| m.bbox)
                .map_or(40.0, |(lo, hi)| (hi - lo).norm() * 0.6);
            (t, size.max(10.0))
        };
        let geom = local.moved(&t);
        let stroke = Stroke::new(2.0, col);
        let fill = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 50);
        let pr = |p: Point3| self.camera.project(&p, rect);
        match geom {
            Geom::Plane { p, n } => {
                let u = if n.x.abs() < 0.9 {
                    n.cross(&crate::kernel::Vector3::x())
                } else {
                    n.cross(&crate::kernel::Vector3::y())
                }
                .normalize();
                let v = n.cross(&u);
                let h = size / 2.0;
                let pts: Vec<Pos2> = [(-h, -h), (h, -h), (h, h), (-h, h)]
                    .iter()
                    .filter_map(|&(a, b)| pr(p + u * a + v * b))
                    .collect();
                if pts.len() == 4 {
                    painter.add(egui::Shape::convex_polygon(pts, fill, stroke));
                }
            }
            Geom::Axis { p, d } => {
                if let (Some(a), Some(b)) = (pr(p - d * size / 2.0), pr(p + d * size / 2.0)) {
                    painter.line_segment([a, b], stroke);
                }
            }
            Geom::Point { p } | Geom::Circle { c: p, .. } => {
                if let Some(s) = pr(p) {
                    painter.circle(s, 5.0, fill, stroke);
                }
            }
        }
    }

    // ---- browser and Properties ----------------------------------------------

    pub(super) fn toggle_constraint_suppress(&mut self, id: u32) {
        let r = self.asm_try("Suppress Constraint", |a| {
            if let Some(c) = a.constraint_mut(id) {
                c.suppressed = !c.suppressed;
            }
        });
        if let Err(e) = r {
            self.file_status = Some(Err(format!("Suppress: {e}")));
        }
    }

    pub(super) fn delete_constraint(&mut self, id: u32) {
        let _ = self.asm_try("Delete Constraint", |a| {
            a.constraints.retain(|c| c.id != id)
        });
        if self.cons.selected == Some(id) {
            self.cons.selected = None;
        }
    }

    /// Properties of the constraint selected in the browser; `false` when
    /// there is none.
    pub(super) fn constraint_properties(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(c) = self
            .cons
            .selected
            .and_then(|id| self.assembly()?.constraint(id).cloned())
        else {
            return false;
        };
        ui.label(RichText::new(&c.name).strong());
        ui.label(format!("{}: {}", self.geom_label(&c.a), c.a.label()));
        ui.label(format!("{}: {}", self.geom_label(&c.b), c.b.label()));
        if let Some((_, e)) = self.cons.last.sick.iter().find(|(id, _)| *id == c.id) {
            ui.colored_label(Color32::from_rgb(235, 80, 70), format!("Sick: {e}"));
        }
        let angle = c.kind == ConstraintKind::Angle;
        let mut v = if angle { c.value.to_degrees() } else { c.value };
        let mut expr = c.expr.clone();
        let before = v;
        ui.horizontal(|ui| {
            ui.label(if angle { "Angle" } else { "Offset" });
            ui.add(
                ExprValue::new(&mut v)
                    .suffix(if angle { "°" } else { " mm" })
                    .equation(&mut expr),
            );
        });
        if v != before {
            let value = if angle { v.to_radians() } else { v };
            if let Err(e) = self.asm_try("Edit Constraint", |a| {
                if let Some(x) = a.constraint_mut(c.id) {
                    x.value = value;
                    x.expr = expr;
                }
            }) {
                self.file_status = Some(Err(format!("{}: {e}", c.kind.label())));
            }
        }
        let mut s = c.suppressed;
        if ui.checkbox(&mut s, "Suppress").changed() {
            self.toggle_constraint_suppress(c.id);
        }
        ui.horizontal(|ui| {
            if ui.button("Edit...").clicked() {
                self.edit_constraint(c.id);
            }
            if ui.button("Delete").clicked() {
                self.delete_constraint(c.id);
            }
        });
        true
    }
}

/// Distance from `p` to the segment `a`–`b` (screen) and the parameter of
/// the nearest point.
fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> (f32, f32) {
    let ab = b - a;
    let len2 = ab.length_sq();
    let s = if len2 < 1e-6 {
        0.0
    } else {
        ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
    };
    ((a + ab * s - p).length(), s)
}
