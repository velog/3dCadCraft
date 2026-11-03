//! The colours of the frame around the view, by name, one set per UI Theme
//! (spec 0.18). One token set per theme is adapted from SolidCraft
//! (`crates/ui-egui/src/theme.rs`, `assets/styles/ui/palettes.rs`,
//! MIT OR Apache-2.0); the Light frame starts from its light palette.

use eframe::egui::Color32;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

/// Application Options › Colors › UI Theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiTheme {
    /// Dark frame, icons for a dark background.
    #[default]
    Dark,
    /// Light frame, icons for a light background.
    Light,
}

impl UiTheme {
    /// Every theme, in the order of the Colors tab.
    pub const ALL: [UiTheme; 2] = [UiTheme::Dark, UiTheme::Light];

    /// The name in the Colors tab.
    pub fn label(self) -> &'static str {
        match self {
            UiTheme::Dark => "Dark",
            UiTheme::Light => "Light",
        }
    }

    /// The theme's colours.
    pub const fn tokens(self) -> Tokens {
        match self {
            UiTheme::Dark => Tokens::dark(),
            UiTheme::Light => Tokens::light(),
        }
    }
}

/// The colours of an icon: ink, accent, and the marks for a hot point, a
/// removed part and a good state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IconColors {
    pub ink: Color32,
    pub accent: Color32,
    pub hot: Color32,
    pub warn: Color32,
    pub ok: Color32,
}

/// Every named colour of the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tokens {
    /// Panels, ribbon, browser, status bar.
    pub panel: Color32,
    /// Floating windows and menus: one step above the panels.
    pub raised: Color32,
    /// Inside of text fields.
    pub field: Color32,
    /// A button at rest, under the cursor, pressed.
    pub control: Color32,
    pub control_hover: Color32,
    pub control_active: Color32,
    /// Thin separators and outlines.
    pub line: Color32,
    pub text: Color32,
    /// Labels: quieter than values, still clearly readable.
    pub text_weak: Color32,
    /// Text on a hovered or pressed control.
    pub text_strong: Color32,
    /// The current choice and the main action of a panel; its text.
    pub accent: Color32,
    pub on_accent: Color32,
    /// A selected tab or row: egui draws its text in `text`, so it must read on this.
    pub selection: Color32,
    pub link: Color32,
    /// Text of a good state, a warning, an error; of actions that delete.
    pub ok: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub danger: Color32,
    /// The warning box of a panel.
    pub warn_box_fill: Color32,
    pub warn_box_line: Color32,
    pub warn_box_text: Color32,
    /// Command icons, enabled and disabled.
    pub icon: IconColors,
    pub icon_disabled: IconColors,
    /// The window's shadow strength.
    pub shadow_alpha: u8,
}

impl Tokens {
    /// The dark frame: neutral graphite and one blue accent.
    pub const fn dark() -> Tokens {
        Tokens {
            panel: Color32::from_rgb(0x22, 0x24, 0x29),
            raised: Color32::from_rgb(0x2a, 0x2d, 0x33),
            field: Color32::from_rgb(0x16, 0x18, 0x1b),
            control: Color32::from_rgb(0x36, 0x3a, 0x41),
            control_hover: Color32::from_rgb(0x43, 0x48, 0x51),
            control_active: Color32::from_rgb(0x4f, 0x55, 0x60),
            line: Color32::from_rgb(0x3a, 0x3e, 0x46),
            text: Color32::from_rgb(0xe6, 0xe8, 0xec),
            text_weak: Color32::from_rgb(0xac, 0xb1, 0xbb),
            text_strong: Color32::WHITE,
            accent: Color32::from_rgb(0x2f, 0x74, 0xc8),
            on_accent: Color32::WHITE,
            selection: Color32::from_rgb(0x28, 0x66, 0xb4),
            link: Color32::from_rgb(0x6c, 0xb0, 0xf5),
            ok: Color32::from_rgb(0x5a, 0xc8, 0x6e),
            warning: Color32::from_rgb(0xec, 0xaa, 0x46),
            error: Color32::from_rgb(0xf0, 0x64, 0x60),
            danger: Color32::from_rgb(0xf0, 0x8a, 0x80),
            warn_box_fill: Color32::from_rgb(0x3d, 0x33, 0x1c),
            warn_box_line: Color32::from_rgb(0x8c, 0x6e, 0x2a),
            warn_box_text: Color32::from_rgb(0xf5, 0xdc, 0xa0),
            icon: IconColors {
                ink: Color32::from_rgb(205, 210, 218),
                accent: Color32::from_rgb(80, 160, 255),
                hot: Color32::from_rgb(255, 160, 50),
                warn: Color32::from_rgb(235, 80, 70),
                ok: Color32::from_rgb(90, 200, 110),
            },
            icon_disabled: IconColors {
                ink: Color32::from_rgb(95, 98, 104),
                accent: Color32::from_rgb(70, 90, 115),
                hot: Color32::from_rgb(110, 95, 75),
                warn: Color32::from_rgb(110, 80, 78),
                ok: Color32::from_rgb(80, 105, 85),
            },
            shadow_alpha: 110,
        }
    }

    /// The light frame.
    pub const fn light() -> Tokens {
        Tokens {
            panel: Color32::from_rgb(246, 247, 249),
            raised: Color32::from_rgb(252, 252, 253),
            field: Color32::WHITE,
            control: Color32::from_rgb(228, 231, 236),
            control_hover: Color32::from_rgb(214, 224, 240),
            control_active: Color32::from_rgb(196, 210, 232),
            line: Color32::from_rgb(208, 212, 220),
            text: Color32::from_rgb(38, 42, 50),
            text_weak: Color32::from_rgb(74, 80, 92),
            text_strong: Color32::from_rgb(16, 18, 22),
            accent: Color32::from_rgb(28, 104, 198),
            on_accent: Color32::WHITE,
            selection: Color32::from_rgb(186, 212, 245),
            link: Color32::from_rgb(20, 90, 180),
            ok: Color32::from_rgb(20, 120, 60),
            warning: Color32::from_rgb(150, 90, 0),
            error: Color32::from_rgb(190, 40, 40),
            danger: Color32::from_rgb(180, 40, 36),
            warn_box_fill: Color32::from_rgb(255, 244, 200),
            warn_box_line: Color32::from_rgb(220, 170, 40),
            warn_box_text: Color32::from_rgb(110, 70, 0),
            icon: IconColors {
                ink: Color32::from_rgb(70, 78, 92),
                accent: Color32::from_rgb(28, 110, 215),
                hot: Color32::from_rgb(215, 120, 15),
                warn: Color32::from_rgb(205, 55, 45),
                ok: Color32::from_rgb(35, 140, 70),
            },
            icon_disabled: IconColors {
                ink: Color32::from_rgb(170, 174, 182),
                accent: Color32::from_rgb(165, 185, 212),
                hot: Color32::from_rgb(212, 190, 160),
                warn: Color32::from_rgb(212, 175, 172),
                ok: Color32::from_rgb(165, 195, 172),
            },
            shadow_alpha: 50,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The UI Theme in use.
pub fn current() -> UiTheme {
    if CURRENT.load(Ordering::Relaxed) == 1 {
        UiTheme::Light
    } else {
        UiTheme::Dark
    }
}

/// Chooses the UI Theme; the style follows at the next `theme::sync`.
pub fn set_current(t: UiTheme) {
    CURRENT.store(u8::from(t == UiTheme::Light), Ordering::Relaxed);
}

/// The colours of the UI Theme in use.
pub fn tokens() -> Tokens {
    current().tokens()
}
