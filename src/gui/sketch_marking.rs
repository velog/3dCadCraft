//! The sketch's constraint menu as a ring (Ctrl + right click, when chosen in
//! Application Options › Sketch; the default is the column of `select.rs`):
//! every constraint within one short move of the mouse, in the places of
//! Inventor's marking menu; the constraints that menu has no place for sit in
//! a row below it.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Vec2};

use super::commands;
use super::icons::{self, Icon, Palette};

/// (command id, label, icon, direction in eighths of a turn from north,
/// clockwise).
const AROUND: [(&str, &str, Icon, u8); 8] = [
    (
        "sketch.perpendicular",
        "Perpendicular",
        Icon::Perpendicular,
        0,
    ),
    ("sketch.vertical", "Vertical", Icon::Vertical, 1),
    ("sketch.coincident", "Coincident", Icon::Coincident, 2),
    ("sketch.horizontal", "Horizontal", Icon::Horizontal, 3),
    ("sketch.parallel", "Parallel", Icon::Parallel, 4),
    ("sketch.concentric", "Concentric", Icon::Concentric, 5),
    ("sketch.tangent", "Tangent", Icon::Tangent, 6),
    ("sketch.equal", "Equal", Icon::Equal, 7),
];

/// The other constraints, in a row under the menu.
const BELOW: [(&str, &str, Icon); 3] = [
    ("sketch.collinear", "Collinear", Icon::Collinear),
    ("sketch.fix", "Fix", Icon::Fix),
    ("sketch.symmetric", "Symmetric", Icon::Symmetric),
];

/// One entry's size.
const ITEM: Vec2 = Vec2::new(104.0, 24.0);
/// Distance of the entries from the cursor.
const RADIUS: Vec2 = Vec2::new(112.0, 84.0);

/// The entry's centre for `eighth` (see [`AROUND`]), around the origin.
fn place(eighth: u8) -> Vec2 {
    let a = f32::from(eighth) * std::f32::consts::FRAC_PI_4;
    Vec2::new(a.sin() * RADIUS.x, -a.cos() * RADIUS.y)
}

impl super::CadCraftApp {
    /// Draws the open menu; closes it on a choice, Esc or a click elsewhere.
    pub(super) fn constraint_menu_ui(&mut self, ctx: &egui::Context) {
        let Some(at) = self.constraint_menu else {
            return;
        };
        let half = RADIUS + ITEM * 0.5 + Vec2::splat(4.0);
        let below = ITEM.y + 6.0;
        let size = Vec2::new(half.x * 2.0, half.y * 2.0 + below);
        let mut chosen = None;
        let area = egui::Area::new(egui::Id::new("sketch_constraint_menu"))
            .fixed_pos(at - half)
            .order(egui::Order::Foreground)
            .constrain(true)
            .show(ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                let middle = rect.min + half;
                ui.painter().circle_filled(
                    middle,
                    4.0,
                    Color32::from_rgba_unmultiplied(120, 150, 200, 200),
                );
                let mut item = |ui: &mut egui::Ui, c: Pos2, id: &'static str, label, icon| {
                    if entry(ui, Rect::from_center_size(c, ITEM), id, label, icon) {
                        chosen = Some(id);
                    }
                };
                for (id, label, icon, eighth) in AROUND {
                    item(ui, middle + place(eighth), id, label, icon);
                }
                let y = rect.max.y - ITEM.y * 0.5;
                for (k, (id, label, icon)) in BELOW.into_iter().enumerate() {
                    let x = middle.x + (k as f32 - 1.0) * (ITEM.x + 4.0);
                    item(ui, Pos2::new(x, y), id, label, icon);
                }
            });
        let clicked_out = ctx.input(|i| i.pointer.any_pressed())
            && !self.constraint_menu_fresh
            && ctx
                .pointer_interact_pos()
                .is_some_and(|p| !area.response.rect.contains(p));
        self.constraint_menu_fresh = false;
        if let Some(id) = chosen {
            self.constraint_menu = None;
            if let Some(c) = commands::by_id(id) {
                self.start_command(c);
            }
        } else if clicked_out {
            self.constraint_menu = None;
        }
    }
}

/// One entry: an icon and its name on a menu plate; true when clicked.
fn entry(ui: &mut egui::Ui, rect: Rect, id: &'static str, label: &str, icon: Icon) -> bool {
    let r = ui.interact(rect, egui::Id::new(("constraint_menu", id)), Sense::click());
    let visuals = ui.style().visuals.clone();
    let fill = if r.hovered() {
        visuals.widgets.hovered.bg_fill
    } else {
        visuals.window_fill
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        3.0,
        fill,
        visuals.window_stroke,
        egui::StrokeKind::Inside,
    );
    let icon_rect = Rect::from_min_size(rect.min + Vec2::new(4.0, 4.0), Vec2::splat(16.0));
    icons::paint(painter, icon_rect, icon, &Palette::ENABLED);
    painter.text(
        rect.left_center() + Vec2::new(24.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        visuals.text_color(),
    );
    r.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every entry names a real command, and the menu offers all of them.
    #[test]
    fn every_constraint_is_on_the_menu() {
        let ids: Vec<&str> = AROUND
            .iter()
            .map(|a| a.0)
            .chain(BELOW.iter().map(|b| b.0))
            .collect();
        for id in &ids {
            assert!(commands::by_id(id).is_some(), "{id}");
        }
        assert_eq!(
            ids.len(),
            super::super::sketch_tools::ConstraintTool::ALL.len()
        );
    }

    #[test]
    fn north_is_up_and_east_is_right() {
        assert!(place(0).y < 0.0 && place(0).x.abs() < 1e-3);
        assert!(place(2).x > 0.0 && place(2).y.abs() < 1e-3);
        assert!(place(4).y > 0.0);
    }
}
