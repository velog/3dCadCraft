//! The Hole and Thread property panels (`docs/spec/2.3-hole.md`,
//! `docs/spec/2.9-thread.md`): Input Geometry, Type, Behavior, a dimension
//! picture with the fields next to it, Threads / Fastener; preview, OK /
//! Apply / Cancel; Properties of existing features.

use eframe::egui::{self, RichText, Stroke};

use super::CadCraftApp;
use super::icons::Icon;
use super::panels::{Fields, field_row, plabel, psection, warning_box};
use crate::kernel::Real;
use crate::part::hole::{self, HoleCenter};
use crate::part::standards::Standards;
use crate::part::{
    ClearanceSpec, DrillPoint, FastenerFit, Feature, HoleSpec, HoleType, SeatType, Termination,
    ThreadSpec,
};

/// A combo box over `items` (shown text, value); returns the picked value.
fn combo<T: Clone + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    current: &T,
    shown: &str,
    items: &[(String, T)],
) -> Option<T> {
    let mut out = None;
    egui::ComboBox::from_id_salt(id)
        .selected_text(shown)
        .width(150.0)
        .show_ui(ui, |ui| {
            for (text, v) in items {
                if ui.selectable_label(v == current, text).clicked() {
                    out = Some(v.clone());
                }
            }
        });
    out
}

/// The thread chooser (Inventor's Threads section): type, size,
/// designation, class, hand. Returns the new thread when one changed.
fn thread_choice(
    ui: &mut egui::Ui,
    id: &str,
    t: &ThreadSpec,
    internal: bool,
    taper: bool,
) -> Option<ThreadSpec> {
    let tables = Standards::global();
    let mut out: Option<ThreadSpec> = None;
    let keep = |mut n: ThreadSpec| {
        n.full_depth = t.full_depth;
        n.depth = t.depth;
        n.right_hand = t.right_hand;
        n.modeled = t.modeled;
        n.clearance = t.clearance;
        n
    };
    plabel(ui, "Type");
    // Taper Tapped holes take the taper pipe threads, the others the
    // straight ones.
    let types: Vec<(String, String)> = tables
        .threads
        .iter()
        .filter(|x| x.taper == taper)
        .map(|x| (x.name.clone(), x.name.clone()))
        .collect();
    if let Some(ty) = combo(
        ui,
        &format!("{id}_type"),
        &t.thread_type,
        &t.thread_type,
        &types,
    ) && ty != t.thread_type
        && let Some(first) = tables.thread_table(&ty).and_then(|x| x.rows.first())
    {
        out = ThreadSpec::from_table(tables, &ty, &first.designation, internal).map(keep);
    }
    ui.end_row();
    let Some(table) = tables.thread_table(&t.thread_type) else {
        return out;
    };
    plabel(ui, "Size");
    let sizes: Vec<(String, String)> = table
        .sizes()
        .into_iter()
        .map(|s| (s.to_owned(), s.to_owned()))
        .collect();
    if let Some(sz) = combo(ui, &format!("{id}_size"), &t.size, &t.size, &sizes)
        && sz != t.size
        && let Some(first) = table.of_size(&sz).next()
    {
        out =
            ThreadSpec::from_table(tables, &t.thread_type, &first.designation, internal).map(keep);
    }
    ui.end_row();
    plabel(ui, "Designation");
    let des: Vec<(String, String)> = table
        .of_size(&t.size)
        .map(|r| (r.designation.clone(), r.designation.clone()))
        .collect();
    if let Some(dz) = combo(
        ui,
        &format!("{id}_des"),
        &t.designation,
        &t.designation,
        &des,
    ) && dz != t.designation
    {
        out = ThreadSpec::from_table(tables, &t.thread_type, &dz, internal).map(keep);
    }
    ui.end_row();
    plabel(ui, "Class");
    let classes: Vec<(String, String)> = table
        .row(&t.designation)
        .map(|r| {
            if internal {
                r.classes_internal.clone()
            } else {
                r.classes_external.clone()
            }
        })
        .unwrap_or_default()
        .into_iter()
        .map(|c| (c.clone(), c))
        .collect();
    if let Some(c) = combo(ui, &format!("{id}_class"), &t.class, &t.class, &classes) {
        let mut n = out.clone().unwrap_or_else(|| t.clone());
        n.class = c;
        out = Some(n);
    }
    ui.end_row();
    plabel(ui, "Direction");
    let mut rh = t.right_hand;
    ui.horizontal(|ui| {
        ui.radio_value(&mut rh, true, "Right Hand");
        ui.radio_value(&mut rh, false, "Left Hand");
    });
    if rh != t.right_hand {
        let mut n = out.clone().unwrap_or_else(|| t.clone());
        n.right_hand = rh;
        out = Some(n);
    }
    ui.end_row();
    plabel(ui, "Diameters");
    ui.label(format!(
        "major {} · minor {} · tap drill {} · P {}",
        hole::num(t.major, 3),
        hole::num(t.minor, 3),
        hole::num(t.tap_drill, 3),
        hole::num(t.pitch, 3)
    ))
    .on_hover_text(
        "From the standards table (data/standards, or your own copy in the user standards folder)",
    );
    ui.end_row();
    out
}

/// A tool's half profile: (radius, z) points.
type HoleTool = Vec<([Real; 2], &'static str)>;

/// Is (r, z) strictly inside the half profile `tool`?
fn in_tool(tool: &HoleTool, q: [Real; 2]) -> bool {
    let mut inside = false;
    for k in 0..tool.len() {
        let (a, b) = (tool[k].0, tool[(k + 1) % tool.len()].0);
        // On the boundary is not inside: two tools share their walls.
        let (dr, dz) = (b[0] - a[0], b[1] - a[1]);
        let len2 = (dr * dr + dz * dz).max(1e-18);
        let u = (((q[0] - a[0]) * dr + (q[1] - a[1]) * dz) / len2).clamp(0.0, 1.0);
        let (er, ez) = (a[0] + u * dr - q[0], a[1] + u * dz - q[1]);
        if er * er + ez * ez < 1e-12 {
            return false;
        }
        if (a[1] > q[1]) != (b[1] > q[1]) && q[0] < a[0] + (q[1] - a[1]) / dz * dr {
            inside = !inside;
        }
    }
    inside
}

/// The hole's outline in the section, as pieces in (radius, z): the tools' edges without
/// the axis, the openings at the face (and at the far face `open_at`) and what lies inside
/// another tool, so a counterbore's floor stops at the bore and no line crosses the hole.
fn hole_outline(tools: &[HoleTool], open_at: Option<Real>) -> Vec<[[Real; 2]; 2]> {
    const PIECES: usize = 24;
    let mut out = Vec::new();
    for (i, tool) in tools.iter().enumerate() {
        for k in 0..tool.len() {
            let (a, b) = (tool[k].0, tool[(k + 1) % tool.len()].0);
            if a[0].abs() < 1e-9 && b[0].abs() < 1e-9 {
                continue;
            }
            for n in 0..PIECES {
                let at = |u: Real| [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u];
                let (s, e) = (n as Real / PIECES as Real, (n + 1) as Real / PIECES as Real);
                let mid = at((s + e) / 2.0);
                let open = mid[1] > -1e-9 || open_at.is_some_and(|z| mid[1] < z + 1e-9);
                let hidden = tools
                    .iter()
                    .enumerate()
                    .any(|(j, other)| j != i && in_tool(other, mid));
                if !open && !hidden {
                    out.push([at(s), at(e)]);
                }
            }
        }
    }
    out
}

/// The hole's section with its sizes (Inventor's dimension picture), drawn to show
/// its shape, not to scale: a small bore in a deep hole must still read. The
/// seat, the bore, the drill point and the far side countersink, with dimension lines.
/// Every size the hole takes as a field is a label that opens its expression
/// field right there (Enter applies, Esc closes); the commit goes with the
/// panel's other fields (`fx.commits`).
fn dimension_picture(ui: &mut egui::Ui, spec: &HoleSpec, fx: &mut Fields, f: &Feature) {
    use crate::part::params::{feature_field_param, feature_field_unit, feature_value};
    let k = super::theme::tokens();
    let width = ui.available_width().clamp(220.0, 300.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 178.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, k.field);
    let through = !matches!(spec.termination, Termination::Distance);
    let r = spec.bore_radius();
    // A through hole is drawn in a plate it leaves (so the far side shows).
    let depth = match spec.termination {
        Termination::Distance => spec.depth.max(1e-3),
        _ => (r * 5.0).max(1.0),
    };
    let mut tools = spec.tools(depth, 0.0, !through);
    if spec.far_csink && through {
        tools.insert(tools.len() - 1, spec.far_tool(depth, 0.0));
    }
    let pts: Vec<[Real; 2]> = tools.iter().flatten().map(|x| x.0).collect();
    let rmax = pts.iter().map(|q| q[0]).fold(1e-3, Real::max);
    let zmin = pts.iter().map(|q| q[1]).fold(0.0, Real::min).min(-depth);
    let cx = rect.center().x;
    let top = rect.top() + 32.0;
    let foot = rect.bottom() - 26.0;
    let half = if rmax > r * 1.05 { 46.0 } else { 26.0 };
    let (sx, sz) = (half / rmax, f64::from(foot - top) / (-zmin).max(1e-3));
    let to = |r: Real, z: Real| egui::pos2(cx + (r * sx) as f32, top - (z * sz) as f32);
    // The material: from the face down (to the far face when the hole goes through).
    let bottom = if through {
        to(0.0, -depth).y
    } else {
        rect.bottom() - 10.0
    };
    let block = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 8.0, top),
        egui::pos2(rect.right() - 8.0, bottom),
    );
    let inside = ui.painter_at(block);
    inside.rect_filled(block, 0.0, k.control);
    let hatch = Stroke::new(1.0, k.line);
    let mut x = block.left() - block.height();
    while x < block.right() {
        inside.line_segment(
            [
                egui::pos2(x, block.bottom()),
                egui::pos2(x + block.height(), block.top()),
            ],
            hatch,
        );
        x += 9.0;
    }
    for t in &tools {
        for side in [1.0, -1.0] {
            let poly: Vec<egui::Pos2> = t.iter().map(|q| to(side * q.0[0], q.0[1])).collect();
            inside.add(egui::Shape::convex_polygon(poly, k.field, Stroke::NONE));
        }
    }
    // The outline, and the faces up to it.
    let pen = Stroke::new(1.5, k.text);
    let open_at = through.then_some(-depth);
    for [a, b] in hole_outline(&tools, open_at) {
        for side in [1.0, -1.0] {
            inside.line_segment([to(side * a[0], a[1]), to(side * b[0], b[1])], pen);
        }
    }
    let width_at = |z: Real| {
        tools
            .iter()
            .flatten()
            .filter(|q| (q.0[1] - z).abs() < 1e-9)
            .map(|q| q.0[0])
            .fold(0.0, Real::max)
    };
    let face = width_at(0.0);
    let far = if through {
        width_at(-depth).max(r)
    } else {
        0.0
    };
    let hline = |y: f32, r: Real| {
        p.line_segment(
            [egui::pos2(block.left(), y), egui::pos2(to(-r, 0.0).x, y)],
            pen,
        );
        p.line_segment(
            [egui::pos2(to(r, 0.0).x, y), egui::pos2(block.right(), y)],
            pen,
        );
    };
    hline(top, face);
    if through {
        hline(bottom, far);
    }
    let axis = Stroke::new(1.0, k.text_weak);
    p.extend(egui::Shape::dashed_line(
        &[
            egui::pos2(cx, top - 8.0),
            egui::pos2(cx, (to(0.0, zmin).y + 6.0).min(rect.bottom() - 2.0)),
        ],
        axis,
        7.0,
        3.0,
    ));
    // Dimension lines.
    let thin = Stroke::new(1.0, k.text_weak);
    let arrow = |at: egui::Pos2, dir: egui::Vec2| {
        let side = egui::vec2(-dir.y, dir.x) * 2.5;
        p.add(egui::Shape::convex_polygon(
            vec![at, at - dir * 6.0 + side, at - dir * 6.0 - side],
            k.text_weak,
            Stroke::NONE,
        ));
    };
    // Across a width at `y`, with extension lines from `from` (arrows point at them).
    let across = |x0: f32, x1: f32, y: f32, from: f32| {
        for x in [x0, x1] {
            p.line_segment([egui::pos2(x, from), egui::pos2(x, y)], thin);
        }
        p.line_segment([egui::pos2(x0, y), egui::pos2(x1, y)], thin);
        arrow(egui::pos2(x0, y), egui::vec2(-1.0, 0.0));
        arrow(egui::pos2(x1, y), egui::vec2(1.0, 0.0));
    };
    // Along a depth at `x`, from `y0` to `y1`, with extension lines from `from`.
    let along = |x: f32, y0: f32, y1: f32, from: f32| {
        for y in [y0, y1] {
            p.line_segment([egui::pos2(from, y), egui::pos2(x, y)], thin);
        }
        p.line_segment([egui::pos2(x, y0), egui::pos2(x, y1)], thin);
        arrow(egui::pos2(x, y0), egui::vec2(0.0, -1.0));
        arrow(egui::pos2(x, y1), egui::vec2(0.0, 1.0));
    };
    // The sizes: (field, text, anchor, alignment).
    let n = |v: Real| hole::num(v, 2);
    let right = to(rmax, 0.0).x + 16.0;
    let left = to(-rmax, 0.0).x - 14.0;
    // Where the bore's own wall begins under a seat.
    let seat_floor = match spec.seat {
        SeatType::Counterbore | SeatType::Spotface => spec.seat_depth,
        SeatType::Countersink => (spec.csink_diameter / 2.0 - r) / (spec.csink_angle / 2.0).tan(),
        SeatType::None => 0.0,
    };
    let bore_y = to(0.0, -(seat_floor + depth) / 2.0).y;
    let (wall_l, wall_r) = (to(-r, 0.0).x, to(r, 0.0).x);
    p.line_segment(
        [
            egui::pos2(wall_l - 12.0, bore_y),
            egui::pos2(wall_r, bore_y),
        ],
        thin,
    );
    arrow(egui::pos2(wall_l, bore_y), egui::vec2(1.0, 0.0));
    arrow(egui::pos2(wall_r, bore_y), egui::vec2(-1.0, 0.0));
    let mut labels: Vec<(&'static str, String, egui::Pos2, egui::Align2)> = vec![(
        "diameter",
        format!("Ø{}", n(2.0 * r)),
        egui::pos2(wall_l - 14.0, bore_y),
        egui::Align2::RIGHT_CENTER,
    )];
    let depth_text = match spec.termination {
        Termination::Distance => n(spec.depth),
        Termination::ThroughAll => "THRU".into(),
        Termination::To(_) => "To".into(),
    };
    let depth_y = to(0.0, -depth).y;
    along(right, top, depth_y, to(r, 0.0).x);
    labels.push((
        "depth",
        depth_text,
        egui::pos2(right + 5.0, (top + depth_y) / 2.0),
        egui::Align2::LEFT_CENTER,
    ));
    let band = top - 15.0;
    match spec.seat {
        SeatType::Counterbore | SeatType::Spotface => {
            across(to(-face, 0.0).x, to(face, 0.0).x, band, top);
            labels.push((
                "seat_diameter",
                format!("Ø{}", n(spec.seat_diameter)),
                egui::pos2(to(face, 0.0).x + 6.0, band),
                egui::Align2::LEFT_CENTER,
            ));
            let floor = to(0.0, -spec.seat_depth).y;
            along(left, top, floor, to(-face, 0.0).x);
            labels.push((
                "seat_depth",
                n(spec.seat_depth),
                egui::pos2(left - 5.0, (top + floor) / 2.0),
                egui::Align2::RIGHT_CENTER,
            ));
        }
        SeatType::Countersink => {
            across(to(-face, 0.0).x, to(face, 0.0).x, band, top);
            labels.push((
                "csink_diameter",
                format!("Ø{}", n(spec.csink_diameter)),
                egui::pos2(to(face, 0.0).x + 6.0, band),
                egui::Align2::LEFT_CENTER,
            ));
            let on_cone = to(-(face + r) / 2.0, -seat_floor / 2.0);
            p.line_segment([on_cone, egui::pos2(left, on_cone.y)], thin);
            labels.push((
                "csink_angle",
                format!("{}°", hole::num(spec.csink_angle.to_degrees(), 1)),
                egui::pos2(left - 2.0, on_cone.y),
                egui::Align2::RIGHT_CENTER,
            ));
        }
        SeatType::None => {}
    }
    if !through && spec.drill_point.angled() {
        labels.push((
            "point_angle",
            format!("{}°", hole::num(spec.point_angle.to_degrees(), 1)),
            egui::pos2(cx, to(0.0, zmin).y + 3.0),
            egui::Align2::CENTER_TOP,
        ));
    }
    if spec.far_csink && through {
        let low = bottom + 14.0;
        across(to(-far, 0.0).x, to(far, 0.0).x, low, bottom);
        labels.push((
            "far_csink_diameter",
            format!("Ø{}", n(spec.far_diameter())),
            egui::pos2(to(far, 0.0).x + 6.0, low),
            egui::Align2::LEFT_CENTER,
        ));
        labels.push((
            "far_csink_angle",
            format!("{}°", hole::num(spec.far_angle().to_degrees(), 1)),
            egui::pos2(left - 2.0, bottom - 8.0),
            egui::Align2::RIGHT_CENTER,
        ));
    }
    let takes = |field: &str| match f {
        Feature::Hole(h) => h.fields().iter().any(|(x, _)| *x == field),
        _ => false,
    };
    let font = egui::FontId::proportional(11.0);
    let (ink, blue) = (k.text, k.link);
    let edit_id = egui::Id::new(("hole_picture_edit", fx.key.as_str()));
    let editing: Option<&'static str> = ui.data(|d| d.get_temp(edit_id));
    let mut open: Option<&'static str> = None;
    for (field, text, at, align) in &labels {
        let editable = takes(field);
        let g = p.layout_no_wrap(
            text.clone(),
            font.clone(),
            if editable { blue } else { ink },
        );
        let lr = align.anchor_size(*at, g.size()).expand(2.0);
        p.rect_filled(lr, 2.0, k.field);
        p.galley(lr.min + egui::vec2(2.0, 2.0), g, ink);
        if !editable {
            continue;
        }
        let resp = ui
            .interact(lr, edit_id.with(*field), egui::Sense::click())
            .on_hover_text("Click to edit (expressions and parameters)");
        if resp.hovered() {
            p.rect_stroke(lr, 2.0, Stroke::new(1.0, blue), egui::StrokeKind::Outside);
        }
        if resp.clicked() {
            open = Some(field);
        }
        if editing == Some(*field) {
            // The field itself, over the label.
            let pm = feature_field_param(f, field);
            let name = pm
                .as_ref()
                .map(|p| p.name.as_str())
                .filter(|n| !n.is_empty());
            // An id of its own: the same field in the list below is on another layer, and
            // one widget on two layers is an egui assertion (a crash in debug builds).
            let fs = super::valuefield::FieldSpec {
                id: egui::Id::new(("hole_picture_field", fx.key.as_str(), *field)),
                value: feature_value(f, field).unwrap_or_default(),
                unit: feature_field_unit(f, field).unwrap_or_default(),
                name,
                expr: pm.as_ref().and_then(|p| p.expr.as_deref()),
                table: fx.table,
            };
            let mut done = ui.input(|i| i.key_pressed(egui::Key::Escape));
            egui::Area::new(edit_id.with(("area", *field)))
                .fixed_pos(lr.left_top() - egui::vec2(4.0, 4.0))
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if let Some(t) = super::valuefield::value_field(ui, &fs) {
                                fx.commits.push((field, t));
                                done = true;
                            }
                            // Once, and only now that the field exists: focus on a widget
                            // this frame does not have stops AccessKit ("not in the node list").
                            let first = edit_id.with("focus");
                            if ui.data_mut(|d| d.remove_temp::<bool>(first)).is_some() {
                                ui.memory_mut(|m| m.request_focus(fs.id.with("te")));
                            }
                            done |= super::theme::glyph(ui, super::theme::Glyph::Close)
                                .on_hover_text("Close (Esc)")
                                .clicked();
                        });
                    });
                });
            if done {
                ui.data_mut(|d| d.remove::<&'static str>(edit_id));
            }
        }
    }
    if let Some(field) = open {
        ui.data_mut(|d| {
            d.insert_temp(edit_id, field);
            d.insert_temp(edit_id.with("focus"), true);
        });
    }
}

/// Sets the hole type, filling the values from the tables (Clearance:
/// ISO 273 M6 Normal; Tapped: M8).
/// A Taper Tapped hole's depth for thread `t`: its thread length and a
/// quarter more, to the next whole millimetre.
fn taper_depth(t: &ThreadSpec) -> f64 {
    (t.depth * 1.25).ceil()
}

fn set_type(spec: &mut HoleSpec, ty: HoleType) {
    let tables = Standards::global();
    spec.hole_type = ty;
    match ty {
        HoleType::Clearance => {
            let c = spec.clearance.clone().unwrap_or(ClearanceSpec {
                standard: "ISO 273".into(),
                fastener: String::new(),
                size: "M6".into(),
                fit: FastenerFit::Normal,
            });
            if let Some(row) = tables.clearance(&c.standard, &c.size) {
                spec.diameter = row.diameter(c.fit);
            }
            spec.clearance = Some(c);
        }
        HoleType::Tapped => {
            let t = spec
                .thread
                .clone()
                .filter(|t| !t.taper)
                .unwrap_or_else(ThreadSpec::default_metric);
            spec.diameter = t.minor;
            spec.thread = Some(t);
        }
        HoleType::TaperTapped => {
            if let Some(t) = spec
                .thread
                .clone()
                .filter(|t| t.taper)
                .or_else(ThreadSpec::default_taper)
            {
                spec.diameter = t.minor;
                spec.depth = spec.depth.max(taper_depth(&t));
                spec.thread = Some(t);
            }
            // No counterbore or spotface on a taper tapped hole (Inventor).
            if matches!(spec.seat, SeatType::Counterbore | SeatType::Spotface) {
                spec.seat = SeatType::None;
            }
        }
        HoleType::Simple => {}
    }
}

/// Is this preview message a problem? Not while nothing is picked yet: an empty
/// message (no draft) or the request for the first centre, which the status bar
/// shows anyway. A warning before any action reads as an error.
fn is_warning(message: &str) -> bool {
    !message.is_empty() && message != super::hole::HOLE_NO_CENTRES
}

impl CadCraftApp {
    /// The Hole command's panel.
    pub(super) fn hole_dialog(&mut self, ui: &mut egui::Ui) {
        use super::hole::{
            ALLOW_CREATE, KEEP_VISIBLE, PRESELECT, WINDOW_CENTERS, option, set_option,
        };
        let Some(mut d) = self.hole_draft.clone() else {
            return;
        };
        let title = match d.edit {
            Some(_) => d.def.name.clone(),
            None => "Hole".into(),
        };
        self.panel_header(ui, Icon::Hole, &title);
        // Breadcrumb `Sketch > Hole1` and the panel menu (Inventor).
        let mut to_sketch = false;
        let mut rename: Option<String> = None;
        ui.horizontal(|ui| {
            let has_sketch = !d.def.sketch.is_empty() || !d.points.is_empty();
            if ui
                .add_enabled(has_sketch, egui::Button::new("Sketch").small())
                .on_hover_text("Finish the hole and edit its sketch (dimensions, more centre points)")
                .clicked()
            {
                to_sketch = true;
            }
            ui.label("›");
            let id = egui::Id::new("hole_crumb_rename");
            let mut editing: Option<String> = ui.data(|x| x.get_temp(id));
            match editing.as_mut() {
                Some(text) => {
                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(90.0));
                    if !r.has_focus() && !r.lost_focus() {
                        r.request_focus();
                    }
                    if r.lost_focus() {
                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !text.trim().is_empty() {
                            rename = Some(text.trim().to_owned());
                        }
                        editing = None;
                    }
                }
                None => {
                    let r = ui
                        .add(egui::Button::new(RichText::new(&title).strong()).small())
                        .on_hover_text("Click twice to rename");
                    if r.double_clicked() && d.edit.is_some() {
                        editing = Some(title.clone());
                    }
                }
            }
            ui.data_mut(|x| match &editing {
                Some(t) => {
                    x.insert_temp(id, t.clone());
                }
                None => x.remove::<String>(id),
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("…", |ui| {
                    for (flag, text, tip) in [
                        (
                            &PRESELECT,
                            "Preselect sketch center points",
                            "On start, the unused Center Points of the visible sketch are the centres",
                        ),
                        (
                            &KEEP_VISIBLE,
                            "Keep sketch visible on (+)",
                            "Apply (+) shares the hole's sketch for the next hole; Cancel unshares it",
                        ),
                        (
                            &WINDOW_CENTERS,
                            "Window select center points only",
                            "A window selection takes Center Points only",
                        ),
                    ] {
                        let mut on = option(flag);
                        if ui.checkbox(&mut on, text).on_hover_text(tip).changed() {
                            set_option(flag, on);
                        }
                    }
                });
            });
        });
        let tables = Standards::global();
        let bodies = self
            .part()
            .bodies_before(d.edit.unwrap_or(self.part().insert_index()));
        let table = self.part().parameter_table();
        let mut dim_changes: Vec<(super::hole_dims::DimRef, Real, Option<String>)> = Vec::new();
        psection(ui, "hole_input", "Input Geometry", |ui| {
            plabel(ui, "Positions");
            ui.horizontal(|ui| {
                ui.label(format!("{} centre(s)", d.count())).on_hover_text(
                    "Click a planar face (a centre where you click; a click on a centre removes it), then straight edges to dimension it; points of a visible sketch; a circular edge or a cylindrical face (concentric)",
                );
                let mut allow = option(&ALLOW_CREATE);
                if ui
                    .checkbox(&mut allow, "Create")
                    .on_hover_text("Allow Center Point Creation: a click on a planar face places a centre; off: existing sketch points only")
                    .changed()
                {
                    set_option(&ALLOW_CREATE, allow);
                }
                if d.edit.is_none()
                    && d.count() > 0
                    && super::theme::glyph(ui, super::theme::Glyph::Close).on_hover_text("Clear Selections").clicked()
                {
                    d.points.clear();
                    d.dims.clear();
                    d.face = None;
                    d.def.centers.clear();
                    d.def.sketch.clear();
                }
            });
            ui.end_row();
            // Add / Remove: what a click on a centre does (Inventor's
            // Positions selector; Ctrl+click removes there).
            plabel(ui, "Mode");
            ui.horizontal(|ui| {
                ui.radio_value(&mut d.remove, false, "Add")
                    .on_hover_text("Clicks add centres");
                ui.radio_value(&mut d.remove, true, "Remove")
                    .on_hover_text("Clicks on centres remove them");
            });
            ui.end_row();
            // The centres one by one, each removable (the sketch of an
            // existing hole is hidden, so its points cannot be clicked).
            let mut drop_center: Option<usize> = None;
            for (k, c) in d.def.centers.iter().enumerate() {
                plabel(ui, &format!("Centre {}", k + 1));
                ui.horizontal(|ui| {
                    ui.label(match c {
                        crate::part::HoleCenter::Point(_) => "sketch point",
                        crate::part::HoleCenter::Concentric(_) => "concentric to an edge",
                        crate::part::HoleCenter::Axis(_) => "on a cylinder's axis",
                    });
                    if super::theme::glyph(ui, super::theme::Glyph::Close)
                        .on_hover_text("Remove this centre")
                        .clicked()
                    {
                        drop_center = Some(k);
                    }
                });
                ui.end_row();
            }
            if let Some(k) = drop_center {
                d.def.centers.remove(k);
                if !d
                    .def
                    .centers
                    .iter()
                    .any(|c| matches!(c, crate::part::HoleCenter::Point(_)))
                    && d.edit.is_none()
                {
                    d.def.sketch.clear();
                }
            }
            let mut drop_point: Option<usize> = None;
            for k in 0..d.points.len() {
                plabel(ui, &format!("New centre {}", k + 1));
                ui.horizontal(|ui| {
                    ui.label("clicked on the face");
                    if super::theme::glyph(ui, super::theme::Glyph::Close)
                        .on_hover_text("Remove this centre")
                        .clicked()
                    {
                        drop_point = Some(k);
                    }
                });
                ui.end_row();
            }
            if let Some(k) = drop_point {
                d.points.remove(k);
                d.dims.retain(|x| x.center != k);
                for x in &mut d.dims {
                    if x.center > k {
                        x.center -= 1;
                    }
                }
                if d.points.is_empty() {
                    d.face = None;
                }
            }
            if let Some(f) = &d.face {
                plabel(ui, "Face");
                ui.label(f);
                ui.end_row();
            }
            let mut unlink: Option<usize> = None;
            for x in d.dim_views() {
                // Dimensions of this command are numbered #1, #2… (as in the
                // view); the sketch's carry their parameter names.
                let label = match (x.r, x.centre) {
                    (super::hole_dims::DimRef::Draft(j), super::hole_dims::CentreRef::Draft(k)) => {
                        format!("#{} (centre {})", j + 1, k + 1)
                    }
                    (super::hole_dims::DimRef::Sketch(id), _) => {
                        d.sk.as_ref()
                            .map(|c| c.sketch.dim_param(id).name)
                            .filter(|n| !n.is_empty())
                            .unwrap_or_else(|| "Centre dim".to_owned())
                    }
                    _ => "Centre dim".to_owned(),
                };
                plabel(ui, &label);
                let j = x.r;
                // A dimension that follows another one of this command.
                let follows = match j {
                    super::hole_dims::DimRef::Draft(k) => d.dims.get(k).and_then(|y| y.link),
                    _ => None,
                };
                if let (Some(src), super::hole_dims::DimRef::Draft(k)) = (follows, j) {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("= #{}   ({})", src + 1, hole::num(x.value, 3)))
                                .strong(),
                        )
                        .on_hover_text(format!(
                            "Follows dimension #{}; at OK its equation is that dimension's name",
                            src + 1
                        ));
                        if super::theme::glyph(ui, super::theme::Glyph::Close)
                            .on_hover_text("Stop following (keep the value)")
                            .clicked()
                        {
                            unlink = Some(k);
                        }
                    });
                    ui.end_row();
                    continue;
                }
                // A reference (driven) dimension: shown, not edited.
                if x.driven {
                    ui.label(RichText::new(format!("({})", hole::num(x.value, 3))).weak())
                        .on_hover_text("A reference dimension of the hole's sketch (driven)");
                    ui.end_row();
                    continue;
                }
                let mut v = x.value;
                let mut eq = x.expr.clone();
                super::valuefield::set_frame_table(ui.ctx(), table.clone());
                let r = ui.add(
                    super::valuefield::ExprValue::new(&mut v)
                        .equation(&mut eq)
                        .range(0.0..=1e6)
                        .suffix(" mm"),
                );
                r.on_hover_text(concat!(
                    "Distance to the edge. In the view: double-click the value to edit it, ",
                    "click another dimension meanwhile to follow it (d2 = d1), ",
                    "right-click for Redefine / Delete"
                ));
                if (v - x.value).abs() > 1e-12 || eq != x.expr {
                    dim_changes.push((j, v, eq));
                }
                ui.end_row();
            }
            if let Some(k) = unlink
                && let Some(y) = d.dims.get_mut(k)
            {
                y.link = None;
            }
            if bodies.len() > 1 {
                plabel(ui, "Solids");
                let current = d.def.solids.first().cloned();
                let mut items: Vec<(String, Option<String>)> = vec![("All".into(), None)];
                items.extend(bodies.iter().map(|(id, n)| (n.clone(), Some(id.clone()))));
                let shown = items
                    .iter()
                    .find(|(_, v)| *v == current)
                    .map_or("All".into(), |(t, _)| t.clone());
                if let Some(v) = combo(ui, "hole_solid", &current, &shown, &items) {
                    d.def.solids = v.into_iter().collect();
                }
                ui.end_row();
            }
        });
        for (j, v, eq) in dim_changes {
            d.set_dim_value(j, v, eq);
        }
        if let Some(name) = rename
            && let Some(k) = d.edit
        {
            self.checkpoint("Rename");
            match self.part_mut().rename_feature(k, &name) {
                Ok(()) => d.def.name = name,
                Err(e) => {
                    self.history.cancel();
                    self.file_status = Some(Err(format!("Rename: {e}")));
                }
            }
        }
        if to_sketch {
            self.hole_draft = Some(d);
            self.hole_to_sketch();
            return;
        }
        let spec = &mut d.def.spec;
        psection(ui, "hole_type", "Type", |ui| {
            plabel(ui, "Hole");
            let items: Vec<(String, HoleType)> = HoleType::ALL
                .iter()
                .map(|t| (t.label().to_owned(), *t))
                .collect();
            if let Some(t) = combo(
                ui,
                "hole_kind",
                &spec.hole_type,
                spec.hole_type.label(),
                &items,
            ) {
                set_type(spec, t);
            }
            ui.end_row();
            plabel(ui, "Seat");
            let items: Vec<(String, SeatType)> = SeatType::ALL
                .iter()
                .map(|t| (t.label().to_owned(), *t))
                .collect();
            if let Some(t) = combo(ui, "hole_seat", &spec.seat, spec.seat.label(), &items) {
                spec.seat = t;
                let r = spec.bore_radius();
                if spec.seat_diameter <= 2.0 * r {
                    spec.seat_diameter = 4.0 * r;
                }
                if spec.csink_diameter <= 2.0 * r {
                    spec.csink_diameter = 4.0 * r;
                }
            }
            ui.end_row();
        });
        if spec.hole_type == HoleType::Clearance {
            psection(ui, "hole_fastener", "Fastener", |ui| {
                let mut c = spec.clearance.clone().unwrap_or(ClearanceSpec {
                    standard: "ISO 273".into(),
                    fastener: String::new(),
                    size: "M6".into(),
                    fit: FastenerFit::Normal,
                });
                plabel(ui, "Standard");
                let items: Vec<(String, String)> = tables
                    .clearance_standards()
                    .into_iter()
                    .map(|s| (s.to_owned(), s.to_owned()))
                    .collect();
                if let Some(s) = combo(ui, "hole_std", &c.standard, &c.standard.clone(), &items) {
                    if s != c.standard {
                        c.size = tables
                            .clearance
                            .iter()
                            .find(|r| r.standard == s)
                            .map(|r| r.size.clone())
                            .unwrap_or_default();
                    }
                    c.standard = s;
                }
                ui.end_row();
                plabel(ui, "Size");
                let items: Vec<(String, String)> = tables
                    .clearance
                    .iter()
                    .filter(|r| r.standard == c.standard)
                    .map(|r| (r.size.clone(), r.size.clone()))
                    .collect();
                if let Some(s) = combo(ui, "hole_size", &c.size, &c.size.clone(), &items) {
                    c.size = s;
                }
                ui.end_row();
                plabel(ui, "Fit");
                let items: Vec<(String, FastenerFit)> = FastenerFit::ALL
                    .iter()
                    .map(|f| (f.label().to_owned(), *f))
                    .collect();
                if let Some(f) = combo(ui, "hole_fit", &c.fit, c.fit.label(), &items) {
                    c.fit = f;
                }
                ui.end_row();
                if let Some(row) = tables.clearance(&c.standard, &c.size) {
                    spec.diameter = row.diameter(c.fit);
                }
                spec.clearance = Some(c);
            });
        }
        // Seats from the tables: the fastener types that have this size.
        let size = spec
            .clearance
            .as_ref()
            .filter(|_| spec.hole_type == HoleType::Clearance)
            .map(|c| c.size.clone())
            .or_else(|| spec.tapped_thread().map(|t| t.size.clone()));
        if let (Some(size), SeatType::Counterbore | SeatType::Countersink) = (&size, spec.seat) {
            let rows = if spec.seat == SeatType::Counterbore {
                &tables.counterbores
            } else {
                &tables.countersinks
            };
            let fasteners: Vec<String> = rows
                .iter()
                .filter(|r| r.size == *size)
                .map(|r| r.fastener.clone())
                .collect();
            if !fasteners.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Seat for").weak());
                    for f in fasteners {
                        if ui
                            .small_button(&f)
                            .on_hover_text("Fill the seat from the standards table")
                            .clicked()
                        {
                            if spec.seat == SeatType::Counterbore {
                                spec.counterbore_from(tables, &f, size);
                            } else {
                                spec.countersink_from(tables, &f, size);
                            }
                            if let Some(c) = spec.clearance.as_mut() {
                                c.fastener = f.clone();
                            }
                        }
                    }
                });
            }
        }
        psection(ui, "hole_behavior", "Behavior", |ui| {
            plabel(ui, "Termination");
            let kind = |t: &Termination| match t {
                Termination::Distance => 0,
                Termination::ThroughAll => 1,
                Termination::To(_) => 2,
            };
            let mut k = kind(&spec.termination);
            ui.horizontal_wrapped(|ui| {
                ui.radio_value(&mut k, 0, "Distance");
                ui.radio_value(&mut k, 1, "Through All");
                ui.radio_value(&mut k, 2, "To");
            });
            if k != kind(&spec.termination) {
                match k {
                    0 => spec.termination = Termination::Distance,
                    1 => spec.termination = Termination::ThroughAll,
                    _ => d.to_pick = true,
                }
            }
            ui.end_row();
            if k == 2 {
                plabel(ui, "To face");
                ui.horizontal(|ui| {
                    let text = match &spec.termination {
                        Termination::To(f) => f.face.clone(),
                        _ => "click a face".into(),
                    };
                    ui.label(text);
                    if ui
                        .selectable_label(d.to_pick, "Pick")
                        .on_hover_text(
                            "The next click on a face is where the hole ends (a planar face's plane, or a curved face)",
                        )
                        .clicked()
                    {
                        d.to_pick = !d.to_pick;
                    }
                    ui.checkbox(&mut spec.extend_to, "Extend To").on_hover_text(
                        "On: the face's whole plane ends the hole; off: the hole must meet the face",
                    );
                });
                ui.end_row();
            }
            plabel(ui, "Along");
            ui.horizontal(|ui| {
                use crate::part::{LineRef, PlaneRef, PullRef};
                let text = match &d.def.direction {
                    None => "square to the face".to_owned(),
                    Some(PullRef::Plane(PlaneRef::Face(n))) => format!("normal of {n}"),
                    Some(PullRef::Plane(p)) => format!("normal of {p:?}"),
                    Some(PullRef::Line(LineRef::Edge(n))) => format!("edge {n}"),
                    Some(PullRef::Line(l)) => format!("{l:?}"),
                };
                ui.label(text);
                if ui
                    .selectable_label(d.dir_pick, "Pick")
                    .on_hover_text(
                        "The next click on a planar face (its normal) or a straight edge sets the drilling direction",
                    )
                    .clicked()
                {
                    d.dir_pick = !d.dir_pick;
                    d.to_pick = false;
                }
                if d.def.direction.is_some()
                    && super::theme::glyph(ui, super::theme::Glyph::Close)
                        .on_hover_text("Square to the face again")
                        .clicked()
                {
                    d.def.direction = None;
                }
            });
            ui.end_row();
            plabel(ui, "Direction");
            let mut dir2 = spec.flip;
            ui.horizontal(|ui| {
                ui.radio_value(&mut dir2, false, "Direction 1");
                ui.radio_value(&mut dir2, true, "Direction 2");
                let can_sym = spec.termination == Termination::ThroughAll
                    && spec.seat == SeatType::None
                    && matches!(spec.hole_type, HoleType::Simple | HoleType::Clearance);
                if can_sym {
                    ui.checkbox(&mut spec.symmetric, "Symmetric")
                        .on_hover_text("Through all in both directions");
                } else {
                    spec.symmetric = false;
                }
            });
            spec.flip = dir2;
            ui.end_row();
            plabel(ui, "Drill Point");
            let items: Vec<(String, DrillPoint)> = DrillPoint::ALL
                .iter()
                .map(|x| (x.label().to_owned(), *x))
                .collect();
            if let Some(v) = combo(
                ui,
                "hole_point",
                &spec.drill_point,
                spec.drill_point.label(),
                &items,
            ) {
                spec.drill_point = v;
            }
            ui.end_row();
            plabel(ui, "Advanced");
            ui.checkbox(&mut spec.extend_start, "Extend Start").on_hover_text(
                "The hole starts above its start plane even inside the material (removes leftover slivers)",
            );
            ui.end_row();
            plabel(ui, "Far side");
            ui.checkbox(&mut spec.far_csink, "Countersink").on_hover_text(
                "A countersink (chamfer) where the hole comes out of the body; its size is in Dimensions",
            );
            ui.end_row();
        });
        if let Some(t) = spec
            .thread
            .clone()
            .filter(|_| matches!(spec.hole_type, HoleType::Tapped | HoleType::TaperTapped))
        {
            psection(ui, "hole_threads", "Threads", |ui| {
                let taper = spec.hole_type == HoleType::TaperTapped;
                if let Some(n) = thread_choice(ui, "hole_thread", &t, true, taper) {
                    spec.diameter = n.minor;
                    if taper {
                        spec.depth = spec.depth.max(taper_depth(&n));
                    }
                    spec.thread = Some(n);
                }
                let th = spec.thread.get_or_insert(t);
                plabel(ui, "Depth");
                ui.checkbox(&mut th.full_depth, "Full Depth");
                ui.end_row();
                plabel(ui, "Model");
                ui.checkbox(&mut th.modeled, "Modeled thread").on_hover_text(
                    "Real helical geometry (for 3D printing); off: a cosmetic thread (data and a display hint)",
                );
                ui.end_row();
            });
        }
        let table = self.part().parameter_table();
        let mut fx = Fields {
            table: &table,
            key: "hole_draft".into(),
            commits: Vec::new(),
            bodies: Vec::new(),
        };
        let snapshot = Feature::Hole(d.def.clone());
        let spec = d.def.spec.clone();
        psection(ui, "hole_dims", "Dimensions", |ui| {
            for (field, label) in [
                ("diameter", "Diameter"),
                ("depth", "Depth"),
                ("point_angle", "Point angle"),
                (
                    "seat_diameter",
                    if spec.seat == SeatType::Spotface {
                        "SF diameter"
                    } else {
                        "CBore diameter"
                    },
                ),
                (
                    "seat_depth",
                    if spec.seat == SeatType::Spotface {
                        "SF depth"
                    } else {
                        "CBore depth"
                    },
                ),
                ("csink_diameter", "CSink diameter"),
                ("csink_angle", "CSink angle"),
                ("far_csink_diameter", "Far CSink diameter"),
                ("far_csink_angle", "Far CSink angle"),
                ("thread_depth", "Thread depth"),
                ("thread_clearance", "Thread clearance"),
            ] {
                if let Feature::Hole(h) = &snapshot
                    && h.fields().iter().any(|(f, _)| *f == field)
                {
                    field_row(ui, &mut fx, label, &snapshot, field);
                }
            }
        });
        dimension_picture(ui, &spec, &mut fx, &snapshot);
        ui.label(RichText::new(format!("Note: {}", spec.note_ascii())).weak())
            .on_hover_text("The hole note a drawing writes (the symbols as text here)");
        // Invalid values in red (Inventor), e.g. a thread longer than the hole.
        if let Err(e) = spec.validate() {
            ui.label(RichText::new(format!("Hole: {e}")).color(super::theme::tokens().error));
        }
        let commits = std::mem::take(&mut fx.commits);
        drop(fx);
        for (field, text) in commits {
            let mut f = Feature::Hole(d.def.clone());
            match self.part().apply_draft_field(&mut f, field, &text) {
                Ok(()) => {
                    if let Feature::Hole(h) = f {
                        d.def = h;
                    }
                    self.file_status = None;
                }
                Err(e) => self.file_status = Some(Err(format!("Hole: {e}"))),
            }
        }
        self.hole_draft = Some(d.clone());
        let preview = self.hole_preview();
        self.ok_apply_cancel(ui, &preview, d.edit.is_none(), Cmd::Hole);
    }

    /// The Thread command's panel.
    pub(super) fn thread_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(mut d) = self.thread_draft.clone() else {
            return;
        };
        self.panel_header(ui, Icon::Thread, "Thread");
        let internal = d
            .face
            .as_ref()
            .and_then(|r| hole::cylinder_face(&self.part().body, r).ok())
            .is_some_and(|c| c.internal);
        psection(ui, "thread_input", "Input Geometry", |ui| {
            plabel(ui, "Face");
            ui.horizontal_wrapped(|ui| {
                let text = match (&d.face, d.more.len()) {
                    (None, _) => "click a cylindrical face".to_owned(),
                    (Some(f), 0) => f.face.clone(),
                    (Some(_), n) => format!("{} faces", n + 1),
                };
                ui.label(text).on_hover_text(
                    "Click near the end the thread starts at; another face of the same diameter adds it",
                );
                if !d.more.is_empty()
                    && ui
                        .small_button("x")
                        .on_hover_text("Keep only the first face")
                        .clicked()
                {
                    d.more.clear();
                }
            });
            ui.end_row();
        });
        psection(ui, "thread_spec", "Specification", |ui| {
            if let Some(n) = thread_choice(ui, "thread", &d.spec, internal, false) {
                d.spec = n;
            }
        });
        psection(ui, "thread_depth", "Depth", |ui| {
            plabel(ui, "Length");
            ui.checkbox(&mut d.spec.full_depth, "Full Length");
            ui.end_row();
            plabel(ui, "Model");
            ui.checkbox(&mut d.spec.modeled, "Modeled thread")
                .on_hover_text("Real helical geometry (for 3D printing)");
            ui.end_row();
            plabel(ui, "Display");
            ui.checkbox(&mut d.display, "Display thread in model");
            ui.end_row();
        });
        let table = self.part().parameter_table();
        let mut fx = Fields {
            table: &table,
            key: "thread_draft".into(),
            commits: Vec::new(),
            bodies: Vec::new(),
        };
        let mut snap = crate::part::ThreadFeature::new(
            d.face.clone().unwrap_or(crate::part::FaceRef {
                face: String::new(),
                hint: [0.0; 3],
            }),
            d.spec.clone(),
        );
        snap.offset = d.offset;
        let fields = snap.fields();
        let snapshot = Feature::Thread(snap);
        if !fields.is_empty() {
            psection(ui, "thread_vals", "Values", |ui| {
                for (field, label) in [
                    ("offset", "Offset"),
                    ("length", "Length"),
                    ("clearance", "Thread clearance"),
                ] {
                    if fields.iter().any(|(f, _)| *f == field) {
                        field_row(ui, &mut fx, label, &snapshot, field);
                    }
                }
            });
        }
        ui.label(RichText::new(format!("Note: {}", d.spec.note())).weak());
        let commits = std::mem::take(&mut fx.commits);
        drop(fx);
        for (field, text) in commits {
            let mut f = snapshot.clone();
            match self.part().apply_draft_field(&mut f, field, &text) {
                Ok(()) => {
                    if let Feature::Thread(t) = f {
                        d.spec = t.spec;
                        d.offset = t.offset;
                    }
                    self.file_status = None;
                }
                Err(e) => self.file_status = Some(Err(format!("Thread: {e}"))),
            }
        }
        self.thread_draft = Some(d.clone());
        let preview = self.thread_preview();
        self.ok_apply_cancel(ui, &preview, d.edit.is_none(), Cmd::Thread);
    }

    /// The preview state and OK / Apply / Cancel of Hole and Thread.
    fn ok_apply_cancel(
        &mut self,
        ui: &mut egui::Ui,
        preview: &Result<Vec<Vec<crate::kernel::Point3>>, String>,
        apply: bool,
        cmd: Cmd,
    ) {
        match preview {
            Err(e) if e == super::PREVIEW_PENDING => {
                ui.label(e);
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(50));
            }
            Err(e) if is_warning(e) => warning_box(ui, e),
            _ => {}
        }
        super::theme::panel_footer(ui, |ui| {
            let ok = preview.is_ok() || matches!(preview, Err(e) if e == super::PREVIEW_PENDING);
            // Enter / Ctrl+Enter: OK; +: create it and start another (Inventor).
            let plus = !ui.ctx().egui_wants_keyboard_input()
                && ui.input(|i| i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals));
            let key_ok = super::keys::panel_ok_key(ui, ok);
            if (ui
                .add_enabled(
                    ok,
                    super::theme::ok_button("OK").min_size(egui::vec2(60.0, 22.0)),
                )
                .on_hover_text("Enter, Ctrl+Enter")
                .clicked()
                || key_ok)
                && !(ok && plus && apply)
            {
                match cmd {
                    Cmd::Hole => self.commit_hole(false),
                    Cmd::Thread => self.commit_thread(false),
                }
            }
            if apply
                && ui
                    .add_enabled(
                        ok,
                        egui::Button::new("Apply").min_size(egui::vec2(60.0, 22.0)),
                    )
                    .on_hover_text("Create it and start a new one with the same settings (+)")
                    .clicked()
                || (apply && ok && plus)
            {
                match cmd {
                    Cmd::Hole => self.commit_hole(true),
                    Cmd::Thread => self.commit_thread(true),
                }
            }
            if ui
                .add(egui::Button::new("Cancel").min_size(egui::vec2(60.0, 22.0)))
                .clicked()
            {
                self.cancel_hole();
                self.thread_draft = None;
                self.end_feature_edit();
            }
        });
    }

    /// Properties of a Hole: what it is, its note, values with expressions.
    pub(super) fn hole_properties(
        &mut self,
        ui: &mut egui::Ui,
        k: usize,
        h: crate::part::HoleFeature,
    ) {
        self.panel_header(ui, Icon::Hole, &h.name);
        if let Some(e) = self.part().feature_health.get(&h.id) {
            warning_box(ui, e);
        }
        let s = &h.spec;
        psection(ui, &format!("hole_in_{k}"), "Hole", |ui| {
            plabel(ui, "Type");
            ui.label(format!("{}, {}", s.hole_type.label(), s.seat.label()));
            ui.end_row();
            plabel(ui, "Termination");
            ui.label(s.termination.label());
            ui.end_row();
            plabel(ui, "Centres");
            ui.label(format!(
                "{} ({})",
                h.centers.len(),
                if h.centers
                    .iter()
                    .all(|c| matches!(c, HoleCenter::Concentric(_)))
                {
                    "concentric".to_owned()
                } else {
                    h.sketch.clone()
                }
            ));
            ui.end_row();
            if let Some(t) = s.tapped_thread() {
                plabel(ui, "Thread");
                ui.label(format!(
                    "{}{}",
                    t.note(),
                    if t.modeled { ", modeled" } else { ", cosmetic" }
                ));
                ui.end_row();
            }
            plabel(ui, "Note");
            ui.label(s.note_ascii());
            ui.end_row();
        });
        let table = self.part().parameter_table();
        let mut fx = Fields {
            table: &table,
            key: format!("feature{k}"),
            commits: Vec::new(),
            bodies: Vec::new(),
        };
        let f = Feature::Hole(h.clone());
        psection(ui, &format!("hole_val_{k}"), "Dimensions", |ui| {
            for (field, _) in h.fields() {
                field_row(ui, &mut fx, field_label(field), &f, field);
            }
        });
        let commits = std::mem::take(&mut fx.commits);
        drop(fx);
        self.commit_feature_fields(k, commits);
        if ui.button("Edit Feature").clicked() {
            self.edit_feature_dialog(k);
        }
        if ui.button("Delete").clicked() {
            self.delete_feature(k);
        }
    }

    /// Properties of a Thread.
    pub(super) fn thread_properties(
        &mut self,
        ui: &mut egui::Ui,
        k: usize,
        t: crate::part::ThreadFeature,
    ) {
        self.panel_header(ui, Icon::Thread, &t.name);
        if let Some(e) = self.part().feature_health.get(&t.id) {
            warning_box(ui, e);
        }
        psection(ui, &format!("thread_in_{k}"), "Thread", |ui| {
            plabel(ui, "Face");
            if t.faces.is_empty() {
                ui.label(&t.face.face);
            } else {
                ui.label(format!("{} faces", 1 + t.faces.len()))
                    .on_hover_text(
                        std::iter::once(&t.face)
                            .chain(&t.faces)
                            .map(|f| f.face.as_str())
                            .collect::<Vec<_>>()
                            .join(
                                "
",
                            ),
                    );
            }
            ui.end_row();
            plabel(ui, "Thread");
            ui.label(t.spec.note());
            ui.end_row();
            plabel(ui, "Kind");
            ui.label(if t.spec.modeled {
                "Modeled"
            } else {
                "Cosmetic"
            });
            ui.end_row();
            plabel(ui, "Length");
            ui.label(if t.spec.full_depth {
                "Full Length".to_owned()
            } else {
                format!(
                    "{} from {}",
                    hole::num(t.spec.depth, 2),
                    hole::num(t.offset, 2)
                )
            });
            ui.end_row();
        });
        let table = self.part().parameter_table();
        let mut fx = Fields {
            table: &table,
            key: format!("feature{k}"),
            commits: Vec::new(),
            bodies: Vec::new(),
        };
        let f = Feature::Thread(t.clone());
        let fields = t.fields();
        if !fields.is_empty() {
            psection(ui, &format!("thread_val_{k}"), "Values", |ui| {
                for (field, _) in fields {
                    field_row(ui, &mut fx, field_label(field), &f, field);
                }
            });
        }
        let commits = std::mem::take(&mut fx.commits);
        drop(fx);
        self.commit_feature_fields(k, commits);
        if ui.button("Edit Feature").clicked() {
            self.edit_feature_dialog(k);
        }
        if ui.button("Delete").clicked() {
            self.delete_feature(k);
        }
    }

    /// Field edits of an existing feature: one Undo step each, a failure
    /// cancels it.
    fn commit_feature_fields(&mut self, k: usize, commits: Vec<(&'static str, String)>) {
        for (field, text) in commits {
            self.checkpoint("Edit Feature");
            match self.part_mut().set_feature_field(k, field, &text) {
                Ok(()) => {
                    if let Err(e) = self.part_mut().regenerate() {
                        self.file_status = Some(Err(e.to_string()));
                    }
                    self.scene_dirty = true;
                }
                Err(e) => {
                    self.history.cancel();
                    self.file_status = Some(Err(format!("Edit Feature: {e}")));
                }
            }
        }
    }
}

/// Which command's buttons.
#[derive(Clone, Copy)]
enum Cmd {
    Hole,
    Thread,
}

/// The label of a hole / thread field.
fn field_label(field: &str) -> &'static str {
    match field {
        "diameter" => "Diameter",
        "depth" => "Depth",
        "point_angle" => "Point angle",
        "seat_diameter" => "Seat diameter",
        "seat_depth" => "Seat depth",
        "csink_diameter" => "CSink diameter",
        "csink_angle" => "CSink angle",
        "far_csink_diameter" => "Far CSink diameter",
        "far_csink_angle" => "Far CSink angle",
        "thread_depth" => "Thread depth",
        "thread_clearance" | "clearance" => "Thread clearance",
        "offset" => "Offset",
        "length" => "Length",
        _ => "Value",
    }
}

#[cfg(test)]
mod tests {
    use super::is_warning;

    #[test]
    fn no_warning_before_the_first_centre() {
        assert!(!is_warning(super::super::hole::HOLE_NO_CENTRES));
        assert!(!is_warning(""));
        assert!(is_warning("Hole: the centre is not on the part"));
    }
}

#[cfg(test)]
mod picture_tests {
    use super::*;

    fn counterbore() -> Vec<HoleTool> {
        vec![
            vec![
                ([0.0, 0.0], "Top"),
                ([6.0, 0.0], "CBore"),
                ([6.0, -2.0], "CBoreFloor"),
                ([0.0, -2.0], "Axis"),
            ],
            vec![
                ([0.0, 0.0], "Top"),
                ([3.0, 0.0], "Bore"),
                ([3.0, -10.0], "Floor"),
                ([0.0, -10.0], "Axis"),
            ],
        ]
    }

    #[test]
    fn no_line_crosses_the_hole() {
        let pieces = hole_outline(&counterbore(), None);
        assert!(!pieces.is_empty());
        for [a, b] in &pieces {
            let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            assert!(mid[1] < 0.0, "the opening at the face: {mid:?}");
            // The counterbore's floor stops at the bore, whose wall starts at the floor.
            if (mid[1] + 2.0).abs() < 1e-9 {
                assert!(mid[0] > 3.0 - 1e-9, "{mid:?}");
            }
            if (mid[0] - 3.0).abs() < 1e-9 {
                assert!(mid[1] < -2.0, "{mid:?}");
            }
        }
        // The seat's wall, its floor, the bore's wall and floor all remain.
        let has = |f: &dyn Fn([Real; 2]) -> bool| {
            pieces
                .iter()
                .any(|[a, b]| f([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]))
        };
        assert!(has(&|m| (m[0] - 6.0).abs() < 1e-9));
        assert!(has(&|m| (m[1] + 2.0).abs() < 1e-9));
        assert!(has(&|m| (m[0] - 3.0).abs() < 1e-9));
        assert!(has(&|m| (m[1] + 10.0).abs() < 1e-9));
    }

    #[test]
    fn a_through_hole_stays_open_at_the_far_face() {
        let pieces = hole_outline(&counterbore(), Some(-10.0));
        assert!(
            pieces
                .iter()
                .all(|[a, b]| (a[1] + b[1]) / 2.0 > -10.0 + 1e-9)
        );
    }

    #[test]
    fn a_point_on_a_shared_wall_is_not_inside() {
        let t = counterbore();
        assert!(in_tool(&t[0], [1.0, -1.0]));
        assert!(!in_tool(&t[0], [6.0, -1.0]));
        assert!(!in_tool(&t[0], [7.0, -1.0]));
    }
}
