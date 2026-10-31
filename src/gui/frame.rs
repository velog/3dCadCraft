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
use crate::kernel::Point3;
use crate::part::frame::Library;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke};

/// Colour of component sketches in the assembly view.
const SKETCH: Color32 = Color32::from_rgb(70, 110, 170);
/// Colour of picked skeleton lines.
const PICKED: Color32 = Color32::from_rgb(110, 220, 120);
/// Colour of the line under the cursor.
const HOVER: Color32 = Color32::from_rgb(255, 160, 50);

/// Is `d` a frame panel whose lists come from the section table?
fn is_frame(d: &PartDialog) -> bool {
    matches!(
        d.kind,
        Planned::Asm(AsmPlanned::InsertFrame | AsmPlanned::FrameChange)
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
        _ => None,
    }
}

/// The chosen entry of list `label` (clamped to the list).
pub(super) fn frame_choice(d: &PartDialog, ti: usize, label: &str) -> Option<String> {
    let list = frame_combo(d, ti, label)?;
    let i = d.index(ti, label).min(list.len().checked_sub(1)?);
    list.into_iter().nth(i)
}

/// The Orientation group of the panel.
fn orientation(d: &PartDialog) -> Orientation {
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
    d.frame_sets.push(super::part_planned::FrameSet {
        skeletons: std::mem::take(&mut d.skeletons),
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
                    if ui
                        .small_button("x")
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
        material: super::asm_planned::choice(d, 0, "Material").map(str::to_owned),
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
    })
}

impl CadCraftApp {
    /// OK of Insert Frame: a member on every picked edge. False (with the
    /// reason in the status bar) leaves everything as it was.
    pub(super) fn frame_ok(&mut self, d: &PartDialog) -> bool {
        if !d.members.list.is_empty() {
            return self.edit_members_ok(d);
        }
        let fail = |app: &mut Self, e: &str| {
            app.file_status = Some(Err(format!("Insert Frame: {e}")));
            false
        };
        if d.refs.get("align").is_some_and(|v| !v.is_empty()) {
            return fail(self, "Align comes with stage F2 (spec 6.1)");
        }
        let Some(o) = insert_options(d) else {
            return fail(self, "the section table is empty");
        };
        // The kept sets, then the lines picked now (with the panel's section).
        let mut sets: Vec<(Vec<Skeleton>, InsertOptions)> = d
            .frame_sets
            .iter()
            .map(|s| (s.skeletons.clone(), s.options.clone()))
            .collect();
        if !d.skeletons.is_empty() {
            sets.push((d.skeletons.clone(), o.clone()));
        }
        if sets.is_empty() {
            return fail(self, "select the skeleton edges or sketch lines");
        }
        let mut trial = self.project.clone();
        let linked = self.linked();
        let lib = Library::current();
        let corners = o.corners;
        // One frame per assembly (Inventor): new members join the frame of a
        // selected member, else the last one; only the first insert makes one.
        let existing = self.assembly().and_then(|a| {
            a.frames
                .iter()
                .find(|f| {
                    f.members
                        .iter()
                        .any(|m| self.asm.sel.contains(&m.occurrence))
                })
                .or(a.frames.last())
                .map(|f| f.id)
        });
        let result = (|| {
            let mut frame_id = existing;
            let mut occs = Vec::new();
            // Several sets, or members joining a frame: the corners once all
            // are in (between the new members and the frame's).
            let many = sets.len() > 1 || existing.is_some();
            for (lines, mut opts) in sets {
                if many {
                    // The corners come once all sets are in.
                    opts.corners = Default::default();
                }
                let (fid, made) = frame::insert(
                    &mut trial,
                    &linked,
                    self.part_id,
                    &lines,
                    &opts,
                    &lib,
                    frame_id,
                )?;
                frame_id = Some(fid);
                occs.extend(made);
            }
            if let Some(fid) = frame_id.filter(|_| many)
                && (corners.kind != frame::CornerType::None
                    || corners.tees != frame::TeeJoint::None)
            {
                frame::add_corners(&mut trial, &linked, self.part_id, fid, &occs, &corners)?;
            }
            Ok::<_, String>((frame_id, occs))
        })();
        match result {
            Ok((_, occs)) => {
                self.history.break_coalescing();
                self.checkpoint("Insert Frame");
                self.project = trial;
                self.history.break_coalescing();
                self.asm.sel = occs.clone();
                self.asm.sub.clear();
                self.scene_dirty = true;
                self.file_status = Some(Ok(format!(
                    "Insert Frame: {} member(s) of {} {}",
                    occs.len(),
                    o.family,
                    o.size
                )));
                true
            }
            Err(e) => fail(self, &e),
        }
    }

    /// Members follow their skeleton (after the skeleton part was edited);
    /// members that cannot are reported in the log and keep their state.
    pub(super) fn follow_frames(&mut self) {
        if !self.assembly().is_some_and(|a| !a.frames.is_empty()) {
            return;
        }
        let linked = self.linked();
        match frame::solve(&mut self.project, &linked, self.part_id) {
            Ok(s) => {
                for (occ, why) in &s.sick {
                    log::warn!("frame member {occ}: {why}");
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

    /// The straight lines of the visible sketches of the components on show
    /// (first-level occurrences), in assembly coordinates, with what picks
    /// each one.
    pub(super) fn sketch_lines(&self) -> Vec<(Skeleton, String, Point3, Point3)> {
        let Some(a) = self.assembly() else {
            return Vec::new();
        };
        // Each component's lines once (part coordinates: sketch id, name,
        // line, ends), then per occurrence.
        type Local = Vec<(String, String, crate::sketch::EntityId, Point3, Point3)>;
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
                            if let crate::sketch::Entity::Line { start, end } = *e
                                && let (Some(a), Some(b)) = (sk.point(start), sk.point(end))
                            {
                                v.push((
                                    ps.id.clone(),
                                    ps.name.clone(),
                                    id,
                                    sk.to_world(&a.position),
                                    sk.to_world(&b.position),
                                ));
                            }
                        }
                    }
                    v
                })
                .unwrap_or_default()
            });
            let t = o.transform;
            for (sketch, name, line, pa, pb) in local.iter() {
                out.push((
                    Skeleton::SketchLine {
                        occurrence: o.id,
                        sub: Vec::new(),
                        sketch: sketch.clone(),
                        line: *line,
                    },
                    format!("{} · {name} line", o.name),
                    t.transform_point(pa),
                    t.transform_point(pb),
                ));
            }
        }
        out
    }

    /// Is Insert Frame open with its edges selector active?
    fn frame_takes_lines(&self) -> bool {
        self.part_planned.as_ref().is_some_and(|d| {
            d.kind == Planned::Asm(AsmPlanned::InsertFrame) && d.active == Some("edges")
        })
    }

    /// The sketch line nearest `p` on screen, within the snap radius.
    fn sketch_line_under(&self, p: Pos2, rect: Rect) -> Option<(Skeleton, String, Point3, Point3)> {
        let near = super::overlay::CURVE_SNAP_PX;
        self.sketch_lines()
            .into_iter()
            .filter_map(|l| {
                let a = self.camera.project(&l.2, rect)?;
                let b = self.camera.project(&l.3, rect)?;
                let d = segment_distance(p, a, b);
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
        let linked = self.linked();
        // The line under the cursor: a sketch line, else a model edge.
        let line = self
            .sketch_line_under(p, rect)
            .map(|l| (l.0, l.2, l.3))
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
            painter.circle(q, 5.0, PICKED, Stroke::new(1.5, Color32::WHITE));
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
        let mut groups: Vec<(&[Skeleton], InsertOptions)> = d
            .frame_sets
            .iter()
            .map(|s| (s.skeletons.as_slice(), s.options.clone()))
            .collect();
        if let Some(o) = insert_options(d) {
            groups.push((d.skeletons.as_slice(), o));
        }
        for (skeletons, o) in groups {
            if !skeletons.is_empty() {
                self.preview_set(skeletons, &o, painter, rect);
            }
        }
    }

    /// The wire preview of one set's members.
    fn preview_set(
        &self,
        skeletons: &[Skeleton],
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
        let linked = self.linked();
        let lines: Vec<(Point3, Point3)> = skeletons
            .iter()
            .filter_map(|s| frame::skeleton_line(&self.project, &linked, a, s).ok())
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
        for &(p, q) in &lines {
            let ori = o.layout.orient(&o.orientation, p, q, centre);
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

    pub(super) fn frame_overlay(&self, painter: &egui::Painter, rect: Rect, pointer: Option<Pos2>) {
        let seg = |a: &Point3, b: &Point3| -> Option<[Pos2; 2]> {
            Some([self.camera.project(a, rect)?, self.camera.project(b, rect)?])
        };
        let lines = self.sketch_lines();
        for (_, _, a, b) in &lines {
            if let Some(s) = seg(a, b) {
                painter.line_segment(s, Stroke::new(1.5, SKETCH));
            }
        }
        let Some(d) = self.part_planned.as_ref() else {
            return;
        };
        if d.kind != Planned::Asm(AsmPlanned::InsertFrame) {
            return;
        }
        if d.members.list.is_empty() {
            self.frame_preview(d, painter, rect);
        }
        self.between_start_dot(d, painter, rect);
        for s in &d.skeletons {
            match s {
                Skeleton::Edge(g) => self.draw_geom(painter, rect, g, PICKED),
                Skeleton::Between { .. } => {
                    if let Some(a) = self.assembly()
                        && let Ok((p, q)) =
                            frame::skeleton_line(&self.project, &self.linked(), a, s)
                        && let Some(s) = seg(&p, &q)
                    {
                        painter.line_segment(s, Stroke::new(3.0, PICKED));
                    }
                }
                Skeleton::SketchLine { .. } => {
                    if let Some((_, _, a, b)) = lines.iter().find(|l| &l.0 == s)
                        && let Some(s) = seg(a, b)
                    {
                        painter.line_segment(s, Stroke::new(3.0, PICKED));
                    }
                }
            }
        }
        if self.frame_takes_lines()
            && let Some(p) = pointer
            && let Some((_, _, a, b)) = self.sketch_line_under(p, rect)
            && let Some(s) = seg(&a, &b)
        {
            painter.line_segment(s, Stroke::new(3.0, HOVER));
            return;
        }
        // The model edge (Select: Edge) or face (Face / Body Edges) a click
        // would take; nothing else lights up (spec 0-ui rule 19).
        if d.active == Some("edges")
            && let Some(p) = pointer
            && let Some(g) = self.geom_under_pub(p, rect)
        {
            let face = matches!(g, crate::assembly::relation::GeomRef::Face { .. });
            if !face || d.index(0, "Select") != 0 {
                self.draw_geom(painter, rect, &g, HOVER);
            }
        }
    }
}

impl CadCraftApp {
    /// The frame members picked in selector `key` (occurrence ids).
    pub(super) fn picked_members(&self, d: &PartDialog, key: &str) -> Vec<u32> {
        let Some(a) = self.assembly() else {
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
        let linked = self.linked();
        let pairs = match members.as_slice() {
            [a, b] => vec![(*a, *b)],
            _ => match self.assembly() {
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
        let mut trial = self.frame_trial(d);
        match frame::add_miter(&mut trial, &linked, self.part_id, &pairs, gap, offset) {
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
        let Some(fid) = self.assembly().and_then(|a| {
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
        let linked = self.linked();
        match frame::edit_frame(&mut trial, &linked, self.part_id, fid, layout, corners) {
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
                let linked = self.linked();
                let Some(a) = self.assembly() else {
                    return false;
                };
                let names: Vec<String> = d.picks_of("members").to_vec();
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
                            .map(|(_, p)| crate::kernel::Point3::new(p[0], p[1], p[2]));
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
        let linked = self.linked();
        let mut trial = self.frame_trial(d);
        match frame::add_treatments(&mut trial, &linked, self.part_id, ts) {
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

    /// OK of Remove End Treatments.
    pub(super) fn remove_treatments_ok(&mut self, d: &PartDialog) -> bool {
        let members = self.picked_members(d, "members");
        if members.is_empty() {
            self.file_status = Some(Err("Remove End Treatments: select frame members".into()));
            return false;
        }
        let linked = self.linked();
        let mut trial = self.project.clone();
        match frame::remove_treatments(&mut trial, &linked, self.part_id, &members) {
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
