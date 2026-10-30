//! Editing a frame (`docs/spec/6.1-frame-generator.md` §3.3, §3.9): the
//! frame's browser node (members, end treatments), the members' right-click
//! menu (rule 17), Edit / Delete of one end treatment, Change's OK and
//! deleting members.

use super::CadCraftApp;
use super::asm_planned::AsmPlanned;
use super::icons::Icon;
use super::part_planned::PartDialog;
use crate::assembly::frame::{self, Treatment};
use crate::document::{DocumentData, Project};
use crate::part::frame::Library;
use eframe::egui;

/// What a row of the frame's browser node asked for.
pub(super) enum FrameCmd {
    Select(Vec<u32>, bool),
    Start(AsmPlanned, Vec<u32>),
    EditTreatment(u32, usize),
    DeleteTreatment(u32, usize),
    DeleteMembers(Vec<u32>),
    Retype(u32, usize, frame::CornerType),
}

/// A frame's browser node: id, name, members (occurrence, name).
type FrameNode = (u32, String, Vec<(u32, String)>);

/// The commands a frame member offers (Inventor's *Frame Generator* menu).
const MEMBER_COMMANDS: &[AsmPlanned] = &[
    AsmPlanned::FrameChange,
    AsmPlanned::Miter,
    AsmPlanned::CornerJoint,
    AsmPlanned::TrimExtend,
    AsmPlanned::Notch,
    AsmPlanned::LengthenShorten,
    AsmPlanned::RemoveEndTreatments,
    AsmPlanned::FrameMemberInfo,
];

impl CadCraftApp {
    /// The frame nodes of the browser: each frame's members, then its end
    /// treatments.
    pub(super) fn frame_rows(&mut self, ui: &mut egui::Ui, cmds: &mut Vec<FrameCmd>) {
        let Some(a) = self.assembly() else {
            return;
        };
        let frames: Vec<FrameNode> = a
            .frames
            .iter()
            .map(|f| {
                let members = f
                    .members
                    .iter()
                    .filter_map(|m| a.find(m.occurrence).map(|o| (o.id, o.name.clone())))
                    .collect();
                (f.id, f.name.clone(), members)
            })
            .collect();
        let rows: Vec<(u32, usize, String, Vec<u32>)> = frame::treatment_rows(a)
            .into_iter()
            .map(|(f, i, name, t)| (f, i, name, t.members()))
            .collect();
        for (fid, fname, members) in frames {
            let all: Vec<u32> = members.iter().map(|m| m.0).collect();
            let on = !all.is_empty() && all.iter().all(|i| self.asm.sel.contains(i));
            egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                ui.make_persistent_id(("asm_frame", fid)),
                true,
            )
            .show_header(ui, |ui| {
                super::panels::tree_icon(ui, Icon::InsertFrame);
                let r = ui.selectable_label(on, &fname);
                if r.clicked() {
                    cmds.push(FrameCmd::Select(all.clone(), false));
                }
                r.context_menu(|ui| {
                    if ui.button("Edit Frame...").clicked() {
                        cmds.push(FrameCmd::Start(AsmPlanned::EditFrame, all.clone()));
                        ui.close();
                    }
                    if ui.button("Change").clicked() {
                        cmds.push(FrameCmd::Start(AsmPlanned::FrameChange, all.clone()));
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        cmds.push(FrameCmd::DeleteMembers(all.clone()));
                        ui.close();
                    }
                });
            })
            .body(|ui| {
                for (id, name) in &members {
                    ui.horizontal(|ui| {
                        super::panels::tree_icon(ui, Icon::Part);
                        let r = ui.selectable_label(self.asm.sel.contains(id), name);
                        if r.clicked() {
                            let add = ui.input(|i| i.modifiers.command || i.modifiers.shift);
                            cmds.push(FrameCmd::Select(vec![*id], add));
                        }
                        // Inventor: double-click a member = Edit with Frame Generator.
                        if r.double_clicked() {
                            cmds.push(FrameCmd::Start(AsmPlanned::FrameChange, vec![*id]));
                        }
                        if r.hovered() {
                            self.asm.hover = Some(*id);
                        }
                        r.context_menu(|ui| {
                            let ids = if self.asm.sel.contains(id) {
                                self.asm.sel.clone()
                            } else {
                                vec![*id]
                            };
                            member_menu(ui, &ids, cmds);
                        });
                    });
                }
                let own: Vec<&(u32, usize, String, Vec<u32>)> =
                    rows.iter().filter(|r| r.0 == fid).collect();
                if own.is_empty() {
                    return;
                }
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    ui.make_persistent_id(("asm_frame_ends", fid)),
                    true,
                )
                .show_header(ui, |ui| {
                    super::panels::tree_icon(ui, Icon::Miter);
                    ui.label("End Treatments");
                })
                .body(|ui| {
                    for (f, i, name, ms) in own {
                        ui.horizontal(|ui| {
                            super::panels::tree_icon(ui, treatment_icon(name));
                            let on = ms.iter().all(|m| self.asm.sel.contains(m));
                            let r = ui.selectable_label(on, name);
                            if r.clicked() {
                                cmds.push(FrameCmd::Select(ms.clone(), false));
                            }
                            if r.double_clicked() {
                                cmds.push(FrameCmd::EditTreatment(*f, *i));
                            }
                            r.context_menu(|ui| {
                                if ui.button("Edit").clicked() {
                                    cmds.push(FrameCmd::EditTreatment(*f, *i));
                                    ui.close();
                                }
                                if ui.button("Delete").clicked() {
                                    cmds.push(FrameCmd::DeleteTreatment(*f, *i));
                                    ui.close();
                                }
                                // A corner can change its kind (spec 6.1 §3.10).
                                if name.starts_with("Miter") || name.starts_with("Corner") {
                                    ui.separator();
                                    for (label, k) in [
                                        ("Corner: Miter", frame::CornerType::Miter),
                                        (
                                            "Corner: Butt - first through",
                                            frame::CornerType::ButtFirst,
                                        ),
                                        (
                                            "Corner: Butt - second through",
                                            frame::CornerType::ButtSecond,
                                        ),
                                    ] {
                                        if ui.button(label).clicked() {
                                            cmds.push(FrameCmd::Retype(*f, *i, k));
                                            ui.close();
                                        }
                                    }
                                }
                            });
                        });
                    }
                });
            });
        }
    }

    /// Runs what the frame rows asked for.
    pub(super) fn frame_cmd(&mut self, c: FrameCmd) {
        match c {
            FrameCmd::Select(ids, add) => {
                if add {
                    for id in ids {
                        if let Some(k) = self.asm.sel.iter().position(|&i| i == id) {
                            self.asm.sel.remove(k);
                        } else {
                            self.asm.sel.push(id);
                        }
                    }
                } else {
                    self.asm.sel = ids;
                }
                self.cons.selected = None;
            }
            FrameCmd::Start(kind, ids) => {
                self.asm.sel = ids;
                self.start_asm_planned(kind);
            }
            FrameCmd::EditTreatment(f, i) => self.edit_treatment(f, i),
            FrameCmd::DeleteTreatment(f, i) => {
                let linked = self.linked();
                let mut trial = self.project.clone();
                match frame::delete_treatment(&mut trial, &linked, self.part_id, f, i) {
                    Ok(_) => self.frame_commit("Delete End Treatment", trial),
                    Err(e) => self.file_status = Some(Err(format!("Delete: {e}"))),
                }
            }
            FrameCmd::DeleteMembers(ids) => self.frame_delete_members(&ids),
            FrameCmd::Retype(f, i, k) => {
                let linked = self.linked();
                let mut trial = self.project.clone();
                match frame::retype_corner(&mut trial, &linked, self.part_id, f, i, k) {
                    Ok(_) => self.frame_commit("Change Corner", trial),
                    Err(e) => self.file_status = Some(Err(format!("Change Corner: {e}"))),
                }
            }
        }
    }

    /// The members' right-click menu in the view (rule 17): true when every
    /// selected occurrence is a frame member and the menu was drawn.
    pub(super) fn frame_member_menu(&mut self, ui: &mut egui::Ui, ids: &[u32]) -> bool {
        if ids.is_empty() || !ids.iter().all(|&o| self.is_frame_member(o)) {
            return false;
        }
        let mut cmds = Vec::new();
        ui.menu_button("Frame Generator", |ui| member_menu(ui, ids, &mut cmds));
        for c in cmds {
            self.frame_cmd(c);
        }
        true
    }

    /// Takes a trial copy of the project into `self` as one Undo step.
    fn frame_commit(&mut self, label: &str, trial: Project) {
        self.history.break_coalescing();
        self.checkpoint(label);
        self.project = trial;
        self.history.break_coalescing();
        self.scene_dirty = true;
        self.file_status = Some(Ok(format!("{label}: done")));
    }

    /// Deletes frame members with their parts and end treatments.
    pub(super) fn frame_delete_members(&mut self, ids: &[u32]) {
        let linked = self.linked();
        let mut trial = self.project.clone();
        match frame::delete_members(&mut trial, &linked, self.part_id, ids) {
            Ok(_) => {
                self.frame_commit("Delete", trial);
                self.asm.sel.retain(|i| !ids.contains(i));
                self.asm.hover = None;
            }
            Err(e) => self.file_status = Some(Err(format!("Delete: {e}"))),
        }
    }

    /// The project an end-treatment OK works on: a copy, without the
    /// treatment being edited (OK puts the new one in its place).
    pub(super) fn frame_trial(&self, d: &PartDialog) -> Project {
        let mut trial = self.project.clone();
        if let Some((f, i)) = d.frame_edit
            && let Some(DocumentData::Assembly(a)) =
                trial.document_mut(self.part_id).map(|x| &mut x.data)
        {
            frame::take_treatment(a, f, i);
        }
        trial
    }

    /// Opens end treatment `i` of frame `f` in its panel with its members
    /// and values; OK replaces it.
    fn edit_treatment(&mut self, f: u32, i: usize) {
        let Some(t) = self
            .assembly()
            .and_then(|a| a.frames.iter().find(|x| x.id == f))
            .and_then(|x| x.treatments.get(i))
            .cloned()
        else {
            return;
        };
        let label = |app: &Self, o: u32| app.occurrence_label_pub(o).unwrap_or_default();
        let kind = match t {
            Treatment::Miter { .. } => AsmPlanned::Miter,
            Treatment::CornerJoint { .. } => AsmPlanned::CornerJoint,
            Treatment::TrimToFace { .. } => AsmPlanned::TrimExtend,
            Treatment::Lengthen { .. } => AsmPlanned::LengthenShorten,
            Treatment::Notch { .. } => AsmPlanned::Notch,
        };
        let mut d = PartDialog::new(kind);
        match &t {
            Treatment::Miter { a, b, gap, offset } => {
                d.picks
                    .insert("members", vec![label(self, *a), label(self, *b)]);
                d.set_num(0, "Distance", *gap);
                let k = match offset {
                    frame::MiterOffset::Default => 0,
                    frame::MiterOffset::Flipped => 1,
                    frame::MiterOffset::Symmetric => 2,
                };
                d.set_index(0, "Offset", k);
            }
            Treatment::CornerJoint { a, b, offsets } => {
                d.picks.insert("first", vec![label(self, *a)]);
                d.picks.insert("second", vec![label(self, *b)]);
                d.set_num(0, "Offset1", offsets[0]);
                d.set_num(0, "Offset2", offsets[1]);
            }
            Treatment::Notch {
                notched,
                by,
                offset,
            } => {
                d.picks.insert("notched", vec![label(self, *notched)]);
                d.picks.insert("tools", vec![label(self, *by)]);
                d.set_num(0, "Offset", *offset);
            }
            Treatment::TrimToFace {
                member,
                face,
                offset,
            } => {
                d.picks.insert("face", vec![self.geom_label(face)]);
                d.refs.insert("face", vec![face.clone()]);
                d.picks.insert("members", vec![label(self, *member)]);
                d.set_num(0, "Offset", *offset);
            }
            Treatment::Lengthen { member, offsets } => {
                // Back to the panel's terms: the end set, the other one.
                let w = label(self, *member);
                let linked = self.linked();
                if let Some((s, e)) = self
                    .assembly()
                    .and_then(|a| frame::member_run(&self.project, &linked, a, *member))
                {
                    let at = if offsets[1].abs() > 1e-12 || offsets[0].abs() < 1e-12 {
                        e
                    } else {
                        s
                    };
                    d.clicks.push((w.clone(), [at.x, at.y, at.z]));
                }
                d.picks.insert("members", vec![w]);
                let (o0, o1) = (offsets[0], offsets[1]);
                if o0.abs() > 1e-12 && o1.abs() > 1e-12 {
                    if (o0 - o1).abs() < 1e-12 {
                        d.set_index(0, "Offset Type", 2);
                        d.set_num(0, "Offset", o1);
                    } else {
                        d.set_index(0, "Offset Type", 3);
                        d.set_num(0, "Offset", o1);
                        d.set_num(0, "Offset 2", o0);
                    }
                } else {
                    d.set_num(0, "Offset", o0 + o1);
                }
            }
        }
        d.active = None;
        d.frame_edit = Some((f, i));
        self.asm.tool = None;
        self.part_planned = Some(d);
    }

    /// Fills Change's Frame Member and Orientation fields from member
    /// `occ` (Inventor shows the picked member's current values).
    pub(super) fn frame_fill_change(&self, d: &mut PartDialog, occ: u32) {
        let Some(m) = self.assembly().and_then(|a| {
            a.frames
                .iter()
                .flat_map(|f| &f.members)
                .find(|m| m.occurrence == occ)
        }) else {
            return;
        };
        let lib = Library::current();
        if let Some(s) = lib.find(&m.family, &m.size) {
            let std_list = lib.standards();
            if let Some(k) = std_list.iter().position(|x| *x == s.standard) {
                d.set_index(0, "Standard", k);
            }
            if let Some(k) = lib
                .families(&s.standard)
                .iter()
                .position(|x| *x == s.family)
            {
                d.set_index(0, "Family", k);
            }
            if let Some(k) = lib.sizes(&s.family).iter().position(|x| *x == s.size) {
                d.set_index(0, "Size", k);
            }
        }
        let o = m.orientation;
        if let Some(k) = frame::Anchor::ALL.iter().position(|x| *x == o.anchor) {
            d.set_index(0, "Anchor", k);
        }
        d.set_num(0, "Horizontal Offset", o.h_offset);
        d.set_num(0, "Vertical Offset", o.v_offset);
        d.set_num(0, "Rotation Angle", o.angle.to_degrees());
        d.set_check(0, "Rotate Around Selected Point", o.about_anchor);
        d.set_check(0, "Mirror", o.mirror);
        d.set_check(0, "Flip Direction", o.flip);
    }

    /// OK of Change.
    pub(super) fn change_ok(&mut self, d: &PartDialog) -> bool {
        let members = self.picked_members(d, "members");
        let Some(o) = super::frame::insert_options(d) else {
            self.file_status = Some(Err("Change: the section table is empty".into()));
            return false;
        };
        let linked = self.linked();
        let mut trial = self.project.clone();
        match frame::change(
            &mut trial,
            &linked,
            self.part_id,
            &members,
            &o,
            &Library::current(),
        ) {
            Ok(_) => {
                self.frame_commit("Change", trial);
                true
            }
            Err(e) => {
                self.file_status = Some(Err(format!("Change: {e}")));
                false
            }
        }
    }
}

impl CadCraftApp {
    /// OK of Author Frame Profile (spec 6.1 §3.11): the picked sketch's
    /// closed region under the click (or its only one) becomes a section of
    /// the user's library.
    pub(super) fn author_profile_ok(&mut self, d: &PartDialog) -> bool {
        let fail = |app: &mut Self, e: &str| {
            app.file_status = Some(Err(format!("Author Frame Profile: {e}")));
            false
        };
        let Some((super::select::SelItem::Sketch(k), at)) = d.items_of("section").first().cloned()
        else {
            return fail(self, "select the sketch of the section");
        };
        let Some(ps) = self.part().sketches.get(k) else {
            return fail(self, "the sketch no longer exists");
        };
        let regions = ps.sketch.regions();
        let here = at.map(|p| {
            ps.sketch
                .plane
                .project(&crate::kernel::Point3::new(p[0], p[1], p[2]))
        });
        // The region under the click, else the largest (a click on a
        // curve of the outline).
        let area = |r: &&crate::sketch::Region| r.area().abs();
        let region = here
            .and_then(|q| {
                regions
                    .iter()
                    .filter(|r| r.contains(q))
                    .min_by(|a, b| area(a).total_cmp(&area(b)))
            })
            .or_else(|| regions.iter().max_by(|a, b| area(a).total_cmp(&area(b))));
        let Some(region) = region.cloned() else {
            return fail(self, "the sketch has no closed region");
        };
        let section = match crate::part::frame::Section::custom(
            &d.text(0, "Standard"),
            &d.text(0, "Family"),
            &d.text(0, "Size"),
            &region,
        ) {
            Ok(s) => s,
            Err(e) => return fail(self, &e),
        };
        let Some(dir) = crate::part::frame::user_dir() else {
            return fail(self, "no user folder to save into");
        };
        match Library::save_custom(&dir, &section) {
            Ok(path) => {
                Library::reload();
                let p = section.properties();
                self.file_status = Some(Ok(format!(
                    "Author Frame Profile: {} {} saved (A = {:.1} mm²) as {}",
                    section.family,
                    section.size,
                    p.area,
                    path.file_name()
                        .map(|n| n.to_string_lossy())
                        .unwrap_or_default()
                )));
                true
            }
            Err(e) => fail(self, &e.to_string()),
        }
    }
}

/// The browser icon of an end treatment row (its command's icon).
fn treatment_icon(name: &str) -> Icon {
    if name.starts_with("Miter") {
        Icon::Miter
    } else if name.starts_with("Corner") {
        Icon::CornerJoint
    } else if name.starts_with("Trim") {
        Icon::TrimExtend
    } else if name.starts_with("Notch") {
        Icon::Notch
    } else {
        Icon::LengthenShorten
    }
}

/// The member commands, each started with `ids` as its first input.
fn member_menu(ui: &mut egui::Ui, ids: &[u32], cmds: &mut Vec<FrameCmd>) {
    for &k in MEMBER_COMMANDS {
        if ui.button(k.title()).clicked() {
            cmds.push(FrameCmd::Start(k, ids.to_vec()));
            ui.close();
        }
    }
    ui.separator();
    if ui.button("Delete").clicked() {
        cmds.push(FrameCmd::DeleteMembers(ids.to_vec()));
        ui.close();
    }
}
