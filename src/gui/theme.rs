//! The interface's look in one place (spec 0-ui §2.1a): the palette, the
//! fonts and egui's style, drawn buttons for the small symbols (close, add,
//! arrows) and the emphasised button kinds.
//!
//! Colours of geometry in the view (sketch states, previews, selection) keep
//! their meanings from spec 0-ui §2.2 and are not defined here.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Pos2, Rect,
    Response, RichText, Sense, Shadow, Stroke, TextStyle, Theme, Ui, Vec2, Widget, WidgetInfo,
    WidgetType,
};
use std::sync::Arc;

/// Panels, ribbon, browser, status bar.
pub const PANEL: Color32 = Color32::from_rgb(0x22, 0x24, 0x29);
/// Floating windows and menus: one step above the panels.
pub const RAISED: Color32 = Color32::from_rgb(0x2a, 0x2d, 0x33);
/// Inside of text fields.
pub const FIELD: Color32 = Color32::from_rgb(0x16, 0x18, 0x1b);
/// A button at rest.
pub const CONTROL: Color32 = Color32::from_rgb(0x36, 0x3a, 0x41);
/// A button under the cursor.
pub const CONTROL_HOVER: Color32 = Color32::from_rgb(0x43, 0x48, 0x51);
/// A pressed button.
pub const CONTROL_ACTIVE: Color32 = Color32::from_rgb(0x4f, 0x55, 0x60);
/// Thin separators and outlines.
pub const LINE: Color32 = Color32::from_rgb(0x3a, 0x3e, 0x46);
pub const TEXT: Color32 = Color32::from_rgb(0xe6, 0xe8, 0xec);
/// Labels: quieter than values, still clearly readable.
pub const TEXT_WEAK: Color32 = Color32::from_rgb(0xac, 0xb1, 0xbb);
/// The one accent: the current choice and the main action of a panel.
pub const ACCENT: Color32 = Color32::from_rgb(0x2f, 0x74, 0xc8);
/// Text of actions that delete something.
pub const DANGER: Color32 = Color32::from_rgb(0xf0, 0x8a, 0x80);

/// Family of text drawn on a drawing sheet: the PDF embeds this same font and
/// Format Text measures with it, so it does not follow the interface's font.
pub const SHEET_FAMILY: &str = "sheet";
/// The font of the sheet (one of egui's built-in ones).
pub const SHEET_FONT: &str = "Ubuntu-Light";
const UI_FONT: &str = "Inter";

/// A font of the drawing sheet's family.
pub fn sheet_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SHEET_FAMILY.into()))
}

/// The fonts: Inter for the interface, the built-in fonts as a fallback for
/// symbols, and a family of its own for the sheet.
fn fonts() -> FontDefinitions {
    let mut f = FontDefinitions::default();
    f.font_data.insert(
        UI_FONT.to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../data/fonts/Inter-Regular.ttf"
        ))),
    );
    let fallback = f
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut sheet = fallback.clone();
    sheet.retain(|n| n != SHEET_FONT);
    sheet.insert(0, SHEET_FONT.to_owned());
    f.families
        .insert(FontFamily::Name(SHEET_FAMILY.into()), sheet);
    let mut ui = fallback;
    ui.insert(0, UI_FONT.to_owned());
    f.families.insert(FontFamily::Proportional, ui);
    f
}

/// Sets the fonts and the dark style; called once at start-up.
pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.style_mut_of(Theme::Dark, style);
}

fn style(s: &mut egui::Style) {
    use FontFamily::{Monospace, Proportional};
    s.text_styles = [
        (TextStyle::Small, FontId::new(10.5, Proportional)),
        (TextStyle::Body, FontId::new(12.5, Proportional)),
        (TextStyle::Button, FontId::new(12.5, Proportional)),
        (TextStyle::Heading, FontId::new(16.0, Proportional)),
        (TextStyle::Monospace, FontId::new(12.0, Monospace)),
    ]
    .into();
    s.spacing.button_padding = Vec2::new(6.0, 2.0);
    s.spacing.menu_margin = Margin::same(5);
    let v = &mut s.visuals;
    v.dark_mode = true;
    v.panel_fill = PANEL;
    v.window_fill = RAISED;
    v.faint_bg_color = RAISED;
    v.extreme_bg_color = FIELD;
    v.code_bg_color = FIELD;
    v.window_stroke = Stroke::new(1.0, LINE);
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(6);
    let shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(110),
    };
    v.window_shadow = shadow;
    v.popup_shadow = shadow;
    v.weak_text_color = Some(TEXT_WEAK);
    v.hyperlink_color = Color32::from_rgb(0x6c, 0xb0, 0xf5);
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    let radius = CornerRadius::same(4);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.weak_bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    w.noninteractive.corner_radius = radius;
    for (k, fill, fg) in [
        (&mut w.inactive, CONTROL, TEXT),
        (&mut w.hovered, CONTROL_HOVER, Color32::WHITE),
        (&mut w.active, CONTROL_ACTIVE, Color32::WHITE),
        (&mut w.open, CONTROL_HOVER, TEXT),
    ] {
        k.bg_fill = fill;
        k.weak_bg_fill = fill;
        k.bg_stroke = Stroke::NONE;
        k.fg_stroke = Stroke::new(1.0, fg);
        k.corner_radius = radius;
        // No growing under the cursor: the neighbours stay quiet.
        k.expansion = 0.0;
    }
}

/// The main action of a panel or dialog (OK).
pub fn ok_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_owned()).color(Color32::WHITE)).fill(ACCENT)
}

/// An action that deletes something.
pub fn danger_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_owned()).color(DANGER))
}

/// The symbols of small buttons, drawn so that they do not depend on a font.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Close,
    Plus,
    Minus,
    Left,
    Right,
    Up,
    Down,
    Check,
    Help,
}

impl Glyph {
    fn name(self) -> &'static str {
        match self {
            Glyph::Close => "Close",
            Glyph::Plus => "Add",
            Glyph::Minus => "Remove",
            Glyph::Left => "Previous",
            Glyph::Right => "Next",
            Glyph::Up => "Up",
            Glyph::Down => "Down",
            Glyph::Check => "OK",
            Glyph::Help => "Help",
        }
    }

    /// The strokes, in a square of -1..1 about the centre.
    fn strokes(self) -> &'static [&'static [(f32, f32)]] {
        match self {
            Glyph::Close => &[&[(-0.7, -0.7), (0.7, 0.7)], &[(-0.7, 0.7), (0.7, -0.7)]],
            Glyph::Plus => &[&[(-0.8, 0.0), (0.8, 0.0)], &[(0.0, -0.8), (0.0, 0.8)]],
            Glyph::Minus => &[&[(-0.8, 0.0), (0.8, 0.0)]],
            Glyph::Left => &[&[(0.35, -0.8), (-0.45, 0.0), (0.35, 0.8)]],
            Glyph::Right => &[&[(-0.35, -0.8), (0.45, 0.0), (-0.35, 0.8)]],
            Glyph::Up => &[&[(-0.8, 0.35), (0.0, -0.45), (0.8, 0.35)]],
            Glyph::Down => &[&[(-0.8, -0.35), (0.0, 0.45), (0.8, -0.35)]],
            Glyph::Check => &[&[(-0.8, 0.05), (-0.25, 0.6), (0.8, -0.6)]],
            // The curl and the dot of a question mark.
            Glyph::Help => &[
                &[
                    (-0.5, -0.45),
                    (-0.3, -0.8),
                    (0.1, -0.9),
                    (0.5, -0.65),
                    (0.5, -0.25),
                    (0.0, 0.1),
                    (0.0, 0.4),
                ],
                &[(0.0, 0.75), (0.0, 0.85)],
            ],
        }
    }
}

/// A small square button with a drawn symbol; `add_enabled` greys it.
pub struct GlyphButton(pub Glyph);

impl Widget for GlyphButton {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, r) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
        let name = self.0.name();
        r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), name));
        if ui.is_rect_visible(rect) {
            let v = ui.style().interact(&r);
            if r.hovered() || r.has_focus() || r.is_pointer_button_down_on() {
                ui.painter()
                    .rect_filled(rect, v.corner_radius, v.weak_bg_fill);
            }
            paint_glyph(ui.painter(), rect.shrink(5.0), self.0, v.fg_stroke.color);
        }
        r
    }
}

/// Draws a symbol in `rect`.
pub fn paint_glyph(p: &egui::Painter, rect: Rect, g: Glyph, color: Color32) {
    let (c, h) = (rect.center(), rect.width() / 2.0);
    for stroke in g.strokes() {
        let pts: Vec<Pos2> = stroke
            .iter()
            .map(|&(x, y)| c + Vec2::new(x, y) * h)
            .collect();
        p.add(egui::Shape::line(pts, Stroke::new(1.4, color)));
    }
}

/// `ui.add(GlyphButton(g))`, for chains that used `small_button`.
pub fn glyph(ui: &mut Ui, g: Glyph) -> Response {
    ui.add(GlyphButton(g))
}

/// A button of the quick access toolbar: the command's icon, no frame until
/// the cursor is over it.
pub fn tool_button(ui: &mut Ui, icon: super::icons::Icon, enabled: bool, name: &str) -> Response {
    use super::icons::{Palette, paint};
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, r) = ui.allocate_exact_size(Vec2::new(26.0, 22.0), sense);
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, name));
    if ui.is_rect_visible(rect) {
        if enabled && r.hovered() {
            let v = ui.style().interact(&r);
            ui.painter()
                .rect_filled(rect, v.corner_radius, v.weak_bg_fill);
        }
        let palette = if enabled {
            &Palette::ENABLED
        } else {
            &Palette::DISABLED
        };
        let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(18.0));
        paint(ui.painter(), icon_rect, icon, palette);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG relative luminance of a colour.
    fn luminance(c: Color32) -> f32 {
        let f = |v: u8| {
            let s = f32::from(v) / 255.0;
            if s <= 0.03928 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn text_stays_readable_on_its_backgrounds() {
        for (fg, bg, min) in [
            (TEXT, PANEL, 7.0),
            (TEXT_WEAK, PANEL, 7.0),
            (TEXT_WEAK, RAISED, 4.5),
            (TEXT, FIELD, 7.0),
            (TEXT, CONTROL, 4.5),
            (Color32::WHITE, ACCENT, 4.5),
            (DANGER, CONTROL, 4.5),
        ] {
            let c = contrast(fg, bg);
            assert!(c >= min, "{fg:?} on {bg:?}: {c:.2} < {min}");
        }
    }

    #[test]
    fn the_sheet_keeps_its_font_when_the_interface_changes_it() {
        let f = fonts();
        let sheet = &f.families[&FontFamily::Name(SHEET_FAMILY.into())];
        assert_eq!(sheet.first().map(String::as_str), Some(SHEET_FONT));
        let ui = &f.families[&FontFamily::Proportional];
        assert_eq!(ui.first().map(String::as_str), Some(UI_FONT));
    }

    #[test]
    fn the_interface_font_has_latin_and_cyrillic() {
        let face = ttf_parser::Face::parse(include_bytes!("../../data/fonts/Inter-Regular.ttf"), 0);
        let Ok(face) = face else {
            panic!("the interface font does not parse");
        };
        for c in "AaЖжЩщЪъЮю0°±Ø".chars() {
            assert!(face.glyph_index(c).is_some(), "no glyph for {c}");
        }
    }
}
