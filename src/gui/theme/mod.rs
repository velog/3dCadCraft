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

mod colors_tab;
pub mod scheme;
mod tokens;

pub use colors_tab::{ColorsUi, ImageSlot, Picture, colors_tab, export_file, scheme_editor};
pub use scheme::{canvas, hover_edge, scene_rgb, set_prehighlight, with_alpha};
pub use tokens::{IconColors, Tokens, UiTheme, current, set_current, tokens};

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
            "../../../data/fonts/Inter-Regular.ttf"
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

/// Sets the fonts and both themes' styles; called once at start-up.
pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    for (theme, ui_theme) in [(Theme::Dark, UiTheme::Dark), (Theme::Light, UiTheme::Light)] {
        ctx.style_mut_of(theme, |s| {
            style(s, &ui_theme.tokens(), theme == Theme::Dark)
        });
    }
    sync(ctx);
}

/// Makes egui follow the chosen UI Theme; cheap enough for every frame.
pub fn sync(ctx: &egui::Context) {
    let wanted = match current() {
        UiTheme::Dark => Theme::Dark,
        UiTheme::Light => Theme::Light,
    };
    if ctx.theme() != wanted {
        ctx.set_theme(wanted);
    }
}

fn style(s: &mut egui::Style, k: &Tokens, dark: bool) {
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
    v.dark_mode = dark;
    v.panel_fill = k.panel;
    v.window_fill = k.raised;
    v.faint_bg_color = k.raised;
    v.extreme_bg_color = k.field;
    v.code_bg_color = k.field;
    v.window_stroke = Stroke::new(1.0, k.line);
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(6);
    let shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(k.shadow_alpha),
    };
    v.window_shadow = shadow;
    v.popup_shadow = shadow;
    v.weak_text_color = Some(k.text_weak);
    v.hyperlink_color = k.link;
    v.warn_fg_color = k.warning;
    v.error_fg_color = k.error;
    v.selection.bg_fill = k.selection;
    v.selection.stroke = Stroke::new(1.0, k.text);
    let radius = CornerRadius::same(4);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = k.panel;
    w.noninteractive.weak_bg_fill = k.panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0, k.line);
    w.noninteractive.fg_stroke = Stroke::new(1.0, k.text);
    w.noninteractive.corner_radius = radius;
    for (state, fill, fg) in [
        (&mut w.inactive, k.control, k.text),
        (&mut w.hovered, k.control_hover, k.text_strong),
        (&mut w.active, k.control_active, k.text_strong),
        (&mut w.open, k.control_hover, k.text),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::NONE;
        state.fg_stroke = Stroke::new(1.0, fg);
        state.corner_radius = radius;
        // No growing under the cursor: the neighbours stay quiet.
        state.expansion = 0.0;
    }
    // Check boxes and radio buttons use `bg_fill`, buttons `weak_bg_fill`: an unset
    // box in `control` hardly showed on a window.
    w.inactive.bg_fill = k.control_hover;
}

/// The main action of a panel or dialog (OK).
pub fn ok_button(text: &str) -> egui::Button<'static> {
    let k = tokens();
    egui::Button::new(RichText::new(text.to_owned()).color(k.on_accent)).fill(k.accent)
}

/// An action that deletes something.
pub fn danger_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_owned()).color(tokens().danger))
}

/// Height of [`panel_footer`]'s row.
const FOOTER_HEIGHT: f32 = 30.0;

/// Where a footer of `FOOTER_HEIGHT` stands: after the panel's content, or at the
/// bottom of what is visible while that lies below it.
fn footer_top(natural_top: f32, visible_bottom: f32) -> f32 {
    natural_top.min(visible_bottom - FOOTER_HEIGHT)
}

/// The OK / Cancel row of a command panel: it stays in view while the panel scrolls.
pub fn panel_footer<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    let (slot, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), FOOTER_HEIGHT),
        Sense::hover(),
    );
    let top = footer_top(slot.top(), ui.clip_rect().bottom());
    let at = Pos2::new(slot.left(), top);
    egui::Area::new(ui.id().with("panel_footer"))
        .order(egui::Order::Middle)
        .fixed_pos(at)
        .show(ui.ctx(), |ui| {
            ui.set_width(slot.width());
            egui::Frame::new()
                .fill(tokens().panel)
                .inner_margin(Margin::symmetric(0, 4))
                .show(ui, |ui| {
                    ui.set_min_width(slot.width());
                    ui.horizontal(add).inner
                })
                .inner
        })
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
            Palette::enabled()
        } else {
            Palette::disabled()
        };
        let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(18.0));
        paint(ui.painter(), icon_rect, icon, &palette);
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

    /// Literal colours left in the frame's files (spec 0.18 §6, step 2):
    /// the numbers only go down.
    const FRAME_LITERALS: &[(&str, usize)] = &[
        ("panels.rs", 1),
        ("home.rs", 1),
        ("files.rs", 1),
        ("ribbon.rs", 0),
        ("layout.rs", 0),
        ("command_ui.rs", 5),
        ("valuefield.rs", 2),
        ("doc_tabs.rs", 0),
        ("breadcrumb.rs", 0),
        ("presets.rs", 0),
        ("file_explorer.rs", 1),
        ("keymap.rs", 0),
    ];

    fn count_literals(text: &str) -> usize {
        text.match_indices("Color32::")
            .filter(|(i, _)| {
                let rest = &text[i + "Color32::".len()..];
                rest.starts_with("from_") || rest.starts_with(|c: char| c.is_ascii_uppercase())
            })
            .count()
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn text_stays_readable_on_its_backgrounds() {
        for theme in UiTheme::ALL {
            let k = theme.tokens();
            for (name, fg, bg, min) in [
                ("text on panel", k.text, k.panel, 7.0),
                ("label on panel", k.text_weak, k.panel, 7.0),
                ("label on window", k.text_weak, k.raised, 4.5),
                ("text in field", k.text, k.field, 7.0),
                ("text on button", k.text, k.control, 4.5),
                (
                    "text on hovered button",
                    k.text_strong,
                    k.control_hover,
                    4.5,
                ),
                (
                    "text on pressed button",
                    k.text_strong,
                    k.control_active,
                    4.5,
                ),
                ("OK", k.on_accent, k.accent, 4.5),
                ("selected tab", k.text, k.selection, 4.5),
                ("Delete", k.danger, k.control, 4.5),
                ("link", k.link, k.panel, 4.5),
                ("good state", k.ok, k.panel, 4.5),
                ("warning", k.warning, k.panel, 4.5),
                ("error", k.error, k.panel, 4.5),
                ("warning box", k.warn_box_text, k.warn_box_fill, 4.5),
                ("icon ink", k.icon.ink, k.panel, 3.0),
                ("icon accent", k.icon.accent, k.panel, 3.0),
            ] {
                let c = contrast(fg, bg);
                assert!(c >= min, "{theme:?} {name}: {c:.2} < {min}");
            }
        }
    }

    #[test]
    fn the_footer_follows_the_content_until_it_would_leave_the_view() {
        // A short panel: right after the content.
        assert_eq!(footer_top(300.0, 800.0), 300.0);
        // A long one: on the bottom edge.
        assert_eq!(footer_top(1500.0, 800.0), 800.0 - FOOTER_HEIGHT);
        // Scrolled to its end: back in the flow, once its whole height fits.
        assert_eq!(footer_top(775.0, 800.0), 800.0 - FOOTER_HEIGHT);
        assert_eq!(footer_top(760.0, 800.0), 760.0);
    }

    #[test]
    fn the_background_gradient_is_soft_and_lets_lines_show() {
        let c = canvas();
        assert!(luminance(c.background_top) > luminance(c.background_bottom));
        assert!(contrast(c.background_top, c.background_bottom) < 1.5);
        // Dark edges and sketch lines stand out on both ends.
        let edge = Color32::from_gray(40);
        assert!(contrast(edge, c.background_bottom) >= 4.5);
    }

    #[test]
    fn an_unset_check_box_shows_on_a_window() {
        for theme in UiTheme::ALL {
            let k = theme.tokens();
            let c = contrast(k.control_hover, k.raised);
            assert!(c >= 1.25, "{theme:?}: {c:.2}");
        }
    }

    #[test]
    fn a_disabled_icon_is_quieter_than_an_enabled_one() {
        for theme in UiTheme::ALL {
            let k = theme.tokens();
            assert!(
                contrast(k.icon_disabled.ink, k.panel) < contrast(k.icon.ink, k.panel),
                "{theme:?}"
            );
        }
    }

    #[test]
    fn both_themes_get_their_own_style() {
        let ctx = egui::Context::default();
        apply(&ctx);
        ctx.style_mut_of(Theme::Dark, |s| {
            assert_eq!(s.visuals.panel_fill, Tokens::dark().panel);
            assert!(s.visuals.dark_mode);
        });
        ctx.style_mut_of(Theme::Light, |s| {
            assert_eq!(s.visuals.panel_fill, Tokens::light().panel);
            assert!(!s.visuals.dark_mode);
        });
    }

    #[test]
    fn the_theme_is_saved_by_name_and_old_settings_mean_dark() {
        let text = serde_json::to_string(&UiTheme::Light).unwrap_or_default();
        assert_eq!(text, "\"Light\"");
        assert_eq!(UiTheme::default(), UiTheme::Dark);
        let old: super::super::layout::AppOptions = serde_json::from_str("{}").unwrap_or_default();
        assert_eq!(old.ui_theme, UiTheme::Dark);
    }

    /// Frame files may only lose literal colours: new ones take a token.
    #[test]
    fn frame_files_do_not_gain_literal_colours() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/gui");
        for (file, allowed) in FRAME_LITERALS {
            let text = std::fs::read_to_string(dir.join(file)).unwrap_or_default();
            let n = count_literals(&text);
            assert!(
                n <= *allowed,
                "{file}: {n} literal colours, {allowed} allowed: use theme::tokens() (spec 0.18)"
            );
        }
    }

    #[test]
    fn literal_colours_are_counted() {
        assert_eq!(
            count_literals("Color32::from_rgb(1, 2, 3); Color32::WHITE; x"),
            2
        );
        assert_eq!(count_literals("Color32::from_gray(1) Color32 color"), 1);
        assert_eq!(
            count_literals("egui::Color32::from_rgba_unmultiplied(1, 2, 3, 4)"),
            1
        );
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
        let face =
            ttf_parser::Face::parse(include_bytes!("../../../data/fonts/Inter-Regular.ttf"), 0);
        let Ok(face) = face else {
            panic!("the interface font does not parse");
        };
        for c in "AaЖжЩщЪъЮю0°±Ø".chars() {
            assert!(face.glyph_index(c).is_some(), "no glyph for {c}");
        }
    }
}
