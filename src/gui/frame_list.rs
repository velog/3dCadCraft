//! Insert Frame's list of picked lines and the overlap checks
//! (`docs/spec/6.1-frame-generator.md` §3.1a, owner's batch 2026-11-12):
//! every picked skeleton line is a row with its icon and text (rule 23), a
//! row under the cursor lights its line in the view, a chosen row is drawn in
//! its own colour and gets a placement of its own (anchor, offsets, rotation,
//! mirror, flip) below the list; a row can be picked again or removed. The
//! lines that lie on one another (Remove Overlapping Members) are found here
//! for the panel and for the browser's warning on placed members.

use super::CadCraftApp;
use super::icons::Icon;
use super::part_planned::PartDialog;
use super::theme::{self, Glyph};
use super::valuefield::ExprValue;
use crate::assembly::frame::{self, Anchor, Orientation, Skeleton};
use crate::kernel::{Point3, Real};
use eframe::egui::{self, Color32, Rect, RichText, Stroke};
use std::cell::RefCell;
use std::rc::Rc;

/// The Anchor list's names, in `Anchor::ALL` order.
pub(super) const ANCHOR_NAMES: [&str; 10] = [
    "Center",
    "Centroid",
    "Top Left",
    "Top",
    "Top Right",
    "Left",
    "Right",
    "Bottom Left",
    "Bottom",
    "Bottom Right",
];

/// The check box of Insert Frame's Advanced Settings.
pub(super) const REMOVE_OVERLAPS: &str = "Remove Overlapping Members";

/// Colour of the chosen row's line in the view.
const CURRENT: Color32 = Color32::from_rgb(60, 140, 255);
/// Colour of a row's line under the cursor in the list.
const HOVER: Color32 = Color32::from_rgb(255, 225, 0);
/// Colour of a line Remove Overlapping Members leaves out.
const DROPPED: Color32 = Color32::from_rgb(200, 60, 60);

/// The row's icon: what the line is.
fn icon_of(s: &Skeleton) -> Icon {
    match s {
        Skeleton::Edge(_) => Icon::ProjectGeometry,
        Skeleton::SketchLine { .. } => Icon::Line,
        Skeleton::Between { .. } => Icon::Point,
        Skeleton::Chain(_) => Icon::Spline,
    }
}

/// What the overlap check says about each picked line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RowNotes {
    /// A warning per row (`None`: nothing lies on it).
    pub warn: Vec<Option<String>>,
    /// Rows OK leaves out (Remove Overlapping Members on).
    pub dropped: Vec<bool>,
    /// Placed members OK removes (a longer new line covers them).
    pub removes: Vec<u32>,
    /// Each straight row's length (mm), shown in its text.
    pub len: Vec<Option<Real>>,
}

/// The list in the panel, under the Edges / Lines selector (inside its grid:
/// one full row).
pub(super) fn skeleton_list_ui(ui: &mut egui::Ui, d: &mut PartDialog) {
    let n = d.skeletons.len();
    if n == 0 {
        return;
    }
    d.skel.own.resize(n, None);
    let labels = d.picks.get("edges").cloned().unwrap_or_default();
    let panel = super::frame::orientation(d);
    let mut drop = None;
    let mut hover = None;
    super::panels::plabel(ui, "");
    ui.vertical(|ui| {
        for i in 0..n {
            ui.horizontal(|ui| {
                super::panels::tree_icon(ui, icon_of(&d.skeletons[i]));
                let name = labels
                    .get(i)
                    .map_or("line", |w| w.split(" · ").next().unwrap_or(w));
                let mut text = match d.skel.notes.len.get(i).copied().flatten() {
                    Some(l) => format!("{}. {l:.0} mm · {name}", i + 1),
                    None => format!("{}. {name}", i + 1),
                };
                if d.skel.own[i].is_some() {
                    text.push_str(" · own placement");
                }
                let current = d.skel.current == Some(i);
                let dropped = d.skel.notes.dropped.get(i).copied().unwrap_or(false);
                let mut rich = RichText::new(text);
                if current {
                    rich = rich.strong().color(CURRENT);
                }
                if dropped {
                    rich = rich.strikethrough();
                }
                // The buttons at the right edge, the text truncated before
                // them (the panel cannot widen).
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::glyph(ui, Glyph::Close)
                        .on_hover_text("Remove this line")
                        .clicked()
                    {
                        drop = Some(i);
                    }
                    if theme::glyph(ui, Glyph::Edit)
                        .on_hover_text(
                            "Re-pick: the next line picked in the view replaces this one",
                        )
                        .clicked()
                    {
                        d.skel.current = Some(i);
                        d.skel.repick = true;
                    }
                    if let Some(w) = d.skel.notes.warn.get(i).cloned().flatten() {
                        ui.colored_label(theme::tokens().warning, "!")
                            .on_hover_text(w);
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let r = ui.add(
                            egui::Label::new(rich)
                                .truncate()
                                .sense(egui::Sense::click()),
                        );
                        if r.clicked() {
                            d.skel.current = if current { None } else { Some(i) };
                            d.skel.repick = false;
                        }
                        if r.hovered() {
                            hover = Some(i);
                        }
                    });
                });
            });
        }
        if d.skel.repick {
            ui.label(
                RichText::new("Re-pick: click the new line in the view")
                    .italics()
                    .color(CURRENT),
            );
        }
        if let Some(i) = d.skel.current.filter(|&i| i < n) {
            own_editor(ui, d, i, panel);
        }
    });
    ui.end_row();
    d.skel.hover = hover;
    if let Some(i) = drop {
        d.drop_skeleton(i);
    }
}

/// The chosen row's own placement.
fn own_editor(ui: &mut egui::Ui, d: &mut PartDialog, i: usize, panel: Orientation) {
    ui.separator();
    let mut on = d.skel.own[i].is_some();
    if ui
        .checkbox(&mut on, format!("Own placement of line {}", i + 1))
        .on_hover_text("Off: this member takes the panel's Orientation (and Position / Level)")
        .changed()
    {
        d.skel.own[i] = on.then_some(panel);
    }
    let Some(o) = d.skel.own[i].as_mut() else {
        return;
    };
    egui::Grid::new(("frame_own", i))
        .num_columns(2)
        .min_col_width(96.0)
        .spacing([6.0, 4.0])
        .show(ui, |ui| {
            ui.label("Anchor");
            let mut k = Anchor::ALL.iter().position(|a| *a == o.anchor).unwrap_or(0);
            egui::ComboBox::from_id_salt(("frame_own_anchor", i))
                .selected_text(ANCHOR_NAMES[k])
                .show_ui(ui, |ui| {
                    for (j, n) in ANCHOR_NAMES.iter().enumerate() {
                        ui.selectable_value(&mut k, j, *n);
                    }
                });
            o.anchor = Anchor::ALL.get(k).copied().unwrap_or_default();
            ui.end_row();
            ui.label("Horizontal Offset");
            ui.add(ExprValue::new(&mut o.h_offset).suffix(" mm"));
            ui.end_row();
            ui.label("Vertical Offset");
            ui.add(ExprValue::new(&mut o.v_offset).suffix(" mm"));
            ui.end_row();
            ui.label("Rotation Angle");
            let was: Real = o.angle.to_degrees();
            let mut deg = was;
            ui.add(ExprValue::new(&mut deg).suffix("°"));
            if deg != was {
                o.angle = deg.to_radians();
            }
            ui.end_row();
        });
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut o.mirror, "Mirror");
        ui.checkbox(&mut o.flip, "Flip Direction");
        ui.checkbox(&mut o.about_anchor, "Rotate Around Selected Point");
    });
    let mine = *o;
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("Rotate 90°")
            .on_hover_text("Turns the section a quarter turn about the line")
            .clicked()
            && let Some(o) = d.skel.own[i].as_mut()
        {
            o.angle = (o.angle + std::f64::consts::FRAC_PI_2) % std::f64::consts::TAU;
        }
        if ui
            .button("Apply to All")
            .on_hover_text("Every picked line takes this placement")
            .clicked()
        {
            for x in d.skel.own.iter_mut() {
                *x = Some(mine);
            }
        }
    });
}

/// Placed members' lines of the frame worked on (active assembly
/// coordinates) and the pairs of them lying on one another, kept until a
/// member moves (rule 20: never per frame).
type MemberLines = Rc<(Vec<(u32, (Point3, Point3))>, Vec<(u32, u32)>)>;

thread_local! {
    static MEMBER_LINES: RefCell<Option<(u64, MemberLines)>> = const { RefCell::new(None) };
}

impl CadCraftApp {
    /// What lies where among the placed members (see [`MemberLines`]).
    fn member_lines(&self) -> MemberLines {
        use std::hash::{Hash, Hasher};
        let Some(a) = self.frame_asm() else {
            return Rc::default();
        };
        let place = self.frame_place();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.frame_doc().hash(&mut h);
        let bits = |h: &mut std::collections::hash_map::DefaultHasher,
                    t: &crate::kernel::Transform3| {
            for v in t.translation.vector.iter().chain(t.rotation.coords.iter()) {
                v.to_bits().hash(h);
            }
        };
        bits(&mut h, &place);
        for o in &a.occurrences {
            o.id.hash(&mut h);
            o.component.variant.hash(&mut h);
            bits(&mut h, &o.transform);
        }
        for f in &a.frames {
            f.members.len().hash(&mut h);
        }
        let key = h.finish();
        if let Some(v) = MEMBER_LINES.with(|c| {
            c.borrow()
                .as_ref()
                .filter(|(k, _)| *k == key)
                .map(|(_, v)| v.clone())
        }) {
            return v;
        }
        let linked = self.frame_linked();
        let lines: Vec<(u32, (Point3, Point3))> = a
            .frames
            .iter()
            .flat_map(|f| &f.members)
            .filter_map(|m| {
                frame::skeleton_line(&self.project, &linked, a, &m.skeleton)
                    .ok()
                    .map(|(p, q)| (m.occurrence, (place * p, place * q)))
            })
            .collect();
        let pairs = frame::overlapping_pairs(&lines);
        let v = Rc::new((lines, pairs));
        MEMBER_LINES.with(|c| *c.borrow_mut() = Some((key, v.clone())));
        v
    }

    /// Placed members' lines (frame document ids, active assembly
    /// coordinates).
    pub(super) fn frame_member_lines(&self) -> Vec<(u32, (Point3, Point3))> {
        self.member_lines().0.clone()
    }

    /// Placed members lying on another member (frame document ids): the
    /// browser marks them.
    pub(super) fn frame_overlaps(&self) -> Vec<(u32, u32)> {
        self.member_lines().1.clone()
    }

    /// The overlap notes of the panel's picked lines (against each other and
    /// the placed members); with Remove Overlapping Members on, also what OK
    /// leaves out and removes.
    pub(super) fn frame_list_notes(&self, d: &PartDialog) -> RowNotes {
        let n = d.skeletons.len();
        if n == 0 {
            return RowNotes::default();
        }
        let Some(a) = self.assembly() else {
            return RowNotes::default();
        };
        let linked = self.frame_linked();
        let mine: Vec<Option<(Point3, Point3)>> = d
            .skeletons
            .iter()
            .map(|s| frame::skeleton_line(&self.project, &linked, a, s).ok())
            .collect();
        let placed = self.member_lines();
        let name = |occ: u32| {
            self.frame_asm()
                .and_then(|f| f.find(occ))
                .map_or_else(|| format!("member {occ}"), |o| o.name.clone())
        };
        let mut warn = vec![None; n];
        for i in 0..n {
            let Some(l) = mine[i] else {
                continue;
            };
            let mut on: Vec<String> = (0..n)
                .filter(|&j| j != i && mine[j].is_some_and(|m| frame::lines_overlap(m, l)))
                .map(|j| format!("line {}", j + 1))
                .collect();
            on.extend(
                placed
                    .0
                    .iter()
                    .filter(|(_, m)| frame::lines_overlap(*m, l))
                    .map(|(o, _)| name(*o)),
            );
            if !on.is_empty() {
                warn[i] = Some(format!("Lies on {}", on.join(", ")));
            }
        }
        let mut notes = RowNotes {
            warn,
            dropped: vec![false; n],
            removes: Vec::new(),
            len: mine
                .iter()
                .map(|l| l.map(|(p, q)| (q - p).norm()))
                .collect(),
        };
        if d.check(0, REMOVE_OVERLAPS) {
            let fixed = placed.0.len();
            let all: Vec<Option<(Point3, Point3)>> = placed
                .0
                .iter()
                .map(|(_, l)| Some(*l))
                .chain(mine.iter().copied())
                .collect();
            for k in frame::overlap_drops(&all, fixed) {
                if k < fixed {
                    notes.removes.push(placed.0[k].0);
                } else {
                    notes.dropped[k - fixed] = true;
                }
            }
        }
        notes
    }

    /// The list's marks in the view: each picked line's number at its middle,
    /// the row under the cursor and the chosen row lit, the lines OK leaves
    /// out crossed.
    pub(super) fn frame_list_overlay(&self, d: &PartDialog, painter: &egui::Painter, rect: Rect) {
        let Some(a) = self.assembly() else {
            return;
        };
        let linked = self.frame_linked();
        let font = egui::FontId::proportional(12.0);
        for (i, s) in d.skeletons.iter().enumerate() {
            let Ok((p, q)) = frame::skeleton_line(&self.project, &linked, a, s) else {
                continue;
            };
            let (Some(sp), Some(sq)) =
                (self.camera.project(&p, rect), self.camera.project(&q, rect))
            else {
                continue;
            };
            let dropped = d.skel.notes.dropped.get(i).copied().unwrap_or(false);
            let col = if d.skel.current == Some(i) {
                Some(CURRENT)
            } else if d.skel.hover == Some(i) {
                Some(HOVER)
            } else if dropped {
                Some(DROPPED)
            } else {
                None
            };
            if let Some(c) = col {
                painter.line_segment([sp, sq], Stroke::new(5.0, c));
            }
            let mid = sp + (sq - sp) / 2.0;
            if dropped {
                let r = 6.0;
                let st = Stroke::new(2.0, DROPPED);
                painter.line_segment([mid + egui::vec2(-r, -r), mid + egui::vec2(r, r)], st);
                painter.line_segment([mid + egui::vec2(-r, r), mid + egui::vec2(r, -r)], st);
            }
            let text = format!("{}", i + 1);
            let at = mid + egui::vec2(8.0, -10.0);
            let g = painter.layout_no_wrap(text, font.clone(), Color32::WHITE);
            let bg = Rect::from_min_size(at, g.size()).expand(2.0);
            painter.rect_filled(bg, 3.0, col.unwrap_or(Color32::from_rgb(40, 110, 60)));
            painter.galley(at, g, Color32::WHITE);
        }
    }
}
