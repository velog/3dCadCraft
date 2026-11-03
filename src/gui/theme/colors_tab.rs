//! Application Options › Colors (spec 0.18). UI Theme works; the options whose
//! logic is not there yet are shown disabled, with their spec row (rule 15).

use super::{Tokens, UiTheme, current, set_current};
use eframe::egui::{self, Color32, CornerRadius, Rect, RichText, Sense, Stroke, Vec2};

/// The tooltip of an option without logic.
const LATER: &str = "Spec 0.18 §3: the logic comes later";

/// A check box that cannot change yet.
fn later_check(ui: &mut egui::Ui, on: bool, text: &str) {
    let mut value = on;
    ui.add_enabled(false, egui::Checkbox::new(&mut value, text))
        .on_disabled_hover_text(LATER);
}

/// A radio button that cannot change yet.
fn later_radio(ui: &mut egui::Ui, on: bool, text: &str) {
    ui.add_enabled(false, egui::RadioButton::new(on, text))
        .on_disabled_hover_text(LATER);
}

/// The frame of a theme in small: title bar, ribbon, browser, view, status bar.
fn preview(ui: &mut egui::Ui, k: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(300.0, 170.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(4), k.panel);
    let band = |top: f32, h: f32| {
        Rect::from_min_size(rect.min + Vec2::new(0.0, top), Vec2::new(rect.width(), h))
    };
    p.hline(rect.x_range(), rect.top() + 34.0, Stroke::new(1.0, k.line));
    let tab = Rect::from_min_size(rect.min + Vec2::new(8.0, 8.0), Vec2::new(44.0, 16.0));
    p.rect_filled(tab, CornerRadius::same(3), k.accent);
    for (i, w) in [36.0_f32, 30.0, 40.0].iter().enumerate() {
        let x = 62.0 + i as f32 * 46.0;
        let r = Rect::from_min_size(rect.min + Vec2::new(x, 13.0), Vec2::new(*w, 5.0));
        p.rect_filled(r, CornerRadius::same(1), k.text_weak);
    }
    let body = band(35.0, rect.height() - 53.0);
    let browser = Rect::from_min_max(body.min, egui::pos2(body.left() + 62.0, body.bottom()));
    for i in 0..4 {
        let r = Rect::from_min_size(
            browser.min + Vec2::new(10.0, 10.0 + i as f32 * 14.0),
            Vec2::new(40.0, 5.0),
        );
        p.rect_filled(r, CornerRadius::same(1), k.text_weak);
    }
    let view = Rect::from_min_max(egui::pos2(browser.right(), body.top()), body.max);
    // The view's colours belong to the In-canvas Color Scheme, not to the theme.
    p.rect_filled(
        view,
        CornerRadius::ZERO,
        Color32::from_rgb(0xe3, 0xe7, 0xec),
    );
    let part = Rect::from_center_size(view.center(), Vec2::new(90.0, 46.0));
    p.rect_filled(
        part,
        CornerRadius::same(3),
        Color32::from_rgb(0x8f, 0xa6, 0xc4),
    );
    p.hline(
        rect.x_range(),
        rect.bottom() - 18.0,
        Stroke::new(1.0, k.line),
    );
    let status = Rect::from_min_size(
        rect.left_bottom() + Vec2::new(8.0, -12.0),
        Vec2::new(120.0, 5.0),
    );
    p.rect_filled(status, CornerRadius::same(1), k.text_weak);
}

/// The tab's content.
pub fn colors_tab(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label("Show:");
        later_radio(ui, true, "Design");
        later_radio(ui, false, "Drafting");
    });
    let mut theme = current();
    preview(ui, &theme.tokens());
    ui.add_space(6.0);
    ui.label(RichText::new("UI Theme").strong());
    ui.horizontal(|ui| {
        for t in UiTheme::ALL {
            ui.radio_value(&mut theme, t, t.label()).on_hover_text(
                "The colours of the frame around the view: ribbon, browser, panels, dialogs, icons",
            );
        }
    });
    if theme != current() {
        set_current(theme);
    }
    ui.add_space(6.0);
    ui.label(RichText::new("Highlight").strong());
    later_check(ui, true, "Enable Prehighlight");
    later_check(ui, false, "Enable Enhanced Highlighting");
    later_check(ui, false, "Highlight Hidden Faces");
    ui.add_space(6.0);
    ui.label(RichText::new("In-canvas Color Scheme").strong());
    ui.horizontal(|ui| {
        ui.add_enabled_ui(false, |ui| {
            egui::ComboBox::from_id_salt("canvas_scheme")
                .selected_text("3DCadCraft Light")
                .show_ui(ui, |_| {});
        })
        .response
        .on_disabled_hover_text(LATER);
        ui.add_enabled(false, egui::Button::new("Customize Schemes..."))
            .on_disabled_hover_text(LATER);
    });
    ui.add_space(6.0);
    ui.label(RichText::new("Background").strong());
    ui.horizontal(|ui| {
        later_radio(ui, true, "1 Color");
        later_radio(ui, false, "Gradient");
        later_radio(ui, false, "Background Image");
    });
    ui.add_space(6.0);
    ui.label(RichText::new("Section Capping Plane Texture").strong());
    ui.horizontal(|ui| {
        later_radio(ui, true, "Default - Gray");
        later_radio(ui, false, "Bitmap Image");
    });
    ui.add_space(6.0);
    ui.label(RichText::new("Preview Image Background").strong());
    later_check(ui, false, "Use a fixed background when saving previews");
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_enabled(false, egui::Button::new("Export..."))
            .on_disabled_hover_text(LATER);
        ui.add_enabled(false, egui::Button::new("Import..."))
            .on_disabled_hover_text(LATER);
    });
}
