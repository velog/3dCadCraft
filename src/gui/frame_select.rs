//! Exact selection of the Frame commands (`docs/spec/6.1-frame-generator.md`
//! §3.8): a member selector takes only frame members, a face selector only
//! faces; the cursor highlights only what the active selector would take,
//! and every selector's picks stay drawn in its colour (Inventor: the first
//! selection yellow, the second blue).

use super::CadCraftApp;
use super::asm_planned::AsmPlanned;
use super::part_planned::{PartDialog, Planned};
use crate::assembly::frame;
use crate::assembly::relation::GeomRef;
use crate::kernel::Point3;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke};

/// The first selection (Corner Joint's first member, Trim/Extend's tool
/// face, Notch's notching members).
pub(super) const YELLOW: [f32; 3] = [0.95, 0.78, 0.1];
/// The second selection: the members that are cut.
pub(super) const BLUE: [f32; 3] = [0.2, 0.45, 0.95];
/// How near (px) a click must come to an anchor point of Edit Frame Members.
const ANCHOR_PICK_PX: f32 = 9.0;

/// A face or member under the cursor that the active selector takes.
const HOVER: Color32 = Color32::from_rgb(110, 220, 120);

fn color(c: [f32; 3]) -> Color32 {
    let b = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(b(c[0]), b(c[1]), b(c[2]))
}

/// The colour of selector `key` when it takes frame members (`None`: it
/// takes something else).
pub(super) fn member_colour(d: &PartDialog, key: &str) -> Option<[f32; 3]> {
    use AsmPlanned::*;
    let Planned::Asm(k) = d.kind else {
        return None;
    };
    let flip = d.check(0, "Flip Members");
    match (k, key) {
        (CornerJoint, "first") => Some(if flip { BLUE } else { YELLOW }),
        (CornerJoint, "second") => Some(if flip { YELLOW } else { BLUE }),
        (Notch, "tools") => Some(YELLOW),
        (Notch, "notched") => Some(BLUE),
        (
            FrameChange | Miter | TrimExtend | LengthenShorten | RemoveEndTreatments | ChangeReuse
            | EditFrame,
            "members",
        )
        | (FrameMemberInfo, "member")
        | (Reuse, "source") => Some(BLUE),
        _ => None,
    }
}

/// The colour of a frame command's face selector (Trim/Extend's tool).
fn face_colour(d: &PartDialog, key: &str) -> Option<[f32; 3]> {
    matches!(
        (d.kind, key),
        (Planned::Asm(AsmPlanned::TrimExtend), "face")
    )
    .then_some(YELLOW)
}

/// A selector of one member: a new pick replaces the old one.
fn single(d: &PartDialog, key: &str) -> bool {
    matches!(
        (d.kind, key),
        (Planned::Asm(AsmPlanned::CornerJoint), "first" | "second")
            | (Planned::Asm(AsmPlanned::FrameMemberInfo), "member")
            | (Planned::Asm(AsmPlanned::Reuse), "source")
    )
}

/// The occurrence name of a pick's label (`Name · component`).
fn pick_name(w: &str) -> &str {
    w.split(" · ").next().unwrap_or(w)
}

impl CadCraftApp {
    /// Is occurrence `occ` a member of a frame of the active assembly?
    pub(super) fn is_frame_member(&self, occ: u32) -> bool {
        self.assembly().is_some_and(|a| {
            a.frames
                .iter()
                .any(|f| f.members.iter().any(|m| m.occurrence == occ))
        })
    }

    /// The colour the open frame panel gives occurrence `id` (its picks).
    pub(super) fn frame_tint(&self, id: u32) -> Option<[f32; 3]> {
        let d = self.part_planned.as_ref()?;
        let name = &self.assembly()?.find(id)?.name;
        d.picks.iter().find_map(|(key, picks)| {
            member_colour(d, key).filter(|_| picks.iter().any(|w| pick_name(w) == name.as_str()))
        })
    }

    /// Every tinted occurrence (the scene is rebuilt when this changes).
    pub(super) fn frame_tints(&self) -> Vec<(u32, [u8; 3])> {
        let Some(a) = self.assembly() else {
            return Vec::new();
        };
        a.occurrences
            .iter()
            .filter_map(|o| {
                let c = color(self.frame_tint(o.id)?);
                Some((o.id, [c.r(), c.g(), c.b()]))
            })
            .collect()
    }

    /// The occurrence under the cursor as the active frame selector sees
    /// it: a member selector highlights only frame members, a face
    /// selector no whole component (its face is drawn by the overlay).
    pub(super) fn frame_hover(&self, hover: Option<u32>) -> Option<u32> {
        let Some(d) = self.part_planned.as_ref() else {
            return hover;
        };
        let Some(key) = d.active else {
            return hover;
        };
        if member_colour(d, key).is_some() {
            return hover.filter(|&o| self.is_frame_member(o));
        }
        // A selector of faces, edges or lines: the whole component is not
        // what a click takes, so it is not highlighted (spec 0-ui rule 19).
        let takes_body = d
            .flow()
            .current()
            .is_some_and(|s| s.accepts.contains(&crate::command::flow::PickKind::Body));
        hover.filter(|_| takes_body)
    }

    /// A click while a member selector is active: only a frame member is
    /// taken (`false`: the active selector takes something else).
    pub(super) fn frame_member_click(&mut self, hit: Option<(u32, Point3)>) -> bool {
        let Some(d) = self.part_planned.as_ref() else {
            return false;
        };
        let Some(key) = d.active else {
            return false;
        };
        if member_colour(d, key).is_none() {
            return false;
        }
        let title = d.kind.title();
        let one = single(d, key);
        let Some((occ, at)) = hit else {
            return true;
        };
        let Some(w) = self.occurrence_label_pub(occ) else {
            return true;
        };
        if !self.is_frame_member(occ) {
            self.file_status = Some(Err(format!(
                "{title}: {} is not a frame member",
                pick_name(&w)
            )));
            return true;
        }
        let Some(d) = self.part_planned.as_mut() else {
            return true;
        };
        // Corner Joint: one member cannot be both the first and the second.
        let other = d
            .picks
            .iter()
            .any(|(k, p)| *k != key && single(d, k) && p.contains(&w));
        if one && other {
            self.file_status = Some(Err(format!(
                "{title}: {} is already the other member",
                pick_name(&w)
            )));
            return true;
        }
        let again = d.picks_of(key).contains(&w);
        d.clicks.retain(|(l, _)| *l != w);
        d.clicks.push((w.clone(), [at.x, at.y, at.z]));
        // Lengthen/Shorten: a click on a picked member picks its other end.
        if again && d.kind == Planned::Asm(AsmPlanned::LengthenShorten) {
            self.file_status = None;
            return true;
        }
        if one && d.picks_of(key).iter().any(|p| *p != w) {
            d.picks.insert(key, Vec::new());
        }
        let first = d.picks_of(key).is_empty();
        d.pick(w);
        // Change shows the first picked member's section and orientation,
        // Frame Member Info the picked member's numbers.
        let change = first && d.kind == Planned::Asm(AsmPlanned::FrameChange);
        let info = d.kind == Planned::Asm(AsmPlanned::FrameMemberInfo);
        if change || info {
            let mut d = self.part_planned.take();
            if let Some(d) = d.as_mut() {
                if change {
                    self.frame_fill_change(d, occ);
                } else {
                    self.fill_member_info(d, occ);
                }
            }
            self.part_planned = d;
        }
        self.file_status = None;
        true
    }

    /// The members selected before a frame command opens, put where the
    /// command takes them (only frame members; Corner Joint: two members
    /// are its first and second). `false`: not a frame command.
    pub(super) fn frame_preselect(&self, d: &mut PartDialog) -> bool {
        let keys: Vec<&'static str> = super::part_planned::selector_defs(d.kind)
            .into_iter()
            .map(|s| s.0)
            .filter(|k| member_colour(d, k).is_some())
            .collect();
        let Some(&first) = keys.first() else {
            return false;
        };
        let members: Vec<u32> = self
            .asm
            .sel
            .iter()
            .copied()
            .filter(|&o| self.is_frame_member(o))
            .collect();
        let names: Vec<String> = members
            .iter()
            .filter_map(|&o| self.occurrence_label_pub(o))
            .collect();
        if names.is_empty() {
            return true;
        }
        if d.kind == Planned::Asm(AsmPlanned::FrameChange) {
            self.frame_fill_change(d, members[0]);
        }
        if d.kind == Planned::Asm(AsmPlanned::FrameMemberInfo) {
            self.fill_member_info(d, members[0]);
        }
        if single(d, first) {
            for (k, w) in keys.iter().zip(names) {
                d.picks.insert(k, vec![w]);
            }
        } else {
            d.picks.insert(first, names);
        }
        // The first selector still empty is the active one.
        let all: Vec<&'static str> = super::part_planned::selector_defs(d.kind)
            .into_iter()
            .map(|s| s.0)
            .collect();
        d.active = all
            .iter()
            .copied()
            .find(|k| d.picks.get(k).is_none_or(Vec::is_empty))
            .or(all.last().copied());
        true
    }

    /// The frame panel's faces (picked, and the one under the cursor when
    /// the face selector is active) and Lengthen/Shorten's end dots.
    pub(super) fn frame_select_overlay(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        pointer: Option<Pos2>,
    ) {
        let Some(d) = self.part_planned.as_ref() else {
            return;
        };
        for (key, refs) in &d.refs {
            if let Some(c) = face_colour(d, key) {
                for g in refs {
                    self.draw_geom(painter, rect, g, color(c));
                }
            }
        }
        if let (Some(key), Some(p)) = (d.active, pointer)
            && face_colour(d, key).is_some()
            && let Some(g @ GeomRef::Face { .. }) = self.geom_under_pub(p, rect)
        {
            self.draw_geom(painter, rect, &g, HOVER);
        }
        if d.kind == Planned::Asm(AsmPlanned::LengthenShorten) {
            self.lengthen_dots(d, painter, rect);
        }
        if let Some(c) = d.members.current {
            self.anchor_dots(d, c, painter, rect, pointer);
        }
    }

    /// The anchor points of member `occ` (the 9 box points and the
    /// centroid) halfway along it, with its section outline there.
    fn anchor_points(&self, occ: u32) -> Option<(Vec<Point3>, Vec<Point3>)> {
        let a = self.assembly()?;
        let o = a.find(occ)?;
        let m = a
            .frames
            .iter()
            .flat_map(|f| &f.members)
            .find(|m| m.occurrence == occ)?;
        let s = m.section()?;
        let z = m.built.as_ref().map(|b| b.length).unwrap_or(0.0) / 2.0;
        let at = |x: f64, y: f64| o.transform.transform_point(&Point3::new(x, y, z));
        let points = frame::Anchor::ALL
            .iter()
            .map(|k| {
                let (x, y) = k.point(&s);
                at(x, y)
            })
            .collect();
        let outline = s.outline().iter().map(|q| at(q.x, q.y)).collect();
        Some((points, outline))
    }

    /// Edit Frame Members: the current member's anchor points; the one in
    /// use is orange, the one under the cursor green.
    fn anchor_dots(
        &self,
        d: &PartDialog,
        occ: u32,
        painter: &egui::Painter,
        rect: Rect,
        pointer: Option<Pos2>,
    ) {
        let Some((points, outline)) = self.anchor_points(occ) else {
            return;
        };
        let line: Vec<Pos2> = outline
            .iter()
            .filter_map(|p| self.camera.project(p, rect))
            .collect();
        if line.len() > 2 {
            let mut l = line.clone();
            l.push(line[0]);
            painter.add(egui::Shape::line(l, Stroke::new(1.5, color(YELLOW))));
        }
        let current = d.members.anchor.get(&occ).copied().unwrap_or(0);
        for (k, p) in points.iter().enumerate() {
            let Some(q) = self.camera.project(p, rect) else {
                continue;
            };
            let hot = pointer.is_some_and(|c| c.distance(q) < ANCHOR_PICK_PX);
            let (r, fill) = if k == current {
                (6.0, Color32::from_rgb(255, 150, 40))
            } else if hot {
                (5.0, HOVER)
            } else {
                (4.0, Color32::from_rgb(70, 130, 230))
            };
            painter.circle(q, r, fill, Stroke::new(1.5, Color32::WHITE));
        }
    }

    /// A click on one of the current member's anchor points sets its anchor
    /// (`true`: the click was taken).
    pub(super) fn frame_anchor_click(&mut self, p: Pos2, rect: Rect) -> bool {
        let Some(occ) = self.part_planned.as_ref().and_then(|d| d.members.current) else {
            return false;
        };
        let Some((points, _)) = self.anchor_points(occ) else {
            return false;
        };
        let hit = points
            .iter()
            .enumerate()
            .filter_map(|(k, w)| self.camera.project(w, rect).map(|q| (k, q.distance(p))))
            .filter(|(_, d)| *d < ANCHOR_PICK_PX)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((k, _)) = hit else {
            return false;
        };
        if let Some(d) = self.part_planned.as_mut() {
            d.members.anchor.insert(occ, k);
            d.set_index(0, "Anchor", k);
        }
        self.apply_member_anchor(occ, k);
        true
    }

    /// Edit Frame Members: member `occ` takes anchor `k` (of `Anchor::ALL`)
    /// at once — it moves in the view, one Undo step.
    pub(super) fn apply_member_anchor(&mut self, occ: u32, k: usize) {
        let Some(&anchor) = frame::Anchor::ALL.get(k) else {
            return;
        };
        let linked = self.linked();
        let mut trial = self.project.clone();
        match frame::set_anchor(&mut trial, &linked, self.part_id, occ, anchor) {
            Ok(_) => {
                self.history.break_coalescing();
                self.checkpoint("Frame Member Anchor");
                self.project = trial;
                self.history.break_coalescing();
                self.scene_dirty = true;
            }
            Err(e) => self.file_status = Some(Err(format!("Edit Frame Members: {e}"))),
        }
    }

    /// A dot at the end of each picked member that Lengthen/Shorten
    /// changes (the end nearer the click).
    fn lengthen_dots(&self, d: &PartDialog, painter: &egui::Painter, rect: Rect) {
        let Some(a) = self.assembly() else {
            return;
        };
        let linked = self.linked();
        for w in d.picks_of("members") {
            let Some(o) = a.occurrences.iter().find(|o| o.name == pick_name(w)) else {
                continue;
            };
            let Some((s, e)) = frame::member_run(&self.project, &linked, a, o.id) else {
                continue;
            };
            let click = d
                .clicks
                .iter()
                .rev()
                .find(|(l, _)| l == w)
                .map(|(_, p)| Point3::new(p[0], p[1], p[2]));
            let end = match click {
                Some(c) if (c - s).norm() < (c - e).norm() => s,
                _ => e,
            };
            if let Some(q) = self.camera.project(&end, rect) {
                painter.circle(q, 5.0, color(BLUE), Stroke::new(1.5, Color32::WHITE));
            }
        }
    }
}
