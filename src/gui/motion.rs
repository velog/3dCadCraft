//! Motion and contact tools of an assembly (spec 3.1b; `assembly::motion`): Activate
//! Contact Solver, Contact Sets, Motion Links, Motion Studies and Exploded Views, in one
//! window with a tab per tool. Adapted from SolidCraft's motion dialogs.

use std::collections::HashMap;

use eframe::egui::{self, RichText};

use super::CadCraftApp;
use super::valuefield::ExprValue;
use crate::assembly::motion::{
    self, ContactSet, ExplodedView, LinkKind, MotionData, MotionLink, MotionStudy, StudyKey,
};
use crate::assembly::relation::{self, Constraint, ConstraintKind, Geom};
use crate::kernel::{Point3, Real, Transform3, Vector3};

/// The window's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MotionTab {
    /// Motion Links.
    #[default]
    Links,
    /// Contact Solver and Contact Sets.
    Contact,
    /// Motion Studies.
    Study,
    /// Exploded Views.
    Exploded,
}

/// A running motion study.
#[derive(Clone, Debug)]
struct Playing {
    study: u32,
    step: Real,
    /// +1 forward, -1 back, 0 stopped.
    dir: i32,
    looping: bool,
    /// Steps per second.
    speed: Real,
    last: f64,
    /// Values, locks and places to put back.
    saved: Saved,
}

/// What the study or explode changed, to put back.
#[derive(Clone, Debug, Default)]
struct Saved {
    values: Vec<(u32, Real, bool)>,
    places: Vec<(u32, Transform3)>,
}

/// The window's state (nothing of it is saved).
#[derive(Debug, Default)]
pub struct MotionUi {
    /// The window is open.
    pub open: bool,
    /// Its tab.
    pub tab: MotionTab,
    /// New link: driver, driven, kind, ratio (shown form), reverse.
    link: (Option<u32>, Option<u32>, LinkKind, Real, bool),
    /// New set's name.
    set_name: String,
    /// The study edited.
    study: Option<u32>,
    playing: Option<Playing>,
    /// Exploded view shown and how far (0..=1).
    shown: Option<(u32, Real)>,
    /// Auto Explode's scale.
    scale: Real,
    /// A manual move: x, y, z of the selected components.
    delta: [Real; 3],
    /// New key's step and value (shown units).
    key: (Real, Real),
    /// Places an exploded view draws components at (`assembly_scene`).
    pub display: HashMap<u32, Transform3>,
}

impl CadCraftApp {
    /// Opens the window on `tab`.
    pub(super) fn open_motion(&mut self, tab: MotionTab) {
        self.motion.open = true;
        self.motion.tab = tab;
        if self.motion.scale == 0.0 {
            self.motion.scale = 1.0;
            self.motion.link.3 = 1.0;
        }
    }

    /// Activate Contact Solver, on or off.
    pub(super) fn toggle_contact_solver(&mut self) {
        let on = self.assembly().is_some_and(|a| !a.motion.contact_enabled);
        self.edit_assembly("Contact Solver", |a| a.motion.contact_enabled = on);
        self.file_status = Some(Ok(if on {
            "Contact Solver: on (components in contact sets stop each other)".to_owned()
        } else {
            "Contact Solver: off".to_owned()
        }));
    }

    /// The value a relationship has as the components stand.
    pub(super) fn relation_value(&self, c: &Constraint) -> Option<Real> {
        if c.kind.is_joint() || self.is_axis_mate_refs(c.kind, Some(&c.a), Some(&c.b)) {
            return self.relation_turn(c);
        }
        let asm = self.assembly()?;
        let world = |r: &relation::GeomRef| -> Option<Geom> {
            let t = match r.occurrence() {
                relation::ASSEMBLY => Transform3::identity(),
                o => asm.find(o)?.transform,
            };
            Some(self.asm_local_geom(r).ok()?.moved(&t))
        };
        relation::measure(c, &world(&c.a)?, &world(&c.b)?)
    }

    /// Whether a relationship's value is an angle.
    pub(super) fn relation_is_angle(&self, c: &Constraint) -> bool {
        matches!(c.kind, ConstraintKind::Angle | ConstraintKind::Rotational)
            || self.is_axis_mate_refs(c.kind, Some(&c.a), Some(&c.b))
    }

    /// After a solve: each link's driven relationship follows its driver; dragging the
    /// driven side turns the link round. No Undo step (part of the change it follows).
    pub(super) fn motion_links_follow(&mut self, drag: Option<&crate::assembly::Drag>) {
        for _ in 0..4 {
            let Some(a) = self.assembly() else {
                return;
            };
            if a.motion.links.is_empty() {
                return;
            }
            let links = a.motion.links.clone();
            let mut changes: Vec<(u32, Real, Option<u32>)> = Vec::new();
            let mut lasts: Vec<(u32, Real)> = Vec::new();
            for l in links.iter().filter(|l| !l.suppressed) {
                let (Some(driver), Some(driven)) = (
                    a.constraint(l.driver).cloned(),
                    a.constraint(l.driven).cloned(),
                ) else {
                    continue;
                };
                // Dragging the driven side: it leads, and the driver follows.
                let turned =
                    drag.is_some_and(|d| driven.uses(d.occurrence) && !driver.uses(d.occurrence));
                if turned {
                    let Some(now) = self.relation_value(&driven) else {
                        continue;
                    };
                    if let Some(v) = l.driver_value(now) {
                        changes.push((l.driver, v, Some(l.driven)));
                    }
                } else {
                    let Some(now) = self.relation_value(&driver) else {
                        continue;
                    };
                    let v = if self.relation_is_angle(&driver) {
                        motion::unwrapped(l.last, now)
                    } else {
                        now
                    };
                    lasts.push((l.id, v));
                    changes.push((l.driven, l.driven_value(v), Some(l.driver)));
                }
            }
            let mut moved = false;
            if let Some(a) = self.assembly_mut() {
                for (id, v) in lasts {
                    if let Some(l) = a.motion.links.iter_mut().find(|l| l.id == id) {
                        l.last = Some(v);
                    }
                }
                for (target, v, free) in changes {
                    // The side that follows is held; the leading one is free.
                    if let Some(c) = free.and_then(|f| a.constraint_mut(f))
                        && c.kind.is_joint()
                    {
                        c.locked = false;
                    }
                    if let Some(c) = a.constraint_mut(target)
                        && (!c.locked || (c.value - v).abs() > 1e-9)
                    {
                        c.locked = true;
                        c.value = v;
                        moved = true;
                    }
                }
            }
            if !moved {
                return;
            }
            self.asm_solve_once(None);
        }
    }

    /// Places of the occurrences now.
    fn places(&self) -> Vec<(u32, Transform3)> {
        self.assembly()
            .map(|a| a.occurrences.iter().map(|o| (o.id, o.transform)).collect())
            .unwrap_or_default()
    }

    /// The first pair in contact that passes through each other, by name.
    pub(super) fn contact_hit(&self) -> Option<(String, String)> {
        let a = self.assembly()?;
        let ids: Vec<u32> = a
            .occurrences
            .iter()
            .filter(|o| !o.suppressed)
            .map(|o| o.id)
            .collect();
        for (x, y) in a.motion.contact_pairs(&ids) {
            let (ox, oy) = (a.find(x)?, a.find(y)?);
            let (Some(mx), Some(my)) = (self.asm_mesh(ox.component), self.asm_mesh(oy.component))
            else {
                continue;
            };
            let tol = motion::contact_tolerance(&mx, &my);
            if motion::mesh_overlap(&mx, &ox.transform, &my, &oy.transform, tol).is_some() {
                return Some((ox.name.clone(), oy.name.clone()));
            }
        }
        None
    }

    /// A drag moved things: where contact is broken they go back to `before` (they
    /// stop at the last place that was free). An assembly already overlapping may
    /// move out of it (`was_hit`).
    pub(super) fn contact_guard(&mut self, before: &[(u32, Transform3)], was_hit: bool) {
        if was_hit {
            return;
        }
        if let Some((x, y)) = self.contact_hit() {
            if let Some(a) = self.assembly_mut() {
                for (id, t) in before {
                    if let Some(o) = a.find_mut(*id) {
                        o.transform = *t;
                    }
                }
            }
            self.scene_dirty = true;
            self.file_status = Some(Ok(format!("Contact: {x} stops at {y}")));
        }
    }

    /// Sets `id`'s value towards `value`, stopping short of a collision (bisection from
    /// where it is, after SolidCraft's `drive_with_contact`). Returns the pair that
    /// stopped it.
    pub(super) fn drive_with_contact(
        &mut self,
        id: u32,
        from: Real,
        value: Real,
    ) -> Option<(String, String)> {
        let set = |app: &mut Self, v: Real| {
            if let Some(c) = app.assembly_mut().and_then(|a| a.constraint_mut(id)) {
                c.value = v;
                c.locked = true;
            }
            app.asm_solve(None);
        };
        let started_free = self.contact_hit().is_none();
        set(self, value);
        if !started_free {
            return None;
        }
        let hit = self.contact_hit()?;
        let (mut lo, mut hi) = (from, value);
        for _ in 0..10 {
            let mid = (lo + hi) / 2.0;
            set(self, mid);
            if self.contact_hit().is_none() {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        set(self, lo);
        Some(hit)
    }

    /// The relationships that have a value to move, for the lists.
    fn drivable(&self) -> Vec<(u32, String, bool)> {
        let Some(a) = self.assembly() else {
            return Vec::new();
        };
        a.constraints
            .iter()
            .filter(|c| !c.kind.is_joint() || c.kind.joint_value().is_some())
            .filter(|c| c.kind != ConstraintKind::Symmetry)
            .map(|c| (c.id, c.name.clone(), self.relation_is_angle(c)))
            .collect()
    }

    /// The window.
    pub(super) fn motion_window(&mut self, ctx: &egui::Context) {
        self.motion_play(ctx);
        if !self.motion.open {
            return;
        }
        if !self.is_assembly() {
            return;
        }
        let mut open = true;
        let win = egui::Window::new("Motion and Contact")
            .id(egui::Id::new("motion_window"))
            .open(&mut open)
            .resizable(true)
            .default_size([430.0, 420.0]);
        self.layout
            .place("motion", ctx.content_rect(), true, win)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (t, label) in [
                        (MotionTab::Links, "Motion Links"),
                        (MotionTab::Contact, "Contact"),
                        (MotionTab::Study, "Motion Study"),
                        (MotionTab::Exploded, "Exploded View"),
                    ] {
                        if ui.selectable_label(self.motion.tab == t, label).clicked() {
                            self.motion.tab = t;
                        }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| match self.motion.tab {
                    MotionTab::Links => self.links_tab(ui),
                    MotionTab::Contact => self.contact_tab(ui),
                    MotionTab::Study => self.study_tab(ui),
                    MotionTab::Exploded => self.exploded_tab(ui),
                });
            });
        if !open {
            self.motion.open = false;
            self.stop_study();
            self.show_exploded(None);
        }
    }

    fn data(&self) -> MotionData {
        self.assembly()
            .map(|a| a.motion.clone())
            .unwrap_or_default()
    }

    // ---- Motion Links ----------------------------------------------------------

    fn links_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new(
                "One relationship's value follows another's: gears (Rotation), rack and pinion or a screw (Rotation-Translation).",
            )
            .weak(),
        );
        let list = self.drivable();
        let name = |id: Option<u32>| {
            id.and_then(|i| list.iter().find(|x| x.0 == i))
                .map_or("Select".to_owned(), |x| x.1.clone())
        };
        let (mut driver, mut driven, mut kind, mut ratio, mut reverse) = self.motion.link;
        egui::Grid::new("motion_link_new")
            .num_columns(2)
            .show(ui, |ui| {
                for (label, slot) in [("Driver", &mut driver), ("Driven", &mut driven)] {
                    ui.label(label);
                    egui::ComboBox::from_id_salt(("motion_link", label))
                        .selected_text(name(*slot))
                        .show_ui(ui, |ui| {
                            for (id, n, _) in &list {
                                ui.selectable_value(slot, Some(*id), n);
                            }
                        });
                    ui.end_row();
                }
                ui.label("Type");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut kind, LinkKind::Rotation, "Rotation");
                    ui.selectable_value(
                        &mut kind,
                        LinkKind::RotationTranslation,
                        "Rotation-Translation",
                    );
                    ui.selectable_value(&mut kind, LinkKind::Linear, "Linear");
                });
                ui.end_row();
                ui.label(if kind == LinkKind::RotationTranslation {
                    "Distance per turn"
                } else {
                    "Ratio"
                });
                let suffix = if kind == LinkKind::RotationTranslation {
                    " mm"
                } else {
                    ""
                };
                ui.add(ExprValue::new(&mut ratio).suffix(suffix));
                ui.end_row();
                ui.label("Solution");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut reverse, false, "Forward");
                    ui.selectable_value(&mut reverse, true, "Reverse");
                });
                ui.end_row();
            });
        self.motion.link = (driver, driven, kind, ratio, reverse);
        let ready = driver.is_some() && driven.is_some() && driver != driven;
        if ui
            .add_enabled(ready, egui::Button::new("Add Link"))
            .clicked()
            && let (Some(x), Some(y)) = (driver, driven)
        {
            self.add_link(x, y, kind, ratio, reverse);
        }
        ui.separator();
        let mut remove = None;
        let mut toggle = None;
        for l in &self.data().links {
            ui.horizontal(|ui| {
                let label = format!(
                    "{} : {}  ×{:.4}",
                    name(Some(l.driver)),
                    name(Some(l.driven)),
                    l.ratio
                );
                let mut on = !l.suppressed;
                if ui.checkbox(&mut on, label).changed() {
                    toggle = Some((l.id, !on));
                }
                if ui
                    .small_button("x")
                    .on_hover_text("Delete the link")
                    .clicked()
                {
                    remove = Some(l.id);
                }
            });
        }
        if let Some((id, off)) = toggle {
            self.edit_assembly("Motion Link", |a| {
                if let Some(l) = a.motion.links.iter_mut().find(|l| l.id == id) {
                    l.suppressed = off;
                }
            });
        }
        if let Some(id) = remove {
            self.edit_assembly("Motion Link", |a| a.motion.links.retain(|l| l.id != id));
        }
    }

    fn add_link(&mut self, driver: u32, driven: u32, kind: LinkKind, ratio: Real, reverse: bool) {
        let Some(a) = self.assembly() else {
            return;
        };
        let (Some(cx), Some(cy)) = (a.constraint(driver).cloned(), a.constraint(driven).cloned())
        else {
            return;
        };
        let raw = if kind == LinkKind::RotationTranslation {
            ratio / std::f64::consts::TAU
        } else {
            ratio
        };
        let signed = if reverse { -raw } else { raw };
        // The driven side stays where it is at the driver's present value.
        let (vx, vy) = (
            self.relation_value(&cx).unwrap_or(cx.value),
            self.relation_value(&cy).unwrap_or(cy.value),
        );
        let link_id = a.motion.next_id();
        let link = MotionLink {
            id: link_id,
            driver,
            driven,
            kind,
            ratio: signed,
            offset: vy - signed * vx,
            suppressed: false,
            last: Some(vx),
        };
        self.edit_assembly("Motion Link", |a| a.motion.links.push(link));
        self.asm_solve(None);
        self.motion.link.0 = None;
        self.motion.link.1 = None;
    }

    // ---- Contact ---------------------------------------------------------------

    fn contact_tab(&mut self, ui: &mut egui::Ui) {
        let data = self.data();
        let mut on = data.contact_enabled;
        if ui
            .checkbox(&mut on, "Activate Contact Solver")
            .on_hover_text("Components in contact stop each other when dragged or driven")
            .changed()
        {
            self.edit_assembly("Contact Solver", |a| a.motion.contact_enabled = on);
        }
        let mut all = data.contact_all;
        if ui
            .checkbox(&mut all, "Every component (Enable All Contact)")
            .on_hover_text("All components instead of the contact sets below")
            .changed()
        {
            self.edit_assembly("Contact Solver", |a| a.motion.contact_all = all);
        }
        ui.separator();
        ui.label(RichText::new("Contact Sets").strong());
        let names: HashMap<u32, String> = self
            .assembly()
            .map(|a| {
                a.occurrences
                    .iter()
                    .map(|o| (o.id, o.name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let mut cmd: Option<(u32, SetCmd)> = None;
        for s in &data.contact_sets {
            ui.horizontal_wrapped(|ui| {
                let mut active = !s.suppressed;
                if ui.checkbox(&mut active, &s.name).changed() {
                    cmd = Some((s.id, SetCmd::Active(active)));
                }
                let list: Vec<&str> = s
                    .occurrences
                    .iter()
                    .filter_map(|o| names.get(o).map(String::as_str))
                    .collect();
                ui.label(RichText::new(list.join(", ")).weak());
                if ui
                    .small_button("+ selected")
                    .on_hover_text("Add the components selected in the view")
                    .clicked()
                {
                    cmd = Some((s.id, SetCmd::Add));
                }
                if ui
                    .small_button("- selected")
                    .on_hover_text("Take the selected components out")
                    .clicked()
                {
                    cmd = Some((s.id, SetCmd::Remove));
                }
                if ui
                    .small_button("x")
                    .on_hover_text("Delete the set")
                    .clicked()
                {
                    cmd = Some((s.id, SetCmd::Delete));
                }
            });
        }
        let selected = self.asm.sel.clone();
        if let Some((id, c)) = cmd {
            self.edit_assembly("Contact Set", |a| {
                let sets = &mut a.motion.contact_sets;
                match c {
                    SetCmd::Delete => sets.retain(|s| s.id != id),
                    SetCmd::Active(on) => {
                        if let Some(s) = sets.iter_mut().find(|s| s.id == id) {
                            s.suppressed = !on;
                        }
                    }
                    SetCmd::Add => {
                        if let Some(s) = sets.iter_mut().find(|s| s.id == id) {
                            for o in &selected {
                                if !s.occurrences.contains(o) {
                                    s.occurrences.push(*o);
                                }
                            }
                        }
                    }
                    SetCmd::Remove => {
                        if let Some(s) = sets.iter_mut().find(|s| s.id == id) {
                            s.occurrences.retain(|o| !selected.contains(o));
                        }
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.motion.set_name)
                    .hint_text("New set's name")
                    .desired_width(160.0),
            );
            if ui
                .add_enabled(selected.len() >= 2, egui::Button::new("New Set"))
                .on_hover_text("A contact set of the components selected in the view")
                .clicked()
            {
                let name = std::mem::take(&mut self.motion.set_name);
                let sel = selected.clone();
                self.edit_assembly("Contact Set", |a| {
                    let id = a.motion.next_id();
                    let name = if name.trim().is_empty() {
                        format!("Contact Set{}", a.motion.contact_sets.len() + 1)
                    } else {
                        name
                    };
                    a.motion.contact_sets.push(ContactSet {
                        id,
                        name,
                        occurrences: sel,
                        suppressed: false,
                    });
                });
            }
        });
        ui.separator();
        if ui
            .button("Check Now")
            .on_hover_text("Report the first pair of components passing through each other")
            .clicked()
        {
            self.file_status = Some(Ok(match self.contact_hit() {
                Some((x, y)) => format!("Contact: {x} passes through {y}"),
                None => "Contact: nothing passes through anything".to_owned(),
            }));
        }
        ui.label(
            RichText::new(
                "Checks the display triangles along the motion: very thin details can slip through.",
            )
            .weak(),
        );
    }

    // ---- Motion Study ----------------------------------------------------------

    fn study_tab(&mut self, ui: &mut egui::Ui) {
        let data = self.data();
        ui.horizontal(|ui| {
            if ui.button("New Study").clicked() {
                let mut made = 0;
                self.edit_assembly("Motion Study", |a| {
                    made = a.motion.next_id();
                    let n = a.motion.studies.len() + 1;
                    a.motion.studies.push(MotionStudy {
                        id: made,
                        name: format!("Study{n}"),
                        steps: 100,
                        keys: Vec::new(),
                    });
                });
                self.motion.study = Some(made);
            }
            egui::ComboBox::from_id_salt("motion_study")
                .selected_text(
                    self.motion
                        .study
                        .and_then(|i| data.studies.iter().find(|s| s.id == i))
                        .map_or("Select", |s| s.name.as_str()),
                )
                .show_ui(ui, |ui| {
                    for s in &data.studies {
                        ui.selectable_value(&mut self.motion.study, Some(s.id), &s.name);
                    }
                });
        });
        let Some(study) = self
            .motion
            .study
            .and_then(|i| data.studies.iter().find(|s| s.id == i))
            .cloned()
        else {
            ui.label(RichText::new("A study keys relationships' values along a timeline.").weak());
            return;
        };
        let list = self.drivable();
        let mut steps = f64::from(study.steps);
        let mut edit: Option<MotionStudy> = None;
        ui.horizontal(|ui| {
            ui.label("Steps");
            if ui
                .add(ExprValue::new(&mut steps).range(1.0..=100_000.0))
                .changed()
            {
                let mut s = study.clone();
                s.steps = steps.round().max(1.0) as u32;
                edit = Some(s);
            }
            if ui.button("Delete Study").clicked() {
                let id = study.id;
                self.edit_assembly("Motion Study", |a| a.motion.studies.retain(|s| s.id != id));
                self.motion.study = None;
            }
        });
        // Tracks.
        ui.label(RichText::new("Tracks").strong());
        let unit = |c: u32| list.iter().find(|x| x.0 == c).is_some_and(|x| x.2);
        for (k, tr) in study.keys.iter().enumerate() {
            let cname = list
                .iter()
                .find(|x| x.0 == tr.constraint)
                .map_or("(deleted)", |x| x.1.as_str());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(cname).strong());
                let angle = unit(tr.constraint);
                for (j, (st, v)) in tr.points.iter().enumerate() {
                    let shown = if angle { v.to_degrees() } else { *v };
                    let text = format!("{st:.0}: {shown:.2}{}", if angle { "°" } else { "" });
                    if ui
                        .small_button(text)
                        .on_hover_text("Click to delete this key")
                        .clicked()
                    {
                        let mut s = study.clone();
                        s.keys[k].points.remove(j);
                        edit = Some(s);
                    }
                }
                if ui
                    .small_button("x")
                    .on_hover_text("Delete the track")
                    .clicked()
                {
                    let mut s = study.clone();
                    s.keys.remove(k);
                    edit = Some(s);
                }
            });
        }
        ui.horizontal(|ui| {
            ui.label("Key");
            ui.add(ExprValue::new(&mut self.motion.key.0).range(0.0..=100_000.0));
            ui.label("Value");
            ui.add(ExprValue::new(&mut self.motion.key.1));
            ui.menu_button("Add to...", |ui| {
                for (id, n, angle) in &list {
                    if ui.button(n).clicked() {
                        let value = if *angle {
                            self.motion.key.1.to_radians()
                        } else {
                            self.motion.key.1
                        };
                        let mut s = study.clone();
                        let at = self.motion.key.0;
                        match s.keys.iter_mut().find(|t| t.constraint == *id) {
                            Some(t) => {
                                t.points.retain(|p| (p.0 - at).abs() > 1e-9);
                                t.points.push((at, value));
                                t.points.sort_by(|a, b| a.0.total_cmp(&b.0));
                            }
                            None => s.keys.push(StudyKey {
                                constraint: *id,
                                points: vec![(at, value)],
                            }),
                        }
                        edit = Some(s);
                        ui.close();
                    }
                }
            });
        });
        if let Some(s) = edit {
            self.edit_assembly("Motion Study", |a| {
                if let Some(x) = a.motion.studies.iter_mut().find(|x| x.id == s.id) {
                    *x = s;
                }
            });
        }
        // Transport.
        ui.separator();
        let mut jump = None;
        let now = ui.input(|i| i.time);
        let mut speed = self.motion.playing.as_ref().map_or(30.0, |p| p.speed);
        let mut looping = self.motion.playing.as_ref().is_some_and(|p| p.looping);
        let step = self
            .motion
            .playing
            .as_ref()
            .filter(|p| p.study == study.id)
            .map_or(0.0, |p| p.step);
        ui.horizontal(|ui| {
            if ui.button("|<").on_hover_text("To the start").clicked() {
                jump = Some((0.0, 0));
            }
            if ui.button("<").on_hover_text("Play back").clicked() {
                jump = Some((step, -1));
            }
            if ui.button("||").on_hover_text("Pause").clicked() {
                jump = Some((step, 0));
            }
            if ui.button(">").on_hover_text("Play").clicked() {
                let from = if step >= f64::from(study.steps) {
                    0.0
                } else {
                    step
                };
                jump = Some((from, 1));
            }
            if ui.button(">|").on_hover_text("To the end").clicked() {
                jump = Some((f64::from(study.steps), 0));
            }
            ui.checkbox(&mut looping, "Loop");
            ui.label("Steps/s");
            ui.add(ExprValue::new(&mut speed).range(1.0..=1000.0));
        });
        let mut at = step;
        if ui
            .add(egui::Slider::new(&mut at, 0.0..=f64::from(study.steps)).text("Step"))
            .changed()
        {
            jump = Some((at, 0));
        }
        if let Some((s, dir)) = jump {
            self.start_study(study.id, s, dir, now);
        }
        if let Some(p) = self.motion.playing.as_mut() {
            p.speed = speed;
            p.looping = looping;
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.motion.playing.is_some(), egui::Button::new("Stop"))
                .on_hover_text("Stop and put the components back")
                .clicked()
            {
                self.stop_study();
            }
            if ui
                .button("Copy CSV")
                .on_hover_text("Each component's place at every step, on the clipboard")
                .clicked()
            {
                let text = self.study_csv(&study);
                ui.ctx().copy_text(text);
                self.file_status = Some(Ok("Motion Study: the poses are on the clipboard".into()));
            }
        });
    }

    /// The saved values and places, once per playing study.
    fn start_study(&mut self, id: u32, step: Real, dir: i32, now: f64) {
        let same = self.motion.playing.as_ref().is_some_and(|p| p.study == id);
        if !same {
            self.stop_study();
            let saved = Saved {
                values: self
                    .assembly()
                    .map(|a| {
                        a.constraints
                            .iter()
                            .map(|c| (c.id, c.value, c.locked))
                            .collect()
                    })
                    .unwrap_or_default(),
                places: self.places(),
            };
            self.motion.playing = Some(Playing {
                study: id,
                step,
                dir,
                looping: false,
                speed: 30.0,
                last: now,
                saved,
            });
        } else if let Some(p) = self.motion.playing.as_mut() {
            p.step = step;
            p.dir = dir;
            p.last = now;
        }
        self.study_pose(id, step);
    }

    /// Puts the components where the study has them at `step`. Returns the pair that
    /// stopped it, when the Contact Solver is on.
    fn study_pose(&mut self, id: u32, step: Real) -> Option<(String, String)> {
        let study = self.data().studies.into_iter().find(|s| s.id == id)?;
        let wanted: Vec<(u32, Real)> = study
            .keys
            .iter()
            .filter_map(|k| Some((k.constraint, k.value_at(step)?)))
            .collect();
        let contact = self.assembly().is_some_and(|a| a.motion.contact_enabled);
        let was_hit = contact && self.contact_hit().is_some();
        let before = self.places();
        let values: Vec<(u32, Real, bool)> = self
            .assembly()
            .map(|a| {
                a.constraints
                    .iter()
                    .map(|c| (c.id, c.value, c.locked))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(a) = self.assembly_mut() {
            for (c, v) in &wanted {
                if let Some(x) = a.constraint_mut(*c) {
                    x.value = *v;
                    x.locked = true;
                }
            }
        }
        self.asm_solve(None);
        if contact
            && !was_hit
            && let Some(hit) = self.contact_hit()
        {
            // Back to the last free pose.
            if let Some(a) = self.assembly_mut() {
                for (id, v, l) in values {
                    if let Some(x) = a.constraint_mut(id) {
                        x.value = v;
                        x.locked = l;
                    }
                }
                for (id, t) in &before {
                    if let Some(o) = a.find_mut(*id) {
                        o.transform = *t;
                    }
                }
            }
            self.scene_dirty = true;
            return Some(hit);
        }
        None
    }

    /// The running study's frame.
    fn motion_play(&mut self, ctx: &egui::Context) {
        let Some(mut p) = self.motion.playing.clone() else {
            return;
        };
        if p.dir == 0 {
            return;
        }
        let now = ctx.input(|i| i.time);
        let total = self
            .data()
            .studies
            .iter()
            .find(|s| s.id == p.study)
            .map_or(0.0, |s| f64::from(s.steps));
        let advance = (now - p.last) * p.speed * f64::from(p.dir);
        p.last = now;
        p.step += advance;
        if p.step > total || p.step < 0.0 {
            if p.looping {
                p.step = if p.dir > 0 { 0.0 } else { total };
            } else {
                p.step = p.step.clamp(0.0, total);
                p.dir = 0;
            }
        }
        if let Some((x, y)) = self.study_pose(p.study, p.step) {
            p.dir = 0;
            self.file_status = Some(Ok(format!("Motion Study: {x} stops at {y}")));
        }
        self.motion.playing = Some(p);
        ctx.request_repaint();
    }

    /// Ends the study and puts everything back.
    pub(super) fn stop_study(&mut self) {
        let Some(p) = self.motion.playing.take() else {
            return;
        };
        if let Some(a) = self.assembly_mut() {
            for (id, v, l) in p.saved.values {
                if let Some(x) = a.constraint_mut(id) {
                    x.value = v;
                    x.locked = l;
                }
            }
            for (id, t) in p.saved.places {
                if let Some(o) = a.find_mut(id) {
                    o.transform = t;
                }
            }
        }
        self.scene_dirty = true;
    }

    /// One line per step and component: step, name, x, y, z, rotation vector.
    fn study_csv(&mut self, study: &MotionStudy) -> String {
        let saved_values: Vec<(u32, Real, bool)> = self
            .assembly()
            .map(|a| {
                a.constraints
                    .iter()
                    .map(|c| (c.id, c.value, c.locked))
                    .collect()
            })
            .unwrap_or_default();
        let saved_places = self.places();
        let mut out = String::from("step,component,x,y,z,rx,ry,rz\n");
        let steps = study.steps.min(2000);
        for k in 0..=steps {
            let at = f64::from(k) * f64::from(study.steps) / f64::from(steps.max(1));
            let _ = self.study_pose(study.id, at);
            if let Some(a) = self.assembly() {
                for o in &a.occurrences {
                    let t = o.transform.translation.vector;
                    let r = o.transform.rotation.scaled_axis();
                    out.push_str(&format!(
                        "{at},{},{},{},{},{},{},{}\n",
                        o.name, t.x, t.y, t.z, r.x, r.y, r.z
                    ));
                }
            }
        }
        if let Some(a) = self.assembly_mut() {
            for (id, v, l) in saved_values {
                if let Some(x) = a.constraint_mut(id) {
                    x.value = v;
                    x.locked = l;
                }
            }
            for (id, t) in saved_places {
                if let Some(o) = a.find_mut(id) {
                    o.transform = t;
                }
            }
        }
        self.scene_dirty = true;
        out
    }

    // ---- Exploded View ---------------------------------------------------------

    fn exploded_tab(&mut self, ui: &mut egui::Ui) {
        let data = self.data();
        ui.label(
            RichText::new("Components moved apart for show: the design keeps its places.").weak(),
        );
        ui.horizontal(|ui| {
            ui.label("Scale");
            ui.add(ExprValue::new(&mut self.motion.scale).range(0.0..=100.0));
            if ui
                .button("Auto Explode")
                .on_hover_text("A new view: each component away from the middle")
                .clicked()
            {
                self.auto_explode();
            }
        });
        let shown = self.motion.shown;
        let mut show = shown;
        let mut delete = None;
        for v in &data.exploded {
            ui.horizontal(|ui| {
                let on = shown.is_some_and(|s| s.0 == v.id);
                if ui.selectable_label(on, &v.name).clicked() {
                    show = if on { None } else { Some((v.id, 1.0)) };
                }
                ui.label(RichText::new(format!("{} moved", v.moves.len())).weak());
                if ui
                    .small_button("x")
                    .on_hover_text("Delete the view")
                    .clicked()
                {
                    delete = Some(v.id);
                }
            });
        }
        if let Some(id) = delete {
            self.edit_assembly("Exploded View", |a| {
                a.motion.exploded.retain(|v| v.id != id)
            });
            if show.is_some_and(|s| s.0 == id) {
                show = None;
            }
        }
        if let Some((id, mut amount)) = show {
            ui.separator();
            if ui
                .add(egui::Slider::new(&mut amount, 0.0..=1.0).text("Explode"))
                .changed()
            {
                show = Some((id, amount));
            }
            ui.horizontal(|ui| {
                ui.label("Move selected");
                for (k, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
                    ui.label(axis);
                    ui.add(ExprValue::new(&mut self.motion.delta[k]).suffix(" mm"));
                }
                if ui
                    .button("Add")
                    .on_hover_text("Adds this move for the components selected in the view")
                    .clicked()
                {
                    let (sel, d) = (self.asm.sel.clone(), self.motion.delta);
                    self.edit_assembly("Exploded View", |a| {
                        if let Some(v) = a.motion.exploded.iter_mut().find(|v| v.id == id) {
                            for o in &sel {
                                match v.moves.iter_mut().find(|m| m.0 == *o) {
                                    Some(m) => {
                                        for (x, y) in m.1.iter_mut().zip(d) {
                                            *x += y;
                                        }
                                    }
                                    None => v.moves.push((*o, d)),
                                }
                            }
                        }
                    });
                }
            });
        }
        if show != shown || show.is_some() {
            self.show_exploded(show);
        }
    }

    fn auto_explode(&mut self) {
        let Some(a) = self.assembly() else {
            return;
        };
        let ids: Vec<u32> = a
            .occurrences
            .iter()
            .filter(|o| o.shown())
            .map(|o| o.id)
            .collect();
        let centres: Vec<(u32, Point3)> = ids
            .iter()
            .filter_map(|&id| {
                let (lo, hi) = self.asm_box(id)?;
                Some((id, Point3::from((lo.coords + hi.coords) / 2.0)))
            })
            .collect();
        if centres.len() < 2 {
            self.file_status = Some(Err("Exploded View: it takes two or more components".into()));
            return;
        }
        let moves = motion::auto_explode(&centres, self.motion.scale);
        let mut made = 0;
        self.edit_assembly("Exploded View", |a| {
            made = a.motion.next_id();
            let n = a.motion.exploded.len() + 1;
            a.motion.exploded.push(ExplodedView {
                id: made,
                name: format!("Exploded View{n}"),
                moves,
            });
        });
        self.show_exploded(Some((made, 1.0)));
    }

    /// Draws the view (`amount` of its moves) or the design's places again.
    pub(super) fn show_exploded(&mut self, show: Option<(u32, Real)>) {
        let changed = self.motion.shown != show;
        self.motion.shown = show;
        let mut display = HashMap::new();
        if let (Some((id, amount)), Some(a)) = (show, self.assembly())
            && let Some(v) = a.motion.exploded.iter().find(|v| v.id == id)
        {
            for (o, m) in &v.moves {
                if let Some(occ) = a.find(*o) {
                    let d = Vector3::new(m[0], m[1], m[2]) * amount;
                    display.insert(*o, Transform3::translation(d.x, d.y, d.z) * occ.transform);
                }
            }
        }
        if changed || display != self.motion.display {
            self.motion.display = display;
            self.scene_dirty = true;
        }
    }
}

/// A contact set row's command.
enum SetCmd {
    Delete,
    Active(bool),
    Add,
    Remove,
}
