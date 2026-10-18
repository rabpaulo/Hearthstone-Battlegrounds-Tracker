//! Colour themes. The panels read every colour from the `Theme` carried in
//! their `Ctx`, so switching one repaints the whole overlay; the settings
//! panel does that live and saves the choice in the config.

use serde::Deserialize;

use super::draw::Rgba;

/// A palette the config and the settings panel can pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeName {
    #[default]
    #[serde(alias = "dark", alias = "Dark", alias = "DARK")]
    Dark,
    #[serde(alias = "light", alias = "Light", alias = "LIGHT")]
    Light,
    #[serde(alias = "high-contrast", alias = "high_contrast", alias = "HighContrast", alias = "highcontrast")]
    HighContrast,
}

impl ThemeName {
    /// Offered by the settings panel, in order.
    pub const ALL: [ThemeName; 3] = [ThemeName::Dark, ThemeName::Light, ThemeName::HighContrast];

    /// Name shown in the settings panel.
    pub fn label(self) -> &'static str {
        match self {
            ThemeName::Dark => "Dark",
            ThemeName::Light => "Light",
            ThemeName::HighContrast => "High contrast",
        }
    }

    /// The value read from and written to `config.toml`.
    pub fn config_name(self) -> &'static str {
        match self {
            ThemeName::Dark => "dark",
            ThemeName::Light => "light",
            ThemeName::HighContrast => "high-contrast",
        }
    }

    pub fn theme(self) -> Theme {
        match self {
            ThemeName::Dark => Theme::DARK,
            ThemeName::Light => Theme::LIGHT,
            ThemeName::HighContrast => Theme::HIGH_CONTRAST,
        }
    }
}

/// Every colour the panels draw with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    /// Translucent panel background (with a compositor).
    pub bg: Rgba,
    /// Opaque panel background and scrolling headers (without one).
    pub bg_opaque: Rgba,
    /// Base of a list row, under the card art that fades in.
    pub row: Rgba,
    /// Background of the search field.
    pub field: Rgba,
    pub border: Rgba,
    /// Band behind section and board headers.
    pub header: Rgba,
    pub button: Rgba,
    pub button_hover: Rgba,
    /// Fill of selected controls (buttons, chips, checkboxes, badges).
    pub accent: Rgba,
    /// Text and icons on `accent` fills and bright badges.
    pub on_accent: Rgba,
    pub text: Rgba,
    /// Secondary text: unselected chips, row details, guide body.
    pub soft: Rgba,
    /// Muted text: captions, hints, subtitles.
    pub dim: Rgba,
    /// Shadow under text drawn over card art (flips with the theme).
    pub shadow: Rgba,
    /// Titles and highlights.
    pub gold: Rgba,
    pub win: Rgba,
    pub tie: Rgba,
    pub loss: Rgba,
    pub win_lethal: Rgba,
    pub loss_lethal: Rgba,
    /// Attack and health gems on a minion.
    pub attack: Rgba,
    pub health: Rgba,
    /// Hover sheen over rows and chips.
    pub tint: Rgba,
    pub scrollbar: Rgba,
}

impl Theme {
    /// The original look, and the default.
    pub const DARK: Theme = Theme {
        bg: Rgba(12, 15, 26, 228),
        bg_opaque: Rgba(12, 15, 26, 255),
        row: Rgba(16, 20, 34, 255),
        field: Rgba(8, 10, 18, 255),
        border: Rgba(74, 96, 150, 150),
        header: Rgba(26, 36, 64, 255),
        button: Rgba(28, 34, 54, 255),
        button_hover: Rgba(45, 58, 96, 255),
        accent: Rgba(64, 98, 180, 255),
        on_accent: Rgba(238, 240, 246, 255),
        text: Rgba(238, 240, 246, 255),
        soft: Rgba(200, 208, 228, 255),
        dim: Rgba(146, 156, 182, 255),
        shadow: Rgba(0, 0, 0, 200),
        gold: Rgba(242, 193, 78, 255),
        win: Rgba(102, 187, 106, 255),
        tie: Rgba(189, 189, 189, 255),
        loss: Rgba(239, 83, 80, 255),
        win_lethal: Rgba(46, 160, 67, 255),
        loss_lethal: Rgba(198, 40, 40, 255),
        attack: Rgba(224, 165, 38, 255),
        health: Rgba(192, 57, 43, 255),
        tint: Rgba(255, 255, 255, 28),
        scrollbar: Rgba(255, 255, 255, 90),
    };

    /// Light panels for a bright room; text over card art gets a pale halo.
    pub const LIGHT: Theme = Theme {
        bg: Rgba(233, 236, 243, 236),
        bg_opaque: Rgba(240, 242, 247, 255),
        row: Rgba(224, 228, 238, 255),
        field: Rgba(255, 255, 255, 255),
        border: Rgba(120, 130, 158, 160),
        header: Rgba(206, 212, 226, 255),
        button: Rgba(213, 218, 230, 255),
        button_hover: Rgba(196, 203, 221, 255),
        accent: Rgba(47, 82, 158, 255),
        on_accent: Rgba(245, 247, 250, 255),
        text: Rgba(30, 34, 46, 255),
        soft: Rgba(68, 76, 96, 255),
        dim: Rgba(104, 112, 132, 255),
        shadow: Rgba(255, 255, 255, 225),
        gold: Rgba(158, 110, 8, 255),
        win: Rgba(46, 125, 50, 255),
        tie: Rgba(96, 100, 110, 255),
        loss: Rgba(183, 28, 28, 255),
        win_lethal: Rgba(27, 94, 32, 255),
        loss_lethal: Rgba(120, 15, 15, 255),
        attack: Rgba(224, 165, 38, 255),
        health: Rgba(192, 57, 43, 255),
        tint: Rgba(0, 0, 0, 24),
        scrollbar: Rgba(0, 0, 0, 70),
    };

    /// Black background and vivid colours; win and loss are blue and orange
    /// to stay apart for colour-vision deficiencies.
    pub const HIGH_CONTRAST: Theme = Theme {
        bg: Rgba(0, 0, 0, 238),
        bg_opaque: Rgba(0, 0, 0, 255),
        row: Rgba(20, 20, 24, 255),
        field: Rgba(0, 0, 0, 255),
        border: Rgba(255, 255, 255, 170),
        header: Rgba(44, 44, 52, 255),
        button: Rgba(28, 28, 32, 255),
        button_hover: Rgba(72, 72, 84, 255),
        accent: Rgba(0, 112, 224, 255),
        on_accent: Rgba(255, 255, 255, 255),
        text: Rgba(255, 255, 255, 255),
        soft: Rgba(225, 228, 235, 255),
        dim: Rgba(178, 182, 192, 255),
        shadow: Rgba(0, 0, 0, 230),
        gold: Rgba(255, 214, 0, 255),
        win: Rgba(64, 196, 255, 255),
        tie: Rgba(200, 200, 205, 255),
        loss: Rgba(255, 150, 40, 255),
        win_lethal: Rgba(0, 132, 255, 255),
        loss_lethal: Rgba(214, 90, 0, 255),
        attack: Rgba(255, 200, 40, 255),
        health: Rgba(255, 90, 70, 255),
        tint: Rgba(255, 255, 255, 40),
        scrollbar: Rgba(255, 255, 255, 140),
    };
}

impl Default for Theme {
    fn default() -> Theme {
        Theme::DARK
    }
}
