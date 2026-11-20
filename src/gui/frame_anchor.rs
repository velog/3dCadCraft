//! The placement points of Insert Frame and Change (spec 6.1 §3.14): under
//! the Anchor list the chosen section is drawn with its nine placement
//! points and its centroid as buttons, like Inventor's profile picture in the
//! Insert panel; a click takes that point as the member's anchor (the same
//! value as the Anchor list and the points in the view).

use super::asm_planned::AsmPlanned;
use super::frame_list::ANCHOR_NAMES;
use super::part_planned::{PartDialog, Planned};
use super::theme;
use crate::assembly::frame::Anchor;
use crate::part::frame::Library;
use eframe::egui::{self, Pos2, Rect, Stroke, Vec2};

/// Size of the picture, px.
const SIZE: Vec2 = Vec2::new(150.0, 132.0);
/// Radius of a point's button, px.
const DOT: f32 = 5.0;

/// Does the panel get the picture under its Anchor list?
pub(super) fn shows(d: &PartDialog) -> bool {
    matches!(
        d.kind,
        Planned::Asm(AsmPlanned::InsertFrame | AsmPlanned::FrameChange)
    )
}

/// The picture (one grid row: the caller draws the label cell).
pub(super) fn anchor_picker(ui: &mut egui::Ui, d: &mut PartDialog) {
    let Some(o) = super::frame::insert_options(d) else {
        return;
    };
    let lib = Library::current();
    let Some(sec) = lib.find(&o.family, &o.size) else {
        return;
    };
    let current = d.index(0, "Anchor");
    let (rect, _) = ui.allocate_exact_size(SIZE, egui::Sense::hover());
    let painter = ui.painter_at(rect.expand(2.0));
    let t = theme::tokens();
    painter.rect_filled(rect, 4.0, t.field);
    let (w, h) = sec.extent();
    // The section above, the point's name in a row of its own below it.
    let area = Rect::from_min_max(
        rect.min + Vec2::new(14.0, 10.0),
        rect.max - Vec2::new(14.0, 26.0),
    );
    let scale = (area.width() as f64 / w.max(1e-6)).min(area.height() as f64 / h.max(1e-6));
    let c = area.center();
    let to = |x: f64, y: f64| Pos2::new(c.x + (x * scale) as f32, c.y - (y * scale) as f32);
    // The bounding box the points stand on, then the outline.
    let bx = Rect::from_two_pos(to(-w / 2.0, -h / 2.0), to(w / 2.0, h / 2.0));
    painter.rect_stroke(
        bx,
        0.0,
        Stroke::new(1.0, t.text_weak.gamma_multiply(0.5)),
        egui::StrokeKind::Middle,
    );
    for l in sec.loops() {
        let pts: Vec<Pos2> = l.points(48).iter().map(|p| to(p.x, p.y)).collect();
        if pts.len() > 1 {
            painter.add(egui::Shape::closed_line(pts, Stroke::new(1.5, t.text)));
        }
    }
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let mut hovered = None;
    for (k, a) in Anchor::ALL.iter().enumerate() {
        let (x, y) = a.point(sec);
        let q = to(x, y);
        let id = ui.id().with(("frame_anchor_dot", k));
        let r = ui.interact(
            Rect::from_center_size(q, Vec2::splat(2.0 * DOT + 4.0)),
            id,
            egui::Sense::click(),
        );
        let hot = r.hovered() || pointer.is_some_and(|p| p.distance(q) <= DOT + 2.0);
        if hot {
            hovered = Some(k);
        }
        let name = ANCHOR_NAMES.get(k).copied().unwrap_or("");
        let r = r.on_hover_text(name);
        if r.clicked() {
            d.set_index(0, "Anchor", k);
        }
        let chosen = d.index(0, "Anchor") == k;
        let fill = if chosen { t.accent } else { t.panel };
        let ring = if hot { t.text_strong } else { t.accent };
        if *a == Anchor::Centroid {
            // A diamond: the centroid is not one of the box's nine points.
            let s = DOT + 0.5;
            let pts = vec![
                q + Vec2::new(0.0, -s),
                q + Vec2::new(s, 0.0),
                q + Vec2::new(0.0, s),
                q + Vec2::new(-s, 0.0),
            ];
            painter.add(egui::Shape::convex_polygon(
                pts,
                fill,
                Stroke::new(1.5, ring),
            ));
        } else {
            painter.circle(q, DOT, fill, Stroke::new(1.5, ring));
        }
    }
    let shown = hovered.unwrap_or(current);
    painter.text(
        rect.left_bottom() + Vec2::new(4.0, -2.0),
        egui::Align2::LEFT_BOTTOM,
        ANCHOR_NAMES.get(shown).copied().unwrap_or(""),
        egui::FontId::proportional(11.0),
        if hovered.is_some() {
            t.text_strong
        } else {
            t.text_weak
        },
    );
}
