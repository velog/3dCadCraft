//! Insert Frame's logic in the GUI (`docs/spec/6.1-frame-generator.md`,
//! stage F1): the panel is the planned one (`gui/asm_planned.rs`); its
//! Standard / Family / Size lists come from the section table
//! (`part::frame::Library`), OK inserts one member per picked edge
//! (`assembly::frame::insert`), and the members follow their skeleton when
//! the assembly is shown again (`follow_frames`).

use super::CadCraftApp;
use super::asm_planned::AsmPlanned;
use super::part_planned::{PartDialog, Planned};
use crate::assembly::frame::Skeleton;
use crate::assembly::frame::{self, Anchor, InsertOptions, Orientation};
use crate::assembly::frame_sub;
use crate::assembly::relation::GeomRef;
use crate::kernel::Point3;
use crate::part::frame::{CapCorners, Library};
use eframe::egui::{self, Color32, Pos2, Rect, Stroke};

/// One set of the Insert Frame preview: its lines, their own placements,
/// the rows OK leaves out, the set's options.
type PreviewGroup<'a> = (
    &'a [Skeleton],
    &'a [Option<Orientation>],
    &'a [bool],
    InsertOptions,
);

/// Chord error of a sketch curve drawn or picked in the assembly view (mm).
const CURVE_SAMPLE_MM: f64 = 0.1;

/// Colour of component sketches in the assembly view.
const SKETCH: Color32 = Color32::from_rgb(70, 110, 170);
/// Colour of picked skeleton lines.
fn picked_color() -> Color32 {
    crate::gui::theme::canvas().hover_edge
}
/// Colour of the line under the cursor.
const HOVER: Color32 = Color32::from_rgb(255, 160, 50);

/// Is `d` a frame panel whose lists come from the section table?
fn is_frame(d: &PartDialog) -> bool {
    matches!(
        d.kind,
        Planned::Asm(AsmPlanned::InsertFrame | AsmPlanned::FrameChange | AsmPlanned::EndCap)
    )
}

/// The choices of drop-down `label` (tab `ti`) of a frame panel, from the
/// section table; `None` for any other list.
pub(super) fn frame_combo(d: &PartDialog, ti: usize, label: &str) -> Option<Vec<String>> {
    if !is_frame(d) {
        return None;
    }
    let lib = Library::current();
    let own = |v: Vec<&str>| v.into_iter().map(str::to_owned).collect::<Vec<_>>();
    match label {
        "Standard" => Some(own(lib.standards())),
        "Family" => Some(own(lib.families(&frame_choice(d, ti, "Standard")?))),
        "Size" => Some(own(lib.sizes(&frame_choice(d, ti, "Family")?))),
        // Every material of the library (built-in and the user's), the usual
        // ones for sections first (the first is the default).
        "Material" => {
            let mut v: Vec<String> = [
                "Steel S235",
                "Steel S355",
                "Stainless Steel 304",
                "Aluminium 6061-T6",
            ]
            .map(str::to_owned)
            .to_vec();
            for m in &crate::materials::with_user().materials {
                if !v.contains(&m.name) {
                    v.push(m.name.clone());
                }
            }
            Some(v)
        }
        // As Material, then every appearance of the library.
        "Appearance" => {
            let mut v = vec![AS_MATERIAL.to_owned()];
            for a in &crate::materials::with_user().appearances {
                if !v.contains(&a.name) {
                    v.push(a.name.clone());
                }
            }
            Some(v)
        }
        _ => None,
    }
}

/// Insert Frame's check box that asks for the new members' names.
pub(super) const NAMING: &str = "Frame Member Naming dialog";

/// The Appearance list's first entry: the member shows its material.
const AS_MATERIAL: &str = "As Material";

/// The chosen entry of list `label` (clamped to the list).
pub(super) fn frame_choice(d: &PartDialog, ti: usize, label: &str) -> Option<String> {
    let list = frame_combo(d, ti, label)?;
    let i = d.index(ti, label).min(list.len().checked_sub(1)?);
    list.into_iter().nth(i)
}

/// The Orientation group of the panel.
pub(super) fn orientation(d: &PartDialog) -> Orientation {
    let deg = |l: &str| d.num(0, l).unwrap_or(0.0);
    Orientation {
        anchor: Anchor::ALL
            .get(d.index(0, "Anchor"))
            .copied()
            .unwrap_or_default(),
        h_offset: deg("Horizontal Offset"),
        v_offset: deg("Vertical Offset"),
        angle: deg("Rotation Angle").to_radians(),
        about_anchor: d.check(0, "Rotate Around Selected Point"),
        mirror: d.check(0, "Mirror"),
        flip: d.check(0, "Flip Direction"),
        align: None,
    }
}

/// The picked curves as the panel takes them: with Merge Frame Members one
/// member along all of them, else one member each.
fn merged(d: &PartDialog, lines: Vec<Skeleton>) -> Vec<Skeleton> {
    if d.check(0, "Merge Frame Members") && d.index(0, "Placement") == 0 && lines.len() > 1 {
        vec![Skeleton::Chain(lines)]
    } else {
        lines
    }
}

/// The *+ Add Set* button: the picked lines with the panel's section and
/// orientation become a set; the selection is cleared for the next one.
/// The panel's note says what was kept.
pub(super) fn add_set(d: &mut PartDialog) -> String {
    if d.skeletons.is_empty() {
        return "+ Add Set: select the lines of this set first".into();
    }
    let Some(options) = insert_options(d) else {
        return "+ Add Set: the section table is empty".into();
    };
    let n = d.skeletons.len();
    let text = format!(
        "Set {}: {}, {n} line(s) kept",
        d.frame_sets.len() + 1,
        options.size
    );
    let lines = std::mem::take(&mut d.skeletons);
    let mut own = std::mem::take(&mut d.skel).own;
    own.resize(lines.len(), None);
    let skeletons = merged(d, lines);
    if skeletons.len() != own.len() {
        own.clear();
    }
    d.frame_sets.push(super::part_planned::FrameSet {
        skeletons,
        own,
        options,
    });
    d.picks.insert("edges", Vec::new());
    text
}

/// The kept sets above Insert Frame's panel: family, size and line count,
/// each with an x that drops it.
pub(super) fn sets_ui(ui: &mut egui::Ui, d: &mut PartDialog) {
    let mut drop = None;
    egui::CollapsingHeader::new(format!("Sets ({})", d.frame_sets.len()))
        .default_open(true)
        .show(ui, |ui| {
            for (i, s) in d.frame_sets.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Label::new(format!(
                            "{}. {} · {} line(s)",
                            i + 1,
                            s.options.size,
                            s.skeletons.len()
                        ))
                        .truncate(),
                    )
                    .on_hover_text(&s.options.family);
                    if super::theme::glyph(ui, super::theme::Glyph::Close)
                        .on_hover_text("Drop this set")
                        .clicked()
                    {
                        drop = Some(i);
                    }
                });
            }
        });
    if let Some(i) = drop {
        d.frame_sets.remove(i);
    }
}

/// The panel's Position and Level.
pub(super) fn layout(d: &PartDialog) -> frame::Layout {
    frame::Layout {
        side: match d.index(0, "Position") {
            1 => frame::Side::Inside,
            2 => frame::Side::Outside,
            _ => frame::Side::Center,
        },
        level: match d.index(0, "Level") {
            1 => frame::Level::Top,
            2 => frame::Level::Bottom,
            _ => frame::Level::Auto,
        },
    }
}

/// Refresh Categories: reads the user's profile folder again; the line the
/// panel shows under the button.
pub(super) fn refresh_categories() -> String {
    let (lib, bad) = Library::reload();
    let mut s = format!(
        "{} sizes in {} standards",
        lib.sections.len(),
        lib.standards().len()
    );
    if let Some(b) = bad.first() {
        s.push_str(&format!("; {} line(s) not read, first: {b}", bad.len()));
    }
    s
}

/// The panel's section, material and orientation (Insert Frame, Change).
pub(super) fn insert_options(d: &PartDialog) -> Option<InsertOptions> {
    Some(InsertOptions {
        family: frame_choice(d, 0, "Family")?,
        size: frame_choice(d, 0, "Size")?,
        material: frame_choice(d, 0, "Material"),
        orientation: orientation(d),
        corners: frame::Corners {
            kind: match d.index(0, "Corner Type") {
                1 => frame::CornerType::Miter,
                2 => frame::CornerType::ButtFirst,
                3 => frame::CornerType::ButtSecond,
                4 => frame::CornerType::ButtLonger,
                5 => frame::CornerType::ButtShorter,
                _ => frame::CornerType::None,
            },
            gap: d.num(0, "Corner Gap").unwrap_or(0.0),
            tees: match d.index(0, "T Joints") {
                1 => frame::TeeJoint::Butt,
                2 => frame::TeeJoint::Notch,
                _ => frame::TeeJoint::None,
            },
        },
        layout: layout(d),
        appearance: frame_choice(d, 0, "Appearance").filter(|a| a != AS_MATERIAL),
        appearance_to_part: d.check(0, "Apply Appearance to Part"),
    })
}

impl CadCraftApp {
    /// OK of Insert Frame: a member on every picked edge. The first one in
    /// an assembly asks for the frame's names first (the panel stays open).
    /// False (with the reason in the status bar) leaves everything as it was.
    pub(super) fn frame_ok(&mut self, d: &PartDialog) -> bool {
        if !d.members.list.is_empty() {
            return self.edit_members_ok(d);
        }
        if d.frame_sets.is_empty() && d.skeletons.is_empty() {
            self.file_status = Some(Err(
                "Insert Frame: select the skeleton edges or sketch lines".into(),
            ));
            return false;
        }
        if self.frame_ask_names() {
            return false;
        }
        match self.frame_insert(d, None) {
            Ok(done) => done,
            Err(e) => {
                self.file_status = Some(Err(format!("Insert Frame: {e}")));
                false
            }
        }
    }

    /// Inserts the panel's members; `names` (frame, skeleton) make a new
    /// frame assembly. On an error nothing changes. `Ok(false)`: the members
    /// wait for the Frame Member Naming dialog.
    pub(super) fn frame_insert(
        &mut self,
        d: &PartDialog,
        names: Option<(&str, &str)>,
    ) -> Result<bool, String> {
        let mut o = insert_options(d).ok_or("the section table is empty")?;
        o.orientation.align = self.frame_align(d)?;
        // The kept sets, then the lines picked now (with the panel's section),
        // each line with its own placement where the list gave it one.
        let mut sets: Vec<(Vec<Skeleton>, Vec<Option<Orientation>>, InsertOptions)> = d
            .frame_sets
            .iter()
            .map(|s| (s.skeletons.clone(), s.own.clone(), s.options.clone()))
            .collect();
        if !d.skeletons.is_empty() {
            let lines = merged(d, d.skeletons.clone());
            let mut own = d.skel.own.clone();
            own.resize(d.skeletons.len(), None);
            if lines.len() != own.len() {
                own.clear();
            }
            sets.push((lines, own, o.clone()));
        }
        let linked = self.frame_linked();
        // Remove Overlapping Members: of two lines on one another the shorter
        // goes (as long: the later one), a placed member too when a longer new
        // line covers it (spec 6.1 §3.1a).
        let mut removes: Vec<u32> = Vec::new();
        let mut left_out = 0;
        if d.check(0, super::frame_list::REMOVE_OVERLAPS)
            && let Some(a) = self.assembly()
        {
            let placed = self.frame_member_lines();
            let fixed = placed.len();
            let mut all: Vec<Option<(Point3, Point3)>> =
                placed.iter().map(|(_, l)| Some(*l)).collect();
            for (lines, ..) in &sets {
                all.extend(
                    lines
                        .iter()
                        .map(|s| frame::skeleton_line(&self.project, &linked, a, s).ok()),
                );
            }
            let drops = frame::overlap_drops(&all, fixed);
            removes = drops
                .iter()
                .filter(|&&k| k < fixed)
                .map(|&k| placed[k].0)
                .collect();
            let mut k = fixed;
            for (lines, own, _) in &mut sets {
                let mut keep_lines = Vec::new();
                let mut keep_own = Vec::new();
                for (j, s) in lines.drain(..).enumerate() {
                    if drops.contains(&k) {
                        left_out += 1;
                    } else {
                        keep_lines.push(s);
                        if let Some(x) = own.get(j) {
                            keep_own.push(*x);
                        }
                    }
                    k += 1;
                }
                *lines = keep_lines;
                if keep_own.len() != lines.len() {
                    keep_own.clear();
                }
                *own = keep_own;
            }
            sets.retain(|(lines, ..)| !lines.is_empty());
        }
        if sets.is_empty() {
            return Err(if left_out > 0 {
                "every picked line lies on a longer member".into()
            } else {
                "select the skeleton edges or sketch lines".into()
            });
        }
        let mut trial = self.project.clone();
        let lib = Library::current();
        let corners = o.corners;
        let mut new_target = None;
        if let Some((name, _)) = names {
            new_target = Some(frame_sub::new_frame_assembly(
                &mut trial,
                self.part_id,
                name,
            )?);
        }
        let doc = new_target.map_or_else(|| self.frame_doc(), |(_, d)| d);
        if !removes.is_empty() {
            frame::delete_members(&mut trial, &linked, doc, &removes)?;
        }
        // Inside the frame's document the lines are in its terms.
        for (lines, ..) in &mut sets {
            for s in lines.iter_mut() {
                *s = match (new_target, names) {
                    (Some((frame, doc)), Some((_, skeleton))) => frame_sub::skeleton_down(
                        &mut trial,
                        &linked,
                        frame_sub::Target {
                            top: self.part_id,
                            frame,
                            doc,
                            skeleton,
                        },
                        s,
                    )?,
                    _ => self.frame_skeleton_down(&mut trial, &linked, s)?,
                };
            }
        }
        // One frame per assembly (Inventor): new members join the frame of a
        // selected member, else the last one; only the first insert makes one.
        let selected = self.frame_selected();
        let existing = if new_target.is_none() {
            self.frame_asm().and_then(|a| {
                a.frames
                    .iter()
                    .find(|f| f.members.iter().any(|m| selected.contains(&m.occurrence)))
                    .or(a.frames.last())
                    .map(|f| f.id)
            })
        } else {
            None
        };
        let mut frame_id = existing;
        let mut occs = Vec::new();
        // Several sets, or members joining a frame: the corners once all
        // are in (between the new members and the frame's).
        let many = sets.len() > 1 || existing.is_some();
        for (lines, own, mut opts) in sets {
            if many {
                // The corners come once all sets are in.
                opts.corners = Default::default();
            }
            let (fid, made) = frame::insert_with(
                &mut trial, &linked, doc, &lines, &own, &opts, &lib, frame_id,
            )?;
            frame_id = Some(fid);
            occs.extend(made);
        }
        if let Some(fid) = frame_id.filter(|_| many)
            && (corners.kind != frame::CornerType::None || corners.tees != frame::TeeJoint::None)
        {
            frame::add_corners(&mut trial, &linked, doc, fid, &occs, &corners)?;
        }
        let overlaps = if left_out + removes.len() > 0 {
            format!(
                ", {} overlapping line(s) left out, {} placed member(s) removed",
                left_out,
                removes.len()
            )
        } else {
            String::new()
        };
        let msg = format!(
            "Insert Frame: {} member(s) of {} {}{overlaps}",
            occs.len(),
            o.family,
            o.size
        );
        if d.check(0, NAMING) {
            let rows = match trial.document(doc).map(|x| &x.data) {
                Some(crate::document::DocumentData::Assembly(a)) => {
                    frame::member_names(&trial, a, &occs)
                }
                _ => Vec::new(),
            };
            self.asm.frame_naming = Some(FrameNaming {
                trial,
                doc,
                rows,
                msg,
                apply: false,
                error: None,
            });
            return Ok(false);
        }
        self.frame_commit_insert(trial, occs, msg);
        Ok(true)
    }

    /// Members follow their skeleton (after the skeleton part was edited);
    /// members that cannot are reported in the log and keep their state.
    pub(super) fn follow_frames(&mut self) {
        if self.frame_ctx().is_none() {
            return;
        }
        let linked = self.frame_linked();
        match frame_sub::follow(&mut self.project, &linked, self.part_id) {
            Ok(s) => {
                for (occ, why) in &s.sick {
                    log::warn!("frame member {occ}: {why}");
                    // Its skeleton geometry is gone: the Missing Member window.
                    if why.contains("no longer exists")
                        && !self.asm.frame_missing_seen.contains(occ)
                    {
                        self.asm.frame_missing_seen.push(*occ);
                        self.asm.frame_missing.push((*occ, why.clone()));
                    }
                }
                if s.moved > 0 {
                    self.scene_dirty = true;
                }
                if let Some((_, why)) = s.sick.first() {
                    self.file_status = Some(Err(format!(
                        "Frame: {} member(s) cannot follow the skeleton ({why})",
                        s.sick.len()
                    )));
                }
            }
            Err(e) => log::warn!("{e}"),
        }
    }

    /// The curves (lines, arcs, splines) of the visible sketches of the components on show
    /// (first-level occurrences), in assembly coordinates, with what picks
    /// each one.
    pub(super) fn sketch_lines(&self) -> Vec<(Skeleton, String, Vec<Point3>)> {
        let Some(a) = self.assembly() else {
            return Vec::new();
        };
        // Each component's lines once (part coordinates: sketch id, name,
        // line, ends), then per occurrence.
        type Local = Vec<(
            String,
            String,
            crate::sketch::EntityId,
            &'static str,
            Vec<Point3>,
        )>;
        let mut parts: std::collections::HashMap<crate::document::DocRef, Local> =
            std::collections::HashMap::new();
        let mut out = Vec::new();
        for o in a.occurrences.iter().filter(|o| self.asm_shown(o)) {
            let local = parts.entry(o.component).or_insert_with(|| {
                self.with_part(o.component, |p| {
                    let mut v = Local::new();
                    for ps in p.sketches.iter().filter(|s| s.visible) {
                        let sk = &ps.sketch;
                        for (id, e) in sk.entities() {
                            let kind = match e {
                                crate::sketch::Entity::Line { .. } => "line",
                                crate::sketch::Entity::Arc { .. } => "arc",
                                crate::sketch::Entity::Spline { .. } => "spline",
                                _ => continue,
                            };
                            let Some(c) = sk.curve_of(id) else {
                                continue;
                            };
                            let pts: Vec<Point3> = c
                                .sample(CURVE_SAMPLE_MM)
                                .iter()
                                .map(|q| sk.to_world(q))
                                .collect();
                            v.push((ps.id.clone(), ps.name.clone(), id, kind, pts));
                        }
                    }
                    v
                })
                .unwrap_or_default()
            });
            let t = o.transform;
            for (sketch, name, line, kind, pts) in local.iter() {
                out.push((
                    Skeleton::SketchLine {
                        occurrence: o.id,
                        sub: Vec::new(),
                        sketch: sketch.clone(),
                        line: *line,
                    },
                    format!("{} · {name} {kind}", o.name),
                    pts.iter().map(|q| t.transform_point(q)).collect(),
                ));
            }
        }
        out
    }

    /// Is Insert Frame open with its edges selector active?
    fn frame_takes_lines(&self) -> bool {
        self.part_planned.as_ref().is_some_and(|d| {
            matches!(
                d.kind,
                Planned::Asm(AsmPlanned::InsertFrame | AsmPlanned::Reuse | AsmPlanned::ChangeReuse)
            ) && d.active == Some("edges")
        })
    }

    /// The sketch curve nearest `p` on screen, within the snap radius.
    fn sketch_line_under(&self, p: Pos2, rect: Rect) -> Option<(Skeleton, String, Vec<Point3>)> {
        let near = super::overlay::CURVE_SNAP_PX;
        self.sketch_lines()
            .into_iter()
            .filter_map(|l| {
                let screen: Vec<Pos2> =
                    l.2.iter()
                        .filter_map(|q| self.camera.project(q, rect))
                        .collect();
                let d = screen
                    .windows(2)
                    .map(|w| segment_distance(p, w[0], w[1]))
                    .fold(f32::MAX, f32::min);
                (d <= near).then_some((d, l))
            })
            .min_by(|x, y| x.0.total_cmp(&y.0))
            .map(|(_, l)| l)
    }

    /// Between Points: a click near an end of a model edge or a sketch line
    /// takes that end — the first as the start, the second as the end of a
    /// new straight skeleton (`Skeleton::Between`). False when Between Points
    /// is off or nothing is near.
    pub(super) fn frame_between_click(&mut self, p: Pos2, rect: Rect) -> bool {
        let on = self.part_planned.as_ref().is_some_and(|d| {
            d.kind == Planned::Asm(AsmPlanned::InsertFrame)
                && d.members.list.is_empty()
                && d.index(0, "Placement") == 1
        });
        if !on {
            return false;
        }
        let Some(a) = self.assembly() else {
            return true;
        };
        let linked = self.frame_linked();
        // The line under the cursor: a sketch line, else a model edge.
        let line = self
            .sketch_line_under(p, rect)
            .filter(|l| l.2.len() == 2)
            .map(|l| (l.0, l.2[0], l.2[1]))
            .or_else(|| {
                let g @ crate::assembly::relation::GeomRef::Edge { .. } =
                    self.geom_under_pub(p, rect)?
                else {
                    return None;
                };
                let s = Skeleton::Edge(g);
                let (u, v) = frame::skeleton_line(&self.project, &linked, a, &s).ok()?;
                Some((s, u, v))
            });
        let Some((s, u, v)) = line else {
            self.file_status = Some(Err(
                "Insert Frame: click near an end of an edge or a sketch line".into(),
            ));
            return true;
        };
        let near = |w: &Point3| {
            self.camera
                .project(w, rect)
                .map_or(f32::MAX, |q| q.distance(p))
        };
        let (end, at) = if near(&v) < near(&u) {
            (1u8, v)
        } else {
            (0u8, u)
        };
        let Some(d) = self.part_planned.as_mut() else {
            return true;
        };
        match d.between_first.take() {
            None => {
                d.between_first = Some((s, end, at));
                d.picks.insert("points", vec!["start point".into()]);
                self.file_status = Some(Ok("Insert Frame: now the end point".into()));
            }
            Some((from, from_end, start)) => {
                if (at - start).norm() < 1e-6 {
                    d.between_first = Some((from, from_end, start));
                    self.file_status =
                        Some(Err("Insert Frame: the end point is the start point".into()));
                    return true;
                }
                let n = d.skeletons.len() + 1;
                d.pick_skeleton(
                    format!("Between Points {n}"),
                    Skeleton::Between {
                        from: Box::new(from),
                        from_end,
                        to: Box::new(s),
                        to_end: end,
                    },
                );
                d.picks.insert("points", Vec::new());
                self.file_status = None;
            }
        }
        true
    }

    /// The start point of Between Points, while the end is still to pick.
    fn between_start_dot(&self, d: &PartDialog, painter: &egui::Painter, rect: Rect) {
        if let Some((_, _, at)) = &d.between_first
            && let Some(q) = self.camera.project(at, rect)
        {
            painter.circle(q, 5.0, picked_color(), Stroke::new(1.5, Color32::WHITE));
        }
    }

    /// A click of Insert Frame on a component's sketch line: it becomes a
    /// skeleton line of the panel. False when no line is under `p`.
    pub(super) fn frame_pick_sketch_line(&mut self, p: Pos2, rect: Rect) -> bool {
        if !self.frame_takes_lines() {
            return false;
        }
        let Some((s, what, ..)) = self.sketch_line_under(p, rect) else {
            return false;
        };
        if let Some(d) = self.part_planned.as_mut() {
            d.pick_skeleton(what, s);
        }
        true
    }

    /// The components' visible sketches (thin lines, always, as Inventor
    /// shows a skeleton's sketches); while Insert Frame is open also the
    /// picked skeleton lines (thick) and the line under the cursor.
    /// Insert Frame's preview (spec 6.1 §3.1 Preview): every picked line's
    /// member as a wire outline — the section at both ends joined by lines
    /// along it — with the panel's family, size, orientation and Position;
    /// drawn from the section table each frame (no kernel work), corners
    /// uncut.
    fn frame_preview(&self, d: &PartDialog, painter: &egui::Painter, rect: Rect) {
        // The kept sets with their own sections, then the lines picked now.
        let none: &[bool] = &[];
        let mut groups: Vec<PreviewGroup> = d
            .frame_sets
            .iter()
            .map(|s| {
                (
                    s.skeletons.as_slice(),
                    s.own.as_slice(),
                    none,
                    s.options.clone(),
                )
            })
            .collect();
        if let Some(o) = insert_options(d) {
            groups.push((
                d.skeletons.as_slice(),
                d.skel.own.as_slice(),
                d.skel.notes.dropped.as_slice(),
                o,
            ));
        }
        for (skeletons, own, dropped, o) in groups {
            if !skeletons.is_empty() {
                self.preview_set(skeletons, own, dropped, &o, painter, rect);
            }
        }
    }

    /// The wire preview of one set's members.
    #[allow(clippy::too_many_arguments)]
    fn preview_set(
        &self,
        skeletons: &[Skeleton],
        own: &[Option<Orientation>],
        dropped: &[bool],
        o: &InsertOptions,
        painter: &egui::Painter,
        rect: Rect,
    ) {
        let lib = Library::current();
        let Some(sec) = lib.find(&o.family, &o.size) else {
            return;
        };
        let Some(a) = self.assembly() else {
            return;
        };
        let linked = self.frame_linked();
        // Merged into one chain, the rows' own placements do not apply.
        let own = if own.len() == skeletons.len() {
            own
        } else {
            &[]
        };
        let runs: Vec<(usize, frame::Run)> = skeletons
            .iter()
            .enumerate()
            .filter(|(k, _)| !dropped.get(*k).copied().unwrap_or(false))
            .filter_map(|(k, s)| {
                frame::skeleton_run(&self.project, &linked, a, s)
                    .ok()
                    .map(|r| (k, r))
            })
            .collect();
        let lines: Vec<(Point3, Point3)> = runs
            .iter()
            .filter(|r| matches!(r.1, frame::Run::Line(..)))
            .map(|r| r.1.chord())
            .collect();
        let n = lines.len().max(1) as f64;
        let centre = Point3::from(
            lines
                .iter()
                .fold(crate::kernel::Vector3::zeros(), |s, (p, q)| {
                    s + (p.coords + q.coords) / 2.0
                })
                / n,
        );
        let outline = sec.outline();
        if outline.len() < 3 {
            return;
        }
        let stroke = Stroke::new(1.5, Color32::from_rgba_unmultiplied(255, 140, 30, 200));
        // Lines along the member at a few points of the outline.
        let step = (outline.len() / 8).max(1);
        for (k, r) in &runs {
            if let frame::Run::Curve(c) = r {
                let ori = own.get(*k).copied().flatten().unwrap_or(o.orientation);
                self.preview_curve(sec, c, &ori, (&outline, step), stroke, painter, rect);
            }
        }
        for (k, r) in &runs {
            let frame::Run::Line(p, q) = *r else {
                continue;
            };
            let ori = own
                .get(*k)
                .copied()
                .flatten()
                .unwrap_or_else(|| o.layout.orient(&o.orientation, p, q, centre));
            let Some(t) = frame::placement(sec, p, q, &ori) else {
                continue;
            };
            let len = (q - p).norm();
            let at = |v: &crate::sketch::Point2, z: f64| {
                self.camera
                    .project(&t.transform_point(&Point3::new(v.x, v.y, z)), rect)
            };
            for z in [0.0, len] {
                let mut ring: Vec<Pos2> = outline.iter().filter_map(|v| at(v, z)).collect();
                if ring.len() == outline.len() {
                    ring.push(ring[0]);
                    painter.add(egui::Shape::line(ring, stroke));
                }
            }
            for v in outline.iter().step_by(step) {
                if let (Some(a), Some(b)) = (at(v, 0.0), at(v, len)) {
                    painter.line_segment([a, b], stroke);
                }
            }
        }
    }

    /// A curved member's wire preview: the section at both ends, lines
    /// along the path from some of its points.
    #[allow(clippy::too_many_arguments)]
    fn preview_curve(
        &self,
        sec: &crate::part::frame::Section,
        c: &crate::part::sweep::PathGeom,
        orientation: &Orientation,
        (outline, step): (&[crate::sketch::Point2], usize),
        stroke: Stroke,
        painter: &egui::Painter,
        rect: Rect,
    ) {
        let Some((t, run)) = frame::curved_place(sec, c, orientation) else {
            return;
        };
        let pieces = frame::member_pieces(&run, &crate::kernel::Transform3::identity());
        let Some((_, n)) = crate::part::frame::path_plane(&pieces) else {
            return;
        };
        const STATIONS: usize = 48;
        let len = run.length();
        let (p0, t0) = run.eval(0.0);
        let axis = nalgebra::Unit::new_normalize(n);
        let at = |v: &crate::sketch::Point2, k: usize| {
            let (q, tq) = run.eval(len * k as f64 / STATIONS as f64);
            let turn = n.dot(&t0.cross(&tq)).atan2(t0.dot(&tq));
            let w = t.transform_point(&Point3::new(v.x, v.y, 0.0));
            let r = nalgebra::Rotation3::from_axis_angle(&axis, turn);
            self.camera.project(&(q + r * (w - p0)), rect)
        };
        for k in [0, STATIONS] {
            let mut ring: Vec<Pos2> = outline.iter().filter_map(|v| at(v, k)).collect();
            if ring.len() == outline.len() {
                ring.push(ring[0]);
                painter.add(egui::Shape::line(ring, stroke));
            }
        }
        for v in outline.iter().step_by(step) {
            let rail: Vec<Pos2> = (0..=STATIONS).filter_map(|k| at(v, k)).collect();
            painter.add(egui::Shape::line(rail, stroke));
        }
    }

    pub(super) fn frame_overlay(&self, painter: &egui::Painter, rect: Rect, pointer: Option<Pos2>) {
        let seg = |a: &Point3, b: &Point3| -> Option<[Pos2; 2]> {
            Some([self.camera.project(a, rect)?, self.camera.project(b, rect)?])
        };
        let curve = |v: &[Point3], stroke: Stroke| {
            let pts: Vec<Pos2> = v
                .iter()
                .filter_map(|q| self.camera.project(q, rect))
                .collect();
            if pts.len() == v.len() {
                painter.add(egui::Shape::line(pts, stroke));
            }
        };
        let lines = self.sketch_lines();
        for (_, _, v) in &lines {
            curve(v, Stroke::new(1.5, SKETCH));
        }
        let Some(d) = self.part_planned.as_ref() else {
            return;
        };
        if !matches!(
            d.kind,
            Planned::Asm(AsmPlanned::InsertFrame | AsmPlanned::Reuse | AsmPlanned::ChangeReuse)
        ) {
            return;
        }
        if d.members.list.is_empty() {
            self.frame_preview(d, painter, rect);
        }
        self.between_start_dot(d, painter, rect);
        for s in &d.skeletons {
            match s {
                Skeleton::Edge(g) => self.draw_geom(painter, rect, g, picked_color()),
                Skeleton::Between { .. } => {
                    if let Some(a) = self.assembly()
                        && let Ok((p, q)) =
                            frame::skeleton_line(&self.project, &self.frame_linked(), a, s)
                        && let Some(s) = seg(&p, &q)
                    {
                        painter.line_segment(s, Stroke::new(3.0, picked_color()));
                    }
                }
                Skeleton::SketchLine { .. } => {
                    if let Some((_, _, v)) = lines.iter().find(|l| &l.0 == s) {
                        curve(v, Stroke::new(3.0, picked_color()));
                    }
                }
                Skeleton::Chain(_) => {}
            }
        }
        if d.members.list.is_empty() {
            self.frame_list_overlay(d, painter, rect);
        }
        if self.frame_takes_lines()
            && let Some(p) = pointer
            && let Some((_, _, v)) = self.sketch_line_under(p, rect)
        {
            curve(&v, Stroke::new(3.0, HOVER));
            return;
        }
        // The model edge (Select: Edge) or face (Face / Body Edges) a click
        // would take; nothing else lights up (spec 0-ui rule 19).
        if d.active == Some("edges")
            && let Some(p) = pointer
            && let Some(g) = self.geom_under_pub(p, rect)
        {
            let face = matches!(g, crate::assembly::relation::GeomRef::Face { .. });
            match d.index(0, "Select") {
                // Body Edges: the edges of the one body under the cursor.
                2 => self.draw_body_edges(painter, rect, &g),
                0 if face => {}
                _ => self.draw_geom(painter, rect, &g, HOVER),
            }
        }
    }
}

impl CadCraftApp {
    /// The frame members picked in selector `key` (occurrence ids).
    pub(super) fn picked_members(&self, d: &PartDialog, key: &str) -> Vec<u32> {
        let Some(a) = self.frame_asm() else {
            return Vec::new();
        };
        d.picks_of(key)
            .iter()
            .filter_map(|w| {
                let name = w.split(" · ").next().unwrap_or(w);
                a.occurrences.iter().find(|o| o.name == name).map(|o| o.id)
            })
            .filter(|o| {
                a.frames
                    .iter()
                    .any(|f| f.members.iter().any(|m| m.occurrence == *o))
            })
            .collect()
    }

    /// OK of Miter: a Full Miter Cut between the two picked members, or
    /// between every pair of the picked ones that meet.
    pub(super) fn miter_ok(&mut self, d: &PartDialog) -> bool {
        let fail = |app: &mut Self, e: &str| {
            app.file_status = Some(Err(format!("Miter: {e}")));
            false
        };
        let members = self.picked_members(d, "members");
        if members.len() < 2 {
            return fail(self, "select two frame members that meet");
        }
        let linked = self.frame_linked();
        let pairs = match members.as_slice() {
            [a, b] => vec![(*a, *b)],
            _ => match self.frame_asm() {
                Some(a) => frame::meeting_pairs(&self.project, &linked, a, &members),
                None => Vec::new(),
            },
        };
        let offset = match d.index(0, "Offset") {
            1 => frame::MiterOffset::Flipped,
            2 => frame::MiterOffset::Symmetric,
            _ => frame::MiterOffset::Default,
        };
        let gap = d.num(0, "Distance").unwrap_or(0.0);
        let full = d.index(0, "Miter Type") == 0;
        let mut trial = self.frame_trial(d);
        let doc = self.frame_doc();
        let delete = d.check(0, "Delete existing end treatments");
        match frame::add_miter_typed(&mut trial, &linked, doc, &pairs, gap, offset, full, delete) {
            Ok(_) => {
                self.history.break_coalescing();
                self.checkpoint("Miter");
                self.project = trial;
                self.history.break_coalescing();
                self.scene_dirty = true;
                self.file_status = Some(Ok(format!("Miter: {} joint(s) cut", pairs.len())));
                true
            }
            Err(e) => fail(self, &e),
        }
    }

    /// OK of Edit Frame: the frame of the picked member gets the panel's
    /// Position / Level and corners (Keep leaves them).
    pub(super) fn edit_frame_ok(&mut self, d: &PartDialog) -> bool {
        let fail = |app: &mut Self, e: &str| {
            app.file_status = Some(Err(format!("Edit Frame: {e}")));
            false
        };
        let members = self.picked_members(d, "members");
        let Some(&m) = members.first() else {
            return fail(self, "select a member of the frame");
        };
        let Some(fid) = self.frame_asm().and_then(|a| {
            a.frames
                .iter()
                .find(|f| f.members.iter().any(|x| x.occurrence == m))
                .map(|f| f.id)
        }) else {
            return fail(self, "the member belongs to no frame");
        };
        let layout = match d.index(0, "Position") {
            0 if d.index(0, "Level") == 0 => None,
            k => Some(frame::Layout {
                side: match k {
                    2 => frame::Side::Inside,
                    3 => frame::Side::Outside,
                    _ => frame::Side::Center,
                },
                level: layout(d).level,
            }),
        };
        let kind = match d.index(0, "Corner Type") {
            0 => None,
            1 => Some(frame::CornerType::None),
            2 => Some(frame::CornerType::Miter),
            3 => Some(frame::CornerType::ButtFirst),
            4 => Some(frame::CornerType::ButtSecond),
            5 => Some(frame::CornerType::ButtLonger),
            _ => Some(frame::CornerType::ButtShorter),
        };
        let corners = kind.map(|kind| frame::Corners {
            kind,
            gap: d.num(0, "Corner Gap").unwrap_or(0.0),
            tees: match d.index(0, "T Joints") {
                1 => frame::TeeJoint::Butt,
                2 => frame::TeeJoint::Notch,
                _ => frame::TeeJoint::None,
            },
        });
        if layout.is_none() && corners.is_none() {
            return fail(
                self,
                "nothing to change (Position and Corner Type are Keep)",
            );
        }
        let mut trial = self.project.clone();
        let linked = self.frame_linked();
        match frame::edit_frame(&mut trial, &linked, self.frame_doc(), fid, layout, corners) {
            Ok(_) => {
                self.history.break_coalescing();
                self.checkpoint("Edit Frame");
                self.project = trial;
                self.history.break_coalescing();
                self.scene_dirty = true;
                self.file_status = Some(Ok("Edit Frame: done".into()));
                true
            }
            Err(e) => fail(self, &e),
        }
    }

    /// OK of Corner Joint, Trim/Extend To Face, Lengthen/Shorten and Notch.
    pub(super) fn treatment_ok(&mut self, kind: AsmPlanned, d: &PartDialog) -> bool {
        use frame::Treatment;
        let title = kind.title();
        let fail = |app: &mut Self, e: &str| {
            let msg = if e.starts_with(title) {
                e.to_owned()
            } else {
                format!("{title}: {e}")
            };
            app.file_status = Some(Err(msg));
            false
        };
        let num = |l: &str| d.num(0, l).unwrap_or(0.0);
        let ts: Vec<Treatment> = match kind {
            AsmPlanned::CornerJoint => {
                let (first, second) = (
                    self.picked_members(d, "first"),
                    self.picked_members(d, "second"),
                );
                // Two members selected before the command both land in the
                // first selector: they are the pair.
                let pair = match (first.as_slice(), second.as_slice()) {
                    ([a, ..], [b, ..]) => Some((*a, *b)),
                    ([a, b, ..], []) => Some((*a, *b)),
                    _ => None,
                };
                let Some((a, b)) = pair else {
                    return fail(self, "select the first and the second frame member");
                };
                let (a, b) = if d.check(0, "Flip Members") {
                    (b, a)
                } else {
                    (a, b)
                };
                vec![Treatment::CornerJoint {
                    a,
                    b,
                    offsets: [num("Offset1"), num("Offset2")],
                }]
            }
            AsmPlanned::Notch => {
                let (notched, tools) = (
                    self.picked_members(d, "notched"),
                    self.picked_members(d, "tools"),
                );
                // Two members selected before the command land in the first
                // selector: the first is notched by the second.
                let (notched, tools) = match (notched.as_slice(), tools.is_empty()) {
                    ([a, b], true) => (vec![*a], vec![*b]),
                    _ => (notched, tools),
                };
                if notched.is_empty() || tools.is_empty() {
                    return fail(self, "select the members to notch and the notching members");
                }
                let offset = num("Offset");
                notched
                    .iter()
                    .flat_map(|&n| {
                        tools
                            .iter()
                            .filter(move |&&b| b != n)
                            .map(move |&by| Treatment::Notch {
                                notched: n,
                                by,
                                offset,
                            })
                    })
                    .collect()
            }
            AsmPlanned::TrimExtend => {
                let Some(face) = d.refs.get("face").and_then(|v| v.first()).cloned() else {
                    return fail(self, "select the face to trim or extend to");
                };
                let members = self.picked_members(d, "members");
                if members.is_empty() {
                    return fail(self, "select the frame members");
                }
                members
                    .into_iter()
                    .map(|member| Treatment::TrimToFace {
                        member,
                        face: face.clone(),
                        offset: num("Offset"),
                    })
                    .collect()
            }
            _ => {
                let members = self.picked_members(d, "members");
                if members.is_empty() {
                    return fail(self, "select the frame members");
                }
                let (o1, o2) = (num("Offset"), num("Offset 2"));
                let kind = d.index(0, "Offset Type");
                let linked = self.frame_linked();
                let Some(a) = self.frame_asm() else {
                    return false;
                };
                let names: Vec<String> = d.picks_of("members").to_vec();
                let place = self.frame_place();
                members
                    .into_iter()
                    .map(|member| {
                        // The end nearer the click (Inventor picks a point
                        // near the end to change); the run's end otherwise.
                        let name = a.find(member).map(|o| o.name.clone()).unwrap_or_default();
                        let click = names
                            .iter()
                            .filter(|w| w.split(" · ").next() == Some(name.as_str()))
                            .find_map(|w| d.clicks.iter().rev().find(|(l, _)| l == w))
                            .map(|(_, p)| crate::kernel::Point3::new(p[0], p[1], p[2]))
                            .map(|c| place.inverse() * c);
                        let near =
                            match (click, frame::member_run(&self.project, &linked, a, member)) {
                                (Some(c), Some((s, e))) => {
                                    usize::from((c - e).norm() < (c - s).norm())
                                }
                                _ => 1,
                            };
                        let far = 1 - near;
                        let mut offsets = [0.0; 2];
                        match kind {
                            // Flipped: the other end.
                            1 => offsets[far] = o1,
                            // Symmetric: both ends.
                            2 => offsets = [o1, o1],
                            // Asymmetric: Offset at the picked end, Offset 2 at the other.
                            3 => {
                                offsets[near] = o1;
                                offsets[far] = o2;
                            }
                            _ => offsets[near] = o1,
                        }
                        Treatment::Lengthen { member, offsets }
                    })
                    .collect()
            }
        };
        let linked = self.frame_linked();
        let mut trial = self.frame_trial(d);
        let ts = match self.frame_treatments_down(&mut trial, &linked, ts) {
            Ok(v) => v,
            Err(e) => return fail(self, &e),
        };
        let delete = d.check(0, "Delete existing end treatments");
        match frame::add_treatments_replacing(&mut trial, &linked, self.frame_doc(), ts, delete) {
            Ok(_) => {
                self.history.break_coalescing();
                self.checkpoint(title);
                self.project = trial;
                self.history.break_coalescing();
                self.scene_dirty = true;
                self.file_status = Some(Ok(format!("{title}: done")));
                true
            }
            Err(e) => fail(self, &e),
        }
    }

    /// OK of Reuse: the source's part on every picked line.
    pub(super) fn reuse_ok(&mut self, d: &PartDialog) -> bool {
        let fail = |app: &mut Self, e: &str| {
            app.file_status = Some(Err(format!("Reuse: {e}")));
            false
        };
        let Some(&source) = self.picked_members(d, "source").first() else {
            return fail(self, "select the member to reuse");
        };
        if d.skeletons.is_empty() {
            return fail(self, "select the lines to reuse it on");
        }
        let linked = self.frame_linked();
        let mut trial = self.project.clone();
        let doc = self.frame_doc();
        let flip = d.check(0, "Flip Direction");
        let result = d
            .skeletons
            .iter()
            .map(|s| self.frame_skeleton_down(&mut trial, &linked, s))
            .collect::<Result<Vec<_>, _>>()
            .and_then(|lines| frame::reuse(&mut trial, &linked, doc, source, &lines, flip));
        match result {
            Ok(made) => {
                self.history.break_coalescing();
                self.checkpoint("Reuse");
                self.project = trial;
                self.history.break_coalescing();
                self.scene_dirty = true;
                self.file_status = Some(Ok(format!("Reuse: {} member(s)", made.len())));
                true
            }
            Err(e) => fail(self, &e),
        }
    }

    /// OK of Change Reuse: the picked reused members move to the picked
    /// lines (one each, in order) or only turn round with Flip Direction.
    pub(super) fn change_reuse_ok(&mut self, d: &PartDialog) -> bool {
        let fail = |app: &mut Self, e: &str| {
            app.file_status = Some(Err(format!("Change Reuse: {e}")));
            false
        };
        let members = self.picked_members(d, "members");
        if members.is_empty() {
            return fail(self, "select the reused members");
        }
        let linked = self.frame_linked();
        let mut trial = self.project.clone();
        let doc = self.frame_doc();
        let flip = d.check(0, "Flip Direction");
        let result = d
            .skeletons
            .iter()
            .map(|s| self.frame_skeleton_down(&mut trial, &linked, s))
            .collect::<Result<Vec<_>, _>>()
            .and_then(|lines| {
                frame::change_reuse(&mut trial, &linked, doc, &members, &lines, flip)
            });
        match result {
            Ok(_) => {
                self.frame_commit("Change Reuse", trial);
                self.file_status = Some(Ok(format!("Change Reuse: {} member(s)", members.len())));
                true
            }
            Err(e) => fail(self, &e),
        }
    }

    /// The Align pick of a frame panel as a direction in the frame's
    /// coordinates (`None`: nothing picked). Edges and sketch lines give their
    /// direction; faces are refused for now.
    pub(super) fn frame_align(&self, d: &PartDialog) -> Result<Option<[f64; 3]>, String> {
        let Some(g) = d.refs.get("align").and_then(|v| v.last()) else {
            return Ok(None);
        };
        let a = self.assembly().ok_or("no assembly is open")?;
        let linked = self.frame_linked();
        let (p, q) = match g {
            GeomRef::Edge { .. } => {
                frame::skeleton_line(&self.project, &linked, a, &Skeleton::Edge(g.clone()))?
            }
            _ => return Err("Align takes a straight edge or a sketch line yet".into()),
        };
        let w = self.frame_place().inverse() * (q - p);
        let w = w
            .try_normalize(1e-9)
            .ok_or("the Align edge has no length")?;
        Ok(Some([w.x, w.y, w.z]))
    }

    /// The View buttons of the frame panels (spec 6.1 §3.1): Look At
    /// Manipulator looks square to the last picked line or member, Zoom to
    /// Manipulator frames it, Initial View goes back to the view the panel
    /// opened with. The line for the panel's note.
    pub(super) fn frame_view_button(
        &mut self,
        d: &PartDialog,
        b: &str,
        ctx: &egui::Context,
    ) -> String {
        let before = self.camera.clone();
        if b == "Initial View" {
            let Some(v) = d.start_view.clone() else {
                return "Initial View: no view kept".into();
            };
            self.camera = v;
            self.view_cube.animate(before, &mut self.camera);
            ctx.request_repaint();
            return "Initial View".into();
        }
        let linked = self.frame_linked();
        let line = d
            .skeletons
            .last()
            .and_then(|s| {
                let a = self.assembly()?;
                frame::skeleton_run(&self.project, &linked, a, s)
                    .ok()
                    .map(|r| r.chord())
            })
            .or_else(|| {
                let key = ["members", "member", "notched", "first", "source"]
                    .into_iter()
                    .find(|k| !d.picks_of(k).is_empty())?;
                let occ = *self.picked_members(d, key).last()?;
                let (p, q) = frame::member_run(&self.project, &linked, self.frame_asm()?, occ)?;
                let place = self.frame_place();
                Some((place * p, place * q))
            });
        let Some((p, q)) = line else {
            return format!("{b}: pick a line or a member first");
        };
        let along = q - p;
        let len = along.norm().max(1.0);
        let mid = nalgebra::center(&p, &q);
        if b == "Look At Manipulator" {
            let z = crate::kernel::Vector3::z();
            let dir = along / len;
            // Square to the line, level with it (a vertical line: from -Y).
            let side = dir
                .cross(&z)
                .try_normalize(1e-6)
                .unwrap_or(-crate::kernel::Vector3::y());
            let up = if dir.z.abs() > 0.99 {
                dir
            } else {
                (z - side * side.dot(&z)).try_normalize(1e-9).unwrap_or(z)
            };
            self.camera.target = mid;
            self.camera.set_frame(side, up);
        } else {
            let r = crate::kernel::Vector3::repeat(len * 0.15);
            self.camera.fit(p.inf(&q) - r, p.sup(&q) + r);
        }
        self.view_cube.animate(before, &mut self.camera);
        ctx.request_repaint();
        b.to_owned()
    }

    /// OK of Refresh: the picked members (none: all) take their family's
    /// numbers from the section table read again from the user's folder.
    pub(super) fn frame_refresh_ok(&mut self, d: &PartDialog) -> bool {
        let members = self.picked_members(d, "members");
        let (lib, _) = Library::reload();
        let linked = self.frame_linked();
        let mut trial = self.project.clone();
        match frame::refresh(&mut trial, &linked, self.frame_doc(), &members, &lib) {
            Ok(done) if done.is_empty() => {
                self.file_status = Some(Ok("Refresh: every member is up to date".into()));
                true
            }
            Ok(done) => {
                self.frame_commit("Refresh", trial);
                self.file_status = Some(Ok(format!("Refresh: {} member(s) updated", done.len())));
                true
            }
            Err(e) => {
                self.file_status = Some(Err(format!("Refresh: {e}")));
                false
            }
        }
    }

    /// OK of Insert End Cap: a plate on every picked end face.
    pub(super) fn end_cap_ok(&mut self, d: &PartDialog) -> bool {
        let fail = |app: &mut Self, e: &str| {
            app.file_status = Some(Err(format!("Insert End Cap: {e}")));
            false
        };
        let refs: Vec<GeomRef> = d.refs.get("faces").cloned().unwrap_or_default();
        if refs.is_empty() && d.caps_edit.is_empty() {
            return fail(self, "select the end faces of the members");
        }
        let Some(a) = self.frame_asm() else {
            return fail(self, "the assembly has no frame");
        };
        let top = self.frame_occ();
        let mut ends = Vec::new();
        for g in &refs {
            let (member, hint) = match (g, top) {
                (
                    GeomRef::Face {
                        occurrence,
                        sub,
                        hint,
                        ..
                    },
                    Some(f),
                ) if *occurrence == f && sub.len() == 1 => (sub[0], *hint),
                (
                    GeomRef::Face {
                        occurrence,
                        sub,
                        hint,
                        ..
                    },
                    None,
                ) if sub.is_empty() => (*occurrence, *hint),
                _ => return fail(self, "that face is not on a frame member"),
            };
            let Some(high) = frame::member_end_at(a, member, hint[2]) else {
                return fail(self, "select a member's end face");
            };
            if !ends.contains(&(member, high)) {
                ends.push((member, high));
            }
        }
        let o = frame::CapOptions {
            internal: d.index(0, "Placement") == 0,
            corners: match d.index(0, "Profile") {
                1 => CapCorners::Chamfered,
                2 => CapCorners::Sharp,
                _ => CapCorners::Filleted,
            },
            thickness: d.num(0, "Thickness").unwrap_or(0.0),
            ratio: d.index(0, "Offset Type") == 1,
            edge_offset: d.num(0, "Edge Offset").unwrap_or(0.0),
            rotate: d.num(0, "Rotate").unwrap_or(0.0).to_radians(),
            offset: d.num(0, "Offset").unwrap_or(0.0),
        };
        let material = frame_choice(d, 0, "Material");
        let linked = self.frame_linked();
        let mut trial = self.project.clone();
        let doc = self.frame_doc();
        let edited = if d.caps_edit.is_empty() {
            Ok(())
        } else {
            frame::edit_caps(
                &mut trial,
                &linked,
                doc,
                &d.caps_edit,
                &o,
                material.as_deref(),
            )
            .map(|_| ())
        };
        let made = edited.and_then(|()| {
            if ends.is_empty() {
                Ok(d.caps_edit.clone())
            } else {
                frame::insert_caps(&mut trial, &linked, doc, &ends, &o, material.as_deref())
            }
        });
        match made {
            Ok(made) => {
                self.history.break_coalescing();
                self.checkpoint("Insert End Cap");
                self.project = trial;
                self.history.break_coalescing();
                self.scene_dirty = true;
                self.file_status = Some(Ok(format!("Insert End Cap: {} cap(s)", made.len())));
                true
            }
            Err(e) => fail(self, &e),
        }
    }

    /// OK of Remove End Treatments.
    pub(super) fn remove_treatments_ok(&mut self, d: &PartDialog) -> bool {
        let members = self.picked_members(d, "members");
        if members.is_empty() {
            self.file_status = Some(Err("Remove End Treatments: select frame members".into()));
            return false;
        }
        let linked = self.frame_linked();
        let mut trial = self.project.clone();
        match frame::remove_treatments(&mut trial, &linked, self.frame_doc(), &members) {
            Ok(_) => {
                self.history.break_coalescing();
                self.checkpoint("Remove End Treatments");
                self.project = trial;
                self.history.break_coalescing();
                self.scene_dirty = true;
                self.file_status = Some(Ok("Remove End Treatments: done".into()));
                true
            }
            Err(e) => {
                self.file_status = Some(Err(format!("Remove End Treatments: {e}")));
                false
            }
        }
    }
}

/// Screen distance from `p` to the segment `a`–`b`.
fn segment_distance(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 {
        ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + ab * t - p).length()
}

/// The straight edges a click on `g` takes (part coordinates): Face Edges =
/// those of its face; Body Edges = those of the one body of the part the
/// click hit (a part of several bodies gives that body only, rule 19).
pub(super) fn body_edges(
    p: &crate::part::PartDocument,
    g: &GeomRef,
    face: Option<&str>,
) -> Vec<crate::part::EdgeRef> {
    frame::straight_edges_in(p, face, body_of(p, g))
}

/// The body of `p` a click on `g` hit.
fn body_of(p: &crate::part::PartDocument, g: &GeomRef) -> Option<crate::kernel::SolidId> {
    match g {
        GeomRef::Face { name, hint, .. } => frame::body_at(p, name, Point3::from(*hint)),
        GeomRef::Edge { edge, .. } => frame::body_at(p, &edge.faces[0], Point3::from(edge.hint)),
        _ => None,
    }
}

/// The straight edges of the body under the cursor, kept per (component,
/// face, body, the part's face and edge counts): drawn every frame while Body
/// Edges hovers (rule 20).
type BodyLines = (u64, std::rc::Rc<Vec<(Point3, Point3)>>);

thread_local! {
    static BODY_HOVER: std::cell::RefCell<Option<BodyLines>> = const { std::cell::RefCell::new(None) };
}

impl CadCraftApp {
    /// Body Edges' hover: every straight edge of the body a click would take.
    fn draw_body_edges(&self, painter: &egui::Painter, rect: Rect, g: &GeomRef) {
        use std::hash::{Hash, Hasher};
        let Some((component, t)) = self.geom_part(g) else {
            return;
        };
        let lines = self
            .with_part(component, |p| {
                let solid = body_of(p, g);
                let mut h = std::collections::hash_map::DefaultHasher::new();
                component.hash(&mut h);
                solid.hash(&mut h);
                p.body.faces().count().hash(&mut h);
                p.body.edges().count().hash(&mut h);
                if let GeomRef::Face { name, .. } = g {
                    name.hash(&mut h);
                }
                let key = h.finish();
                if let Some(v) = BODY_HOVER.with(|c| {
                    c.borrow()
                        .as_ref()
                        .filter(|(k, _)| *k == key)
                        .map(|(_, v)| v.clone())
                }) {
                    return v;
                }
                let v: std::rc::Rc<Vec<(Point3, Point3)>> = std::rc::Rc::new(
                    frame::straight_edge_lines(p, None, solid)
                        .into_iter()
                        .map(|(_, a, b)| (a, b))
                        .collect(),
                );
                BODY_HOVER.with(|c| *c.borrow_mut() = Some((key, v.clone())));
                v
            })
            .unwrap_or_default();
        for (a, b) in lines.iter() {
            if let (Some(sa), Some(sb)) = (
                self.camera.project(&(t * a), rect),
                self.camera.project(&(t * b), rect),
            ) {
                painter.line_segment([sa, sb], Stroke::new(3.0, HOVER));
            }
        }
    }
}

/// The Frame Member Naming dialog (spec 6.1 §3.1): the inserted members wait
/// in `trial` until OK gives them their names; Cancel drops them.
pub(super) struct FrameNaming {
    /// The project with the new members.
    pub trial: crate::document::Project,
    /// The frame document they are in.
    pub doc: crate::document::DocumentId,
    /// Their names, edited in the dialog.
    pub rows: Vec<frame::MemberName>,
    /// The status line once committed.
    pub msg: String,
    /// The insert was an Apply: the panel stays open.
    pub apply: bool,
    /// Why the last OK was refused.
    pub error: Option<String>,
}

impl CadCraftApp {
    /// The Frame Member Naming dialog: Display Name and File Name of each new
    /// member; OK names them and commits the insert (one Undo step), Cancel
    /// returns to the panel without them.
    pub(super) fn frame_naming_window(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.asm.frame_naming.take() else {
            return;
        };
        let (mut ok, mut cancel) = (false, false);
        let win = egui::Window::new("Frame Member Naming")
            .collapsible(false)
            .resizable(true)
            .default_size([520.0, 300.0])
            .default_pos(ctx.content_rect().center() - egui::vec2(260.0, 150.0));
        self.layout
            .place("frame_naming", ctx.content_rect(), true, win)
            .show(ctx, |ui| {
                ui.label(format!("{} new member(s)", st.rows.len()));
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        egui::Grid::new("frame_naming_grid")
                            .num_columns(3)
                            .striped(true)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                ui.label("");
                                ui.strong("Display Name");
                                ui.strong("File Name");
                                ui.end_row();
                                for r in &mut st.rows {
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(16.0, 16.0),
                                        egui::Sense::hover(),
                                    );
                                    super::icons::paint(
                                        ui.painter(),
                                        rect,
                                        super::icons::Icon::Part,
                                        &super::icons::Palette::enabled(),
                                    );
                                    ui.add_sized(
                                        [200.0, 20.0],
                                        egui::TextEdit::singleline(&mut r.display),
                                    );
                                    ui.add_sized(
                                        [200.0, 20.0],
                                        egui::TextEdit::singleline(&mut r.file),
                                    );
                                    ui.end_row();
                                }
                            });
                    });
                ui.label(
                    egui::RichText::new(concat!(
                        "File Location: in this design file ",
                        "(Manage › Documents › Save to Separate File moves a part out)"
                    ))
                    .weak()
                    .small(),
                );
                if let Some(e) = &st.error {
                    ui.colored_label(super::theme::tokens().error, e);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ok = ui.add(super::theme::ok_button("OK")).clicked();
                    cancel = ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape));
                });
            });
        if cancel {
            self.file_status = Some(Ok("Insert Frame: cancelled, no member made".into()));
            return;
        }
        if !ok {
            self.asm.frame_naming = Some(st);
            return;
        }
        let mut trial = std::mem::replace(&mut st.trial, crate::document::Project::new(""));
        if let Err(e) = frame::rename_members(&mut trial, st.doc, &st.rows) {
            st.trial = trial;
            st.error = Some(format!("Frame Member Naming: {e}"));
            self.asm.frame_naming = Some(st);
            return;
        }
        let occs: Vec<u32> = st.rows.iter().map(|r| r.occurrence).collect();
        self.frame_commit_insert(trial, occs, st.msg);
        if st.apply {
            if let Some(d) = self.part_planned.as_mut() {
                d.applied();
            }
        } else {
            self.part_planned = None;
        }
    }

    /// Commits an Insert Frame on `trial` (one Undo step) with the new
    /// members selected.
    pub(super) fn frame_commit_insert(
        &mut self,
        trial: crate::document::Project,
        occs: Vec<u32>,
        msg: String,
    ) {
        self.history.break_coalescing();
        self.checkpoint("Insert Frame");
        self.project = trial;
        self.history.break_coalescing();
        self.frame_set_selected(occs);
        self.scene_dirty = true;
        self.file_status = Some(Ok(msg));
    }
}
